//! `NSPasteboard` snapshot-set-⌘V-restore for text injection.

use std::time::Duration;

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSPasteboard, NSPasteboardItem, NSPasteboardTypeString, NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSData, NSString};

use crate::inject::InjectError;

const DEFAULT_PASTE_DELAY_MS: u64 = 120;

fn paste_delay_ms() -> u64 {
    std::env::var("SIDECAR_INJECT_PASTE_DELAY_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PASTE_DELAY_MS)
}

/// One item from a clipboard snapshot: a list of (UTType-string, raw-bytes)
/// pairs covering every type the item advertised with concrete data.
///
/// Types backed by promised/lazy data (i.e. `dataForType:` returns `nil`)
/// cannot be read eagerly and are silently dropped from the snapshot.
/// This is a known limitation: promised data is typically provided by the
/// source app via a data-provider callback that outlives our snapshot window.
type SnapshotItem = Vec<(String, Vec<u8>)>;

/// A complete snapshot of a pasteboard's items and their type/data pairs.
pub(crate) struct ClipboardSnapshot {
    items: Vec<SnapshotItem>,
}

/// Snapshot every item + every concrete type on `pb`.
/// Types with nil data (promised/lazy) are silently skipped.
pub(crate) fn snapshot(pb: &NSPasteboard) -> ClipboardSnapshot {
    let Some(pb_items) = pb.pasteboardItems() else {
        return ClipboardSnapshot { items: vec![] };
    };
    let items = pb_items
        .iter()
        .map(|item| {
            let types = item.types();
            types
                .iter()
                .filter_map(|t| {
                    item.dataForType(&t).map(|data| {
                        let type_str = t.to_string();
                        let bytes = data.to_vec();
                        (type_str, bytes)
                    })
                })
                .collect::<SnapshotItem>()
        })
        .collect();
    ClipboardSnapshot { items }
}

/// Restore a previously snapshotted clipboard onto `pb`.
/// Calls `clearContents`, then rebuilds each item with `setData:forType:`.
pub(crate) fn restore(pb: &NSPasteboard, snap: ClipboardSnapshot) {
    pb.clearContents();
    if snap.items.is_empty() {
        return;
    }

    let rebuilt: Vec<Retained<NSPasteboardItem>> = snap
        .items
        .into_iter()
        .map(|type_data_pairs| {
            let new_item = NSPasteboardItem::new();
            for (type_str, bytes) in type_data_pairs {
                let ns_type = NSString::from_str(&type_str);
                let ns_data = NSData::from_vec(bytes);
                new_item.setData_forType(&ns_data, &ns_type);
            }
            new_item
        })
        .collect();

    // writeObjects: expects &NSArray<ProtocolObject<dyn NSPasteboardWriting>>.
    // NSPasteboardItem conforms to NSPasteboardWriting, so we can convert via
    // ProtocolObject::from_retained.
    let protocol_items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = rebuilt
        .into_iter()
        .map(ProtocolObject::from_retained)
        .collect();
    let array = NSArray::from_retained_slice(&protocol_items);
    pb.writeObjects(&array);
}

/// Last-resort clipboard write: replaces the general pasteboard's string
/// content with `text`. Used as a fallback when text injection fails so the
/// user can paste manually. Returns `true` on success.
pub fn set_text(text: &str) -> bool {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    let ns = NSString::from_str(text);
    // SAFETY: NSPasteboardTypeString is a valid extern static provided by AppKit.
    unsafe { pb.setString_forType(&ns, NSPasteboardTypeString) }
}

pub(crate) fn pasteboard_inject(text: &str) -> Result<(), InjectError> {
    let pb = NSPasteboard::generalPasteboard();

    // 1. Snapshot every type on the current pasteboard, not just strings.
    //    Promised/lazy data types (where dataForType: returns nil) are silently
    //    skipped — they cannot be read eagerly because the source app's
    //    data-provider callback owns the data, not the pasteboard itself.
    let saved = snapshot(&pb);

    // 2. Clear and set our text.
    pb.clearContents();
    let our_ns = NSString::from_str(text);
    // SAFETY: NSPasteboardTypeString is a valid extern static provided by AppKit.
    let ok = unsafe { pb.setString_forType(&our_ns, NSPasteboardTypeString) };
    if !ok {
        return Err(InjectError::PasteboardWrite(
            "setString_forType returned false".into(),
        ));
    }

    // 3. Synthesize Cmd+V.
    synth_cmd_v()?;

    // 4. Wait for the target app to consume the paste.
    std::thread::sleep(Duration::from_millis(paste_delay_ms()));

    // 5. Restore the full pre-injection pasteboard state.
    restore(&pb, saved);

    Ok(())
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

#[cfg(test)]
mod tests {
    use objc2_app_kit::NSPasteboardTypePNG;

    use super::*;

    /// Round-trip test using a PRIVATE named pasteboard so we never clobber the
    /// developer's real general pasteboard during `cargo test`.
    ///
    /// Step 1 — write a string AND fake PNG bytes onto the private pasteboard.
    /// Step 2 — snapshot it.
    /// Step 3 — overwrite with unrelated text (simulate injection).
    /// Step 4 — restore from snapshot.
    /// Step 5 — assert both the string and the PNG data round-tripped intact.
    #[test]
    fn snapshot_restore_preserves_all_types() {
        // Use a unique private pasteboard; never touches the general pasteboard.
        let pb_name = NSString::from_str("ai.lirevo.test.snapshot_restore");
        let pb = NSPasteboard::pasteboardWithName(&pb_name);

        // Seed the pasteboard with two types on one item.
        pb.clearContents();
        let item = NSPasteboardItem::new();

        let text = "hello clip";
        let text_ns = NSString::from_str(text);
        // SAFETY: NSPasteboardTypeString / NSPasteboardTypePNG are valid AppKit
        // extern statics.
        unsafe {
            item.setString_forType(&text_ns, NSPasteboardTypeString);
        }

        // A few non-zero bytes under the PNG UTI — any non-empty data is
        // sufficient to test the round-trip; we're not writing a valid PNG.
        let fake_png: Vec<u8> = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        let png_data = NSData::from_vec(fake_png.clone());
        unsafe {
            item.setData_forType(&png_data, NSPasteboardTypePNG);
        }

        let protocol_item: Retained<ProtocolObject<dyn NSPasteboardWriting>> =
            ProtocolObject::from_retained(item);
        let arr = NSArray::from_retained_slice(&[protocol_item]);
        pb.writeObjects(&arr);

        // Take the snapshot.
        let snap = snapshot(&pb);

        // Overwrite with something completely different.
        pb.clearContents();
        let inject_ns = NSString::from_str("injected text");
        unsafe {
            pb.setString_forType(&inject_ns, NSPasteboardTypeString);
        }

        // Restore.
        restore(&pb, snap);

        // Assert the original string came back.
        let recovered_string = unsafe {
            pb.stringForType(NSPasteboardTypeString)
                .map(|s| s.to_string())
        };
        assert_eq!(
            recovered_string.as_deref(),
            Some(text),
            "string type not restored"
        );

        // Assert the PNG data came back.
        let recovered_png =
            unsafe { pb.dataForType(NSPasteboardTypePNG).map(|d| d.to_vec()) };
        assert_eq!(
            recovered_png.as_deref(),
            Some(fake_png.as_slice()),
            "PNG data not restored"
        );

        // The private named pasteboard is automatically cleaned up when
        // its Retained handle drops at end of scope; no explicit release needed.
    }
}
