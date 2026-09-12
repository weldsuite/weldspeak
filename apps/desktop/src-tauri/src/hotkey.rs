//! Global hotkeys, including push-to-talk.
//!
//! Tauri's global-shortcut plugin is built on `RegisterHotKey` / `CGEvent`,
//! which do not reliably report a **modifier held on its own** (Right Ctrl,
//! Right Option). Those are exactly the keys a dictation app should use.
//!
//! Detection is a short poll of the physical key state (`GetAsyncKeyState` /
//! `CGEventSourceKeyState`). A `WH_KEYBOARD_LL` callback is too easy for
//! Windows to skip or silently unhook — especially while our own WebView2
//! settings window, or Chrome, has focus — and calling into Tauri from that
//! callback is enough work to trip the system's hook timeout.
//!
//! Bindings are KeyboardEvent `code` strings (`ControlRight`, `KeyA`, `F8`)
//! captured in Settings, then polled by native code.

#[path = "hotkey_codes.rs"]
mod codes;

pub use codes::{label, native_code, types_while_held};

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::AppHandle;

#[cfg(target_os = "windows")]
#[path = "hotkey_windows.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "hotkey_macos.rs"]
mod platform;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    pub fn is_down(_code: u16) -> bool {
        false
    }
    pub fn is_escape_down() -> bool {
        false
    }
}

static APP: OnceLock<AppHandle> = OnceLock::new();
/// Native key currently watched. 0 means none.
static CURRENT: AtomicU16 = AtomicU16::new(0);
/// True while Settings is capturing a new key, so that press is not a dictation.
static SUSPENDED: AtomicBool = AtomicBool::new(false);
/// Set when the user picks a different hold key so a still-held previous key
/// cannot keep a dictation open.
static CANCEL_HOLD: AtomicBool = AtomicBool::new(false);

const HOLD_BEFORE_PTT: Duration = Duration::from_millis(140);
const DOUBLE_TAP: Duration = Duration::from_millis(420);

/// How the hotkey behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Hold to dictate, release to finish.
    #[default]
    PushToTalk,
    /// Press to start, press again to finish.
    Toggle,
}

/// A hotkey binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub mode: Mode,
    /// KeyboardEvent `code`, e.g. `ControlRight` or `F8`.
    pub accelerator: String,
}

impl Default for Binding {
    fn default() -> Self {
        Self {
            mode: Mode::PushToTalk,
            accelerator: default_accelerator().into(),
        }
    }
}

/// The default hold key for this platform.
///
/// Right-hand modifiers are chosen deliberately: they are rarely bound by other
/// software, and holding one does not shadow a shortcut the user relies on.
pub const fn default_accelerator() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "AltRight"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "ControlRight"
    }
}

/// Whether an accelerator can carry push-to-talk on this platform.
pub fn validate_for_push_to_talk(accelerator: &str) -> Result<(), String> {
    let accelerator = accelerator.trim();
    if accelerator.is_empty() {
        return Err("Choose a key to hold.".into());
    }

    if accelerator.eq_ignore_ascii_case("Fn") || accelerator.eq_ignore_ascii_case("Function") {
        return Err(
            "macOS does not report the Fn key to applications. Try holding Right Option instead."
                .into(),
        );
    }

    if accelerator.eq_ignore_ascii_case("Escape") {
        return Err("Escape cancels a dictation. Pick another key to hold.".into());
    }

    // Old plugin-style chords cannot be polled as a single physical key.
    if accelerator.contains('+') {
        return Err(
            "Combinations such as Ctrl+Space cannot be held on their own. Press a single key."
                .into(),
        );
    }

    if native_code(accelerator).is_none() {
        return Err("That key cannot be watched on this computer. Try another.".into());
    }

    Ok(())
}

/// Hint shown under the bind button when the key will also type.
pub fn hold_warning(accelerator: &str) -> Option<String> {
    if types_while_held(accelerator) {
        Some(format!(
            "Holding {} also types into whatever has focus. A modifier or function key is quieter.",
            label(accelerator)
        ))
    } else {
        None
    }
}

/// Start watching the hold key. Safe to call once, at launch.
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
    std::thread::Builder::new()
        .name("weldspeak-ptt".into())
        .spawn(poll_loop)
        .expect("failed to start push-to-talk");
}

/// Point the watcher at the key currently chosen in Settings.
pub fn listen_for(accelerator: &str) {
    let code = native_code(accelerator).unwrap_or(0);
    CURRENT.store(code, Ordering::SeqCst);
    CANCEL_HOLD.store(true, Ordering::SeqCst);
}

