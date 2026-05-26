use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mistralrs::{
    ChatCompletionResponse, GgufModelBuilder, Model, RequestBuilder, StopTokens, TextMessageRole,
};
use tokio::runtime::Runtime;
use tracing::info;

use crate::backend::{
    ChatRequest, ChatResponse, LlmBackendTrait, LlmError, ModelInfo, StoppedBy, TokenUsage,
};

/// In-process LLM cleanup backend, owned by the Tauri host and by the dev
/// sidecar (`crates/inference-core/src/main.rs`). Mistral.rs's high-level API
/// is async, but every caller of `chat_sync` runs on a `spawn_blocking`
/// worker, so it is safe to `block_on` a dedicated runtime held inside the
/// backend without nesting runtimes.
pub struct LlmBackend {
    model: Arc<Model>,
    // A dedicated multi-thread runtime owns the mistral.rs engine task.
    // Wrapped in `Mutex<Option<_>>` so `Drop` can take it out and shut it
    // down on a detached OS thread — `Runtime::drop` panics when called from
    // inside another runtime, which is exactly what would happen if
    // `LlmBackend` is dropped on a tokio worker thread (e.g. when
    // `AppState` clears `inner.llm` during a settings-driven reload).
    rt: Arc<Mutex<Option<Runtime>>>,
    model_path: PathBuf,
    model_id: String,
    ctx_size: u32,
}

impl LlmBackend {
    /// Load a GGUF model from a local file path. `ctx_size` is recorded for
    /// `ModelInfo`; mistral.rs derives the actual context length from the
    /// GGUF metadata, and `max_num_seqs` defaults to 32 which is fine for
    /// our single-stream cleanup workload.
    ///
    /// Builder shape mirrors `mistralrs/examples/getting_started/gguf_locally/main.rs`
    /// from the pinned fork: directory + filename, no remote HF download.
    pub fn load(model_path: PathBuf, ctx_size: u32) -> Result<Self, LlmError> {
        let parent_path = model_path.parent().ok_or_else(|| {
            LlmError::Engine(format!("model path has no parent: {}", model_path.display()))
        })?;
        let parent = parent_path
            .to_str()
            .ok_or_else(|| {
                LlmError::Engine(format!(
                    "model parent dir is not UTF-8: {}",
                    parent_path.display()
                ))
            })?
            .to_string();
        let file = model_path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| {
                LlmError::Engine(format!(
                    "model file name is not UTF-8: {}",
                    model_path.display()
                ))
            })?
            .to_string();

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("lirevo-llm")
            .build()
            .map_err(|e| LlmError::Engine(format!("llm runtime build: {e}")))?;

        let model = rt
            .block_on(
                GgufModelBuilder::new(parent, vec![file])
                    .with_logging()
                    .build(),
            )
            .map_err(|e| LlmError::Engine(format!("load model: {e}")))?;

        let model_id = model_path
            .file_stem()
            .map_or_else(|| "llm".to_string(), |s| s.to_string_lossy().to_string());

        info!(?model_path, ctx_size, "mistral.rs model loaded");

