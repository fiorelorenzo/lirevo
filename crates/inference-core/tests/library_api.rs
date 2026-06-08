//! Compile-time smoke checks for the library-facing API surface.
//!
//! Pins the LLM `chat_sync` shape that the dev-only sidecar and CLI consume.
//! Real STT now runs in the Tauri host via parakeet-cpp; the sidecar's
//! `/v1/stt` is stub-only.

use inference_core::{ChatRequest, ChatResponse, LlamaBackend, LlmError};

#[test]
fn llama_chat_sync_signature_compiles() {
    fn _check(l: &LlamaBackend, req: ChatRequest) {
        let _r: Result<ChatResponse, LlmError> = l.chat_sync(req);
    }
    let _ = _check as fn(&LlamaBackend, ChatRequest);
}
