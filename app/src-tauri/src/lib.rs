mod commands;
mod db;
mod engine;
mod error;
mod hotkey;
mod logging;
mod models;
mod paths;
mod settings;
mod state;
mod stt;
mod tray;

pub use error::AppError;
pub use settings::Settings;
pub use state::{AppState, ModelState};

use tauri::{Emitter, Manager};
use tracing_appender::non_blocking::WorkerGuard;

// Hold the guard for the program's lifetime to avoid losing buffered log lines.
static LOGGING_GUARD: std::sync::OnceLock<WorkerGuard> = std::sync::OnceLock::new();

// Flipped to true the moment Tauri reports `RunEvent::ExitRequested`. The
// macOS SIGABRT handler only short-circuits to `_exit(0)` when this is set
// — otherwise it restores the default handler and re-raises so genuine
// runtime aborts (assertion failures, double-frees, panic=abort) still
// surface in the crash reporter instead of being silently swallowed.
#[cfg(target_os = "macos")]
pub(crate) static LIREVO_EXIT_REQUESTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// macOS-only: register a late `atexit` handler that flips `LIREVO_EXIT_REQUESTED`
/// before `libc::exit`'s C++ destructor chain runs. Call this AFTER ggml-metal
/// has been touched (i.e. after a parakeet-cpp / llama-cpp-2 load) so our
/// handler is registered LATER than ggml's `__cxa_atexit` and therefore runs
/// EARLIER in the LIFO finalize order. Idempotent in effect — multiple
/// registrations just queue multiple no-op flag flips. Safe to call from
/// every `load_models` invocation.
#[cfg(target_os = "macos")]
pub(crate) fn register_quit_safety_atexit() {
    extern "C" fn flip_exit_flag() {
        LIREVO_EXIT_REQUESTED.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    unsafe extern "C" {
        fn atexit(handler: extern "C" fn()) -> std::ffi::c_int;
    }
    // Return value is 0 on success, non-zero on the (extremely unlikely)
    // failure to register. A failure here means we fall back to the
    // existing RunEvent::ExitRequested path; logging it is enough.
    let rc = unsafe { atexit(flip_exit_flag) };
    if rc != 0 {
        tracing::warn!(rc, "atexit registration for quit-safety handler failed");
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn register_quit_safety_atexit() {
    // Non-macOS hosts don't have the ggml-metal teardown crash path.
}

/// Show the Dock icon (Regular) when any real (non-overlay) window is visible;
/// otherwise stay menu-bar-only (Accessory). macOS-only effect; a no-op on
/// other platforms so it is safe to call unconditionally.
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
pub(crate) fn refresh_activation_policy(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        let any_visible = app
            .webview_windows()
            .iter()
            .filter(|(label, _)| label.as_str() != "overlay")
            .any(|(_, w)| w.is_visible().unwrap_or(false));
        let policy = if any_visible {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        let _ = app.set_activation_policy(policy);
    }
}

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
            // Login auto-launch passes this flag so the app starts silently in
            // the tray; a manual launch (no flag) shows a window. This is what
            // replaced the old `launch_minimized` setting.
            Some(vec!["--minimized"]),
        ))
        .setup(|app| {
            // Logging first so subsequent code can log.
            match logging::init(app.handle()) {
                Ok(guard) => { let _ = LOGGING_GUARD.set(guard); }
                Err(e) => eprintln!("[lda] failed to init logging: {e}"),
            }

            // Data/log dirs are app-name based (Lirevo / "Lirevo (Dev)"),
            // separate for dev vs prod. On debug builds, move any legacy
            // bundle-id data dir into the new location before anything reads it.
            paths::migrate_legacy_data_dir(app.handle());

            // Menu-bar / agent app: never show a Dock icon. `LSUIElement` in
            // Info.plist declares this, but tao forces the Regular activation
            // policy at launch and overrides it — set Accessory explicitly.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Settings + AppState.
            let settings = Settings::load(app.handle())?;
            let onboarding_complete = settings.onboarding_complete;
            let start_minimized = settings.start_minimized;
            // Apply the persisted paste delay before any pasteboard inject
            // could run — the injector reads it from env on every paste.
            commands::settings::apply_paste_delay(settings.paste_delay_ms);
            // Open the local DB once at startup. A broken/locked file falls back
            // to an in-memory DB (logged) so history never blocks app launch.
            let data_dir = paths::data_dir(app.handle())?;
            std::fs::create_dir_all(&data_dir).ok();
            let db = std::sync::Arc::new(crate::db::Db::open_or_memory(&data_dir.join("data.db")));
            let models_dir = crate::models::models_dir(app.handle())
                .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;
            let app_state = AppState::new(app.handle(), settings, db, models_dir);
            app.manage(app_state);

            crate::models::init_active_downloads();

            // Install tray.
            if let Err(e) = tray::install(app.handle()) {
                tracing::warn!(?e, "tray install failed");
            }

            // Install hotkey listener — but only when Accessibility is
            // already granted. Calling `CGEventTapCreate` against an
            // untrusted process triggers macOS' "this app would like to
            // control your computer using accessibility features" dialog,
            // and we don't want that to fire at app launch (before the
            // wizard has even rendered the explanation + button). The
            // wizard's accessibility step calls `prompt_accessibility`
            // when the user clicks the grant button — that's the right
            // moment for the dialog. After grant, the home page's
            // `permissionsState` store polls AX status and calls
            // `retry_hotkey_install` from its `$effect`, which performs
            // the deferred install transparently.
            let (hotkey, activation_mode) = {
                let state = app.state::<AppState>();
                let inner = state.inner.lock().unwrap();
                (inner.settings.hotkey.clone(), inner.settings.activation_mode)
            };
            if os_integration::check_accessibility()
                == os_integration::PermissionStatus::Granted
            {
                if let Err(e) = hotkey::install(app.handle().clone(), hotkey, activation_mode) {
                    tracing::warn!(?e, "hotkey install failed");
                }
            } else {
                tracing::info!(
                    "hotkey install deferred: Accessibility not granted yet — wizard / home will retry after the user grants it"
                );
            }

            // Open the initial window: wizard on first run, home otherwise.
            // A login auto-launch passes `--minimized`; the `start_minimized`
            // setting achieves the same effect on every launch. The wizard
            // always opens on first run regardless — hiding it would leave the
            // user no way to configure the app.
            let autostarted = std::env::args().any(|a| a == "--minimized");
            let should_open_window = !onboarding_complete || (!autostarted && !start_minimized);
            if should_open_window {
                let route = if onboarding_complete { "home" } else { "wizard" };
                if let Err(e) = commands::windows::open_window_internal(app.handle(), route) {
                    tracing::warn!(?e, "open initial window failed (stub)");
                }
            } else {
                tracing::info!(
                    "starting silently in the tray (no initial window)"
                );
            }
            // Dock icon: show when a real window is visible, hide otherwise.
            refresh_activation_policy(app.handle());

            // Always create the recording overlay up-front so it's ready to
            // be shown the moment the user hits the hotkey. It stays hidden
            // until `recording:state = true`.
            if let Err(e) = commands::windows::open_window_internal(app.handle(), "overlay") {
                tracing::warn!(?e, "overlay window install failed");
            }

            // Spawn the resource monitor, profile selector, and engine
            // lifecycle loop. The selector pushes profile decisions into the
            // engine's policy (PUSH via subscribe_changes); the engine consumes
            // raw signals for unload/preload. monitor + selector are kept alive
            // for the app process lifetime by the spawned task (the
            // lifecycle_loop await never returns until the channel closes at
            // shutdown).
            // Give the Engine a handle so its load/unload/reload transitions
            // emit the informational `engine:llm_state_changed` event. Done
            // synchronously here (before the spawns below) so it's installed
            // before the startup `load_models` task can call `ensure_llm`. This
            // event is separate from `model:state` — an idle-unload does NOT
            // regress the UI's Ready lifecycle.
            {
                let state = app.handle().state::<AppState>();
                let engine = state.inner.lock().unwrap().engine.clone();
                engine.set_state_reporter(app.handle().clone());
            }

            let app_handle_for_lifecycle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let engine = {
                    let state = app_handle_for_lifecycle.state::<AppState>();
                    let inner = state.inner.lock().unwrap();
                    inner.engine.clone()
                };

                match resource_monitor::ResourceMonitor::spawn().await {
                    Ok(monitor) => {
                        // Start from the persisted profile mode (Auto unless the
                        // user pinned one in Settings). An unparseable stored
                        // value falls back to Auto.
                        let initial_mode = {
                            let state = app_handle_for_lifecycle.state::<AppState>();
                            let inner = state.inner.lock().unwrap();
                            inference_core::profile::mode_from_str(&inner.settings.profile_mode)
                                .unwrap_or(inference_core::profile::ProfileMode::Auto)
                        };
                        let selector = inference_core::profile::ProfileSelector::new(
                            monitor.subscribe(),
                            initial_mode,
                            inference_core::profile::ProfileName::Balanced,
                        );
                        // Make the selector reachable from the profile commands.
                        {
                            let state = app_handle_for_lifecycle.state::<AppState>();
                            state.set_profile_selector(selector.clone());
                        }
                        // Feed selector decisions into the engine policy (push)
                        // and mirror each change to the frontend as a
                        // `profile:changed` event (plus an emergency toast).
                        let engine_for_policy = engine.clone();
                        let app_for_events = app_handle_for_lifecycle.clone();
                        // `selector` is moved into this task; the task lives as
                        // long as the watch channel (i.e. as long as the
                        // selector itself) so this keeps the selector alive.
                        let mut changes = selector.subscribe_changes();
                        tauri::async_runtime::spawn(async move {
                            let selector = selector; // keep alive for the task
                            loop {
                                let profile = *changes.borrow_and_update();
                                engine_for_policy.set_policy(
                                    inference_core::profile::policy_for(profile),
                                );
                                let status = crate::commands::profile::ProfileStatus {
                                    active: profile,
                                    mode: inference_core::profile::mode_to_str(
                                        selector.current_mode(),
                                    )
                                    .to_string(),
                                    emergency: selector
                                        .emergency()
                                        .map(inference_core::profile::emergency_label),
                                };
                                let _ = app_for_events.emit("profile:changed", &status);
                                // Rebuild the tray so the energy submenu's
                                // checkmarks + active-profile status reflect the
                                // fresh decision live.
                                crate::tray::refresh(&app_for_events);
                                if let Some(reason) = selector.emergency() {
                                    let _ = app_for_events.emit(
                                        "toast",
                                        crate::commands::toast(
                                            "warn",
                                            format!(
                                                "Switched to Power Saver: {}",
                                                inference_core::profile::emergency_label(reason)
                                            ),
                                        ),
                                    );
                                }
                                if changes.changed().await.is_err() {
                                    break;
                                }
                            }
                        });
                        // Engine reacts to raw signals for unload/preload. The
                        // trailing `drop(monitor)` is load-bearing: it extends
                        // `monitor`'s lifetime across the await. Dropping the
                        // ResourceMonitor aborts its tick task AND closes the
                        // broadcast sender, so if NLL dropped it right after
                        // `subscribe()` the signal stream would die before the
                        // loop ever ran.
                        let signals = monitor.subscribe();
                        engine.lifecycle_loop(signals).await;
                        drop(monitor);
                    }
                    Err(e) => {
                        tracing::warn!(
                            ?e,
                            "resource monitor unavailable; engine runs without auto-unload"
                        );
                    }
                }
            });

            // Initial model load for the UI's ModelState (Loading→Ready) +
            // warm-up + preload. With no paths configured this returns after
            // transitioning to ModelState::Idle. load_models refreshes the
            // engine config + ensures the slots; it also calls
            // `register_quit_safety_atexit` on completion so the macOS
            // shutdown-abort short-circuit (see below) works for both the
            // initial load and any subsequent reloads from Settings.
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
            // CRITICAL: the handler only short-circuits to `_exit(0)` when
            // `LIREVO_EXIT_REQUESTED` is set. The flag flips in two places:
            //   1. The `RunEvent::ExitRequested` branch below — covers Cmd+Q,
            //      tray Quit, and any other path Tauri surfaces as an exit
            //      event. We then call `_exit(0)` directly and never reach
            //      `libc::exit` at all.
            //   2. An `atexit` handler registered by `register_quit_safety_atexit`
            //      from inside `load_models` AFTER ggml-metal has been
            //      initialized. This covers the paths Tauri does NOT surface
            //      as `ExitRequested` (notably Dock right-click → Quit on some
            //      macOS versions), where `libc::exit` is reached without our
            //      event handler firing. atexit handlers run in LIFO order
            //      during `__cxa_finalize`; by registering AFTER ggml's
            //      `__cxa_atexit` we guarantee our flag-flip runs BEFORE
            //      ggml's destructor aborts. Registering this in `setup()`
            //      directly would be the wrong order — ggml hasn't initialized
            //      yet and our handler would be the OLDEST entry, running LAST
            //      (i.e. after the abort, too late).
            //
            // Outside the exit sequence the handler restores the default and
            // re-raises so real runtime aborts (panics, debug assertions,
            // double-frees) still surface as crashes rather than being
            // swallowed silently with exit 0.
            //
            // No conflict with tokio's signal handling — tokio uses
            // kqueue/signalfd, not the C `signal()` table.
            #[cfg(target_os = "macos")]
            {
                extern "C" fn lirevo_sigabrt_handler(sig: std::ffi::c_int) {
                    unsafe extern "C" {
                        fn _exit(status: std::ffi::c_int) -> !;
                        fn signal(
                            signum: std::ffi::c_int,
                            handler: usize,
                        ) -> *const ();
                        fn raise(sig: std::ffi::c_int) -> std::ffi::c_int;
                    }
                    use std::sync::atomic::Ordering;
                    if LIREVO_EXIT_REQUESTED.load(Ordering::SeqCst) {
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
                // Two `signal` declarations exist in the lirevo_sigabrt_handler
                // body above (handler typed as `usize` so we can pass SIG_DFL)
                // and here (handler typed as the fn pointer we install). Rust
                // surfaces this as `clashing_extern_declarations`; both shapes
                // are correct for their callsite — installing uses a typed fn
                // pointer, restoring uses 0. Keeping them separate avoids a
                // transmute. Allowed locally with a one-line justification.
                #[allow(clashing_extern_declarations)]
                unsafe extern "C" {
                    fn signal(
                        signum: std::ffi::c_int,
                        handler: extern "C" fn(std::ffi::c_int),
                    ) -> *const ();
                }
                const SIGABRT: std::ffi::c_int = 6;
                unsafe { signal(SIGABRT, lirevo_sigabrt_handler); }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::models::models_catalog,
            commands::models::get_stt_catalog,
            commands::models::models_list_local,
            commands::models::models_download,
            commands::models::stt_download,
            commands::models::models_cancel_download,
            commands::models::models_delete,
            commands::inference::transcribe,
            commands::inference::clean,
            commands::inference::get_model_state,
            commands::inference::get_active_backend,
            commands::inference::reload_models,
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
            commands::history::history_list,
            commands::history::history_get,
            commands::history::history_delete,
            commands::history::history_clear,
            commands::profile::profile_get,
            commands::profile::profile_set_mode,
            commands::hotkey::start_hotkey_capture,
            commands::hotkey::stop_hotkey_capture,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Intercept window close: hide the window instead of letting Tauri
            // destroy it, so the menu-bar app keeps running and the user can
            // re-open instantly from the tray without re-creating the webview.
            // Quit is via the tray menu. Skipped for the overlay (its own
            // show/hide lifecycle driven by recording state).
            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } = &event
            {
                if label != "overlay" {
                    api.prevent_close();
                    if let Some(w) = app.get_webview_window(label) {
                        let _ = w.hide();
                    }
                    refresh_activation_policy(app);
                    return;
                }
            }

            // Reopen (Finder / Launchpad / Spotlight while already running):
            // with no Dock icon, this is how the user re-summons the UI — show
            // the home window (or the wizard if onboarding isn't finished).
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = &event {
                let route = app.try_state::<AppState>().map_or("home", |s| {
                    if s.inner.lock().unwrap().settings.onboarding_complete {
                        "home"
                    } else {
                        "wizard"
                    }
                });
                let _ = commands::windows::open_window_internal(app, route);
            }
            // Flip the macOS shutdown flag so the SIGABRT handler installed
            // in setup() converts ggml-metal's teardown abort into
            // `_exit(0)`. Outside this branch the handler re-raises with
            // the default action, preserving real-crash visibility.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                #[cfg(target_os = "macos")]
                {
                    LIREVO_EXIT_REQUESTED.store(true, std::sync::atomic::Ordering::SeqCst);
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