        Ok(Self {
            model: Arc::new(model),
            rt: Arc::new(Mutex::new(Some(rt))),
            model_path,
            model_id,
            ctx_size,
        })
    }

    /// Synchronous, blocking chat completion for in-process callers.
    ///
    /// The mistral.rs `send_chat_request` call is async; we drive it on the
    /// dedicated runtime held inside the backend. Callers (Tauri commands,
    /// HTTP server) already wrap this in `spawn_blocking`, so no nested
    /// runtime panic is possible here.
    #[allow(clippy::needless_pass_by_value, clippy::cast_possible_truncation)]
    pub fn chat_sync(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        let mut builder = RequestBuilder::new();
        if let Some(sys) = &req.system {
            builder = builder.add_message(TextMessageRole::System, sys.clone());
        }
        for m in &req.history {
            let role = match m.role {
                crate::backend::ChatRole::User => TextMessageRole::User,
                crate::backend::ChatRole::Assistant => TextMessageRole::Assistant,
                crate::backend::ChatRole::System => TextMessageRole::System,
            };
            builder = builder.add_message(role, m.content.clone());
        }
        builder = builder.add_message(TextMessageRole::User, req.user.clone());

        builder = builder
            .set_sampler_temperature(f64::from(req.temperature))
            .set_sampler_topk(40)
            .set_sampler_topp(0.9)
            .set_sampler_max_len(req.max_tokens as usize);
        if !req.stop.is_empty() {
            builder = builder.set_sampler_stop_toks(StopTokens::Seqs(req.stop.clone()));
        }

        let rt_guard = self
            .rt
            .lock()
            .map_err(|_| LlmError::Internal("llm runtime mutex poisoned".into()))?;
        let rt = rt_guard
            .as_ref()
            .ok_or_else(|| LlmError::Internal("llm runtime already dropped".into()))?;
        let response: ChatCompletionResponse = rt
            .block_on(self.model.send_chat_request(builder))
            .map_err(|e| LlmError::Engine(format!("send_chat_request: {e}")))?;

        let choice = response
            .choices
            .first()
            .ok_or_else(|| LlmError::Engine("mistral.rs returned no choices".into()))?;
        let text = choice
            .message
            .content
            .clone()
            .unwrap_or_default()
            .trim_end()
            .to_string();
        // mistral.rs uses the OpenAI finish_reason string ("stop", "length",
        // ...). We collapse the EOS / stop-sequence case to StoppedBy::Eos
        // and only flag StoppedBy::Stop when the request explicitly set stop
        // sequences, matching the previous llama-cpp-2 backend's semantics.
        let stopped_by = match choice.finish_reason.as_str() {
            "length" => StoppedBy::Length,
            "stop" if !req.stop.is_empty() => StoppedBy::Stop,
            _ => StoppedBy::Eos,
        };

        let prompt_tokens = u32::try_from(response.usage.prompt_tokens).unwrap_or(u32::MAX);
        let completion_tokens =
            u32::try_from(response.usage.completion_tokens).unwrap_or(u32::MAX);

        if prompt_tokens.saturating_add(completion_tokens) > self.ctx_size {
            tracing::warn!(
                prompt_tokens,
                completion_tokens,
                ctx_size = self.ctx_size,
                "chat response used more tokens than configured ctx_size"
            );
        }

        Ok(ChatResponse {
            text,
            model: self.model_id.clone(),
            stopped_by,
            tokens: TokenUsage {
                prompt: prompt_tokens,
                completion: completion_tokens,
            },
        })
    }
}

impl Drop for LlmBackend {
    fn drop(&mut self) {
        // `Runtime::drop` panics if it runs from inside another runtime
        // (the common case here: `inner.llm = None` is set on a tokio worker
        // thread during model reload). Take the runtime out and shut it
        // down in a detached OS thread to sidestep the panic.
        if let Ok(mut guard) = self.rt.lock() {
            if let Some(rt) = guard.take() {
                std::thread::spawn(move || drop(rt));
            }
        }
    }
}

#[async_trait]
impl LlmBackendTrait for LlmBackend {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        // chat_sync block_on's an internal runtime, so it must not run on a
        // tokio worker. Delegate through block_in_place so the multi-threaded
        // tokio runtime can keep making progress on other tasks while
        // mistral.rs is busy.
        tokio::task::block_in_place(|| self.chat_sync(req))
    }

    fn model_info(&self) -> ModelInfo {
        ModelInfo {
            id: self.model_id.clone(),
            kind: "llm",
            backend: "mistral.rs",
            path: self.model_path.clone(),
            coreml: false,
            loaded: true,
            ctx_size: Some(self.ctx_size),
        }
    }
}
