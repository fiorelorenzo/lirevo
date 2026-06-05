//! App data + log directory resolution.
//!
//! Uses the human-readable app name ("Lirevo") instead of Tauri's default
//! bundle-id leaf (`ai.lirevo.app`), and a distinct "(Dev)" suffix for debug
//! builds — so a local `just dev` / `dev-bundle` (debug) and a shipped `.dmg`
//! (release) keep entirely separate models, database, and settings.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

/// Per-app subfolder name: brand name, with a "(Dev)" suffix in debug builds so
/// dev and prod data never mix.
#[must_use]
pub fn app_dir_name() -> &'static str {
    if cfg!(debug_assertions) {
        "Lirevo (Dev)"
    } else {
        "Lirevo"
    }
}

/// Replace the bundle-id leaf of a Tauri-resolved app dir with our app-name dir,
/// keeping the platform-correct base (e.g. `~/Library/Application Support`).
fn rebase(tauri_dir: PathBuf) -> PathBuf {
    match tauri_dir.parent() {
        Some(base) => base.join(app_dir_name()),
        None => tauri_dir,
    }
}

/// Data directory for models, the database, and settings, e.g.
/// `~/Library/Application Support/Lirevo` (or `Lirevo (Dev)` in debug).
pub fn data_dir(app: &AppHandle) -> tauri::Result<PathBuf> {
    Ok(rebase(app.path().app_data_dir()?))
}

/// Log directory, e.g. `~/Library/Logs/Lirevo` (or `Lirevo (Dev)`).
pub fn log_dir(app: &AppHandle) -> tauri::Result<PathBuf> {
    Ok(rebase(app.path().app_log_dir()?))
}

/// One-time, debug-only migration: if the new data dir doesn't exist yet but the
/// legacy bundle-id dir (`.../ai.lirevo.app`) does, move it in so an existing
/// dev install keeps its models + history. Release builds never migrate (they
/// start clean — the legacy dir was only ever written by debug `dev-bundle`s).
#[cfg(debug_assertions)]
pub fn migrate_legacy_data_dir(app: &AppHandle) {
    let (Ok(new), Ok(legacy)) = (data_dir(app), app.path().app_data_dir()) else {
        return;
    };
    if new == legacy || new.exists() || !legacy.exists() {
        return;
    }
    match std::fs::rename(&legacy, &new) {
        Ok(()) => {
            tracing::info!(from = %legacy.display(), to = %new.display(), "migrated legacy data dir");
        }
        Err(e) => tracing::warn!(?e, "failed to migrate legacy data dir"),
    }
}

#[cfg(not(debug_assertions))]
pub fn migrate_legacy_data_dir(_app: &AppHandle) {}
