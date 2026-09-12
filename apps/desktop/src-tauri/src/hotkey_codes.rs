//! Map a KeyboardEvent `code` (what Settings captures) onto a platform key.
//!
//! The settings window records `event.code` — `ControlRight`, `KeyA`, `F8` —
//! so a binding is the same string on every OS. This module turns that string
//! into the value `GetAsyncKeyState` / `CGEventSourceKeyState` actually poll.

/// Native scan/virtual-key for `code`, if this OS can watch it.
pub fn native_code(code: &str) -> Option<u16> {
    #[cfg(target_os = "windows")]
    {
        windows_vk(code)
    }
    #[cfg(target_os = "macos")]
    {
        macos_hid(code)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = code;
        None
    }
}

/// Keys that still type into the focused app while held, because we poll
/// rather than swallow the event. Allowed, but Settings should warn.
pub fn types_while_held(code: &str) -> bool {
    let code = code.trim();
    code.starts_with("Key")
        || code.starts_with("Digit")
        || code.starts_with("Numpad")
        || matches!(
            code,
            "Space"
                | "Tab"
                | "Enter"
                | "Backspace"
                | "Delete"
                | "Comma"
                | "Period"
                | "Slash"
                | "Backslash"
                | "Minus"
                | "Equal"
                | "Semicolon"
                | "Quote"
                | "Backquote"
                | "BracketLeft"
                | "BracketRight"
                | "IntlBackslash"
        )
}

/// Short label for Settings and the window subtitle.
pub fn label(code: &str) -> String {
    match code.trim() {
        "ControlRight" => "Right Ctrl".into(),
        "ControlLeft" => "Left Ctrl".into(),
        "AltRight" => {
            if cfg!(target_os = "macos") {
                "Right Option".into()
            } else {
                "Right Alt".into()
            }
        }
        "AltLeft" => {
            if cfg!(target_os = "macos") {
                "Left Option".into()
            } else {
                "Left Alt".into()
            }
        }
        "ShiftRight" => "Right Shift".into(),
        "ShiftLeft" => "Left Shift".into(),
        "MetaRight" => {
            if cfg!(target_os = "macos") {
                "Right Command".into()
            } else {
                "Right Win".into()
            }
        }
        "MetaLeft" => {
            if cfg!(target_os = "macos") {
                "Left Command".into()
            } else {
                "Left Win".into()
            }
        }
        "Escape" => "Esc".into(),
        " " | "" => "None".into(),
        other if other.starts_with("Key") && other.len() == 4 => other[3..].to_string(),
        other if other.starts_with("Digit") => other[5..].to_string(),
        other => other.to_string(),
    }
}

#[cfg(target_os = "windows")]
fn windows_vk(code: &str) -> Option<u16> {
    Some(match code.trim() {
        "ControlLeft" | "CtrlLeft" => 0xA2,
        "ControlRight" | "CtrlRight" => 0xA3,
        "AltLeft" | "OptionLeft" => 0xA4,
        "AltRight" | "OptionRight" => 0xA5,
        "ShiftLeft" => 0xA0,
        "ShiftRight" => 0xA1,
        "MetaLeft" => 0x5B,
        "MetaRight" => 0x5C,
        "Space" => 0x20,
        "Tab" => 0x09,
        "Enter" => 0x0D,
        "Backspace" => 0x08,
        "Delete" => 0x2E,
        "Insert" => 0x2D,
        "Home" => 0x24,
        "End" => 0x23,
        "PageUp" => 0x21,
        "PageDown" => 0x22,
        "CapsLock" => 0x14,
        "ContextMenu" => 0x5D,
        "ArrowLeft" => 0x25,
        "ArrowUp" => 0x26,
        "ArrowRight" => 0x27,
        "ArrowDown" => 0x28,
        "PrintScreen" => 0x2C,
        "ScrollLock" => 0x91,
        "Pause" => 0x13,
        "NumLock" => 0x90,
        "Comma" => 0xBC,
        "Period" => 0xBE,
        "Slash" => 0xBF,
        "Backslash" => 0xDC,
        "Minus" => 0xBD,
        "Equal" => 0xBB,
        "Semicolon" => 0xBA,
        "Quote" => 0xDE,
        "Backquote" => 0xC0,
        "BracketLeft" => 0xDB,
        "BracketRight" => 0xDD,
        "IntlBackslash" => 0xE2,
        "Numpad0" => 0x60,
        "Numpad1" => 0x61,
        "Numpad2" => 0x62,
        "Numpad3" => 0x63,
        "Numpad4" => 0x64,
        "Numpad5" => 0x65,
        "Numpad6" => 0x66,
        "Numpad7" => 0x67,
        "Numpad8" => 0x68,
        "Numpad9" => 0x69,
        "NumpadAdd" => 0x6B,
        "NumpadSubtract" => 0x6D,
        "NumpadMultiply" => 0x6A,
        "NumpadDivide" => 0x6F,
        "NumpadDecimal" => 0x6E,
        "NumpadEnter" => 0x0D,
        "F1" => 0x70,
        "F2" => 0x71,
        "F3" => 0x72,
        "F4" => 0x73,
        "F5" => 0x74,
        "F6" => 0x75,
        "F7" => 0x76,
        "F8" => 0x77,
        "F9" => 0x78,
        "F10" => 0x79,
        "F11" => 0x7A,
        "F12" => 0x7B,
        "F13" => 0x7C,
        "F14" => 0x7D,
        "F15" => 0x7E,
        "F16" => 0x7F,
        "F17" => 0x80,
        "F18" => 0x81,
        "F19" => 0x82,
        "F20" => 0x83,
        "F21" => 0x84,
        "F22" => 0x85,
        "F23" => 0x86,
        "F24" => 0x87,
        "Escape" => 0x1B,
        other => letter_or_digit_vk(other)?,
    })
}

