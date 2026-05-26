//! Mock STT handle for fast dev iteration and CI without real ONNX weights.
//!
//! Compiled in when the `test-stt` Cargo feature is enabled or under
//! `cfg(test)`. The loader (`super::load`) routes to this when
//! `LIREVO_DEV_USE_MOCK_STT=1` is set in the environment (debug builds only).

use audiopipe::{TranscribeOptions, TranscribeResult};

/// Synchronous STT mock. Returns a canned transcript regardless of input;
/// the audio buffer is consumed only to compute a plausible
/// `segments[0].end_secs` so downstream code that inspects timings stays
/// happy.
pub struct MockModel {
    canned_text: String,
}

impl MockModel {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self { canned_text: text.into() }
    }

    /// API-compatible with [`audiopipe::Model::transcribe_with_sample_rate`]
    /// — ignores audio content, returns the canned text, and times one
    /// fake segment spanning the duration the buffer implies.
    pub fn transcribe(
        &mut self,
        audio: &[f32],
        sample_rate: u32,
        _opts: TranscribeOptions,
    ) -> Result<TranscribeResult, audiopipe::Error> {
        let secs = if sample_rate == 0 {
            0.0
        } else {
            audio.len() as f64 / f64::from(sample_rate)
        };
        Ok(TranscribeResult {
            text: self.canned_text.clone(),
            segments: vec![audiopipe::Segment {
                start_secs: 0.0,
                end_secs: secs,
                text: self.canned_text.clone(),
            }],
        })
    }
}
