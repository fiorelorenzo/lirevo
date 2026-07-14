use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::toast;
use crate::state::ModelState;
use crate::{AppError, AppState, Settings};

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, AppError> {
    Ok(state.inner.lock().unwrap().settings.clone())
}

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: serde_json::Value,
) -> Result<Settings, AppError> {
    let (before, after) = {
        let mut inner = state.inner.lock().unwrap();
        let before = inner.settings.clone();
        inner.settings.merge_patch(&patch)?;
        inner.settings.persist(&app)?;
        (before, inner.settings.clone())
    };

    // Side effects (no lock held).
    //
    // Skip the reload path during onboarding: the wizard owns model download
    // and shows its own progress, so its only settings write (`language`, on
    // the language step) must not flash a "Reloading models" toast or kick
    // `load_models` (which early-returns until onboarding completes anyway).
    // `complete_wizard` does the first real load once onboarding is done.
    if Settings::env_affecting_diff(&before, &after) && after.onboarding_complete {
        state.set_model_state(
            &app,
            ModelState::Reloading {
                reason: "settings changed".into(),
            },
        );
        let _ = app.emit("toast", toast("info", "Reloading models..."));
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app2.state::<AppState>();
            crate::commands::inference::load_models(&app2, state).await;
        });
    }
    if before.hotkey != after.hotkey || before.activation_mode != after.activation_mode {
        if let Err(e) = crate::hotkey::reinstall(&app, after.hotkey.clone(), after.activation_mode)
        {
            let busy = matches!(e, AppError::HotkeyBusy);
            tracing::warn!(%e, "hotkey reinstall failed; restoring previous spec");
            let restored = {
                let mut inner = state.inner.lock().unwrap();
                inner.settings.hotkey = before.hotkey.clone();
                inner.settings.activation_mode = before.activation_mode;
                let _ = inner.settings.persist(&app);
                inner.settings.clone()
            };
            // When rejected because a recording is active, `reinstall`'s guard
            // fires before the old coordinator is ever swapped out, so it's
            // still installed — re-issuing `reinstall(before)` would just hit
            // the same guard again for no benefit.
            if !busy {
                let _ =
                    crate::hotkey::reinstall(&app, before.hotkey.clone(), before.activation_mode);
            }
            let message = if busy {
                "Can't change hotkey while recording — try again after"
            } else {
                "Couldn't set that hotkey — reverted"
            };
            let _ = app.emit("toast", toast("error", message));
            return Ok(restored);
        }
    }
    if before.paste_delay_ms != after.paste_delay_ms {
        apply_paste_delay(after.paste_delay_ms);
    }
    if before.launch_at_login != after.launch_at_login {
        update_autostart(&app, after.launch_at_login)?;
    }

    Ok(after)
}

/// The pasteboard injector reads `SIDECAR_INJECT_PASTE_DELAY_MS` from the
/// process environment on every paste. Mirror the settings field into it so
/// the Settings → Paste delay slider actually does something. Also called
/// once at startup so the stored value applies before the first paste.
pub fn apply_paste_delay(ms: u32) {
    // SAFETY: env vars are process-global; mutating one is unsafe in 2024
    // edition. This runs on a Tauri command thread or at setup, before any
    // pasteboard inject can read it, so there's no concurrent reader race.
    unsafe { std::env::set_var("SIDECAR_INJECT_PASTE_DELAY_MS", ms.to_string()) };
}

fn update_autostart(app: &AppHandle, enabled: bool) -> Result<(), AppError> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager
            .enable()
            .map_err(|e| AppError::Settings(format!("autostart enable: {e}")))?;
    } else {
        manager
            .disable()
            .map_err(|e| AppError::Settings(format!("autostart disable: {e}")))?;
    }
    Ok(())
}
