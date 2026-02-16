//! Voice Commands Module
//!
//! Recognizes spoken commands in transcribed text and maps them to keyboard actions.
//! Supports both English and Russian commands with fuzzy matching.
//!
//! Instead of pasting text, voice commands execute keyboard actions like
//! pressing Enter, deleting text, selecting all, etc.

use log::debug;
use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Represents a keyboard action to execute
#[derive(Debug, Clone, PartialEq)]
pub enum VoiceAction {
    /// Press a single key
    KeyPress(KeyAction),
    /// Press a key combination (modifier + key)
    KeyCombo(Vec<KeyAction>),
    /// Type literal text (e.g., for punctuation insertion)
    TypeText(String),
}

/// Individual key actions
#[derive(Debug, Clone, PartialEq)]
pub enum KeyAction {
    Enter,
    Backspace,
    Delete,
    Tab,
    Escape,
    Space,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    // Modifiers
    Control,
    Shift,
    Alt,
    // Letters/symbols (for combos)
    Key(char),
}

/// A recognized voice command with its action
#[derive(Debug, Clone)]
pub struct VoiceCommand {
    /// The action to execute
    pub action: VoiceAction,
    /// Human-readable description for logging
    pub description: &'static str,
}

/// Result of checking text for voice commands
#[derive(Debug)]
pub enum VoiceCommandResult {
    /// Text is a voice command — execute this action
    Command(VoiceCommand),
    /// Text is not a command — paste it as usual
    Text(String),
}

/// Normalize text for command matching: lowercase, trim, remove extra spaces
fn normalize(text: &str) -> String {
    let trimmed = text.trim().to_lowercase();
    // Collapse multiple spaces
    let mut result = String::with_capacity(trimmed.len());
    let mut prev_space = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                result.push(' ');
                prev_space = true;
            }
        } else {
            result.push(ch);
            prev_space = false;
        }
    }
    result
}

