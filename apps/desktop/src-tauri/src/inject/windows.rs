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
use weldspeak_protocol::stream::{FieldContext, MAX_CONTEXT_AFTER, MAX_CONTEXT_BEFORE};
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

/// Longest field text read back; enough to hold the dictation and the edit
/// around it without copying a whole document on every poll.
const MAX_FIELD_CHARS: i32 = 20_000;

/// Text in the focused control.
///
/// UI Automation first: it reads browsers, Office, and Electron apps, where
/// nearly all dictation lands. `WM_GETTEXT` only sees classic Win32 edit
/// controls such as Notepad's, and stays as the fallback. Call this off the
/// UI thread — UI Automation calls into other processes and can block.
pub fn focused_text() -> Option<String> {
    focused_text_automation().or_else(focused_text_native)
}

/// Run `read` against the focused UI Automation element.
///
/// COM is initialised per call so a short-lived thread leaves it as it found
/// it; RPC_E_CHANGED_MODE means the thread is already STA, which works for a
/// client too but must not be uninitialised here. Password fields are never
/// handed to `read`.
fn with_focused_element<T>(
    read: impl FnOnce(&windows::Win32::UI::Accessibility::IUIAutomationElement) -> Option<T>,
) -> Option<T> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};

    unsafe {
        let initialised = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();

        let result = (|| {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let element = automation.GetFocusedElement().ok()?;
            // Never read a password field into memory, let alone send it.
            if element
                .CurrentIsPassword()
                .map(|b| b.as_bool())
                .unwrap_or(true)
            {
                return None;
            }
            read(&element)
        })();

        if initialised {
            CoUninitialize();
        }
        result
    }
}

fn focused_text_automation() -> Option<String> {
    use windows::Win32::UI::Accessibility::{
        IUIAutomationTextPattern, IUIAutomationValuePattern, UIA_TextPatternId, UIA_ValuePatternId,
    };

    let text = with_focused_element(|element| unsafe {
        if let Ok(pattern) =
            element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
        {
            if let Ok(value) = pattern.CurrentValue() {
                let value = value.to_string();
                if !value.trim().is_empty() {
                    return Some(value);
                }
            }
        }

        let pattern = element
            .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
            .ok()?;
        let text = pattern
            .DocumentRange()
            .ok()?
            .GetText(MAX_FIELD_CHARS)
            .ok()?;
        Some(text.to_string())
    })?;

    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// The text either side of the cursor, and the window title.
///
/// With a text pattern (browsers, Word, most Electron apps) the ranges are
/// taken relative to the caret, clipped to the characters nearest it so a long
/// document is never copied whole. A plain value field has no caret position,
/// so its text counts as "before" — the caret is almost always at the end
/// when someone starts dictating into one.
pub fn focused_context() -> Option<FieldContext> {
    let window_title = foreground_window_title();
    let (before, after) = with_focused_element(|element| unsafe {
        caret_context(element).or_else(|| {
            use windows::Win32::UI::Accessibility::{
                IUIAutomationValuePattern, UIA_ValuePatternId,
            };
            let pattern = element
                .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .ok()?;
            let value = pattern.CurrentValue().ok()?.to_string();
            Some((Some(value), None))
        })
    })
    .unwrap_or((None, None));

    FieldContext {
        before,
        after,
        window_title,
    }
    .trimmed()
}

unsafe fn caret_context(
    element: &windows::Win32::UI::Accessibility::IUIAutomationElement,
) -> Option<(Option<String>, Option<String>)> {
    use windows::Win32::UI::Accessibility::{
        IUIAutomationTextPattern, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
        TextUnit_Character, UIA_TextPatternId,
    };

    let pattern = element
        .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        .ok()?;
    let selections = pattern.GetSelection().ok()?;
    if selections.Length().ok()? < 1 {
        return None;
    }
    let caret = selections.GetElement(0).ok()?;

    // Collapse a copy onto the caret's start, then reach back.
    let before = caret.Clone().ok()?;
    before
        .MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            &caret,
            TextPatternRangeEndpoint_Start,
        )
        .ok()?;
    let _ = before.MoveEndpointByUnit(
        TextPatternRangeEndpoint_Start,
        TextUnit_Character,
        -(MAX_CONTEXT_BEFORE as i32),
    );

    // Collapse a copy onto the caret's end, then reach forward.
    let after = caret.Clone().ok()?;
    after
        .MoveEndpointByRange(
            TextPatternRangeEndpoint_Start,
            &caret,
            TextPatternRangeEndpoint_End,
        )
        .ok()?;
    let _ = after.MoveEndpointByUnit(
        TextPatternRangeEndpoint_End,
        TextUnit_Character,
        MAX_CONTEXT_AFTER as i32,
    );

    let read = |range: &windows::Win32::UI::Accessibility::IUIAutomationTextRange| {
        range
            .GetText(MAX_FIELD_CHARS)
            .ok()
            .map(|text| text.to_string())
    };
    Some((read(&before), read(&after)))
}

fn foreground_window_title() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

    unsafe {
        let window = GetForegroundWindow();
        if window.is_invalid() {
            return None;
        }
        let mut buffer = [0u16; 512];
        let length = GetWindowTextW(window, &mut buffer);
        if length <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buffer[..length as usize]))
    }
}

fn focused_text_native() -> Option<String> {
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
