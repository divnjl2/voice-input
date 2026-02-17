#[cfg(unix)]
use crate::TranscriptionCoordinator;
#[cfg(unix)]
use log::{debug, info, warn};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use tauri::{AppHandle, Manager};

#[cfg(unix)]
use signal_hook::consts::{SIGUSR1, SIGUSR2};
#[cfg(unix)]
use signal_hook::iterator::Signals;

#[cfg(unix)]
pub fn setup_signal_handler(app_handle: AppHandle, mut signals: Signals) {
    debug!("Signal handler registered for SIGUSR1 and SIGUSR2");
    thread::spawn(move || {
        debug!("Signal handler thread started");
        for sig in signals.forever() {
            let (binding_id, signal_name) = match sig {
                SIGUSR2 => ("transcribe", "SIGUSR2"),
                SIGUSR1 => ("transcribe_with_post_process", "SIGUSR1"),
                _ => continue,
            };
            debug!("Received {signal_name} signal");

            if let Some(coordinator) = app_handle.try_state::<TranscriptionCoordinator>() {
                coordinator.send_input(binding_id, signal_name, true, false);
                info!("{signal_name}: sent toggle to coordinator for '{binding_id}'");
            } else {
                warn!("TranscriptionCoordinator is not initialized");
            }
        }
    });
}
