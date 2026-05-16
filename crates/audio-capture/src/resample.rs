//! Resample helper. Real impl lands in T6.

use crate::AudioError;

pub(crate) fn to_16k(_samples: &[f32], _src_rate: u32) -> Result<Vec<f32>, AudioError> {
    Err(AudioError::Internal("resample not implemented yet (T6)".into()))
}
