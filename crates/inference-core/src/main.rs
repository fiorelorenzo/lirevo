#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

use inference_core::backend::LlmBackendHandle;
use inference_core::stt::{AudiopipeEngine, SttEngineHandle, StubEngine};
use inference_core::{llama, server, stub_llm};

const DEFAULT_STT_MODEL_NAME: &str = "parakeet-tdt-0.6b-v3";

fn socket_path_from_env() -> Result<PathBuf> {
    let path = env::var("SIDECAR_SOCKET_PATH")
        .context("SIDECAR_SOCKET_PATH env var is required")?;
    Ok(PathBuf::from(path))
}

/// Picks the STT backend at startup based on env.
///
/// `SIDECAR_STT_BACKEND` selects the lane:
///   * unset → no STT loaded (`/v1/stt` returns 503). Mirrors the pre-M4
///     behaviour where the sidecar started fast unless an explicit STT
///     was configured; keeps test harnesses that only exercise `/v1/chat`
///     or `/healthz` from waiting on a Hugging Face download.
///   * `"stub"` → canned-text [`StubEngine`] for smoke tests.
///   * `"audiopipe"` → loads [`AudiopipeEngine`] with the model name
///     from `SIDECAR_STT_MODEL_NAME` (defaulting to
///     [`DEFAULT_STT_MODEL_NAME`]). May block on a HF download the first
///     time a given model name is loaded.
fn load_stt_backend() -> Option<SttEngineHandle> {
    let Ok(kind) = env::var("SIDECAR_STT_BACKEND") else {
        tracing::info!("SIDECAR_STT_BACKEND not set; STT disabled (/v1/stt → 503)");
        return None;
    };
    match kind.as_str() {
        "stub" => Some(Arc::new(StubEngine::new()) as SttEngineHandle),
        "audiopipe" => {
            let model_name = env::var("SIDECAR_STT_MODEL_NAME")
                .unwrap_or_else(|_| DEFAULT_STT_MODEL_NAME.to_string());
            match AudiopipeEngine::from_pretrained(&model_name) {
                Ok(b) => Some(Arc::new(b) as SttEngineHandle),
                Err(e) => {
                    tracing::error!(error = ?e, model = %model_name, "audiopipe load failed; STT disabled");
                    None
                }
            }
        }
        other => {
            tracing::warn!(backend = %other, "unknown SIDECAR_STT_BACKEND, ignoring");
            None
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
    tracing_subscriber::fmt()
        .with_env_filter(log_level)
        .init();

    let socket_path = socket_path_from_env()?;
    let stt = load_stt_backend();
    let llm = load_llm_backend();
    server::run(socket_path, stt, llm).await
}
