//! Push-to-talk coordinator: bridges os-integration's CFRunLoop-based
//! HotkeyListener into tokio, drives the Recorder, and emits recording state +
//! audio level events to the frontend.
//!
//! On Hotkey Down: start the Recorder + spawn a task that forwards live RMS
//! levels (from the recorder's `watch::Receiver<f32>`) to the
//! `recording:level` Tauri event and the shared `audio_level_tx`.
//! On Hotkey Up:   stop the Recorder, encode the captured samples to WAV, and
//! hand the bytes off to the (currently empty) pipeline body.

use std::sync::Mutex;

use once_cell::sync::Lazy;
use tauri::{AppHandle, Emitter, Manager};

use audio_capture::{Recorder, RecorderConfig};
use os_integration::{Hotkey as OsHotkey, HotkeyEvent, HotkeyListener};

use crate::settings::Hotkey;
use crate::state::ModelState;
use crate::{AppError, AppState};

/// Currently-installed coordinator. Held in a global so `reinstall` can drop
/// the previous listener (which uninstalls its CGEventTap via `Drop`) and
/// replace it with a freshly-installed one when the user changes hotkey.
static COORDINATOR: Lazy<Mutex<Option<DictationCoordinator>>> = Lazy::new(|| Mutex::new(None));

pub struct DictationCoordinator {
    // Held purely for ownership: dropping the listener stops the CFRunLoop
    // thread and uninstalls the EventTap.
    _listener: HotkeyListener,
}

pub fn install(app: AppHandle, hotkey: Hotkey) -> Result<(), AppError> {
    let coord = build_coordinator(app, hotkey)?;
    *COORDINATOR.lock().unwrap() = Some(coord);
    Ok(())
}

pub fn reinstall(app: &AppHandle, hotkey: Hotkey) -> Result<(), AppError> {
    let coord = build_coordinator(app.clone(), hotkey)?;
    // Replace (and thereby drop) the previous coordinator. Drop on the old
    // HotkeyListener stops its run loop + tears down the EventTap.
    *COORDINATOR.lock().unwrap() = Some(coord);
    Ok(())
}

fn build_coordinator(app: AppHandle, hotkey: Hotkey) -> Result<DictationCoordinator, AppError> {
    let os_hotkey = map_hotkey(hotkey);
    let (listener, rx) =
        HotkeyListener::install(os_hotkey).map_err(|e| AppError::Hotkey(e.to_string()))?;

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        dictation_loop(app2, rx).await;
    });

    Ok(DictationCoordinator { _listener: listener })
}

fn map_hotkey(h: Hotkey) -> OsHotkey {
    match h {
        Hotkey::RightOption => OsHotkey::RightOption,
        Hotkey::LeftOption => OsHotkey::LeftOption,
        Hotkey::RightCommand => OsHotkey::RightCommand,
        Hotkey::Fn => OsHotkey::Fn,
        Hotkey::F5 => OsHotkey::F5,
    }
}

async fn dictation_loop(app: AppHandle, mut rx: tokio::sync::mpsc::Receiver<HotkeyEvent>) {
    while let Some(event) = rx.recv().await {
        let state = app.state::<AppState>();
        match event {
            HotkeyEvent::Down => handle_down(&app, &state),
            HotkeyEvent::Up => handle_up(&app, &state),
        }
    }
    tracing::warn!("hotkey event channel closed; dictation loop exiting");
}

fn handle_down(app: &AppHandle, state: &tauri::State<AppState>) {
    let ms = state.current_model_state();
    let whisper_ok = matches!(ms, ModelState::Ready { whisper: true, .. });
    if !whisper_ok {
        let _ = app.emit(
            "toast",
            crate::commands::toast("warn", "Whisper model not ready — open Settings"),
        );
        return;
    }

    let mut inner = state.inner.lock().unwrap();
    if inner.recorder.is_some() {
        // Already recording (duplicate Down). Ignore — Up will clean up.
        return;
    }

    let result = (|| -> Result<Recorder, String> {
        let mut recorder =
            Recorder::new(RecorderConfig::default()).map_err(|e| e.to_string())?;
        recorder.start().map_err(|e| e.to_string())?;
        Ok(recorder)
    })();

    match result {
        Ok(recorder) => {
            // Forward audio levels (RMS, throttled to ~33 Hz inside the recorder)
            // to the shared watch channel + a Tauri event for the RecordingIndicator.
            let mut level_rx = recorder.level_rx();
            let app2 = app.clone();
            let level_tx = state.audio_level_tx.clone();
            tauri::async_runtime::spawn(async move {
                while level_rx.changed().await.is_ok() {
                    let level = *level_rx.borrow();
                    let _ = level_tx.send(level);
                    let _ = app2.emit("recording:level", level);
                }
            });

            inner.recorder = Some(recorder);
            let _ = state.recording_state_tx.send(true);
            let _ = app.emit("recording:state", true);
        }
        Err(e) => {
            tracing::warn!(error = %e, "recorder start failed");
            let _ = app.emit(
                "toast",
                crate::commands::toast("error", format!("Mic start failed: {e}")),
            );
        }
    }
}

fn handle_up(app: &AppHandle, state: &tauri::State<AppState>) {
    let recorder = state.inner.lock().unwrap().recorder.take();
    let Some(mut r) = recorder else {
        // Up without an active Down (e.g. permission popup ate the Down event).
        return;
    };

    let wav = match r.stop() {
        Ok(recording) => convert_recording_to_wav(&recording),
        Err(e) => {
            tracing::warn!(error = %e, "recorder stop failed");
            let _ = state.recording_state_tx.send(false);
            let _ = app.emit("recording:state", false);
            let _ = app.emit(
                "toast",
                crate::commands::toast("error", format!("Mic stop failed: {e}")),
            );
            return;
        }
    };

    let _ = state.recording_state_tx.send(false);
    let _ = app.emit("recording:state", false);

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        run_pipeline(app2, wav).await;
    });
}

/// Encode the captured 16 kHz mono f32 samples to a PCM16 WAV byte vector.
/// Mirrors `lda-prototype`: `audio_capture::samples_to_wav(&rec.samples)`.
fn convert_recording_to_wav(recording: &audio_capture::Recording) -> Vec<u8> {
    audio_capture::samples_to_wav(&recording.samples)
}

/// Full STT → cleanup → inject pipeline. T14 fills the body.
async fn run_pipeline(_app: AppHandle, _wav: Vec<u8>) {
    // Intentionally empty until T14.
}
