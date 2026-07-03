#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

use inference_core::backend::LlmBackendHandle;
use inference_core::stt::{SttEngineHandle, StubEngine};
use inference_core::{llama, server, stub_llm};

fn socket_path_from_env() -> Result<PathBuf> {
    let path =
        env::var("SIDECAR_SOCKET_PATH").context("SIDECAR_SOCKET_PATH env var is required")?;
    Ok(PathBuf::from(path))
}

/// Picks the STT backend at startup based on env.
///
/// `SIDECAR_STT_BACKEND` selects the lane:
///   * unset or `"stub"` → canned-text [`StubEngine`] for smoke tests and CI
///     (the sidecar is stub-only; real STT runs in the Tauri host).
///   * Any other value → logged as unknown; resolves to [`StubEngine`].
fn load_stt_backend() -> SttEngineHandle {
    let kind = env::var("SIDECAR_STT_BACKEND").unwrap_or_else(|_| "stub".to_string());
    match kind.as_str() {
        "stub" => Arc::new(StubEngine::new()) as SttEngineHandle,
        other => {
            tracing::warn!(backend = %other, "unknown SIDECAR_STT_BACKEND; falling back to stub");
            Arc::new(StubEngine::new()) as SttEngineHandle
        }
    }
}

/// Picks the LLM backend at startup based on env.
/// Precedence: `SIDECAR_LLM_BACKEND=stub` > llama.
fn load_llm_backend() -> Option<LlmBackendHandle> {
    let kind = env::var("SIDECAR_LLM_BACKEND").unwrap_or_else(|_| "llama".to_string());
    match kind.as_str() {
        "stub" => Some(Arc::new(stub_llm::StubLlmBackend::new()) as LlmBackendHandle),
        "llama" => {
            let Ok(model_path_s) = env::var("SIDECAR_LLM_MODEL_PATH") else {
                tracing::warn!("SIDECAR_LLM_MODEL_PATH not set; /v1/chat will return 503");
                return None;
            };
            let model_path = PathBuf::from(model_path_s);
            if !model_path.exists() {
                tracing::error!(?model_path, "llama model file does not exist; LLM disabled");
                return None;
            }
            let ctx_size: u32 = env::var("SIDECAR_LLM_CTX_SIZE")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(4096);
            // 0 = "all cores" (dev CLI; not the shipped lifecycle path).
            match llama::LlamaBackend::load(model_path, ctx_size, 0) {
                Ok(b) => Some(Arc::new(b) as LlmBackendHandle),
                Err(e) => {
                    tracing::error!(error = ?e, "failed to load LlamaBackend; LLM disabled");
                    None
                }
            }
        }
        other => {
            tracing::warn!(backend = %other, "unknown SIDECAR_LLM_BACKEND, ignoring");
            None
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let log_level = env::var("SIDECAR_LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt().with_env_filter(log_level).init();

    let socket_path = socket_path_from_env()?;
    let stt = Some(load_stt_backend());
    let llm = load_llm_backend();
    server::run(socket_path, stt, llm).await
}
