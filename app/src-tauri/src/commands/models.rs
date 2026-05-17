use tauri::{AppHandle, State};
use crate::{AppError, AppState};

#[derive(Clone, serde::Serialize)]
pub struct CatalogEntry {
    pub id: String,
    pub kind: String,
}

#[derive(Clone, serde::Serialize)]
pub struct LocalModel {
    pub id: String,
    pub path: String,
}

#[tauri::command]
pub fn models_catalog() -> Vec<CatalogEntry> {
    vec![] // T15 fills with real catalog
}

#[tauri::command]
pub fn models_list_local(_app: AppHandle) -> Result<Vec<LocalModel>, AppError> {
    Ok(vec![]) // T15 fills
}

#[tauri::command]
pub async fn models_download(
    _app: AppHandle,
    _state: State<'_, AppState>,
    _id: String,
) -> Result<(), AppError> {
    Ok(()) // T16 fills
}

#[tauri::command]
pub fn models_cancel_download(_id: String) -> Result<(), AppError> {
    Ok(()) // T16 fills
}
