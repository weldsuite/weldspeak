//! Global hotkeys, including push-to-talk.
//!
//! Tauri's global-shortcut plugin is built on `RegisterHotKey` / `CGEvent`,
//! which do not reliably report a **modifier held on its own** (Right Ctrl,
//! Right Option). Those are exactly the keys a dictation app should use, so
//! push-to-talk is observed through a platform hook instead: `WH_KEYBOARD_LL`
//! on Windows, `NSEvent` monitors on macOS.
//!
//! A note on defaults: holding **Fn** is the gesture everyone asks for, and on
//! macOS it is the one key a normal event tap does not deliver. The defaults
//! are Right Option on macOS and Right Ctrl on Windows.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};
use tauri::AppHandle;

#[cfg(target_os = "windows")]
#[path = "hotkey_windows.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "hotkey_macos.rs"]
mod platform;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    pub fn install(_app: tauri::AppHandle) {}
}

/// Encoded `PttKey` observed by the platform hook. 0 means none yet.
static CURRENT: AtomicU8 = AtomicU8::new(0);

/// How the hotkey behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Hold to dictate, release to finish.
    #[default]
    PushToTalk,
    /// Press to start, press again to finish.
    Toggle,
}

/// A hotkey binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub mode: Mode,
    /// Accelerator string, in the form `tauri-plugin-global-shortcut` parses.
    pub accelerator: String,
}

impl Default for Binding {
    fn default() -> Self {
        Self {
            mode: Mode::PushToTalk,
            accelerator: default_accelerator().into(),
        }
    }
}

/// The default hold key for this platform.
///
/// Right-hand modifiers are chosen deliberately: they are rarely bound by other
/// software, and holding one does not shadow a shortcut the user relies on.
pub const fn default_accelerator() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "AltRight"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "ControlRight"
    }
}

/// Whether an accelerator can carry push-to-talk on this platform.
///
/// Returns a reason rather than a bare bool so the settings UI can explain the
/// refusal instead of silently rejecting a key the user just pressed.
pub fn validate_for_push_to_talk(accelerator: &str) -> Result<(), String> {
    if accelerator.trim().is_empty() {
        return Err("Choose a key to hold.".into());
    }

    // Fn is the one users ask for and the one macOS will not deliver: it
    // arrives as a modifier flag rather than a key event, and recent hardware
    // reserves part of its behaviour for the system.
    if accelerator.eq_ignore_ascii_case("Fn") || accelerator.eq_ignore_ascii_case("Function") {
        return Err(
            "macOS does not report the Fn key to applications. Try holding Right Option instead."
                .into(),
        );
    }

    // A hold binding with a printable key would insert characters into whatever
    // has focus for as long as the user speaks.
    if is_printable_key(accelerator) {
        return Err(format!(
            "Holding {accelerator} would type into whatever you are working in. \
             Choose a modifier key such as Right Option or Right Ctrl."
        ));
    }

    Ok(())
}

/// A single key the user can hold to talk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PttKey {
    ControlRight = 1,
    ControlLeft = 2,
    AltRight = 3,
    AltLeft = 4,
    F8 = 5,
    F13 = 6,
}

impl PttKey {
    pub fn parse(accelerator: &str) -> Option<Self> {
        match accelerator.trim() {
            "ControlRight" | "CtrlRight" => Some(Self::ControlRight),
            "ControlLeft" | "CtrlLeft" => Some(Self::ControlLeft),
            "AltRight" | "OptionRight" => Some(Self::AltRight),
            "AltLeft" | "OptionLeft" => Some(Self::AltLeft),
            "F8" => Some(Self::F8),
            "F13" => Some(Self::F13),
            _ => None,
        }
    }

    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::ControlRight),
            2 => Some(Self::ControlLeft),
            3 => Some(Self::AltRight),
            4 => Some(Self::AltLeft),
            5 => Some(Self::F8),
            6 => Some(Self::F13),
            _ => None,
        }
    }
}

/// Start the platform hook. Safe to call once, at launch.
pub fn install(app: &AppHandle) {
    platform::install(app.clone());
}

/// Point the hook at the key currently chosen in Settings.
pub fn listen_for(accelerator: &str) {
    let code = PttKey::parse(accelerator).map(|key| key as u8).unwrap_or(0);
    CURRENT.store(code, Ordering::Relaxed);
}

pub(crate) fn current_key() -> Option<PttKey> {
    PttKey::from_u8(CURRENT.load(Ordering::Relaxed))
}

/// Whether an accelerator names a single character-producing key.
fn is_printable_key(accelerator: &str) -> bool {
    // Combinations are fine; it is a lone printable key that causes trouble.
    if accelerator.contains('+') {
        return false;
    }
    accelerator.chars().count() == 1 && accelerator.chars().all(|c| c.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_hold_to_talk_on_a_right_hand_modifier() {
        let binding = Binding::default();

        assert_eq!(binding.mode, Mode::PushToTalk);
        assert!(binding.accelerator.contains("Right"));
    }

    #[test]
    fn explains_why_fn_cannot_be_used() {
        let error = validate_for_push_to_talk("Fn").unwrap_err();

        // The message has to name an alternative: "unsupported" alone leaves
        // the user guessing at what will work.
        assert!(error.contains("Right Option"));
    }

    #[test]
    fn rejects_a_lone_printable_key() {
        // Holding `a` for a sentence types "aaaaaaaa" into the user's document.
        let error = validate_for_push_to_talk("a").unwrap_err();
        assert!(error.contains("type into"));
    }

    #[test]
    fn accepts_modifiers_and_combinations() {
        for accelerator in ["AltRight", "ControlRight", "CommandOrControl+Space", "F13"] {
            assert!(
                validate_for_push_to_talk(accelerator).is_ok(),
                "{accelerator} should be allowed"
            );
        }
    }

    #[test]
    fn rejects_an_empty_binding() {
        assert!(validate_for_push_to_talk("  ").is_err());
    }

    #[test]
    fn parses_the_keys_settings_offers() {
        assert_eq!(PttKey::parse("ControlRight"), Some(PttKey::ControlRight));
        assert_eq!(PttKey::parse("AltRight"), Some(PttKey::AltRight));
        assert_eq!(PttKey::parse("F8"), Some(PttKey::F8));
        assert_eq!(PttKey::parse("F13"), Some(PttKey::F13));
        assert_eq!(PttKey::parse("CommandOrControl+Space"), None);
    }

    #[test]
    fn round_trips_through_settings_json() {
        let binding = Binding { mode: Mode::Toggle, accelerator: "F13".into() };

        let json = serde_json::to_string(&binding).unwrap();
        assert!(json.contains("\"toggle\""), "modes are camelCase on the wire: {json}");

        assert_eq!(serde_json::from_str::<Binding>(&json).unwrap(), binding);
    }
}
