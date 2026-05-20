//! Library facade for `inference-core`.
//!
//! This crate ships both a binary (the unix-socket sidecar; see `main.rs`) and a
//! library — the library surface lets in-process consumers (e.g. the Tauri backend)
//! call inference directly without going through the HTTP layer.
//!
//! The HTTP/axum sidecar remains the canonical path used by `lda-prototype` and
//! `lda-cli`; both code paths delegate to the same underlying inference helpers.

#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

pub mod audio;
pub mod backend;
pub mod catalog;
pub mod llama;
pub mod server;
pub mod stub;
pub mod stub_llm;
pub mod whisper;
pub mod wire;

// Re-exports for the most common library entry points.
pub use backend::{
    ChatMessage, ChatRequest, ChatResponse, ChatRole, LlmBackend, LlmBackendHandle, LlmError,
    ModelInfo, Segment, SttBackend, SttBackendHandle, SttError, SttOptions, StoppedBy,
    TokenUsage, Transcript,
};
pub use llama::LlamaBackend;
pub use whisper::WhisperBackend;
