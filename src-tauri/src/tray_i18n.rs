//! Tray menu internationalization
//!
//! Everything is auto-generated at compile time by build.rs from the
//! frontend locale files (src/i18n/locales/*/translation.json).
//!
//! The English translation.json is the single source of truth:
//! - TrayStrings struct fields are derived from the English "tray" keys
//! - All languages are auto-discovered from the locales directory
//!
//! To add a new tray menu item:
//! 1. Add the key to en/translation.json under "tray"
//! 2. Add translations to other locale files
//! 3. Update tray.rs to use the new field (e.g., strings.new_field)

use once_cell::sync::Lazy;
use std::collections::HashMap;

// Include the auto-generated TrayStrings struct and TRANSLATIONS static
include!(concat!(env!("OUT_DIR"), "/tray_translations.rs"));

/// Get the language code from a locale string (e.g., "en-US" -> "en")
fn get_language_code(locale: &str) -> &str {
    locale.split(['-', '_']).next().unwrap_or("en")
}

/// Get localized tray menu strings based on the system locale
pub fn get_tray_translations(locale: Option<String>) -> TrayStrings {
    let lang = locale.as_deref().map(get_language_code).unwrap_or("en");

    // Try requested language, fall back to English
    TRANSLATIONS
        .get(lang)
        .or_else(|| TRANSLATIONS.get("en"))
        .cloned()
        .expect("English translations must exist")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_language_code_simple() {
        assert_eq!(get_language_code("en"), "en");
        assert_eq!(get_language_code("ru"), "ru");
        assert_eq!(get_language_code("fr"), "fr");
    }

    #[test]
    fn test_get_language_code_with_region() {
        assert_eq!(get_language_code("en-US"), "en");
        assert_eq!(get_language_code("pt-BR"), "pt");
        assert_eq!(get_language_code("zh-Hans"), "zh");
    }

    #[test]
    fn test_get_language_code_with_underscore() {
        assert_eq!(get_language_code("en_US"), "en");
        assert_eq!(get_language_code("ja_JP"), "ja");
    }

    #[test]
    fn test_get_language_code_empty() {
        assert_eq!(get_language_code(""), "");
    }

    #[test]
    fn test_get_tray_translations_english() {
        let strings = get_tray_translations(Some("en".to_string()));
        // English strings should exist and be non-empty
        assert!(!strings.settings.is_empty());
        assert!(!strings.quit.is_empty());
    }

    #[test]
    fn test_get_tray_translations_russian() {
        let strings = get_tray_translations(Some("ru".to_string()));
        assert!(!strings.settings.is_empty());
        assert!(!strings.quit.is_empty());
    }

    #[test]
    fn test_get_tray_translations_fallback_to_english() {
        // Unknown language should fall back to English
        let strings = get_tray_translations(Some("xx".to_string()));
        let en_strings = get_tray_translations(Some("en".to_string()));
        assert_eq!(strings.settings, en_strings.settings);
    }

    #[test]
    fn test_get_tray_translations_none_locale() {
        // None should default to English
        let strings = get_tray_translations(None);
        let en_strings = get_tray_translations(Some("en".to_string()));
        assert_eq!(strings.quit, en_strings.quit);
    }

    #[test]
    fn test_translations_map_has_english() {
        assert!(TRANSLATIONS.contains_key("en"));
    }
}
