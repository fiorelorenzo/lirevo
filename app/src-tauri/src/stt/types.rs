//! Lirevo-owned neutral STT types. The seam between the app and whatever STT
//! backend is wired in `stt::mod`. Consumers (commands, engine, hotkey) depend
//! ONLY on these, never on a backend crate's types — so swapping the backend is
//! a change to `stt/` alone.

/// Options for a transcription call. `None` language = engine default/auto.
#[derive(Debug, Clone, Default)]
pub struct SttOptions {
    pub language: Option<String>,
}

/// Result of a transcription. Minimal by design: the pipeline only needs text.
#[derive(Debug, Clone, Default)]
pub struct Transcript {
    pub text: String,
}

/// One incremental update emitted by the live (pseudo-)streaming worker.
#[derive(Debug, Clone, Default)]
pub struct PartialTranscript {
    /// Full cumulative transcript so far.
    pub text: String,
    /// Newly appended tail vs the previous `text` (a hint; may be rewritten).
    pub delta: String,
    pub is_final: bool,
}

/// Neutral STT error. Backend errors are flattened to a message.
#[derive(Debug, thiserror::Error)]
pub enum SttError {
    #[error("STT backend error: {0}")]
    Backend(String),
}
