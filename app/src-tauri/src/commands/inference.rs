use std::sync::Arc;
use tauri::{AppHandle, State};

use inference_core::{ChatRequest, LlamaBackend, WhisperBackend};

use crate::state::ModelState;
use crate::{AppError, AppState};

#[tauri::command]
pub async fn transcribe(
    state: State<'_, AppState>,
    wav: Vec<u8>,
    language: Option<String>,
) -> Result<String, AppError> {
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

    // SAFETY: env vars are process-global. Mutating them is unsafe in 2024-edition Rust
    // (race with other threads reading env). Here this runs in a single orchestrator
    // task before spawn_blocking; concurrent reads come only from the whisper loader
    // we kick off next.
    // We use std::env::set_var/remove_var which the whisper backend reads on load().
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
            Some(Ok(Err(e))) => tracing::warn!(?e, "whisper load failed"),
            Some(Err(e)) => tracing::warn!(%e, "whisper load join error"),
            None => {}
        }
        match llama_result {
            Some(Ok(Ok(l))) => {
                inner.llama = Some(Arc::new(l));
                llama_ready = true;
            }
            Some(Ok(Err(e))) => tracing::warn!(?e, "llama load failed"),
            Some(Err(e)) => tracing::warn!(%e, "llama load join error"),
            None => {}
        }
    }

    let next = if !whisper_ready && !llama_ready {
        ModelState::Error {
            reason: "all configured models failed to load".into(),
        }
    } else {
        ModelState::Ready {
            whisper: whisper_ready,
            llama: llama_ready,
        }
    };
    state.set_model_state(app, next);
}
