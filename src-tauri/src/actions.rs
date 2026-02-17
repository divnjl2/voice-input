#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::apple_intelligence;
use crate::audio_feedback::{play_feedback_sound, play_feedback_sound_blocking, SoundType};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{get_settings, AppSettings, APPLE_INTELLIGENCE_PROVIDER_ID};
use crate::shortcut;
use crate::tray::{change_tray_icon, TrayIconState};
use crate::utils::{
    self, show_processing_overlay, show_recording_overlay, show_transcribing_overlay,
};
use crate::voice_commands::{self, KeyAction, VoiceAction, VoiceCommandResult};
use crate::TranscriptionCoordinator;
use ferrous_opencc::{config::BuiltinConfig, OpenCC};
use log::{debug, error, info};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::AppHandle;
use tauri::Manager;

/// Drop guard that notifies the [`TranscriptionCoordinator`] when the
/// transcription pipeline finishes — whether it completes normally or panics.
struct FinishGuard(AppHandle);
impl Drop for FinishGuard {
    fn drop(&mut self) {
        if let Some(c) = self.0.try_state::<TranscriptionCoordinator>() {
            c.notify_processing_finished();
        }
    }
}

// Shortcut Action Trait
pub trait ShortcutAction: Send + Sync {
    fn start(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
}

// Transcribe Action
struct TranscribeAction {
    post_process: bool,
    streaming_active: Arc<AtomicBool>,
    streaming_handle: Arc<std::sync::Mutex<Option<std::thread::JoinHandle<()>>>>,
    /// Final text produced by the streaming loop (displayed in overlay only).
    /// `stop()` uses this to decide whether to skip full re-transcription.
    streaming_final_text: Arc<std::sync::Mutex<Option<String>>>,
}

async fn post_process_transcription(settings: &AppSettings, transcription: &str) -> Option<String> {
    let provider = match settings.active_post_process_provider().cloned() {
        Some(provider) => provider,
        None => {
            debug!("Post-processing enabled but no provider is selected");
            return None;
        }
    };

    let model = settings
        .post_process_models
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();

    if model.trim().is_empty() {
        debug!(
            "Post-processing skipped because provider '{}' has no model configured",
            provider.id
        );
        return None;
    }

    let selected_prompt_id = match &settings.post_process_selected_prompt_id {
        Some(id) => id.clone(),
        None => {
            debug!("Post-processing skipped because no prompt is selected");
            return None;
        }
    };

    let prompt = match settings
        .post_process_prompts
        .iter()
        .find(|prompt| prompt.id == selected_prompt_id)
    {
        Some(prompt) => prompt.prompt.clone(),
        None => {
            debug!(
                "Post-processing skipped because prompt '{}' was not found",
                selected_prompt_id
            );
            return None;
        }
    };

    if prompt.trim().is_empty() {
        debug!("Post-processing skipped because the selected prompt is empty");
        return None;
    }

    debug!(
        "Starting LLM post-processing with provider '{}' (model: {})",
        provider.id, model
    );

    // Replace ${output} variable in the prompt with the actual text
    let processed_prompt = prompt.replace("${output}", transcription);
    debug!("Processed prompt length: {} chars", processed_prompt.len());

    if provider.id == APPLE_INTELLIGENCE_PROVIDER_ID {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            if !apple_intelligence::check_apple_intelligence_availability() {
                debug!("Apple Intelligence selected but not currently available on this device");
                return None;
            }

            let token_limit = model.trim().parse::<i32>().unwrap_or(0);
            return match apple_intelligence::process_text(&processed_prompt, token_limit) {
                Ok(result) => {
                    if result.trim().is_empty() {
                        debug!("Apple Intelligence returned an empty response");
                        None
                    } else {
                        debug!(
                            "Apple Intelligence post-processing succeeded. Output length: {} chars",
                            result.len()
                        );
                        Some(result)
                    }
                }
                Err(err) => {
                    error!("Apple Intelligence post-processing failed: {}", err);
                    None
                }
            };
        }

        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            debug!("Apple Intelligence provider selected on unsupported platform");
            return None;
        }
    }

    let api_key = settings
        .post_process_api_keys
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();

    // Send the chat completion request
    match crate::llm_client::send_chat_completion(&provider, api_key, &model, processed_prompt)
        .await
    {
        Ok(Some(content)) => {
            // Strip invisible Unicode characters that some LLMs (e.g., Qwen) may insert
            let content = content
                .replace('\u{200B}', "") // Zero-Width Space
                .replace('\u{200C}', "") // Zero-Width Non-Joiner
                .replace('\u{200D}', "") // Zero-Width Joiner
                .replace('\u{FEFF}', ""); // Byte Order Mark / Zero-Width No-Break Space
            debug!(
                "LLM post-processing succeeded for provider '{}'. Output length: {} chars",
                provider.id,
                content.len()
            );
            Some(content)
        }
        Ok(None) => {
            error!("LLM API response has no content");
            None
        }
        Err(e) => {
            error!(
                "LLM post-processing failed for provider '{}': {}. Falling back to original transcription.",
                provider.id,
                e
            );
            None
        }
    }
}

