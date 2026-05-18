use tauri::{AppHandle, WebviewWindowBuilder, WebviewUrl};

use crate::{AppError, AppState};

/// Restart the app process. Needed after Accessibility is granted in System
/// Settings — `AXIsProcessTrusted` caches its answer for the process
/// lifetime, so the only way for the hotkey listener install to see the
/// updated permission is a fresh process.
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

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
    // Overlay is its own thing — special-case it before the regular flow.
    if route == "overlay" {
        if app.get_webview_window("overlay").is_some() {
            return Ok(());
        }
        return build_overlay_window(app);
    }
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

    // macOS: raise above frontmost app windows + make sticky across spaces +
    // pass clicks through to whatever is underneath. None of this is reachable
    // from Tauri's portable API surface so we drop to objc2.
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        match window.ns_window() {
            Ok(ns_window) => {
                tracing::info!(
                    ptr = ?ns_window,
                    "overlay: applying NSWindow level + collection behavior",
                );
                let ns_window = ns_window as *mut objc2::runtime::AnyObject;
                unsafe {
                    // NSStatusWindowLevel = 25 — above NSNormalWindowLevel (0)
                    // and NSFloatingWindowLevel (3), below the screensaver/menu.
                    // setLevel: takes NSInteger; on 64-bit macOS that's i64.
                    let _: () = msg_send![ns_window, setLevel: 25_i64];
                    // CanJoinAllSpaces (1) | Stationary (16) | IgnoresCycle (64).
                    let _: () = msg_send![ns_window, setCollectionBehavior: 81_u64];
                    // Clicks fall through to the app below.
                    let _: () = msg_send![ns_window, setIgnoresMouseEvents: true];
                    // Read it back so we can confirm in the logs whether the
                    // setter actually stuck.
                    let level: i64 = msg_send![ns_window, level];
                    let behavior: u64 = msg_send![ns_window, collectionBehavior];
                    let ignores: bool = msg_send![ns_window, ignoresMouseEvents];
                    tracing::info!(level, behavior, ignores, "overlay: NSWindow attrs after set");
                }
            }
            Err(e) => {
                tracing::warn!(?e, "overlay: ns_window() failed");
            }
        }
    }

    Ok(())
}
