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
    tracing::info!(?hotkey, "hotkey::install");
    let coord = build_coordinator(app, hotkey)?;
    *COORDINATOR.lock().unwrap() = Some(coord);
    tracing::info!("hotkey::install: coordinator installed");
    Ok(())
}

pub fn reinstall(app: &AppHandle, hotkey: Hotkey) -> Result<(), AppError> {
    tracing::info!(?hotkey, "hotkey::reinstall");
    let coord = build_coordinator(app.clone(), hotkey)?;
    // Replace (and thereby drop) the previous coordinator. Drop on the old
    // HotkeyListener stops its run loop + tears down the EventTap.
    *COORDINATOR.lock().unwrap() = Some(coord);
    tracing::info!("hotkey::reinstall: coordinator replaced");
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
    tracing::info!("dictation_loop: started");
    while let Some(event) = rx.recv().await {
        tracing::info!(?event, "dictation_loop: received hotkey event");
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
    let device_name = inner.settings.input_device_name.clone();

    let result = (|| -> Result<Recorder, String> {
        let cfg = RecorderConfig { device_name, ..Default::default() };
        let mut recorder = Recorder::new(cfg).map_err(|e| e.to_string())?;
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

/// Full STT → cleanup → inject pipeline.
///
/// Stages:
///   1. Whisper STT (blocking → `spawn_blocking`).
///   2. LLM cleanup (blocking → `spawn_blocking`); graceful degrade to raw
///      transcript if the llama backend is missing or fails.
///   3. Text injection (AX → pasteboard fallback inside `Injector`); on hard
///      failure, copy the cleaned text to the system clipboard and toast.
///
/// Each failure mode emits a `toast` event so the UI can surface it. Successful
/// runs emit a single tracing line with per-stage and total wall-clock timings.
async fn run_pipeline(app: AppHandle, wav: Vec<u8>) {
    let t0 = std::time::Instant::now();
    let state = app.state::<AppState>();

    // Snapshot what we need; release the lock before any heavy work so we
    // never hold the std::sync::Mutex across an await.
    let (whisper, llama, language, force_pasteboard) = {
        let inner = state.inner.lock().unwrap();
        (
            inner.whisper.clone(),
            inner.llama.clone(),
            inner.settings.language.clone(),
            inner.settings.force_pasteboard,
        )
    };

    let Some(whisper) = whisper else {
        let _ = app.emit(
            "toast",
            crate::commands::toast("warn", "Whisper model not loaded"),
        );
        return;
    };

    // 1. Transcribe.
    let lang_for_stt = if language == "auto" {
        String::new()
    } else {
        language.clone()
    };
    let wav_for_stt = wav;
    let stt = tokio::task::spawn_blocking(move || whisper.transcribe(&wav_for_stt, &lang_for_stt))
        .await;
    let raw_text = match stt {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => {
            let _ = app.emit(
                "toast",
                crate::commands::toast("error", format!("Transcription failed: {e}")),
            );
            return;
        }
        Err(e) => {
            let _ = app.emit(
                "toast",
                crate::commands::toast("error", format!("STT join error: {e}")),
            );
            return;
        }
    };
    let t1 = t0.elapsed();

    // 2. Clean (graceful degrade if LLM missing or fails).
    let cleaned = if let Some(llama) = llama {
        let lang_for_clean = language.clone();
        let raw_for_clean = raw_text.clone();
        let r = tokio::task::spawn_blocking(move || {
            llama.chat_sync(inference_core::ChatRequest {
                system: Some(lda_prompts::build_clean_system_prompt(&lang_for_clean)),
                history: vec![],
                user: raw_for_clean,
                temperature: 0.2,
                max_tokens: 2048,
                stop: vec![],
            })
        })
        .await;
        match r {
            Ok(Ok(resp)) => resp.text,
            _ => {
                let _ = app.emit(
                    "toast",
                    crate::commands::toast("warn", "Cleanup failed — typed raw transcript"),
                );
                raw_text
            }
        }
    } else {
        raw_text
    };
    let t2 = t0.elapsed();

    // 3. Inject (graceful degrade to clipboard).
    let injector = if force_pasteboard {
        os_integration::Injector::with_force_pasteboard(true)
    } else {
        os_integration::Injector::new()
    };
    match injector.inject(&cleaned) {
        Ok(method) => {
            let t3 = t0.elapsed();
            tracing::info!(
                stt_ms = t1.as_millis() as u64,
                clean_ms = (t2 - t1).as_millis() as u64,
                inject_ms = (t3 - t2).as_millis() as u64,
                total_ms = t3.as_millis() as u64,
                method = ?method,
                "dictation complete"
            );
        }
        Err(e) => {
            let copied = os_integration::clipboard::set_text(&cleaned);
            let msg = if copied {
                format!("Inject failed: {e} — text copied to clipboard")
            } else {
                format!("Inject failed: {e} — clipboard copy also failed")
            };
            let _ = app.emit("toast", crate::commands::toast("error", msg));
        }
    }
}
