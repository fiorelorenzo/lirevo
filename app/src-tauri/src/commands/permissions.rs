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
    let status = tokio::task::spawn_blocking(prompt_mic)
        .await
        .map_err(|e| AppError::Internal(format!("prompt_microphone join: {e}")))?;
    Ok(to_status(status))
}

/// Open the OS' privacy settings on the Microphone pane. macOS has a deep
/// link via the `x-apple.systempreferences:` scheme; Linux/Windows have
/// nothing portable, so the command becomes a no-op there for now (callers
/// can wire platform-specific deep links later without changing the
/// frontend contract).
#[tauri::command]
pub fn open_system_settings_microphone() -> Result<(), AppError> {
    open_privacy_pane("microphone")
}

/// Same as the microphone variant but targets the Accessibility pane.
#[tauri::command]
pub fn open_system_settings_accessibility() -> Result<(), AppError> {
    open_privacy_pane("accessibility")
}

#[cfg(target_os = "macos")]
fn open_privacy_pane(which: &'static str) -> Result<(), AppError> {
    let url = match which {
        "microphone" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        }
        "accessibility" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        _ => return Err(AppError::Internal(format!("unknown privacy pane: {which}"))),
    };
    tracing::info!(url, "opening System Settings privacy pane");
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|e| AppError::Internal(format!("open settings: {e}")))?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn open_privacy_pane(which: &'static str) -> Result<(), AppError> {
    tracing::info!(which, "open_privacy_pane not implemented on this platform");
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
    let (hotkey, activation_mode) = {
        let inner = state.inner.lock().unwrap();
        (
            inner.settings.hotkey.clone(),
            inner.settings.activation_mode,
        )
    };
    tracing::info!(
        ?hotkey,
        ?activation_mode,
        "retry_hotkey_install: invoked from frontend"
    );
    let result = crate::hotkey::reinstall(&app, hotkey, activation_mode);
    match &result {
        Ok(()) => tracing::info!("retry_hotkey_install: success"),
        Err(e) => tracing::warn!(?e, "retry_hotkey_install: failed"),
    }
    result
}

/// Debug helper — pipe a string from any webview into the backend tracing
/// stream so we can see it in `~/Library/Logs/ai.lirevo.app/*.log`
/// without needing devtools on the overlay window (which is click-through).
/// Caps + sanitizes inputs so a compromised renderer can't fill the disk
/// with multi-GB log lines or sneak ANSI escapes / newlines into log files
/// that are read by support workflows.
const FRONTEND_LOG_MAX_SOURCE: usize = 32;
const FRONTEND_LOG_MAX_MSG: usize = 4 * 1024;

#[tauri::command]
pub fn frontend_log(source: &str, msg: &str) {
    fn sanitize(s: &str, max: usize) -> String {
        s.chars()
            .filter(|c| !c.is_control() || *c == ' ')
            .take(max)
            .collect()
    }
    let source = sanitize(source, FRONTEND_LOG_MAX_SOURCE);
    let msg = sanitize(msg, FRONTEND_LOG_MAX_MSG);
    tracing::info!(%source, %msg, "frontend_log");
}
