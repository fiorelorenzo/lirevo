use tauri::AppHandle;
use crate::AppError;

#[derive(serde::Serialize)]
pub struct UpdateInfo {
    pub available: bool,
    pub version: Option<String>,
}

#[tauri::command]
pub async fn check_for_updates(_app: AppHandle) -> Result<UpdateInfo, AppError> {
    Ok(UpdateInfo { available: false, version: None }) // T36 wires real updater check
}
