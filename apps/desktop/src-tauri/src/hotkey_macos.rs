//! macOS helpers for push-to-talk.
//!
//! `CGEventSourceKeyState` reads the HID key table directly, so holding Right
//! Option works whether WeldSpeak, another app, or nothing is focused. It does
//! not need Accessibility, which `NSEvent` global monitors do.

/// `kCGEventSourceStateHIDSystemState` — physical keys, not synthesized ones.
const HID_SYSTEM_STATE: i32 = 1;
const KEY_ESCAPE: u16 = 53;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
}

pub fn is_down(hid: u16) -> bool {
    unsafe { CGEventSourceKeyState(HID_SYSTEM_STATE, hid) }
}

pub fn is_escape_down() -> bool {
    is_down(KEY_ESCAPE)
}
