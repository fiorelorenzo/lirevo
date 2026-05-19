//! Pluggable backend abstraction for the eval harness.

use std::path::PathBuf;

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct GenerateReq {
    pub system_prompt: String,
    pub transcript: String,
    pub max_tokens: u32,
    pub temperature: f32,
}

#[derive(Debug, Clone)]
pub struct GenerateOut {
    pub text: String,
    pub latency_ms: u64,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub peak_rss_kb: Option<u64>,
    pub from_warm_cache: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Gguf,
    ClaudeCli,
}

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("backend not found: {0}")]
    NotFound(String),
    #[error("backend timed out")]
    Timeout,
    #[error("backend busy")]
    Busy,
    #[error("llama failure: {0}")]
    Llama(String),
    #[error("subprocess failure: {0}")]
    Process(String),
    #[error("not supported by this backend: {0}")]
    Unsupported(String),
}

/// All backends implement this trait. `warm_system_prompt` is best-effort —
/// backends without local KV state (e.g. the Claude CLI) return
/// `Unsupported` and the harness records cold-only timings for them.
#[async_trait]
pub trait EvalBackend: Send + Sync {
    fn id(&self) -> &str;
    fn kind(&self) -> BackendKind;
    async fn generate(&self, req: GenerateReq) -> Result<GenerateOut, BackendError>;
    async fn warm_system_prompt(&mut self, _sys: &str) -> Result<(), BackendError> {
        Err(BackendError::Unsupported("warm cache".into()))
    }
}

// ---- Spec parsing ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendKindSpec {
    Gguf,
    ClaudeCli,
}

#[derive(Debug, Clone)]
pub struct BackendSpec {
    pub kind: BackendKindSpec,
    pub id: String,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum SpecError {
    #[error("missing ':' in backend spec — expected `<kind>:<id>[@<path>]`, got `{0}`")]
    Malformed(String),
    #[error("unknown backend kind `{0}` — supported: gguf, claude-p")]
    UnknownKind(String),
    #[error("gguf backends require `@<path>`")]
    MissingPath,
}

pub fn parse_spec(s: &str) -> Result<BackendSpec, SpecError> {
    let (kind_str, rest) = s
        .split_once(':')
        .ok_or_else(|| SpecError::Malformed(s.into()))?;
    let (id, path) = match rest.split_once('@') {
        Some((id, p)) => (id.to_string(), Some(PathBuf::from(p))),
        None => (rest.to_string(), None),
    };
    let kind = match kind_str {
        "gguf" => {
            if path.is_none() {
                return Err(SpecError::MissingPath);
            }
            BackendKindSpec::Gguf
        }
        "claude-p" => BackendKindSpec::ClaudeCli,
        other => return Err(SpecError::UnknownKind(other.into())),
    };
    Ok(BackendSpec { kind, id, path })
}

#[cfg(test)]
mod tests {
    use super::{parse_spec, BackendKindSpec, SpecError};

    #[test]
    fn parse_spec_gguf_with_path() {
        let s = parse_spec("gguf:qwen3-4b@/tmp/x.gguf").unwrap();
        assert_eq!(s.kind, BackendKindSpec::Gguf);
        assert_eq!(s.id, "qwen3-4b");
        assert_eq!(s.path.as_deref(), Some(std::path::Path::new("/tmp/x.gguf")));
    }

    #[test]
    fn parse_spec_claude_p_no_path() {
        let s = parse_spec("claude-p:claude-3-5-sonnet").unwrap();
        assert_eq!(s.kind, BackendKindSpec::ClaudeCli);
        assert_eq!(s.id, "claude-3-5-sonnet");
        assert!(s.path.is_none());
    }

    #[test]
    fn parse_spec_rejects_unknown_kind() {
        let err = parse_spec("magic:foo").unwrap_err();
        assert!(matches!(err, SpecError::UnknownKind(_)));
    }

    #[test]
    fn parse_spec_rejects_gguf_without_path() {
        let err = parse_spec("gguf:foo").unwrap_err();
        assert!(matches!(err, SpecError::MissingPath));
    }
}
