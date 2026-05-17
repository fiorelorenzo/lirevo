use tauri::{AppHandle, State};
use crate::{AppError, AppState};
use crate::models::{CATALOG, CatalogEntry, LocalModel, list_local};

#[tauri::command]
pub fn models_catalog() -> Vec<CatalogEntry> {
    CATALOG.to_vec()
}

#[tauri::command]
pub fn models_list_local(app: AppHandle) -> Result<Vec<LocalModel>, AppError> {
    list_local(&app).map_err(|e| AppError::Fs(e.to_string()))
}

#[tauri::command]
pub async fn models_download(
    _app: AppHandle,
    _state: State<'_, AppState>,
    _id: String,
) -> Result<(), AppError> {
    // T16 fills with real streaming download
    Err(AppError::Download("not implemented yet".into()))
}

#[tauri::command]
pub fn models_cancel_download(_id: String) -> Result<(), AppError> {
    // T16 fills
    Ok(())
}