async fn maybe_convert_chinese_variant(
    settings: &AppSettings,
    transcription: &str,
) -> Option<String> {
    // Check if language is set to Simplified or Traditional Chinese
    let is_simplified = settings.selected_language == "zh-Hans";
    let is_traditional = settings.selected_language == "zh-Hant";

    if !is_simplified && !is_traditional {
        debug!("selected_language is not Simplified or Traditional Chinese; skipping translation");
        return None;
    }

    debug!(
        "Starting Chinese translation using OpenCC for language: {}",
        settings.selected_language
    );

    // Use OpenCC to convert based on selected language
    let config = if is_simplified {
        // Convert Traditional Chinese to Simplified Chinese
        BuiltinConfig::Tw2sp
    } else {
        // Convert Simplified Chinese to Traditional Chinese
        BuiltinConfig::S2twp
    };

    match OpenCC::from_config(config) {
        Ok(converter) => {
            let converted = converter.convert(transcription);
            debug!(
                "OpenCC translation completed. Input length: {}, Output length: {}",
                transcription.len(),
                converted.len()
            );
            Some(converted)
        }
        Err(e) => {
            error!("Failed to initialize OpenCC converter: {}. Falling back to original transcription.", e);
            None
        }
    }
}

/// Execute a voice command by simulating key presses via Enigo.
/// Returns Ok(true) if a command was executed, Ok(false) if it was a TypeText action
/// that should be pasted instead.
fn execute_voice_command(app: &AppHandle, action: &VoiceAction) -> Result<(), String> {
    use crate::input::EnigoState;
    use enigo::{Direction, Key, Keyboard};

    // Release any held modifiers before executing voice commands
    release_all_modifiers(app);

    let enigo_state = app
        .try_state::<EnigoState>()
        .ok_or("Enigo state not initialized")?;
    let mut enigo = enigo_state
        .0
        .lock()
        .map_err(|e| format!("Failed to lock Enigo: {}", e))?;

    fn key_action_to_enigo(ka: &KeyAction) -> Key {
        match ka {
            KeyAction::Enter => Key::Return,
            KeyAction::Backspace => Key::Backspace,
            KeyAction::Delete => Key::Delete,
            KeyAction::Tab => Key::Tab,
            KeyAction::Escape => Key::Escape,
            KeyAction::Space => Key::Space,
            KeyAction::Up => Key::UpArrow,
            KeyAction::Down => Key::DownArrow,
            KeyAction::Left => Key::LeftArrow,
            KeyAction::Right => Key::RightArrow,
            KeyAction::Home => Key::Home,
            KeyAction::End => Key::End,
            KeyAction::PageUp => Key::PageUp,
            KeyAction::PageDown => Key::PageDown,
            KeyAction::Control => Key::Control,
            KeyAction::Shift => Key::Shift,
            KeyAction::Alt => Key::Alt,
            KeyAction::Key(c) => Key::Unicode(*c),
        }
    }

    match action {
        VoiceAction::KeyPress(key) => {
            let k = key_action_to_enigo(key);
            enigo
                .key(k, Direction::Click)
                .map_err(|e| format!("Failed to press key: {}", e))?;
        }
        VoiceAction::KeyCombo(keys) => {
            // Separate modifiers from regular keys
            let mut modifiers = Vec::new();
            let mut regular_keys = Vec::new();
            for key in keys {
                match key {
                    KeyAction::Control | KeyAction::Shift | KeyAction::Alt => {
                        modifiers.push(key_action_to_enigo(key));
                    }
                    _ => {
                        regular_keys.push(key_action_to_enigo(key));
                    }
                }
            }

            // Press modifiers
            for m in &modifiers {
                enigo
                    .key(*m, Direction::Press)
                    .map_err(|e| format!("Failed to press modifier: {}", e))?;
            }

            // Click regular keys
            for k in &regular_keys {
                enigo
                    .key(*k, Direction::Click)
                    .map_err(|e| format!("Failed to click key: {}", e))?;
                std::thread::sleep(std::time::Duration::from_millis(30));
            }

            // Release modifiers in reverse order
            for m in modifiers.iter().rev() {
                enigo
                    .key(*m, Direction::Release)
                    .map_err(|e| format!("Failed to release modifier: {}", e))?;
            }
        }
        VoiceAction::TypeText(text) => {
            enigo
                .text(text)
                .map_err(|e| format!("Failed to type text: {}", e))?;
        }
    }

    Ok(())
}

