//! Library facade for `inference-core`.
//!
//! This crate ships both a binary (the dev-only unix-socket sidecar; see
//! `main.rs`) and a library. The shipped Tauri host uses only the library
//! surface, calling the LLM helpers in-process — never the sidecar or any
//! HTTP layer.
//!
//! The HTTP/axum sidecar remains the canonical path used by `lirevo-prototype`
//! and `lirevo-cli`. Real STT runs in the Tauri host via parakeet-cpp; the
//! sidecar's STT endpoint (`/v1/stt`) is stub-only.

#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

pub mod audio;
pub mod backend;
pub mod catalog;
pub mod llama;
pub mod profile;
// The HTTP sidecar binds a Unix-domain socket (dev-only tooling) — Unix-only,
// so non-Unix targets (Windows) compile the rest of the crate cleanly.
#[cfg(unix)]
pub mod server;
pub mod stt;
pub mod stub_llm;
pub mod wire;

pub use backend::{
    ChatMessage, ChatRequest, ChatResponse, ChatRole, LlmBackend, LlmBackendHandle, LlmError,
    ModelInfo, StoppedBy, TokenUsage,
};
pub use llama::LlamaBackend;
pub use stt::{Segment, SttEngine, SttEngineHandle, SttError, SttOptions, StubEngine, Transcript};
