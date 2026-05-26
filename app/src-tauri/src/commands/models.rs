use tauri::{AppHandle, State};
use crate::{AppError, AppState};
use crate::models::{catalog, CatalogEntry, LocalModel, list_local};
use crate::stt::catalog as stt_catalog;

#[tauri::command]
pub fn models_catalog() -> Vec<CatalogEntry> {
    catalog()
}

/// M4 wizard contract: surface the hardcoded STT catalog so the frontend
/// can assert (in dev builds) that its mirror in
/// `app/src/lib/models/catalog.ts` hasn't drifted. Production builds also
/// call this for the wizard's model picker — keeping the contract one-way
/// (backend is source of truth) means a stale TS catalog can be detected
/// before it ships a model the loader can't resolve.
#[tauri::command]
pub fn get_stt_catalog() -> Vec<stt_catalog::Metadata> {
    stt_catalog::list_models().to_vec()
}

#[tauri::command]
pub fn models_list_local(app: AppHandle) -> Result<Vec<LocalModel>, AppError> {
    list_local(&app).map_err(|e| AppError::Fs(e.to_string()))
}

#[tauri::command]
pub async fn models_download(
    app: AppHandle,
    _state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    crate::models::download(app, id).await
}

#[tauri::command]
pub fn models_cancel_download(id: String) -> Result<(), AppError> {
    crate::models::cancel(&id)
}

#[tauri::command]
pub async fn models_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    tracing::info!(id = %id, "models_delete: invoked");
    // Clear the path setting if the deleted model is the one currently
    // selected; otherwise the picker would still point at a now-missing
    // file and the next load_models would error. Done BEFORE the unlink
    // so a delete failure doesn't leave the settings half-updated.
    let (whisper_p, llm_p, this_filename) = {
        let inner = state.inner.lock().unwrap();
        let f = crate::models::catalog()
            .into_iter()
            .find(|c| c.id == id)
            .map(|c| c.filename.clone());
        (
            inner.settings.whisper_model_path.clone(),
            inner.settings.llm_model_path.clone(),
            f,
        )
    };
    let mut patch = serde_json::Map::new();
    if let Some(filename) = this_filename.as_deref() {
        if let Some(p) = whisper_p.as_ref() {
            if p.file_name().and_then(|n| n.to_str()) == Some(filename) {
                patch.insert("whisperModelPath".into(), serde_json::Value::Null);
            }
        }
        if let Some(p) = llm_p.as_ref() {
            if p.file_name().and_then(|n| n.to_str()) == Some(filename) {
                patch.insert("llmModelPath".into(), serde_json::Value::Null);
            }
        }
    }
    if !patch.is_empty() {
        tracing::info!(id = %id, ?patch, "models_delete: clearing active path setting");
        crate::commands::settings::update_settings(
            app.clone(),
            state,
            serde_json::Value::Object(patch),
        )
        .await?;
    }

    let result = crate::models::delete_by_id(&app, &id)
        .map_err(|e| AppError::Fs(e.to_string()));
    match &result {
        Ok(()) => tracing::info!(id = %id, "models_delete: success"),
        Err(e) => {
            tracing::warn!(id = %id, ?e, "models_delete: failed");
            use tauri::Emitter;
            let _ = app.emit(
                "toast",
                crate::commands::toast("error", format!("Uninstall failed: {e}")),
            );
        }
    }
    result
}
