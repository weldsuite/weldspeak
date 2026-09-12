//! Putting text into whatever application has focus.
//!
//! This is the part of a dictation app with the least portable code and the
//! most ways to fail quietly. The policy — type or paste, and what to do when
//! the OS says no — lives in `weldspeak-core::inject` where it is testable.
//! What lives here is the platform-specific event synthesis those decisions
//! drive.

use anyhow::Result;
use weldspeak_core::inject::{Fallback, Method, Plan, Preference};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;

// Linux is not a supported target, but the crate must still build there for
// `cargo check` on a developer's machine and in CI lint jobs.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod unsupported;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use unsupported as platform;

/// Whether the OS currently permits synthesising input.
///
/// On macOS this is the Accessibility permission and is genuinely dynamic — the
/// user can revoke it while the app runs, and it resets when the app's
/// signature changes. It is therefore checked per dictation rather than cached
/// at startup.
pub fn can_synthesise_input() -> bool {
    platform::can_synthesise_input()
}

/// Open the OS settings page where the user grants the missing permission.
///
/// Deep-linking beats telling someone to "go to System Settings": the pane is
/// several levels down and easy to give up on.
pub fn open_permission_settings() -> Result<()> {
    platform::open_permission_settings()
}

/// The outcome of trying to deliver text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The text is in the focused application.
    Injected,
    /// Input could not be synthesised; the text is on the clipboard instead.
    ClipboardOnly { reason: String },
}

/// Deliver `text` to the focused application.
///
/// Never returns an error for a missing permission — that is an expected state
/// on a managed machine, and the right response is a usable fallback plus an
/// explanation, not a failure the user cannot act on.
pub fn deliver(text: &str, preference: Preference) -> Result<Outcome> {
    match weldspeak_core::inject::plan_or_clipboard_only(
        text,
        preference,
        can_synthesise_input(),
    ) {
        Fallback::Inject(plan) => {
            execute(&plan)?;
            Ok(Outcome::Injected)
        }
        Fallback::ClipboardOnly { text, reason } => {
            set_clipboard(&text)?;
            Ok(Outcome::ClipboardOnly { reason })
        }
    }
}

fn execute(plan: &Plan) -> Result<()> {
    match plan.method {
        Method::Type => platform::type_text(&plan.text),
        Method::Paste => paste(&plan.text, plan.preserve_clipboard),
    }
}

/// Put text on the clipboard, send the paste shortcut, then restore.
///
/// The restore is why this is not simply "set clipboard, press paste": people
/// keep things on their clipboard, and silently destroying that is the kind of
/// small betrayal that gets an app uninstalled.
fn paste(text: &str, preserve_clipboard: bool) -> Result<()> {
    use std::thread::sleep;
    use std::time::Duration;
    use weldspeak_core::inject::{CLIPBOARD_RESTORE_MS, CLIPBOARD_SETTLE_MS};

    let previous = if preserve_clipboard {
        // Non-text clipboard contents (an image, a file) cannot be round-tripped
        // through a string, so they are lost. Failing the whole dictation over
        // it would be worse.
        arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok())
    } else {
        None
    };

    set_clipboard(text)?;

    // Some applications — Electron ones especially — read the clipboard
    // asynchronously and paste stale contents if the shortcut arrives too soon.
    sleep(Duration::from_millis(CLIPBOARD_SETTLE_MS));
    platform::send_paste_shortcut()?;

    if let Some(previous) = previous {
        // Restoring immediately races the paste and puts the old text into the
        // user's document instead of their dictation.
        sleep(Duration::from_millis(CLIPBOARD_RESTORE_MS));
        let _ = set_clipboard(&previous);
    }

    Ok(())
}

pub fn copy_to_clipboard(text: &str) -> Result<()> {
    set_clipboard(text)
}

fn set_clipboard(text: &str) -> Result<()> {
    arboard::Clipboard::new()?.set_text(text.to_string())?;
    Ok(())
}
