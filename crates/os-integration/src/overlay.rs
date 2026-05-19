//! Native window adjustments for the recording overlay.
//!
//! Tauri's portable API can't express the combination the overlay needs:
//! float above frontmost-app windows, follow the user across spaces /
//! stay out of cmd-tab, pass mouse clicks through to the app below.
//! macOS expresses this via `NSWindow` properties; other platforms have
//! analogues (`WS_EX_TRANSPARENT` + `HWND_TOPMOST` on Windows, layer-
//! shell on Wayland) but they aren't wired yet.
//!
//! This module hides all the unsafe `ObjC` plumbing from the Tauri host so
//! the host's only platform branch is the one around obtaining the native
//! window handle from Tauri itself.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OverlayError {
    #[error("overlay tweaks not implemented on this platform")]
    NotSupported,
    #[error("internal: {0}")]
    Internal(String),
}

/// Make a window float above frontmost-app windows, stay sticky across
/// spaces, and pass clicks through to whatever is underneath.
///
/// On macOS this performs the matching `ObjC` `setLevel:`,
/// `setCollectionBehavior:`, and `setIgnoresMouseEvents:` calls and logs
/// the readback values. On other platforms it returns `NotSupported`.
///
/// # Safety
/// `ns_window` must be a live, non-null `NSWindow *` belonging to this
/// process. Pointers obtained from `tauri::WebviewWindow::ns_window()`
/// satisfy this. The macOS path is otherwise self-contained and does not
/// retain the pointer beyond the call.
pub unsafe fn apply_floating_click_through(
    ns_window: *mut std::ffi::c_void,
) -> Result<(), OverlayError> {
    // SAFETY: forwarded contract — caller upholds the live-NSWindow
    // invariant; impl below uses the pointer only for the duration of
    // the call.
    unsafe { apply_floating_click_through_impl(ns_window) }
}

#[cfg(target_os = "macos")]
unsafe fn apply_floating_click_through_impl(
    ns_window: *mut std::ffi::c_void,
) -> Result<(), OverlayError> {
    use objc2::msg_send;

    if ns_window.is_null() {
        return Err(OverlayError::Internal("ns_window is null".into()));
    }
    let ns_window = ns_window.cast::<objc2::runtime::AnyObject>();
    // SAFETY: forwarded — `ns_window` is a live NSWindow as per the
    // public function's safety contract.
    unsafe {
        // NSStatusWindowLevel = 25 — above NSNormalWindowLevel (0) and
        // NSFloatingWindowLevel (3), below the screensaver / menu.
        // setLevel: takes NSInteger; on 64-bit macOS that's i64.
        let _: () = msg_send![ns_window, setLevel: 25_i64];
        // CanJoinAllSpaces (1) | Stationary (16) | IgnoresCycle (64).
        let _: () = msg_send![ns_window, setCollectionBehavior: 81_u64];
        // Clicks fall through to the app below.
        let _: () = msg_send![ns_window, setIgnoresMouseEvents: true];
        // Read it back so we can confirm in the logs whether the setters
        // actually stuck.
        let level: i64 = msg_send![ns_window, level];
        let behavior: u64 = msg_send![ns_window, collectionBehavior];
        let ignores: bool = msg_send![ns_window, ignoresMouseEvents];
        tracing::info!(level, behavior, ignores, "overlay: NSWindow attrs after set");
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
unsafe fn apply_floating_click_through_impl(
    _ns_window: *mut std::ffi::c_void,
) -> Result<(), OverlayError> {
    Err(OverlayError::NotSupported)
}
