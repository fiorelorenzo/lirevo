use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend as LlamaCppBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
#[allow(deprecated)]
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use tracing::info;

use crate::backend::{
    ChatRequest, ChatResponse, LlmBackend, LlmError, ModelInfo, StoppedBy, TokenUsage,
};

// llama.cpp requires a single global backend init. Use OnceLock to guarantee it runs once.
static LLAMA_BACKEND: OnceLock<Arc<LlamaCppBackend>> = OnceLock::new();

fn global_backend() -> Result<Arc<LlamaCppBackend>, LlmError> {
    if let Some(b) = LLAMA_BACKEND.get() {
        return Ok(b.clone());
    }
    let backend = LlamaCppBackend::init()
        .map_err(|e| LlmError::Llama(format!("backend init: {e}")))?;
    let arc = Arc::new(backend);
    let _ = LLAMA_BACKEND.set(arc.clone());
    Ok(LLAMA_BACKEND.get().expect("set just now").clone())
}

/// Compile-time directory of the loadable ggml backend MODULES (Metal, the CPU
/// variants, …) built for `llama-cpp-2` under the `dynamic-backends` feature.
/// `None` on a static build. Mirrors `llama_cpp_2::llama_backend::BACKENDS_DIR`.
#[must_use]
pub fn llm_backends_dir() -> Option<&'static str> {
    llama_cpp_2::llama_backend::BACKENDS_DIR
}

/// Load the ggml backend modules from `dir` (dlopen of `libggml-metal.so`
/// etc.). MUST run before the global llama backend is initialized (i.e. before
/// the first [`LlamaBackend::load`]); idempotent at the ggml level. Thin wrapper
/// over `llama_cpp_2::llama_backend::load_backends_from_path` so host code does
/// not depend on `llama-cpp-2` directly.
pub fn load_llm_backends_from_path(dir: &std::path::Path) {
    llama_cpp_2::llama_backend::load_backends_from_path(dir);
}

/// Name of the compute device the LLM backend resolved to, e.g. `"Metal"` /
/// `"CUDA"` / `"Vulkan"` / `"CPU"`. Returns the first non-CPU (GPU/iGPU/accel)
/// device's backend name if any was discovered, else the first device's, else
/// an empty string. Meaningful only after [`load_llm_backends_from_path`] has
/// run (the dynamic modules must be loaded for a GPU device to register).
#[must_use]
pub fn active_llm_backend_name() -> String {
    use llama_cpp_2::LlamaBackendDeviceType as Ty;
    let devices = llama_cpp_2::list_llama_ggml_backend_devices();
    devices
        .iter()
        .find(|d| !matches!(d.device_type, Ty::Cpu))
        .or_else(|| devices.first())
        .map(|d| d.backend.clone())
        .unwrap_or_default()
}

pub struct LlamaBackend {
    model: Arc<LlamaModel>,
    // std::sync::Mutex is used because all llama.cpp work runs inside
    // tokio::task::block_in_place; no async locking is needed.
    ctx: Arc<Mutex<LlamaContext<'static>>>,
    model_path: PathBuf,
    model_id: String,
    ctx_size: u32,
    _backend: Arc<LlamaCppBackend>,
}

// SAFETY: `LlamaContext` is !Send/!Sync upstream because it wraps a raw pointer
// and holds a `&LlamaModel` borrow. We restore Send+Sync via two invariants:
//
// 1. All access to the wrapped context goes through `std::sync::Mutex`, so
//    only one thread at a time touches the raw llama.cpp state — upholding
//    the single-threaded-access requirement.
//
// 2. The `'static` lifetime on `LlamaContext<'static>` is fabricated via
//    `std::mem::transmute` from a borrow into the `Arc<LlamaModel>` we own.
//    Field drop order is forward (model drops before ctx), so the borrow
//    is technically dangling for the brief window between `Arc<LlamaModel>::drop`
//    and `LlamaContext::drop`. This is safe because `LlamaContext::drop` only
//    calls `llama_free(self.context.as_ptr())` and never dereferences
//    `self.model`, so the dangling reference is never read.
//
// `LlamaModel` itself is Send+Sync per llama-cpp-2's own unsafe impl.
unsafe impl Send for LlamaBackend {}
unsafe impl Sync for LlamaBackend {}

