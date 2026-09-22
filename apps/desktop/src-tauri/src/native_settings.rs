//! Listening-pill colours and Hub refresh hooks.
//!
//! The Hub UI itself is a Tauri webview (`apps/desktop/src`) with its own
//! stylesheet. Only the native listening pill reads these colours.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Listening pill palette, taken from Wispr Flow's pill stylesheet so the two
/// read the same on screen.
pub mod theme {
    /// Pill fill: pure black (`--shade-black`).
    pub const OVERLAY_BG_RGB: (u8, u8, u8) = (0x00, 0x00, 0x00);
    /// 1 px rim that lifts the pill off dark backgrounds (`--vast-900`).
    pub const OVERLAY_BORDER_RGB: (u8, u8, u8) = (0x30, 0x30, 0x2f);
    /// Bars while the mic is live: white.
    pub const OVERLAY_LISTEN_RGB: (u8, u8, u8) = (0xff, 0xff, 0xff);
    /// Bars while thinking: 40 % white over the black fill.
    pub const OVERLAY_MUTED_RGB: (u8, u8, u8) = (0x66, 0x66, 0x66);
    /// Notice text.
    pub const OVERLAY_TEXT_RGB: (u8, u8, u8) = (0xff, 0xff, 0xff);
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
