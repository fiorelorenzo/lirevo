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
use os_integration::audio_cue::{self, CueKind};
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
            HotkeyEvent::Down => handle_down(&app, &state).await,
            HotkeyEvent::Up => handle_up(&app, &state),
        }
    }
    tracing::warn!("hotkey event channel closed; dictation loop exiting");
}

async fn handle_down(app: &AppHandle, state: &tauri::State<'_, AppState>) {
    tracing::info!("handle_down: hotkey pressed");
    let ms = state.current_model_state();
    // The engine lazy-unloads STT under memory pressure, so the UI may report
    // Ready while the slot is currently unloaded; `ensure_stt` below reloads it
    // on demand. We still gate on a non-error, non-idle state so a genuinely
    // unconfigured / failed setup short-circuits with a clear toast.
    let stt_ok = matches!(
        ms,
        ModelState::Ready { stt: true, .. } | ModelState::Loading { stt: true, .. }
    );
    if !stt_ok {
        tracing::warn!(?ms, "handle_down: STT not ready, ignoring");
        let _ = app.emit(
            "toast",
            crate::commands::toast("warn", "Transcription model not ready — open Settings"),
        );
        return;
    }

    // Snapshot what we need under the lock, then drop the guard before any
    // heavy work (Recorder::new opens the CoreAudio device — tens of ms).
    // Holding std::sync::Mutex across that blocks other Tauri commands that
    // also lock AppState.
    let device_name = {
        let inner = state.inner.lock().unwrap();
        if inner.recorder.is_some() {
            tracing::info!("handle_down: already recording (duplicate Down)");
            return;
        }
        inner.settings.input_device_name.clone()
    };

    let result = (|| -> Result<Recorder, String> {
        let cfg = RecorderConfig { device_name, ..Default::default() };
        let mut recorder = Recorder::new(cfg).map_err(|e| e.to_string())?;
        recorder.start().map_err(|e| e.to_string())?;
        Ok(recorder)
    })();

    match result {
        Ok(recorder) => {
            tracing::info!("handle_down: recorder started");
            audio_cue::play(CueKind::Start);
            // Forward audio levels (RMS, throttled to ~33 Hz inside the recorder)
            // to the shared watch channel + a Tauri event for the overlay.
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

            // Snapshot what the streaming worker needs (engine + dictation
            // language) before re-acquiring the inner lock to install the
            // recorder + streaming handle.
            let (engine, language) = {
                let inner = state.inner.lock().unwrap();
                (inner.engine.clone(), inner.settings.language.clone())
            };
            // Lazy-load the STT slot for this dictation. If it's still
            // downloading (None) or errors, fall back to the no-streaming path
            // (run_pipeline will retry via ensure_stt one-shot).
            let stt_slot = match engine.ensure_stt().await {
                Ok(slot) => slot,
                Err(e) => {
                    tracing::warn!(%e, "handle_down: ensure_stt failed; no streaming session");
                    None
                }
            };
            if stt_slot.is_some() {
                engine.mark_stt_used();
            }
            let streaming = stt_slot.map(|slot| {
                let lang_opt = if language == "auto" || language.is_empty() {
                    None
                } else {
                    Some(language.clone())
                };
                crate::commands::inference::spawn_streaming_session(
                    app.clone(),
                    slot,
                    lang_opt,
                )
            });

            // Re-acquire briefly to install the recorder + streaming handle.
            {
                let mut inner = state.inner.lock().unwrap();
                inner.recorder = Some(recorder);
                inner.streaming = streaming;
            }
            let _ = state.recording_state_tx.send(true);
            let _ = app.emit("recording:state", true);
            show_overlay(app);
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
    tracing::info!("handle_up: hotkey released");
    let (recorder, streaming) = {
        let mut inner = state.inner.lock().unwrap();
        (inner.recorder.take(), inner.streaming.take())
    };
    let Some(mut r) = recorder else {
        tracing::warn!("handle_up: no active recorder (Down was ignored?)");
        // Up without an active Down (e.g. permission popup ate the Down event).
        return;
    };

    let samples = match r.stop() {
        Ok(recording) => {
            tracing::info!(samples = recording.samples.len(), "handle_up: recording stopped");
            audio_cue::play(CueKind::Stop);
            recording.samples
        }
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
    hide_overlay_with_delay(app);

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        run_pipeline(app2, samples, streaming).await;
    });
}

/// Show the recording overlay window if it exists. Silent no-op if it
/// hasn't been created (e.g. setup failed). Best-effort.
fn show_overlay(app: &AppHandle) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.show();
    }
}

