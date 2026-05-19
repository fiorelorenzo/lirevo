//! Claude CLI backend: shells out to `claude -p '<combined>'` with a timeout.
//!
//! `claude -p` reads a single prompt argument and prints the model's
//! reply to stdout. The CLI does not expose a separate system-prompt flag,
//! so we concatenate `system_prompt + "\n\n---\n\n" + transcript` into one
//! user-side message — the model receives it as plain context.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::process::Command;
use tokio::time::timeout;

use super::{BackendError, BackendKind, EvalBackend, GenerateOut, GenerateReq};

pub struct ClaudeCliBackend {
    id: String,
    model: String,
    binary: String,
    timeout: Duration,
}

impl ClaudeCliBackend {
    #[must_use]
    pub fn new(id: String, model: String, binary: String, timeout: Duration) -> Self {
        Self {
            id,
            model,
            binary,
            timeout,
        }
    }

    #[must_use]
    pub fn with_defaults(id: String, model: String) -> Self {
        Self::new(id, model, "claude".into(), Duration::from_secs(60))
    }
}

pub(crate) fn combine_prompt(system: &str, transcript: &str) -> String {
    format!("{system}\n\n---\n\n{transcript}")
}

#[async_trait]
impl EvalBackend for ClaudeCliBackend {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> BackendKind {
        BackendKind::ClaudeCli
    }

    async fn generate(&self, req: GenerateReq) -> Result<GenerateOut, BackendError> {
        let combined = combine_prompt(&req.system_prompt, &req.transcript);
        let mut cmd = Command::new(&self.binary);
        cmd.arg("-p")
            .arg(&combined)
            .arg("--model")
            .arg(&self.model)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let start = Instant::now();
        let child = cmd.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => BackendError::ExecutableMissing {
                name: self.binary.clone(),
            },
            _ => BackendError::Process(e.to_string()),
        })?;

        let output = timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| BackendError::Timeout)?
            .map_err(|e| BackendError::Process(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BackendError::Process(format!(
                "exit {}: {stderr}",
                output.status.code().unwrap_or(-1)
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(GenerateOut {
            text,
            latency_ms: elapsed_ms,
            // claude -p stdout doesn't carry token counts; report 0 to keep
            // the shape consistent with GgufBackend. Task 11's report writer
            // distinguishes "0 reported" from "actually 0".
            prompt_tokens: 0,
            completion_tokens: 0,
            peak_rss_kb: None,
            from_warm_cache: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{combine_prompt, ClaudeCliBackend};
    use crate::backend::{BackendError, EvalBackend, GenerateReq};
    use std::time::Duration;

    /// We don't have `claude` in CI; verify the binary-not-found path is
    /// reported as ExecutableMissing rather than panicking.
    #[tokio::test]
    async fn missing_binary_returns_executable_missing() {
        let b = ClaudeCliBackend::new(
            "test".into(),
            "test-model".into(),
            "lda-eval-nonexistent-binary".into(),
            Duration::from_secs(5),
        );
        let req = GenerateReq {
            system_prompt: "sys".into(),
            transcript: "hi".into(),
            max_tokens: 4,
            temperature: 0.0,
        };
        let err = b.generate(req).await.expect_err("should fail");
        assert!(
            matches!(err, BackendError::ExecutableMissing { .. }),
            "got: {err:?}"
        );
    }

    /// Validate the prompt-combining helper.
    #[test]
    fn combine_prompt_concats_system_and_transcript() {
        let combined = combine_prompt("You are X.", "Hello!");
        assert!(combined.contains("You are X."));
        assert!(combined.contains("Hello!"));
        assert!(combined.find("You are X.").unwrap() < combined.find("Hello!").unwrap());
    }
}
