//! Native Hub window — Win32 / AppKit, no webview.
//!
//! Shared page model and data helpers used by both platform UIs.

use tauri::{AppHandle, Manager};

use crate::commands::{DictionaryTerm, OrgSummary, Status, TranscriptRecord};
use crate::settings::{InjectionPreference, Settings};
use crate::snippets::Snippet;
use crate::AppState;

pub const DASHBOARD_URL: &str = "https://weldspeak.com/dictionary";

pub const WINDOW_WIDTH: i32 = 980;
pub const WINDOW_HEIGHT: i32 = 700;
pub const WINDOW_MIN_WIDTH: i32 = 760;
pub const WINDOW_MIN_HEIGHT: i32 = 540;
pub const SIDEBAR_WIDTH: i32 = 212;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Dictionary,
    Snippets,
    Settings,
}

impl Page {
    pub const ALL: [Page; 4] = [Page::Home, Page::Dictionary, Page::Snippets, Page::Settings];

    pub fn label(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::Dictionary => "Dictionary",
            Page::Snippets => "Snippets",
            Page::Settings => "Settings",
        }
    }

    pub fn from_index(index: usize) -> Page {
        Self::ALL.get(index).copied().unwrap_or(Page::Home)
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }
}

pub const LOCALES: &[(&str, &str)] = &[
    ("", "Auto"),
    ("en", "English"),
    ("nl", "Dutch"),
    ("de", "German"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("pt", "Portuguese"),
    ("it", "Italian"),
    ("pl", "Polish"),
    ("sv", "Swedish"),
    ("da", "Danish"),
    ("nb", "Norwegian"),
    ("fi", "Finnish"),
    ("tr", "Turkish"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh", "Chinese"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
];

pub fn show(app: &AppHandle) {
    platform::show(app);
}

pub fn on_signed_in() {
    platform::refresh();
}

/// Call after a dictation is injected so Home can reload history.
pub fn on_history_changed() {
    platform::refresh();
}

#[cfg(target_os = "windows")]
#[path = "native_settings_windows.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "native_settings_macos.rs"]
mod platform;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    use tauri::AppHandle;
    pub fn show(_app: &AppHandle) {}
    pub fn refresh() {}
}

pub fn load_settings(app: &AppHandle) -> Settings {
    app.state::<AppState>()
        .settings
        .lock()
        .ok()
        .map(|s| s.clone())
        .unwrap_or_default()
}

pub fn patch_settings(app: &AppHandle, edit: impl FnOnce(&mut Settings)) {
    let path = crate::settings::path_for(app).ok();
    let state = app.state::<AppState>();
    let Ok(mut settings) = state.settings.lock() else {
        return;
    };
    edit(&mut settings);
    if let Some(path) = path {
        let _ = settings.save(&path);
    }
}

pub fn is_signed_in(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .auth
        .lock()
        .ok()
        .and_then(|auth| {
            auth.access_token(weldspeak_core::auth::now_secs())
                .map(str::to_owned)
        })
        .is_some()
}

pub fn hotkey_label(app: &AppHandle) -> String {
    let settings = load_settings(app);
    crate::hotkey::label(&settings.hotkey.accelerator)
}

pub fn words_dictated(app: &AppHandle) -> u64 {
    load_settings(app).words_dictated
}

pub fn version_footer(app: &AppHandle) -> String {
    format!(
        "WeldSpeak {} · {} words dictated",
        env!("CARGO_PKG_VERSION"),
        words_dictated(app)
    )
}

pub async fn fetch_status(app: AppHandle) -> Status {
    crate::commands::get_status(app).await
}

pub async fn fetch_transcripts(app: AppHandle) -> Result<Vec<TranscriptRecord>, String> {
    crate::commands::list_transcripts(app).await
}

pub async fn fetch_dictionary(app: AppHandle) -> Result<Vec<DictionaryTerm>, String> {
    crate::commands::list_dictionary(app).await
}

pub fn load_snippets(app: &AppHandle) -> Vec<Snippet> {
    load_settings(app).snippets
}

pub fn add_snippet(app: &AppHandle, trigger: String, expansion: String) -> Result<(), String> {
    let trigger = trigger.trim().to_string();
    let expansion = expansion.trim().to_string();
    if trigger.is_empty() {
        return Err("Type a cue, e.g. my address.".into());
    }
    if expansion.is_empty() {
        return Err("Type the text to insert.".into());
    }
    if trigger.chars().count() > 60 {
        return Err("Cue must be 60 characters or fewer.".into());
    }
    if expansion.chars().count() > 4000 {
        return Err("Expansion must be 4000 characters or fewer.".into());
    }
    patch_settings(app, |s| {
        s.snippets.retain(|snippet| snippet.trigger != trigger);
        s.snippets.push(Snippet { trigger, expansion });
    });
    Ok(())
}

pub fn remove_snippet(app: &AppHandle, index: usize) {
    patch_settings(app, |s| {
        if index < s.snippets.len() {
            s.snippets.remove(index);
        }
    });
}

pub fn set_org(app: &AppHandle, org_id: Option<String>) {
    patch_settings(app, |s| s.org_id = org_id);
}

pub fn set_locale(app: &AppHandle, locale: Option<String>) {
    patch_settings(app, |s| s.locale = locale);
}

pub fn set_injection(app: &AppHandle, injection: InjectionPreference) {
    patch_settings(app, |s| s.injection = injection);
}

pub fn set_microphone(app: &AppHandle, name: Option<String>) {
    patch_settings(app, |s| s.microphone = name);
    crate::reopen_microphone(app);
}

pub fn set_clean_up(app: &AppHandle, on: bool) {
    patch_settings(app, |s| s.clean_up_text = on);
}

pub fn set_pause_media(app: &AppHandle, on: bool) {
    patch_settings(app, |s| s.pause_media = on);
}

pub fn set_keep_history(app: &AppHandle, on: bool) {
    patch_settings(app, |s| s.keep_history = on);
}

pub fn set_hotkey(app: &AppHandle, accelerator: String) -> Result<(), String> {
    crate::hotkey::validate_for_push_to_talk(&accelerator)?;
    patch_settings(app, |s| {
        s.hotkey.accelerator = accelerator;
        s.hotkey.mode = crate::hotkey::Mode::PushToTalk;
    });
    crate::reregister_hotkey(app);
    Ok(())
}

pub fn copy_transcript_text(text: &str) -> Result<(), String> {
    crate::inject::copy_to_clipboard(text).map_err(|e| e.to_string())
}

pub fn org_label(orgs: &[OrgSummary], selected: Option<&str>) -> String {
    if orgs.is_empty() {
        return "Personal".into();
    }
    orgs.iter()
        .find(|o| Some(o.org_id.as_str()) == selected)
        .map(|o| o.name.clone())
        .unwrap_or_else(|| orgs[0].name.clone())
}

pub fn truncate(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}
