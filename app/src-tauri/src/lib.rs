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
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
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

            crate::models::init_active_downloads();

            // Install tray (no-op stub until T18).
            if let Err(e) = tray::install(app.handle()) {
                tracing::warn!(?e, "tray install failed (stub)");
            }

            // Install hotkey listener. Fails when Accessibility permission
            // is missing — surface that to the UI as a toast so the user
            // doesn't think the app is broken when pressing the hotkey
            // does nothing.
            let hotkey = {
                let state = app.state::<AppState>();
                let inner = state.inner.lock().unwrap();
                inner.settings.hotkey
            };
            if let Err(e) = hotkey::install(app.handle().clone(), hotkey) {
                tracing::warn!(?e, "hotkey install failed");
                let app_for_toast = app.handle().clone();
                let msg = format!("Hotkey unavailable: {e}");
                tauri::async_runtime::spawn(async move {
                    // Defer until the frontend has had time to attach its toast listener.
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    use tauri::Emitter;
                    let _ = app_for_toast.emit("toast", commands::toast("error", msg));
                });
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

            // Register an atexit handler that short-circuits to `_exit(0)`
            // BEFORE any C++ static destructors run. Background:
            // `[NSApplication terminate:]` (Cmd+Q on macOS) ends up calling
            // libc::exit, which calls __cxa_finalize_ranges, which fires
            // ggml-metal's `unique_ptr<ggml_metal_device>` destructor — and
            // that asserts `[rsets->data count] == 0`, but Metal is still
            // draining and the count is non-zero → SIGABRT, "Chiusura
            // inattesa" dialog.
            //
            // atexit handlers run LIFO during __cxa_finalize. We register
            // ours at the end of setup so it fires FIRST, then _exit jumps
            // out of the process before the C++ destructors execute.
            //
            // (RunEvent::ExitRequested was the previous attempt but Tauri
            // never sees Cmd+Q on macOS — AppKit goes straight to exit.)
            extern "C" fn lda_early_exit() {
                unsafe extern "C" {
                    fn _exit(status: std::ffi::c_int) -> !;
                }
                unsafe { _exit(0) }
            }
            unsafe extern "C" {
                fn atexit(cb: extern "C" fn()) -> std::ffi::c_int;
            }
            unsafe { atexit(lda_early_exit); }

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
            commands::inference::get_model_state,
            commands::dictation::manual_dictate,
            commands::dictation::test_mic,
            commands::dictation::cancel_test_mic,
            commands::dictation::list_input_devices,
            commands::permissions::check_accessibility,
            commands::permissions::prompt_accessibility,
            commands::permissions::check_microphone,
            commands::permissions::prompt_microphone,
            commands::permissions::open_system_settings_microphone,
            commands::windows::open_window,
            commands::windows::close_window,
            commands::windows::complete_wizard,
            commands::dialog::pick_file,
            commands::updater::check_for_updates,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Force a clean exit on shutdown via the raw `_exit` syscall
            // (NOT std::process::exit). At quit time ggml-metal's C++ static
            // `unique_ptr<ggml_metal_device>` deleter fires from
            // __cxa_finalize_ranges and asserts the residency-set list is
            // empty — but the Metal command queue is still being drained,
            // so [rsets->data count] != 0 and the process SIGABRTs. macOS
            // then surfaces "Chiusura inattesa" on every quit.
            //
            // std::process::exit DOES run __cxa_finalize, so it doesn't fix
            // the crash. _exit skips atexit handlers and C++ static
            // destructors entirely. Safe here: settings persist on every
            // update, the tracing-appender WorkerGuard already flushed
            // before we get here, and download progress is checkpointed
            // per-chunk in models.rs.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                unsafe extern "C" {
                    fn _exit(status: std::ffi::c_int) -> !;
                }
                unsafe { _exit(0) }
            }
        });
}
