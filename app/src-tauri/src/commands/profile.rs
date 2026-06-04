use tauri::State;

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
    let m = mode_from_str(&mode)
        .ok_or_else(|| AppError::Internal(format!("unknown profile mode: {mode}")))?;
    let sel = state
        .profile_selector()
        .ok_or_else(|| AppError::Internal("profile selector not ready".into()))?;
    sel.set_mode(m);
    crate::commands::settings::update_settings(app, state, serde_json::json!({ "profileMode": mode }))
        .await?;
    Ok(())
}
