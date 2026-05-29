use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{mpsc, oneshot};

use audiopipe::{PartialTranscript, TranscribeOptions};
use inference_core::ChatRequest;

use crate::state::{ModelState, SttSlot};
use crate::stt;
use crate::{AppError, AppState};

/// Hard ceilings on Tauri-command inputs from the webview. The renderer is
/// the only caller, and at 200 MB PCM16 / 100 KB text these caps are far
/// above any legitimate dictation but well below what would let a
/// compromised webview exhaust process RAM with a single `invoke()`.
const MAX_TRANSCRIBE_WAV_BYTES: usize = 200 * 1024 * 1024;
const MAX_CLEAN_TEXT_BYTES: usize = 100 * 1024;

/// WAV-bytes-in / text-out command used by the renderer for manual dictation
/// and by `commands::dictation::manual_dictate`. Decodes the WAV to 16 kHz
/// mono f32 on the audio worker (which audiopipe owns internally — we just
/// hand the file off via the raw-sample-rate path) and runs audiopipe
/// inference on the blocking pool.
#[tauri::command]
pub async fn transcribe(
    state: State<'_, AppState>,
    wav: Vec<u8>,
    language: Option<String>,
) -> Result<String, AppError> {
    if wav.len() > MAX_TRANSCRIBE_WAV_BYTES {
        return Err(AppError::Internal(format!(
            "transcribe: wav too large ({} > {} bytes)",
            wav.len(),
            MAX_TRANSCRIBE_WAV_BYTES
        )));
    }
    let engine = {
        let inner = state.inner.lock().unwrap();
        inner.engine.clone()
    };
    let stt_slot = engine
        .ensure_stt()
        .await
        .map_err(AppError::Internal)?
        .ok_or(AppError::SttNotLoaded)?;
    engine.mark_stt_used();
    let lang_opt = normalize_language(language);

    tokio::task::spawn_blocking(move || -> Result<String, AppError> {
        let (samples, sample_rate) = decode_wav_to_mono_f32(&wav)?;
        let mut guard = stt_slot.blocking_lock();
        let result = guard
            .transcribe(
                &samples,
                sample_rate,
                TranscribeOptions { language: lang_opt, word_timestamps: false },
            )
            .map_err(AppError::from)?;
        Ok(result.text.trim().to_string())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

#[tauri::command]
pub async fn clean(
    state: State<'_, AppState>,
    text: String,
    language: String,
) -> Result<String, AppError> {
    if text.len() > MAX_CLEAN_TEXT_BYTES {
        return Err(AppError::Internal(format!(
            "clean: text too large ({} > {} bytes)",
            text.len(),
            MAX_CLEAN_TEXT_BYTES
        )));
    }
    let engine = {
        let inner = state.inner.lock().unwrap();
        inner.engine.clone()
    };
    let req = ChatRequest {
        system: Some(lirevo_prompts::build_clean_system_prompt(&language)),
        history: vec![],
        user: text.clone(),
        temperature: 0.2,
        max_tokens: 2048,
        stop: vec![],
    };
    match engine.chat(req).await? {
        Some(resp) => Ok(resp.text),
        // No cleanup model configured: return the text unchanged so the
        // command stays usable in STT-only mode.
        None => Ok(text),
    }
}

/// Snapshot of the current model loading state. The frontend needs this at
/// mount time because it subscribes to `model:state` events AFTER the layout
/// mounts, but the backend's startup load_models task fires its Loading /
/// Ready event right at app launch — so without an initial fetch the
/// frontend can miss the first event and stay stuck on its `idle` default
/// even after models are loaded.
#[tauri::command]
pub fn get_model_state(state: State<'_, AppState>) -> Result<ModelState, AppError> {
    Ok(state.current_model_state())
}

/// Refresh the Engine config from current settings, eagerly load both
/// backends, and emit the resulting `ModelState` for the UI.
///
/// Since M5.3 the Engine owns the model lifecycle (lazy load +
/// resource-aware unload). `load_models` is the eager-load entry point used
/// at startup and after a settings change: it pushes the latest config into
/// the engine, then drives `ensure_stt` + `ensure_llm` so the UI gets a
/// Loading→Ready signal up front (rather than only loading on first use).
///
/// Cancellation: a newer load (e.g. user changes settings) increments the
/// token; the stale invocation skips its ModelState emission.
///
/// STT: the catalog id comes from `settings.stt_model_id`, falling back to
/// [`stt::catalog::default_model_id`]. `ensure_stt` prefers the HF cache; on
/// a cache miss it spawns a background download and returns `None` — we
/// surface a `ModelState::Loading` until the next reload picks the cached
/// weights up.
pub async fn load_models(app: &AppHandle, state: State<'_, AppState>) {
    let (engine, stt_model_id, llm_path, ctx_size, keep_warm, token) = {
        let mut inner = state.inner.lock().unwrap();
        inner.current_load_token += 1;
        let token = inner.current_load_token;
        let stt_model_id = inner
            .settings
            .stt_model_id
            .clone()
            .unwrap_or_else(|| stt::catalog::default_model_id().to_string());
        (
            inner.engine.clone(),
            stt_model_id,
            inner.settings.llm_model_path.clone(),
            inner.settings.llm_ctx_size,
            inner.settings.keep_models_warm,
            token,
        )
    };

    // Runtime existence check: settings migration clears stale paths at
    // startup, but the file can disappear mid-session (model manager remove,
    // user deleted the .gguf manually). Treating a missing file as "not
    // configured" lets the dictation pipeline keep running in STT-only mode
    // instead of bubbling a confusing load error to the UI.
    let effective_llm_path = llm_path.and_then(|p| {
        if p.exists() {
            Some(p)
        } else {
            tracing::warn!(
                path = %p.display(),
                "configured LLM path is missing on disk; treating as not configured"
            );
            None
        }
    });

    // Push the latest settings into the engine so its lazy/lifecycle paths see
    // the right config (the catalog default for STT, the existence-checked LLM
    // path).
    let stt_id_for_engine = if stt_model_id.is_empty() {
        None
    } else {
        Some(stt_model_id.clone())
    };
    let new_cfg = crate::engine::EngineConfig {
        llm_model_path: effective_llm_path.clone(),
        llm_ctx_size: ctx_size,
        stt_model_id: stt_id_for_engine,
        keep_warm,
    };

    // Settings-change reload: `ensure_llm` / `ensure_stt` short-circuit when a
    // backend is already Loaded and never compare the loaded model against the
    // new config — so a model swap from Settings would otherwise keep serving
    // the stale backend. Diff the engine's current config against the new one
    // and unload any affected slot so the `ensure_*` calls below reload fresh.
    //
    // At startup this is a no-op: the slots are Unloaded (so `unload_*` does
    // nothing) and `ensure_*` performs the initial load. The configs can differ
    // even at startup (AppState::new uses the raw settings path, here we
    // existence-check it and fall STT back to the catalog default), but an
    // unload against an already-Unloaded slot is harmless.
    let old_cfg = engine.current_config();
    let llm_changed = old_cfg.llm_model_path != new_cfg.llm_model_path
        || old_cfg.llm_ctx_size != new_cfg.llm_ctx_size;
    let stt_changed = old_cfg.stt_model_id != new_cfg.stt_model_id;
    engine.update_config(new_cfg.clone());
    if llm_changed {
        engine
            .unload_llm(crate::engine::UnloadReason::ConfigChanged)
            .await;
    }
    if stt_changed {
        engine
            .unload_stt(crate::engine::UnloadReason::ConfigChanged)
            .await;
    }

    if effective_llm_path.is_none() && stt_model_id.is_empty() {
        let inner = state.inner.lock().unwrap();
        if inner.current_load_token == token {
            drop(inner);
            state.set_model_state(app, ModelState::Idle);
        } else {
            tracing::info!("stale load skipped (token mismatch)");
        }
        return;
    }

    {
        let inner = state.inner.lock().unwrap();
        if inner.current_load_token == token {
            drop(inner);
            state.set_model_state(
                app,
                ModelState::Loading {
                    stt: !stt_model_id.is_empty(),
                    llama: effective_llm_path.is_some(),
                },
            );
        }
    }

    // Drive eager loads through the engine. `ensure_stt` returns Ok(None) when
    // the weights are still downloading; `ensure_llm` returns Ok(None) when no
    // path is configured (graceful STT-only mode).
    //
    // Eager-vs-lazy deviation (deliberate): the lifecycle design preloads the
    // LLM only when the profile is Balanced/Performance and on AC. Here we call
    // `ensure_llm` unconditionally at startup because we need a definitive
    // Ready/Error `ModelState` for the UI up front, and because eagerly loading
    // is the only way to detect a broken LLM path (corrupt GGUF, unsupported
    // arch) so the auto-recover block below can clear it. The energy policy is
    // still honored after this first load: `lifecycle_loop` idle-unloads the
    // LLM once the active profile's `llm_idle_unload` window elapses.
    let stt_outcome = if stt_model_id.is_empty() {
        Ok(None)
    } else {
        engine.ensure_stt().await
    };
    let llama_outcome = if effective_llm_path.is_some() {
        engine.ensure_llm().await
    } else {
        Ok(None)
    };

    let mut stt_ready = false;
    let mut stt_err: Option<String> = None;
    let mut stt_downloading = false;
    match stt_outcome {
        Ok(Some(_)) => stt_ready = true,
        Ok(None) => {
            if !stt_model_id.is_empty() {
                tracing::info!(
                    model = %stt_model_id,
                    "STT weights downloading in background; will be ready after the next reload"
                );
                stt_downloading = true;
            }
        }
        Err(e) => {
            tracing::warn!(%e, "STT load failed");
            stt_err = Some(e);
        }
    }

    let mut llama_ready = false;
    let mut llama_err: Option<String> = None;
    match llama_outcome {
        Ok(Some(_)) => llama_ready = true,
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(?e, "llama load failed");
            llama_err = Some(e.to_string());
        }
    }

    // Load-token guard: bail before touching settings/engine if a newer
    // reload started while we were loading. Must precede the auto-recover
    // block below — otherwise a stale load could wipe a freshly-set good
    // `llm_model_path` that the newer reload just persisted.
    {
        let inner = state.inner.lock().unwrap();
        if inner.current_load_token != token {
            tracing::info!("stale load result discarded (token mismatch)");
            return;
        }
    }

    // Auto-recover from an unloadable LLM: if the cleanup model failed to
    // load (corrupted GGUF, unsupported architecture, missing file, etc.),
    // clear the stale path so the dictation pipeline drops cleanly into
    // STT-only mode and the user gets a clear "pick a different model"
    // signal instead of an infinite "Cleanup loading…" pill.
    if llama_err.is_some() {
        let mut inner = state.inner.lock().unwrap();
        if inner.settings.llm_model_path.is_some() {
            tracing::warn!("clearing settings.llm_model_path after load failure");
            inner.settings.llm_model_path = None;
            if let Err(persist_err) = inner.settings.persist(app) {
                tracing::warn!(?persist_err, "failed to persist cleared llm_model_path");
            }
            // Keep the engine config in sync so a later lazy ensure_llm does
            // not re-attempt the broken path.
            let cfg = crate::engine::EngineConfig {
                llm_model_path: None,
                llm_ctx_size: ctx_size,
                stt_model_id: if stt_model_id.is_empty() {
                    None
                } else {
                    Some(stt_model_id.clone())
                },
                keep_warm,
            };
            drop(inner);
            engine.update_config(cfg);
            let _ = app.emit(
                "toast",
                crate::commands::toast(
                    "warn",
                    "Cleanup model couldn't load; cleared selection. Pick a supported model in Settings.",
                ),
            );
        }
    }

    let next = if !stt_ready && !llama_ready {
        if stt_downloading {
            ModelState::Loading {
                stt: true,
                llama: false,
            }
        } else {
            let parts: Vec<String> = [
                stt_err.as_ref().map(|e| format!("STT: {e}")),
                llama_err.as_ref().map(|e| format!("LLM: {e}")),
            ]
            .into_iter()
            .flatten()
            .collect();
            ModelState::Error {
                reason: if parts.is_empty() {
                    "models failed to load".into()
                } else {
                    parts.join(" — ")
                },
            }
        }
    } else if stt_downloading {
        ModelState::Loading {
            stt: true,
            llama: llama_ready,
        }
    } else {
        ModelState::Ready {
            stt: stt_ready,
            llama: llama_ready,
        }
    };
    // The token guard above already returned early on a stale load, so this
    // emission is the live reload's to own.
    state.set_model_state(app, next);

    // Register the macOS shutdown-safety atexit handler now that GPU
    // backends (Metal via audiopipe / llama-cpp-2, plus CoreML when
    // enabled) may have been touched. This must happen AFTER the loads
    // above so our atexit is registered later than ggml's `__cxa_atexit`
    // and therefore runs earlier in the LIFO finalize order — see
    // `register_quit_safety_atexit` for the full reasoning. Called on
    // every load_models invocation (initial and settings-triggered
    // reloads) so the ordering invariant holds even when the user
    // switches model selections mid-session.
    //
    // Skipped on the no-models-configured early-return above: if nothing
    // is configured, no Metal context exists, no destructor is registered,
    // and there is nothing to swallow on shutdown.
    crate::register_quit_safety_atexit();

    if keep_warm && (stt_ready || llama_ready) {
        let engine_for_warmup = engine.clone();
        tokio::spawn(async move {
            warm_up(&engine_for_warmup).await;
        });
    }
}

/// One-shot dummy inference per loaded backend to force GPU kernel
/// compilation + KV cache allocation up front. Errors are swallowed: a
/// warm-up failure is not user-visible and the first real dictation
/// will surface any genuine problem through its normal error path.
///
/// Each call logs an `elapsed_ms` field so the log file is a quick
/// sanity check that the warm-up is doing real work — a sub-50ms
/// elapsed means the backend skipped the inference (e.g. silence was
/// rejected too early), while a hundreds-of-ms elapsed confirms the
/// GPU pipeline actually built kernels and allocated buffers.
///
/// Exposed at crate visibility so `commands::settings::update_settings`
/// can also fire it when the user toggles `keep_models_warm` from off
/// to on while models are already loaded — without this they'd have
/// to either trigger a reload or restart the app to see the benefit.
///
/// Drives the warm-up through the Engine: `ensure_stt` (lazy-loads if needed)
/// for a silent transcribe, and a 1-token `chat` for the LLM. Both are
/// best-effort — a warm-up failure is not user-visible.
pub(crate) async fn warm_up(engine: &Arc<crate::engine::Engine>) {
    match engine.ensure_stt().await {
        Ok(Some(slot)) => {
            // 1 second of silence at 16 kHz mono — long enough for any of
            // the audiopipe backends to exercise their full encode/decode
            // path without producing real text.
            let warm = tokio::task::spawn_blocking(move || {
                let silence = vec![0.0_f32; 16_000];
                let t0 = std::time::Instant::now();
                let elapsed_ms = || u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
                let mut guard = slot.blocking_lock();
                match guard.transcribe(
                    &silence,
                    16_000,
                    TranscribeOptions {
                        language: Some("en".to_string()),
                        word_timestamps: false,
                    },
                ) {
                    Ok(_) => tracing::info!(elapsed_ms = elapsed_ms(), "STT warm-up completed"),
                    Err(e) => {
                        tracing::warn!(?e, elapsed_ms = elapsed_ms(), "STT warm-up failed (non-fatal)");
                    }
                }
            })
            .await;
            if let Err(e) = warm {
                tracing::warn!(%e, "STT warm-up join error (non-fatal)");
            }
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(%e, "STT warm-up ensure failed (non-fatal)"),
    }

    let req = ChatRequest {
        system: None,
        history: vec![],
        user: "a".into(),
        temperature: 0.0,
        max_tokens: 1,
        stop: vec![],
    };
    let t0 = std::time::Instant::now();
    let elapsed_ms = || u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
    match engine.chat(req).await {
        Ok(_) => tracing::info!(elapsed_ms = elapsed_ms(), "llama warm-up completed"),
        Err(e) => tracing::warn!(?e, elapsed_ms = elapsed_ms(), "llama warm-up failed (non-fatal)"),
    }
}

/// Hot-path entry used by the hotkey pipeline: lock the slot, run
/// audiopipe inference on the blocking pool, return trimmed text.
///
/// The recorder already produces 16 kHz mono f32, so the caller passes
/// `samples` directly without a WAV round-trip. The lock is held for the
/// duration of the inference call — the dictation pipeline is the only
/// concurrent caller against a single backend, so blocking on contention
/// here is the right behaviour (it just queues hotkey presses, which is
/// what the user expects).
pub async fn transcribe_samples_async(
    slot: SttSlot,
    samples: Vec<f32>,
    language: Option<String>,
) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || -> Result<String, AppError> {
        let mut guard = slot.blocking_lock();
        let result = guard
            .transcribe(
                &samples,
                16_000,
                TranscribeOptions { language, word_timestamps: false },
            )
            .map_err(AppError::from)?;
        Ok(result.text.trim().to_string())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

/// Handle returned by [`spawn_streaming_session`]. The hotkey coordinator
/// stores this for the duration of a dictation and uses it to (a) signal
/// "stop" with the authoritative final 16 kHz samples, and (b) collect
/// the cumulative streamed transcript back from the worker.
///
/// `result_rx` resolves to:
///   * `Some(text)` — streaming produced a full transcript (skip the
///     one-shot stage).
///   * `None`      — engine returned `Error::NotSupported`, the session
///     errored mid-stream, or the loader couldn't acquire the slot;
///     caller should fall back to the one-shot transcribe path.
pub struct StreamingHandle {
    pub stop_tx: oneshot::Sender<Vec<f32>>,
    pub result_rx: oneshot::Receiver<Option<String>>,
}

/// Cadence of the streaming push loop. Each tick: peek the recorder for
/// new 16 kHz samples, push them into the audiopipe session, emit a
/// partial-transcript event.
const STREAM_TICK: Duration = Duration::from_millis(400);

/// Tauri event name for partial-transcript updates pushed by the
/// streaming worker. Payload is [`PartialTranscriptEvent`].
const PARTIAL_TRANSCRIPT_EVENT: &str = "recording:partial_transcript";

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PartialTranscriptEvent {
    text: String,
    delta: String,
    is_final: bool,
}

impl PartialTranscriptEvent {
    fn from_partial(p: &PartialTranscript) -> Self {
        Self {
            text: p.text.clone(),
            delta: p.delta.clone(),
            is_final: p.is_final,
        }
    }

    fn final_text(text: &str) -> Self {
        Self {
            text: text.to_string(),
            delta: String::new(),
            is_final: true,
        }
    }
}

/// Snapshot of the recorder's resampled buffer past `cursor`, taken
/// without stopping the audio stream. Returns the new tail and the
/// updated cursor (in 16 kHz output samples).
fn peek_recorder_tail(
    state: &AppState,
    cursor: usize,
) -> Option<(Vec<f32>, usize)> {
    let inner = state.inner.lock().ok()?;
    let rec = inner.recorder.as_ref()?;
    rec.peek_resampled_since(cursor).ok()
}

/// Spawn the live streaming worker for the current dictation.
///
/// The worker opens an audiopipe `StreamSession` (graceful degrade on
/// `NotSupported`), then loops every [`STREAM_TICK`]: peek the recorder
/// for new 16 kHz samples, push them into the session, emit a
/// `recording:partial_transcript` Tauri event. When `handle_up` sends
/// the authoritative final samples on `stop_tx`, the worker pushes the
/// remaining tail, calls `session.finish()`, emits one `is_final` event,
/// and resolves `result_rx` with the cumulative text.
///
/// Concurrency: the worker runs on `tokio::task::spawn_blocking` and
/// holds the STT slot's `blocking_lock` for the entire dictation —
/// matching the existing one-shot lock-for-duration pattern. Each
/// `session.push` is synchronous and runs in the same blocking thread.
pub fn spawn_streaming_session(
    app: AppHandle,
    slot: SttSlot,
    language: Option<String>,
) -> StreamingHandle {
    let (stop_tx, stop_rx) = oneshot::channel::<Vec<f32>>();
    let (result_tx, result_rx) = oneshot::channel::<Option<String>>();
    // mpsc with capacity 1 lets the tick-driver coalesce wake-ups against
    // the (rare) stop signal without dropping the final samples.
    let (tick_tx, tick_rx) = mpsc::channel::<TickEvent>(2);

    // Pacing driver: every STREAM_TICK push a Tick; when stop_rx fires
    // forward the final samples as a Stop event then exit.
    let tick_tx_drive = tick_tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(STREAM_TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Discard the immediate first tick — we want the first peek to
        // happen STREAM_TICK after handle_down, not instantly.
        interval.tick().await;
        tokio::pin!(stop_rx);
        loop {
            tokio::select! {
                biased;
                final_samples = &mut stop_rx => {
                    let payload = final_samples.unwrap_or_default();
                    let _ = tick_tx_drive.send(TickEvent::Stop(payload)).await;
                    return;
                }
                _ = interval.tick() => {
                    if tick_tx_drive.send(TickEvent::Tick).await.is_err() {
                        return;
                    }
                }
            }
        }
    });

    // Blocking worker: owns the session + slot lock for the dictation.
    tokio::task::spawn_blocking(move || {
        run_streaming_worker(app, slot, language, tick_rx, result_tx);
    });

    StreamingHandle { stop_tx, result_rx }
}

enum TickEvent {
    Tick,
    Stop(Vec<f32>),
}

fn run_streaming_worker(
    app: AppHandle,
    slot: SttSlot,
    language: Option<String>,
    mut tick_rx: mpsc::Receiver<TickEvent>,
    result_tx: oneshot::Sender<Option<String>>,
) {
    let state = app.state::<AppState>();
    let mut guard = slot.blocking_lock();
    let opts = TranscribeOptions { language, word_timestamps: false };

    let mut session = match guard.transcribe_stream(16_000, opts) {
        Ok(s) => s,
        Err(audiopipe::Error::NotSupported) => {
            tracing::info!(
                "streaming worker: engine reports NotSupported — falling back to one-shot path"
            );
            let _ = result_tx.send(None);
            return;
        }
        Err(e) => {
            tracing::warn!(?e, "streaming worker: transcribe_stream open failed");
            let _ = result_tx.send(None);
            return;
        }
    };

    let mut cursor: usize = 0;
    let mut last_text = String::new();
    let mut had_error = false;

    while let Some(event) = tick_rx.blocking_recv() {
        match event {
            TickEvent::Tick => {
                let Some((tail, new_cursor)) = peek_recorder_tail(&state, cursor) else {
                    // Recorder gone (handle_up already took it) — wait for
                    // the Stop event to arrive with the final samples.
                    continue;
                };
                cursor = new_cursor;
                if tail.is_empty() {
                    continue;
                }
                match session.push(&tail) {
                    Ok(partial) => {
                        last_text = partial.text.clone();
                        let _ = app.emit(
                            PARTIAL_TRANSCRIPT_EVENT,
                            PartialTranscriptEvent::from_partial(&partial),
                        );
                    }
                    Err(e) => {
                        tracing::warn!(?e, "streaming worker: session.push failed");
                        // Bail to the fallback path below; the caller will
                        // run the one-shot transcribe instead.
                        break;
                    }
                }
            }
            TickEvent::Stop(final_samples) => {
                let tail_start = cursor.min(final_samples.len());
                let tail = &final_samples[tail_start..];
                if !tail.is_empty() {
                    if let Err(e) = session.push(tail) {
                        tracing::warn!(?e, "streaming worker: final session.push failed");
                        had_error = true;
                    }
                }
                match session.finish() {
                    Ok(result) => {
                        let text = result.text.trim().to_string();
                        let _ = app.emit(
                            PARTIAL_TRANSCRIPT_EVENT,
                            PartialTranscriptEvent::final_text(&text),
                        );
                        let _ = result_tx.send(if had_error { None } else { Some(text) });
                        return;
                    }
                    Err(e) => {
                        tracing::warn!(?e, "streaming worker: session.finish failed");
                        let _ = result_tx.send(None);
                        return;
                    }
                }
            }
        }
    }

    // Channel closed without a Stop event (driver task dropped early).
    // Emit a final event with whatever cumulative text we have and bail.
    let _ = app.emit(
        PARTIAL_TRANSCRIPT_EVENT,
        PartialTranscriptEvent::final_text(&last_text),
    );
    let _ = result_tx.send(None);
}

/// Coerce a frontend-supplied language string into the `Option<String>`
/// audiopipe expects (`None` = auto-detect). Empty string and the sentinel
/// `"auto"` both map to auto-detect; everything else passes through.
fn normalize_language(lang: Option<String>) -> Option<String> {
    match lang {
        Some(s) if s.is_empty() || s == "auto" => None,
        other => other,
    }
}

/// Decode a WAV byte slice into `(mono_f32_samples, sample_rate_hz)`.
/// audiopipe resamples internally via `transcribe_with_sample_rate`, so we
/// only need to surface the source rate and mono-mix any stereo input.
#[allow(clippy::cast_precision_loss)]
fn decode_wav_to_mono_f32(bytes: &[u8]) -> Result<(Vec<f32>, u32), AppError> {
    use hound::SampleFormat;
    let cursor = std::io::Cursor::new(bytes);
    let reader = hound::WavReader::new(cursor)
        .map_err(|e| AppError::Inference(format!("wav decode: {e}")))?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    if channels == 0 || channels > 2 {
        return Err(AppError::Inference(format!(
            "wav decode: unsupported channel count {channels}"
        )));
    }
    let interleaved: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Inference(format!("wav decode: {e}")))?,
        SampleFormat::Int => {
            let max = match spec.bits_per_sample {
                8 => f32::from(i8::MAX),
                16 => f32::from(i16::MAX),
                24 => 8_388_607.0,
                32 => i32::MAX as f32,
                bits => {
                    return Err(AppError::Inference(format!(
                        "wav decode: unsupported int bits {bits}"
                    )))
                }
            };
            reader
                .into_samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AppError::Inference(format!("wav decode: {e}")))?
        }
    };
    let mono = if channels == 1 {
        interleaved
    } else {
        let mut out = Vec::with_capacity(interleaved.len() / channels);
        for frame in interleaved.chunks_exact(channels) {
            let sum: f32 = frame.iter().sum();
            out.push(sum / channels as f32);
        }
        out
    };
    Ok((mono, spec.sample_rate))
}
