//! STT model layer for the Tauri host.
//!
//! Built on top of `parakeet_cpp`. Exposes:
//!   * A static catalog of supported model ids (see [`catalog`]).
//!   * [`SttModelHandle`] — thin enum that unifies the real parakeet_cpp model
//!     with the test-only [`mock::MockModel`] so the dictation pipeline can
//!     call `transcribe` uniformly.
//!   * [`load`] — loader that returns either a [`LoadOutcome::Ready`] handle
//!     or [`LoadOutcome::NeedsDownload`] when the GGUF isn't on disk yet.

mod types;
pub use types::{SttError, SttOptions, Transcript};

pub mod catalog;
pub use catalog::STT_GGUF_FILENAME;

#[cfg(any(test, feature = "test-stt"))]
pub mod mock;

use std::path::{Path, PathBuf};

use crate::AppError;

/// Environment variable that routes the loader to a mock handle. Debug only.
pub const ENV_USE_MOCK_STT: &str = "LIREVO_DEV_USE_MOCK_STT";

/// Outcome of [`load`].
pub enum LoadOutcome {
    Ready(SttModelHandle),
    /// The GGUF wasn't on disk yet; caller should download then retry.
    NeedsDownload,
}

pub enum SttModelHandle {
    Real(parakeet_cpp::Model),
    #[cfg(any(test, feature = "test-stt"))]
    Mock(mock::MockModel),
}

impl SttModelHandle {
    /// Transcribe mono f32 samples at `sample_rate` Hz. Synchronous + heavy —
    /// run on a blocking pool from async contexts.
    pub fn transcribe(
        &mut self,
        audio: &[f32],
        sample_rate: u32,
        opts: &SttOptions,
    ) -> Result<Transcript, SttError> {
        match self {
            Self::Real(m) => {
                let popts = parakeet_cpp::TranscribeOptions {
                    language: opts.language.clone(),
                    word_timestamps: false,
                };
                let t = m
                    .transcribe(audio, sample_rate, &popts)
                    .map_err(|e| SttError::Backend(e.to_string()))?;
                Ok(Transcript { text: t.text })
            }
            #[cfg(any(test, feature = "test-stt"))]
            Self::Mock(m) => m.transcribe(audio, sample_rate, opts),
        }
    }

    /// Name of the compute backend the (process-global) ggml backend resolved
    /// to for this model — e.g. `"MTL0"` for Metal, `"cpu"` for the CPU
    /// fallback. Only meaningful for a real model (the ggml backend is created
    /// lazily on load); the mock returns an empty string.
    #[must_use]
    pub fn backend_name(&self) -> String {
        match self {
            Self::Real(m) => m.backend_name(),
            #[cfg(any(test, feature = "test-stt"))]
            Self::Mock(_) => String::new(),
        }
    }
}

/// Absolute path of the shipped STT GGUF inside the app's models dir.
pub fn gguf_path(models_dir: &Path) -> PathBuf {
    models_dir.join(STT_GGUF_FILENAME)
}

/// Load the STT model. `catalog_id` must be the shipped model id; `models_dir`
/// is the app data models directory.
pub fn load(catalog_id: &str, models_dir: &Path) -> Result<LoadOutcome, AppError> {
    if cfg!(debug_assertions) && std::env::var(ENV_USE_MOCK_STT).is_ok() {
        #[cfg(any(test, feature = "test-stt"))]
        {
            tracing::warn!("{ENV_USE_MOCK_STT}=1 — routing STT to MockModel");
            return Ok(LoadOutcome::Ready(SttModelHandle::Mock(mock::MockModel::new(
                "hello world from mock",
            ))));
        }
    }

    if catalog::model_metadata(catalog_id).is_none() {
        return Err(AppError::Internal(format!(
            "STT catalog id '{catalog_id}' not recognised"
        )));
    }

    let path = gguf_path(models_dir);
    if !path.exists() {
        tracing::info!(?path, "STT GGUF not on disk; caller should download");
        return Ok(LoadOutcome::NeedsDownload);
    }

    let model = parakeet_cpp::Model::load(&path)
        .map_err(|e| AppError::Internal(format!("parakeet-cpp load failed: {e}")))?;
    Ok(LoadOutcome::Ready(SttModelHandle::Real(model)))
}
