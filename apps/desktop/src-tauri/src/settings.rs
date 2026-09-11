//! User settings.
//!
//! Kept deliberately small. A dictation utility that needs configuring before
//! it works has already failed; these are the choices people genuinely differ
//! on, and everything else has a defensible default.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use weldspeak_core::inject::Preference;
use crate::hotkey::Binding;

/// Where the API lives. Overridable for local development.
pub const DEFAULT_API_BASE: &str = "https://weldspeak.weldsuite.org";

/// Hosts from earlier drafts. If a saved settings file still points at one of
/// these, rewrite it to the live hostname so an old file cannot silently send
/// sign-in at a domain that does not exist.
const LEGACY_API_BASES: &[&str] = &[
    "https://app.weldspeak.io",
    "https://api.weldspeak.io",
    "https://weldspeak.com",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub api_base: String,
    pub hotkey: Binding,

    /// Active organization. Its shared glossary and policy apply to dictations.
    pub org_id: Option<String>,

    /// How text is delivered to the focused application.
    pub injection: InjectionPreference,

    /// Run the cleanup pass. Off means the raw transcript is inserted verbatim,
    /// which some people prefer for code and note-taking.
    pub clean_up_text: bool,

    /// Language hint. None lets the model detect it.
    pub locale: Option<String>,

    /// Keep a local history of dictations.
    ///
    /// An organization admin can force this off for everyone; when they do, the
    /// server flag wins and no local copy is written either. Storing the text
    /// of everything someone dictates is not a decision to make casually.
    pub keep_history: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            api_base: DEFAULT_API_BASE.into(),
            hotkey: Binding::default(),
            org_id: None,
            injection: InjectionPreference::Automatic,
            clean_up_text: true,
            locale: None,
            keep_history: true,
        }
    }
}

impl Settings {
    /// Load from disk, or the built-in defaults if the file is missing or junk.
    pub fn load(path: &Path) -> Self {
        let mut settings = match std::fs::read_to_string(path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        settings.migrate_api_base();
        settings
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self).context("serializing settings")?;
        std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    fn migrate_api_base(&mut self) {
        let trimmed = self.api_base.trim_end_matches('/');
        if LEGACY_API_BASES.contains(&trimmed) {
            self.api_base = DEFAULT_API_BASE.into();
        }
    }
}

/// `%APPDATA%\io.weldspeak.desktop\settings.json` on Windows.
pub fn path_for(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?
        .join("settings.json"))
}

/// Serializable mirror of `weldspeak_core::inject::Preference`.
///
/// The core type is deliberately free of serde so the portable crate stays
/// dependency-light; this converts at the edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum InjectionPreference {
    #[default]
    Automatic,
    AlwaysType,
    AlwaysPaste,
}

impl From<InjectionPreference> for Preference {
    fn from(value: InjectionPreference) -> Self {
        match value {
            InjectionPreference::Automatic => Preference::Automatic,
            InjectionPreference::AlwaysType => Preference::AlwaysType,
            InjectionPreference::AlwaysPaste => Preference::AlwaysPaste,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works_out_of_the_box() {
        let settings = Settings::default();

        // Cleanup on and automatic delivery: the app should be useful before
        // anyone opens settings.
        assert!(settings.clean_up_text);
        assert_eq!(settings.injection, InjectionPreference::Automatic);
        assert_eq!(settings.api_base, "https://weldspeak.weldsuite.org");
    }

    #[test]
    fn rewrites_legacy_api_hosts() {
        let dir = std::env::temp_dir().join(format!("weldspeak-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"apiBase":"https://app.weldspeak.io"}"#).unwrap();

        let settings = Settings::load(&path);
        assert_eq!(settings.api_base, DEFAULT_API_BASE);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("weldspeak-settings-rt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");

        let mut original = Settings::default();
        original.keep_history = false;
        original.save(&path).unwrap();

        let loaded = Settings::load(&path);
        assert!(!loaded.keep_history);
        assert_eq!(loaded.api_base, DEFAULT_API_BASE);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        // Settings written by an older version must still load; `default` on the
        // serde container is what makes adding a field a non-event.
        let settings: Settings = serde_json::from_str(r#"{"apiBase":"http://localhost:8787"}"#)
            .expect("partial settings should load");

        assert_eq!(settings.api_base, "http://localhost:8787");
        assert!(settings.clean_up_text);
    }

    #[test]
    fn round_trips() {
        let settings = Settings {
            org_id: Some("org_acme".into()),
            injection: InjectionPreference::AlwaysPaste,
            ..Default::default()
        };

        let json = serde_json::to_string(&settings).unwrap();
        let parsed: Settings = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.org_id, settings.org_id);
        assert_eq!(parsed.injection, InjectionPreference::AlwaysPaste);
    }

    #[test]
    fn maps_onto_the_core_preference() {
        assert_eq!(Preference::from(InjectionPreference::AlwaysType), Preference::AlwaysType);
        assert_eq!(Preference::from(InjectionPreference::Automatic), Preference::Automatic);
    }
}
