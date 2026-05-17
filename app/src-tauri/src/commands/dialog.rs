use tauri::AppHandle;
use crate::AppError;

#[derive(serde::Deserialize)]
pub struct FileFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

#[tauri::command]
pub async fn pick_file(
    _app: AppHandle,
    _filters: Vec<FileFilter>,
) -> Result<Option<String>, AppError> {
    Ok(None) // T12 fills with real dialog
}
