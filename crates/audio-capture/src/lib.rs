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

pub use recorder::{Recorder, RecorderConfig, Recording};

use thiserror::Error;

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
}
