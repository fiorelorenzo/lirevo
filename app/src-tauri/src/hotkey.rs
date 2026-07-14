//! Push-to-talk coordinator: bridges os-integration's CFRunLoop-based
//! HotkeyListener into tokio, drives the Recorder, and emits recording state +
//! audio level events to the frontend.
//!
//! On Hotkey Down: start the Recorder + spawn a task that forwards live RMS
//! levels (from the recorder's `watch::Receiver<f32>`) to the
//! `recording:level` Tauri event and the shared `audio_level_tx`.
//! On Hotkey Up:   stop the Recorder, encode the captured samples to WAV, and
//! hand the bytes off to the (currently empty) pipeline body.

use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;
use tauri::{AppHandle, Emitter, Manager};

use audio_capture::{Recorder, RecorderConfig};
use os_integration::audio_cue::{self, CueKind};
use os_integration::{ActivationMode, HotkeyEvent, HotkeyListener, HotkeySpec};

use crate::state::ModelState;
use crate::{AppError, AppState};

/// Currently-installed coordinator. Held in a global so `reinstall` can drop
/// the previous listener (which uninstalls its CGEventTap via `Drop`) and
/// replace it with a freshly-installed one when the user changes hotkey.
static COORDINATOR: Lazy<Mutex<Option<DictationCoordinator>>> = Lazy::new(|| Mutex::new(None));

pub struct DictationCoordinator {
    // Owns the listener: dropping it stops the CFRunLoop thread and uninstalls
    // the EventTap. Also read by `start_capture`/`stop_capture` to toggle the
    // tap's capture mode for live hotkey re-recording.
    listener: HotkeyListener,
}

pub fn install(app: AppHandle, spec: HotkeySpec, mode: ActivationMode) -> Result<(), AppError> {
    tracing::info!(?spec, ?mode, "hotkey::install");
    let coord = build_coordinator(app, spec, mode)?;
    *COORDINATOR.lock().unwrap() = Some(coord);
    tracing::info!("hotkey::install: coordinator installed");
    Ok(())
}

/// Reject a hotkey/activation-mode reinstall while a recording is in flight.
///
/// Swapping `COORDINATOR` drops the previously-installed `HotkeyListener`,
/// tearing down its CGEventTap and abandoning the `dictation_loop` task that
/// would have delivered the matching `Up` event. Without this guard, an
/// in-flight `AppStateInner::recorder`/`streaming` would be stranded until
/// app restart. Split out as a pure function so the decision is testable
/// without a live `AppHandle`/`HotkeyListener`.
fn reject_if_recording(recording: bool) -> Result<(), AppError> {
    if recording {
        return Err(AppError::HotkeyBusy);
    }
    Ok(())
}

pub fn reinstall(app: &AppHandle, spec: HotkeySpec, mode: ActivationMode) -> Result<(), AppError> {
    tracing::info!(?spec, ?mode, "hotkey::reinstall");
    let recording = {
        let state = app.state::<AppState>();
        let inner = state.inner.lock().unwrap();
        inner.recorder.is_some()
    };
    reject_if_recording(recording).inspect_err(|_| {
        tracing::warn!("hotkey::reinstall: rejected — recording in progress");
    })?;
    let coord = build_coordinator(app.clone(), spec, mode)?;
    // Replace (and thereby drop) the previous coordinator. Drop on the old
    // HotkeyListener stops its run loop + tears down the EventTap.
    *COORDINATOR.lock().unwrap() = Some(coord);
    tracing::info!("hotkey::reinstall: coordinator replaced");
    Ok(())
}

fn build_coordinator(
    app: AppHandle,
    spec: HotkeySpec,
    mode: ActivationMode,
) -> Result<DictationCoordinator, AppError> {
    let (listener, rx) =
        HotkeyListener::install(spec).map_err(|e| AppError::Hotkey(e.to_string()))?;

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        dictation_loop(app2, rx, mode).await;
    });

    Ok(DictationCoordinator { listener })
}

