use crate::AppError;
use os_integration::{
    check_accessibility as check_ax, check_microphone as check_mic,
    prompt_accessibility as prompt_ax, prompt_microphone as prompt_mic, PermissionStatus,
};

fn to_status(s: PermissionStatus) -> String {
    match s {
        PermissionStatus::Granted => "granted",
        PermissionStatus::Denied => "denied",
        PermissionStatus::NotDetermined => "not_determined",
    }
    .to_string()
}

#[tauri::command]
pub fn check_accessibility() -> Result<String, AppError> {
    Ok(to_status(check_ax()))
}

#[tauri::command]
pub fn prompt_accessibility() -> Result<String, AppError> {
    Ok(to_status(prompt_ax()))
}

#[tauri::command]
pub fn check_microphone() -> Result<String, AppError> {
    Ok(to_status(check_mic()))
}

/// Trigger the macOS microphone TCC prompt and block until the user responds
/// (or 60s elapses). Must be awaited off the main thread on the frontend
/// — it can take several seconds. Idempotent: if permission was already
/// granted/denied this call returns the existing status without prompting.
#[tauri::command]
pub async fn prompt_microphone() -> Result<String, AppError> {
    // Move the blocking AVCaptureDevice call off the runtime thread.
    let status =
        tokio::task::spawn_blocking(prompt_mic)
            .await
            .map_err(|e| AppError::Internal(format!("prompt_microphone join: {e}")))?;
    Ok(to_status(status))
}

/// Open System Settings directly on the Microphone privacy pane. We shell out
/// to `open` rather than going through tauri-plugin-shell so we don't depend
/// on a configured URL scope — `x-apple.systempreferences:` is not in any
/// default plugin allowlist.
#[tauri::command]
pub fn open_system_settings_microphone() -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    {
        let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
        tracing::info!(url, "opening System Settings microphone pane");
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| AppError::Internal(format!("open settings: {e}")))?;
    }
    Ok(())
}

/// Same as the microphone variant but targets the Accessibility pane.
#[tauri::command]
pub fn open_system_settings_accessibility() -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    {
        let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
        tracing::info!(url, "opening System Settings accessibility pane");
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| AppError::Internal(format!("open settings: {e}")))?;
    }
    Ok(())
}

/// Re-install the hotkey listener after the user has (presumably just) granted
/// Accessibility. The initial install at startup fails silently when the
/// permission is missing; this lets the UI recover without an app restart.
#[tauri::command]
pub async fn retry_hotkey_install(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), AppError> {
    let hotkey = state.inner.lock().unwrap().settings.hotkey;
    crate::hotkey::reinstall(&app, hotkey)
}