const WHISPER_SAMPLE_RATE: usize = 16000;

/// Release all modifier keys (Ctrl, Shift, Alt) to prevent them from
/// interfering with Enigo text input. This is critical when the user triggers
/// recording via a hotkey like Ctrl+D — the Ctrl key may still be physically
/// held when the streaming loop starts typing, causing Ctrl+Backspace or
/// Ctrl+letter combos instead of plain text.
fn release_all_modifiers(app: &AppHandle) {
    use crate::input::EnigoState;
    use enigo::{Direction, Key, Keyboard};

    if let Some(enigo_state) = app.try_state::<EnigoState>() {
        if let Ok(mut enigo) = enigo_state.0.lock() {
            let _ = enigo.key(Key::Control, Direction::Release);
            let _ = enigo.key(Key::Shift, Direction::Release);
            let _ = enigo.key(Key::Alt, Direction::Release);
        }
    }
}

/// Check if Chinese variant conversion would be needed (without doing it)
fn maybe_needs_chinese_conversion(settings: &AppSettings) -> bool {
    settings.selected_language == "zh-Hans" || settings.selected_language == "zh-Hant"
}

/// Apply post-processing (Chinese conversion + LLM) to transcription text.
/// Returns (final_text, post_processed_text_for_history, post_process_prompt_for_history).
async fn apply_post_processing(
    settings: &AppSettings,
    transcription: &str,
    post_process: bool,
) -> (String, Option<String>, Option<String>) {
    let mut final_text = transcription.to_string();
    let mut post_processed_text: Option<String> = None;
    let mut post_process_prompt: Option<String> = None;

    // Chinese variant conversion
    if let Some(converted_text) = maybe_convert_chinese_variant(settings, transcription).await {
        final_text = converted_text;
    }

    // LLM post-processing
    let processed = if post_process {
        post_process_transcription(settings, &final_text).await
    } else {
        None
    };
    if let Some(processed_text) = processed {
        post_processed_text = Some(processed_text.clone());
        final_text = processed_text;

        if let Some(prompt_id) = &settings.post_process_selected_prompt_id {
            if let Some(prompt) = settings
                .post_process_prompts
                .iter()
                .find(|p| &p.id == prompt_id)
            {
                post_process_prompt = Some(prompt.prompt.clone());
            }
        }
    } else if final_text != transcription {
        // Chinese conversion was applied but no LLM post-processing
        post_processed_text = Some(final_text.clone());
    }

    (final_text, post_processed_text, post_process_prompt)
}

