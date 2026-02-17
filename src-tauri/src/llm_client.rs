use crate::settings::PostProcessProvider;
use log::debug;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
    content: Option<String>,
}

/// Build headers for API requests based on provider type
fn build_headers(provider: &PostProcessProvider, api_key: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();

    // Common headers
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://github.com/divnjl2/voice-input"),
    );
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("VoiceInput/1.0 (+https://github.com/divnjl2/voice-input)"),
    );
    headers.insert("X-Title", HeaderValue::from_static("Voice Input"));

    // Provider-specific auth headers
    if !api_key.is_empty() {
        if provider.id == "anthropic" {
            headers.insert(
                "x-api-key",
                HeaderValue::from_str(api_key)
                    .map_err(|e| format!("Invalid API key header value: {}", e))?,
            );
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        } else {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", api_key))
                    .map_err(|e| format!("Invalid authorization header value: {}", e))?,
            );
        }
    }

    Ok(headers)
}

/// Create an HTTP client with provider-specific headers
fn create_client(provider: &PostProcessProvider, api_key: &str) -> Result<reqwest::Client, String> {
    let headers = build_headers(provider, api_key)?;
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}

/// Send a chat completion request to an OpenAI-compatible API
/// Returns Ok(Some(content)) on success, Ok(None) if response has no content,
/// or Err on actual errors (HTTP, parsing, etc.)
pub async fn send_chat_completion(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    prompt: String,
) -> Result<Option<String>, String> {
    let base_url = provider.base_url.trim_end_matches('/');
    let url = format!("{}/chat/completions", base_url);

    debug!("Sending chat completion request to: {}", url);

    let client = create_client(provider, &api_key)?;

    let request_body = ChatCompletionRequest {
        model: model.to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
        }],
    };

    let response = client
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Failed to read error response".to_string());
        return Err(format!(
            "API request failed with status {}: {}",
            status, error_text
        ));
    }

    let completion: ChatCompletionResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse API response: {}", e))?;

    Ok(completion
        .choices
        .first()
        .and_then(|choice| choice.message.content.clone()))
}

/// Fetch available models from an OpenAI-compatible API
/// Returns a list of model IDs
pub async fn fetch_models(
    provider: &PostProcessProvider,
    api_key: String,
) -> Result<Vec<String>, String> {
    let base_url = provider.base_url.trim_end_matches('/');
    let url = format!("{}/models", base_url);

    debug!("Fetching models from: {}", url);

    let client = create_client(provider, &api_key)?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch models: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!(
            "Model list request failed ({}): {}",
            status, error_text
        ));
    }

    let parsed: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let mut models = Vec::new();

    // Handle OpenAI format: { data: [ { id: "..." }, ... ] }
    if let Some(data) = parsed.get("data").and_then(|d| d.as_array()) {
        for entry in data {
            if let Some(id) = entry.get("id").and_then(|i| i.as_str()) {
                models.push(id.to_string());
            } else if let Some(name) = entry.get("name").and_then(|n| n.as_str()) {
                models.push(name.to_string());
            }
        }
    }
    // Handle array format: [ "model1", "model2", ... ]
    else if let Some(array) = parsed.as_array() {
        for entry in array {
            if let Some(model) = entry.as_str() {
                models.push(model.to_string());
            }
        }
    }

    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::PostProcessProvider;

    fn make_provider(id: &str, base_url: &str) -> PostProcessProvider {
        PostProcessProvider {
            id: id.to_string(),
            label: id.to_string(),
            base_url: base_url.to_string(),
            allow_base_url_edit: false,
            models_endpoint: Some("/models".to_string()),
        }
    }

    // ── Header Building ─────────────────────────────────────────────

    #[test]
    fn test_build_headers_openai_with_api_key() {
        let provider = make_provider("openai", "https://api.openai.com/v1");
        let headers = build_headers(&provider, "sk-test123").unwrap();

        assert_eq!(headers.get("content-type").unwrap(), "application/json");
        assert_eq!(headers.get("authorization").unwrap(), "Bearer sk-test123");
        // Should NOT have x-api-key header for OpenAI
        assert!(headers.get("x-api-key").is_none());
    }

    #[test]
    fn test_build_headers_anthropic_uses_x_api_key() {
        let provider = make_provider("anthropic", "https://api.anthropic.com/v1");
        let headers = build_headers(&provider, "sk-ant-test").unwrap();

        assert_eq!(headers.get("x-api-key").unwrap(), "sk-ant-test");
        assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");
        // Should NOT have authorization header for Anthropic
        assert!(headers.get("authorization").is_none());
    }

    #[test]
    fn test_build_headers_no_api_key() {
        let provider = make_provider("custom", "http://localhost:11434/v1");
        let headers = build_headers(&provider, "").unwrap();

        // When empty, no auth headers should be set
        assert!(headers.get("authorization").is_none());
        assert!(headers.get("x-api-key").is_none());
    }

    #[test]
    fn test_build_headers_common_fields() {
        let provider = make_provider("openai", "https://api.openai.com/v1");
        let headers = build_headers(&provider, "key").unwrap();

        assert!(headers.get("referer").is_some());
        assert!(headers.get("user-agent").is_some());
        assert!(headers.get("x-title").is_some());
    }

    // ── URL Construction ────────────────────────────────────────────

    #[test]
    fn test_chat_completion_url_no_trailing_slash() {
        let base_url = "https://api.openai.com/v1";
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn test_chat_completion_url_with_trailing_slash() {
        let base_url = "https://api.openai.com/v1/";
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn test_models_url() {
        let base_url = "https://api.groq.com/openai/v1/";
        let url = format!("{}/models", base_url.trim_end_matches('/'));
        assert_eq!(url, "https://api.groq.com/openai/v1/models");
    }

    // ── Request Body ────────────────────────────────────────────────

    #[test]
    fn test_chat_completion_request_serialization() {
        let request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "Fix this: hello wrold".to_string(),
            }],
        };

        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["model"], "gpt-4");
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Fix this: hello wrold");
    }

    // ── Response Parsing ────────────────────────────────────────────

    #[test]
    fn test_chat_completion_response_parsing() {
        let json = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "Hello world"
                }
            }]
        });

        let response: ChatCompletionResponse = serde_json::from_value(json).unwrap();
        assert_eq!(response.choices.len(), 1);
        assert_eq!(
            response.choices[0].message.content.as_deref(),
            Some("Hello world")
        );
    }

    #[test]
    fn test_chat_completion_response_empty_choices() {
        let json = serde_json::json!({
            "choices": []
        });

        let response: ChatCompletionResponse = serde_json::from_value(json).unwrap();
        assert!(response.choices.is_empty());
    }

    #[test]
    fn test_chat_completion_response_null_content() {
        let json = serde_json::json!({
            "choices": [{
                "message": {
                    "content": null
                }
            }]
        });

        let response: ChatCompletionResponse = serde_json::from_value(json).unwrap();
        assert!(response.choices[0].message.content.is_none());
    }

    // ── Client Creation ─────────────────────────────────────────────

    #[test]
    fn test_create_client_success() {
        let provider = make_provider("openai", "https://api.openai.com/v1");
        let result = create_client(&provider, "test-key");
        assert!(result.is_ok());
    }
}