impl LlamaBackend {
    /// Synchronous, blocking chat completion for in-process callers (e.g. Tauri).
    ///
    /// Performs the same llama.cpp dance as `<LlamaBackend as LlmBackend>::chat`,
    /// but without the `block_in_place` wrapper — call this from a real OS thread
    /// (e.g. a `spawn_blocking` task or a non-async context). The async trait
    /// impl below simply wraps this in `tokio::task::block_in_place`.
    // Take `req` by value to mirror the async trait method's signature and the
    // ownership pattern callers already use (request is consumed for tokenization).
    #[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
    pub fn chat_sync(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        // try_lock detects a busy context (another inference in flight) and
        // returns Busy immediately, matching the async path's semantics.
        let mut ctx_guard = self.ctx.try_lock().map_err(|_| LlmError::Busy)?;

        // Reset the KV cache between requests. llama.cpp keeps the per-sequence
        // position counter across decodes, so without this clear the second
        // request's batch starts at position 0 while the cache expects N+1,
        // failing with "inconsistent sequence positions" / "n_tokens == 0".
        // Single-turn /v1/chat semantics: each call is independent, so wipe.
        ctx_guard.clear_kv_cache();

        let prompt = build_prompt(&self.model, &req)?;
        let tokens: Vec<LlamaToken> = self
            .model
            .str_to_token(&prompt, AddBos::Always)
            .map_err(|e| LlmError::Llama(format!("tokenize: {e}")))?;

        let prompt_tokens = u32::try_from(tokens.len()).unwrap_or(u32::MAX);
        if prompt_tokens.saturating_add(req.max_tokens) > self.ctx_size {
            return Err(LlmError::ContextOverflow(format!(
                "prompt {prompt_tokens} tokens + max_tokens {} > ctx_size {}",
                req.max_tokens, self.ctx_size
            )));
        }

        let mut batch = LlamaBatch::new(tokens.len(), 1);
        for (i, tok) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            batch
                .add(*tok, i32::try_from(i).unwrap(), &[0], is_last)
                .map_err(|e| LlmError::Llama(format!("batch add: {e}")))?;
        }
        ctx_guard
            .decode(&mut batch)
            .map_err(|e| LlmError::Llama(format!("decode prompt: {e}")))?;

        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::top_k(40),
            LlamaSampler::top_p(0.9, 1),
            LlamaSampler::temp(req.temperature),
            LlamaSampler::dist(0xC0FF_EE00_u32),
        ]);

        let mut out = String::new();
        let mut completion_count: u32 = 0;
        let mut next_pos = i32::try_from(tokens.len()).unwrap();
        // `stopped` is always written before the loop exits, so no initialiser needed.
        let stopped;

        loop {
            let token = sampler.sample(&ctx_guard, -1);
            sampler.accept(token);

            if self.model.is_eog_token(token) {
                stopped = StoppedBy::Eos;
                break;
            }

            // API deviation (v0.1.146): token_to_str is deprecated upstream in favour of
            // token_to_piece (which requires a stateful encoding_rs::Decoder).  The function
            // still works correctly; suppress the deprecation warning narrowly.
            #[allow(deprecated)]
            let fragment = self
                .model
                .token_to_str(token, Special::Tokenize)
                .map_err(|e| LlmError::Llama(format!("detokenize: {e}")))?;
            out.push_str(&fragment);
            completion_count += 1;

            if req.stop.iter().any(|s| out.ends_with(s)) {
                for s in &req.stop {
                    if let Some(stripped) = out.strip_suffix(s) {
                        out = stripped.to_string();
                        break;
                    }
                }
                stopped = StoppedBy::Stop;
                break;
            }

            if completion_count >= req.max_tokens {
                stopped = StoppedBy::Length;
                break;
            }

            let mut next_batch = LlamaBatch::new(1, 1);
            next_batch
                .add(token, next_pos, &[0], true)
                .map_err(|e| LlmError::Llama(format!("batch add next: {e}")))?;
            ctx_guard
                .decode(&mut next_batch)
                .map_err(|e| LlmError::Llama(format!("decode next: {e}")))?;
            next_pos += 1;
        }

        Ok(ChatResponse {
            text: out.trim_end().to_string(),
            model: self.model_id.clone(),
            stopped_by: stopped,
            tokens: TokenUsage {
                prompt: prompt_tokens,
                completion: completion_count,
            },
        })
    }

    pub fn load(model_path: PathBuf, ctx_size: u32, n_threads: i32) -> Result<Self, LlmError> {
        // llama-cpp-2's load_from_file panics (not Err) on a missing file, so
        // guard here to surface it as a recoverable error instead.
        if !model_path.exists() {
            return Err(LlmError::Llama(format!(
                "model file does not exist: {}",
                model_path.display()
            )));
        }

        let backend = global_backend()?;

        // API deviation (v0.1.146): with_n_gpu_layers takes u32 (not i32).
        // 999 = "offload all layers to GPU" (Metal on macOS arm64).
        let model_params = LlamaModelParams::default().with_n_gpu_layers(999_u32);

        // API deviation (v0.1.146): load_from_file takes impl AsRef<Path> directly.
        let model = LlamaModel::load_from_file(&backend, model_path.as_path(), &model_params)
            .map_err(|e| LlmError::Llama(format!("load model: {e}")))?;
        let model = Arc::new(model);

        let n_threads = if n_threads >= 1 {
            n_threads
        } else {
            i32::try_from(num_cpus::get()).unwrap_or(1)
        };
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(ctx_size))
            .with_n_threads(n_threads)
            .with_n_threads_batch(n_threads);

        // SAFETY: We extend the model borrow's lifetime to 'static to satisfy the
        // `LlamaContext<'static>` API. The `Arc<LlamaModel>` is stored as a field
        // alongside the context, so the allocation stays alive for the lifetime of
        // `LlamaBackend`. Because Rust drops fields in forward declaration order,
        // `model` drops before `ctx`, leaving the transmuted borrow technically
        // dangling for a brief window. This is sound because `LlamaContext::drop`
        // only calls `llama_free(self.context.as_ptr())` and never dereferences
        // `self.model`, so no read of dangling memory occurs. See also the
        // Send+Sync safety comment above for the full invariant set.
        let model_ref: &'static LlamaModel = unsafe { std::mem::transmute(model.as_ref()) };
        let ctx = model_ref
            .new_context(&backend, ctx_params)
            .map_err(|e| LlmError::Llama(format!("create context: {e}")))?;

        let model_id = model_path
            .file_stem()
            .map_or_else(|| "llama".to_string(), |s| s.to_string_lossy().to_string());

        info!(?model_path, ctx_size, "llama model loaded");

        Ok(Self {
            model,
            // SAFETY: LlamaBackend implements Send+Sync manually (see impls above).
            // The Arc is justified by the shared ownership pattern even though
            // LlamaContext is not independently Send+Sync.
            #[allow(clippy::arc_with_non_send_sync)]
            ctx: Arc::new(Mutex::new(ctx)),
            model_path,
            model_id,
            ctx_size,
            _backend: backend,
        })
    }
}

