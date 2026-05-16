#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

mod backend;
mod audio;
mod llama;
mod server;
mod stub;
mod stub_llm;
mod whisper;
mod wire;

use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::backend::{LlmBackendHandle, SttBackendHandle};
use crate::stub::StubBackend;

fn socket_path_from_env() -> Result<PathBuf> {
    let path = env::var("SIDECAR_SOCKET_PATH")
        .context("SIDECAR_SOCKET_PATH env var is required")?;
    Ok(PathBuf::from(path))
}

/// Picks the STT backend at startup based on env.
/// Precedence: `SIDECAR_STT_BACKEND=stub` > whisper (default).
fn load_stt_backend() -> Option<SttBackendHandle> {
    let kind = env::var("SIDECAR_STT_BACKEND").unwrap_or_else(|_| "whisper".to_string());
    match kind.as_str() {
        "stub" => Some(Arc::new(StubBackend::new()) as SttBackendHandle),
        "whisper" => {
            let Ok(model_path_s) = env::var("SIDECAR_WHISPER_MODEL_PATH") else {
                tracing::warn!("SIDECAR_WHISPER_MODEL_PATH not set; /v1/stt will return 503");
                return None;
            };
            let model_path = PathBuf::from(model_path_s);
            if !model_path.exists() {
                tracing::error!(?model_path, "whisper model file does not exist; STT disabled");
                return None;
            }
            match whisper::WhisperBackend::load(model_path) {
                Ok(b) => Some(Arc::new(b) as SttBackendHandle),
                Err(e) => {
                    tracing::error!(error = ?e, "failed to load WhisperBackend; STT disabled");
                    None
                }
            }
        }
        other => {
            tracing::warn!(backend=%other, "unknown SIDECAR_STT_BACKEND, ignoring");
            None
        }
    }
}

/// Picks the LLM backend at startup based on env.
/// Precedence: `SIDECAR_LLM_BACKEND=stub` > llama (real backend lands in T11).
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
            match llama::LlamaBackend::load(model_path, ctx_size) {
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
    tracing_subscriber::fmt()
        .with_env_filter(log_level)
        .init();

    let socket_path = socket_path_from_env()?;
    let stt = load_stt_backend();
    let llm = load_llm_backend();
    server::run(socket_path, stt, llm).await
}
