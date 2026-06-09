//! Linux permission helpers.
//!
//! Linux has no per-app TCC-style consent gate. The one capability that is
//! genuinely gated is reading `/dev/input/event*` for the global push-to-talk
//! hotkey: those nodes are owned by the `input` group, so a normal user can
//! only read them after being added to that group (`sudo usermod -aG input
//! $USER`, then re-login). We model the "accessibility" status as the
//! readability of an event device, which is exactly the prerequisite the
//! hotkey reader needs.
//!
//! Microphone access on Linux is mediated by ALSA / `PulseAudio` / `PipeWire`,
//! not
//! by a queryable per-app permission, so we report `Granted` and let the actual
//! capture path surface any failure (matching the Windows backend).
//!
//! UNVALIDATED: written and compile-checked on macOS via CI; the `/dev/input`
//! probe has not been exercised on real Linux.

use std::io::ErrorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionStatus {
    Granted,
    Denied,
    NotDetermined,
}

/// Debug-build escape hatch matching the macOS/Windows implementation: when
/// `LIREVO_DEV_SKIP_PERMS` is set to a non-empty, non-"0" value all permission
/// checks/prompts short-circuit to `Granted`. Compiled out of release builds.
#[must_use]
pub fn dev_skip_perms() -> bool {
    #[cfg(debug_assertions)]
    {
        std::env::var("LIREVO_DEV_SKIP_PERMS")
            .ok()
            .is_some_and(|v| !v.is_empty() && v != "0")
    }
    #[cfg(not(debug_assertions))]
    {
        false
    }
}

/// "Accessibility" on Linux means: can we read `/dev/input` for the global
/// hotkey? Probe whether at least one event device opens without a permission
/// error.
///   - `Granted`: an event device opened successfully.
///   - `Denied`: event devices exist but every open was refused (EACCES) —
///     the user is not in the `input` group.
///   - `NotDetermined`: no event nodes were found at all (headless / unusual
///     setup), so the answer is genuinely unknown rather than a refusal.
#[must_use]
pub fn check_accessibility() -> PermissionStatus {
    if dev_skip_perms() {
        tracing::warn!("check_accessibility: LIREVO_DEV_SKIP_PERMS active — returning Granted");
        return PermissionStatus::Granted;
    }
    probe_input_access()
}

fn probe_input_access() -> PermissionStatus {
    use evdev::Device;

    let mut saw_node = false;
    let mut saw_permission_denied = false;

    for idx in 0..32 {
        let node = format!("/dev/input/event{idx}");
        match Device::open(&node) {
            Ok(_) => return PermissionStatus::Granted,
            Err(e) => match e.kind() {
                ErrorKind::PermissionDenied => {
                    saw_node = true;
                    saw_permission_denied = true;
                }
                ErrorKind::NotFound => {}
                // Busy / other transient errors still prove the node exists.
                _ => saw_node = true,
            },
        }
    }

    if saw_permission_denied {
        PermissionStatus::Denied
    } else {
        // Either no nodes exist (saw_node == false: headless / unusual) or
        // nodes exist but none opened and none reported EACCES. Neither is a
        // hard denial, so defer to the wizard's setup copy.
        let _ = saw_node;
        PermissionStatus::NotDetermined
    }
}

/// There is no OS prompt to request `/dev/input` access — it is a one-time
/// group-membership change the user performs in a terminal. We return the
/// current probe result so the UI can decide whether to show the
/// `input`-group setup instructions; it never silently grants.
#[must_use]
pub fn prompt_accessibility() -> PermissionStatus {
    if dev_skip_perms() {
        tracing::warn!("prompt_accessibility: LIREVO_DEV_SKIP_PERMS active — returning Granted");
        return PermissionStatus::Granted;
    }
    // No OS prompt to trigger — just re-probe. `Denied`/`NotDetermined` signal
    // the wizard to show the `input`-group setup instructions; it never
    // silently grants.
    check_accessibility()
}

/// Microphone is not a queryable per-app permission on Linux; report `Granted`
/// and let the capture path surface any real failure.
#[must_use]
pub fn check_microphone() -> PermissionStatus {
    if dev_skip_perms() {
        tracing::warn!("check_microphone: LIREVO_DEV_SKIP_PERMS active — returning Granted");
        return PermissionStatus::Granted;
    }
    PermissionStatus::Granted
}

#[must_use]
pub fn prompt_microphone() -> PermissionStatus {
    if dev_skip_perms() {
        tracing::warn!("prompt_microphone: LIREVO_DEV_SKIP_PERMS active — returning Granted");
        return PermissionStatus::Granted;
    }
    PermissionStatus::Granted
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
    fn microphone_reports_granted() {
        assert_eq!(check_microphone(), PermissionStatus::Granted);
    }
}
