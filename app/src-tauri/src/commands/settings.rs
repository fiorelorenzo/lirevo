use tauri::State;
use crate::{AppState, Settings, AppError};

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, AppError> {
    Ok(state.inner.lock().unwrap().settings.clone())
}

#[tauri::command]
pub async fn update_settings(
    _app: tauri::AppHandle,
    state: State<'_, AppState>,
    _patch: serde_json::Value,
) -> Result<Settings, AppError> {
    // T10 fills in merge + persist + side effects
    Ok(state.inner.lock().unwrap().settings.clone())
}