/// Enter capture mode: stream live key snapshots from the installed listener to
/// `tx`. The bound hotkey is suppressed while capturing. No-op (with a warn) if
/// no coordinator is installed.
pub fn start_capture(tx: tokio::sync::mpsc::Sender<os_integration::CaptureEvent>) {
    if let Some(c) = COORDINATOR.lock().unwrap().as_ref() {
        c.listener.start_capture(tx);
    } else {
        tracing::warn!("start_capture: no coordinator installed");
    }
}

/// Leave capture mode and resume normal hotkey evaluation. Silent no-op if no
/// coordinator is installed.
pub fn stop_capture() {
    if let Some(c) = COORDINATOR.lock().unwrap().as_ref() {
        c.listener.stop_capture();
    }
}

async fn dictation_loop(
    app: AppHandle,
    mut rx: tokio::sync::mpsc::Receiver<HotkeyEvent>,
    mode: ActivationMode,
) {
    use std::time::{Duration, Instant};
    let mut last_toggle = Instant::now() - Duration::from_secs(1);
    while let Some(event) = rx.recv().await {
        let state = app.state::<AppState>();
        match (mode, event) {
            (ActivationMode::Hold, HotkeyEvent::Down) => handle_down(&app, &state),
            (ActivationMode::Hold, HotkeyEvent::Up) => handle_up(&app, &state),
            (ActivationMode::Tap, HotkeyEvent::Down) => {
                if last_toggle.elapsed() < Duration::from_millis(300) {
                    continue;
                }
                last_toggle = Instant::now();
                let recording = { state.inner.lock().unwrap().recorder.is_some() };
                if recording {
                    handle_up(&app, &state);
                } else {
                    handle_down(&app, &state);
                }
            }
            (ActivationMode::Tap, HotkeyEvent::Up) => {}
        }
    }
    tracing::warn!("hotkey event channel closed; dictation loop exiting");
}

