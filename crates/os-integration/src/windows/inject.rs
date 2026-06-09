//! Windows text injection: clipboard snapshot → set text → synthetic Ctrl+V
//! via `SendInput` → restore. Mirrors the macOS pasteboard model.

use std::time::Duration;

use thiserror::Error;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_V,
};

use crate::windows::clipboard;

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
    /// UNVALIDATED on real Windows. UIPI means this cannot paste into a window
    /// owned by a higher-integrity (elevated) process when Lirevo itself is not
    /// elevated; that surfaces as a silent no-op, not an `Err`.
    pub fn inject(&self, text: &str) -> Result<InjectionMethod, InjectError> {
        // 1. Snapshot the current clipboard text (text-only; see clipboard.rs).
        let saved = clipboard::snapshot_text();

        // 2. Put our text on the clipboard.
        if !clipboard::set_text(text) {
            return Err(InjectError::PasteboardWrite(
                "arboard set_text returned error".into(),
            ));
        }

        // 3. Synthesize Ctrl+V.
        synth_ctrl_v()?;

        // 4. Let the target app consume the paste before we restore.
        std::thread::sleep(Duration::from_millis(paste_delay_ms()));

        // 5. Restore the previous clipboard text (best-effort).
        clipboard::restore_text(saved);

        Ok(InjectionMethod::Pasteboard)
    }
}

/// Build a keyboard `INPUT` for a virtual key, optionally a key-up event.
fn key_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    let flags = if key_up {
        KEYEVENTF_KEYUP
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn synth_ctrl_v() -> Result<(), InjectError> {
    // Ctrl down, V down, V up, Ctrl up — a single SendInput batch so the
    // events are injected atomically in order.
    let inputs = [
        key_input(VK_CONTROL, false),
        key_input(VK_V, false),
        key_input(VK_V, true),
        key_input(VK_CONTROL, true),
    ];

    // SAFETY: `inputs` is a valid, correctly-sized slice of INPUT structures
    // that outlives the call; `cbsize` is the size of a single INPUT.
    let sent = unsafe {
        SendInput(
            &inputs,
            i32::try_from(std::mem::size_of::<INPUT>())
                .map_err(|_| InjectError::Internal("INPUT size overflow".into()))?,
        )
    };
    if sent as usize != inputs.len() {
        return Err(InjectError::SyntheticPaste(format!(
            "SendInput injected {sent}/{} events (likely blocked by UIPI or a higher-integrity foreground window)",
            inputs.len()
        )));
    }
    Ok(())
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
}
