mod commands;
mod error;
mod hotkey;
mod logging;
mod models;
mod settings;
mod state;
mod tray;

pub use error::AppError;
pub use settings::Settings;
pub use state::{AppState, ModelState};

use tauri::Manager;
use tracing_appender::non_blocking::WorkerGuard;

// Hold the guard for the program's lifetime to avoid losing buffered log lines.
static LOGGING_GUARD: std::sync::OnceLock<WorkerGuard> = std::sync::OnceLock::new();

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        // NOTE: tauri-plugin-updater requires a `plugins.updater` block in
        // tauri.conf.json (pubkey, endpoints, …). T36 adds that config and
        // re-enables this plugin. Until then `check_for_updates` returns
        // `{ available: false }` so the frontend remains functional.
        // .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            // Logging first so subsequent code can log.
            match logging::init(app.handle()) {
                Ok(guard) => { let _ = LOGGING_GUARD.set(guard); }
                Err(e) => eprintln!("[lda] failed to init logging: {e}"),
            }

            // Settings + AppState.
            let settings = Settings::load(app.handle())?;
            let onboarding_complete = settings.onboarding_complete;
            let app_state = AppState::new(settings);
            app.manage(app_state);

            // Install tray (no-op stub until T18).
            if let Err(e) = tray::install(app.handle()) {
                tracing::warn!(?e, "tray install failed (stub)");
            }

            // Install hotkey listener (no-op stub until T13).
            let hotkey = {
                let state = app.state::<AppState>();
                let inner = state.inner.lock().unwrap();
                inner.settings.hotkey
            };
            if let Err(e) = hotkey::install(app.handle().clone(), hotkey) {
                tracing::warn!(?e, "hotkey install failed (stub)");
            }

            // Open initial window: wizard if first-run, home otherwise.
            let route = if onboarding_complete { "home" } else { "wizard" };
            if let Err(e) = commands::windows::open_window_internal(app.handle(), route) {
                tracing::warn!(?e, "open initial window failed (stub)");
            }

            // Kick off model loading in the background. With no paths configured
            // this returns immediately after transitioning to ModelState::Idle.
            let app_handle_for_load = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle_for_load.state::<AppState>();
                commands::inference::load_models(&app_handle_for_load, state).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::models::models_catalog,
            commands::models::models_list_local,
            commands::models::models_download,
            commands::models::models_cancel_download,
            commands::inference::transcribe,
            commands::inference::clean,
            commands::dictation::manual_dictate,
            commands::permissions::check_accessibility,
            commands::permissions::prompt_accessibility,
            commands::permissions::check_microphone,
            commands::windows::open_window,
            commands::windows::close_window,
            commands::windows::complete_wizard,
            commands::dialog::pick_file,
            commands::updater::check_for_updates,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
