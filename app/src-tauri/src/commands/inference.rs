use tauri::State;
use crate::{AppError, AppState};

#[tauri::command]
pub async fn transcribe(
    _state: State<'_, AppState>,
    _wav: Vec<u8>,
    _language: Option<String>,
) -> Result<String, AppError> {
    Err(AppError::WhisperNotLoaded) // T11 fills with real impl
}

#[tauri::command]
pub async fn clean(
    _state: State<'_, AppState>,
    _text: String,
    _language: String,
) -> Result<String, AppError> {
    Err(AppError::LlamaNotLoaded) // T11 fills
}

/// load_models orchestrator (called from setup closure async task).
/// Stub for T9 — T11 fills.
pub async fn load_models(_app: &tauri::AppHandle, _state: tauri::State<'_, AppState>) {
    // No-op until T11.
}
