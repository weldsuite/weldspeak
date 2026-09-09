//! Text injection on Windows.
//!
//! Simpler than macOS: `SendInput` with `KEYEVENTF_UNICODE` delivers arbitrary
//! Unicode to essentially every application, needs no special permission, and
//! never touches the clipboard. There is no Accessibility equivalent to ask for.
//!
//! The one subtlety is UTF-16. `KEYBDINPUT.wScan` holds a single 16-bit code
//! unit, so a character outside the Basic Multilingual Plane — an emoji, most
//! commonly — must be sent as its two surrogate halves in order. Sending only
//! the first produces a replacement character.

use anyhow::{anyhow, Result};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_V,
};

/// Windows imposes no permission gate on synthesising input.
pub fn can_synthesise_input() -> bool {
    true
}

/// Nothing to open: there is no permission to grant.
pub fn open_permission_settings() -> Result<()> {
    Ok(())
}

/// One `INPUT` carrying a UTF-16 code unit as a Unicode keystroke.
fn unicode_input(code_unit: u16, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                // Zero: the character travels in wScan, not as a virtual key,
                // so the active keyboard layout is bypassed entirely.
                wVk: VIRTUAL_KEY(0),
                wScan: code_unit,
                dwFlags: if key_up {
                    KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                } else {
                    KEYEVENTF_UNICODE
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// One `INPUT` for a virtual key, used for shortcuts rather than characters.
fn virtual_key_input(key: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<()> {
    if inputs.is_empty() {
        return Ok(());
    }

    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        // The usual cause is a foreground window at higher integrity than us —
        // an elevated console, say — which silently swallows synthetic input.
        return Err(anyhow!(
            "SendInput delivered {sent} of {} events; the focused window may be running elevated",
            inputs.len()
        ));
    }
    Ok(())
}

/// Type `text` as Unicode keystrokes.
pub fn type_text(text: &str) -> Result<()> {
    // encode_utf16 splits astral characters into surrogate pairs for us, and
    // sending them in order is exactly what Windows expects.
    let mut inputs = Vec::new();
    for code_unit in text.encode_utf16() {
        inputs.push(unicode_input(code_unit, false));
        inputs.push(unicode_input(code_unit, true));
    }

    // One SendInput call keeps the batch atomic with respect to other input, so
    // a keystroke from the user cannot land in the middle of the dictation.
    send(&inputs)
}

/// Send Ctrl+V.
pub fn send_paste_shortcut() -> Result<()> {
    send(&[
        virtual_key_input(VK_CONTROL, false),
        virtual_key_input(VK_V, false),
        virtual_key_input(VK_V, true),
        virtual_key_input(VK_CONTROL, true),
    ])
}
