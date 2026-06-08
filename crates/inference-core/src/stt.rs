//! STT engine layer used by the dev-only HTTP sidecar.
//!
//! Replaces the pre-M4 `whisper_rs`-backed `SttBackend` trait. One
//! implementation ships here:
//!   * [`StubEngine`] — canned text for `lirevo-cli` smoke tests and CI
//!     runs without real weights. Used when `SIDECAR_STT_BACKEND=stub`
//!     (or unset). Real STT now runs in-process in the Tauri host via
//!     parakeet-cpp; the sidecar is stub-only.
//!
//! The wire protocol (`/v1/stt`: WAV in → `Transcript` JSON out) is unchanged
//! from the pre-M4 sidecar so existing CLI tooling keeps working.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Serialize;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::backend::ModelInfo;

const STUB_LOCK_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Debug, Error)]
pub enum SttError {
    #[error("audio decode failed: {0}")]
    AudioDecode(String),
    #[error("unsupported audio: {0}")]
    AudioUnsupported(String),
    #[error("model not loaded")]
    ModelNotLoaded,
    #[error("backend busy (mutex timeout)")]
    Busy,
    #[error("backend error: {0}")]
    Backend(String),
    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Default)]
pub struct SttOptions {
    pub language: Option<String>,
    pub want_segments: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub start_ms: u32,
    pub end_ms: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Transcript {
    pub text: String,
    pub language: String,
    pub duration_ms: u32,
    pub processing_ms: u32,
    pub model: String,
    pub backend: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<Segment>>,
}

#[async_trait]
pub trait SttEngine: Send + Sync + 'static {
    /// Transcribe a buffer of mono f32 samples at the given sample rate.
    async fn transcribe(
        &self,
        samples: Vec<f32>,
        sample_rate: u32,
        opts: SttOptions,
    ) -> Result<Transcript, SttError>;
    fn model_info(&self) -> ModelInfo;
}

pub type SttEngineHandle = Arc<dyn SttEngine>;

/// Canned-text STT for sidecar smoke tests. Mirrors the pre-M4
/// `StubBackend` output verbatim (`"[stub] N samples"`) so existing
/// `lirevo-cli` smoke tests keep passing without changes.
pub struct StubEngine {
    sleep: Duration,
    lock: Arc<Mutex<()>>,
}

impl StubEngine {
    #[must_use]
    pub fn new() -> Self {
        let ms = std::env::var("SIDECAR_STUB_SLEEP_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        Self {
            sleep: Duration::from_millis(ms),
            lock: Arc::new(Mutex::new(())),
        }
    }
}

impl Default for StubEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SttEngine for StubEngine {
    async fn transcribe(
        &self,
        samples: Vec<f32>,
        sample_rate: u32,
        _opts: SttOptions,
    ) -> Result<Transcript, SttError> {
        let started = Instant::now();
        let _guard = tokio::time::timeout(STUB_LOCK_TIMEOUT, self.lock.lock())
            .await
            .map_err(|_| SttError::Busy)?;
        if !self.sleep.is_zero() {
            tokio::time::sleep(self.sleep).await;
        }
        #[allow(clippy::cast_possible_truncation)]
        let duration_ms =
            u32::try_from(samples.len() * 1000 / sample_rate.max(1) as usize).unwrap_or(u32::MAX);
        Ok(Transcript {
            text: format!("[stub] {} samples", samples.len()),
            language: "stub".to_string(),
            duration_ms,
            processing_ms: u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX),
            model: "stub".to_string(),
            backend: "stub",
            segments: None,
        })
    }

    fn model_info(&self) -> ModelInfo {
        ModelInfo {
            id: "stub".to_string(),
            kind: "stt",
            backend: "stub",
            path: PathBuf::from("(none)"),
            coreml: false,
            loaded: true,
            ctx_size: None,
        }
    }
}
