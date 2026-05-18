use std::sync::Mutex;
use std::time::{Duration, Instant};

use audio_capture::{InputDeviceInfo, Recorder, RecorderConfig};
use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::oneshot;

use crate::{AppError, AppState};

#[tauri::command]
pub async fn manual_dictate(
    _app: AppHandle,
    state: State<'_, AppState>,
    wav: Vec<u8>,
) -> Result<String, AppError> {
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
pub struct InputDeviceEntry {
    pub name: String,
    pub is_default: bool,
}

impl From<InputDeviceInfo> for InputDeviceEntry {
    fn from(d: InputDeviceInfo) -> Self {
        Self { name: d.name, is_default: d.is_default }
    }
}

#[tauri::command]
pub fn list_input_devices() -> Result<Vec<InputDeviceEntry>, AppError> {
    let devices = audio_capture::list_inputs()
        .map_err(|e| AppError::Permission(format!("enumerate devices: {e}")))?;
    Ok(devices.into_iter().map(Into::into).collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestMicResult {
    /// Peak RMS (0..1) observed during the test.
    pub peak: f32,
    /// Number of level samples counted (post-warmup).
    pub sample_count: u32,
    /// Human-readable label of the device sampled.
    pub device_label: String,
    /// True iff peak crossed the audible threshold within the test window.
    pub detected: bool,
    /// True iff the test was stopped by `cancel_test_mic` (user pressed Stop).
    pub cancelled: bool,
    /// True iff cpal produced samples but every level read was exactly zero
    /// for ≥ 3 seconds. Classic AirPods-stuck-in-A2DP or "device opened but
    /// not capturing" signature — different from "captured but quiet".
    pub device_silent: bool,
}

/// Cancel sender for the currently running test_mic, if any.
static TEST_MIC_CANCEL: Lazy<Mutex<Option<oneshot::Sender<()>>>> = Lazy::new(|| Mutex::new(None));

/// Audible threshold for the adaptive mic test.
const TEST_MIC_THRESHOLD: f32 = 0.02;
/// Discarded period at the start of the test — Bluetooth devices like AirPods
/// emit silent zeros for ~300-500 ms while macOS negotiates HFP.
const TEST_MIC_WARMUP: Duration = Duration::from_millis(500);
/// Hard upper bound on test duration: if no audio is detected and the user
/// doesn't press Stop, we still return after this so the wizard isn't stuck.
const TEST_MIC_MAX_DURATION: Duration = Duration::from_secs(30);

/// Sample the input device adaptively: stream live RMS levels via the
/// `recording:state` + `recording:level` events and resolve as soon as the
/// peak crosses the audible threshold (or the user cancels via
/// `cancel_test_mic`, or after a 30-second safety cap).
#[tauri::command]
pub async fn test_mic(
    app: AppHandle,
    device_name: Option<String>,
) -> Result<TestMicResult, AppError> {
    // Cancel any previous in-flight test so a fresh one always wins.
    if let Some(tx) = TEST_MIC_CANCEL.lock().unwrap().take() {
        let _ = tx.send(());
    }
    let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
    *TEST_MIC_CANCEL.lock().unwrap() = Some(cancel_tx);

    if os_integration::dev_skip_perms() {
        return mock_test_mic(app, cancel_rx).await;
    }

    let cfg = RecorderConfig {
        device_name: device_name.clone(),
        ..Default::default()
    };
    let device_label = device_name.clone().unwrap_or_else(|| {
        audio_capture::default_input_device_label().unwrap_or_else(|_| "(unknown)".into())
    });

    tracing::info!(device = %device_label, "test_mic: starting");

    let mut recorder = Recorder::new(cfg)
        .map_err(|e| AppError::Permission(format!("recorder new: {e}")))?;
    recorder
        .start()
        .map_err(|e| AppError::Permission(format!("recorder start: {e}")))?;

    tracing::info!("test_mic: recorder started, entering level loop");

    let mut rx = recorder.level_rx();
    let _ = app.emit("recording:state", true);

    let started = Instant::now();
    let max_sleep = tokio::time::sleep(TEST_MIC_MAX_DURATION);
    tokio::pin!(max_sleep);

    let mut peak: f32 = 0.0;
    let mut sample_count: u32 = 0;
    let mut emit_count: u32 = 0;
    let mut detected = false;
    let mut cancelled = false;
    let mut device_silent = false;
    let mut first_post_warmup: Option<Instant> = None;
    /// If we've received samples for this long with peak still exactly 0,
    /// we conclude the device is producing silence (AirPods stuck in A2DP,
    /// muted hardware mic, etc.) and abort early.
    const SILENT_DEVICE_TIMEOUT: Duration = Duration::from_secs(3);

    loop {
        tokio::select! {
            biased;
            () = &mut max_sleep => {
                tracing::info!("test_mic: 30s safety cap reached");
                break;
            }
            _ = &mut cancel_rx => {
                tracing::info!("test_mic: cancellation received");
                cancelled = true;
                break;
            }
            res = rx.changed() => {
                if res.is_err() {
                    tracing::warn!("test_mic: level channel closed unexpectedly");
                    break;
                }
                let level = *rx.borrow();
                let elapsed = started.elapsed();
                if elapsed >= TEST_MIC_WARMUP {
                    if level > peak { peak = level; }
                    sample_count += 1;
                    if first_post_warmup.is_none() {
                        first_post_warmup = Some(Instant::now());
                    }
                    if level >= TEST_MIC_THRESHOLD {
                        detected = true;
                        tracing::info!(level, peak, sample_count, "test_mic: threshold crossed");
                        let _ = app.emit("recording:level", level);
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        break;
                    }
                    // Detect "device silent" — many samples but exactly zero
                    // peak. Classic AirPods-in-A2DP or unsigned-dev-build
                    // TCC-stuck pattern. Abort early so the user doesn't
                    // wait the full 30s safety cap.
                    if peak == 0.0 && sample_count >= 30 {
                        if let Some(t0) = first_post_warmup {
                            if t0.elapsed() >= SILENT_DEVICE_TIMEOUT {
                                device_silent = true;
                                tracing::warn!(
                                    sample_count,
                                    "test_mic: device producing silence — aborting"
                                );
                                break;
                            }
                        }
                    }
                }
                let _ = app.emit("recording:level", level);
                emit_count += 1;
                // Log first 5 + every 33rd (~1s) so we can verify the channel
                // is producing data without flooding the log file.
                if emit_count <= 5 || emit_count % 33 == 0 {
                    tracing::info!(
                        emit_count, level, elapsed_ms = elapsed.as_millis() as u64,
                        "test_mic: emitted level"
                    );
                }
            }
        }
    }

    let _ = app.emit("recording:state", false);
    if let Err(e) = recorder.stop() {
        tracing::warn!(?e, "test_mic: recorder.stop() failed (non-fatal)");
    }
    // Clear the cancel registration only if it still refers to ours; a newer
    // test_mic may have already installed its own sender.
    {
        let mut g = TEST_MIC_CANCEL.lock().unwrap();
        if g.as_ref().map(oneshot::Sender::is_closed).unwrap_or(true) {
            *g = None;
        }
    }

    tracing::info!(
        peak,
        samples = sample_count,
        device = %device_label,
        detected,
        cancelled,
        device_silent,
        "test_mic: complete"
    );

    Ok(TestMicResult {
        peak,
        sample_count,
        device_label,
        detected,
        cancelled,
        device_silent,
    })
}

/// Simulated test_mic for `LDA_DEV_SKIP_PERMS=1` so the wizard's listening UI
/// + result-state UI can be iterated under `tauri dev` (where real cpal
/// capture returns silence because TCC auto-denies bare-binary launches).
/// Streams a 1.5s synthetic sine-ish envelope on `recording:level`, then
/// resolves as a successful detection — or cancelled if the user pressed Stop.
async fn mock_test_mic(
    app: AppHandle,
    mut cancel_rx: oneshot::Receiver<()>,
) -> Result<TestMicResult, AppError> {
    tracing::warn!("test_mic: LDA_DEV_SKIP_PERMS active — running synthetic capture");
    let _ = app.emit("recording:state", true);

    let started = Instant::now();
    let max = Duration::from_millis(1500);
    let mut interval = tokio::time::interval(Duration::from_millis(33));
    interval.tick().await; // discard the immediate first tick

    let mut peak: f32 = 0.0;
    let mut samples: u32 = 0;
    let mut cancelled = false;

    let deadline = tokio::time::sleep(max);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            biased;
            () = &mut deadline => break,
            _ = &mut cancel_rx => { cancelled = true; break; }
            _ = interval.tick() => {
                let t = started.elapsed().as_millis() as f32 / 1000.0;
                // Envelope that ramps up then plateaus around 0.35.
                let env = (t * 3.5).tanh();
                let level = (0.10 + 0.30 * env) + 0.05 * (t * 18.0).sin();
                let level = level.clamp(0.0, 1.0);
                if level > peak { peak = level; }
                samples += 1;
                let _ = app.emit("recording:level", level);
            }
        }
    }

    let _ = app.emit("recording:state", false);
    {
        let mut g = TEST_MIC_CANCEL.lock().unwrap();
        if g.as_ref().map(oneshot::Sender::is_closed).unwrap_or(true) {
            *g = None;
        }
    }
    Ok(TestMicResult {
        peak,
        sample_count: samples,
        device_label: "(dev mock — LDA_DEV_SKIP_PERMS)".into(),
        detected: !cancelled,
        cancelled,
        device_silent: false,
    })
}

/// Stop an in-flight `test_mic` early. The pending `test_mic` call resolves
/// with `cancelled: true`. No-op if nothing is running.
#[tauri::command]
pub fn cancel_test_mic() -> Result<(), AppError> {
    let taken = TEST_MIC_CANCEL.lock().unwrap().take();
    match taken {
        Some(tx) => {
            let send_result = tx.send(());
            tracing::info!(send_ok = send_result.is_ok(), "cancel_test_mic: cancellation dispatched");
        }
        None => {
            tracing::info!("cancel_test_mic: no in-flight test_mic to cancel");
        }
    }
    Ok(())
}
