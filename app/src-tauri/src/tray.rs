use std::sync::Mutex;
use std::time::Duration;
use once_cell::sync::Lazy;
use tauri::{AppHandle, Manager, image::Image};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};

use crate::{AppError, AppState};
use crate::state::ModelState;

// Icons embedded at compile time. Paths relative to this source file.
const ICON_LOADING:    &[u8] = include_bytes!("../icons/tray/tray-loading.png");
const ICON_READY:      &[u8] = include_bytes!("../icons/tray/tray-ready.png");
const ICON_RECORDING1: &[u8] = include_bytes!("../icons/tray/tray-recording-1.png");
const ICON_RECORDING2: &[u8] = include_bytes!("../icons/tray/tray-recording-2.png");
const ICON_ERROR:      &[u8] = include_bytes!("../icons/tray/tray-error.png");

static TRAY: Lazy<Mutex<Option<TrayIcon>>> = Lazy::new(|| Mutex::new(None));

pub fn install(app: &AppHandle) -> Result<(), AppError> {
    let icon = Image::from_bytes(ICON_LOADING)
        .map_err(|e| AppError::Internal(format!("tray icon: {e}")))?;
    let menu = build_menu(app, false, "Loading...")?;
    let tray = TrayIconBuilder::new()
        .icon(icon)
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
            }
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
    }
}

fn update_for_state(app: &AppHandle, model: &ModelState, recording: bool) -> Result<(), AppError> {
    let bytes: &[u8] = if recording {
        ICON_RECORDING1
    } else {
        match model {
            ModelState::Ready { .. } => ICON_READY,
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
        let _ = tray.set_menu(Some(menu));
    }
    Ok(())
}

fn state_label(s: &ModelState) -> String {
    match s {
        ModelState::Idle => "Idle".into(),
        ModelState::Loading { whisper, llama } => format!("Loading (STT {} LLM {})", flag(*whisper), flag(*llama)),
        ModelState::Ready { whisper, llama } => format!("Ready (STT {} LLM {})", flag(*whisper), flag(*llama)),
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
    let sep1 = PredefinedMenuItem::separator(app).map_err(menu_err)?;
    let settings_item = MenuItem::with_id(app, "open-settings", "Settings...", true, Some("CmdOrCtrl+,")).map_err(menu_err)?;
    let mm_item = MenuItem::with_id(app, "open-mm", "Open Model Manager...", true, None::<&str>).map_err(menu_err)?;
    let wiz_item = MenuItem::with_id(app, "open-wizard", "Re-run setup wizard", true, None::<&str>).map_err(menu_err)?;
    let sep2 = PredefinedMenuItem::separator(app).map_err(menu_err)?;
    let logs_item = MenuItem::with_id(app, "view-logs", "View logs", true, None::<&str>).map_err(menu_err)?;
    let updates_item = MenuItem::with_id(app, "check-updates", "Check for updates", true, None::<&str>).map_err(menu_err)?;
    let sep3 = PredefinedMenuItem::separator(app).map_err(menu_err)?;
    let quit_item = PredefinedMenuItem::quit(app, None).map_err(menu_err)?;

    Menu::with_items(app, &[
        &state_item, &hotkey_item, &sep1,
        &settings_item, &mm_item, &wiz_item, &sep2,
        &logs_item, &updates_item, &sep3,
        &quit_item,
    ]).map_err(menu_err)
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
        "open-settings" => { let _ = crate::commands::windows::open_window_internal(app, "settings"); }
        "open-mm" => { let _ = crate::commands::windows::open_window_internal(app, "model-manager"); }
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
        _ => {}
    }
}
