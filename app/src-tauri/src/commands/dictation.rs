use tauri::State;
use crate::{AppError, AppState};

#[tauri::command]
pub async fn manual_dictate(
    _app: tauri::AppHandle,
    _state: State<'_, AppState>,
    _wav: Vec<u8>,
) -> Result<String, AppError> {
    Err(AppError::Internal("manual_dictate not implemented yet".into())) // T11
}
