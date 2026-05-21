use std::sync::Arc;
use tauri::{AppHandle, State};

use inference_core::{ChatRequest, LlamaBackend, WhisperBackend};

use crate::state::ModelState;
use crate::{AppError, AppState};

/// Hard ceilings on Tauri-command inputs from the webview. The renderer is
/// the only caller, and at 200 MB PCM16 / 100 KB text these caps are far
/// above any legitimate dictation but well below what would let a
/// compromised webview exhaust process RAM with a single `invoke()`.
const MAX_TRANSCRIBE_WAV_BYTES: usize = 200 * 1024 * 1024;
const MAX_CLEAN_TEXT_BYTES: usize = 100 * 1024;

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
    let whisper = {
        let inner = state.inner.lock().unwrap();
        inner
            .whisper
            .as_ref()
            .ok_or(AppError::WhisperNotLoaded)?
            .clone()
    };
    // Empty string = auto-detect in WhisperBackend::transcribe.
    let lang = match language {
        Some(s) if s == "auto" => String::new(),
        Some(s) => s,
        None => String::new(),
    };
    let text = tokio::task::spawn_blocking(move || whisper.transcribe(&wav, &lang))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(text)
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
    let llama = {
        let inner = state.inner.lock().unwrap();
        inner
            .llama
            .as_ref()
            .ok_or(AppError::LlamaNotLoaded)?
            .clone()
    };
    let req = ChatRequest {
        system: Some(lda_prompts::build_clean_system_prompt(&language)),
        history: vec![],
        user: text,
        temperature: 0.2,
        max_tokens: 2048,
        stop: vec![],
    };
    let resp = tokio::task::spawn_blocking(move || llama.chat_sync(req))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(resp.text)
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