/// Hide the overlay after a short grace period so the waveform visibly
/// settles before the pill disappears.
fn hide_overlay_with_delay(app: &AppHandle) {
    use tauri::Manager;
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if let Some(w) = app2.get_webview_window("overlay") {
            let _ = w.hide();
        }
    });
}

/// Full STT → cleanup → inject pipeline.
///
/// Stages:
///   1. audiopipe STT (blocking → `spawn_blocking`).
///   2. LLM cleanup (blocking → `spawn_blocking`); graceful degrade to raw
///      transcript if the llama backend is missing or fails.
///   3. Text injection (AX → pasteboard fallback inside `Injector`); on hard
///      failure, copy the cleaned text to the system clipboard and toast.
///
/// Each failure mode emits a `toast` event so the UI can surface it. Successful
/// runs emit a single tracing line with per-stage and total wall-clock timings.
async fn run_pipeline(
    app: AppHandle,
    samples: Vec<f32>,
    streaming: Option<crate::commands::inference::StreamingHandle>,
) {
    let t0 = std::time::Instant::now();
    let state = app.state::<AppState>();

    // Audio duration (16 kHz mono) captured before STT consumes `samples`.
    let audio_ms = (samples.len() as i64) * 1000 / 16_000;

    // Snapshot what we need; release the lock before any heavy work so we
    // never hold the std::sync::Mutex across an await.
    let (engine, language, force_pasteboard, record_history) = {
        let inner = state.inner.lock().unwrap();
        (
            inner.engine.clone(),
            inner.settings.language.clone(),
            inner.settings.force_pasteboard,
            inner.settings.record_history,
        )
    };
    // `Db` is internally synchronized and lives outside `inner`'s mutex; clone
    // the Arc so it can move into the history-insert task at the end.
    let db_arc = state.db.clone();

    // Lazy-load the STT slot for the one-shot fallback path. The streaming
    // worker (if any) already holds its own clone.
    let stt_slot = match engine.ensure_stt().await {
        Ok(Some(slot)) => slot,
        Ok(None) | Err(_) => {
            let _ = app.emit(
                "toast",
                crate::commands::toast("warn", "Transcription model not loaded"),
            );
            return;
        }
    };

    let lang_for_stt = if language == "auto" || language.is_empty() {
        None
    } else {
        Some(language.clone())
    };

    // 1. Transcribe. Prefer the streaming worker's cumulative output; fall
    // back to the one-shot path when streaming is unsupported (Whisper /
    // Qwen3-ASR) or errored mid-stream.
    let raw_text = match streaming {
        Some(handle) => {
            let _ = handle.stop_tx.send(samples.clone());
            match handle.result_rx.await {
                Ok(Some(text)) => text,
                _ => {
                    tracing::info!(
                        "run_pipeline: streaming worker yielded no text — running one-shot"
                    );
                    match crate::commands::inference::transcribe_samples_async(
                        stt_slot, samples, lang_for_stt,
                    )
                    .await
                    {
                        Ok(t) => t,
                        Err(e) => {
                            let _ = app.emit(
                                "toast",
                                crate::commands::toast("error", format!("Transcription failed: {e}")),
                            );
                            return;
                        }
                    }
                }
            }
        }
        None => match crate::commands::inference::transcribe_samples_async(
            stt_slot, samples, lang_for_stt,
        )
        .await
        {
            Ok(t) => t,
            Err(e) => {
                let _ = app.emit(
                    "toast",
                    crate::commands::toast("error", format!("Transcription failed: {e}")),
                );
                return;
            }
        },
    };
    // Transcription done — mark the STT used so the idle-unload timer is
    // use-relative (not load-relative).
    engine.mark_stt_used();
    let t1 = t0.elapsed();

    // Model ids for the history row, derived from the engine's live config.
    // `stt_model` falls back to the default catalog id when unset (matching
    // `ensure_stt`); `llm_model` is the LLM file's basename, or `None` in
    // STT-only mode.
    let cfg = engine.current_config();
    let stt_model = cfg
        .stt_model_id
        .clone()
        .unwrap_or_else(|| crate::stt::catalog::default_model_id().to_string());
    let llm_model: Option<String> = cfg.llm_model_path.as_ref().and_then(|p| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .map(std::string::ToString::to_string)
    });

    // 2. Clean (graceful degrade if LLM missing or fails). The engine
    // lazy-loads the LLM on demand; Ok(None) means no cleanup model is
    // configured (STT-only mode), so we type the raw transcript as-is.
    //
    // The match also records the cleanup outcome for the history row:
    //   - Applied: cleanup ran (LLM configured), no error, clean stage timed.
    //   - Skipped: STT-only mode, no LLM, no clean stage timing.
    //   - Failed:  cleanup attempted (LLM configured) but errored; the failed
    //     attempt still consumed wall time, so the clean stage is timed.
    let mut cleanup_status = crate::db::history::CLEANUP_APPLIED;
    let mut cleanup_error: Option<String> = None;
    // Whether the clean stage ran (Applied or Failed) vs was skipped.
    let mut cleanup_ran = true;
    let cleaned = match engine
        .chat(inference_core::ChatRequest {
            system: Some(lirevo_prompts::build_clean_system_prompt(&language)),
            history: vec![],
            user: raw_text.clone(),
            temperature: 0.2,
            max_tokens: 2048,
            stop: vec![],
        })
        .await
    {
        Ok(Some(resp)) => resp.text,
        Ok(None) => {
            cleanup_status = crate::db::history::CLEANUP_SKIPPED;
            cleanup_ran = false;
            raw_text.clone()
        }
        Err(e) => {
            tracing::warn!(?e, "run_pipeline: cleanup failed; typing raw transcript");
            cleanup_status = crate::db::history::CLEANUP_FAILED;
            cleanup_error = Some(e.to_string());
            let _ = app.emit(
                "toast",
                crate::commands::toast("warn", "Cleanup failed — typed raw transcript"),
            );
            raw_text.clone()
        }
    };
    let t2 = t0.elapsed();
    // Timed for Applied + Failed; None for Skipped (STT-only).
    let clean_ms: Option<i64> = if cleanup_ran {
        Some((t2 - t1).as_millis() as i64)
    } else {
        None
    };

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

            // Record this successful dictation. Best-effort: a DB failure must
            // never disrupt the dictation flow (we've already typed the text).
            if record_history {
                let (target_app, target_bundle) = os_integration::frontmost_app()
                    .map(|a| (a.name, a.bundle_id))
                    .unwrap_or((None, None));
                let created_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                let entry = crate::db::history::NewDictation {
                    created_at,
                    language: Some(language.clone()),
                    stt_model,
                    audio_ms: Some(audio_ms),
                    raw_text: raw_text.clone(),
                    stt_ms: t1.as_millis() as i64,
                    llm_model,
                    cleaned_text: cleaned.clone(),
                    clean_ms,
                    cleanup_status: cleanup_status.to_string(),
                    cleanup_error,
                    inject_method: format!("{method:?}").to_lowercase(),
                    inject_ms: Some((t3 - t2).as_millis() as i64),
                    total_ms: t3.as_millis() as i64,
                    target_app,
                    target_bundle,
                };
                let db = db_arc.clone();
                let app2 = app.clone();
                tokio::task::spawn_blocking(move || {
                    match crate::db::history::insert(&db, &entry) {
                        Ok(id) => {
                            let mut preview: String =
                                entry.cleaned_text.chars().take(120).collect();
                            if entry.cleaned_text.chars().count() > 120 {
                                preview.push('…');
                            }
                            let summary = crate::db::history::DictationSummary {
                                id,
                                created_at: entry.created_at,
                                preview,
                                stt_model: entry.stt_model.clone(),
                                llm_model: entry.llm_model.clone(),
                                target_app: entry.target_app.clone(),
                                total_ms: entry.total_ms,
                                cleanup_status: entry.cleanup_status.clone(),
                            };
                            let _ = app2.emit("dictation:saved", &summary);
                        }
                        Err(e) => {
                            tracing::warn!(?e, "failed to save dictation history (non-fatal)");
                        }
                    }
                });
            }
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

    // Record the last-dictation time on the engine. Reserved for future
    // preload / idle-unload refinement — `lifecycle_decision` does not yet
    // consume `last_dictation` (it takes it as `_last_dictation`).
    engine.mark_dictation();
}
