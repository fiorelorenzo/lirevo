use std::sync::Mutex;
use std::time::Duration;
use once_cell::sync::Lazy;
use tauri::{AppHandle, Manager, image::Image};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

use inference_core::profile::{mode_to_str, ProfileName};

use crate::{AppError, AppState};
use crate::state::ModelState;

// Icons embedded at compile time. Paths relative to this source file.
// Monochrome template images (black + alpha) — the tray is built with
// `icon_as_template(true)` so macOS auto-tints them per light/dark menu bar.
// The three Ready variants encode the active energy profile via waveform
// amplitude. Regenerate with scripts/gen-icons.sh.
const ICON_LOADING:            &[u8] = include_bytes!("../icons/tray/tray-loading.png");
const ICON_READY_POWER_SAVER:  &[u8] = include_bytes!("../icons/tray/tray-ready-power_saver.png");
const ICON_READY_BALANCED:     &[u8] = include_bytes!("../icons/tray/tray-ready-balanced.png");
const ICON_READY_PERFORMANCE:  &[u8] = include_bytes!("../icons/tray/tray-ready-performance.png");
const ICON_RECORDING1:         &[u8] = include_bytes!("../icons/tray/tray-recording-1.png");
const ICON_RECORDING2:         &[u8] = include_bytes!("../icons/tray/tray-recording-2.png");
const ICON_ERROR:              &[u8] = include_bytes!("../icons/tray/tray-error.png");

/// Ready-state tray icon whose waveform amplitude encodes the active energy
/// profile (PowerSaver = low, Balanced = medium, Performance = tall).
fn ready_icon_for(profile: ProfileName) -> &'static [u8] {
    match profile {
        ProfileName::PowerSaver => ICON_READY_POWER_SAVER,
        ProfileName::Balanced => ICON_READY_BALANCED,
        ProfileName::Performance => ICON_READY_PERFORMANCE,
    }
}

static TRAY: Lazy<Mutex<Option<TrayIcon>>> = Lazy::new(|| Mutex::new(None));

pub fn install(app: &AppHandle) -> Result<(), AppError> {
    let icon = Image::from_bytes(ICON_LOADING)
        .map_err(|e| AppError::Internal(format!("tray icon: {e}")))?;
    let menu = build_menu(app, false, "Loading...")?;
    let tray = TrayIconBuilder::new()
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .on_menu_event(handle_menu_event)
        .build(app)
        .map_err(|e| AppError::Internal(format!("tray build: {e}")))?;
    *TRAY.lock().unwrap() = Some(tray);

    // Subscribe to model/recording state and update tray accordingly.
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app2.state::<AppState>();
        let mut model_rx = state.model_state_tx.subscribe();
        let mut rec_rx = state.recording_state_tx.subscribe();
        let mut was_recording = false;
        loop {
            let model = model_rx.borrow().clone();
            let recording = *rec_rx.borrow();
            let _ = update_for_state(&app2, &model, recording);
            // Only spawn the pulse animation on the RISING edge of
            // `recording`. The previous code spawned a fresh task on every
            // model-state change while recording — multiple pulse tasks
            // would then race on `tray.set_icon` and the icon would
            // sometimes get stuck on a single frame.
            if recording && !was_recording {
                tauri::async_runtime::spawn(spawn_recording_pulse(app2.clone()));
            }
            was_recording = recording;
            tokio::select! {
                _ = model_rx.changed() => {}
                _ = rec_rx.changed() => {}
            }
        }
    });

    Ok(())
}

async fn spawn_recording_pulse(app: AppHandle) {
    let mut frame: u8 = 0;
    loop {
        let state = app.state::<AppState>();
        if !*state.recording_state_tx.subscribe().borrow() { break; }
        frame = if frame == 0 { 1 } else { 0 };
        let bytes = if frame == 0 { ICON_RECORDING1 } else { ICON_RECORDING2 };
        if let Ok(img) = Image::from_bytes(bytes) {
            if let Some(tray) = TRAY.lock().unwrap().as_ref() {
                let _ = tray.set_icon(Some(img));
                let _ = tray.set_icon_as_template(true);
            }
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
    }
}

/// Rebuild the tray from the current model/recording state. Used by callers
/// outside the model/recording watch loop (e.g. the profile-change loop) that
/// need the menu's energy submenu + status to reflect a fresh selection.
pub fn refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    let model = state.current_model_state();
    let recording = *state.recording_state_tx.borrow();
    let _ = update_for_state(app, &model, recording);
}

