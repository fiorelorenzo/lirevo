//! Resample arbitrary-rate mono f32 to 16 kHz mono f32 using rubato.

use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

use crate::AudioError;

const TARGET_RATE: u32 = 16_000;

pub(crate) fn to_16k(samples: &[f32], src_rate: u32) -> Result<Vec<f32>, AudioError> {
    if src_rate == TARGET_RATE {
        return Ok(samples.to_vec());
    }

    let mut resampler = Fft::<f32>::new(
        src_rate as usize,
        TARGET_RATE as usize,
        1024,
        2,
        1,
        FixedSync::Both,
    )
    .map_err(|e| AudioError::Internal(format!("resample init: {e}")))?;

    let input_frames = samples.len();
    let input = InterleavedSlice::new(samples, 1, input_frames)
        .map_err(|e| AudioError::Internal(format!("resample input adapter: {e}")))?;

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let out_frames = resampler.process_all_needed_output_len(input_frames);
    let mut out_data = vec![0.0_f32; out_frames];
    let mut output = InterleavedSlice::new_mut(&mut out_data, 1, out_frames)
        .map_err(|e| AudioError::Internal(format!("resample output adapter: {e}")))?;

    let (_n_in, n_out) = resampler
        .process_all_into_buffer(&input, &mut output, input_frames, None)
        .map_err(|e| AudioError::Internal(format!("resample: {e}")))?;

    out_data.truncate(n_out);
    Ok(out_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_at_target_rate() {
        let s: Vec<f32> = vec![0.5; 1600];
        let out = to_16k(&s, 16_000).unwrap();
        assert_eq!(out, s);
    }

    #[test]
    fn downsample_44100_to_16k_within_5pct() {
        let s: Vec<f32> = vec![0.0; 44_100];
        let out = to_16k(&s, 44_100).unwrap();
        let lo = 16_000 * 95 / 100;
        let hi = 16_000 * 105 / 100;
        assert!(
            out.len() >= lo && out.len() <= hi,
            "got {} samples, expected ~16000",
            out.len()
        );
    }
}
