use std::sync::atomic::{AtomicBool, Ordering};
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
// Loading is a 6-frame waveform animation (cycled by `spawn_loading_pulse`),
// mirroring the in-app logo's loading ripple. Frame 0 is the static icon shown
// before the animation loop spawns / when idle.
const ICON_LOADING_FRAMES: [&[u8]; 6] = [
    include_bytes!("../icons/tray/tray-loading-1.png"),
    include_bytes!("../icons/tray/tray-loading-2.png"),
    include_bytes!("../icons/tray/tray-loading-3.png"),
    include_bytes!("../icons/tray/tray-loading-4.png"),
    include_bytes!("../icons/tray/tray-loading-5.png"),
    include_bytes!("../icons/tray/tray-loading-6.png"),
];
const ICON_LOADING:            &[u8] = ICON_LOADING_FRAMES[0];
const ICON_READY_POWER_SAVER:  &[u8] = include_bytes!("../icons/tray/tray-ready-power_saver.png");
const ICON_READY_BALANCED:     &[u8] = include_bytes!("../icons/tray/tray-ready-balanced.png");
const ICON_READY_PERFORMANCE:  &[u8] = include_bytes!("../icons/tray/tray-ready-performance.png");
const ICON_RECORDING1:         &[u8] = include_bytes!("../icons/tray/tray-recording-1.png");
const ICON_RECORDING2:         &[u8] = include_bytes!("../icons/tray/tray-recording-2.png");
const ICON_ERROR:              &[u8] = include_bytes!("../icons/tray/tray-error.png");
// Shown (in place of the Ready icon) when Accessibility or Microphone is
// missing — the waveform with a dot badge. Dictation can't work without both.
const ICON_ATTENTION:          &[u8] = include_bytes!("../icons/tray/tray-attention.png");

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

/// Whether Accessibility AND Microphone are currently granted. Kept fresh by a
/// background poller in `install` so the tray reflects missing permissions even
/// with no window open (the frontend `permissionsState` store only polls while
/// a window is visible). Defaults to `true` so the tray doesn't flash an
/// attention badge before the first check runs.
static PERMISSIONS_OK: AtomicBool = AtomicBool::new(true);

/// Both permissions dictation needs end to end: Accessibility (global hotkey +
/// text injection) and Microphone (audio capture). Missing either means the
/// tray shows its attention badge.
fn check_perms_ok() -> bool {
    use os_integration::PermissionStatus::Granted;
    os_integration::check_accessibility() == Granted
        && os_integration::check_microphone() == Granted
}

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
        let mut was_loading = false;
        loop {
            let model = model_rx.borrow().clone();
            let recording = *rec_rx.borrow();
            let _ = update_for_state(&app2, &model, recording);
            // Spawn each animation only on the RISING edge of its state, so two
            // pulse tasks never race on `tray.set_icon` (which would freeze the
            // icon on a single frame).
            if recording && !was_recording {
                tauri::async_runtime::spawn(spawn_recording_pulse(app2.clone()));
            }
            let loading = !recording
                && matches!(model, ModelState::Loading { .. } | ModelState::Reloading { .. });
            if loading && !was_loading {
                tauri::async_runtime::spawn(spawn_loading_pulse(app2.clone()));
            }
            was_recording = recording;
            was_loading = loading;
            tokio::select! {
                _ = model_rx.changed() => {}
                _ = rec_rx.changed() => {}
            }
        }
    });

    // Background permission monitor. A menu-bar app must reflect missing
    // Accessibility/Microphone even with no window open, so the tray can't
    // rely on the frontend's `permissionsState` poll. Re-check every 3s and
    // refresh the tray when the granted-ness flips (TCC checks are cheap).
    let app3 = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3));
        loop {
            tick.tick().await;
            let ok = check_perms_ok();
            // swap returns the previous value; refresh only on a real change.
            if PERMISSIONS_OK.swap(ok, Ordering::Relaxed) != ok {
                refresh(&app3);
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

/// Cycle the loading-waveform frames (~150 ms each) while the model is loading
/// and not recording — the tray's version of the in-app logo's loading ripple.
/// Stops as soon as loading ends or recording starts.
async fn spawn_loading_pulse(app: AppHandle) {
    let mut frame = 0usize;
    loop {
        let state = app.state::<AppState>();
        let recording = *state.recording_state_tx.borrow();
        let loading = matches!(
            state.current_model_state(),
            ModelState::Loading { .. } | ModelState::Reloading { .. }
        );
        if recording || !loading {
            break;
        }
        if let Ok(img) = Image::from_bytes(ICON_LOADING_FRAMES[frame]) {
            if let Some(tray) = TRAY.lock().unwrap().as_ref() {
                let _ = tray.set_icon(Some(img));
                let _ = tray.set_icon_as_template(true);
            }
        }
        frame = (frame + 1) % ICON_LOADING_FRAMES.len();
        tokio::time::sleep(Duration::from_millis(150)).await;
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
    // Missing Accessibility/Microphone takes over the resting (Ready) icon: the
    // app looks Ready but can't actually dictate. Recording implies both are
    // granted, so it never collides with the attention state.
    let needs_permission = !recording
        && !PERMISSIONS_OK.load(Ordering::Relaxed)
        && matches!(model, ModelState::Ready { .. });
    let bytes: &[u8] = if recording {
        ICON_RECORDING1
    } else if needs_permission {
        ICON_ATTENTION
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
    let label = if needs_permission {
        "Permissions needed".to_string()
    } else {
        state_label(model)
    };
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
    // "Show window" exists primarily for the silent-at-login flow — the
    // home window may never have been opened, and the user needs a single
    // tray-clickable affordance to bring up the dashboard. Also useful in
    // the close-to-tray flow when the home window is just hidden.
    let show_item = MenuItem::with_id(app, "show-home", "Show Lirevo", true, None::<&str>).map_err(menu_err)?;
    let settings_item = MenuItem::with_id(app, "open-settings", "Settings...", true, Some("CmdOrCtrl+,")).map_err(menu_err)?;
    let sep2 = PredefinedMenuItem::separator(app).map_err(menu_err)?;
    let logs_item = MenuItem::with_id(app, "view-logs", "View logs", true, None::<&str>).map_err(menu_err)?;
    let updates_item = MenuItem::with_id(app, "check-updates", "Check for updates", true, None::<&str>).map_err(menu_err)?;
    let sep3 = PredefinedMenuItem::separator(app).map_err(menu_err)?;
    let quit_item = PredefinedMenuItem::quit(app, None).map_err(menu_err)?;

    Menu::with_items(app, &[
        &state_item, &hotkey_item, &energy_item, &sep1,
        &show_item, &settings_item, &sep2,
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
        "view-logs" => {
            match app.path().app_log_dir() {
                Ok(dir) => {
                    // Use the same reliable `open` path the System Settings
                    // helpers use; the opener plugin call was silently failing.
                    #[cfg(target_os = "macos")]
                    {
                        if let Err(e) = std::process::Command::new("open").arg(&dir).spawn() {
                            tracing::warn!(?e, dir = %dir.display(), "view-logs: open failed");
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        use tauri_plugin_opener::OpenerExt;
                        if let Err(e) =
                            app.opener().open_path(dir.to_string_lossy().to_string(), None::<&str>)
                        {
                            tracing::warn!(?e, "view-logs: open_path failed");
                        }
                    }
                }
                Err(e) => tracing::warn!(?e, "view-logs: app_log_dir failed"),
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
