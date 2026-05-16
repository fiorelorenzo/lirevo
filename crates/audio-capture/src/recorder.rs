//! Synchronous Recorder using cpal. Captures samples into an in-memory ring
//! buffer at the device's native rate. Resampling to 16 kHz happens at `stop()`.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::Stream;
use tracing::warn;

use crate::device;
use crate::{to_mono, AudioError};

#[derive(Debug, Clone)]
pub struct RecorderConfig {
    pub device_name: Option<String>,
    pub max_duration_secs: u32,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self { device_name: None, max_duration_secs: 60 }
    }
}

pub struct Recording {
    pub samples: Vec<f32>,
    pub duration_ms: u32,
    pub device_label: String,
}

pub struct Recorder {
    cfg: RecorderConfig,
    state: Option<Active>,
}

struct Active {
    stream: Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    source_rate: u32,
    device_label: String,
}

impl Recorder {
    pub fn new(cfg: RecorderConfig) -> Result<Self, AudioError> {
        // Eagerly resolve so configuration errors surface at construction.
        let _ = device::resolve(cfg.device_name.as_deref())?;
        Ok(Self { cfg, state: None })
    }

    pub fn start(&mut self) -> Result<(), AudioError> {
        if self.state.is_some() {
            return Err(AudioError::AlreadyRecording);
        }
        let dev = device::resolve(self.cfg.device_name.as_deref())?;
        let channels = dev.config.channels();
        let source_rate = dev.config.sample_rate();
        let sample_format = dev.config.sample_format();
        let stream_cfg: cpal::StreamConfig = dev.config.clone().into();

        let cap = usize::try_from(u64::from(self.cfg.max_duration_secs) * u64::from(source_rate))
            .unwrap_or(usize::MAX);
        let buf = Arc::new(Mutex::new(Vec::<f32>::with_capacity(cap)));
        let buf_cb = buf.clone();
        let max_samples = cap;

        let err_fn = |err| tracing::error!(?err, "cpal stream error");

        let stream = match sample_format {
            cpal::SampleFormat::F32 => dev
                .device
                .build_input_stream(
                    &stream_cfg,
                    move |data: &[f32], _| {
                        let mono = to_mono(data.to_vec(), channels);
                        let mut g = buf_cb.lock().expect("buf lock");
                        let take = max_samples.saturating_sub(g.len());
                        if take == 0 {
                            warn!("max_duration_secs reached; dropping further audio");
                            return;
                        }
                        let n = mono.len().min(take);
                        g.extend_from_slice(&mono[..n]);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| AudioError::Cpal(format!("build f32 stream: {e}")))?,
            cpal::SampleFormat::I16 => dev
                .device
                .build_input_stream(
                    &stream_cfg,
                    move |data: &[i16], _| {
                        let f32_buf: Vec<f32> = data
                            .iter()
                            .map(|s| f32::from(*s) / f32::from(i16::MAX))
                            .collect();
                        let mono = to_mono(f32_buf, channels);
                        let mut g = buf_cb.lock().expect("buf lock");
                        let take = max_samples.saturating_sub(g.len());
                        if take == 0 {
                            warn!("max_duration_secs reached; dropping further audio");
                            return;
                        }
                        let n = mono.len().min(take);
                        g.extend_from_slice(&mono[..n]);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| AudioError::Cpal(format!("build i16 stream: {e}")))?,
            other => {
                return Err(AudioError::UnsupportedConfig(format!(
                    "sample format {other:?} not supported (need F32 or I16)"
                )));
            }
        };

        stream
            .play()
            .map_err(|e| AudioError::Cpal(format!("stream play: {e}")))?;

        self.state = Some(Active { stream, buf, source_rate, device_label: dev.label });
        Ok(())
    }

    pub fn stop(&mut self) -> Result<Recording, AudioError> {
        let active = self.state.take().ok_or(AudioError::NotRecording)?;
        let Active { stream, buf, source_rate, device_label } = active;
        drop(stream);

        let mono = Arc::try_unwrap(buf)
            .map_err(|_| AudioError::Internal("ring buffer still has live refs".into()))?
            .into_inner()
            .map_err(|e| AudioError::Internal(format!("buf mutex poisoned: {e}")))?;

        let samples = if source_rate == 16_000 {
            mono
        } else {
            crate::resample::to_16k(&mono, source_rate)?
        };

        let duration_ms = u32::try_from(samples.len() * 1000 / 16_000).unwrap_or(u32::MAX);
        Ok(Recording { samples, duration_ms, device_label })
    }
}
