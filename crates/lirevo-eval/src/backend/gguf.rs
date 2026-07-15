//! GGUF backend: wraps `inference-core::LlamaBackend::chat_sync`.
//!
//! Reuses the exact production code path so quality measurements
//! transfer directly to the shipped app.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};
use std::time::Instant;

use async_trait::async_trait;
use inference_core::{ChatRequest, LlamaBackend as InnerBackend, LlmError};
use tokio::task;

use super::{BackendError, BackendKind, EvalBackend, GenerateOut, GenerateReq};

pub struct GgufBackend {
    id: String,
    inner: Arc<InnerBackend>,
}

static LOAD_LLM_BACKENDS: Once = Once::new();

/// Dlopen the ggml compute backend modules (Metal, CPU variants) before the
/// first `LlamaBackend::init`. `inference-core` is built with
/// `dynamic-backends`, so those modules are not statically linked and
/// nothing in `llama-cpp-2` loads them implicitly — the shipped app does
/// this once in `app/src-tauri/src/engine/backend.rs`
/// (`BackendManager::prepare`); the eval harness has no equivalent startup
/// hook, so every path that can construct a [`GgufBackend`] (the top-level
/// CLI, direct library callers, and the `bake-cell` subprocess spawned by
/// `cli::run`) must call this itself. Doing it here, at the single
/// construction choke point, covers all of them. `None` means a static
/// build (e.g. Windows) — not an error, just nothing to load.
fn ensure_llm_backends_loaded() {
    LOAD_LLM_BACKENDS.call_once(|| {
        if let Some(dir) = inference_core::llm_backends_dir() {
            inference_core::load_llm_backends_from_path(Path::new(dir));
        } else {
            tracing::warn!(
                "no llama backends dir at build time; LLM may fall back to a non-dynamic backend"
            );
        }
    });
}

impl GgufBackend {
    /// Loads the model from `path`. Returns `ModelFileMissing(path)` if the
    /// file does not exist, mapping other init failures to `Inference`.
    /// Context size defaults to 4096; override via `LIREVO_EVAL_CTX_SIZE`.
    pub fn load(id: String, path: PathBuf) -> Result<Self, BackendError> {
        if !path.exists() {
            return Err(BackendError::ModelFileMissing(path));
        }
        ensure_llm_backends_loaded();
        let ctx_size: u32 = std::env::var("LIREVO_EVAL_CTX_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4096);
        // n_threads = 0 → backend auto-detects (num_cpus). The eval harness is
        // not profile-aware, so it always lets the backend pick the count.
        let inner = InnerBackend::load(path, ctx_size, 0).map_err(map_err)?;
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
            top_p: req.top_p,
            top_k: req.top_k,
            min_p: req.min_p,
            presence_penalty: req.presence_penalty,
            repetition_penalty: req.repetition_penalty,
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
        LlmError::Llama(s)
        | LlmError::Internal(s)
        | LlmError::ContextOverflow(s)
        | LlmError::BadRequest(s) => BackendError::Inference(s),
    }
}
