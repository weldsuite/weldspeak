//! Global hotkeys, including push-to-talk.
//!
//! Two modes, and they need different machinery.
//!
//! **Toggle** — press once to start, again to stop — is a plain global
//! shortcut, and `tauri-plugin-global-shortcut` handles it on both platforms.
//!
//! **Push-to-talk** — hold to dictate — needs key *down* and *up* separately,
//! which that plugin does not expose. So it goes through a platform hook:
//! `WH_KEYBOARD_LL` on Windows, `NSEvent`'s global monitor on macOS.
//!
//! A note on defaults, since this is where dictation apps disappoint people:
//! holding **Fn** is the gesture everyone asks for, and on macOS it is the one
//! key a normal event tap does not deliver. Fn arrives as a modifier flag on
//! `flagsChanged` rather than as a key event, and on recent hardware it is
//! partly claimed by the system. Rather than promising it and shipping
//! something flaky, the defaults are Right Option on macOS and Right Ctrl on
//! Windows — both unused by almost everything, both reliably observable — and
//! the binding is configurable.

use serde::{Deserialize, Serialize};

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
    fn round_trips_through_settings_json() {
        let binding = Binding { mode: Mode::Toggle, accelerator: "F13".into() };

        let json = serde_json::to_string(&binding).unwrap();
        assert!(json.contains("\"toggle\""), "modes are camelCase on the wire: {json}");

        assert_eq!(serde_json::from_str::<Binding>(&json).unwrap(), binding);
    }
}
