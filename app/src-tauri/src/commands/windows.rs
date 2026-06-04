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
    // Onboarding is done and the wizard downloaded both models, so eager-load
    // them from the cache now. `load_models` early-returns while onboarding is
    // incomplete (the wizard owns downloads), so this is the first real load —
    // it makes home ready to dictate immediately instead of lazily on the first
    // hotkey press.
    {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app2.state::<AppState>();
            crate::commands::inference::load_models(&app2, state).await;
        });
    }
    // Destroy (not close) a separate wizard window: `close` would be caught by
    // the menu-bar keep-alive handler and merely hide it. When the wizard was
    // reached via client-side nav this is a no-op (the window is "home"); the
    // frontend then routes that window back to home after this command.
    if let Some(w) = app.get_webview_window("wizard") {
        let _ = w.destroy();
    }
    open_window_internal(&app, "home")?;
    Ok(())
}

pub fn open_window_internal(app: &AppHandle, route: &str) -> Result<(), AppError> {
    open_window_internal_with_query(app, route, None)
}

/// Like [`open_window_internal`] but appends `?<query>` to the loaded URL on
/// first build. When the window already exists this is a no-op for the URL
/// (the webview is not reloaded). Callers that need to nudge an already-open
/// window into a specific sub-view should emit a window-scoped event instead.
pub fn open_window_internal_with_query(
    app: &AppHandle,
    route: &str,
    query: Option<&str>,
) -> Result<(), AppError> {
    use tauri::Manager;
    // Overlay is its own thing — special-case it before the regular flow.
    if route == "overlay" {
        if app.get_webview_window("overlay").is_some() {
            return Ok(());
        }
        return build_overlay_window(app);
    }
    // Focus existing window if alive. Order matters: `show` first (it's a
    // no-op if already visible), then `unminimize` (handles macOS minimize),
    // then `set_focus`. Without the `show`, a window that was hidden by the
    // close-handler (which hides instead of quitting) stays hidden forever
    // because `set_focus` on an invisible window is silently ignored.
    if let Some(w) = app.get_webview_window(route) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }
    // Note: wizard is NOT always_on_top — the user must be able to switch to
    // System Settings to grant permissions, so it can't trap focus.
    let (w, h, resizable) = match route {
        "home" => (800u32, 600u32, true),
        "wizard" => (860, 720, false),
        "settings" => (900, 680, true),
        _ => return Err(AppError::Internal(format!("unknown route: {route}"))),
    };
    // SvelteKit with adapter-static: routes are paths like /settings, /wizard, etc.
    // Tauri loads build/<route>/index.html which prerendered SvelteKit gives us.
    // For "home" we load the root (/).
    let base = if route == "home" { String::new() } else { format!("/{route}") };
    let path = match query {
        Some(q) if !q.is_empty() => format!("{}?{q}", if base.is_empty() { "/" } else { base.as_str() }),
        _ => if base.is_empty() { "/".to_string() } else { base },
    };
    let url = WebviewUrl::App(path.into());
    let mut builder = WebviewWindowBuilder::new(app, route, url)
        .title("Lirevo")
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

/// The recording overlay: a small, transparent, click-through bar that
/// floats at the top of the primary display and shows a live waveform.
/// Stays above every other window (including frontmost-app windows) and
/// follows the user across spaces.
fn build_overlay_window(app: &AppHandle) -> Result<(), AppError> {
    const OVERLAY_W: f64 = 320.0;
    const OVERLAY_H: f64 = 80.0;

    let mut builder = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("/overlay".into()))
        .title("")
        .inner_size(OVERLAY_W, OVERLAY_H)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .transparent(true)
        .skip_taskbar(true)
        .focused(false)
        .shadow(false)
        .visible(false); // Show/hide driven by hotkey.rs on recording state.
    #[cfg(target_os = "macos")]
    {
        builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay);
    }

    let window = builder
        .build()
        .map_err(|e| AppError::Internal(format!("overlay build: {e}")))?;

    // Centered horizontally near the top of the primary monitor.
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let mon_size = monitor.size();
        let mon_pos = monitor.position();
        let scale = monitor.scale_factor();
        let logical_w = mon_size.width as f64 / scale;
        let x = mon_pos.x as f64 / scale + (logical_w - OVERLAY_W) / 2.0;
        let y = mon_pos.y as f64 / scale + 12.0; // 12 px from the top
        let _ = window.set_position(tauri::LogicalPosition::new(x, y));
    }

    // Native window tweaks (float above frontmost apps, sticky across
    // spaces, click-through) live in `os_integration::overlay`. The cfg
    // here is only around obtaining the macOS NSWindow handle from Tauri,
    // which is the platform-specific bit Tauri itself gates.
    #[cfg(target_os = "macos")]
    {
        match window.ns_window() {
            Ok(ns_window) => {
                tracing::info!(ptr = ?ns_window, "overlay: applying NSWindow attrs");
                // SAFETY: `ns_window` is a live `NSWindow *` returned by
                // Tauri; it remains valid for the duration of this call.
                if let Err(e) = unsafe {
                    os_integration::overlay::apply_floating_click_through(ns_window)
                } {
                    tracing::warn!(?e, "overlay: apply_floating_click_through failed");
                }
            }
            Err(e) => {
                tracing::warn!(?e, "overlay: ns_window() failed");
            }
        }
    }

    Ok(())
}
