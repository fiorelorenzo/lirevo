//! Compile-time smoke checks for the library-facing API surface.
//!
//! Pre-M4 this also pinned `WhisperBackend::transcribe`. With STT migrated
//! into `audiopipe` (re-exported from this crate) the only first-party
//! library entry point we lock down here is the LLM `chat_sync` shape; the
//! audiopipe `Model::transcribe_with_sample_rate` signature is owned by an
//! external crate and pinned via the `rev` in `Cargo.toml`.

use inference_core::{ChatRequest, ChatResponse, LlmBackend, LlmError};

#[test]
fn llm_chat_sync_signature_compiles() {
    fn _check(l: &LlmBackend, req: ChatRequest) {
        let _r: Result<ChatResponse, LlmError> = l.chat_sync(req);
    }
    let _ = _check as fn(&LlmBackend, ChatRequest);
}

#[test]
fn audiopipe_reexport_is_reachable() {
    // The Tauri host pulls audiopipe in directly, but the dev-only sidecar
    // path also wraps it via this crate. Verify the re-export resolves so
    // a future refactor doesn't accidentally hide it.
    let _opts: inference_core::audiopipe::TranscribeOptions = Default::default();
}