fn update_for_state(app: &AppHandle, model: &ModelState, recording: bool) -> Result<(), AppError> {
    let bytes: &[u8] = if recording {
        ICON_RECORDING1
    } else {
        match model {
            ModelState::Ready { .. } => {
                // The Ready icon's amplitude tracks the active (resolved)
                // energy profile. Fall back to Balanced if the selector isn't
                // wired yet (very early startup).
                let profile = app
                    .state::<AppState>()
                    .profile_selector()
                    .map_or(ProfileName::Balanced, |s| s.current_profile());
                ready_icon_for(profile)
            }
            ModelState::Error { .. } => ICON_ERROR,
            _ => ICON_LOADING,
        }
    };
    let icon = Image::from_bytes(bytes)
        .map_err(|e| AppError::Internal(format!("tray icon: {e}")))?;
    let label = state_label(model);
    let menu = build_menu(app, recording, &label)?;
    if let Some(tray) = TRAY.lock().unwrap().as_ref() {
        let _ = tray.set_icon(Some(icon));
        // set_icon resets the NSImage, which drops the template flag — re-apply
        // so macOS keeps auto-tinting for light/dark menu bars.
        let _ = tray.set_icon_as_template(true);
        let _ = tray.set_menu(Some(menu));
    }
    Ok(())
}

fn state_label(s: &ModelState) -> String {
    match s {
        ModelState::Idle => "Idle".into(),
        ModelState::Loading { stt, llama } => format!("Loading (STT {} LLM {})", flag(*stt), flag(*llama)),
        ModelState::Ready { stt, llama } => format!("Ready (STT {} LLM {})", flag(*stt), flag(*llama)),
        ModelState::Reloading { reason } => format!("Reloading — {reason}"),
        ModelState::Error { reason } => format!("⚠️ {reason}"),
    }
}

fn flag(ok: bool) -> &'static str { if ok { "✓" } else { "✗" } }

fn build_menu(app: &AppHandle, recording: bool, status_label: &str) -> Result<Menu<tauri::Wry>, AppError> {
    let state_item = MenuItem::with_id(app, "status", status_label, false, None::<&str>).map_err(menu_err)?;
    let hotkey_label = if recording {
        "Recording...".to_string()
    } else {
        let state = app.state::<AppState>();
        let h = state.inner.lock().unwrap().settings.hotkey;
        format!("Hold {} to dictate", hotkey_display(h))
    };
    let hotkey_item = MenuItem::with_id(app, "hotkey", &hotkey_label, false, None::<&str>).map_err(menu_err)?;
    let energy_item = build_energy_submenu(app)?;
    let sep1 = PredefinedMenuItem::separator(app).map_err(menu_err)?;
    // "Show window" exists primarily for the `launch_minimized` flow — the
    // home window may never have been opened, and the user needs a single
    // tray-clickable affordance to bring up the dashboard. Also useful in
    // the close-to-tray flow when the home window is just hidden.
    let show_item = MenuItem::with_id(app, "show-home", "Show Lirevo", true, None::<&str>).map_err(menu_err)?;
    let settings_item = MenuItem::with_id(app, "open-settings", "Settings...", true, Some("CmdOrCtrl+,")).map_err(menu_err)?;
    let wiz_item = MenuItem::with_id(app, "open-wizard", "Re-run setup wizard", true, None::<&str>).map_err(menu_err)?;
    let sep2 = PredefinedMenuItem::separator(app).map_err(menu_err)?;
    let logs_item = MenuItem::with_id(app, "view-logs", "View logs", true, None::<&str>).map_err(menu_err)?;
    let updates_item = MenuItem::with_id(app, "check-updates", "Check for updates", true, None::<&str>).map_err(menu_err)?;
    let sep3 = PredefinedMenuItem::separator(app).map_err(menu_err)?;
    let quit_item = PredefinedMenuItem::quit(app, None).map_err(menu_err)?;

    Menu::with_items(app, &[
        &state_item, &hotkey_item, &energy_item, &sep1,
        &show_item, &settings_item, &wiz_item, &sep2,
        &logs_item, &updates_item, &sep3,
        &quit_item,
    ]).map_err(menu_err)
}

