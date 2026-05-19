//! Text injection: `AXUIElement` primary, `NSPasteboard` fallback.

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use thiserror::Error;

use crate::pasteboard;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionMethod {
    Accessibility,
    Pasteboard,
}

#[derive(Debug, Error)]
pub enum InjectError {
    #[error("no focused application")]
    NoFocusedApp,
    #[error("no focused UI element")]
    NoFocusedElement,
    #[error("focused element does not accept text input")]
    NotTextEditable,
    #[error("accessibility permission denied")]
    PermissionDenied,
    #[error("pasteboard write failed: {0}")]
    PasteboardWrite(String),
    #[error("synthetic Cmd+V failed: {0}")]
    SyntheticPaste(String),
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Default)]
pub struct Injector {
    force_pasteboard: bool,
}

impl Injector {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_force_pasteboard(force_pasteboard: bool) -> Self {
        Self { force_pasteboard }
    }

    pub fn inject(&self, text: &str) -> Result<InjectionMethod, InjectError> {
        if self.force_pasteboard {
            pasteboard::pasteboard_inject(text)?;
            return Ok(InjectionMethod::Pasteboard);
        }

        match try_ax_inject(text) {
            Ok(()) => Ok(InjectionMethod::Accessibility),
            // Permission errors must surface — the user has to grant
            // Accessibility before either path works.
            Err(InjectError::PermissionDenied) => Err(InjectError::PermissionDenied),
            // Anything else (NoFocusedApp / NoFocusedElement / NotTextInput
            // / Internal) means the AX path can't reach this target —
            // Electron apps like VS Code, Slack, Discord, Cursor are the
            // typical case: AXFocusedApplication returns NoValue because
            // their windows don't expose a standard AX hierarchy.
            // Cmd+V via the pasteboard works on all of them.
            Err(_) => {
                pasteboard::pasteboard_inject(text)?;
                Ok(InjectionMethod::Pasteboard)
            }
        }
    }
}

// ----- AX path -----

type AXUIElementRef = *mut std::ffi::c_void;
type AXError = i32;

const KAX_ERROR_SUCCESS: AXError = 0;
const KAX_ERROR_API_DISABLED: AXError = -25211;
const KAX_ERROR_NO_VALUE: AXError = -25212;
const KAX_ERROR_INVALID_UI_ELEMENT: AXError = -25202;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut *const std::ffi::c_void,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *const std::ffi::c_void,
    ) -> AXError;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const std::ffi::c_void);
}

/// RAII guard for a `+1` retained `CFType` pointer obtained from an AX `Copy*`
/// call. `AXUIElementCopyAttributeValue` follows Core Foundation's "Copy"
/// rule: the caller owns one retain count and must release it. We were
/// leaking one `AXUIElement` per `try_ax_inject` invocation for both
/// `focused_app` and `focused_elem`. The guard ensures cleanup runs on
/// every early-return path.
struct CFGuard(*const std::ffi::c_void);
impl Drop for CFGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

fn try_ax_inject(text: &str) -> Result<(), InjectError> {
    unsafe {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return Err(InjectError::Internal(
                "AXUIElementCreateSystemWide returned null".into(),
            ));
        }

        // Focused application
        let attr_focused_app = CFString::new("AXFocusedApplication");
        let mut focused_app_raw: *const std::ffi::c_void = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(
            system,
            attr_focused_app.as_concrete_TypeRef(),
            &raw mut focused_app_raw,
        );
        match err {
            KAX_ERROR_SUCCESS => {}
            KAX_ERROR_API_DISABLED => return Err(InjectError::PermissionDenied),
            KAX_ERROR_NO_VALUE => return Err(InjectError::NoFocusedApp),
            other => {
                return Err(InjectError::Internal(format!(
                    "AXCopy focused app: {other}"
                )))
            }
        }
        if focused_app_raw.is_null() {
            return Err(InjectError::NoFocusedApp);
        }
        let _focused_app_guard = CFGuard(focused_app_raw);
        let focused_app = focused_app_raw.cast_mut();

        // Focused UI element
        let attr_focused_elem = CFString::new("AXFocusedUIElement");
        let mut focused_elem_raw: *const std::ffi::c_void = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(
            focused_app,
            attr_focused_elem.as_concrete_TypeRef(),
            &raw mut focused_elem_raw,
        );
        match err {
            KAX_ERROR_SUCCESS => {}
            KAX_ERROR_API_DISABLED => return Err(InjectError::PermissionDenied),
            KAX_ERROR_NO_VALUE | KAX_ERROR_INVALID_UI_ELEMENT => {
                return Err(InjectError::NoFocusedElement)
            }
            other => {
                return Err(InjectError::Internal(format!(
                    "AXCopy focused elem: {other}"
                )))
            }
        }
        if focused_elem_raw.is_null() {
            return Err(InjectError::NoFocusedElement);
        }
        let _focused_elem_guard = CFGuard(focused_elem_raw);
        let focused_elem = focused_elem_raw.cast_mut();

        // Role check
        let attr_role = CFString::new("AXRole");
        let mut role_raw: *const std::ffi::c_void = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(
            focused_elem,
            attr_role.as_concrete_TypeRef(),
            &raw mut role_raw,
        );
        if err != KAX_ERROR_SUCCESS || role_raw.is_null() {
            return Err(InjectError::NotTextEditable);
        }
        let role_cf = CFString::wrap_under_create_rule(role_raw as CFStringRef);
        let role_str = role_cf.to_string();
        if !matches!(
            role_str.as_str(),
            "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField"
        ) {
            return Err(InjectError::NotTextEditable);
        }

        // Write via AXSelectedText (inserts at caret / replaces selection)
        let attr_sel = CFString::new("AXSelectedText");
        let text_cf = CFString::new(text);
        let err = AXUIElementSetAttributeValue(
            focused_elem,
            attr_sel.as_concrete_TypeRef(),
            text_cf.as_concrete_TypeRef().cast(),
        );
        match err {
            KAX_ERROR_SUCCESS => Ok(()),
            KAX_ERROR_API_DISABLED => Err(InjectError::PermissionDenied),
            other => Err(InjectError::Internal(format!(
                "AXSetSelectedText: {other}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injector_default_is_not_force_pasteboard() {
        let i = Injector::new();
        assert!(!i.force_pasteboard);
    }

    #[test]
    fn injector_with_force_pasteboard_sets_field() {
        let i = Injector::with_force_pasteboard(true);
        assert!(i.force_pasteboard);
    }

    #[test]
    fn injection_method_variants_eq() {
        assert_eq!(InjectionMethod::Accessibility, InjectionMethod::Accessibility);
        assert_ne!(InjectionMethod::Accessibility, InjectionMethod::Pasteboard);
    }

    #[test]
    #[ignore = "requires accessibility permission + a focused text field; manual dev box only"]
    fn injector_real_inject_to_focused_text_field() {
        // Manual smoke: open TextEdit, click in the document, then run:
        //   cargo test -p os-integration -- --ignored
        let inj = Injector::new();
        let result = inj.inject("[lda-test-injection] ");
        eprintln!("injection result: {result:?}");
        // No assertion on success — environment may not be ready;
        // the test exists to make manual smoke easy.
    }
}
