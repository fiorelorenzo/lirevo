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
    let mut builder = app.dialog().file();
    for f in &filters {
        let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
        builder = builder.add_filter(&f.name, &exts);
    }
    let result = builder.blocking_pick_file();
    Ok(result.map(|p| p.to_string()))
}
