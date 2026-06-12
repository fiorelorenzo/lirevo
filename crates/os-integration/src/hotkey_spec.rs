//! Platform-neutral hotkey model + chord matcher.
//!
//! `HotkeySpec` describes a bindable trigger; `LiveState` is what the OS layer
//! observes right now; `EdgeDetector` turns "is the spec currently satisfied?"
//! transitions into `HotkeyEvent::Down` / `Up`. None of this touches OS APIs,
//! so it compiles and unit-tests on every target.

use serde::{Deserialize, Serialize};

/// Down/Up edge of the bound hotkey. The coordinator interprets these per
/// `ActivationMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Modifier {
    Control,
    Option,
    Command,
    Shift,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    Left,
    Right,
}

/// Side-agnostic modifier mask. Non-empty only for `Trigger::Key` combos.
// Modifier keys are genuinely independent flags; a named-bool mask mirrors the
// frontend wire shape better than packing them into an enum.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ModifierFlags {
    pub control: bool,
    pub option: bool,
    pub command: bool,
    pub shift: bool,
    #[serde(rename = "fn")]
    pub fnkey: bool,
}

impl ModifierFlags {
    #[must_use]
    pub fn count(self) -> usize {
        usize::from(self.control)
            + usize::from(self.option)
            + usize::from(self.command)
            + usize::from(self.shift)
            + usize::from(self.fnkey)
    }
}

/// The activating input of a hotkey.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Trigger {
    /// A base key by canonical name (`"K"`, `"F5"`, `"Space"`, `"ArrowUp"`).
    Key(String),
    /// Fn / Globe pressed alone.
    Fn,
    /// A single side-specific modifier held alone (classic push-to-talk).
    ModifierOnly { modifier: Modifier, side: Side },
    /// Mouse side button: 4 or 5.
    Mouse(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeySpec {
    #[serde(default)]
    pub modifiers: ModifierFlags,
    pub trigger: Trigger,
}

/// How a satisfied chord drives recording.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivationMode {
    /// Push-to-talk: record while the chord is held (current behavior).
    #[default]
    Hold,
    /// Toggle: one full press starts, the next stops.
    Tap,
}

/// What the OS layer currently observes. The macOS tap maintains this from
/// `CGEvent`s; the matcher reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveState {
    /// Side-agnostic modifier mask currently held (includes the Fn flag).
    pub mods: ModifierFlags,
    /// Canonical name of the base key currently held, if any.
    pub base_key: Option<String>,
    /// A single side-specific modifier currently held alone.
    pub mod_only: Option<(Modifier, Side)>,
    /// Mouse side button currently held (4 or 5).
    pub mouse: Option<u8>,
}

impl HotkeySpec {
    /// Is this spec fully satisfied by the current live state?
    #[must_use]
    pub fn satisfied(&self, st: &LiveState) -> bool {
        match &self.trigger {
            Trigger::Key(name) => {
                st.base_key.as_deref() == Some(name.as_str()) && st.mods == self.modifiers
            }
            Trigger::ModifierOnly { modifier, side } => st.mod_only == Some((*modifier, *side)),
            Trigger::Fn => st.mods == (ModifierFlags { fnkey: true, ..ModifierFlags::default() }),
            Trigger::Mouse(b) => st.mouse == Some(*b),
        }
    }
}

/// Turns `satisfied` transitions into edges, suppressing repeats.
pub struct EdgeDetector {
    spec: HotkeySpec,
    down: bool,
}

impl EdgeDetector {
    #[must_use]
    pub fn new(spec: HotkeySpec) -> Self {
        Self { spec, down: false }
    }

    /// Feed the latest live state; returns an edge on a transition.
    pub fn update(&mut self, st: &LiveState) -> Option<HotkeyEvent> {
        let now = self.spec.satisfied(st);
        if now == self.down {
            return None;
        }
        self.down = now;
        Some(if now { HotkeyEvent::Down } else { HotkeyEvent::Up })
    }
}

/// Snapshot streamed to the webview during capture mode.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureEvent {
    pub modifiers: ModifierFlags,
    /// Canonical base-key name currently held, if any.
    pub base_key: Option<String>,
    pub mod_only: Option<ModOnly>,
    pub mouse: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModOnly {
    pub modifier: Modifier,
    pub side: Side,
}

/// Canonical key name → macOS virtual keycode. Mirrors the names the frontend
/// derives from `KeyboardEvent.code` (see `app/src/lib/hotkey.ts`). Extend as
/// needed; unknown names map to `None` (rejected by validation).
#[must_use]
pub fn key_to_macos_keycode(name: &str) -> Option<i64> {
    #[rustfmt::skip]
    let code = match name {
        "A" => 0x00, "S" => 0x01, "D" => 0x02, "F" => 0x03, "H" => 0x04, "G" => 0x05,
        "Z" => 0x06, "X" => 0x07, "C" => 0x08, "V" => 0x09, "B" => 0x0B, "Q" => 0x0C,
        "W" => 0x0D, "E" => 0x0E, "R" => 0x0F, "Y" => 0x10, "T" => 0x11, "O" => 0x1F,
        "U" => 0x20, "I" => 0x22, "P" => 0x23, "L" => 0x25, "J" => 0x26, "K" => 0x28,
        "N" => 0x2D, "M" => 0x2E,
        "1" => 0x12, "2" => 0x13, "3" => 0x14, "4" => 0x15, "5" => 0x17, "6" => 0x16,
        "7" => 0x1A, "8" => 0x1C, "9" => 0x19, "0" => 0x1D,
        "Space" => 0x31, "Return" => 0x24, "Tab" => 0x30, "Esc" => 0x35, "Delete" => 0x33,
        "ArrowLeft" => 0x7B, "ArrowRight" => 0x7C, "ArrowDown" => 0x7D, "ArrowUp" => 0x7E,
        "Home" => 0x73, "End" => 0x77, "PageUp" => 0x74, "PageDown" => 0x79,
        "F1" => 0x7A, "F2" => 0x78, "F3" => 0x63, "F4" => 0x76, "F5" => 0x60, "F6" => 0x61,
        "F7" => 0x62, "F8" => 0x64, "F9" => 0x65, "F10" => 0x6D, "F11" => 0x67, "F12" => 0x6F,
        "F13" => 0x69, "F14" => 0x6B, "F15" => 0x71, "F16" => 0x6A, "F17" => 0x40,
        "F18" => 0x4F, "F19" => 0x50, "F20" => 0x5A,
        _ => return None,
    };
    Some(code)
}