/// Load whisper + llama in parallel based on current settings.
///
/// Cancellation: a newer load (e.g. user changes path) increments the token;
/// stale results are discarded.
///
/// CoreML control: `WhisperBackend::load` reads `SIDECAR_WHISPER_COREML_DISABLE`
/// at load time. We set/unset it here based on the `whisperCoreMLDisable`
/// setting so the same process can toggle the behaviour across reloads.
pub async fn load_models(app: &AppHandle, state: State<'_, AppState>) {
    let (whisper_path, llm_path, ctx_size, coreml_disable, token) = {
        let mut inner = state.inner.lock().unwrap();
        inner.current_load_token += 1;
        let token = inner.current_load_token;
        (
            inner.settings.whisper_model_path.clone(),
            inner.settings.llm_model_path.clone(),
            inner.settings.llm_ctx_size,
            inner.settings.whisper_coreml_disable,
            token,
        )
    };

    if whisper_path.is_none() && llm_path.is_none() {
        state.set_model_state(app, ModelState::Idle);
        return;
    }

    state.set_model_state(
        app,
        ModelState::Loading {
            whisper: whisper_path.is_some(),
            llama: llm_path.is_some(),
        },
    );

    // SAFETY: env vars are process-global; mutating them is unsafe in
    // 2024-edition Rust because of the race with concurrent readers.
    // The whisper backend reads this env var inside its `load()` call,
    // which we spawn just below — so the mutation here happens
    // strictly BEFORE the only reader we know of. Two overlapping
    // `update_settings` invocations could in principle race on this var,
    // but the result is "whichever whisper load won the race uses the
    // value it observed", which is acceptable since both loads carry the
    // same `coreml_disable` value derived from the freshly persisted
    // settings.
    if coreml_disable {
        unsafe { std::env::set_var("SIDECAR_WHISPER_COREML_DISABLE", "1") };
    } else {
        unsafe { std::env::remove_var("SIDECAR_WHISPER_COREML_DISABLE") };
    }

    let whisper_handle = whisper_path.map(|p| {
        tokio::task::spawn_blocking(move || WhisperBackend::load(p))
    });
    let llama_handle = llm_path.map(|p| {
        tokio::task::spawn_blocking(move || LlamaBackend::load(p, ctx_size))
    });

    let whisper_result = match whisper_handle {
        Some(h) => Some(h.await),
        None => None,
    };
    let llama_result = match llama_handle {
        Some(h) => Some(h.await),
        None => None,
    };

    // Commit only if this token is still current.
    let mut whisper_ready = false;
    let mut llama_ready = false;
    let mut whisper_err: Option<String> = None;
    let mut llama_err: Option<String> = None;
    {
        let mut inner = state.inner.lock().unwrap();
        if inner.current_load_token != token {
            tracing::info!("stale load result discarded (token mismatch)");
            return;
        }
        match whisper_result {
            Some(Ok(Ok(w))) => {
                inner.whisper = Some(Arc::new(w));
                whisper_ready = true;
            }
            Some(Ok(Err(e))) => {
                tracing::warn!(?e, "whisper load failed");
                whisper_err = Some(e.to_string());
            }
            Some(Err(e)) => {
                tracing::warn!(%e, "whisper load join error");
                whisper_err = Some(format!("worker panic: {e}"));
            }
            None => {}
        }
        match llama_result {
            Some(Ok(Ok(l))) => {
                inner.llama = Some(Arc::new(l));
                llama_ready = true;
            }
            Some(Ok(Err(e))) => {
                tracing::warn!(?e, "llama load failed");
                llama_err = Some(e.to_string());
            }
            Some(Err(e)) => {
                tracing::warn!(%e, "llama load join error");
                llama_err = Some(format!("worker panic: {e}"));
            }
            None => {}
        }
    }

    let next = if !whisper_ready && !llama_ready {
        // Surface the actual failure reasons so the UI can show something
        // more useful than "models failed to load". Format: each model that
        // was configured but failed reports its own error message.
        let parts: Vec<String> = [
            whisper_err.as_ref().map(|e| format!("Whisper: {e}")),
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
    } else {
        ModelState::Ready {
            whisper: whisper_ready,
            llama: llama_ready,
        }
    };
    state.set_model_state(app, next);

    // Register the macOS shutdown-safety atexit handler now that ggml-metal
    // has been touched (whisper-rs and/or llama-cpp-2 backends initialize it
    // lazily). This must happen AFTER the loads above so our atexit is
    // registered later than ggml's __cxa_atexit and therefore runs earlier
    // in the LIFO finalize order — see `register_quit_safety_atexit` for
    // the full reasoning. Called on every load_models invocation (initial
    // and settings-triggered reloads) so the ordering invariant holds even
    // when the user switches model files mid-session.
    //
    // Skipped on the no-paths-configured early-return above (line ~117): if
    // neither model is configured, no Metal context exists, no destructor
    // is registered, and there is nothing to swallow on shutdown.
    crate::register_quit_safety_atexit();

    // Warm up the loaded backends so the first real dictation doesn't pay
    // the one-time GPU-kernel-compile + KV-cache-allocate cost. Gated by
    // the `keep_models_warm` setting (default on). Runs on a blocking
    // thread — it's a few hundred ms of inference and we don't want to
    // stall the tokio runtime.
    let (keep_warm, whisper_for_warmup, llama_for_warmup) = {
        let inner = state.inner.lock().unwrap();
        (
            inner.settings.keep_models_warm,
            if whisper_ready { inner.whisper.clone() } else { None },
            if llama_ready { inner.llama.clone() } else { None },
        )
    };
    if keep_warm && (whisper_for_warmup.is_some() || llama_for_warmup.is_some()) {
        tokio::task::spawn_blocking(move || {
            warm_up(whisper_for_warmup, llama_for_warmup);
        });
    }
}

/// One-shot dummy inference per loaded backend to force GPU kernel
/// compilation + KV cache allocation up front. Errors are swallowed: a
/// warm-up failure is not user-visible and the first real dictation
/// will surface any genuine problem through its normal error path.
fn warm_up(whisper: Option<Arc<WhisperBackend>>, llama: Option<Arc<LlamaBackend>>) {
    if let Some(w) = whisper {
        // 1 second of silence at 16 kHz mono — long enough for whisper to
        // exercise its full encode/decode path without producing real text.
        // Language pinned ("en") so it doesn't burn a language-detection pass.
        let silence = vec![0.0_f32; 16_000];
        match w.transcribe_samples(&silence, "en") {
            Ok(_) => tracing::info!("whisper warm-up completed"),
            Err(e) => tracing::warn!(?e, "whisper warm-up failed (non-fatal)"),
        }
    }
    if let Some(l) = llama {
        // Single-token generation is enough to force kernel compile + KV
        // alloc. Empty system + a one-character user prompt; max_tokens=1
        // keeps the work minimal.
        let req = ChatRequest {
            system: None,
            history: vec![],
            user: "a".into(),
            temperature: 0.0,
            max_tokens: 1,
            stop: vec![],
        };
        match l.chat_sync(req) {
            Ok(_) => tracing::info!("llama warm-up completed"),
            Err(e) => tracing::warn!(?e, "llama warm-up failed (non-fatal)"),
        }
    }
}
