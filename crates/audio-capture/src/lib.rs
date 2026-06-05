//! Microphone capture for the dictation app.
//!
//! Provides a synchronous `Recorder` that grabs samples from the system default
//! input device (or a named override) into an in-memory buffer, then on `stop()`
//! returns the captured audio resampled to 16 kHz mono f32 — the format expected
//! by the sidecar `/v1/stt` endpoint.

#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

mod device;
mod recorder;
mod resample;
mod smart_input;

pub use device::{list_inputs, InputDeviceInfo};
pub use recorder::{Recorder, RecorderConfig, Recording};
pub use smart_input::{
    builtin_input_name, choose_input_device, input_is_bluetooth, output_is_active, InputChoice,
};

use thiserror::Error;

/// Return the OS-visible name of the default input device (e.g. `MacBook Pro
/// Microphone`). Used by the wizard to show the user which device the mic
/// test is sampling from.
pub fn default_input_device_label() -> Result<String, AudioError> {
    Ok(device::resolve(None)?.label)
}

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("no default input device")]
    NoDevice,
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    #[error("unsupported config: {0}")]
    UnsupportedConfig(String),
    #[error("cpal error: {0}")]
    Cpal(String),
    #[error("permission denied (microphone TCC)")]
    PermissionDenied,
    #[error("not recording")]
    NotRecording,
    #[error("already recording")]
    AlreadyRecording,
    #[error("internal: {0}")]
    Internal(String),
}

/// Encode 16 kHz mono f32 samples to a PCM16 WAV byte vector.
/// Pure helper used by both the recorder and the prototype binary.
#[must_use]
pub fn samples_to_wav(samples: &[f32]) -> Vec<u8> {
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::io::Cursor;

    let spec = WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut buf = Vec::new();
    {
        let mut w = WavWriter::new(Cursor::new(&mut buf), spec).expect("wav writer");
        for s in samples {
            #[allow(clippy::cast_possible_truncation)]
            let v = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
            w.write_sample(v).expect("write sample");
        }
        w.finalize().expect("wav finalize");
    }
    buf
}

/// Average interleaved multi-channel samples into mono.
/// Returns the same buffer if `channels == 1`.
#[must_use]
pub fn to_mono(interleaved: Vec<f32>, channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return interleaved;
    }
    let mut out = Vec::with_capacity(interleaved.len() / usize::from(channels));
    to_mono_into_f32(&interleaved, channels, &mut out);
    out
}

/// Streaming variant of `to_mono` that writes into a caller-provided
/// scratch `Vec<f32>` instead of allocating. The cpal callback runs on the
/// realtime audio thread ~100 times a second; allocating two `Vec`s per
/// frame (one for `data.to_vec()`, one for the output of `to_mono`) thrashes
/// the audio-thread allocator and contributes directly to dropouts.
pub fn to_mono_into_f32(interleaved: &[f32], channels: u16, out: &mut Vec<f32>) {
    out.clear();
    if channels <= 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    let ch = usize::from(channels);
    #[allow(clippy::cast_precision_loss)]
    let inv = 1.0_f32 / ch as f32;
    let mut i = 0;
    while i + ch <= interleaved.len() {
        let mut sum = 0.0_f32;
        for k in 0..ch {
            sum += interleaved[i + k];
        }
        out.push(sum * inv);
        i += ch;
    }
}

/// Same as `to_mono_into_f32` but takes raw i16 samples (cpal's I16 stream
/// format) and decodes-plus-mixes in one pass, so the i16 path needs zero
/// allocations per callback (was previously `data.iter().map(...).collect()`
/// to convert to f32, then `to_mono` on top of that — two Vec allocs).
pub fn to_mono_into_i16(interleaved: &[i16], channels: u16, out: &mut Vec<f32>) {
    out.clear();
    let denom = 1.0_f32 / f32::from(i16::MAX);
    if channels <= 1 {
        out.extend(interleaved.iter().map(|s| f32::from(*s) * denom));
        return;
    }
    let ch = usize::from(channels);
    #[allow(clippy::cast_precision_loss)]
    let inv = 1.0_f32 / ch as f32;
    let mut i = 0;
    while i + ch <= interleaved.len() {
        let mut sum = 0.0_f32;
        for k in 0..ch {
            sum += f32::from(interleaved[i + k]) * denom;
        }
        out.push(sum * inv);
        i += ch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_to_wav_roundtrips_via_hound() {
        let s: Vec<f32> = (0..1600u16).map(|i| (f32::from(i) * 0.001).sin()).collect();
        let wav = samples_to_wav(&s);
        let mut reader = hound::WavReader::new(std::io::Cursor::new(&wav)).expect("read wav");
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.bits_per_sample, 16);
        let samples: Vec<i16> = reader
            .samples::<i16>()
            .collect::<Result<_, _>>()
            .expect("samples");
        assert_eq!(samples.len(), 1600);
    }

    #[test]
    fn samples_to_wav_clamps_overflows() {
        let s = vec![2.0_f32, -2.0_f32, 0.5_f32];
        let wav = samples_to_wav(&s);
        let mut reader = hound::WavReader::new(std::io::Cursor::new(&wav)).expect("read");
        let samples: Vec<i16> = reader.samples::<i16>().collect::<Result<_, _>>().unwrap();
        assert_eq!(samples[0], i16::MAX);
        assert_eq!(samples[1], -i16::MAX);
        assert!(samples[2] > 16000 && samples[2] < 17000);
    }

    #[test]
    fn to_mono_passes_through_mono() {
        let s = vec![1.0_f32, -1.0, 0.5];
        let out = to_mono(s.clone(), 1);
        assert_eq!(out, s);
    }

    #[test]
    fn to_mono_averages_stereo_channels() {
        let s: Vec<f32> = (0..200).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let out = to_mono(s, 2);
        assert_eq!(out.len(), 100);
        for v in &out {
            assert!(v.abs() < 0.001, "expected ~0, got {v}");
        }
    }

    #[test]
    fn to_mono_handles_trailing_partial_frame() {
        let s = vec![1.0_f32, 0.0, 1.0, 0.0, 1.0];
        let out = to_mono(s, 2);
        assert_eq!(out.len(), 2);
    }
}
