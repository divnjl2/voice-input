//! Shared shortcut event handling logic
//!
//! This module contains the common logic for handling shortcut events,
//! used by both the Tauri and handy-keys implementations.

use log::{debug, warn};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use crate::settings::get_settings;
use crate::ManagedToggleState;

/// Tracks when recording started (epoch millis) to prevent spurious key-up stops.
/// In push-to-talk mode, overlay window show can cause a fake key-up event on Windows.
static RECORDING_START_MS: AtomicU64 = AtomicU64::new(0);

/// Minimum time (ms) between start and stop to prevent spurious key-up events
const MIN_RECORDING_DURATION_MS: u64 = 800;

/// Handle a shortcut event from either implementation.
///
/// This function contains the shared logic for:
/// - Looking up the action in ACTION_MAP
/// - Handling the cancel binding (only fires when recording)
/// - Handling push-to-talk mode (start on press, stop on release)
/// - Handling toggle mode (toggle state on press only)
///
/// # Arguments
/// * `app` - The Tauri app handle
/// * `binding_id` - The ID of the binding (e.g., "transcribe", "cancel")
/// * `hotkey_string` - The string representation of the hotkey
/// * `is_pressed` - Whether this is a key press (true) or release (false)
pub fn handle_shortcut_event(
    app: &AppHandle,
    binding_id: &str,
    hotkey_string: &str,
    is_pressed: bool,
) {
    let settings = get_settings(app);

    let Some(action) = ACTION_MAP.get(binding_id) else {
        warn!(
            "No action defined in ACTION_MAP for shortcut ID '{}'. Shortcut: '{}', Pressed: {}",
            binding_id, hotkey_string, is_pressed
        );
        return;
    };

    // Cancel binding: only fires when recording and key is pressed
    if binding_id == "cancel" {
        let audio_manager = app.state::<Arc<AudioRecordingManager>>();
        if audio_manager.is_recording() && is_pressed {
            action.start(app, binding_id, hotkey_string);
        }
        return;
    }

    // Toggle settings: fire on press only, ignore PTT mode
    if binding_id == "toggle_settings" {
        if is_pressed {
            action.start(app, binding_id, hotkey_string);
        }
        return;
    }

    // Push-to-talk mode: start on press, stop on release
    if settings.push_to_talk {
        if is_pressed {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            RECORDING_START_MS.store(now, Ordering::SeqCst);
            action.start(app, binding_id, hotkey_string);
        } else {
            // Guard against spurious key-up events (e.g. from overlay window activation).
            // Ignore key-up if recording started less than MIN_RECORDING_DURATION_MS ago.
            let started = RECORDING_START_MS.load(Ordering::SeqCst);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let elapsed = now.saturating_sub(started);
            if elapsed < MIN_RECORDING_DURATION_MS {
                debug!(
                    "Ignoring spurious key-up after {}ms (min {}ms)",
                    elapsed, MIN_RECORDING_DURATION_MS
                );
                return;
            }
            RECORDING_START_MS.store(0, Ordering::SeqCst);
            action.stop(app, binding_id, hotkey_string);
        }
        return;
    }

    // Toggle mode: toggle state on press only
    if is_pressed {
        // Debounce: ignore key-repeat events that arrive too quickly after the last toggle.
        // Windows key repeat delay (~500ms) can fire repeated presses while holding the key,
        // which would toggle recording off prematurely.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let last = RECORDING_START_MS.load(Ordering::SeqCst);
        let elapsed = now.saturating_sub(last);
        if last > 0 && elapsed < MIN_RECORDING_DURATION_MS {
            debug!(
                "Toggle mode: ignoring key-repeat after {}ms (min {}ms)",
                elapsed, MIN_RECORDING_DURATION_MS
            );
            return;
        }

        // Determine action and update state while holding the lock,
        // but RELEASE the lock before calling the action to avoid deadlocks.
        // (Actions may need to acquire the lock themselves, e.g., cancel_current_operation)
        let should_start: bool;
        {
            let toggle_state_manager = app.state::<ManagedToggleState>();
            let mut states = toggle_state_manager
                .lock()
                .expect("Failed to lock toggle state manager");

            let is_currently_active = states
                .active_toggles
                .entry(binding_id.to_string())
                .or_insert(false);

            should_start = !*is_currently_active;
            *is_currently_active = should_start;
        } // Lock released here

        // Track toggle timestamp for debounce
        RECORDING_START_MS.store(now, Ordering::SeqCst);

        // Now call the action without holding the lock
        if should_start {
            action.start(app, binding_id, hotkey_string);
        } else {
            action.stop(app, binding_id, hotkey_string);
        }
    }
}
