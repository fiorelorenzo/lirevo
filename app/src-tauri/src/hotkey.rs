use tauri::AppHandle;
use crate::AppError;
use crate::settings::Hotkey;

pub fn install(_app: AppHandle, _hotkey: Hotkey) -> Result<(), AppError> {
    Ok(()) // T13 fills
}

pub fn reinstall(_app: &AppHandle, _hotkey: Hotkey) -> Result<(), AppError> {
    Ok(()) // T13 fills
}
