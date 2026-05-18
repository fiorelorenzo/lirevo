use std::time::{Duration, Instant};

use audio_capture::{Recorder, RecorderConfig};
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

/// Briefly start the recorder for 2 seconds, forward audio levels via the
/// `recording:level` event (the RecordingIndicator overlay subscribes to it),
/// and return the peak RMS observed. Used by the wizard's microphone test.
#[tauri::command]
pub async fn test_mic(app: AppHandle) -> Result<f32, AppError> {
    let mut recorder = Recorder::new(RecorderConfig::default())
        .map_err(|e| AppError::Permission(format!("recorder new: {e}")))?;
    recorder
        .start()
        .map_err(|e| AppError::Permission(format!("recorder start: {e}")))?;

    let mut rx = recorder.level_rx();
    let _ = app.emit("recording:state", true);

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut peak: f32 = 0.0;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        tokio::select! {
            res = rx.changed() => {
                if res.is_err() {
                    break;
                }
                let level = *rx.borrow();
                if level > peak {
                    peak = level;
                }
                let _ = app.emit("recording:level", level);
            }
            _ = tokio::time::sleep(remaining) => break,
        }
    }

    let _ = app.emit("recording:state", false);
    let _ = recorder.stop();
    Ok(peak)
}
