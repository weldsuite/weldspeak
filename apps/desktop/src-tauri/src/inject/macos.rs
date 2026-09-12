//! Text injection on macOS.
//!
//! Two constraints shape everything here.
//!
//! **Accessibility permission.** Without it no synthetic event reaches another
//! application — `CGEvent::post` succeeds and nothing happens, which is far
//! worse than an error. The permission is checked before every dictation
//! because it is genuinely dynamic: it can be revoked while the app runs, and
//! it resets whenever the app's code signature changes, so it silently
//! disappears after an update signed with a different identity.
//!
//! **The main thread.** The Text Input Source APIs that `CGEvent` relies on
//! (`TISCopyCurrentKeyboardInputSource` and friends) abort the process when
//! called off the main thread. This is the documented cause of a well-known
//! crash in Tauri apps that reach for `enigo` from a worker thread. Everything
//! in this module must therefore be dispatched to the main thread by the
//! caller — see `commands::inject_text`, which does so via Tauri's main-thread
//! runner.

use anyhow::{anyhow, Result};
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

/// Virtual key code for `v` on the ANSI layout.
///
/// Key codes are positional, not character-based: this is the physical key that
/// sits where `v` is on a US keyboard, and it remains the paste key on other
/// layouts because the shortcut is defined positionally too.
const KEY_V: u16 = 0x09;

/// Longest run of characters to submit in one synthetic event.
///
/// `CGEventKeyboardSetUnicodeString` takes an arbitrary string, but very long
/// ones are unreliable across applications; chunking keeps each event small
/// enough to be delivered intact.
const CHUNK_CHARS: usize = 20;

extern "C" {
    fn AXIsProcessTrustedWithOptions(options: core_foundation::dictionary::CFDictionaryRef) -> bool;
}

/// Whether Accessibility permission has been granted.
///
/// Deliberately passes `kAXTrustedCheckOptionPrompt: false`: the system prompt
/// is a single modal that users dismiss reflexively and which then never
/// reappears. WeldSpeak shows its own explanation and deep-links to the
/// settings pane instead.
pub fn can_synthesise_input() -> bool {
    unsafe {
        let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
        let options = CFDictionary::from_CFType_pairs(&[(
            key.as_CFType(),
            CFBoolean::false_value().as_CFType(),
        )]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
    }
}

/// Open System Settings at Privacy & Security → Accessibility.
pub fn open_permission_settings() -> Result<()> {
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn()?;
    Ok(())
}

fn event_source() -> Result<CGEventSource> {
    // HIDSystemState makes events behave as though they came from the keyboard,
    // which is what applications listening for real input expect.
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow!("could not create a CoreGraphics event source"))
}

/// Type `text` as Unicode keyboard events.
///
/// This inserts the characters directly rather than going through the
/// clipboard, so the user's copied content survives. It is the default for
/// ordinary dictation lengths.
///
/// # Panics on the wrong thread
///
/// Must be called on the main thread; see the module docs.
pub fn type_text(text: &str) -> Result<()> {
    let source = event_source()?;

    // Chunked by character, not byte: slicing a UTF-8 string mid-codepoint
    // would panic, and an accented character or emoji is several bytes.
    let characters: Vec<char> = text.chars().collect();

    for chunk in characters.chunks(CHUNK_CHARS) {
        let piece: String = chunk.iter().collect();

        // Key code 0 with a Unicode string attached: the event carries the
        // characters themselves rather than a key that must be interpreted
        // through the current layout.
        let event = CGEvent::new_keyboard_event(source.clone(), 0, true)
            .map_err(|_| anyhow!("could not create a keyboard event"))?;
        event.set_string(&piece);
        event.post(CGEventTapLocation::HID);

        let release = CGEvent::new_keyboard_event(source.clone(), 0, false)
            .map_err(|_| anyhow!("could not create a keyboard event"))?;
        release.set_string(&piece);
        release.post(CGEventTapLocation::HID);
    }

    Ok(())
}

/// Send ⌘V.
///
/// # Panics on the wrong thread
///
/// Must be called on the main thread; see the module docs.
pub fn send_paste_shortcut() -> Result<()> {
    let source = event_source()?;

    let down = CGEvent::new_keyboard_event(source.clone(), KEY_V, true)
        .map_err(|_| anyhow!("could not create a keyboard event"))?;
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    down.post(CGEventTapLocation::HID);

    let up = CGEvent::new_keyboard_event(source, KEY_V, false)
        .map_err(|_| anyhow!("could not create a keyboard event"))?;
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    up.post(CGEventTapLocation::HID);

    Ok(())
}

/// Text in the focused control, if Accessibility will tell us.
pub fn focused_text() -> Option<String> {
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};

    type AXUIElementRef = *const std::ffi::c_void;
    type AXError = i32;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut *const std::ffi::c_void,
        ) -> AXError;
        fn CFRelease(cf: *const std::ffi::c_void);
    }

    unsafe {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return None;
        }
        let focused_attr = CFString::from_static_string("AXFocusedUIElement");
        let mut focused: *const std::ffi::c_void = std::ptr::null();
        let status = AXUIElementCopyAttributeValue(
            system,
            focused_attr.as_concrete_TypeRef(),
            &mut focused,
        );
        CFRelease(system);
        if status != 0 || focused.is_null() {
            return None;
        }
        let value_attr = CFString::from_static_string("AXValue");
        let mut value: *const std::ffi::c_void = std::ptr::null();
        let status =
            AXUIElementCopyAttributeValue(focused, value_attr.as_concrete_TypeRef(), &mut value);
        CFRelease(focused);
        if status != 0 || value.is_null() {
            return None;
        }
        let cf = CFString::wrap_under_create_rule(value as CFStringRef);
        let text = cf.to_string();
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.len() > 8_000 {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}
