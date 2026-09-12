//! WeldSpeak desktop client.
//!
//! A tray application with no window of its own most of the time: hold the
//! hotkey, speak, release, and the text appears wherever you were typing.
//!
//! The portable logic — audio conditioning, the session state machine,
//! injection policy, token lifetime — lives in `weldspeak-core`, where it is
//! covered by tests that run on any host. This crate is the platform edge:
//! microphone, hotkeys, event synthesis, and the Tauri shell that holds them
//! together.

pub mod api;
pub mod audio;
pub mod auth;
pub mod commands;
pub mod dictation;
pub mod hotkey;
pub mod inject;
pub mod learn;
pub mod media;
pub mod overlay;
pub mod settings;
pub mod snippets;
pub mod transport;
pub mod updater;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};
use tokio::sync::mpsc::UnboundedSender;
use weldspeak_core::{AuthStore, Frame, Session};

use transport::Outbound;

/// Everything the app holds between dictations.
pub struct AppState {
    pub settings: Mutex<settings::Settings>,
    pub auth: Mutex<AuthStore>,
    pub session: Mutex<Session>,
    /// The open microphone. Held for the app's lifetime so the pre-roll buffer
    /// always has audio in it when the hotkey goes down.
    pub capture: Mutex<Option<audio::Capture>>,
    /// Cloneable end of the capture → pump channel. Kept so a microphone
    /// change can open a new stream without tearing down the pump.
    pub frames: Mutex<Option<Sender<Frame>>>,
    /// Channel into the dictation currently in progress, if any.
    pub outbound: Mutex<Option<UnboundedSender<Outbound>>>,
    /// True while the listening pill is on screen, so the audio thread can
    /// drive the waveform without the overlay polling.
    pub overlay_live: AtomicBool,
    /// Last successfully injected dictation, for Paste last / Copy last.
    pub last_transcript: Mutex<Option<String>>,
    /// Media we paused or muted for the current dictation.
    pub media: Mutex<media::MediaPause>,
    /// Settings asked for a different microphone during a dictation.
    pub mic_dirty: AtomicBool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            settings: Mutex::new(settings::Settings::default()),
            auth: Mutex::new(AuthStore::new()),
            session: Mutex::new(Session::new()),
            capture: Mutex::new(None),
            frames: Mutex::new(None),
            outbound: Mutex::new(None),
            overlay_live: AtomicBool::new(false),
            last_transcript: Mutex::new(None),
            media: Mutex::new(media::MediaPause::default()),
            mic_dirty: AtomicBool::new(false),
        }
    }
}

/// Inject text into the focused application.
///
/// Dispatched to the main thread, which is not optional on macOS: the Text
/// Input Source APIs behind `CGEvent` abort the process when called from a
/// worker thread. That is the documented cause of the crash Tauri apps hit when
/// they synthesise input from a background task, and running the injection here
/// is what avoids it.
pub fn inject_on_main_thread(
    app: &AppHandle,
    text: String,
    preference: weldspeak_core::inject::Preference,
) {
    // The closure needs an owned handle of its own; `app` stays borrowed as the
    // receiver of run_on_main_thread.
    let for_closure = app.clone();

    let _ = app.run_on_main_thread(move || {
        match inject::deliver(&text, preference) {
            Ok(inject::Outcome::Injected) => {
                tracing::debug!(chars = text.chars().count(), "injected");
                crate::learn::after_inject(&for_closure, text);
            }
            Ok(inject::Outcome::ClipboardOnly { reason }) => {
                // Not an error: on a managed machine Accessibility may simply be
                // unavailable. The text is on the clipboard and the user is told.
                tracing::info!(%reason, "injection unavailable; text left on the clipboard");
                notify(&for_closure, &reason);
                crate::learn::after_inject(&for_closure, text);
            }
            Err(error) => {
                tracing::error!(?error, "injection failed");
                notify(
                    &for_closure,
                    "WeldSpeak could not insert the text. It is on your clipboard.",
                );
            }
        }
    });
}

