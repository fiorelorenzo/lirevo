//! `NSPasteboard` save-set-⌘V-restore for text injection fallback.

use std::time::Duration;

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

use crate::inject::InjectError;

const DEFAULT_PASTE_DELAY_MS: u64 = 120;

fn paste_delay_ms() -> u64 {
    std::env::var("SIDECAR_INJECT_PASTE_DELAY_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PASTE_DELAY_MS)
}

/// Last-resort clipboard write: replaces the general pasteboard's string
/// content with `text`. Used as a fallback when text injection fails so the
/// user can paste manually. Returns `true` on success.
pub fn set_text(text: &str) -> bool {
    // SAFETY: NSPasteboard methods are safe to call from any thread on macOS.
    unsafe {
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        let ns = NSString::from_str(text);
        pb.setString_forType(&ns, NSPasteboardTypeString)
    }
}

pub(crate) fn pasteboard_inject(text: &str) -> Result<(), InjectError> {
    // SAFETY: NSPasteboard methods are safe to call from any thread on macOS.
    // All Retained<T> values are properly reference-counted by objc2.
    unsafe {
        let pb = NSPasteboard::generalPasteboard();

        // 1. Save current string content (if any).
        let pb_type = NSPasteboardTypeString;
        let saved: Option<String> = pb
            .stringForType(pb_type)
            .map(|s| s.to_string());

        // 2. Clear and set our text.
        pb.clearContents();
        let our_ns = NSString::from_str(text);
        let ok = pb.setString_forType(&our_ns, pb_type);
        if !ok {
            return Err(InjectError::PasteboardWrite(
                "setString_forType returned false".into(),
            ));
        }

        // 3. Synthesize Cmd+V.
        synth_cmd_v()?;

        // 4. Wait for the target app to consume the paste.
        std::thread::sleep(Duration::from_millis(paste_delay_ms()));

        // 5. Restore saved content (or just clear).
        pb.clearContents();
        if let Some(prev) = saved {
            let prev_ns = NSString::from_str(&prev);
            let _ = pb.setString_forType(&prev_ns, pb_type);
        }

        Ok(())
    }
}

fn synth_cmd_v() -> Result<(), InjectError> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|()| InjectError::SyntheticPaste("CGEventSource::new failed".into()))?;

    // Virtual key code for 'V' on US keyboard layout.
    let v_keycode: u16 = 0x09;

    let down = CGEvent::new_keyboard_event(source.clone(), v_keycode, true)
        .map_err(|()| InjectError::SyntheticPaste("new_keyboard_event (down) failed".into()))?;
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    down.post(CGEventTapLocation::HID);

    let up = CGEvent::new_keyboard_event(source, v_keycode, false)
        .map_err(|()| InjectError::SyntheticPaste("new_keyboard_event (up) failed".into()))?;
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    up.post(CGEventTapLocation::HID);

    Ok(())
}
