//! User settings.
//!
//! Kept deliberately small. A dictation utility that needs configuring before
//! it works has already failed; these are the choices people genuinely differ
//! on, and everything else has a defensible default.

use serde::{Deserialize, Serialize};
use weldspeak_core::inject::Preference;
use crate::hotkey::Binding;

/// Where the API lives. Overridable for local development.
pub const DEFAULT_API_BASE: &str = "https://app.weldspeak.io";

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