/// Static command map: normalized phrase -> VoiceCommand
static COMMAND_MAP: Lazy<HashMap<String, VoiceCommand>> = Lazy::new(|| {
    let mut map = HashMap::new();

    // Helper to insert multiple aliases for the same command
    let mut add = |phrases: &[&str], action: VoiceAction, description: &'static str| {
        for phrase in phrases {
            map.insert(
                normalize(phrase),
                VoiceCommand {
                    action: action.clone(),
                    description,
                },
            );
        }
    };

    // ── Enter / New Line ──────────────────────────────────────────────
    add(
        &[
            "press enter",
            "enter",
            "new line",
            "newline",
            "нажми ввод",
            "ввод",
            "энтер",
            "новая строка",
            "перенос строки",
        ],
        VoiceAction::KeyPress(KeyAction::Enter),
        "Press Enter",
    );

    // ── Backspace ─────────────────────────────────────────────────────
    add(
        &[
            "backspace",
            "delete back",
            "удали символ",
            "бэкспейс",
            "назад удалить",
        ],
        VoiceAction::KeyPress(KeyAction::Backspace),
        "Backspace",
    );

    // ── Delete ────────────────────────────────────────────────────────
    add(
        &[
            "delete",
            "удали",
            "удалить",
        ],
        VoiceAction::KeyPress(KeyAction::Delete),
        "Delete",
    );

    // ── Tab ───────────────────────────────────────────────────────────
    add(
        &[
            "tab",
            "press tab",
            "таб",
            "табуляция",
            "нажми таб",
        ],
        VoiceAction::KeyPress(KeyAction::Tab),
        "Tab",
    );

    // ── Escape ────────────────────────────────────────────────────────
    add(
        &[
            "escape",
            "cancel",
            "эскейп",
            "отмена",
        ],
        VoiceAction::KeyPress(KeyAction::Escape),
        "Escape",
    );

    // ── Space ─────────────────────────────────────────────────────────
    add(
        &[
            "space",
            "press space",
            "пробел",
            "нажми пробел",
        ],
        VoiceAction::KeyPress(KeyAction::Space),
        "Space",
    );

    // ── Select All (Ctrl+A) ──────────────────────────────────────────
    add(
        &[
            "select all",
            "выдели все",
            "выделить все",
            "выдели всё",
            "выделить всё",
        ],
        VoiceAction::KeyCombo(vec![KeyAction::Control, KeyAction::Key('a')]),
        "Select All (Ctrl+A)",
    );

    // ── Delete All (Ctrl+A, Delete) ──────────────────────────────────
    add(
        &[
            "delete all",
            "delete everything",
            "clear all",
            "erase all",
            "erase everything",
            "сотри все",
            "сотри всё",
            "удали все",
            "удали всё",
            "очисти все",
            "очисти всё",
            "очистить все",
            "очистить всё",
            "стереть все",
            "стереть всё",
        ],
        VoiceAction::KeyCombo(vec![
            KeyAction::Control,
            KeyAction::Key('a'),
            // After select all, press Delete
            KeyAction::Delete,
        ]),
        "Delete All (Ctrl+A, Delete)",
    );

    // ── Undo (Ctrl+Z) ───────────────────────────────────────────────
    add(
        &[
            "undo",
            "отмени",
            "отменить",
            "назад",
            "ctrl z",
        ],
        VoiceAction::KeyCombo(vec![KeyAction::Control, KeyAction::Key('z')]),
        "Undo (Ctrl+Z)",
    );

    // ── Redo (Ctrl+Y / Ctrl+Shift+Z) ────────────────────────────────
    add(
        &[
            "redo",
            "повтори",
            "повторить",
            "вперед",
            "вперёд",
        ],
        VoiceAction::KeyCombo(vec![KeyAction::Control, KeyAction::Key('y')]),
        "Redo (Ctrl+Y)",
    );

    // ── Copy (Ctrl+C) ───────────────────────────────────────────────
    add(
        &[
            "copy",
            "copy that",
            "копируй",
            "копировать",
            "скопируй",
            "скопировать",
        ],
        VoiceAction::KeyCombo(vec![KeyAction::Control, KeyAction::Key('c')]),
        "Copy (Ctrl+C)",
    );

    // ── Cut (Ctrl+X) ────────────────────────────────────────────────
    add(
        &[
            "cut",
            "cut that",
            "вырежи",
            "вырезать",
        ],
        VoiceAction::KeyCombo(vec![KeyAction::Control, KeyAction::Key('x')]),
        "Cut (Ctrl+X)",
    );

    // ── Paste (Ctrl+V) ──────────────────────────────────────────────
    add(
        &[
            "paste",
            "paste that",
            "вставь",
            "вставить",
        ],
        VoiceAction::KeyCombo(vec![KeyAction::Control, KeyAction::Key('v')]),
        "Paste (Ctrl+V)",
    );

    // ── Save (Ctrl+S) ───────────────────────────────────────────────
    add(
        &[
            "save",
            "save file",
            "сохрани",
            "сохранить",
        ],
        VoiceAction::KeyCombo(vec![KeyAction::Control, KeyAction::Key('s')]),
        "Save (Ctrl+S)",
    );

    // ── Delete Word (Ctrl+Backspace) ─────────────────────────────────
    add(
        &[
            "delete word",
            "удали слово",
            "удалить слово",
            "сотри слово",
            "стереть слово",
        ],
        VoiceAction::KeyCombo(vec![KeyAction::Control, KeyAction::Backspace]),
        "Delete Word (Ctrl+Backspace)",
    );

    // ── Arrow Keys ───────────────────────────────────────────────────
    add(
        &["up", "arrow up", "вверх", "стрелка вверх"],
        VoiceAction::KeyPress(KeyAction::Up),
        "Arrow Up",
    );
    add(
        &["down", "arrow down", "вниз", "стрелка вниз"],
        VoiceAction::KeyPress(KeyAction::Down),
        "Arrow Down",
    );
    add(
        &["left", "arrow left", "влево", "стрелка влево"],
        VoiceAction::KeyPress(KeyAction::Left),
        "Arrow Left",
    );
    add(
        &["right", "arrow right", "вправо", "стрелка вправо"],
        VoiceAction::KeyPress(KeyAction::Right),
        "Arrow Right",
    );

    // ── Home / End ───────────────────────────────────────────────────
    add(
        &["home", "go home", "в начало", "начало строки"],
        VoiceAction::KeyPress(KeyAction::Home),
        "Home",
    );
    add(
        &["end", "go end", "в конец", "конец строки"],
        VoiceAction::KeyPress(KeyAction::End),
        "End",
    );

    // ── Punctuation ──────────────────────────────────────────────────
    add(
        &["period", "dot", "точка"],
        VoiceAction::TypeText(".".to_string()),
        "Period (.)",
    );
    add(
        &["comma", "запятая"],
        VoiceAction::TypeText(",".to_string()),
        "Comma (,)",
    );
    add(
        &[
            "question mark",
            "вопросительный знак",
            "знак вопроса",
        ],
        VoiceAction::TypeText("?".to_string()),
        "Question Mark (?)",
    );
    add(
        &[
            "exclamation mark",
            "exclamation point",
            "восклицательный знак",
        ],
        VoiceAction::TypeText("!".to_string()),
        "Exclamation Mark (!)",
    );
    add(
        &["colon", "двоеточие"],
        VoiceAction::TypeText(":".to_string()),
        "Colon (:)",
    );
    add(
        &["semicolon", "точка с запятой"],
        VoiceAction::TypeText(";".to_string()),
        "Semicolon (;)",
    );

    map
});

