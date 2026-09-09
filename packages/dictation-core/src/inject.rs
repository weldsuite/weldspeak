//! Deciding *how* to put text into the focused application.
//!
//! There are two ways to get text into another app, and neither works
//! everywhere:
//!
//!   - **Synthesising keystrokes** types the characters one at a time. It
//!     leaves the clipboard alone, which users notice, but it is linear in the
//!     length of the text and becomes visibly slow past a paragraph or so.
//!   - **Paste** puts the text on the clipboard and sends the paste shortcut.
//!     It is instant at any length, but it clobbers whatever the user had
//!     copied, so the previous contents have to be saved and put back.
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

/// Length past which typing character-by-character becomes noticeably slow.
///
/// Synthesised keystrokes need a small delay between them for applications to
/// keep up; a couple of thousand characters is where the accumulated delay
/// stops feeling instant and starts looking like the app has hung.
pub const TYPING_LENGTH_LIMIT: usize = 2_000;

/// Delay between setting the clipboard and sending the paste shortcut.
///
/// Some applications — Electron ones especially — read the clipboard
/// asynchronously and paste stale contents if the shortcut arrives too soon.
pub const CLIPBOARD_SETTLE_MS: u64 = 30;

/// Delay before restoring the user's previous clipboard contents.
///
/// Restoring too eagerly races the paste and puts the old text in instead. This
/// is generous on purpose: the cost of waiting is that a fast Cmd-V within
/// 150 ms pastes the dictation, while the cost of not waiting is pasting the
/// wrong thing entirely.
pub const CLIPBOARD_RESTORE_MS: u64 = 150;

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
    /// Choose per dictation: type short text, paste long text.
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
        Preference::AlwaysPaste => Method::Paste,
        Preference::Automatic => {
            if text.chars().count() > TYPING_LENGTH_LIMIT {
                Method::Paste
            } else {
                Method::Type
            }
        }
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
    fn short_text_is_typed_and_leaves_the_clipboard_alone() {
        let plan = plan("The weld looks good.", Preference::Automatic);

        assert_eq!(plan.method, Method::Type);
        assert!(!plan.preserve_clipboard);
    }

    #[test]
    fn long_text_is_pasted_because_typing_it_would_crawl() {
        let plan = plan(&text_of(TYPING_LENGTH_LIMIT + 1), Preference::Automatic);

        assert_eq!(plan.method, Method::Paste);
        assert!(plan.preserve_clipboard, "the user's clipboard must be restored");
    }

    #[test]
    fn the_threshold_counts_characters_not_bytes() {
        // Accented characters and emoji are multi-byte; counting bytes would
        // switch to pasting far earlier than intended for non-English dictation.
        let accented = "é".repeat(TYPING_LENGTH_LIMIT - 1);
        assert!(accented.len() > TYPING_LENGTH_LIMIT, "precondition: multi-byte");

        assert_eq!(plan(&accented, Preference::Automatic).method, Method::Type);
    }

    #[test]
    fn preferences_override_the_automatic_choice() {
        let long = text_of(TYPING_LENGTH_LIMIT + 100);
        assert_eq!(plan(&long, Preference::AlwaysType).method, Method::Type);

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

    #[test]
    fn clipboard_restore_waits_longer_than_the_paste_itself() {
        // Restoring before the paste lands puts the old clipboard into the
        // user's document instead of their dictation.
        assert!(CLIPBOARD_RESTORE_MS > CLIPBOARD_SETTLE_MS);
    }
}
