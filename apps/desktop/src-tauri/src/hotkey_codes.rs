//! Map a KeyboardEvent `code` (what Settings captures) onto a platform key.
//!
//! The settings window records `event.code` — `ControlRight`, `KeyA`, `F8` —
//! so a binding is the same string on every OS. This module turns that string
//! into the value `GetAsyncKeyState` / `CGEventSourceKeyState` actually poll.

/// KeyboardEvent `code` parts in a binding (`ControlRight` or `ControlRight+MetaLeft`).
pub fn parts(accelerator: &str) -> Vec<&str> {
    accelerator
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

/// Native codes to poll. The second is `0` when the binding is a single key.
pub fn parse_codes(accelerator: &str) -> Option<(u16, u16)> {
    let parts = parts(accelerator);
    if parts.is_empty() || parts.len() > 2 {
        return None;
    }
    let first = native_code(parts[0])?;
    if first == 0 {
        return None;
    }
    if parts.len() == 1 {
        return Some((first, 0));
    }
    let second = native_code(parts[1])?;
    if second == 0 || second == first {
        return None;
    }
    Some((first, second))
}

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
pub fn types_while_held(accelerator: &str) -> bool {
    parts(accelerator).into_iter().any(part_types)
}

fn part_types(code: &str) -> bool {
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
pub fn label(accelerator: &str) -> String {
    let labeled: Vec<String> = parts(accelerator).into_iter().map(label_one).collect();
    if labeled.is_empty() {
        "None".into()
    } else {
        labeled.join(" + ")
    }
}

fn label_one(code: &str) -> String {
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

/// KeyboardEvent `code` for a Windows virtual-key, used when capturing a
/// binding in the native settings window.
#[cfg(target_os = "windows")]
pub fn code_from_windows_vk(vk: u16, extended: bool, scan: u16) -> Option<String> {
    Some(
        match vk {
            0x11 => {
                if extended {
                    "ControlRight"
                } else {
                    "ControlLeft"
                }
            }
            0x12 => {
                if extended {
                    "AltRight"
                } else {
                    "AltLeft"
                }
            }
            0x10 => {
                if scan == 0x36 {
                    "ShiftRight"
                } else {
                    "ShiftLeft"
                }
            }
            0x5B => "MetaLeft",
            0x5C => "MetaRight",
            0x20 => "Space",
            0x09 => "Tab",
            0x0D => "Enter",
            0x08 => "Backspace",
            0x2E => "Delete",
            0x1B => "Escape",
            0x25 => "ArrowLeft",
            0x26 => "ArrowUp",
            0x27 => "ArrowRight",
            0x28 => "ArrowDown",
            0x70..=0x87 => {
                return Some(format!("F{}", vk - 0x6F));
            }
            other if (0x41..=0x5A).contains(&other) => {
                return Some(format!("Key{}", char::from(other as u8)));
            }
            other if (0x30..=0x39).contains(&other) => {
                return Some(format!("Digit{}", char::from(other as u8)));
            }
            0xA2 => "ControlLeft",
            0xA3 => "ControlRight",
            0xA4 => "AltLeft",
            0xA5 => "AltRight",
            0xA0 => "ShiftLeft",
            0xA1 => "ShiftRight",
            _ => return None,
        }
        .into(),
    )
}

/// KeyboardEvent `code` for a macOS HID keycode.
#[cfg(target_os = "macos")]
pub fn code_from_macos_hid(hid: u16) -> Option<String> {
    Some(
        match hid {
            0 => "KeyA",
            1 => "KeyS",
            2 => "KeyD",
            3 => "KeyF",
            4 => "KeyH",
            5 => "KeyG",
            6 => "KeyZ",
            7 => "KeyX",
            8 => "KeyC",
            9 => "KeyV",
            11 => "KeyB",
            12 => "KeyQ",
            13 => "KeyW",
            14 => "KeyE",
            15 => "KeyR",
            16 => "KeyY",
            17 => "KeyT",
            18 => "Digit1",
            19 => "Digit2",
            20 => "Digit3",
            21 => "Digit4",
            22 => "Digit6",
            23 => "Digit5",
            24 => "Equal",
            25 => "Digit9",
            26 => "Digit7",
            27 => "Minus",
            28 => "Digit8",
            29 => "Digit0",
            31 => "KeyO",
            32 => "KeyU",
            34 => "KeyI",
            35 => "KeyP",
            36 => "Enter",
            37 => "KeyL",
            38 => "KeyJ",
            40 => "KeyK",
            45 => "KeyN",
            46 => "KeyM",
            48 => "Tab",
            49 => "Space",
            51 => "Backspace",
            53 => "Escape",
            54 => "MetaRight",
            55 => "MetaLeft",
            56 => "ShiftLeft",
            58 => "AltLeft",
            59 => "ControlLeft",
            60 => "ShiftRight",
            61 => "AltRight",
            62 => "ControlRight",
            122 => "F1",
            120 => "F2",
            99 => "F3",
            118 => "F4",
            96 => "F5",
            97 => "F6",
            98 => "F7",
            100 => "F8",
            101 => "F9",
            109 => "F10",
            103 => "F11",
            111 => "F12",
            123 => "ArrowLeft",
            124 => "ArrowRight",
            125 => "ArrowDown",
            126 => "ArrowUp",
            _ => return None,
        }
        .into(),
    )
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
        assert!(label("ControlRight+MetaLeft").contains("Right Ctrl"));
        assert!(label("ControlRight+MetaLeft").contains(" + "));
    }

    #[test]
    fn letters_type_while_held() {
        assert!(types_while_held("KeyA"));
        assert!(types_while_held("Space"));
        assert!(types_while_held("ControlRight+KeyA"));
        assert!(!types_while_held("ControlRight"));
        assert!(!types_while_held("F8"));
        assert!(!types_while_held("ControlRight+MetaLeft"));
    }

    #[test]
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn parses_one_or_two_physical_keys() {
        assert!(parse_codes("ControlRight").is_some());
        assert!(parse_codes("ControlRight+MetaLeft").is_some());
        assert!(parse_codes("CommandOrControl+Space").is_none());
        assert!(parse_codes("ControlRight+ControlRight").is_none());
        assert!(parse_codes("ControlRight+MetaLeft+ShiftLeft").is_none());
    }
}
