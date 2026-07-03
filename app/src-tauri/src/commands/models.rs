use crate::models::{catalog, list_local, CatalogEntry, LocalModel};
use crate::stt::catalog as stt_catalog;
use crate::{AppError, AppState};
use tauri::{AppHandle, State};

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

/// Download the STT GGUF into the app's models dir, emitting the same
/// `download:progress` events as the LLM downloads so the wizard renders one
/// progress bar per model. Uses the same streaming mechanism as
/// `crate::models::download_inner` (reqwest bytes_stream + 100ms throttle +
/// `.partial` temp file → rename).
#[tauri::command]
pub async fn stt_download(app: AppHandle, id: String) -> Result<(), AppError> {
    use crate::models::{models_dir, DownloadProgress, DownloadProgressState};
    use futures_util::StreamExt;
    use tauri::Emitter;
    use tokio::io::AsyncWriteExt;

    let known_total = crate::stt::catalog::model_metadata(&id)
        .map(|m| m.size_bytes)
        .unwrap_or(0);
    let url = crate::stt::catalog::stt_gguf_url();
    let dest = models_dir(&app)
        .map_err(|e| AppError::Fs(e.to_string()))?
        .join(crate::stt::STT_GGUF_FILENAME);
    let tmp = dest.with_extension(format!(
        "{}.partial",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));

    tracing::info!(id = %id, %url, "stt_download: starting");

    let _ = app.emit(
        "download:progress",
        DownloadProgress {
            id: id.clone(),
            state: DownloadProgressState::Queued,
            bytes_received: 0,
            bytes_total: known_total,
            error_message: None,
        },
    );

    // Stream-download, mirroring download_inner exactly.
    let do_download = async {
        let client = reqwest::Client::new();
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("http: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let total = resp.content_length().unwrap_or(known_total);

        let mut file = tokio::fs::File::create(&tmp)
            .await
            .map_err(|e| format!("create tmp: {e}"))?;

        let mut received: u64 = 0;
        let mut stream = resp.bytes_stream();
        let mut last_emit = std::time::Instant::now();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| format!("stream: {e}"))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("write: {e}"))?;
            received += chunk.len() as u64;
            if last_emit.elapsed() >= std::time::Duration::from_millis(100) {
                last_emit = std::time::Instant::now();
                let _ = app.emit(
                    "download:progress",
                    DownloadProgress {
                        id: id.clone(),
                        state: DownloadProgressState::Downloading,
                        bytes_received: received,
                        bytes_total: total,
                        error_message: None,
                    },
                );
            }
        }
        // Final 100% event before transitioning to Complete.
        let _ = app.emit(
            "download:progress",
            DownloadProgress {
                id: id.clone(),
                state: DownloadProgressState::Downloading,
                bytes_received: received,
                bytes_total: total,
                error_message: None,
            },
        );
        file.flush().await.map_err(|e| format!("flush: {e}"))?;
        drop(file);
        tokio::fs::rename(&tmp, &dest)
            .await
            .map_err(|e| format!("rename: {e}"))?;
        Ok::<(), String>(())
    };

    match do_download.await {
        Ok(()) => {
            tracing::info!(id = %id, "stt_download: complete");
            let _ = app.emit(
                "download:progress",
                DownloadProgress {
                    id,
                    state: DownloadProgressState::Complete,
                    bytes_received: known_total,
                    bytes_total: known_total,
                    error_message: None,
                },
            );
            Ok(())
        }
        Err(msg) => {
            tracing::error!(id = %id, error = %msg, "stt_download: failed");
            let _ = tokio::fs::remove_file(&tmp).await;
            let _ = app.emit(
                "download:progress",
                DownloadProgress {
                    id,
                    state: DownloadProgressState::Error,
                    bytes_received: 0,
                    bytes_total: 0,
                    error_message: Some(msg.clone()),
                },
            );
            Err(AppError::Download(msg))
        }
    }
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

    let result = crate::models::delete_by_id(&app, &id).map_err(|e| AppError::Fs(e.to_string()));
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