/// Check if the transcribed text matches a voice command.
///
/// Returns `VoiceCommandResult::Command` if matched, or `VoiceCommandResult::Text`
/// with the original text if not.
///
/// The matching is exact (after normalization): the entire transcribed text
/// must match a known command phrase. This prevents false positives when
/// the user is dictating normal text that happens to contain command words.
pub fn check_voice_command(text: &str) -> VoiceCommandResult {
    let normalized = normalize(text);

    // Strip trailing period/comma that Whisper sometimes adds
    let stripped = normalized
        .trim_end_matches('.')
        .trim_end_matches(',')
        .trim();

    if let Some(cmd) = COMMAND_MAP.get(stripped) {
        debug!(
            "Voice command recognized: '{}' -> {}",
            text, cmd.description
        );
        return VoiceCommandResult::Command(cmd.clone());
    }

    // Also try the un-stripped version (in case stripping removed meaningful punctuation)
    if stripped != normalized {
        if let Some(cmd) = COMMAND_MAP.get(normalized.as_str()) {
            debug!(
                "Voice command recognized (with punctuation): '{}' -> {}",
                text, cmd.description
            );
            return VoiceCommandResult::Command(cmd.clone());
        }
    }

    VoiceCommandResult::Text(text.to_string())
}

/// Get a list of all available voice commands with descriptions.
/// Useful for UI display / help.
pub fn list_commands() -> Vec<(String, &'static str)> {
    let mut commands: Vec<(String, &'static str)> = COMMAND_MAP
        .iter()
        .map(|(phrase, cmd)| (phrase.clone(), cmd.description))
        .collect();
    commands.sort_by(|a, b| a.0.cmp(&b.0));
    commands.dedup_by(|a, b| a.1 == b.1);
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic Command Recognition ────────────────────────────────────

    #[test]
    fn test_enter_command_english() {
        match check_voice_command("press enter") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::KeyPress(KeyAction::Enter));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command, got text"),
        }
    }

    #[test]
    fn test_enter_command_russian() {
        match check_voice_command("нажми ввод") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::KeyPress(KeyAction::Enter));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command, got text"),
        }
    }

    #[test]
    fn test_new_line() {
        match check_voice_command("new line") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::KeyPress(KeyAction::Enter));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command, got text"),
        }
    }

    #[test]
    fn test_delete_all_english() {
        match check_voice_command("delete all") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(
                    cmd.action,
                    VoiceAction::KeyCombo(vec![
                        KeyAction::Control,
                        KeyAction::Key('a'),
                        KeyAction::Delete,
                    ])
                );
            }
            VoiceCommandResult::Text(_) => panic!("Expected command, got text"),
        }
    }

    #[test]
    fn test_delete_all_russian() {
        match check_voice_command("сотри все") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.description, "Delete All (Ctrl+A, Delete)");
            }
            VoiceCommandResult::Text(_) => panic!("Expected command, got text"),
        }
    }

    #[test]
    fn test_delete_all_russian_yo() {
        // Test with ё variant
        match check_voice_command("сотри всё") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.description, "Delete All (Ctrl+A, Delete)");
            }
            VoiceCommandResult::Text(_) => panic!("Expected command, got text"),
        }
    }

    // ── Case Insensitivity ──────────────────────────────────────────

    #[test]
    fn test_case_insensitive() {
        match check_voice_command("Press Enter") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::KeyPress(KeyAction::Enter));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command, got text"),
        }
    }

    #[test]
    fn test_all_caps() {
        match check_voice_command("SELECT ALL") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(
                    cmd.action,
                    VoiceAction::KeyCombo(vec![KeyAction::Control, KeyAction::Key('a')])
                );
            }
            VoiceCommandResult::Text(_) => panic!("Expected command, got text"),
        }
    }

    // ── Whitespace Handling ─────────────────────────────────────────

    #[test]
    fn test_extra_whitespace() {
        match check_voice_command("  press   enter  ") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::KeyPress(KeyAction::Enter));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command, got text"),
        }
    }

    // ── Trailing Punctuation Stripping ───────────────────────────────

    #[test]
    fn test_trailing_period() {
        // Whisper sometimes adds a period at the end
        match check_voice_command("press enter.") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::KeyPress(KeyAction::Enter));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command, got text"),
        }
    }

    #[test]
    fn test_trailing_comma() {
        match check_voice_command("undo,") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(
                    cmd.action,
                    VoiceAction::KeyCombo(vec![KeyAction::Control, KeyAction::Key('z')])
                );
            }
            VoiceCommandResult::Text(_) => panic!("Expected command, got text"),
        }
    }

    // ── Non-Command Text ────────────────────────────────────────────

    #[test]
    fn test_normal_text_not_command() {
        match check_voice_command("I want to press enter to continue") {
            VoiceCommandResult::Text(text) => {
                assert_eq!(text, "I want to press enter to continue");
            }
            VoiceCommandResult::Command(_) => panic!("Expected text, got command"),
        }
    }

    #[test]
    fn test_empty_text() {
        match check_voice_command("") {
            VoiceCommandResult::Text(text) => {
                assert_eq!(text, "");
            }
            VoiceCommandResult::Command(_) => panic!("Expected text, got command"),
        }
    }

    #[test]
    fn test_partial_match_not_command() {
        // "delete" alone is a command, but "delete the file" is not
        match check_voice_command("delete the file") {
            VoiceCommandResult::Text(_) => {} // expected
            VoiceCommandResult::Command(_) => panic!("Should not match partial text"),
        }
    }

    // ── Specific Commands ───────────────────────────────────────────

    #[test]
    fn test_undo_command() {
        match check_voice_command("undo") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(
                    cmd.action,
                    VoiceAction::KeyCombo(vec![KeyAction::Control, KeyAction::Key('z')])
                );
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    #[test]
    fn test_redo_command() {
        match check_voice_command("redo") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(
                    cmd.action,
                    VoiceAction::KeyCombo(vec![KeyAction::Control, KeyAction::Key('y')])
                );
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    #[test]
    fn test_copy_command() {
        match check_voice_command("copy") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.description, "Copy (Ctrl+C)");
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    #[test]
    fn test_save_command() {
        match check_voice_command("save") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.description, "Save (Ctrl+S)");
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    #[test]
    fn test_tab_command() {
        match check_voice_command("tab") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::KeyPress(KeyAction::Tab));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    #[test]
    fn test_backspace_command() {
        match check_voice_command("backspace") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::KeyPress(KeyAction::Backspace));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    #[test]
    fn test_delete_word_russian() {
        match check_voice_command("удали слово") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.description, "Delete Word (Ctrl+Backspace)");
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    #[test]
    fn test_arrow_keys() {
        match check_voice_command("up") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::KeyPress(KeyAction::Up));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }

        match check_voice_command("вниз") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::KeyPress(KeyAction::Down));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    #[test]
    fn test_punctuation_commands() {
        match check_voice_command("period") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::TypeText(".".to_string()));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }

        match check_voice_command("запятая") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::TypeText(",".to_string()));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }

        match check_voice_command("question mark") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::TypeText("?".to_string()));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    // ── Russian Aliases ─────────────────────────────────────────────

    #[test]
    fn test_russian_new_line() {
        match check_voice_command("новая строка") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.action, VoiceAction::KeyPress(KeyAction::Enter));
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    #[test]
    fn test_russian_select_all() {
        match check_voice_command("выдели все") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.description, "Select All (Ctrl+A)");
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    #[test]
    fn test_russian_undo() {
        match check_voice_command("отмени") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.description, "Undo (Ctrl+Z)");
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    #[test]
    fn test_russian_copy() {
        match check_voice_command("скопируй") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.description, "Copy (Ctrl+C)");
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    #[test]
    fn test_russian_paste() {
        match check_voice_command("вставь") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.description, "Paste (Ctrl+V)");
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    #[test]
    fn test_russian_save() {
        match check_voice_command("сохрани") {
            VoiceCommandResult::Command(cmd) => {
                assert_eq!(cmd.description, "Save (Ctrl+S)");
            }
            VoiceCommandResult::Text(_) => panic!("Expected command"),
        }
    }

    // ── Normalize ───────────────────────────────────────────────────

    #[test]
    fn test_normalize() {
        assert_eq!(normalize("  Hello   World  "), "hello world");
        assert_eq!(normalize("PRESS ENTER"), "press enter");
        assert_eq!(normalize("нажми  ввод"), "нажми ввод");
        assert_eq!(normalize(""), "");
    }

    // ── List Commands ───────────────────────────────────────────────

    #[test]
    fn test_list_commands_not_empty() {
        let commands = list_commands();
        assert!(!commands.is_empty());
        // Should have at least the core commands
        assert!(commands.len() >= 10);
    }
}