fn streaming_transcription_loop(
    active: Arc<AtomicBool>,
    final_text_out: Arc<std::sync::Mutex<Option<String>>>,
    app: AppHandle,
) {
    info!("Streaming loop: started, waiting for audio to accumulate");

    // Wait for model to be ready + audio to accumulate (~1.2s)
    // Check flag every 50ms so we can exit quickly if recording stops
    for i in 0..24 {
        if !active.load(Ordering::SeqCst) {
            info!(
                "Streaming loop: cancelled during initial delay (after {}ms)",
                i * 50
            );
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    info!("Streaming loop: entering main loop");

    // ── Chunked streaming state ──
    // Audio is processed in bounded chunks to keep transcription fast for long recordings.
    // When a chunk exceeds MAX_CHUNK_SAMPLES and the partial text stabilizes,
    // we "finalize" it: save the text and advance the offset so only new audio
    // is transcribed on subsequent iterations.
    const MAX_CHUNK_SECS: usize = 15;
    const MAX_CHUNK_SAMPLES: usize = WHISPER_SAMPLE_RATE * MAX_CHUNK_SECS;
    const FORCE_FINALIZE_SECS: usize = 20;
    const FORCE_FINALIZE_SAMPLES: usize = WHISPER_SAMPLE_RATE * FORCE_FINALIZE_SECS;
    const STABILIZE_ITERS: usize = 2;

    let mut finalized_text = String::new();
    let mut finalized_offset: usize = 0;
    let mut prev_partial = String::new();
    let mut stable_count: usize = 0;
    let mut prev_displayed = String::new();

    while active.load(Ordering::SeqCst) {
        let rm = app.state::<Arc<AudioRecordingManager>>();
        let chunk = rm.peek_samples_from(finalized_offset);

        if let Some(chunk) = chunk {
            let chunk_len = chunk.len();

            // Only transcribe if we have at least 0.5s of new audio
            if chunk_len > WHISPER_SAMPLE_RATE / 2 {
                debug!(
                    "Streaming loop: chunk {:.1}s (offset {}, +{} samples), transcribing...",
                    chunk_len as f64 / WHISPER_SAMPLE_RATE as f64,
                    finalized_offset,
                    chunk_len,
                );

                let tm = app.state::<Arc<TranscriptionManager>>();
                match tm.transcribe_partial(chunk) {
                    Ok(partial) => {
                        // Build full display text: finalized + current partial
                        let full_text = match (finalized_text.is_empty(), partial.is_empty()) {
                            (true, _) => partial.clone(),
                            (_, true) => finalized_text.clone(),
                            _ => format!("{} {}", finalized_text, partial),
                        };

                        // Track text stability for chunk finalization
                        if !partial.is_empty() && partial == prev_partial {
                            stable_count += 1;
                        } else {
                            stable_count = 0;
                        }

                        // Finalize chunk when:
                        // 1. Audio exceeds window and text is stable, OR
                        // 2. Audio exceeds force limit (prevents unbounded growth)
                        let should_finalize = (chunk_len >= MAX_CHUNK_SAMPLES
                            && stable_count >= STABILIZE_ITERS)
                            || chunk_len >= FORCE_FINALIZE_SAMPLES;
                        if should_finalize {
                            info!(
                                "Streaming loop: finalizing chunk at offset {} ({:.1}s total), text so far: '{}'",
                                finalized_offset + chunk_len,
                                (finalized_offset + chunk_len) as f64 / WHISPER_SAMPLE_RATE as f64,
                                full_text,
                            );
                            finalized_text = full_text.clone();
                            finalized_offset += chunk_len;
                            prev_partial.clear();
                            stable_count = 0;
                        } else {
                            prev_partial = partial;
                        }

                        // Show streaming text in overlay (not typed into active window)
                        if full_text != prev_displayed {
                            debug!("Streaming loop: overlay display '{}'", full_text);
                            crate::overlay::emit_streaming_text(&app, &full_text);
                            prev_displayed = full_text;
                        } else {
                            debug!("Streaming loop: text unchanged, skipping update");
                        }
                    }
                    Err(e) => {
                        info!("Streaming loop: transcription error: {}", e);
                    }
                }
            } else {
                debug!(
                    "Streaming loop: chunk only {:.1}s, too short",
                    chunk_len as f64 / WHISPER_SAMPLE_RATE as f64
                );
            }
        } else {
            debug!("Streaming loop: peek returned None");
        }

        // Wait before next peek (~500ms in 50ms increments for fast exit)
        for _ in 0..10 {
            if !active.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    // Store final streamed text so stop() can use it for the final paste
    if !prev_displayed.is_empty() {
        info!(
            "Streaming loop: final streamed text: '{}' ({} chars)",
            prev_displayed,
            prev_displayed.chars().count()
        );
        *final_text_out.lock().unwrap() = Some(prev_displayed);
    }
    info!(
        "Streaming loop: exited (finalized {} chunks, offset {})",
        if finalized_offset > 0 {
            finalized_offset / (MAX_CHUNK_SAMPLES.max(1)) + 1
        } else {
            0
        },
        finalized_offset,
    );
}

impl ShortcutAction for TranscribeAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        let start_time = Instant::now();
        debug!("TranscribeAction::start called for binding: {}", binding_id);

        // Load model in the background
        let tm = app.state::<Arc<TranscriptionManager>>();
        tm.initiate_model_load();

        let binding_id = binding_id.to_string();
        change_tray_icon(app, TrayIconState::Recording);
        show_recording_overlay(app);

        let rm = app.state::<Arc<AudioRecordingManager>>();

        // Get the microphone mode to determine audio feedback timing
        let settings = get_settings(app);
        let is_always_on = settings.always_on_microphone;
        debug!("Microphone mode - always_on: {}", is_always_on);

        let mut recording_started = false;
        if is_always_on {
            // Always-on mode: Play audio feedback immediately, then apply mute after sound finishes
            debug!("Always-on mode: Playing audio feedback immediately");
            let rm_clone = Arc::clone(&rm);
            let app_clone = app.clone();
            // The blocking helper exits immediately if audio feedback is disabled,
            // so we can always reuse this thread to ensure mute happens right after playback.
            std::thread::spawn(move || {
                play_feedback_sound_blocking(&app_clone, SoundType::Start);
                rm_clone.apply_mute();
            });

            recording_started = rm.try_start_recording(&binding_id);
            debug!("Recording started: {}", recording_started);
        } else {
            // On-demand mode: Start recording first, then play audio feedback, then apply mute
            // This allows the microphone to be activated before playing the sound
            debug!("On-demand mode: Starting recording first, then audio feedback");
            let recording_start_time = Instant::now();
            if rm.try_start_recording(&binding_id) {
                recording_started = true;
                debug!("Recording started in {:?}", recording_start_time.elapsed());
                // Small delay to ensure microphone stream is active
                let app_clone = app.clone();
                let rm_clone = Arc::clone(&rm);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    debug!("Handling delayed audio feedback/mute sequence");
                    // Helper handles disabled audio feedback by returning early, so we reuse it
                    // to keep mute sequencing consistent in every mode.
                    play_feedback_sound_blocking(&app_clone, SoundType::Start);
                    rm_clone.apply_mute();
                });
            } else {
                debug!("Failed to start recording");
            }
        }

        if recording_started {
            // Dynamically register the cancel shortcut in a separate task to avoid deadlock
            shortcut::register_cancel_shortcut(app);

            // Start streaming transcription loop
            self.streaming_active.store(true, Ordering::SeqCst);
            *self.streaming_final_text.lock().unwrap() = None;
            let streaming_flag = self.streaming_active.clone();
            let final_text_out = self.streaming_final_text.clone();
            let app_clone = app.clone();
            let handle = std::thread::spawn(move || {
                streaming_transcription_loop(streaming_flag, final_text_out, app_clone);
            });
            *self.streaming_handle.lock().unwrap() = Some(handle);
        }

        debug!(
            "TranscribeAction::start completed in {:?}",
            start_time.elapsed()
        );
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        // Signal the streaming loop to stop (non-blocking)
        self.streaming_active.store(false, Ordering::SeqCst);
        // Take the join handle so the async task can wait for it
        let streaming_join = self.streaming_handle.lock().unwrap().take();

        // Unregister the cancel shortcut when transcription stops
        shortcut::unregister_cancel_shortcut(app);

        let stop_time = Instant::now();
        debug!("TranscribeAction::stop called for binding: {}", binding_id);

        let ah = app.clone();
        let rm = Arc::clone(&app.state::<Arc<AudioRecordingManager>>());
        let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());
        let hm = Arc::clone(&app.state::<Arc<HistoryManager>>());

        change_tray_icon(app, TrayIconState::Transcribing);
        show_transcribing_overlay(app);

        // Unmute before playing audio feedback so the stop sound is audible
        rm.remove_mute();

        // Play audio feedback for recording stop
        play_feedback_sound(app, SoundType::Stop);

        let binding_id = binding_id.to_string(); // Clone binding_id for the async task
        let post_process = self.post_process;
        let streaming_final_text = self.streaming_final_text.clone();

        tauri::async_runtime::spawn(async move {
            let _guard = FinishGuard(ah.clone());
            let binding_id = binding_id.clone(); // Clone for the inner async task
            debug!(
                "Starting async transcription task for binding: {}",
                binding_id
            );

            // Wait for streaming loop to finish
            if let Some(handle) = streaming_join {
                info!("Waiting for streaming loop to finish...");
                let _ = handle.join();
                info!("Streaming loop finished");
            }

            // Grab the text the streaming loop produced (shown in overlay, not typed)
            let streamed_text = streaming_final_text.lock().unwrap().take();

            let stop_recording_time = Instant::now();
            if let Some(samples) = rm.stop_recording(&binding_id) {
                debug!(
                    "Recording stopped and samples retrieved in {:?}, sample count: {}",
                    stop_recording_time.elapsed(),
                    samples.len()
                );

                let settings = get_settings(&ah);

                // Decide whether we need a full re-transcription.
                // If streaming already produced text and no post-processing is needed,
                // we can skip the expensive full transcription and use the streamed result.
                let needs_post_processing =
                    post_process || maybe_needs_chinese_conversion(&settings);

                let (transcription, final_text, post_processed_text, post_process_prompt) =
                    if let Some(ref streamed) = streamed_text {
                        if !needs_post_processing {
                            // Fast path: streaming text is already on screen, no post-processing needed.
                            // Skip full re-transcription entirely.
                            info!(
                                "Using streamed text directly (no post-processing): '{}'",
                                streamed
                            );
                            // Unload the model since we won't call transcribe()
                            tm.maybe_unload_immediately("streaming-only transcription");
                            (streamed.clone(), streamed.clone(), None, None)
                        } else {
                            // Post-processing needed: do full transcription for best quality,
                            // then replace the streamed text with the post-processed result.
                            info!("Post-processing requested, running full transcription");
                            if post_process {
                                show_processing_overlay(&ah);
                            }
                            let transcription_time = Instant::now();
                            match tm.transcribe(samples.clone()) {
                                Ok(transcription) => {
                                    debug!(
                                        "Transcription completed in {:?}: '{}'",
                                        transcription_time.elapsed(),
                                        transcription
                                    );
                                    let (ft, ppt, ppp) = apply_post_processing(
                                        &settings,
                                        &transcription,
                                        post_process,
                                    )
                                    .await;
                                    (transcription, ft, ppt, ppp)
                                }
                                Err(err) => {
                                    error!(
                                        "Full transcription failed, using streamed text: {}",
                                        err
                                    );
                                    (streamed.clone(), streamed.clone(), None, None)
                                }
                            }
                        }
                    } else {
                        // No streaming text — do full transcription as usual
                        let transcription_time = Instant::now();
                        match tm.transcribe(samples.clone()) {
                            Ok(transcription) => {
                                debug!(
                                    "Transcription completed in {:?}: '{}'",
                                    transcription_time.elapsed(),
                                    transcription
                                );
                                if transcription.is_empty() {
                                    utils::hide_recording_overlay(&ah);
                                    change_tray_icon(&ah, TrayIconState::Idle);
                                    return;
                                }
                                if post_process {
                                    show_processing_overlay(&ah);
                                }
                                let (ft, ppt, ppp) =
                                    apply_post_processing(&settings, &transcription, post_process)
                                        .await;
                                (transcription, ft, ppt, ppp)
                            }
                            Err(err) => {
                                debug!("Global Shortcut Transcription error: {}", err);
                                utils::hide_recording_overlay(&ah);
                                change_tray_icon(&ah, TrayIconState::Idle);
                                return;
                            }
                        }
                    };

                if final_text.is_empty() {
                    utils::hide_recording_overlay(&ah);
                    change_tray_icon(&ah, TrayIconState::Idle);
                    return;
                }

                // Save to history
                let hm_clone = Arc::clone(&hm);
                let transcription_for_history = transcription.clone();
                let pp_text = post_processed_text.clone();
                let pp_prompt = post_process_prompt.clone();
                let samples_clone = samples;
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = hm_clone
                        .save_transcription(
                            samples_clone,
                            transcription_for_history,
                            pp_text,
                            pp_prompt,
                        )
                        .await
                    {
                        error!("Failed to save transcription to history: {}", e);
                    }
                });

                // Streaming text was shown in overlay only (not typed into active window).
                // Always do a single paste via clipboard at the end.
                let settings_for_vc = get_settings(&ah);
                let voice_commands_enabled = settings_for_vc.voice_commands_enabled;

                let ah_clone = ah.clone();
                let paste_time = Instant::now();

                // Clone final_text for overlay-done emission after paste
                let done_text = final_text.clone();

                if voice_commands_enabled {
                    match voice_commands::check_voice_command(&final_text) {
                        VoiceCommandResult::Command(cmd) => {
                            info!(
                                "Executing voice command: {} (from '{}')",
                                cmd.description, final_text
                            );
                            let action = cmd.action.clone();
                            ah.run_on_main_thread(move || {
                                match execute_voice_command(&ah_clone, &action) {
                                    Ok(()) => debug!(
                                        "Voice command executed in {:?}",
                                        paste_time.elapsed()
                                    ),
                                    Err(e) => error!("Failed to execute voice command: {}", e),
                                }
                                // Voice commands: hide overlay (no text to show)
                                utils::hide_recording_overlay(&ah_clone);
                                change_tray_icon(&ah_clone, TrayIconState::Idle);
                            })
                            .unwrap_or_else(|e| {
                                error!("Failed to run voice command on main thread: {:?}", e);
                                utils::hide_recording_overlay(&ah);
                                change_tray_icon(&ah, TrayIconState::Idle);
                            });
                        }
                        VoiceCommandResult::Text(text) => {
                            let dt = done_text.clone();
                            ah.run_on_main_thread(move || {
                                match utils::paste(text, ah_clone.clone()) {
                                    Ok(()) => debug!(
                                        "Text pasted successfully in {:?}",
                                        paste_time.elapsed()
                                    ),
                                    Err(e) => error!("Failed to paste transcription: {}", e),
                                }
                                // Transition overlay to "done" state with copy/close buttons
                                crate::overlay::emit_overlay_done(&ah_clone, &dt);
                                change_tray_icon(&ah_clone, TrayIconState::Idle);
                            })
                            .unwrap_or_else(|e| {
                                error!("Failed to run paste on main thread: {:?}", e);
                                utils::hide_recording_overlay(&ah);
                                change_tray_icon(&ah, TrayIconState::Idle);
                            });
                        }
                    }
                } else {
                    // Voice commands disabled — single paste
                    ah.run_on_main_thread(move || {
                        match utils::paste(final_text, ah_clone.clone()) {
                            Ok(()) => {
                                debug!("Text pasted successfully in {:?}", paste_time.elapsed())
                            }
                            Err(e) => error!("Failed to paste transcription: {}", e),
                        }
                        // Transition overlay to "done" state with copy/close buttons
                        crate::overlay::emit_overlay_done(&ah_clone, &done_text);
                        change_tray_icon(&ah_clone, TrayIconState::Idle);
                    })
                    .unwrap_or_else(|e| {
                        error!("Failed to run paste on main thread: {:?}", e);
                        utils::hide_recording_overlay(&ah);
                        change_tray_icon(&ah, TrayIconState::Idle);
                    });
                }
            } else {
                debug!("No samples retrieved from recording stop");
                utils::hide_recording_overlay(&ah);
                change_tray_icon(&ah, TrayIconState::Idle);
            }
        });

        debug!(
            "TranscribeAction::stop completed in {:?}",
            stop_time.elapsed()
        );
    }
}

