//! Minimal WAV decoder used by the dev-only HTTP sidecar.
//!
//! Returns mono f32 samples plus the WAV's source sample rate. The in-process
//! STT backend handles resampling to 16 kHz internally. Pre-M4 this file also
//! held a rubato-based resampler — gone now that resampling is owned by the STT
//! engine.

use std::io::Cursor;

use crate::stt::SttError;

const MIN_RATE: u32 = 8_000;
const MAX_RATE: u32 = 96_000;
const MAX_CHANNELS: u16 = 2;

/// Decode a WAV byte slice into `(mono_f32_samples, sample_rate_hz)`.
pub fn decode_wav(bytes: &[u8]) -> Result<(Vec<f32>, u32), SttError> {
    let cursor = Cursor::new(bytes);
    let reader = hound::WavReader::new(cursor)
        .map_err(|e| SttError::AudioDecode(e.to_string()))?;
    let spec = reader.spec();

    if spec.sample_rate < MIN_RATE || spec.sample_rate > MAX_RATE {
        return Err(SttError::AudioUnsupported(format!(
            "sample rate {} outside [{MIN_RATE}, {MAX_RATE}]",
            spec.sample_rate
        )));
    }
    if spec.channels == 0 || spec.channels > MAX_CHANNELS {
        return Err(SttError::AudioUnsupported(format!(
            "channels {} not in [1, {MAX_CHANNELS}]",
            spec.channels
        )));
    }

    let interleaved = decode_samples(reader)?;
    if interleaved.is_empty() {
        return Err(SttError::AudioUnsupported("zero samples".to_string()));
    }

    let mono = to_mono(interleaved, spec.channels);
    Ok((mono, spec.sample_rate))
}

fn decode_samples(reader: hound::WavReader<Cursor<&[u8]>>) -> Result<Vec<f32>, SttError> {
    let spec = reader.spec();
    match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<Result<Vec<f32>, _>>()
            .map_err(|e| SttError::AudioDecode(e.to_string())),
        hound::SampleFormat::Int => {
            let max = match spec.bits_per_sample {
                8 => f32::from(i8::MAX),
                16 => f32::from(i16::MAX),
                24 => 8_388_607.0,
                32 => {
                    #[allow(clippy::cast_precision_loss)]
                    let m = i32::MAX as f32;
                    m
                }
                bits => {
                    return Err(SttError::AudioUnsupported(format!(
                        "unsupported int bits: {bits}"
                    )))
                }
            };
            reader
                .into_samples::<i32>()
                .map(|s| {
                    s.map(|v| {
                        #[allow(clippy::cast_precision_loss)]
                        let sample = v as f32 / max;
                        sample
                    })
                })
                .collect::<Result<Vec<f32>, _>>()
                .map_err(|e| SttError::AudioDecode(e.to_string()))
        }
    }
}

fn to_mono(interleaved: Vec<f32>, channels: u16) -> Vec<f32> {
    if channels == 1 {
        return interleaved;
    }
    let ch = usize::from(channels);
    let mut out = Vec::with_capacity(interleaved.len() / ch);
    let mut i = 0;
    while i + ch <= interleaved.len() {
        let mut sum = 0.0_f32;
        for k in 0..ch {
            sum += interleaved[i + k];
        }
        #[allow(clippy::cast_precision_loss)]
        out.push(sum / ch as f32);
        i += ch;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::io::Cursor;

    fn synth_wav_i16(samples: &[i16], channels: u16, rate: u32) -> Vec<u8> {
        let spec = WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut buf = Vec::new();
        {
            let mut writer = WavWriter::new(Cursor::new(&mut buf), spec).unwrap();
            for s in samples {
                writer.write_sample(*s).unwrap();
            }
            writer.finalize().unwrap();
        }
        buf
    }

    #[test]
    fn decodes_mono_16k_i16_keeps_source_rate() {
        #[allow(clippy::cast_possible_truncation)]
        let s: Vec<i16> = (0..1000).map(|i| (i * 32) as i16).collect();
        let wav = synth_wav_i16(&s, 1, 16_000);
        let (out, rate) = decode_wav(&wav).unwrap();
        assert_eq!(out.len(), 1000);
        assert_eq!(rate, 16_000);
        assert!(out[0].abs() < 0.001);
    }

    #[test]
    fn decodes_stereo_44100_and_mixes_to_mono_preserving_rate() {
        // 100 stereo frames = 200 interleaved samples at 44.1 kHz.
        let s: Vec<i16> = (0..200).map(|i| if i % 2 == 0 { 32767 } else { -32768 }).collect();
        let wav = synth_wav_i16(&s, 2, 44_100);
        let (out, rate) = decode_wav(&wav).unwrap();
        assert_eq!(out.len(), 100);
        assert_eq!(rate, 44_100);
        for v in &out {
            assert!(v.abs() < 0.01, "got {v}");
        }
    }

    #[test]
    fn rejects_out_of_range_sample_rate() {
        let s: Vec<i16> = vec![0; 10];
        let wav = synth_wav_i16(&s, 1, 4_000);
        let err = decode_wav(&wav).unwrap_err();
        match err {
            SttError::AudioUnsupported(_) => {}
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn rejects_garbage_bytes_as_bad_audio() {
        let err = decode_wav(b"not a wav file").unwrap_err();
        match err {
            SttError::AudioDecode(_) => {}
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