/// Ignore the hold key while Settings is capturing a replacement.
pub fn suspend(paused: bool) {
    SUSPENDED.store(paused, Ordering::SeqCst);
    if paused {
        CANCEL_HOLD.store(true, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Pressed,
    Ptt,
    HandsFree,
}

fn poll_loop() {
    tracing::info!("push-to-talk key watcher started");
    let mut phase = Phase::Idle;
    let mut pressed_at: Option<Instant> = None;
    let mut last_short_release: Option<Instant> = None;
    let mut escape_held = false;
    let mut hf_stop_armed = false;

    loop {
        let cancel = CANCEL_HOLD.swap(false, Ordering::SeqCst);
        let suspended = SUSPENDED.load(Ordering::Relaxed);
        let code = CURRENT.load(Ordering::Relaxed);
        let key_down = !cancel && !suspended && code != 0 && platform::is_down(code);
        let escape = !suspended && platform::is_escape_down();
        let now = Instant::now();

        if cancel && phase != Phase::Idle {
            if matches!(phase, Phase::Ptt | Phase::HandsFree) {
                dispatch_end(true);
            }
            phase = Phase::Idle;
            pressed_at = None;
            hf_stop_armed = false;
        }

        if escape && !escape_held && phase != Phase::Idle {
            if matches!(phase, Phase::Ptt | Phase::HandsFree | Phase::Pressed) {
                if matches!(phase, Phase::Ptt | Phase::HandsFree) {
                    dispatch_end(true);
                }
            }
            phase = Phase::Idle;
            pressed_at = None;
            hf_stop_armed = false;
            last_short_release = None;
        }
        escape_held = escape;

        match phase {
            Phase::Idle => {
                if key_down {
                    phase = Phase::Pressed;
                    pressed_at = Some(now);
                }
            }
            Phase::Pressed => {
                if !key_down {
                    let is_double = last_short_release
                        .is_some_and(|t| now.duration_since(t) < DOUBLE_TAP);
                    if is_double {
                        phase = Phase::HandsFree;
                        last_short_release = None;
                        hf_stop_armed = false;
                        dispatch_begin();
                    } else {
                        last_short_release = Some(now);
                        phase = Phase::Idle;
                    }
                    pressed_at = None;
                } else if pressed_at.is_some_and(|t| now.duration_since(t) >= HOLD_BEFORE_PTT) {
                    phase = Phase::Ptt;
                    last_short_release = None;
                    dispatch_begin();
                }
            }
            Phase::Ptt => {
                if !key_down {
                    phase = Phase::Idle;
                    dispatch_end(false);
                }
            }
            Phase::HandsFree => {
                if key_down {
                    hf_stop_armed = true;
                } else if hf_stop_armed {
                    hf_stop_armed = false;
                    phase = Phase::Idle;
                    dispatch_end(false);
                }
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

fn dispatch_begin() {
    dispatch(|app| crate::dictation::begin(app));
}

fn dispatch_end(cancel: bool) {
    dispatch(move |app| {
        if cancel {
            crate::dictation::cancel(app);
        } else {
            crate::dictation::end(app);
        }
    });
}

fn dispatch(action: impl FnOnce(&AppHandle) + Send + 'static) {
    let Some(app) = APP.get() else {
        return;
    };
    let app = app.clone();
    if let Err(error) = app.clone().run_on_main_thread(move || action(&app)) {
        tracing::warn!(%error, "could not dispatch push-to-talk");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_hold_to_talk_on_a_right_hand_modifier() {
        let binding = Binding::default();

        assert_eq!(binding.mode, Mode::PushToTalk);
        assert!(binding.accelerator.contains("Right"));
    }

    #[test]
    fn explains_why_fn_cannot_be_used() {
        let error = validate_for_push_to_talk("Fn").unwrap_err();
        assert!(error.contains("Right Option"));
    }

    #[test]
    fn rejects_escape_because_it_cancels() {
        let error = validate_for_push_to_talk("Escape").unwrap_err();
        assert!(error.to_lowercase().contains("cancel"));
    }

    #[test]
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn accepts_letters_and_function_keys() {
        for accelerator in ["AltRight", "ControlRight", "F8", "F13", "KeyA", "Space", "F5"] {
            assert!(
                validate_for_push_to_talk(accelerator).is_ok(),
                "{accelerator} should be allowed"
            );
        }
    }

    #[test]
    fn warns_that_letters_will_type() {
        assert!(hold_warning("KeyA").is_some());
        assert!(hold_warning("ControlRight").is_none());
    }

    #[test]
    fn rejects_plugin_style_chords() {
        assert!(validate_for_push_to_talk("CommandOrControl+Space").is_err());
    }

    #[test]
    fn rejects_an_empty_binding() {
        assert!(validate_for_push_to_talk("  ").is_err());
    }

    #[test]
    fn round_trips_through_settings_json() {
        let binding = Binding {
            mode: Mode::Toggle,
            accelerator: "F13".into(),
        };

        let json = serde_json::to_string(&binding).unwrap();
        assert!(
            json.contains("\"toggle\""),
            "modes are camelCase on the wire: {json}"
        );

        assert_eq!(serde_json::from_str::<Binding>(&json).unwrap(), binding);
    }
}
