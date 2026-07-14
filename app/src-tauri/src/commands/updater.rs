use crate::AppError;
use tauri::AppHandle;

#[derive(serde::Serialize)]
pub struct UpdateInfo {
    pub available: bool,
    pub version: Option<String>,
}

/// Stub: always reports "no update available" without checking anything. A
/// real check (release-manifest fetch + semver compare) is tracked by
/// XPLAT-2. Until that lands, no caller may present this result as a genuine
/// check — the Settings "Check for updates" control intentionally does not
/// call this command; it opens the GitHub Releases page in the browser
/// instead (see `checkUpdates()` in `app/src/routes/settings/+page.svelte`).
#[tauri::command]
pub async fn check_for_updates(_app: AppHandle) -> Result<UpdateInfo, AppError> {
    Ok(UpdateInfo {
        available: false,
        version: None,
    }) // T36 wires real updater check
}
