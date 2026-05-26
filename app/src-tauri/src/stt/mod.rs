//! STT model layer for the Tauri host.
//!
//! Built on top of `audiopipe::Model`. Exposes:
//!   * A static catalog of supported model ids (see [`catalog`]).
//!   * [`SttModelHandle`] — thin enum that unifies the real audiopipe model
//!     with the test-only [`mock::MockModel`] so the dictation pipeline can
//!     call `transcribe` uniformly.
//!   * [`load`] — loader that returns either a [`LoadOutcome::Ready`] handle
//!     or a [`LoadOutcome::Downloading`] status when the HF cache is empty
//!     (a background download is spawned in that case).

pub mod catalog;

#[cfg(any(test, feature = "test-stt"))]
pub mod mock;

use audiopipe::{TranscribeOptions, TranscribeResult};

use crate::AppError;

/// Environment variable that routes the loader to [`mock::MockModel`]
/// instead of touching audiopipe. Honoured only in debug builds —
/// see the runtime check inside [`load`].
pub const ENV_USE_MOCK_STT: &str = "LIREVO_DEV_USE_MOCK_STT";

/// Outcome of a load attempt.
pub enum LoadOutcome {
    /// Model is ready for inference.
    Ready(SttModelHandle),
    /// Weights weren't in the HF cache yet. A background thread is now
    /// downloading them; the caller should surface a "downloading" status
    /// to the UI and retry the load when notified (M4 wizard handles this).
    Downloading {
        /// The audiopipe model name being fetched. Useful for UI copy.
        audiopipe_name: String,
    },
}

/// Unified STT handle. Test-time mock branch is feature-gated so a release
/// build can't accidentally ship the canned-text path.
pub enum SttModelHandle {
    Real(audiopipe::Model),
    #[cfg(any(test, feature = "test-stt"))]
    Mock(mock::MockModel),
}

impl SttModelHandle {
    /// Transcribe a buffer of mono f32 samples at `sample_rate` Hz.
    ///
    /// Synchronous and CPU-heavy — callers should run this on a blocking
    /// pool (`tokio::task::spawn_blocking` for async contexts).
    pub fn transcribe(
        &mut self,
        audio: &[f32],
        sample_rate: u32,
        opts: TranscribeOptions,
    ) -> Result<TranscribeResult, audiopipe::Error> {
        match self {
            Self::Real(m) => m.transcribe_with_sample_rate(audio, sample_rate, opts),
            #[cfg(any(test, feature = "test-stt"))]
            Self::Mock(m) => m.transcribe(audio, sample_rate, opts),
        }
    }
}

/// Load the model identified by `catalog_id`.
///
/// * Honours `LIREVO_DEV_USE_MOCK_STT=1` in debug builds — returns a
///   canned-text [`mock::MockModel`] without touching audiopipe at all.
/// * Otherwise tries the HF cache first (`from_pretrained_cache_only`);
///   on [`audiopipe::Error::ModelNotCached`] spawns a background download
///   and returns [`LoadOutcome::Downloading`] so the caller doesn't block
///   on network I/O.
/// * Unknown catalog ids are rejected with [`AppError::Internal`] rather
///   than silently falling back to the default — a stale settings id means
///   the UI is desynced from the catalog and the user deserves to know.
pub fn load(catalog_id: &str) -> Result<LoadOutcome, AppError> {
    if cfg!(debug_assertions) && std::env::var(ENV_USE_MOCK_STT).is_ok() {
        #[cfg(any(test, feature = "test-stt"))]
        {
            tracing::warn!(
                "{ENV_USE_MOCK_STT}=1 — routing STT to MockModel (debug + test-stt feature)"
            );
            return Ok(LoadOutcome::Ready(SttModelHandle::Mock(
                mock::MockModel::new("hello world from mock"),
            )));
        }
        #[cfg(not(any(test, feature = "test-stt")))]
        {
            tracing::warn!(
                "{ENV_USE_MOCK_STT}=1 set but the `test-stt` Cargo feature is off; \
                 ignoring and loading the real model"
            );
        }
    }

    let metadata = catalog::model_metadata(catalog_id).ok_or_else(|| {
        AppError::Internal(format!(
            "STT catalog id '{catalog_id}' not recognised; aborting load"
        ))
    })?;
    if matches!(
        metadata.feature_requirement,
        catalog::FeatureRequirement::AudiopipeWhisperFeature
    ) && !cfg!(feature = "audiopipe-whisper")
    {
        // Phase-2 baseline: the `whisper` feature on audiopipe is OFF.
        // Surface a clear error instead of letting audiopipe fail with
        // "unknown model 'whisper-...'" deep in its match arms.
        return Err(AppError::Internal(format!(
            "STT model '{catalog_id}' requires the `whisper` audiopipe feature, \
             which is disabled in this build"
        )));
    }

    let audiopipe_name = catalog::audiopipe_name_for_platform(catalog_id).to_string();
    match audiopipe::Model::from_pretrained_cache_only(&audiopipe_name) {
        Ok(model) => Ok(LoadOutcome::Ready(SttModelHandle::Real(model))),
        Err(e) if e.is_model_not_cached() => {
            tracing::info!(
                model = %audiopipe_name,
                "STT weights not cached; spawning background HF download"
            );
            audiopipe::Model::spawn_pretrained_download(audiopipe_name.clone());
            Ok(LoadOutcome::Downloading { audiopipe_name })
        }
        Err(e) => Err(AppError::Internal(format!(
            "audiopipe load failed for '{audiopipe_name}': {e}"
        ))),
    }
}
