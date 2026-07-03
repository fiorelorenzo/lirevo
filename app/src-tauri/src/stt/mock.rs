//! Mock STT handle for dev/CI without real weights. Compiled in under
//! `cfg(test)` or the `test-stt` feature; the loader routes here when
//! `LIREVO_DEV_USE_MOCK_STT=1` (debug builds).

use super::types::{SttError, SttOptions, Transcript};

pub struct MockModel {
    canned_text: String,
}

impl MockModel {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            canned_text: text.into(),
        }
    }

    /// Ignores audio content; returns the canned transcript.
    pub fn transcribe(
        &mut self,
        _audio: &[f32],
        _sample_rate: u32,
        _opts: &SttOptions,
    ) -> Result<Transcript, SttError> {
        Ok(Transcript {
            text: self.canned_text.clone(),
        })
    }
}