#[cfg(target_os = "windows")]
fn letter_or_digit_vk(code: &str) -> Option<u16> {
    if let Some(letter) = code.strip_prefix("Key") {
        if letter.len() == 1 {
            let ch = letter.as_bytes()[0];
            if ch.is_ascii_uppercase() {
                return Some(u16::from(ch));
            }
        }
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        if digit.len() == 1 {
            let ch = digit.as_bytes()[0];
            if ch.is_ascii_digit() {
                return Some(u16::from(ch));
            }
        }
    }
    None
}

/// ANSI HID keycodes used by `CGEventSourceKeyState`.
#[cfg(target_os = "macos")]
fn macos_hid(code: &str) -> Option<u16> {
    Some(match code.trim() {
        "KeyA" => 0,
        "KeyS" => 1,
        "KeyD" => 2,
        "KeyF" => 3,
        "KeyH" => 4,
        "KeyG" => 5,
        "KeyZ" => 6,
        "KeyX" => 7,
        "KeyC" => 8,
        "KeyV" => 9,
        "KeyB" => 11,
        "KeyQ" => 12,
        "KeyW" => 13,
        "KeyE" => 14,
        "KeyR" => 15,
        "KeyY" => 16,
        "KeyT" => 17,
        "Digit1" => 18,
        "Digit2" => 19,
        "Digit3" => 20,
        "Digit4" => 21,
        "Digit6" => 22,
        "Digit5" => 23,
        "Equal" => 24,
        "Digit9" => 25,
        "Digit7" => 26,
        "Minus" => 27,
        "Digit8" => 28,
        "Digit0" => 29,
        "BracketRight" => 30,
        "KeyO" => 31,
        "KeyU" => 32,
        "BracketLeft" => 33,
        "KeyI" => 34,
        "KeyP" => 35,
        "Enter" => 36,
        "KeyL" => 37,
        "KeyJ" => 38,
        "Quote" => 39,
        "KeyK" => 40,
        "Semicolon" => 41,
        "Backslash" => 42,
        "Comma" => 43,
        "Slash" => 44,
        "KeyN" => 45,
        "KeyM" => 46,
        "Period" => 47,
        "Tab" => 48,
        "Space" => 49,
        "Backquote" => 50,
        "Backspace" => 51,
        "Escape" => 53,
        "MetaRight" => 54,
        "MetaLeft" => 55,
        "ShiftLeft" => 56,
        "CapsLock" => 57,
        "AltLeft" | "OptionLeft" => 58,
        "ControlLeft" | "CtrlLeft" => 59,
        "ShiftRight" => 60,
        "AltRight" | "OptionRight" => 61,
        "ControlRight" | "CtrlRight" => 62,
        "F17" => 64,
        "NumpadDecimal" => 65,
        "NumpadMultiply" => 67,
        "NumpadAdd" => 69,
        "NumLock" => 71,
        "NumpadDivide" => 75,
        "NumpadEnter" => 76,
        "NumpadSubtract" => 78,
        "F18" => 79,
        "F19" => 80,
        "NumpadEqual" => 81,
        "Numpad0" => 82,
        "Numpad1" => 83,
        "Numpad2" => 84,
        "Numpad3" => 85,
        "Numpad4" => 86,
        "Numpad5" => 87,
        "Numpad6" => 88,
        "Numpad7" => 89,
        "F20" => 90,
        "Numpad8" => 91,
        "Numpad9" => 92,
        "F5" => 96,
        "F6" => 97,
        "F7" => 98,
        "F3" => 99,
        "F8" => 100,
        "F9" => 101,
        "F11" => 103,
        "F13" => 105,
        "F16" => 106,
        "F14" => 107,
        "F10" => 109,
        "F12" => 111,
        "F15" => 113,
        "Home" => 115,
        "PageUp" => 116,
        "Delete" => 117,
        "F4" => 118,
        "End" => 119,
        "F2" => 120,
        "PageDown" => 121,
        "F1" => 122,
        "ArrowLeft" => 123,
        "ArrowRight" => 124,
        "ArrowDown" => 125,
        "ArrowUp" => 126,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_the_keys_people_actually_bind() {
        assert_eq!(label("ControlRight"), "Right Ctrl");
        assert_eq!(label("KeyA"), "A");
        assert_eq!(label("F8"), "F8");
        assert_eq!(label("Space"), "Space");
    }

    #[test]
    fn letters_type_while_held() {
        assert!(types_while_held("KeyA"));
        assert!(types_while_held("Space"));
        assert!(!types_while_held("ControlRight"));
        assert!(!types_while_held("F8"));
    }
}
