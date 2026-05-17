//! Compile-time smoke checks for the library-facing API surface added in T2.
//!
//! We don't load real models here (covered by M1a/M1b tests); the point is to
//! pin the exact signatures the Tauri backend will call into so they don't
//! drift silently.

use inference_core::{ChatRequest, ChatResponse, LlamaBackend, LlmError, SttError, WhisperBackend};

#[test]
fn whisper_transcribe_signature_compiles() {
    fn _check(w: &WhisperBackend, wav: &[u8], lang: &str) {
        let _r: Result<String, SttError> = w.transcribe(wav, lang);
    }
    let _ = _check as fn(&WhisperBackend, &[u8], &str);
}

#[test]
fn llama_chat_sync_signature_compiles() {
    fn _check(l: &LlamaBackend, req: ChatRequest) {
        let _r: Result<ChatResponse, LlmError> = l.chat_sync(req);
    }
    let _ = _check as fn(&LlamaBackend, ChatRequest);
}
