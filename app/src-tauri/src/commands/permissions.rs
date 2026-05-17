use crate::AppError;
use os_integration::{
    check_accessibility as check_ax, check_microphone as check_mic,
    prompt_accessibility as prompt_ax, PermissionStatus,
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
