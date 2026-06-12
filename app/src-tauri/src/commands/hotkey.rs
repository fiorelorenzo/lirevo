use tauri::{AppHandle, Emitter};

use crate::AppError;

/// Enter capture mode: stream live key snapshots to the webview as
/// `hotkey:capture` events until `stop_hotkey_capture` is called. The bound
/// hotkey does not fire while capturing.
#[tauri::command]
pub async fn start_hotkey_capture(app: AppHandle) -> Result<(), AppError> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<os_integration::CaptureEvent>(64);
    crate::hotkey::start_capture(tx);
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = app.emit("hotkey:capture", &ev);
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn stop_hotkey_capture() -> Result<(), AppError> {
    crate::hotkey::stop_capture();
    Ok(())
}