// Toggle Settings Window Action
struct ToggleSettingsAction;

impl ShortcutAction for ToggleSettingsAction {
    fn start(&self, app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        if let Some(main_window) = app.get_webview_window("main") {
            if main_window.is_visible().unwrap_or(false) {
                let _ = main_window.hide();
                #[cfg(target_os = "macos")]
                {
                    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            } else {
                let _ = main_window.show();
                let _ = main_window.set_focus();
                #[cfg(target_os = "macos")]
                {
                    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
                }
            }
        }
    }

    fn stop(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        // Nothing to do on stop for toggle settings
    }
}

// Cancel Action
struct CancelAction;

impl ShortcutAction for CancelAction {
    fn start(&self, app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        utils::cancel_current_operation(app);
    }

    fn stop(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        // Nothing to do on stop for cancel
    }
}

// Test Action
struct TestAction;

impl ShortcutAction for TestAction {
    fn start(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str) {
        log::info!(
            "Shortcut ID '{}': Started - {} (App: {})", // Changed "Pressed" to "Started" for consistency
            binding_id,
            shortcut_str,
            app.package_info().name
        );
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str) {
        log::info!(
            "Shortcut ID '{}': Stopped - {} (App: {})", // Changed "Released" to "Stopped" for consistency
            binding_id,
            shortcut_str,
            app.package_info().name
        );
    }
}

// Static Action Map
pub static ACTION_MAP: Lazy<HashMap<String, Arc<dyn ShortcutAction>>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert(
        "transcribe".to_string(),
        Arc::new(TranscribeAction {
            post_process: false,
            streaming_active: Arc::new(AtomicBool::new(false)),
            streaming_handle: Arc::new(std::sync::Mutex::new(None)),
            streaming_final_text: Arc::new(std::sync::Mutex::new(None)),
        }) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "transcribe_with_post_process".to_string(),
        Arc::new(TranscribeAction {
            post_process: true,
            streaming_active: Arc::new(AtomicBool::new(false)),
            streaming_handle: Arc::new(std::sync::Mutex::new(None)),
            streaming_final_text: Arc::new(std::sync::Mutex::new(None)),
        }) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "toggle_settings".to_string(),
        Arc::new(ToggleSettingsAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "cancel".to_string(),
        Arc::new(CancelAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "test".to_string(),
        Arc::new(TestAction) as Arc<dyn ShortcutAction>,
    );
    map
});
