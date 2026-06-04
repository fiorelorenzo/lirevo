use tauri::State;

use crate::db::history;
use crate::{AppError, AppState};

#[tauri::command]
pub fn history_list(
    state: State<'_, AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<history::DictationSummary>, AppError> {
    history::list(state.db(), limit.unwrap_or(50), offset.unwrap_or(0))
        .map_err(|e| AppError::Internal(e.to_string()))
}

#[tauri::command]
pub fn history_get(
    state: State<'_, AppState>,
    id: i64,
) -> Result<Option<history::Dictation>, AppError> {
    history::get(state.db(), id).map_err(|e| AppError::Internal(e.to_string()))
}

#[tauri::command]
pub fn history_delete(state: State<'_, AppState>, id: i64) -> Result<(), AppError> {
    history::delete(state.db(), id).map_err(|e| AppError::Internal(e.to_string()))
}

#[tauri::command]
pub fn history_clear(state: State<'_, AppState>) -> Result<(), AppError> {
    history::clear(state.db()).map_err(|e| AppError::Internal(e.to_string()))
}
