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

// Flipped to true the moment Tauri reports `RunEvent::ExitRequested`. The
// macOS SIGABRT handler only short-circuits to `_exit(0)` when this is set
// — otherwise it restores the default handler and re-raises so genuine
// runtime aborts (assertion failures, double-frees, panic=abort) still
// surface in the crash reporter instead of being silently swallowed.
#[cfg(target_os = "macos")]
static LDA_EXIT_REQUESTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

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
            // Apply the persisted paste delay before any pasteboard inject
            // could run — the injector reads it from env on every paste.
            commands::settings::apply_paste_delay(settings.paste_delay_ms);
            let app_state = AppState::new(settings);
            app.manage(app_state);

            crate::models::init_active_downloads();

            // Install tray.
            if let Err(e) = tray::install(app.handle()) {
                tracing::warn!(?e, "tray install failed");
            }

            // Install hotkey listener. Fails when Accessibility permission
            // is missing; the home page's `permissionsState` store polls
            // AX status and calls `retry_hotkey_install` from its `$effect`
            // when it flips to granted, so we don't surface a toast here
            // (the persistent banner already covers the case visually).
            let hotkey = {
                let state = app.state::<AppState>();
                let inner = state.inner.lock().unwrap();
                inner.settings.hotkey
            };
            if let Err(e) = hotkey::install(app.handle().clone(), hotkey) {
                tracing::warn!(?e, "hotkey install failed");
            }

            // Open initial window: wizard if first-run, home otherwise.
            let route = if onboarding_complete { "home" } else { "wizard" };
            if let Err(e) = commands::windows::open_window_internal(app.handle(), route) {
                tracing::warn!(?e, "open initial window failed (stub)");
            }

            // Always create the recording overlay up-front so it's ready to
            // be shown the moment the user hits the hotkey. It stays hidden
            // until `recording:state = true`.
            if let Err(e) = commands::windows::open_window_internal(app.handle(), "overlay") {
                tracing::warn!(?e, "overlay window install failed");
            }

            // Kick off model loading in the background. With no paths configured
            // this returns immediately after transitioning to ModelState::Idle.
            let app_handle_for_load = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle_for_load.state::<AppState>();
                commands::inference::load_models(&app_handle_for_load, state).await;
            });

            // macOS-only: install a scoped SIGABRT handler.
            //
            // Full crash path we work around:
            //   [NSApplication terminate:]
            //     → libc::exit
            //       → __cxa_finalize_ranges
            //         → ~unique_ptr<ggml_metal_device>()
            //           → ggml_metal_rsets_free
            //             → GGML_ASSERT([rsets->data count] == 0)  // Metal still draining
            //               → ggml_abort → abort() → raise(SIGABRT)
            //
            // We tried `atexit` first, but C++ statics register their own
            // __cxa_atexit handlers when first initialized — ggml-metal
            // initializes inside `load_models`, which is spawned as a
            // background tokio task that runs AFTER setup. So our atexit
            // (registered in setup) is registered BEFORE ggml-metal's, and
            // LIFO ordering means ggml's handler fires first → crash.
            //
            // CRITICAL: the handler only short-circuits to `_exit(0)` when
            // `LDA_EXIT_REQUESTED` is set (flipped in the `RunEvent::
            // ExitRequested` branch below). Outside the exit sequence we
            // restore the default handler and re-raise, so real runtime
            // aborts (panics, debug assertions, double-frees) still surface
            // as crashes rather than being swallowed silently with exit 0.
            //
            // No conflict with tokio's signal handling — tokio uses
            // kqueue/signalfd, not the C `signal()` table.
            #[cfg(target_os = "macos")]
            {
                extern "C" fn lda_sigabrt_handler(sig: std::ffi::c_int) {
                    unsafe extern "C" {
                        fn _exit(status: std::ffi::c_int) -> !;
                        fn signal(
                            signum: std::ffi::c_int,
                            handler: usize,
                        ) -> *const ();
                        fn raise(sig: std::ffi::c_int) -> std::ffi::c_int;
                    }
                    use std::sync::atomic::Ordering;
                    if LDA_EXIT_REQUESTED.load(Ordering::SeqCst) {
                        unsafe { _exit(0) }
                    }
                    // Genuine runtime abort. Re-raise with default handler
                    // so the OS crash reporter sees it.
                    const SIG_DFL: usize = 0;
                    unsafe {
                        signal(sig, SIG_DFL);
                        raise(sig);
                    }
                }
                unsafe extern "C" {
                    fn signal(
                        signum: std::ffi::c_int,
                        handler: extern "C" fn(std::ffi::c_int),
                    ) -> *const ();
                }
                const SIGABRT: std::ffi::c_int = 6;
                unsafe { signal(SIGABRT, lda_sigabrt_handler); }
            }

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
            commands::permissions::open_system_settings_accessibility,
            commands::permissions::retry_hotkey_install,
            commands::permissions::frontend_log,
            commands::windows::open_window,
            commands::windows::close_window,
            commands::windows::complete_wizard,
            commands::dialog::pick_file,
            commands::updater::check_for_updates,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Flip the macOS shutdown flag so the SIGABRT handler installed
            // in setup() converts ggml-metal's teardown abort into
            // `_exit(0)`. Outside this branch the handler re-raises with
            // the default action, preserving real-crash visibility.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                #[cfg(target_os = "macos")]
                {
                    LDA_EXIT_REQUESTED.store(true, std::sync::atomic::Ordering::SeqCst);
                    unsafe extern "C" {
                        fn _exit(status: std::ffi::c_int) -> !;
                    }
                    unsafe { _exit(0) }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    std::process::exit(0);
                }
            }
        });
}
