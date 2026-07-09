use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{mpsc, oneshot};

use inference_core::ChatRequest;
use parakeet_cpp::common_prefix_len;

use crate::stt::SttOptions;

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
/// mono f32 and runs STT inference on the blocking pool.
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
            .transcribe(&samples, sample_rate, &SttOptions { language: lang_opt })
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
        ..Default::default()
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

/// Wire type for [`get_active_backend`]: the compute backend each engine
/// resolved to, plus a convenience GPU flag the UI can render without
/// re-implementing the "is this CPU?" check. `*_is_gpu` is true when the
/// backend string is non-empty and not (case-insensitively) `"cpu"`.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveBackendInfo {
    pub stt: String,
    pub llm: String,
    pub stt_is_gpu: bool,
    pub llm_is_gpu: bool,
}

/// A backend is "GPU" when it resolved to a real, non-CPU compute backend
/// (e.g. ggml's `"MTL0"` / `"Metal"`). Empty (no model loaded yet) and any
/// case spelling of `"cpu"` count as not-GPU.
fn is_gpu_backend(name: &str) -> bool {
    !name.is_empty() && !name.eq_ignore_ascii_case("cpu")
}

/// Report the active STT + LLM compute backends to the frontend (consumed by
/// Settings → Engine).
///
/// The backends are resolved lazily — the ggml backend is created on the
/// first model load, so `Engine::active_backends()` is `None` until then. In
/// that not-ready case we return empty strings with `*_is_gpu = false` rather
/// than erroring, so the UI can render a neutral "resolving" state.
#[tauri::command]
pub fn get_active_backend(state: State<'_, AppState>) -> Result<ActiveBackendInfo, AppError> {
    let engine = {
        let inner = state.inner.lock().unwrap();
        inner.engine.clone()
    };
    let info = match engine.active_backends() {
        Some(active) => ActiveBackendInfo {
            stt_is_gpu: is_gpu_backend(&active.stt),
            llm_is_gpu: is_gpu_backend(&active.llm),
            stt: active.stt,
            llm: active.llm,
        },
        None => ActiveBackendInfo {
            stt: String::new(),
            llm: String::new(),
            stt_is_gpu: false,
            llm_is_gpu: false,
        },
    };
    Ok(info)
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
/// STT is a single fixed model (id from [`stt::catalog::default_model_id`]) —
/// there is no user selection to read. `ensure_stt` loads the GGUF from
/// `models_dir` (see `stt::gguf_path`); on a cache miss it returns
/// `Ok(None)` without spawning a download — we surface a
/// `ModelState::Loading` until the file shows up (via the wizard or a
/// Settings → Models → Repair download) and the next reload picks it up.
pub async fn load_models(app: &AppHandle, state: State<'_, AppState>) {
    let (engine, ctx_size, onboarding_complete, token) = {
        let mut inner = state.inner.lock().unwrap();
        inner.current_load_token += 1;
        let token = inner.current_load_token;
        (
            inner.engine.clone(),
            inner.settings.llm_ctx_size,
            inner.settings.onboarding_complete,
            token,
        )
    };
    // STT is a single fixed model; there is no user selection to read.
    let stt_model_id = stt::catalog::default_model_id().to_string();

    // During onboarding the wizard owns model download + selection: it triggers
    // both downloads explicitly and shows real progress. Skip the eager
    // load/download here so we don't race the wizard's downloads on a fresh
    // cache (a second, progress-less download would fight hf_hub's blob lock).
    // The first post-onboarding launch loads from the now-populated cache.
    if !onboarding_complete {
        let inner = state.inner.lock().unwrap();
        if inner.current_load_token == token {
            drop(inner);
            state.set_model_state(app, ModelState::Idle);
        }
        return;
    }

    // Thin-fetch of the GPU backend module on first run (Linux). DORMANT on
    // macOS (Metal is bundled → returns immediately, no network) and on Windows
    // (static engines → nothing to fetch). On Linux wanting CUDA/Vulkan it
    // downloads + verifies + places the module and wires the engines at it,
    // BEFORE the ensure_stt/ensure_llm below create the ggml backends. A failure
    // here is non-fatal: the engines fall back to the bundled CPU module.
    let manifest_url = crate::engine::backend_manifest_url();
    crate::engine::BackendManager::ensure_fetched_backends(app, manifest_url).await;

    // Cleanup runs only when the fixed model file is present on disk. There is
    // no persisted path any more: we derive it from the shipped catalog and
    // gate on existence. A missing file means graceful STT-only mode until the
    // user re-downloads it (Settings → Models → Repair) or onboarding fetches it.
    let effective_llm_path = crate::models::effective_llm_path(app);

    // Push the latest settings into the engine so its lazy/lifecycle paths see
    // the right config (the catalog default for STT, the existence-checked LLM
    // path).
    let new_cfg = crate::engine::EngineConfig {
        llm_model_path: effective_llm_path.clone(),
        llm_ctx_size: ctx_size,
        stt_model_id: Some(stt_model_id.clone()),
    };

    // Settings-change reload: `ensure_llm` / `ensure_stt` short-circuit when a
    // backend is already Loaded and never compare the loaded model against the
    // new config — so a model swap from Settings would otherwise keep serving
    // the stale backend. Diff the engine's current config against the new one
    // and unload any affected slot so the `ensure_*` calls below reload fresh.
    //
    // At startup this is a no-op: the slots are Unloaded (so `unload_*` does
    // nothing) and `ensure_*` performs the initial load. `AppState::new`
    // resolves the same fixed-path-plus-existence-check config, so the two
    // rarely differ — but an unload against an already-Unloaded slot is
    // harmless if the on-disk file changed between the two checks.
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

    {
        let inner = state.inner.lock().unwrap();
        if inner.current_load_token == token {
            drop(inner);
            state.set_model_state(
                app,
                ModelState::Loading {
                    stt: true,
                    llama: effective_llm_path.is_some(),
                },
            );
        }
    }

    // Drive eager loads through the engine. `ensure_stt` returns Ok(None) when
    // the weights are still downloading; `ensure_llm` returns Ok(None) when the
    // fixed model file is absent (graceful STT-only mode).
    //
    // Eager-vs-lazy deviation (deliberate): the lifecycle design preloads the
    // LLM only under Balanced/Performance on AC. We call `ensure_llm` here
    // unconditionally (when the file exists) because the UI needs a definitive
    // Ready/Error ModelState up front, and because eagerly loading is the only
    // way to detect a broken GGUF so the auto-recover block below can react.
    // The energy policy still applies afterward: `lifecycle_loop` idle-unloads
    // the LLM once the active profile's window elapses.
    let stt_outcome = engine.ensure_stt().await;
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
            tracing::info!(
                model = %stt_model_id,
                "STT weights downloading in background; will be ready after the next reload"
            );
            stt_downloading = true;
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
    // block below — otherwise a stale load's failure could clear the
    // engine's `llm_model_path` out from under the newer reload that's
    // already in flight (or has already loaded successfully).
    {
        let inner = state.inner.lock().unwrap();
        if inner.current_load_token != token {
            tracing::info!("stale load result discarded (token mismatch)");
            return;
        }
    }

    // Auto-recover from an unloadable cleanup model (corrupt GGUF, unsupported
    // arch): drop the engine's LLM path so the pipeline falls cleanly into
    // STT-only mode and the next reload (after a Repair re-download) can retry.
    // There is no persisted selection to clear any more — the fixed path is
    // recomputed from disk on every load.
    if llama_err.is_some() {
        let cfg = crate::engine::EngineConfig {
            llm_model_path: None,
            llm_ctx_size: ctx_size,
            stt_model_id: Some(stt_model_id.clone()),
        };
        engine.update_config(cfg);
        let _ = app.emit(
            "toast",
            crate::commands::toast(
                "warn",
                "Cleanup model couldn't load. Re-download it in Settings → Models.",
            ),
        );
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
    // backends (Metal via parakeet-cpp / llama-cpp-2, plus CoreML when
    // enabled) may have been touched. This must happen AFTER the loads
    // above so our atexit is registered later than ggml's `__cxa_atexit`
    // and therefore runs earlier in the LIFO finalize order — see
    // `register_quit_safety_atexit` for the full reasoning. Called on
    // every load_models invocation (initial and settings-triggered
    // reloads) so the ordering invariant holds even when the user
    // switches model selections mid-session. Not reached when onboarding
    // isn't complete yet (early-return near the top of this function, before
    // any GPU backend is touched) or when a newer reload has superseded this
    // one (the load-token guard above) — those are the only two early-return
    // paths in this function.
    crate::register_quit_safety_atexit();

    // Warm-up policy is owned by the active energy profile, not a separate
    // setting: Balanced/Performance keep models resident, so precompiling GPU
    // kernels + allocating the KV cache up front pays off; PowerSaver stays
    // cold (it idle-unloads aggressively, so a warm-up would be wasted). If the
    // selector isn't wired yet (very early startup), default to warming up.
    let warm = state
        .profile_selector()
        .is_none_or(|sel| sel.current_profile().keeps_models_warm());
    if warm && (stt_ready || llama_ready) {
        let engine_for_warmup = engine.clone();
        tokio::spawn(async move {
            warm_up(&engine_for_warmup).await;
        });
    }
}

/// Explicit re-load of both fixed models. Used after a Repair re-download in
/// Settings → Models, where there is no settings change to piggy-back a reload
/// on (model selection no longer lives in settings).
#[tauri::command]
pub async fn reload_models(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    load_models(&app, state).await;
    Ok(())
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
/// Called at the end of `load_models` when the active energy profile keeps
/// models resident (Balanced/Performance — see
/// [`inference_core::profile::ProfileName::keeps_models_warm`]). PowerSaver
/// skips it.
///
/// Drives the warm-up through the Engine: `ensure_stt` (lazy-loads if needed)
/// for a silent transcribe, and a 1-token `chat` for the LLM. Both are
/// best-effort — a warm-up failure is not user-visible.
pub(crate) async fn warm_up(engine: &Arc<crate::engine::Engine>) {
    match engine.ensure_stt().await {
        Ok(Some(slot)) => {
            // 1 second of silence at 16 kHz mono — long enough for any of
            // the STT backend to exercise its full encode/decode
            // path without producing real text.
            let warm = tokio::task::spawn_blocking(move || {
                let silence = vec![0.0_f32; 16_000];
                let t0 = std::time::Instant::now();
                let elapsed_ms = || u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
                let mut guard = slot.blocking_lock();
                match guard.transcribe(
                    &silence,
                    16_000,
                    &SttOptions {
                        language: Some("en".to_string()),
                    },
                ) {
                    Ok(_) => tracing::info!(elapsed_ms = elapsed_ms(), "STT warm-up completed"),
                    Err(e) => {
                        tracing::warn!(
                            ?e,
                            elapsed_ms = elapsed_ms(),
                            "STT warm-up failed (non-fatal)"
                        );
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
        ..Default::default()
    };
    let t0 = std::time::Instant::now();
    let elapsed_ms = || u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
    match engine.chat(req).await {
        Ok(_) => tracing::info!(elapsed_ms = elapsed_ms(), "llama warm-up completed"),
        Err(e) => tracing::warn!(
            ?e,
            elapsed_ms = elapsed_ms(),
            "llama warm-up failed (non-fatal)"
        ),
    }
}

/// Hot-path entry used by the hotkey pipeline: lock the slot, run
/// STT inference on the blocking pool, return trimmed text.
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
            .transcribe(&samples, 16_000, &SttOptions { language })
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
///   * `None`      — the worker errored mid-stream or the driver dropped
///     early; caller should fall back to the one-shot transcribe path.
pub struct StreamingHandle {
    pub stop_tx: oneshot::Sender<Vec<f32>>,
    pub result_rx: oneshot::Receiver<Option<String>>,
}

/// Cadence of the pseudo-streaming loop. Each tick: peek the recorder for
/// new 16 kHz samples, append to buffer, re-transcribe, emit a
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
fn peek_recorder_tail(state: &AppState, cursor: usize) -> Option<(Vec<f32>, usize)> {
    let inner = state.inner.lock().ok()?;
    let rec = inner.recorder.as_ref()?;
    rec.peek_resampled_since(cursor).ok()
}

/// Spawn the live streaming worker for the current dictation.
///
/// The worker loops every [`STREAM_TICK`]: peek the recorder for new 16 kHz
/// samples, append them to a growing buffer, re-transcribe the full buffer,
/// compute an LCP delta against the previous text, and emit a
/// `recording:partial_transcript` Tauri event. When `handle_up` sends the
/// authoritative final samples on `stop_tx`, the worker transcribes them
/// wholesale as the definitive result, emits one `is_final` event, and
/// resolves `result_rx` with the trimmed text.
///
/// Concurrency: the worker runs on `tokio::task::spawn_blocking` and holds
/// the STT slot's `blocking_lock` for the entire dictation — matching the
/// existing one-shot lock-for-duration pattern.
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
    let opts = SttOptions { language };

    // Pseudo-streaming: accumulate new samples into a growing buffer and
    // re-transcribe the whole buffer each tick, diffing against the last text.
    let mut buffer: Vec<f32> = Vec::new();
    let mut cursor: usize = 0;
    let mut last_text = String::new();

    while let Some(event) = tick_rx.blocking_recv() {
        match event {
            TickEvent::Tick => {
                let Some((tail, new_cursor)) = peek_recorder_tail(&state, cursor) else {
                    continue;
                };
                cursor = new_cursor;
                if tail.is_empty() {
                    continue;
                }
                buffer.extend_from_slice(&tail);
                match guard.transcribe(&buffer, 16_000, &opts) {
                    Ok(t) => {
                        let n = common_prefix_len(&last_text, &t.text);
                        let delta = t.text[n..].to_string();
                        last_text = t.text.clone();
                        let _ = app.emit(
                            PARTIAL_TRANSCRIPT_EVENT,
                            PartialTranscriptEvent {
                                text: t.text,
                                delta,
                                is_final: false,
                            },
                        );
                    }
                    Err(e) => {
                        tracing::warn!(?e, "streaming worker: transcribe failed");
                        break;
                    }
                }
            }
            TickEvent::Stop(final_samples) => {
                // `final_samples` from handle_up is the authoritative full
                // recording. Transcribe it wholesale for the final result.
                match guard.transcribe(&final_samples, 16_000, &opts) {
                    Ok(t) => {
                        let text = t.text.trim().to_string();
                        let _ = app.emit(
                            PARTIAL_TRANSCRIPT_EVENT,
                            PartialTranscriptEvent::final_text(&text),
                        );
                        let _ = result_tx.send(Some(text));
                        return;
                    }
                    Err(e) => {
                        tracing::warn!(?e, "streaming worker: final transcribe failed");
                        let _ = result_tx.send(None);
                        return;
                    }
                }
            }
        }
    }

    // Channel closed without a Stop (driver dropped early): emit final-from-last.
    let _ = app.emit(
        PARTIAL_TRANSCRIPT_EVENT,
        PartialTranscriptEvent::final_text(&last_text),
    );
    let _ = result_tx.send(None);
}

/// Coerce a frontend-supplied language string into `Option<String>` for
/// `SttOptions` (`None` = auto-detect). Empty string and the sentinel
/// `"auto"` both map to auto-detect; everything else passes through.
fn normalize_language(lang: Option<String>) -> Option<String> {
    match lang {
        Some(s) if s.is_empty() || s == "auto" => None,
        other => other,
    }
}

/// Decode a WAV byte slice into `(mono_f32_samples, sample_rate_hz)`.
/// The STT backend expects 16 kHz mono f32; WAV files at other rates are
/// decoded here and the source rate is surfaced to the transcribe call.
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
