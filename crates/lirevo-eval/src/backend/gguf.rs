//! GGUF backend: wraps `inference-core::LlmBackend::chat_sync`.
//!
//! Reuses the exact production code path so quality measurements
//! transfer directly to the shipped app.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use inference_core::{ChatRequest, LlmBackend as InnerBackend, LlmError};
use tokio::task;

use super::{BackendError, BackendKind, EvalBackend, GenerateOut, GenerateReq};

pub struct GgufBackend {
    id: String,
    inner: Arc<InnerBackend>,
}

impl GgufBackend {
    /// Loads the model from `path`. Returns `ModelFileMissing(path)` if the
    /// file does not exist, mapping other init failures to `Inference`.
    /// Context size defaults to 4096; override via `LIREVO_EVAL_CTX_SIZE`.
    pub fn load(id: String, path: PathBuf) -> Result<Self, BackendError> {
        if !path.exists() {
            return Err(BackendError::ModelFileMissing(path));
        }
        let ctx_size: u32 = std::env::var("LIREVO_EVAL_CTX_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4096);
        let inner = InnerBackend::load(path, ctx_size).map_err(map_err)?;
        Ok(Self {
            id,
            inner: Arc::new(inner),
        })
    }
}

#[async_trait]
impl EvalBackend for GgufBackend {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> BackendKind {
        BackendKind::Gguf
    }

    async fn generate(&self, req: GenerateReq) -> Result<GenerateOut, BackendError> {
        let inner = self.inner.clone();
        let start = Instant::now();
        let chat_req = ChatRequest {
            system: Some(req.system_prompt),
            user: req.transcript,
            history: Vec::new(),
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            stop: Vec::new(),
        };
        let resp = task::spawn_blocking(move || inner.chat_sync(chat_req))
            .await
            .map_err(|e| BackendError::Inference(format!("join: {e}")))?
            .map_err(map_err)?;
        Ok(GenerateOut {
            text: resp.text,
            latency_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
            prompt_tokens: resp.tokens.prompt,
            completion_tokens: resp.tokens.completion,
            peak_rss_kb: None,
            from_warm_cache: false,
        })
    }
    // warm_system_prompt: default Unsupported impl from the trait stands until
    // Task 9 wires llama state save/load.
}

fn map_err(e: LlmError) -> BackendError {
    match e {
        LlmError::Busy => BackendError::Busy,
        LlmError::ModelNotLoaded => BackendError::Inference("model not loaded".into()),
        LlmError::Engine(s)
        | LlmError::Internal(s)
        | LlmError::ContextOverflow(s)
        | LlmError::BadRequest(s) => BackendError::Inference(s),
    }
}
