//! macOS helpers for push-to-talk.
//!
//! `CGEventSourceKeyState` reads the HID key table directly, so holding Right
//! Option works whether WeldSpeak, another app, or nothing is focused. It does
//! not need Accessibility, which `NSEvent` global monitors do.

use super::PttKey;

/// Right Control / Right Option / F8 / F13 on the ANSI layout.
const KEY_RIGHT_CONTROL: u16 = 62;
const KEY_LEFT_CONTROL: u16 = 59;
const KEY_RIGHT_OPTION: u16 = 61;
const KEY_LEFT_OPTION: u16 = 58;
const KEY_F8: u16 = 100;
const KEY_F13: u16 = 105;

/// `kCGEventSourceStateHIDSystemState` — physical keys, not synthesized ones.
const HID_SYSTEM_STATE: i32 = 1;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
}

pub fn is_down(key: PttKey) -> bool {
    let code = match key {
        PttKey::ControlRight => KEY_RIGHT_CONTROL,
        PttKey::ControlLeft => KEY_LEFT_CONTROL,
        PttKey::AltRight => KEY_RIGHT_OPTION,
        PttKey::AltLeft => KEY_LEFT_OPTION,
        PttKey::F8 => KEY_F8,
        PttKey::F13 => KEY_F13,
    };
    unsafe { CGEventSourceKeyState(HID_SYSTEM_STATE, code) }
}
