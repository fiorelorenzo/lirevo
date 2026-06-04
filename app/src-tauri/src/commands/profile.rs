use tauri::{Manager, State};

use inference_core::profile::{emergency_label, mode_from_str, mode_to_str, ProfileName};

use crate::{AppError, AppState};

/// Snapshot of the energy-profile selector for the UI. `active` is the
/// currently-decided profile (serialized camelCase), `mode` is the persisted
/// selection mode string (`"auto"` / `"power_saver"` / ...), and `emergency`
/// is the human-readable reason if an emergency is forcing Power Saver.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStatus {
    pub active: ProfileName,
    pub mode: String,
    pub emergency: Option<String>,
}

#[tauri::command]
pub fn profile_get(state: State<'_, AppState>) -> Result<ProfileStatus, AppError> {
    let sel = state
        .profile_selector()
        .ok_or_else(|| AppError::Internal("profile selector not ready".into()))?;
    Ok(ProfileStatus {
        active: sel.current_profile(),
        mode: mode_to_str(sel.current_mode()).to_string(),
        emergency: sel.emergency().map(emergency_label),
    })
}

#[tauri::command]
pub async fn profile_set_mode(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mode: String,
) -> Result<(), AppError> {
    let _ = state;
    apply_profile_mode(&app, mode).await
}

/// Set the energy-profile mode on the live selector and persist it to
/// settings. Shared by the `profile_set_mode` Tauri command and the tray
/// energy submenu so both code paths set + persist identically.
pub async fn apply_profile_mode(app: &tauri::AppHandle, mode: String) -> Result<(), AppError> {
    let m = mode_from_str(&mode)
        .ok_or_else(|| AppError::Internal(format!("unknown profile mode: {mode}")))?;
    let sel = app
        .state::<AppState>()
        .profile_selector()
        .ok_or_else(|| AppError::Internal("profile selector not ready".into()))?;
    sel.set_mode(m);
    crate::commands::settings::update_settings(
        app.clone(),
        app.state::<AppState>(),
        serde_json::json!({ "profileMode": mode }),
    )
    .await?;
    Ok(())
}
