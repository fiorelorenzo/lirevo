//! macOS Accessibility + microphone permission helpers.
//!
//! Uses raw FFI against the `ApplicationServices` framework (linked by build.rs)
//! to avoid binding-crate API churn. The exposed Rust API stays stable across
//! upstream changes; only the extern declarations would need updates if Apple
//! ever renamed the C symbols (extremely unlikely for these stable APIs).

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionStatus {
    Granted,
    Denied,
    NotDetermined,
}

/// Debug-build escape hatch: when `LDA_DEV_SKIP_PERMS` is set to a non-empty,
/// non-"0" value, all permission checks/prompts return `Granted` without
/// touching macOS TCC. Lets us iterate on wizard UI from `tauri dev` (where
/// the bare-binary TCC prompt cannot appear) without needing a `.app` bundle.
/// Compiled out of release builds entirely. Also consumed by `test_mic` so
/// the wizard sees a faked successful capture under the same flag.
#[must_use]
pub fn dev_skip_perms() -> bool {
    #[cfg(debug_assertions)]
    {
        std::env::var("LDA_DEV_SKIP_PERMS")
            .ok()
            .is_some_and(|v| !v.is_empty() && v != "0")
    }
    #[cfg(not(debug_assertions))]
    {
        false
    }
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: core_foundation::base::CFTypeRef) -> bool;

    /// Defined in `ApplicationServices` as a `CFStringRef` constant; the symbol is
    /// `kAXTrustedCheckOptionPrompt`. We declare it as a `CFStringRef` because
    /// the Rust linker resolves it to the `CFString` global.
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// Returns Granted if this process is trusted for Accessibility
/// (i.e., listed and toggled on in System Settings → Privacy & Security → Accessibility).
/// Used by `CGEventTap` installation + AX text injection.
#[must_use]
pub fn check_accessibility() -> PermissionStatus {
    if dev_skip_perms() {
        tracing::warn!("check_accessibility: LDA_DEV_SKIP_PERMS active — returning Granted");
        return PermissionStatus::Granted;
    }
    if unsafe { AXIsProcessTrusted() } {
        PermissionStatus::Granted
    } else {
        PermissionStatus::Denied
    }
}

/// Shows the macOS Accessibility prompt sheet, asking the user to grant access.
/// Note: the user must manually toggle the switch and (likely) restart the binary.
/// Returns the current status — usually still `Denied`/`NotDetermined` immediately
/// after the prompt because the user hasn't responded yet.
#[must_use]
pub fn prompt_accessibility() -> PermissionStatus {
    if dev_skip_perms() {
        tracing::warn!("prompt_accessibility: LDA_DEV_SKIP_PERMS active — returning Granted");
        return PermissionStatus::Granted;
    }
    unsafe {
        // Build dict { kAXTrustedCheckOptionPrompt: true }
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let val = CFBoolean::true_value();
        let dict = CFDictionary::from_CFType_pairs(&[(key, val)]);
        let trusted = AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef().cast());
        if trusted {
            PermissionStatus::Granted
        } else {
            PermissionStatus::NotDetermined
        }
    }
}

/// Microphone permission via `AVCaptureDevice.authorizationStatusForMediaType:`.
/// Returns the current TCC state without prompting the user.
#[must_use]
pub fn check_microphone() -> PermissionStatus {
    if dev_skip_perms() {
        tracing::warn!("check_microphone: LDA_DEV_SKIP_PERMS active — returning Granted");
        return PermissionStatus::Granted;
    }
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

    // Safety: `AVMediaTypeAudio` is a process-lifetime NSString constant.
    let media_type = unsafe { AVMediaTypeAudio.unwrap() };
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type) };
    match status {
        AVAuthorizationStatus::Authorized => PermissionStatus::Granted,
        AVAuthorizationStatus::Denied | AVAuthorizationStatus::Restricted => {
            PermissionStatus::Denied
        }
        AVAuthorizationStatus::NotDetermined => PermissionStatus::NotDetermined,
        _ => PermissionStatus::NotDetermined,
    }
}

/// Trigger the macOS microphone TCC prompt and block until the user responds
/// (or 60s elapses). cpal opens the audio device through Core Audio HAL
/// which does NOT trigger the TCC prompt automatically on unsigned dev
/// builds — the stream succeeds but produces silent zeros until permission
/// is explicitly requested.
///
/// CAVEAT: for the macOS TCC dialog to appear, the running binary must be
/// able to find `NSMicrophoneUsageDescription`. For .app bundles this comes
/// from `Info.plist`. For bare binaries (`cargo run`), the Info.plist must
/// be embedded into the Mach-O `__TEXT,__info_plist` section (see
/// `app/src-tauri/embedded-info.plist` + `build.rs`).
#[must_use]
pub fn prompt_microphone() -> PermissionStatus {
    if dev_skip_perms() {
        tracing::warn!("prompt_microphone: LDA_DEV_SKIP_PERMS active — returning Granted");
        return PermissionStatus::Granted;
    }
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};
    use std::sync::{Arc, Condvar, Mutex};

    let current = check_microphone();
    tracing::info!(?current, "prompt_microphone: pre-request status");
    if current != PermissionStatus::NotDetermined {
        return current;
    }

    let media_type = unsafe { AVMediaTypeAudio.unwrap() };

    let pair: Arc<(Mutex<Option<bool>>, Condvar)> = Arc::new((Mutex::new(None), Condvar::new()));
    let pair2 = pair.clone();

    let handler = RcBlock::new(move |granted: Bool| {
        let (lock, cvar) = &*pair2;
        let mut g = lock.lock().unwrap();
        let value = granted.as_bool();
        tracing::info!(granted = value, "prompt_microphone: handler fired");
        *g = Some(value);
        cvar.notify_one();
    });

    tracing::info!("prompt_microphone: calling AVCaptureDevice.requestAccessForMediaType:");
    unsafe {
        AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type, &handler);
    }

    let (lock, cvar) = &*pair;
    let g = lock.lock().unwrap();
    let (_g, wait_result) = cvar
        .wait_timeout_while(g, std::time::Duration::from_secs(60), |opt| opt.is_none())
        .unwrap();
    if wait_result.timed_out() {
        tracing::warn!("prompt_microphone: 60s timeout — handler never fired (TCC dialog likely never appeared)");
        return PermissionStatus::NotDetermined;
    }
    let final_status = check_microphone();
    tracing::info!(?final_status, "prompt_microphone: post-request status");
    final_status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_status_eq_and_debug() {
        assert_eq!(PermissionStatus::Granted, PermissionStatus::Granted);
        assert_ne!(PermissionStatus::Granted, PermissionStatus::Denied);
        let _ = format!("{:?}", PermissionStatus::Denied);
    }

    #[test]
    fn check_accessibility_returns_valid_status() {
        let s = check_accessibility();
        assert!(matches!(
            s,
            PermissionStatus::Granted | PermissionStatus::Denied | PermissionStatus::NotDetermined
        ));
    }

    #[test]
    fn check_microphone_returns_valid_status() {
        let s = check_microphone();
        assert!(matches!(
            s,
            PermissionStatus::Granted | PermissionStatus::Denied | PermissionStatus::NotDetermined
        ));
    }
}
