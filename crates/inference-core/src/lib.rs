//! Library facade for `inference-core`.
//!
//! This crate ships both a binary (the unix-socket sidecar; see `main.rs`) and a
//! library — the library surface lets in-process consumers call the LLM helpers
//! directly without going through the HTTP layer.
//!
//! The HTTP/axum sidecar remains the canonical path used by `lirevo-prototype` and
//! `lirevo-cli`. STT now goes through `audiopipe` (re-exported from this crate
//! for convenience); the production Tauri host wraps it through its own
//! `app/src-tauri/src/stt` module rather than via this sidecar.

#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

pub mod audio;
pub mod backend;
pub mod catalog;
pub mod llama;
pub mod server;
pub mod stt;
pub mod stub_llm;
pub mod wire;

pub use audiopipe;
pub use backend::{
    ChatMessage, ChatRequest, ChatResponse, ChatRole, LlmBackend, LlmBackendHandle, LlmError,
    ModelInfo, StoppedBy, TokenUsage,
};
pub use llama::LlamaBackend;
pub use stt::{
    AudiopipeEngine, Segment, SttEngine, SttEngineHandle, SttError, SttOptions, StubEngine,
    Transcript,
};
