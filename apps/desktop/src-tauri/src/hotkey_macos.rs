//! Push-to-talk on macOS via local and global `NSEvent` monitors.
//!
//! The global monitor sees keys while another app is focused (the usual
//! dictation case). The local monitor sees them while WeldSpeak itself is
//! focused. Modifier keys arrive as `flagsChanged`, function keys as up/down.

use std::ptr::NonNull;
use std::sync::OnceLock;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType};
use tauri::AppHandle;

use super::PttKey;

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Right Control / Right Option / F8 / F13 on the ANSI layout.
const KEY_RIGHT_CONTROL: u16 = 62;
const KEY_LEFT_CONTROL: u16 = 59;
const KEY_RIGHT_OPTION: u16 = 61;
const KEY_LEFT_OPTION: u16 = 58;
const KEY_F8: u16 = 100;
const KEY_F13: u16 = 105;

// NSEventType: KeyDown = 10, KeyUp = 11, FlagsChanged = 12.
const TYPE_KEY_DOWN: NSEventType = NSEventType(10);
const TYPE_KEY_UP: NSEventType = NSEventType(11);
const TYPE_FLAGS_CHANGED: NSEventType = NSEventType(12);

// NSEventModifierFlags: Control = 1<<18, Option = 1<<19.
const FLAG_CONTROL: NSEventModifierFlags = NSEventModifierFlags(1 << 18);
const FLAG_OPTION: NSEventModifierFlags = NSEventModifierFlags(1 << 19);

pub fn install(app: AppHandle) {
    if APP.set(app).is_err() {
        return;
    }
    // setup() already runs on the main thread, which NSEvent monitors require.
    unsafe { register_monitors() };
}

fn key_matches(key_code: u16, key: PttKey) -> bool {
    match key {
        PttKey::ControlRight => key_code == KEY_RIGHT_CONTROL,
        PttKey::ControlLeft => key_code == KEY_LEFT_CONTROL,
        PttKey::AltRight => key_code == KEY_RIGHT_OPTION,
        PttKey::AltLeft => key_code == KEY_LEFT_OPTION,
        PttKey::F8 => key_code == KEY_F8,
        PttKey::F13 => key_code == KEY_F13,
    }
}

fn modifier_down(key: PttKey, flags: NSEventModifierFlags) -> bool {
    match key {
        PttKey::ControlRight | PttKey::ControlLeft => flags.contains(FLAG_CONTROL),
        PttKey::AltRight | PttKey::AltLeft => flags.contains(FLAG_OPTION),
        PttKey::F8 | PttKey::F13 => false,
    }
}

fn is_modifier(key: PttKey) -> bool {
    matches!(
        key,
        PttKey::ControlRight | PttKey::ControlLeft | PttKey::AltRight | PttKey::AltLeft
    )
}

fn dispatch(down: bool) {
    let Some(app) = APP.get() else {
        return;
    };
    let app = app.clone();
    if down {
        crate::dictation::begin(&app);
    } else {
        crate::dictation::end(&app);
    }
}

fn handle(event: &NSEvent) {
    let Some(key) = super::current_key() else {
        return;
    };
    let key_code = event.keyCode();
    if !key_matches(key_code, key) {
        return;
    }

    let event_type = event.r#type();
    if is_modifier(key) {
        if event_type == TYPE_FLAGS_CHANGED {
            dispatch(modifier_down(key, event.modifierFlags()));
        }
        return;
    }

    if event_type == TYPE_KEY_DOWN {
        dispatch(true);
    } else if event_type == TYPE_KEY_UP {
        dispatch(false);
    }
}

unsafe fn register_monitors() {
    let mask = NSEventMask::from_bits_retain((1 << 10) | (1 << 11) | (1 << 12));

    let global = RcBlock::new(|event: NonNull<NSEvent>| {
        handle(unsafe { event.as_ref() });
    });

    // Keep the monitor alive for the process lifetime. Dropping it would
    // silently stop push-to-talk.
    let global_token: Option<Retained<AnyObject>> =
        unsafe { NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &global) };

    let local = RcBlock::new(|event: NonNull<NSEvent>| {
        handle(unsafe { event.as_ref() });
        event.as_ptr()
    });

    let local_token: Option<Retained<AnyObject>> =
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local) };

    std::mem::forget(global_token);
    std::mem::forget(local_token);
    std::mem::forget(global);
    std::mem::forget(local);
    tracing::info!("push-to-talk event monitors installed");
}
