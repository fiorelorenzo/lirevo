use std::time::Duration;

use audio_capture::{Recorder, RecorderConfig};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::{AppError, AppState};

#[tauri::command]
pub async fn manual_dictate(
    _app: AppHandle,
    state: State<'_, AppState>,
    wav: Vec<u8>,
) -> Result<String, AppError> {
    // Reusable for testing without going through the hotkey path.
    let language = state.inner.lock().unwrap().settings.language.clone();
    let raw = super::inference::transcribe(state.clone(), wav, Some(language.clone())).await?;
    let cleaned = match super::inference::clean(state.clone(), raw.clone(), language).await {
        Ok(c) => c,
        Err(_) => raw,
    };
    let method = {
        let inner = state.inner.lock().unwrap();
        inner
            .injector
            .inject(&cleaned)
            .map_err(|e| AppError::Inject(e.to_string()))?
    };
    Ok(format!("injected via {method:?}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestMicResult {
    /// Peak RMS (0..1) observed during the test window.
    pub peak: f32,
    /// Number of level samples received from the recorder thread. Zero means
    /// the cpal callback never fired — typically a permission or device issue.
    pub sample_count: u32,
    /// Human-readable label of the device sampled (e.g. "MacBook Pro Microphone").
    pub device_label: String,
}

/// Sample the default input device for 2 seconds (plus a 500 ms warmup window
/// that's discarded — Bluetooth devices like AirPods take time to negotiate
/// HFP and emit silent zeros at first), forwarding live RMS levels via the
/// `recording:state` + `recording:level` events (the RecordingIndicator
/// overlay subscribes to them) and returning peak + sample count + device.
///
/// Hard timeout at 4 seconds prevents any hang from blocking the wizard.
#[tauri::command]
pub async fn test_mic(app: AppHandle) -> Result<TestMicResult, AppError> {
    use std::time::Instant;

    let device_label = audio_capture::default_input_device_label()
        .map_err(|e| AppError::Permission(format!("default device: {e}")))?;

    tracing::info!(device = %device_label, "test_mic: starting");

    let mut recorder = Recorder::new(RecorderConfig::default())
        .map_err(|e| AppError::Permission(format!("recorder new: {e}")))?;
    recorder
        .start()
        .map_err(|e| AppError::Permission(format!("recorder start: {e}")))?;

    let mut rx = recorder.level_rx();
    let _ = app.emit("recording:state", true);

    let mut peak: f32 = 0.0;
    let mut sample_count: u32 = 0;

    let test_started = Instant::now();
    let warmup = Duration::from_millis(500);
    let total = Duration::from_millis(2500); // 500ms warmup + 2s measurement

    let sleep = tokio::time::sleep(total);
    tokio::pin!(sleep);

    // Defensive outer timeout: even if both branches misbehave, we cap at 4s.
    let _ = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            tokio::select! {
                biased;
                () = &mut sleep => break,
                res = rx.changed() => {
                    if res.is_err() { break; }
                    let level = *rx.borrow();
                    let elapsed = test_started.elapsed();
                    // During the warmup, still forward levels to the UI so the
                    // waveform shows audio is "starting", but don't count them
                    // toward peak detection — BT devices may emit silence here.
                    if elapsed >= warmup {
                        if level > peak { peak = level; }
                        sample_count += 1;
                    }
                    let _ = app.emit("recording:level", level);
                }
            }
        }
    })
    .await;

    let _ = app.emit("recording:state", false);
    let stop_result = recorder.stop();
    if let Err(e) = &stop_result {
        tracing::warn!(?e, "test_mic: recorder.stop() failed (non-fatal)");
    }

    tracing::info!(
        peak,
        samples = sample_count,
        device = %device_label,
        "test_mic: complete"
    );

    Ok(TestMicResult { peak, sample_count, device_label })
}
