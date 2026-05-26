use tauri::{AppHandle, Emitter, Manager, State};

use crate::{AppError, AppState, Settings};
use crate::state::ModelState;
use crate::commands::toast;

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
    if Settings::env_affecting_diff(&before, &after) {
        state.set_model_state(&app, ModelState::Reloading { reason: "settings changed".into() });
        let _ = app.emit("toast", toast("info", "Reloading models..."));
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app2.state::<AppState>();
            crate::commands::inference::load_models(&app2, state).await;
        });
    } else if !before.keep_models_warm && after.keep_models_warm {
        // User flipped keep-warm on with the models already loaded (no
        // env-affecting change to trigger a reload). Without this path,
        // the user has to either change a model path or restart the app
        // before the warm-up actually runs against the live handles —
        // which doesn't match the intent of "turn it on so my next
        // dictation is fast".
        //
        // Skipped in the env_affecting branch above because that reload
        // will warm up at the end of load_models anyway; firing here too
        // would warm a cold (still-loading) handle and then fight the
        // reload's own warm-up on the new handle.
        let (stt, llm) = {
            let inner = state.inner.lock().unwrap();
            (inner.stt.clone(), inner.llm.clone())
        };
        if stt.is_some() || llm.is_some() {
            tokio::task::spawn_blocking(move || {
                crate::commands::inference::warm_up(stt, llm);
            });
        }
    }
    if before.hotkey != after.hotkey {
        crate::hotkey::reinstall(&app, after.hotkey)?;
    }
    if before.force_pasteboard != after.force_pasteboard {
        state.rebuild_injector(after.force_pasteboard);
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
        manager.enable().map_err(|e| AppError::Settings(format!("autostart enable: {e}")))?;
    } else {
        manager.disable().map_err(|e| AppError::Settings(format!("autostart disable: {e}")))?;
    }
    Ok(())
}
