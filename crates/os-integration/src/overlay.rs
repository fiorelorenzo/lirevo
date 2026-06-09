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

/// Windows: make the overlay click-through (`WS_EX_TRANSPARENT` +
/// `WS_EX_LAYERED`) and topmost so synthetic input and mouse clicks fall
/// through to the app underneath while the overlay floats above it.
///
/// The `native_window` pointer is the Win32 `HWND` of the overlay window. The
/// macOS path receives an `NSWindow *` from `tauri::WebviewWindow::ns_window()`;
/// the Windows analogue is `tauri::WebviewWindow::hwnd()`. The host currently
/// only wires the macOS call site, so this path is UNVALIDATED — it is kept
/// signature-compatible so a future host branch can call it unchanged.
#[cfg(target_os = "windows")]
unsafe fn apply_floating_click_through_impl(
    native_window: *mut std::ffi::c_void,
) -> Result<(), OverlayError> {
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT,
    };

    if native_window.is_null() {
        return Err(OverlayError::Internal("hwnd is null".into()));
    }
    let hwnd = HWND(native_window);

    // SAFETY: forwarded contract — `native_window` is a live HWND owned by this
    // process for the duration of the call.
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        // WS_EX_TRANSPARENT: clicks pass through; WS_EX_LAYERED: required for a
        // transparent window to actually be hit-test-transparent; WS_EX_NOACTIVATE:
        // never steal focus from the dictation target.
        // The three ex-style bits fit comfortably in 32 bits; widening the u32
        // mask to the isize the API takes never wraps.
        #[allow(clippy::cast_possible_wrap)]
        let added = (WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0 | WS_EX_NOACTIVATE.0) as isize;
        let new = current | added;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new);
        // Float above the frontmost app's windows (analogue of macOS
        // NSStatusWindowLevel) without moving, resizing, or activating it.
        if let Err(e) = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        ) {
            return Err(OverlayError::Internal(format!("SetWindowPos: {e}")));
        }
        tracing::info!(exstyle = new, "overlay: HWND ex-style after set");
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
unsafe fn apply_floating_click_through_impl(
    _ns_window: *mut std::ffi::c_void,
) -> Result<(), OverlayError> {
    Err(OverlayError::NotSupported)
}

/// Top inset (in logical points) reserved by the system at the top of the
/// primary display: the macOS menu bar, whose height already grows to clear
/// the camera notch on notched Macs. Callers position top-anchored floating
/// UI (the recording overlay) below this so it is never hidden behind the
/// notch or menu bar.
///
/// Returns `0.0` on platforms without a top system bar (Windows/Linux), and a
/// conservative fallback if the screen can't be queried.
#[must_use]
pub fn primary_top_inset() -> f64 {
    primary_top_inset_impl()
}

#[cfg(target_os = "macos")]
fn primary_top_inset_impl() -> f64 {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    // Standard menu bar height; used if the screen query is unavailable (e.g.
    // called off the main thread). Notched Macs report ~37 via the query.
    const FALLBACK: f64 = 24.0;

    let Some(mtm) = MainThreadMarker::new() else {
        return FALLBACK;
    };
    let Some(screen) = NSScreen::mainScreen(mtm) else {
        return FALLBACK;
    };
    // `visibleFrame` excludes the menu bar (top) and Dock (bottom). The top
    // inset is the gap between the full frame's top and the visible top — i.e.
    // the menu bar, isolated from the Dock regardless of where the Dock sits.
    let frame = screen.frame();
    let visible = screen.visibleFrame();
    let frame_top = frame.origin.y + frame.size.height;
    let visible_top = visible.origin.y + visible.size.height;
    let inset = frame_top - visible_top;
    if inset.is_finite() && inset >= 0.0 {
        inset
    } else {
        FALLBACK
    }
}

#[cfg(not(target_os = "macos"))]
fn primary_top_inset_impl() -> f64 {
    // Windows/Linux have no always-on top system bar that overlaps the
    // primary display the way the macOS menu bar does (the taskbar is at the
    // bottom by default and is excluded from the work area separately), so
    // top-anchored UI needs no inset here.
    0.0
}
