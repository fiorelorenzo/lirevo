//! Synchronous Recorder using cpal. Captures samples into an in-memory ring
//! buffer at the device's native rate. Resampling to 16 kHz happens at `stop()`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::Stream;
use tokio::sync::watch;
use tracing::warn;

use crate::device;
use crate::{to_mono_into_f32, to_mono_into_i16, AudioError};

/// Throttle interval for RMS level emission — roughly 33 Hz.
const LEVEL_EMIT_INTERVAL: Duration = Duration::from_millis(30);

/// Audio buffer sizes are typically 256-2048 frames; pre-allocate to the
/// upper end so the mono-mix scratch never reallocs on the audio thread
/// (which runs the cpal callback ~100×/sec).
const MONO_SCRATCH_CAP: usize = 4096;

/// Compute the root-mean-square of a slice of normalized samples (range
/// roughly `[-1.0, 1.0]`). Empty input yields `0.0`; the result is clamped
/// to `1.0` so out-of-range inputs cannot produce a level above unity.
#[must_use]
pub(crate) fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    #[allow(clippy::cast_precision_loss)]
    let mean_sq = sum_sq / samples.len() as f32;
    mean_sq.sqrt().min(1.0)
}

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
    level_tx: watch::Sender<f32>,
    level_rx: watch::Receiver<f32>,
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
        let (level_tx, level_rx) = watch::channel(0.0_f32);
        Ok(Self { cfg, state: None, level_tx, level_rx })
    }

    /// Subscribe to live RMS levels emitted while recording.
    ///
    /// The receiver yields a fresh value approximately every 30 ms during an
    /// active recording. Outside of an active recording, the value is the
    /// last level emitted (or `0.0` if recording has never started). The
    /// channel is preserved across `start`/`stop` cycles.
    #[must_use]
    pub fn level_rx(&self) -> watch::Receiver<f32> {
        self.level_rx.clone()
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
            cpal::SampleFormat::F32 => {
                let level_tx = self.level_tx.clone();
                let mut last_emit = Instant::now();
                let mut mono_scratch: Vec<f32> = Vec::with_capacity(MONO_SCRATCH_CAP);
                dev.device
                    .build_input_stream(
                        &stream_cfg,
                        move |data: &[f32], _| {
                            to_mono_into_f32(data, channels, &mut mono_scratch);
                            emit_level_if_due(&level_tx, &mut last_emit, &mono_scratch);
                            let mut g = buf_cb.lock().expect("buf lock");
                            let take = max_samples.saturating_sub(g.len());
                            if take == 0 {
                                warn!("max_duration_secs reached; dropping further audio");
                                return;
                            }
                            let n = mono_scratch.len().min(take);
                            g.extend_from_slice(&mono_scratch[..n]);
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| AudioError::Cpal(format!("build f32 stream: {e}")))?
            }
            cpal::SampleFormat::I16 => {
                let level_tx = self.level_tx.clone();
                let mut last_emit = Instant::now();
                let mut mono_scratch: Vec<f32> = Vec::with_capacity(MONO_SCRATCH_CAP);
                dev.device
                    .build_input_stream(
                        &stream_cfg,
                        move |data: &[i16], _| {
                            // Fused decode + mono-mix: was two separate
                            // allocations per callback (Vec<f32> for the
                            // decode pass, then Vec<f32> again from to_mono).
                            to_mono_into_i16(data, channels, &mut mono_scratch);
                            emit_level_if_due(&level_tx, &mut last_emit, &mono_scratch);
                            let mut g = buf_cb.lock().expect("buf lock");
                            let take = max_samples.saturating_sub(g.len());
                            if take == 0 {
                                warn!("max_duration_secs reached; dropping further audio");
                                return;
                            }
                            let n = mono_scratch.len().min(take);
                            g.extend_from_slice(&mono_scratch[..n]);
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| AudioError::Cpal(format!("build i16 stream: {e}")))?
            }
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

        // Clone-out rather than try_unwrap: peek_resampled_since may hold an
        // Arc clone on a streaming worker; we no longer have the single-owner
        // invariant that try_unwrap required.
        let mono = buf
            .lock()
            .map_err(|e| AudioError::Internal(format!("buf mutex poisoned: {e}")))?
            .clone();

        let samples = resample_to_16k_mono(&mono, source_rate)?;

        let duration_ms = u32::try_from(samples.len() * 1000 / 16_000).unwrap_or(u32::MAX);
        Ok(Recording { samples, duration_ms, device_label })
    }

    /// Read the currently-captured buffer, resample it to 16 kHz mono, and
    /// return the tail past `cursor` along with a new cursor for the next
    /// call.
    ///
    /// `cursor` is an index into the 16 kHz output stream — callers pass
    /// `0` on first call and the second tuple element on each subsequent
    /// call. The returned cursor equals the full resampled length and is
    /// monotonically non-decreasing across calls, so the concatenation of
    /// every returned slice (in order) reconstructs the same buffer
    /// `stop()` would yield up to the same moment.
    ///
    /// Resampling the full buffer each call (rather than a window) avoids
    /// rubato's stateful filter drift on overlapping chunks; the encoder
    /// cost dwarfs the resample cost, so this stays in budget.
    ///
    /// Returns `Err(NotRecording)` if the recorder hasn't been started.
    /// Returns an empty tail (and an unchanged cursor) if no new samples
    /// have accumulated since the last call.
    pub fn peek_resampled_since(
        &self,
        cursor: usize,
    ) -> Result<(Vec<f32>, usize), AudioError> {
        let active = self.state.as_ref().ok_or(AudioError::NotRecording)?;
        let snapshot = active
            .buf
            .lock()
            .map_err(|e| AudioError::Internal(format!("buf mutex poisoned: {e}")))?
            .clone();
        let resampled = resample_to_16k_mono(&snapshot, active.source_rate)?;
        let new_cursor = resampled.len();
        if cursor >= new_cursor {
            return Ok((Vec::new(), new_cursor));
        }
        let tail = resampled[cursor..].to_vec();
        Ok((tail, new_cursor))
    }
}

/// Resample arbitrary-rate mono samples to 16 kHz mono. Pass-through when
/// the source is already at 16 kHz. Pulled out of `stop()` so the same
/// contract is reused by `peek_resampled_since`.
fn resample_to_16k_mono(samples: &[f32], source_rate: u32) -> Result<Vec<f32>, AudioError> {
    if source_rate == 16_000 {
        Ok(samples.to_vec())
    } else {
        crate::resample::to_16k(samples, source_rate)
    }
}

/// Compute RMS over `samples` and publish on `level_tx`, but only if at
/// least `LEVEL_EMIT_INTERVAL` has elapsed since the previous emission. Send
/// errors (no live receivers) are intentionally ignored. The cpal callback
/// is a single producer so the `last_emit` cursor can be owned by the
/// closure (no Arc/Mutex needed).
fn emit_level_if_due(level_tx: &watch::Sender<f32>, last_emit: &mut Instant, samples: &[f32]) {
    let now = Instant::now();
    if now.duration_since(*last_emit) >= LEVEL_EMIT_INTERVAL {
        let level = rms(samples);
        let _ = level_tx.send(level);
        *last_emit = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_zero_for_empty_samples() {
        let r = rms(&[]);
        assert!(r.abs() < f32::EPSILON, "expected 0.0, got {r}");
    }

    #[test]
    fn rms_known_value() {
        let r = rms(&[1.0, -1.0, 1.0, -1.0]);
        assert!((r - 1.0).abs() < 1e-6, "expected 1.0, got {r}");
    }

    #[test]
    fn rms_clamped_to_one() {
        let r = rms(&[2.0, -2.0]);
        assert!((r - 1.0).abs() < 1e-6, "expected 1.0, got {r}");
    }

    /// Simulate the cpal callback writing into the shared buffer and
    /// assert that the concatenation of all incremental peeks equals what
    /// a final `stop`-equivalent drain would return. We test the pure
    /// resample helper + the cursor math without spinning up cpal so the
    /// test runs deterministically in CI.
    #[test]
    fn incremental_peeks_reconstruct_full_buffer() {
        let buf = Arc::new(Mutex::new(Vec::<f32>::new()));

        let push = |chunk: &[f32]| {
            buf.lock().unwrap().extend_from_slice(chunk);
        };

        // Helper that mirrors peek_resampled_since's contract on the
        // shared buffer; the recorder's method uses the same body
        // (snapshot → resample → slice past cursor).
        let peek = |cursor: usize| -> (Vec<f32>, usize) {
            let snapshot = buf.lock().unwrap().clone();
            let resampled = resample_to_16k_mono(&snapshot, 16_000).unwrap();
            let new_cursor = resampled.len();
            if cursor >= new_cursor {
                return (Vec::new(), new_cursor);
            }
            (resampled[cursor..].to_vec(), new_cursor)
        };

        push(&[0.1; 800]);
        let (a, c1) = peek(0);
        assert_eq!(a.len(), 800);
        assert_eq!(c1, 800);

        push(&[0.2; 1200]);
        let (b, c2) = peek(c1);
        assert_eq!(b.len(), 1200);
        assert_eq!(c2, 2000);

        // Empty interval — no new samples since last peek.
        let (empty, c3) = peek(c2);
        assert!(empty.is_empty());
        assert_eq!(c3, c2);

        push(&[0.3; 400]);
        let (tail, c4) = peek(c3);
        assert_eq!(tail.len(), 400);
        assert_eq!(c4, 2400);

        let combined: Vec<f32> = [a, b, tail].concat();
        let full = resample_to_16k_mono(&buf.lock().unwrap().clone(), 16_000).unwrap();
        assert_eq!(combined, full);
    }
}
