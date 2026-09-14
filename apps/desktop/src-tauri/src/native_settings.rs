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
pub const SIDEBAR_WIDTH: i32 = 200;

/// Shared Hub palette (brand-aligned, light content + dark rail).
pub mod theme {
    /// Content background `#faf9f6`
    pub const CONTENT_BG_RGB: (u8, u8, u8) = (0xfa, 0xf9, 0xf6);
    /// Sidebar `#1c1f1d`
    pub const SIDEBAR_BG_RGB: (u8, u8, u8) = (0x1c, 0x1f, 0x1d);
    /// Selected nav chip `#2a2e2b`
    pub const SIDEBAR_CHIP_RGB: (u8, u8, u8) = (0x2a, 0x2e, 0x2b);
    /// Brand orange `#de713e`
    pub const BRAND_RGB: (u8, u8, u8) = (0xde, 0x71, 0x3e);
    /// Primary text `#242723`
    pub const TEXT_RGB: (u8, u8, u8) = (0x24, 0x27, 0x23);
    /// Muted text `#666d63`
    pub const MUTED_RGB: (u8, u8, u8) = (0x66, 0x6d, 0x63);
    /// Sidebar label `#f3f2ee`
    pub const SIDEBAR_TEXT_RGB: (u8, u8, u8) = (0xf3, 0xf2, 0xee);
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