/// Surface a message in the overlay window.
fn notify(app: &AppHandle, message: &str) {
    overlay::show_notice(app, message);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "weldspeak_desktop_lib=info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(build_shortcut_plugin())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::update_settings,
            commands::list_microphones,
            commands::validate_hotkey,
            commands::open_permission_settings,
            commands::get_status,
            commands::begin_sign_in,
            commands::sign_out,
            commands::list_dictionary,
            commands::add_dictionary_term,
            commands::delete_dictionary_term,
            commands::suspend_hotkey,
            commands::hotkey_warning,
            commands::hotkey_label,
            commands::paste_last_transcript,
            commands::copy_last_transcript,
            commands::list_transcripts,
            commands::delete_transcript,
            updater::install_update,
        ])
        .setup(|app| {
            // Accessory activation policy: WeldSpeak is a tray utility, and a
            // dock icon for something with no main window is just clutter.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();

            let settings_path = settings::path_for(&handle).map_err(anyhow::Error::msg)?;
            {
                let state = handle.state::<AppState>();
                *state.settings.lock().expect("settings poisoned") =
                    settings::Settings::load(&settings_path);
            }

            restore_session(&handle);
            build_tray(&handle)?;

            // The microphone opens now and stays open. That is what makes the
            // pre-roll possible, and it keeps device-start latency — a couple of
            // hundred milliseconds on macOS — off the front of every dictation.
            // The pump starts even if capture fails, so choosing a mic later in
            // Settings does not need a restart.
            let (frames_tx, frames_rx) = std::sync::mpsc::channel();
            {
                let state = handle.state::<AppState>();
                *state.frames.lock().expect("frames poisoned") = Some(frames_tx.clone());
            }
            dictation::spawn_audio_pump(handle.clone(), frames_rx);
            reopen_microphone(&handle);

            overlay::prepare(&handle)?;
            hotkey::install(&handle);
            reregister_hotkey(&handle);
            updater::spawn(handle.clone());

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build WeldSpeak")
        .run(|_app, event| {
            // Closing the settings window must not quit: the app lives in the
            // tray and the hotkey has to keep working. Programmatic exits
            // (tray Quit → app.exit) carry a code and must be allowed through;
            // otherwise a zombie process keeps the refresh token and the next
            // launch triggers reuse detection, which signs the user out.
            if let RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}


/// Restore credentials saved by a previous run.
///
/// Only the refresh token is persisted; the access token is short-lived and
/// cheap to re-obtain, so writing it to disk would add exposure for no benefit.
fn restore_session(app: &AppHandle) {
    let Some(refresh_token) = auth::load_refresh_token() else {
        return;
    };

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let api_base = {
            let state = app.state::<AppState>();
            let Ok(settings) = state.settings.lock() else {
                return;
            };
            settings.api_base.clone()
        };

        match auth::refresh(&api_base, &refresh_token).await {
            Ok(tokens) => {
                // Refresh tokens are single-use. If we fail to persist the
                // replacement, the next launch will present the old one and
                // the server will revoke the device.
                if let Err(error) = auth::save_refresh_token(&tokens.refresh_token) {
                    tracing::error!(?error, "could not persist credentials after restore");
                }

                let state = app.state::<AppState>();
                let mut store = state.auth.lock().expect("auth store poisoned");
                store.accept(tokens);
                drop(store);

                // Settings mounts before restore finishes and would stay on
                // the sign-in panel without this — same event as begin_sign_in.
                use tauri::Emitter;
                let _ = app.emit("weldspeak://signed-in", ());

                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    crate::learn::flush_to_dictionary(&app).await;
                });
            }
            Err(fatal) => {
                // Fatal means the server refused: revoked, reused, or the user
                // was removed from their organization. Anything else is a
                // network problem worth retrying rather than signing out over.
                {
                    let state = app.state::<AppState>();
                    let mut store = state.auth.lock().expect("auth store poisoned");
                    store.refresh_failed(weldspeak_core::auth::now_secs(), fatal);
                }
                if fatal {
                    let _ = auth::clear_refresh_token();
                }
            }
        }
    });
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let settings_item = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let paste_item = MenuItem::with_id(app, "paste-last", "Paste last dictation", true, None::<&str>)?;
    let copy_item = MenuItem::with_id(app, "copy-last", "Copy last dictation", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit WeldSpeak", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings_item, &paste_item, &copy_item, &quit_item])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_settings(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => show_settings(app),
            "paste-last" => {
                let _ = commands::paste_last_transcript(app.clone());
            }
            "copy-last" => {
                let _ = commands::copy_last_transcript(app.clone());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// The plugin is loaded so its capability stays valid; push-to-talk is
/// observed by the platform hook in `hotkey.rs`, not by a shortcut handler.
fn build_shortcut_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new().build()
}

fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Point capture at the microphone currently in settings.
///
/// Drops the old stream first so WASAPI releases the device, then opens the
/// saved name (or the system default). Skipped while a dictation is in
/// progress so arming is not lost mid-sentence.
pub(crate) fn reopen_microphone(app: &AppHandle) {
    let state = app.state::<AppState>();
    let idle = state
        .session
        .lock()
        .map(|session| session.is_idle())
        .unwrap_or(true);
    if !idle {
        state.mic_dirty.store(true, Ordering::SeqCst);
        tracing::info!("deferring microphone change until dictation ends");
        return;
    }
    state.mic_dirty.store(false, Ordering::SeqCst);

    let preferred = state
        .settings
        .lock()
        .ok()
        .and_then(|settings| settings.microphone.clone());
    let Some(frames) = state
        .frames
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
    else {
        return;
    };

    *state.capture.lock().expect("capture poisoned") = None;
    match audio::Capture::start(frames, preferred) {
        Ok(capture) => {
            *state.capture.lock().expect("capture poisoned") = Some(capture);
        }
        // A refused or missing microphone must not stop the app: the user
        // needs the settings window to pick another device.
        Err(error) => tracing::error!(?error, "could not open the microphone"),
    }
}

/// Point the platform watcher at the key currently in settings.
///
/// The Tauri shortcut plugin is still loaded (other capabilities use it) but
/// is not how push-to-talk is observed: a modifier held on its own never
/// arrives through `RegisterHotKey`.
pub(crate) fn reregister_hotkey(app: &AppHandle) {
    let accelerator = {
        let state = app.state::<AppState>();
        let Ok(settings) = state.settings.lock() else {
            return;
        };
        settings.hotkey.accelerator.clone()
    };

    hotkey::listen_for(&accelerator);
}

/// Escape cancels a dictation in progress.
#[allow(dead_code)]
fn escape_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::empty()), Code::Escape)
}
