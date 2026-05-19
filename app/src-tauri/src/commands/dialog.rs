use crate::AppError;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

#[derive(serde::Deserialize)]
pub struct FileFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

#[tauri::command]
pub async fn pick_file(
    app: AppHandle,
    filters: Vec<FileFilter>,
) -> Result<Option<String>, AppError> {
    // `blocking_pick_file` parks the calling thread until the user dismisses
    // the native dialog; inside an `async` Tauri command that would pin one
    // of the tokio runtime threads and stall every other invoke until the
    // user clicks Cancel. Move it onto the blocking pool.
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut builder = app.dialog().file();
        for f in &filters {
            let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
            builder = builder.add_filter(&f.name, &exts);
        }
        builder.blocking_pick_file()
    })
    .await
    .map_err(|e| AppError::Internal(format!("pick_file join: {e}")))?;
    Ok(result.map(|p| p.to_string()))
}
