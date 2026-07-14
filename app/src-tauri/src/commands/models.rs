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
/// progress bar per model. Delegates the streaming, `.partial` handling, and
/// SHA-256 verification to `crate::models::download_file` — the same routine
/// backing the LLM path (`crate::models::download_inner`) — and registers in
/// `ACTIVE_DOWNLOADS` the same way so `models_cancel_download` can interrupt
/// it mid-transfer (checked once per chunk via `cancel_rx.try_recv()`).
#[tauri::command]
pub async fn stt_download(app: AppHandle, id: String) -> Result<(), AppError> {
    use crate::models::{
        download_file, models_dir, DownloadError, DownloadProgress, DownloadProgressState,
        ACTIVE_DOWNLOADS,
    };
    use tauri::Emitter;
    use tokio::sync::oneshot;

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

    if let Err(msg) = crate::models::check_disk_space(&app, known_total) {
        tracing::error!(id = %id, error = %msg, "stt_download: insufficient disk space");
        let _ = app.emit(
            "download:progress",
            DownloadProgress {
                id: id.clone(),
                state: DownloadProgressState::Error,
                bytes_received: 0,
                bytes_total: known_total,
                error_message: Some(msg.clone()),
            },
        );
        return Err(AppError::Download(msg));
    }

    let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
    {
        let mut g = ACTIVE_DOWNLOADS.lock().unwrap();
        let map = g.as_mut().expect("init_active_downloads not called");
        if map.contains_key(&id) {
            return Err(AppError::Download(format!("already downloading: {id}")));
        }
        map.insert(id.clone(), cancel_tx);
    }

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

    let expected_sha256 = crate::stt::catalog::model_metadata(&id).map(|m| m.sha256);
    let result = download_file(
        &app,
        &url,
        &dest,
        &id,
        known_total,
        expected_sha256,
        &mut cancel_rx,
    )
    .await;

    {
        let mut g = ACTIVE_DOWNLOADS.lock().unwrap();
        if let Some(map) = g.as_mut() {
            map.remove(&id);
        }
    }

    match result {
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
        Err(DownloadError::Cancelled) => {
            tracing::info!(id = %id, "stt_download: cancelled");
            let _ = app.emit(
                "download:progress",
                DownloadProgress {
                    id,
                    state: DownloadProgressState::Cancelled,
                    bytes_received: 0,
                    bytes_total: 0,
                    error_message: None,
                },
            );
            Ok(())
        }
        Err(DownloadError::Failed(msg)) => {
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