/// Title-cased English label for a profile, used for both the submenu item
/// text and the active-profile status line.
fn profile_display(p: ProfileName) -> &'static str {
    match p {
        ProfileName::PowerSaver => "Power Saver",
        ProfileName::Balanced => "Balanced",
        ProfileName::Performance => "Performance",
    }
}

/// Build the "Energy" submenu: a status item showing the resolved active
/// profile, then four mutually-exclusive `CheckMenuItem`s (Auto + the three
/// pinned profiles). The item matching the current MODE is checked; if the
/// selector isn't ready yet, Auto is checked.
fn build_energy_submenu(app: &AppHandle) -> Result<Submenu<tauri::Wry>, AppError> {
    let selector = app.state::<AppState>().profile_selector();
    // `profile-auto` when in Auto (or selector not ready), else the pinned id.
    let checked_id = selector
        .as_ref()
        .map_or("profile-auto", |sel| match mode_to_str(sel.current_mode()) {
            "auto" => "profile-auto",
            "power_saver" => "profile-power_saver",
            "balanced" => "profile-balanced",
            "performance" => "profile-performance",
            _ => "profile-auto",
        });

    let status_label = match &selector {
        Some(sel) => {
            let active = profile_display(sel.current_profile());
            if matches!(mode_to_str(sel.current_mode()), "auto") {
                format!("Active: Auto - {active}")
            } else {
                format!("Active: {active}")
            }
        }
        None => "Active: Auto".to_string(),
    };
    let status_item = MenuItem::with_id(app, "profile-status", &status_label, false, None::<&str>)
        .map_err(menu_err)?;
    let sep = PredefinedMenuItem::separator(app).map_err(menu_err)?;

    let auto = check_item(app, "profile-auto", "Auto", checked_id)?;
    let saver = check_item(
        app,
        "profile-power_saver",
        profile_display(ProfileName::PowerSaver),
        checked_id,
    )?;
    let balanced = check_item(
        app,
        "profile-balanced",
        profile_display(ProfileName::Balanced),
        checked_id,
    )?;
    let perf = check_item(
        app,
        "profile-performance",
        profile_display(ProfileName::Performance),
        checked_id,
    )?;

    Submenu::with_items(
        app,
        "Energy",
        true,
        &[&status_item, &sep, &auto, &saver, &balanced, &perf],
    )
    .map_err(menu_err)
}

fn check_item(
    app: &AppHandle,
    id: &str,
    label: &str,
    checked_id: &str,
) -> Result<CheckMenuItem<tauri::Wry>, AppError> {
    CheckMenuItem::with_id(app, id, label, true, id == checked_id, None::<&str>).map_err(menu_err)
}

fn menu_err(e: tauri::Error) -> AppError { AppError::Internal(format!("menu: {e}")) }

fn hotkey_display(h: crate::settings::Hotkey) -> &'static str {
    match h {
        crate::settings::Hotkey::RightOption => "Right ⌥",
        crate::settings::Hotkey::LeftOption  => "Left ⌥",
        crate::settings::Hotkey::RightCommand => "Right ⌘",
        crate::settings::Hotkey::Fn => "Fn",
        crate::settings::Hotkey::F5 => "F5",
    }
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        "show-home" => { let _ = crate::commands::windows::open_window_internal(app, "home"); }
        "open-settings" => { let _ = crate::commands::windows::open_window_internal(app, "settings"); }
        "open-wizard" => { let _ = crate::commands::windows::open_window_internal(app, "wizard"); }
        "view-logs" => {
            if let Ok(dir) = app.path().app_log_dir() {
                use tauri_plugin_opener::OpenerExt;
                let _ = app.opener().open_path(dir.to_string_lossy().to_string(), None::<&str>);
            }
        }
        "check-updates" => {
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = crate::commands::updater::check_for_updates(app2).await;
            });
        }
        id @ ("profile-auto" | "profile-power_saver" | "profile-balanced"
        | "profile-performance") => {
            // Map the menu id to the persisted mode string (strip the
            // `profile-` prefix): `profile-auto` -> `auto`, etc.
            let mode = id.trim_start_matches("profile-").to_string();
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                if crate::commands::profile::apply_profile_mode(&app2, mode).await.is_ok() {
                    // The decided profile may not change (e.g. re-pinning the
                    // current band), so the watch-driven refresh in lib.rs
                    // won't always fire — refresh here so the checkmark moves.
                    refresh(&app2);
                }
            });
        }
        _ => {}
    }
}
