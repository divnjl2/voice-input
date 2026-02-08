# Handy Voice Input — Fork Improvement Plan

## Overview
Fork of [cjpais/Handy](https://github.com/cjpais/Handy) with 4 targeted improvements.

## Task 1: Fix Unicode Paths (Bug #574)
**Problem:** Parakeet/Whisper models fail to load when Windows username contains Cyrillic (e.g. `C:\Users\пк\AppData\...`)
**Root cause:** `transcribe-rs` or underlying ONNX runtime can't handle non-ASCII paths
**Files to modify:**
- `src-tauri/src/managers/model.rs` — `get_model_path()`, `download_model()`, model extraction
**Fix strategy:**
- Option A: Convert paths to Windows short names (8.3 format) before passing to ML runtime
- Option B: Use `\\?\` extended-length path prefix
- Option C: Symlink/junction models dir to ASCII-safe path automatically
- Option D: Copy model to temp ASCII path at load time

## Task 2: Minimize to System Tray
**Current state:** Tray already exists (`src-tauri/src/tray.rs`) with menu items
**Missing:** Close/minimize window → hide to tray instead of quit
**Files to modify:**
- `src-tauri/src/tray.rs` — add "Show/Hide" menu item, handle tray icon click
- `src-tauri/src/lib.rs` — intercept window close event → hide instead of close
- `src-tauri/tauri.conf.json` — window close behavior config
**Fix strategy:**
- Hook `on_window_event` for CloseRequested → `window.hide()` instead of close
- Add tray click handler to toggle main window visibility
- Add "Show/Hide Settings" to tray menu

## Task 3: Global Hotkey for Settings Window
**Current state:** Only overlay has hotkey, settings window has no global shortcut
**Files to modify:**
- `src-tauri/src/shortcut/` — add new shortcut binding for toggle settings
- `src-tauri/src/settings.rs` — add `toggle_settings_shortcut` field
- `src/components/settings/` — add UI for configuring this shortcut
**Fix strategy:**
- Add new `ShortcutBinding` for settings toggle
- Register it alongside existing recording shortcut
- Handler: show/hide main window + focus

## Task 4: Fix Overlay Bugs on Windows 11 (Bug #508)
**Problem:** Overlay shows wrong states (transcribing text instead of bars)
**Files to investigate:**
- `src-tauri/src/overlay.rs` — platform-specific overlay management
- `src/overlay/RecordingOverlay.tsx` — React overlay component
- Event timing between "show-overlay", "hide-overlay", "mic-level"
**Fix strategy:**
- Audit event emission order in recording flow
- Add state machine validation (recording→transcribing→idle only)
- Fix Win32 window Z-order timing issues
