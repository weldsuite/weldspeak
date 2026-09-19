//! Shared Hub theme tokens and refresh hooks.
//!
//! The Hub UI itself is a Tauri webview (`apps/desktop/src`). The listening
//! pill stays native and reuses these overlay colours.

use tauri::{AppHandle, Emitter};

/// Shared Hub + pill palette — cool mist content, slate rail, teal accent
/// (Wispr Flow–adjacent; no cream/orange charcoal).
pub mod theme {
    /// Content background `#f7f9fb`
    pub const CONTENT_BG_RGB: (u8, u8, u8) = (0xf7, 0xf9, 0xfb);
    /// Soft white surface / history card `#ffffff`
    pub const SURFACE_RGB: (u8, u8, u8) = (0xff, 0xff, 0xff);
    /// Hairline on surfaces `#e2e8f0`
    pub const SURFACE_BORDER_RGB: (u8, u8, u8) = (0xe2, 0xe8, 0xf0);
    /// Sidebar `#0f172a`
    pub const SIDEBAR_BG_RGB: (u8, u8, u8) = (0x0f, 0x17, 0x2a);
    /// Selected nav chip `#1e293b`
    pub const SIDEBAR_CHIP_RGB: (u8, u8, u8) = (0x1e, 0x29, 0x3b);
    /// Accent teal `#0f766e` (nav selection + Hub accent rail)
    pub const BRAND_RGB: (u8, u8, u8) = (0x0f, 0x76, 0x6e);
    /// Soft accent wash behind selected chip `#134e4a`
    pub const SIDEBAR_CHIP_ACCENT_RGB: (u8, u8, u8) = (0x13, 0x4e, 0x4a);
    /// Primary text `#0f172a`
    pub const TEXT_RGB: (u8, u8, u8) = (0x0f, 0x17, 0x2a);
    /// Muted text `#64748b`
    pub const MUTED_RGB: (u8, u8, u8) = (0x64, 0x74, 0x8b);
    /// Sidebar label `#f1f5f9`
    pub const SIDEBAR_TEXT_RGB: (u8, u8, u8) = (0xf1, 0xf5, 0xf9);
    /// Listening pill fill `#0f172a`
    pub const OVERLAY_BG_RGB: (u8, u8, u8) = (0x0f, 0x17, 0x2a);
    /// Listening waveform `#14b8a6`
    pub const OVERLAY_LISTEN_RGB: (u8, u8, u8) = (0x14, 0xb8, 0xa6);
    /// Thinking / muted bars `#94a3b8`
    pub const OVERLAY_MUTED_RGB: (u8, u8, u8) = (0x94, 0xa3, 0xb8);
    /// Notice text on pill `#f8fafc`
    pub const OVERLAY_TEXT_RGB: (u8, u8, u8) = (0xf8, 0xfa, 0xfc);
}

pub const DASHBOARD_URL: &str = "https://weldspeak.com/dictionary";

/// Notify the Hub webview that the account is signed in.
///
/// Callers usually emit `weldspeak://signed-in` themselves; this exists so
/// older call sites stay compiling.
pub fn on_signed_in() {}

/// Call after a dictation is injected so Home can reload history.
pub fn on_history_changed(app: &AppHandle) {
    let _ = app.emit("weldspeak://history-changed", ());
}
