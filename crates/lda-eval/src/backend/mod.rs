//! Pluggable backend abstraction for the eval harness.

use std::path::PathBuf;

use async_trait::async_trait;
use thiserror::Error;

pub mod gguf;

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
    #[error("model file missing: {0}")]
    ModelFileMissing(PathBuf),
    #[error("executable `{name}` not in PATH")]
    ExecutableMissing { name: String },
    #[error("backend timed out")]
    Timeout,
    #[error("backend busy")]
    Busy,
    #[error("inference failure: {0}")]
    Inference(String),
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

#[derive(Debug, Clone)]
pub struct BackendSpec {
    pub kind: BackendKind,
    pub id: String,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum SpecError {
    #[error("missing ':' in backend spec — expected `<kind>:<id>[@<path>]`, got `{0}`")]
    Malformed(String),
    #[error("empty backend kind — expected `<kind>:<id>[@<path>]`")]
    EmptyKind,
    #[error("unknown backend kind `{0}` — supported: gguf, claude-p")]
    UnknownKind(String),
    #[error("empty backend id — expected `<kind>:<id>[@<path>]`")]
    EmptyId,
    #[error("gguf backends require `@<path>`")]
    MissingPath,
    #[error("empty path after `@` — expected `<kind>:<id>@<path>`")]
    EmptyPath,
    #[error("backend kind `{kind}` does not accept a path")]
    UnexpectedPath { kind: String },
}

pub fn parse_spec(s: &str) -> Result<BackendSpec, SpecError> {
    let s = s.trim();
    let (kind_str, rest) = s
        .split_once(':')
        .ok_or_else(|| SpecError::Malformed(s.into()))?;
    if kind_str.is_empty() {
        return Err(SpecError::EmptyKind);
    }
    let (id, path) = match rest.split_once('@') {
        Some((id, p)) => {
            if p.is_empty() {
                return Err(SpecError::EmptyPath);
            }
            (id.to_string(), Some(PathBuf::from(p)))
        }
        None => (rest.to_string(), None),
    };
    if id.is_empty() {
        return Err(SpecError::EmptyId);
    }
    let kind = match kind_str {
        "gguf" => {
            if path.is_none() {
                return Err(SpecError::MissingPath);
            }
            BackendKind::Gguf
        }
        "claude-p" => {
            if path.is_some() {
                return Err(SpecError::UnexpectedPath {
                    kind: "claude-p".into(),
                });
            }
            BackendKind::ClaudeCli
        }
        other => return Err(SpecError::UnknownKind(other.into())),
    };
    Ok(BackendSpec { kind, id, path })
}

// ---- Runtime factory ----

/// Build a runtime backend from a spec string. Returns a boxed trait object.
///
/// Spec-parser errors are widened into `BackendError::Inference(string)` so
/// callers can handle a single error surface. The `parse_spec` validation
/// already guarantees `gguf` carries a path, but the factory keeps an explicit
/// guard so a future regression cannot turn into a panic.
// `async` is kept because Task 4's `ClaudeCli` arm will perform async work
// (PATH probe + subprocess); changing the signature later would ripple through
// every caller (`build_from_spec(&spec).await`).
#[allow(clippy::unused_async)]
pub async fn build_from_spec(spec: &str) -> Result<Box<dyn EvalBackend>, BackendError> {
    let parsed = parse_spec(spec).map_err(|e| BackendError::Inference(e.to_string()))?;
    match parsed.kind {
        BackendKind::Gguf => {
            let path = parsed
                .path
                .ok_or_else(|| BackendError::Inference("gguf needs path".into()))?;
            let b = gguf::GgufBackend::load(parsed.id, path)?;
            Ok(Box::new(b))
        }
        BackendKind::ClaudeCli => Err(BackendError::Unsupported(
            "claude-p not yet implemented".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_spec, BackendKind, SpecError};

    #[test]
    fn parse_spec_gguf_with_path() {
        let s = parse_spec("gguf:qwen3-4b@/tmp/x.gguf").unwrap();
        assert_eq!(s.kind, BackendKind::Gguf);
        assert_eq!(s.id, "qwen3-4b");
        assert_eq!(s.path.as_deref(), Some(std::path::Path::new("/tmp/x.gguf")));
    }

    #[test]
    fn parse_spec_claude_p_no_path() {
        let s = parse_spec("claude-p:claude-3-5-sonnet").unwrap();
        assert_eq!(s.kind, BackendKind::ClaudeCli);
        assert_eq!(s.id, "claude-3-5-sonnet");
        assert!(s.path.is_none());
    }

    #[test]
    fn parse_spec_rejects_unknown_kind() {
        let err = parse_spec("magic:foo").unwrap_err();
        assert!(matches!(err, SpecError::UnknownKind(k) if k == "magic"));
    }

    #[test]
    fn parse_spec_rejects_gguf_without_path() {
        let err = parse_spec("gguf:foo").unwrap_err();
        assert!(matches!(err, SpecError::MissingPath));
    }

    #[test]
    fn parse_spec_rejects_empty_id() {
        let err = parse_spec("gguf:@/path").unwrap_err();
        assert!(matches!(err, SpecError::EmptyId));
    }

    #[test]
    fn parse_spec_rejects_empty_path() {
        let err = parse_spec("gguf:foo@").unwrap_err();
        assert!(matches!(err, SpecError::EmptyPath));
    }

    #[test]
    fn parse_spec_rejects_claude_p_with_path() {
        let err = parse_spec("claude-p:claude-3-5-sonnet@anything").unwrap_err();
        assert!(matches!(err, SpecError::UnexpectedPath { kind } if kind == "claude-p"));
    }

    #[test]
    fn parse_spec_rejects_empty_kind() {
        let err = parse_spec(":foo").unwrap_err();
        assert!(matches!(err, SpecError::EmptyKind));
    }

    #[test]
    fn parse_spec_rejects_malformed_no_colon() {
        let err = parse_spec("").unwrap_err();
        assert!(matches!(err, SpecError::Malformed(s) if s.is_empty()));
    }

    #[test]
    fn parse_spec_trims_whitespace() {
        let s = parse_spec("  gguf:foo@/p  ").unwrap();
        assert_eq!(s.kind, BackendKind::Gguf);
        assert_eq!(s.id, "foo");
        assert_eq!(s.path.as_deref(), Some(std::path::Path::new("/p")));
    }

    #[test]
    fn parse_spec_rejects_claude_p_empty_id() {
        let err = parse_spec("claude-p:").unwrap_err();
        assert!(matches!(err, SpecError::EmptyId));
    }
}