#[cfg(test)]
// Tests mutate a `LiveState::default()` incrementally across assertions to model
// hold/release transitions; a single struct initializer can't express that. The
// `mods(..)` helper takes one bool per modifier key for terse fixtures.
#[allow(
    clippy::field_reassign_with_default,
    clippy::fn_params_excessive_bools,
    clippy::many_single_char_names
)]
mod tests {
    use super::*;

    fn mods(c: bool, o: bool, m: bool, s: bool, f: bool) -> ModifierFlags {
        ModifierFlags { control: c, option: o, command: m, shift: s, fnkey: f }
    }

    #[test]
    fn combo_satisfied_only_with_exact_mods_and_base() {
        let spec = HotkeySpec {
            modifiers: mods(true, false, false, true, false), // Ctrl+Shift
            trigger: Trigger::Key("K".into()),
        };
        let mut st = LiveState::default();
        st.mods = mods(true, false, false, false, false);
        st.base_key = Some("K".into());
        assert!(!spec.satisfied(&st));
        st.mods = mods(true, false, false, true, false);
        assert!(spec.satisfied(&st));
        st.mods = mods(true, false, true, true, false);
        assert!(!spec.satisfied(&st));
    }

    #[test]
    fn modifier_only_satisfied_by_side_specific_press() {
        let spec = HotkeySpec {
            modifiers: ModifierFlags::default(),
            trigger: Trigger::ModifierOnly { modifier: Modifier::Option, side: Side::Right },
        };
        let mut st = LiveState::default();
        st.mod_only = Some((Modifier::Option, Side::Left));
        assert!(!spec.satisfied(&st));
        st.mod_only = Some((Modifier::Option, Side::Right));
        assert!(spec.satisfied(&st));
    }

    #[test]
    fn fn_satisfied_when_only_fn_flag_held() {
        let spec = HotkeySpec { modifiers: ModifierFlags::default(), trigger: Trigger::Fn };
        let mut st = LiveState::default();
        st.mods = mods(false, false, false, false, true);
        assert!(spec.satisfied(&st));
        st.mods = mods(true, false, false, false, true);
        assert!(!spec.satisfied(&st));
    }

    #[test]
    fn mouse_satisfied_by_button() {
        let spec = HotkeySpec { modifiers: ModifierFlags::default(), trigger: Trigger::Mouse(4) };
        let mut st = LiveState::default();
        st.mouse = Some(5);
        assert!(!spec.satisfied(&st));
        st.mouse = Some(4);
        assert!(spec.satisfied(&st));
    }

    #[test]
    fn edge_detector_emits_down_then_up_once() {
        let spec = HotkeySpec { modifiers: ModifierFlags::default(), trigger: Trigger::Mouse(4) };
        let mut det = EdgeDetector::new(spec);
        let mut st = LiveState::default();
        assert_eq!(det.update(&st), None);
        st.mouse = Some(4);
        assert_eq!(det.update(&st), Some(HotkeyEvent::Down));
        assert_eq!(det.update(&st), None);
        st.mouse = None;
        assert_eq!(det.update(&st), Some(HotkeyEvent::Up));
        assert_eq!(det.update(&st), None);
    }

    #[test]
    fn spec_json_roundtrips_for_each_variant() {
        let cases = [
            HotkeySpec { modifiers: mods(true, false, false, true, false), trigger: Trigger::Key("K".into()) },
            HotkeySpec { modifiers: ModifierFlags::default(), trigger: Trigger::Fn },
            HotkeySpec { modifiers: ModifierFlags::default(), trigger: Trigger::ModifierOnly { modifier: Modifier::Command, side: Side::Right } },
            HotkeySpec { modifiers: ModifierFlags::default(), trigger: Trigger::Mouse(5) },
        ];
        for c in cases {
            let j = serde_json::to_value(&c).unwrap();
            let back: HotkeySpec = serde_json::from_value(j).unwrap();
            assert_eq!(c, back);
        }
    }

    #[test]
    fn key_name_maps_to_distinct_macos_keycodes() {
        assert_eq!(key_to_macos_keycode("F5"), Some(0x60));
        assert_eq!(key_to_macos_keycode("Space"), Some(0x31));
        assert_eq!(key_to_macos_keycode("K"), Some(0x28));
        assert_eq!(key_to_macos_keycode("does-not-exist"), None);
    }
}
