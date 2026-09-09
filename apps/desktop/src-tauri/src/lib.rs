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

pub mod audio;
pub mod auth;
pub mod commands;
pub mod dictation;
pub mod hotkey;
pub mod inject;
pub mod settings;
pub mod transport;

use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tokio::sync::mpsc::UnboundedSender;
use weldspeak_core::{AuthStore, Session};

use transport::Outbound;

/// Everything the app holds between dictations.
pub struct AppState {
    pub settings: Mutex<settings::Settings>,
    pub auth: Mutex<AuthStore>,
    pub session: Mutex<Session>,
    /// The open microphone. Held for the app's lifetime so the pre-roll buffer
    /// always has audio in it when the hotkey goes down.
    pub capture: Mutex<Option<audio::Capture>>,
    /// Channel into the dictation currently in progress, if any.
    pub outbound: Mutex<Option<UnboundedSender<Outbound>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            settings: Mutex::new(settings::Settings::default()),
            auth: Mutex::new(AuthStore::new()),
            session: Mutex::new(Session::new()),
            capture: Mutex::new(None),
            outbound: Mutex::new(None),
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
            }
            Ok(inject::Outcome::ClipboardOnly { reason }) => {
                // Not an error: on a managed machine Accessibility may simply be
                // unavailable. The text is on the clipboard and the user is told.
                tracing::info!(%reason, "injection unavailable; text left on the clipboard");
                notify(&for_closure, &reason);
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
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.emit("weldspeak://notice", message);
    }
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
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::update_settings,
            commands::validate_hotkey,
            commands::open_permission_settings,
            commands::get_status,
            commands::begin_sign_in,
            commands::sign_out,
        ])
        .setup(|app| {
            // Accessory activation policy: WeldSpeak is a tray utility, and a
            // dock icon for something with no main window is just clutter.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();

            restore_session(&handle);
            build_tray(&handle)?;

            // The microphone opens now and stays open. That is what makes the
            // pre-roll possible, and it keeps device-start latency — a couple of
            // hundred milliseconds on macOS — off the front of every dictation.
            let (frames_tx, frames_rx) = std::sync::mpsc::channel();
            match audio::Capture::start(frames_tx) {
                Ok(capture) => {
                    *handle.state::<AppState>().capture.lock().unwrap() = Some(capture);
                    dictation::spawn_audio_pump(handle.clone(), frames_rx);
                }
                // A refused or missing microphone must not stop the app from
                // starting: the user needs the settings window to fix it.
                Err(error) => tracing::error!(?error, "could not open the microphone"),
            }

            register_hotkey(&handle)?;

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build WeldSpeak")
        .run(|_app, event| {
            // Closing the settings window must not quit: the app lives in the
            // tray and the hotkey has to keep working.
            if let RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
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
                let _ = auth::save_refresh_token(&tokens.refresh_token);
                let state = app.state::<AppState>();
                let mut store = state.auth.lock().expect("auth store poisoned");
                store.accept(tokens);
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
    let quit_item = MenuItem::with_id(app, "quit", "Quit WeldSpeak", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings_item, &quit_item])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => {
                if let Some(window) = app.get_webview_window("settings") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// Build the global-shortcut plugin with the dictation handler attached.
///
/// The plugin reports press and release separately, which is what push-to-talk
/// needs. Its coverage of held modifier keys varies by platform — the module
/// docs in `hotkey.rs` explain why Fn in particular is not offered — so the
/// defaults are right-hand modifiers that are reliably delivered on both.
fn build_shortcut_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            let app = app.clone();
            match event.state() {
                ShortcutState::Pressed => dictation::begin(&app),
                ShortcutState::Released => dictation::end(&app),
            }
        })
        .build()
}

/// Register the configured hotkey.
fn register_hotkey(app: &AppHandle) -> tauri::Result<()> {
    let accelerator = {
        let state = app.state::<AppState>();
        let Ok(settings) = state.settings.lock() else {
            return Ok(());
        };
        settings.hotkey.accelerator.clone()
    };

    if let Err(error) = app.global_shortcut().register(accelerator.as_str()) {
        // A hotkey another application already owns is a configuration problem,
        // not a reason to refuse to start: the user needs the settings window to
        // choose a different one.
        tracing::error!(?error, %accelerator, "could not register the dictation hotkey");
        let _ = app.emit(
            "weldspeak://notice",
            format!("{accelerator} is already in use. Choose another key in Settings."),
        );
    }

    Ok(())
}

/// Escape cancels a dictation in progress.
#[allow(dead_code)]
fn escape_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::empty()), Code::Escape)
}
