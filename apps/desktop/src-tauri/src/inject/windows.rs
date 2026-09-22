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

/// Executable name of the foreground window's process, without `.exe`.
///
/// `PROCESS_QUERY_LIMITED_INFORMATION` is enough for the image path and is
/// granted even for elevated processes, so this works for most windows.
pub fn focused_app_name() -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(foreground, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let queried = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        queried.ok()?;

        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        std::path::Path::new(&path)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
    }
}

/// Text in the focused control, if this is a native field we can read.
pub fn focused_text() -> Option<String> {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, SendMessageW,
        GUITHREADINFO, WM_GETTEXT, WM_GETTEXTLENGTH,
    };

    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return None;
        }
        let thread = GetWindowThreadProcessId(foreground, None);
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(thread, &mut info).is_err() {
            return None;
        }
        let hwnd = if info.hwndFocus.is_invalid() {
            foreground
        } else {
            info.hwndFocus
        };
        let length = SendMessageW(hwnd, WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0)).0;
        if length <= 0 || length > 8_000 {
            return None;
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let copied = SendMessageW(
            hwnd,
            WM_GETTEXT,
            WPARAM(buffer.len()),
            LPARAM(buffer.as_mut_ptr() as isize),
        )
        .0;
        if copied <= 0 {
            return None;
        }
        buffer.truncate(copied as usize);
        let text = String::from_utf16(&buffer).ok()?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}
