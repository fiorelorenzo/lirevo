use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum AppError {
    #[error("STT model not loaded")]
    SttNotLoaded,
    #[error("LLM model not loaded")]
    LlamaNotLoaded,
    #[error("Inference error: {0}")]
    Inference(String),
    #[error("Hotkey listener error: {0}")]
    Hotkey(String),
    #[error("Settings error: {0}")]
    Settings(String),
    #[error("Permission denied: {0}")]
    Permission(String),
    #[error("Download error: {0}")]
    Download(String),
    #[error("Inject error: {0}")]
    Inject(String),
    #[error("File system error: {0}")]
    Fs(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<audiopipe::Error> for AppError {
    fn from(e: audiopipe::Error) -> Self {
        AppError::Inference(e.to_string())
    }
}

impl From<inference_core::LlmError> for AppError {
    fn from(e: inference_core::LlmError) -> Self {
        AppError::Inference(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Fs(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Settings(format!("json: {e}"))
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_tagged_format() {
        let e = AppError::SttNotLoaded;
        let j = serde_json::to_value(&e).unwrap();
        // Unit variants under #[serde(tag, content)] omit the content key entirely.
        // Frontend should treat a missing `message` as absent/null.
        assert_eq!(j, serde_json::json!({ "kind": "stt_not_loaded" }));
    }

    #[test]
    fn serializes_with_message() {
        let e = AppError::Inference("bad utf8".into());
        let j = serde_json::to_value(&e).unwrap();
        assert_eq!(j, serde_json::json!({ "kind": "inference", "message": "bad utf8" }));
    }

    #[test]
    fn display_includes_message() {
        assert_eq!(AppError::SttNotLoaded.to_string(), "STT model not loaded");
        assert_eq!(
            AppError::Hotkey("EventTap install failed".into()).to_string(),
            "Hotkey listener error: EventTap install failed"
        );
    }
}
