use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::backend::{
    ChatRequest, ChatResponse, LlmBackendTrait, LlmError, ModelInfo, StoppedBy, TokenUsage,
};

const LOCK_TIMEOUT: Duration = Duration::from_millis(200);
const DEFAULT_CTX_SIZE: u32 = 4096;

pub struct StubLlmBackend {
    sleep: Duration,
    ctx_size: u32,
    lock: Arc<Mutex<()>>,
}

impl StubLlmBackend {
    #[must_use]
    pub fn new() -> Self {
        let sleep_ms = std::env::var("SIDECAR_LLM_STUB_SLEEP_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let ctx_size = std::env::var("SIDECAR_LLM_STUB_CTX_SIZE")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(DEFAULT_CTX_SIZE);
        Self {
            sleep: Duration::from_millis(sleep_ms),
            ctx_size,
            lock: Arc::new(Mutex::new(())),
        }
    }
}

impl Default for StubLlmBackend {
    fn default() -> Self {
        Self::new()
    }
}

fn approx_token_count(text: &str) -> u32 {
    u32::try_from(text.chars().count() / 4).unwrap_or(u32::MAX).max(1)
}

#[async_trait]
impl LlmBackendTrait for StubLlmBackend {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        let started = Instant::now();
        let _guard = tokio::time::timeout(LOCK_TIMEOUT, self.lock.lock())
            .await
            .map_err(|_| LlmError::Busy)?;

        let mut prompt_chars = 0_usize;
        if let Some(s) = req.system.as_ref() {
            prompt_chars += s.chars().count();
        }
        for m in &req.history {
            prompt_chars += m.content.chars().count();
        }
        prompt_chars += req.user.chars().count();
        let prompt_tokens = u32::try_from(prompt_chars / 4).unwrap_or(u32::MAX).max(1);

        if prompt_tokens.saturating_add(req.max_tokens) > self.ctx_size {
            return Err(LlmError::ContextOverflow(format!(
                "prompt {prompt_tokens} tokens + max_tokens {} > ctx_size {}",
                req.max_tokens, self.ctx_size
            )));
        }

        if !self.sleep.is_zero() {
            tokio::time::sleep(self.sleep).await;
        }

        let text = format!(
            "[stub-llm] system={} user={} history={} max_tokens={}",
            req.system.as_deref().unwrap_or("<unset>"),
            req.user,
            req.history.len(),
            req.max_tokens
        );
        let completion = approx_token_count(&text);
        let _ = started;

        Ok(ChatResponse {
            text,
            model: "stub-llm".to_string(),
            stopped_by: StoppedBy::Eos,
            tokens: TokenUsage { prompt: prompt_tokens, completion },
        })
    }

    fn model_info(&self) -> ModelInfo {
        ModelInfo {
            id: "stub-llm".to_string(),
            kind: "llm",
            backend: "stub",
            path: PathBuf::from("(none)"),
            coreml: false,
            loaded: true,
            ctx_size: Some(self.ctx_size),
        }
    }
}