fn build_prompt(model: &LlamaModel, req: &ChatRequest) -> Result<String, LlmError> {
    let mut msgs: Vec<LlamaChatMessage> = Vec::new();
    if let Some(s) = &req.system {
        msgs.push(
            LlamaChatMessage::new("system".to_string(), s.clone())
                .map_err(|e| LlmError::Llama(format!("chat message: {e}")))?,
        );
    }
    for m in &req.history {
        msgs.push(
            LlamaChatMessage::new(m.role.as_str().to_string(), m.content.clone())
                .map_err(|e| LlmError::Llama(format!("chat message: {e}")))?,
        );
    }
    msgs.push(
        LlamaChatMessage::new("user".to_string(), req.user.clone())
            .map_err(|e| LlmError::Llama(format!("chat message: {e}")))?,
    );

    // API deviation (v0.1.146): apply_chat_template now requires an explicit &LlamaChatTemplate
    // (no longer accepts None to fall back to a built-in default).  Retrieve the template that
    // is baked into the model first, then pass a reference to apply_chat_template.
    let tmpl = model
        .chat_template(None)
        .map_err(|e| LlmError::Llama(format!("get chat template: {e}")))?;

    model
        .apply_chat_template(&tmpl, &msgs, true)
        .map_err(|e| LlmError::Llama(format!("apply chat template: {e}")))
}

#[async_trait]
impl LlmBackend for LlamaBackend {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        // All heavy work runs synchronously inside `chat_sync`; we delegate
        // through `block_in_place` so the multi-threaded tokio runtime can
        // continue making progress on other tasks while llama.cpp churns.
        tokio::task::block_in_place(|| self.chat_sync(req))
    }

    fn model_info(&self) -> ModelInfo {
        ModelInfo {
            id: self.model_id.clone(),
            kind: "llm",
            backend: "llama-cpp-2",
            path: self.model_path.clone(),
            coreml: false,
            loaded: true,
            ctx_size: Some(self.ctx_size),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_signature_takes_n_threads() {
        // Compile-time contract: load takes (PathBuf, u32 ctx, i32 n_threads).
        // A missing model path errors, but the signature is what we assert.
        let r = LlamaBackend::load(std::path::PathBuf::from("/nonexistent.gguf"), 4096, 4);
        assert!(r.is_err());
    }
}
