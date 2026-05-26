#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------- Shared model metadata ----------

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub kind: &'static str,
    pub backend: &'static str,
    pub path: PathBuf,
    pub coreml: bool,
    pub loaded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctx_size: Option<u32>,
}

// ---------- LLM ----------

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("context overflow: {0}")]
    ContextOverflow(String),
    #[error("model not loaded")]
    ModelNotLoaded,
    #[error("backend busy (mutex timeout)")]
    Busy,
    #[error("llama failure: {0}")]
    Llama(String),
    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
    System,
}

impl ChatRole {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
            ChatRole::System => "system",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub system: Option<String>,
    pub user: String,
    pub history: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: u32,
    pub stop: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StoppedBy {
    Eos,
    Length,
    Stop,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TokenUsage {
    pub prompt: u32,
    pub completion: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatResponse {
    pub text: String,
    pub model: String,
    pub stopped_by: StoppedBy,
    pub tokens: TokenUsage,
}

#[async_trait]
pub trait LlmBackend: Send + Sync + 'static {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError>;
    fn model_info(&self) -> ModelInfo;
}

pub type LlmBackendHandle = Arc<dyn LlmBackend>;
