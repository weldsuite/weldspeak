//! Deciding *how* to put text into the focused application.
//!
//! There are two ways to get text into another app, and neither works
//! everywhere:
//!
//!   - **Synthesising keystrokes** types the characters one at a time. It
//!     leaves the clipboard alone, but the text visibly streams in word by
//!     word, and editors with autocomplete or auto-indent react to every key.
//!   - **Paste** puts the text on the clipboard and sends the paste shortcut.
//!     The whole dictation lands in one sweep at any length, but it clobbers
//!     whatever the user had copied, so the previous contents are saved and
//!     put back.
//!
//! Automatic mode pastes, as Wispr Flow does: text appearing all at once is
//! what makes dictation feel instant. Typing stays available as a setting for
//! the odd app that blocks paste.
//!
//! The choice is a policy decision that has nothing to do with any particular
//! operating system, so it lives here where it can be tested, while the actual
//! event synthesis lives in the platform modules of the desktop crate.

/// How to deliver text to the focused application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    /// Synthesise a keystroke per character.
    Type,
    /// Set the clipboard and send the paste shortcut, restoring it afterwards.
    Paste,
}

/// Delay between setting the clipboard and sending the paste shortcut.
///
/// Some applications — Electron ones especially — read the clipboard
/// asynchronously and paste stale contents if the shortcut arrives too soon.
pub const CLIPBOARD_SETTLE_MS: u64 = 30;

/// Delay before restoring the user's previous clipboard contents.
///
/// Restoring too eagerly races the paste and puts the old text in instead.
/// Browsers and Electron apps read the clipboard asynchronously and can take a
/// few hundred milliseconds under load, so this is generous on purpose: the
/// cost of waiting is that a fast Cmd-V within the window pastes the
/// dictation, while the cost of not waiting is pasting the wrong thing
/// entirely.
pub const CLIPBOARD_RESTORE_MS: u64 = 400;

/// A decision about how to deliver `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub method: Method,
    pub text: String,
    /// Whether the previous clipboard contents must be saved and restored.
    pub preserve_clipboard: bool,
}

/// User preference for delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preference {
    /// Paste the whole dictation in one sweep, restoring the clipboard.
    #[default]
    Automatic,
    /// Always type, never touch the clipboard.
    AlwaysType,
    /// Always paste.
    AlwaysPaste,
}

/// Decide how to deliver `text`.
///
/// `accessibility_granted` is the macOS reality check: without that permission
/// no keystroke can be synthesised at all, including the paste shortcut, so the
/// caller has to fall back to leaving the text on the clipboard and telling the
/// user. That is handled by [`plan_or_clipboard_only`].
pub fn plan(text: &str, preference: Preference) -> Plan {
    let method = match preference {
        Preference::AlwaysType => Method::Type,
        Preference::AlwaysPaste | Preference::Automatic => Method::Paste,
    };

    Plan {
        preserve_clipboard: method == Method::Paste,
        method,
        text: text.to_string(),
    }
}

/// What to do when text cannot be injected at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fallback {
    /// Proceed with the plan.
    Inject(Plan),
    /// Injection is impossible; the text is on the clipboard instead.
    ///
    /// Reached when the OS withholds the permission needed to synthesise input
    /// — commonly a managed machine where Accessibility is blocked by policy.
    /// Leaving the text on the clipboard and saying so keeps the app useful
    /// instead of appearing broken.
    ClipboardOnly { text: String, reason: String },
}

/// Plan delivery, degrading to clipboard-only when input cannot be synthesised.
pub fn plan_or_clipboard_only(
    text: &str,
    preference: Preference,
    can_synthesise_input: bool,
) -> Fallback {
    if can_synthesise_input {
        Fallback::Inject(plan(text, preference))
    } else {
        Fallback::ClipboardOnly {
            text: text.to_string(),
            reason: "WeldSpeak needs Accessibility permission to type into other apps. \
                     Your dictation is on the clipboard — press paste to insert it."
                .into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(length: usize) -> String {
        "a".repeat(length)
    }

    #[test]
    fn automatic_pastes_in_one_sweep_and_restores_the_clipboard() {
        // Even a short sentence is pasted: typed keystrokes stream in visibly
        // and trip autocomplete in editors.
        for text in ["The weld looks good.".to_string(), text_of(50_000)] {
            let plan = plan(&text, Preference::Automatic);
            assert_eq!(plan.method, Method::Paste);
            assert!(
                plan.preserve_clipboard,
                "the user's clipboard must be restored"
            );
        }
    }

    #[test]
    fn preferences_override_the_automatic_choice() {
        assert_eq!(plan("short", Preference::AlwaysType).method, Method::Type);
        assert_eq!(plan("short", Preference::AlwaysPaste).method, Method::Paste);
    }

    #[test]
    fn always_type_never_touches_the_clipboard() {
        let plan = plan(&text_of(50_000), Preference::AlwaysType);
        assert!(!plan.preserve_clipboard);
    }

    #[test]
    fn text_is_carried_through_unmodified() {
        // Whatever cleanup produced is what the user sees; this stage must not
        // trim, wrap or otherwise touch it.
        let text = "  Check the root pass.\n\n- Purge argon\n- Re-test  ";
        assert_eq!(plan(text, Preference::Automatic).text, text);
    }

    #[test]
    fn degrades_to_the_clipboard_when_input_cannot_be_synthesised() {
        let fallback = plan_or_clipboard_only("hello", Preference::Automatic, false);

        match fallback {
            Fallback::ClipboardOnly { text, reason } => {
                assert_eq!(text, "hello");
                // The message has to say what happened and what to do next; a
                // silent failure reads as the app being broken.
                assert!(reason.contains("Accessibility"));
                assert!(reason.contains("clipboard"));
            }
            other => panic!("expected clipboard fallback, got {other:?}"),
        }
    }

    #[test]
    fn injects_normally_when_permission_is_present() {
        let fallback = plan_or_clipboard_only("hello", Preference::Automatic, true);
        assert!(matches!(fallback, Fallback::Inject(_)));
    }

    /// Restoring before the paste lands puts the old clipboard into the user's
    /// document instead of their dictation. Checked at compile time so the
    /// ordering cannot be broken by editing either constant.
    const _: () = assert!(CLIPBOARD_RESTORE_MS > CLIPBOARD_SETTLE_MS);
}
