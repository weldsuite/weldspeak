//! Listening-pill colours and Hub refresh hooks.
//!
//! The Hub UI itself is a Tauri webview (`apps/desktop/src`) with its own
//! stylesheet. Only the native listening pill reads these colours.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Listening pill palette: ink-black capsule, white bars — quiet enough to sit
/// over any app without drawing the eye away from the text being written.
pub mod theme {
    /// Pill fill `#111210`
    pub const OVERLAY_BG_RGB: (u8, u8, u8) = (0x11, 0x12, 0x10);
    /// Hairline around the pill so it separates from dark backgrounds `#3a3c37`
    pub const OVERLAY_BORDER_RGB: (u8, u8, u8) = (0x3a, 0x3c, 0x37);
    /// Waveform while listening `#faf9f6`
    pub const OVERLAY_LISTEN_RGB: (u8, u8, u8) = (0xfa, 0xf9, 0xf6);
    /// Bars while the transcript is being cleaned up `#7d8279`
    pub const OVERLAY_MUTED_RGB: (u8, u8, u8) = (0x7d, 0x82, 0x79);
    /// Notice text `#faf9f6`
    pub const OVERLAY_TEXT_RGB: (u8, u8, u8) = (0xfa, 0xf9, 0xf6);
}

/// Notify the Hub webview that the account is signed in.
///
/// Callers usually emit `weldspeak://signed-in` themselves; this exists so
/// older call sites stay compiling.
pub fn on_signed_in() {}

#[derive(Clone, Serialize)]
struct Dictated<'a> {
    text: &'a str,
}

/// Call after a dictation is injected so Home can show it and reload history.
///
/// The text travels with the event because the server stores the transcript
/// only after replying, so an immediate refetch would not include it yet.
pub fn on_history_changed(app: &AppHandle, text: &str) {
    let _ = app.emit("weldspeak://history-changed", Dictated { text });
}