fn handle_down(app: &AppHandle, state: &tauri::State<'_, AppState>) {
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
    let (configured, smart_mic_routing, backup_input_device) = {
        let inner = state.inner.lock().unwrap();
        if inner.recorder.is_some() {
            tracing::info!("handle_down: already recording (duplicate Down)");
            return;
        }
        (
            inner.settings.input_device_name.clone(),
            inner.settings.smart_mic_routing,
            inner.settings.backup_input_device.clone(),
        )
    };

    // Decide which mic to open. With smart routing enabled, if audio is
    // playing through a Bluetooth output and the configured/default mic is a
    // Bluetooth device, route to the configured backup mic (or the built-in
    // mic by default) so the output stays in stereo.
    let choice =
        audio_capture::choose_input_device(configured, smart_mic_routing, backup_input_device);
    if choice.rerouted {
        tracing::info!(
            device = ?choice.device,
            "handle_down: smart mic routing → backup mic (Bluetooth output active)"
        );
    }
    // Human label of the device actually used, for the history row.
    let input_device = match &choice.device {
        Some(name) => name.clone(),
        None => audio_capture::default_input_device_label().unwrap_or_else(|_| "(default)".into()),
    };
    let recording_meta = crate::state::RecordingMeta {
        input_device,
        smart_routing_enabled: smart_mic_routing,
        smart_routing_applied: choice.rerouted,
    };

    let result = (|| -> Result<Recorder, String> {
        let cfg = RecorderConfig {
            device_name: choice.device.clone(),
            ..Default::default()
        };
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

            // Install the recorder + show the overlay IMMEDIATELY, before any
            // model work. The first dictation's STT load can take seconds; if we
            // blocked the hotkey loop awaiting it here, the overlay wouldn't
            // appear, the user would mash the key, and the queued Down/Up events
            // would fire in a burst once the load finished — recording several
            // empty dictations. Installing the recorder now also arms the
            // duplicate-Down guard above.
            {
                let mut inner = state.inner.lock().unwrap();
                inner.recorder = Some(recorder);
                inner.recording_meta = Some(recording_meta);
            }
            let _ = state.recording_state_tx.send(true);
            let _ = app.emit("recording:state", true);
            show_overlay(app);
            let _ = app.emit("overlay:phase", "recording");

            // Open the live-streaming session off the hotkey loop: `ensure_stt`
            // lazy-loads the model (slow on first use), so awaiting it inline
            // would block the loop. If the user releases before it's ready,
            // `handle_up` falls back to the one-shot transcribe path; if it only
            // becomes ready after recording already stopped, it's discarded.
            let app3 = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app3.state::<AppState>();
                let (engine, language) = {
                    let inner = state.inner.lock().unwrap();
                    (inner.engine.clone(), inner.settings.language.clone())
                };
                let stt_slot = match engine.ensure_stt().await {
                    Ok(slot) => slot,
                    Err(e) => {
                        tracing::warn!(%e, "handle_down: ensure_stt failed; no streaming session");
                        None
                    }
                };
                let Some(slot) = stt_slot else { return };
                engine.mark_stt_used();
                let lang_opt = if language == "auto" || language.is_empty() {
                    None
                } else {
                    Some(language)
                };
                // Attach only if still recording; otherwise the user already
                // released and the one-shot path is handling this dictation.
                let mut inner = state.inner.lock().unwrap();
                if inner.recorder.is_some() {
                    inner.streaming = Some(crate::commands::inference::spawn_streaming_session(
                        app3.clone(),
                        slot,
                        lang_opt,
                    ));
                } else {
                    tracing::info!(
                        "handle_down: streaming ready but recording already stopped; discarding"
                    );
                }
            });
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
            tracing::info!(
                samples = recording.samples.len(),
                "handle_up: recording stopped"
            );
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
    // Keep the overlay up and switch it to the processing animation. The
    // pipeline hides it once the final text is injected (see run_pipeline's
    // OverlayPhaseGuard).
    let _ = app.emit("overlay:phase", "processing");

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

/// Emits the overlay `done` phase when dropped, so the overlay's exit
/// animation fires on every `run_pipeline` exit path (success, early return,
/// or error) without threading the emit through each branch. The overlay
/// webview hides its own window once the exit animation finishes.
struct OverlayPhaseGuard(AppHandle);
impl Drop for OverlayPhaseGuard {
    fn drop(&mut self) {
        use tauri::Emitter;
        let _ = self.0.emit("overlay:phase", "done");
    }
}

/// Current wall-clock time in epoch milliseconds, for `NewDictation::created_at`.
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Current wall-clock time in epoch seconds, matching `db::style_examples`'
/// caller-owns-the-clock convention (see `style_examples::touch_use`).
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Whether `run_pipeline` should attempt to fetch style examples at all:
/// gated on the `style_learning_enabled` setting and on actually knowing
/// which app to scope retrieval to. Split into a pure function (no `Db`/
/// `AppHandle`) so the on/off gating — including the zero-regression
/// fallback when the setting is off — is unit-testable in isolation.
fn should_fetch_style_examples(style_learning_enabled: bool, target_bundle: Option<&str>) -> bool {
    style_learning_enabled && target_bundle.is_some()
}

/// Splits `top_k` rows into the `(raw, final)` pairs consumed by
/// `lirevo_prompts::build_clean_system_prompt_with_examples` and the ids to
/// `touch_use` once the cleanup request actually goes out.
fn style_examples_from_rows(
    rows: Vec<crate::db::style_examples::StyleExample>,
) -> (Vec<(String, String)>, Vec<i64>) {
    rows.into_iter()
        .map(|r| ((r.raw_text, r.final_text), r.id))
        .unzip()
}

/// Best-effort history insert shared by the failure-path call sites below:
/// runs the insert on a blocking task and, on success, emits `dictation:saved`
/// so History updates live — same as the success-path insert further down,
/// which stays independent to avoid coupling the two paths.
fn spawn_failure_history_insert(
    app: &AppHandle,
    db_arc: Arc<crate::db::Db>,
    entry: crate::db::history::NewDictation,
) {
    let app2 = app.clone();
    tokio::task::spawn_blocking(move || match crate::db::history::insert(&db_arc, &entry) {
        Ok(id) => {
            let summary = crate::db::history::DictationSummary {
                id,
                created_at: entry.created_at,
                preview: crate::db::history::preview_of(&entry.cleaned_text),
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
    });
}

/// Records a terminal STT-stage failure (model not loaded, or the transcribe
/// call itself errored) as a history row, mirroring the success-path fields
/// where available and falling back to empty/`None` for the transcript and
/// inject columns that never ran. No-op when `record_history` is off.
#[allow(clippy::too_many_arguments)]
fn record_stt_failure(
    app: &AppHandle,
    db_arc: Arc<crate::db::Db>,
    record_history: bool,
    error: String,
    t0: std::time::Instant,
    language: &str,
    stt_model: &str,
    llm_model: Option<String>,
    audio_ms: i64,
    recording_meta: Option<&crate::state::RecordingMeta>,
) {
    if !record_history {
        return;
    }
    let elapsed_ms = t0.elapsed().as_millis() as i64;
    let entry = crate::db::history::NewDictation {
        created_at: now_millis(),
        language: Some(language.to_string()),
        stt_model: stt_model.to_string(),
        audio_ms: Some(audio_ms),
        raw_text: String::new(),
        stt_ms: elapsed_ms,
        llm_model,
        cleaned_text: String::new(),
        clean_ms: None,
        cleanup_status: crate::db::history::STT_FAILED.to_string(),
        cleanup_error: Some(error),
        inject_method: "none".to_string(),
        inject_ms: None,
        total_ms: elapsed_ms,
        target_app: None,
        target_bundle: None,
        input_device: recording_meta.map(|m| m.input_device.clone()),
        smart_routing_enabled: recording_meta.is_some_and(|m| m.smart_routing_enabled),
        smart_routing_applied: recording_meta.is_some_and(|m| m.smart_routing_applied),
    };
    spawn_failure_history_insert(app, db_arc, entry);
}

/// Full STT → cleanup → inject pipeline.
///
/// Stages:
///   1. parakeet-cpp STT (blocking → `spawn_blocking`).
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
    // Fires the overlay `done` phase on any exit (success / early return /
    // error), so the overlay stays through STT + cleanup and only animates out
    // once the final text has been handled.
    let _overlay_done = OverlayPhaseGuard(app.clone());

    let t0 = std::time::Instant::now();
    let state = app.state::<AppState>();

    // Audio duration (16 kHz mono) captured before STT consumes `samples`.
    let audio_ms = (samples.len() as i64) * 1000 / 16_000;

    // Snapshot what we need; release the lock before any heavy work so we
    // never hold the std::sync::Mutex across an await.
    let (engine, language, record_history, style_learning_enabled, recording_meta) = {
        let inner = state.inner.lock().unwrap();
        (
            inner.engine.clone(),
            inner.settings.language.clone(),
            inner.settings.record_history,
            inner.settings.style_learning_enabled,
            inner.recording_meta.clone(),
        )
    };
    // `Db` is internally synchronized and lives outside `inner`'s mutex; clone
    // the Arc so it can move into the history-insert task at the end.
    let db_arc = state.db.clone();

    // Model ids for the history row, derived from the engine's live config.
    // Computed up front (rather than after a successful transcription) so
    // STT-failure history rows below can still record real ids. `stt_model`
    // falls back to the default catalog id when unset (matching
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

    // Lazy-load the STT slot for the one-shot fallback path. The streaming
    // worker (if any) already holds its own clone.
    let stt_slot = match engine.ensure_stt().await {
        Ok(Some(slot)) => slot,
        Ok(None) | Err(_) => {
            let _ = app.emit(
                "toast",
                crate::commands::toast("warn", "Transcription model not loaded"),
            );
            record_stt_failure(
                &app,
                db_arc.clone(),
                record_history,
                "Transcription model not loaded".to_string(),
                t0,
                &language,
                &stt_model,
                llm_model.clone(),
                audio_ms,
                recording_meta.as_ref(),
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
                        stt_slot,
                        samples,
                        lang_for_stt,
                    )
                    .await
                    {
                        Ok(t) => t,
                        Err(e) => {
                            let _ = app.emit(
                                "toast",
                                crate::commands::toast(
                                    "error",
                                    format!("Transcription failed: {e}"),
                                ),
                            );
                            record_stt_failure(
                                &app,
                                db_arc.clone(),
                                record_history,
                                format!("Transcription failed: {e}"),
                                t0,
                                &language,
                                &stt_model,
                                llm_model.clone(),
                                audio_ms,
                                recording_meta.as_ref(),
                            );
                            return;
                        }
                    }
                }
            }
        }
        None => match crate::commands::inference::transcribe_samples_async(
            stt_slot,
            samples,
            lang_for_stt,
        )
        .await
        {
            Ok(t) => t,
            Err(e) => {
                let _ = app.emit(
                    "toast",
                    crate::commands::toast("error", format!("Transcription failed: {e}")),
                );
                record_stt_failure(
                    &app,
                    db_arc.clone(),
                    record_history,
                    format!("Transcription failed: {e}"),
                    t0,
                    &language,
                    &stt_model,
                    llm_model.clone(),
                    audio_ms,
                    recording_meta.as_ref(),
                );
                return;
            }
        },
    };
    // Transcription done — mark the STT used so the idle-unload timer is
    // use-relative (not load-relative).
    engine.mark_stt_used();
    let t1 = t0.elapsed();

    // Nothing transcribed (silence / accidental tap): don't inject empty text
    // or record an empty history row. The overlay still dismisses via the
    // OverlayPhaseGuard on return.
    if raw_text.trim().is_empty() {
        tracing::info!("run_pipeline: empty transcript — skipping inject + history");
        return;
    }

    // Resolve the frontmost app once, right after STT, and reuse it both for
    // style-example retrieval below and for the history row further down
    // (rather than re-querying after inject, which could observe a different
    // app if the user switched focus during cleanup latency).
    let (target_app, target_bundle) = os_integration::frontmost_app()
        .map(|a| (a.name, a.bundle_id))
        .unwrap_or((None, None));

    // Style-example retrieval: gated on the setting, scoped to the current
    // app and, when a recipient can be resolved (STYLE-14's allowlisted
    // apps), further scoped by recipient — `top_k` degrades that to
    // recipient -> app -> global on its own. A DB error here must never fail
    // the dictation — degrade to the empty-examples path (byte-identical
    // prompt) and log.
    let mut style_examples_used: Vec<i64> = Vec::new();
    let examples: Vec<(String, String)> = if should_fetch_style_examples(
        style_learning_enabled,
        target_bundle.as_deref(),
    ) {
        let bundle = target_bundle.as_deref().expect("checked above");
        let context_key =
            os_integration::recipient_context_key(bundle, false).map(|ctx| ctx.context_key);
        match crate::db::style_examples::top_k(&db_arc, bundle, context_key.as_deref(), 3) {
            Ok(rows) => {
                let (pairs, ids) = style_examples_from_rows(rows);
                style_examples_used = ids;
                pairs
            }
            Err(e) => {
                tracing::warn!(
                        ?e,
                        bundle,
                        "run_pipeline: style example retrieval failed — falling back to no-examples prompt"
                    );
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

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
            system: Some(lirevo_prompts::build_clean_system_prompt_with_examples(
                &language, &examples,
            )),
            history: vec![],
            user: raw_text.clone(),
            temperature: 0.2,
            max_tokens: 2048,
            stop: vec![],
            ..Default::default()
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

    // The examples were actually spliced into a request only when a cleanup
    // call was really made (Applied/Failed); in the Skipped (STT-only) case
    // no LLM request went out, so don't record a use.
    if cleanup_ran {
        let now = now_secs();
        for id in &style_examples_used {
            if let Err(e) = crate::db::style_examples::touch_use(&db_arc, *id, now) {
                tracing::warn!(?e, id, "run_pipeline: touch_use failed (non-fatal)");
            }
        }
    }

    // 3. Inject (graceful degrade to clipboard).
    match os_integration::Injector::new().inject(&cleaned) {
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
            // `target_app`/`target_bundle` were resolved once, right after STT
            // (see above), and reused here rather than re-queried post-inject.
            if record_history {
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
                    input_device: recording_meta.as_ref().map(|m| m.input_device.clone()),
                    smart_routing_enabled: recording_meta
                        .as_ref()
                        .is_some_and(|m| m.smart_routing_enabled),
                    smart_routing_applied: recording_meta
                        .as_ref()
                        .is_some_and(|m| m.smart_routing_applied),
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

            // Record this failed dictation. STT + cleanup already ran (their
            // text/timings are known), only inject fell through; best-effort,
            // same as the success-path insert above. `target_app`/`target_bundle`
            // are the same values resolved once right after STT (see above).
            if record_history {
                let entry = crate::db::history::NewDictation {
                    created_at: now_millis(),
                    language: Some(language.clone()),
                    stt_model,
                    audio_ms: Some(audio_ms),
                    raw_text: raw_text.clone(),
                    stt_ms: t1.as_millis() as i64,
                    llm_model,
                    cleaned_text: cleaned.clone(),
                    clean_ms,
                    cleanup_status: crate::db::history::INJECT_FAILED.to_string(),
                    cleanup_error: Some(format!("Inject failed: {e}")),
                    inject_method: "failed".to_string(),
                    inject_ms: None,
                    total_ms: t0.elapsed().as_millis() as i64,
                    target_app,
                    target_bundle,
                    input_device: recording_meta.as_ref().map(|m| m.input_device.clone()),
                    smart_routing_enabled: recording_meta
                        .as_ref()
                        .is_some_and(|m| m.smart_routing_enabled),
                    smart_routing_applied: recording_meta
                        .as_ref()
                        .is_some_and(|m| m.smart_routing_applied),
                };
                spawn_failure_history_insert(&app, db_arc.clone(), entry);
            }
        }
    }

    // Record the last-dictation time on the engine. Reserved for future
    // preload / idle-unload refinement — `lifecycle_decision` does not yet
    // consume `last_dictation` (it takes it as `_last_dictation`).
    engine.mark_dictation();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reinstall_guard_rejects_while_recording() {
        assert!(matches!(
            reject_if_recording(true),
            Err(AppError::HotkeyBusy)
        ));
    }

    #[test]
    fn reinstall_guard_allows_when_idle() {
        assert!(reject_if_recording(false).is_ok());
    }

    #[test]
    fn style_examples_not_fetched_when_setting_disabled() {
        assert!(!should_fetch_style_examples(false, Some("com.apple.mail")));
    }

    #[test]
    fn style_examples_not_fetched_without_a_target_bundle() {
        assert!(!should_fetch_style_examples(true, None));
    }

    #[test]
    fn style_examples_fetched_when_enabled_with_a_target_bundle() {
        assert!(should_fetch_style_examples(true, Some("com.apple.mail")));
    }

    /// Zero-regression guarantee at the call site: whenever
    /// `should_fetch_style_examples` says "don't fetch" (setting off, or no
    /// resolved app), the examples list stays empty and the system prompt
    /// actually sent to the LLM is byte-identical to the pre-STYLE-4 prompt.
    #[test]
    fn style_examples_disabled_setting_yields_byte_identical_prompt() {
        for target_bundle in [None, Some("com.apple.mail")] {
            let examples: Vec<(String, String)> =
                if should_fetch_style_examples(false, target_bundle) {
                    panic!("should_fetch_style_examples must be false when the setting is off");
                } else {
                    Vec::new()
                };
            assert_eq!(
                lirevo_prompts::build_clean_system_prompt_with_examples("en", &examples),
                lirevo_prompts::build_clean_system_prompt("en")
            );
        }
    }

    #[test]
    fn style_examples_from_rows_maps_pairs_and_ids() {
        let rows = vec![
            crate::db::style_examples::StyleExample {
                id: 1,
                dictation_id: None,
                context_key: None,
                target_bundle: Some("com.apple.mail".into()),
                raw_text: "raw one".into(),
                final_text: "final one".into(),
                edit_distance_ratio: None,
                source: "manual_pin".into(),
                pinned: true,
                use_count: 0,
                last_used_at: None,
                created_at: 1,
            },
            crate::db::style_examples::StyleExample {
                id: 2,
                dictation_id: None,
                context_key: None,
                target_bundle: Some("com.apple.mail".into()),
                raw_text: "raw two".into(),
                final_text: "final two".into(),
                edit_distance_ratio: None,
                source: "manual_pin".into(),
                pinned: false,
                use_count: 3,
                last_used_at: Some(100),
                created_at: 2,
            },
        ];

        let (pairs, ids) = style_examples_from_rows(rows);
        assert_eq!(
            pairs,
            vec![
                ("raw one".to_string(), "final one".to_string()),
                ("raw two".to_string(), "final two".to_string()),
            ]
        );
        assert_eq!(ids, vec![1, 2]);
    }
}
