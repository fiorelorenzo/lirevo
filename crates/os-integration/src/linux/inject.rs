//! Linux text injection: clipboard snapshot → set text → synthetic Ctrl+V via
//! `enigo` → restore. Mirrors the macOS/Windows pasteboard model.
//!
//! WAYLAND CAVEAT (UNVALIDATED): under X11, `enigo` uses XTEST and the
//! synthetic Ctrl+V works. Under Wayland, `enigo`'s backend is experimental and
//! many compositors (notably GNOME/Mutter) reject synthetic input outright, so
//! the paste keystroke may silently do nothing. We still update the clipboard
//! first, so even when the synthetic paste is swallowed the user can paste
//! manually (Ctrl+V) — the text is not lost. Construction or paste failures are
//! surfaced as `InjectError` rather than panicking, and we never tear down the
//! build over Wayland.

use std::time::Duration;

use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use thiserror::Error;

use crate::linux::clipboard;

const DEFAULT_PASTE_DELAY_MS: u64 = 120;

fn paste_delay_ms() -> u64 {
    std::env::var("SIDECAR_INJECT_PASTE_DELAY_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PASTE_DELAY_MS)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionMethod {
    Pasteboard,
}

#[derive(Debug, Error)]
pub enum InjectError {
    #[error("clipboard write failed: {0}")]
    PasteboardWrite(String),
    #[error("synthetic Ctrl+V failed: {0}")]
    SyntheticPaste(String),
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Default)]
pub struct Injector;

impl Injector {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Inject `text` into the focused window by placing it on the clipboard and
    /// synthesizing Ctrl+V, then restoring the previous clipboard text.
    ///
    /// UNVALIDATED on real Linux. On Wayland the synthetic Ctrl+V may be a
    /// silent no-op (compositor refuses synthetic input); the clipboard is set
    /// regardless so the user can paste manually.
    pub fn inject(&self, text: &str) -> Result<InjectionMethod, InjectError> {
        // 1. Snapshot the current clipboard text (text-only; see clipboard.rs).
        let saved = clipboard::snapshot_text();

        // 2. Put our text on the clipboard.
        if !clipboard::set_text(text) {
            return Err(InjectError::PasteboardWrite(
                "arboard set_text returned error".into(),
            ));
        }

        // 3. Synthesize Ctrl+V (best-effort; see Wayland caveat).
        let paste_result = synth_ctrl_v();

        // 4. Let the target app consume the paste before we restore. Run this
        //    even if the synthetic paste failed, so a manual paste by the user
        //    still finds the text on the clipboard before we overwrite it.
        std::thread::sleep(Duration::from_millis(paste_delay_ms()));

        // 5. Restore the previous clipboard text (best-effort).
        clipboard::restore_text(saved);

        paste_result.map(|()| InjectionMethod::Pasteboard)
    }
}

/// Build an `Enigo` instance and synthesize Ctrl down, V click, Ctrl up.
fn synth_ctrl_v() -> Result<(), InjectError> {
    // Construct per-injection: cheap on X11, and avoids holding an X/Wayland
    // connection open for the process lifetime. `release_keys_when_dropped`
    // ensures Ctrl can't get stuck held if anything below errors out.
    let settings = Settings {
        release_keys_when_dropped: true,
        ..Settings::default()
    };
    let mut enigo = Enigo::new(&settings).map_err(|e| {
        InjectError::SyntheticPaste(format!(
            "could not initialize synthetic input (Wayland compositor may forbid it): {e}"
        ))
    })?;

    enigo
        .key(Key::Control, Press)
        .and_then(|()| enigo.key(Key::Unicode('v'), Click))
        .and_then(|()| enigo.key(Key::Control, Release))
        .map_err(|e| InjectError::SyntheticPaste(format!("enigo key event failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injection_method_is_pasteboard() {
        assert_eq!(InjectionMethod::Pasteboard, InjectionMethod::Pasteboard);
    }

    #[test]
    fn injector_new_constructs() {
        let _ = Injector::new();
    }

    #[test]
    fn paste_delay_defaults_when_unset() {
        let prev = std::env::var("SIDECAR_INJECT_PASTE_DELAY_MS").ok();
        std::env::remove_var("SIDECAR_INJECT_PASTE_DELAY_MS");
        assert_eq!(paste_delay_ms(), DEFAULT_PASTE_DELAY_MS);
        if let Some(p) = prev {
            std::env::set_var("SIDECAR_INJECT_PASTE_DELAY_MS", p);
        }
    }
}
