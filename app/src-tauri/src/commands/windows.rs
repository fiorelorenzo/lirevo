use tauri::{AppHandle, WebviewWindowBuilder, WebviewUrl};

use crate::{AppError, AppState};

#[tauri::command]
pub fn open_window(app: AppHandle, route: String) -> Result<(), AppError> {
    open_window_internal(&app, &route)
}

#[tauri::command]
pub fn close_window(window: tauri::Window) -> Result<(), AppError> {
    window.close().map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub async fn complete_wizard(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    let hotkey = {
        let mut inner = state.inner.lock().unwrap();
        inner.settings.onboarding_complete = true;
        inner.settings.persist(&app)?;
        inner.settings.hotkey
    };
    // Re-install the hotkey listener now that Accessibility has presumably
    // been granted during the wizard. The initial install at startup may
    // have failed silently if the permission was revoked or not-yet-given.
    if let Err(e) = crate::hotkey::reinstall(&app, hotkey) {
        tracing::warn!(?e, "hotkey reinstall after wizard failed");
    }
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("wizard") {
        let _ = w.close();
    }
    open_window_internal(&app, "home")?;
    Ok(())
}

pub fn open_window_internal(app: &AppHandle, route: &str) -> Result<(), AppError> {
    use tauri::Manager;
    // Focus existing window if alive.
    if let Some(w) = app.get_webview_window(route) {
        let _ = w.set_focus();
        return Ok(());
    }
    // Note: wizard is NOT always_on_top — the user must be able to switch to
    // System Settings to grant permissions, so it can't trap focus.
    let (w, h, resizable) = match route {
        "home" => (720u32, 520u32, true),
        "wizard" => (760, 620, false),
        "settings" => (820, 600, true),
        "model-manager" => (720, 640, true),
        _ => return Err(AppError::Internal(format!("unknown route: {route}"))),
    };
    // SvelteKit with adapter-static: routes are paths like /settings, /wizard, etc.
    // Tauri loads build/<route>/index.html which prerendered SvelteKit gives us.
    // For "home" we load the root (/).
    let path = if route == "home" { "/".to_string() } else { format!("/{route}") };
    let url = WebviewUrl::App(path.into());
    let mut builder = WebviewWindowBuilder::new(app, route, url)
        .title("local-dictation-app")
        .inner_size(w as f64, h as f64)
        .resizable(resizable);
    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true)
            .traffic_light_position(tauri::LogicalPosition::new(16.0, 18.0));
    }
    builder
        .build()
        .map_err(|e| AppError::Internal(format!("window build: {e}")))?;
    Ok(())
}
