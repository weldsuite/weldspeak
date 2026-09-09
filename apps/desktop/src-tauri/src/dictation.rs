//! Driving one dictation.
//!
//! The state machine in `weldspeak-core` decides *what* should happen; this
//! module does it. Keeping the two apart is what lets the awkward timing cases
//! — a tap released before the socket opens, Escape landing mid-cleanup, a
//! result arriving after a cancel — be tested without a microphone.

use std::sync::mpsc::Receiver;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::unbounded_channel;
use weldspeak_core::{Action, Event as SessionEvent, Frame};
use weldspeak_protocol::{audio::SAMPLE_RATE, ClientFrame, ServerEvent};

use crate::transport::{self, Outbound};
use crate::AppState;

/// Start forwarding captured audio to whichever dictation is currently open.
///
/// Runs for the app's lifetime. The microphone is always recording — that is
/// what fills the pre-roll — so this thread is always draining it, and simply
/// drops frames when no dictation is in progress.
pub fn spawn_audio_pump(app: AppHandle, frames: Receiver<Frame>) {
    std::thread::Builder::new()
        .name("weldspeak-pump".into())
        .spawn(move || {
            while let Ok(frame) = frames.recv() {
                let state = app.state::<AppState>();
                let sender = state.outbound.lock().ok().and_then(|slot| slot.clone());

                if let Some(sender) = sender {
                    // A closed channel means the dictation ended between the
                    // framer emitting this frame and us forwarding it, which is
                    // ordinary rather than an error.
                    let _ = sender.send(Outbound::Audio(frame));
                }
            }
        })
        .expect("failed to start the audio pump");
}

/// Hotkey pressed: open a session.
pub fn begin(app: &AppHandle) {
    let state = app.state::<AppState>();

    let actions = {
        let Ok(mut session) = state.session.lock() else {
            return;
        };
        if !session.is_idle() {
            // Key repeat, or a user leaning on the key. Restarting here would
            // discard the utterance already in progress.
            return;
        }
        session.handle(SessionEvent::HotkeyDown)
    };

    perform(app, actions);
}

/// Hotkey released: finish and wait for the text.
pub fn end(app: &AppHandle) {
    let actions = {
        let state = app.state::<AppState>();
        let Ok(mut session) = state.session.lock() else {
            return;
        };
        session.handle(SessionEvent::HotkeyUp)
    };

    perform(app, actions);
}

/// Escape: abandon the dictation.
pub fn cancel(app: &AppHandle) {
    let actions = {
        let state = app.state::<AppState>();
        let Ok(mut session) = state.session.lock() else {
            return;
        };
        session.handle(SessionEvent::Cancel)
    };

    perform(app, actions);
}

/// Carry out what the state machine decided.
fn perform(app: &AppHandle, actions: Vec<Action>) {
    for action in actions {
        match action {
            Action::OpenSocket => open_socket(app),
            Action::StartStreaming => start_streaming(app),
            Action::SendStop => {
                send(app, Outbound::Control(ClientFrame::Stop));
                let _ = app.emit("weldspeak://thinking", ());
            }
            Action::SendCancel => {
                send(app, Outbound::Control(ClientFrame::Cancel));
                teardown(app);
            }
            Action::Inject { text } => {
                let preference = app
                    .state::<AppState>()
                    .settings
                    .lock()
                    .map(|settings| settings.injection.into())
                    .unwrap_or_default();

                crate::inject_on_main_thread(app, text, preference);

                // The injector reports completion by driving the machine on;
                // without this the session would never return to idle.
                let state = app.state::<AppState>();
                let actions = state
                    .session
                    .lock()
                    .map(|mut session| session.handle(SessionEvent::Injected))
                    .unwrap_or_default();
                perform(app, actions);
            }
            Action::Teardown => teardown(app),
            Action::Notify { message } => {
                let _ = app.emit("weldspeak://notice", message);
            }
        }
    }
}

fn open_socket(app: &AppHandle) {
    let state = app.state::<AppState>();

    let (api_base, org_id, locale, format) = {
        let Ok(settings) = state.settings.lock() else {
            return;
        };
        (
            settings.api_base.clone(),
            settings.org_id.clone(),
            settings.locale.clone(),
            settings.clean_up_text,
        )
    };

    let Some(access_token) = state
        .auth
        .lock()
        .ok()
        .and_then(|auth| auth.access_token(weldspeak_core::auth::now_secs()).map(str::to_owned))
    else {
        fail(app, "Sign in to WeldSpeak before dictating.");
        return;
    };

    let (outbound_tx, outbound_rx) = unbounded_channel();
    let (events_tx, mut events_rx) = unbounded_channel();

    if let Ok(mut slot) = state.outbound.lock() {
        *slot = Some(outbound_tx.clone());
    }

    // The `start` frame goes out immediately so the server can open its upstream
    // connection while the user is still drawing breath.
    let _ = outbound_tx.send(Outbound::Control(ClientFrame::Start {
        sample_rate: SAMPLE_RATE,
        encoding: weldspeak_protocol::ENCODING.into(),
        locale,
        org_id: org_id.clone(),
        // The server merges the org glossary itself; sending terms from here
        // would let a client probe another org's vocabulary by guessing.
        keyterms: None,
        app_name: None,
        format: Some(format),
    }));

    let for_events = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events_rx.recv().await {
            handle_server_event(&for_events, event);
        }
    });

    let for_socket = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = transport::run(
            &api_base,
            &access_token,
            org_id.as_deref(),
            outbound_rx,
            events_tx,
        )
        .await
        {
            tracing::error!(?error, "dictation socket failed");
            fail(&for_socket, "WeldSpeak lost its connection. Try again.");
        }
    });
}

fn start_streaming(app: &AppHandle) {
    let state = app.state::<AppState>();

    // Arming hands back the pre-roll: the audio from just before the key went
    // down, which is where the first syllable lives.
    let preroll = state
        .capture
        .lock()
        .ok()
        .and_then(|capture| capture.as_ref().map(|capture| capture.arm()))
        .unwrap_or_default();

    for frame in preroll {
        send(app, Outbound::Audio(frame));
    }

    let _ = app.emit("weldspeak://listening", ());
}

fn handle_server_event(app: &AppHandle, event: ServerEvent) {
    let actions = {
        let state = app.state::<AppState>();
        let Ok(mut session) = state.session.lock() else {
            return;
        };

        match event {
            ServerEvent::Ready { .. } => session.handle(SessionEvent::Ready),

            ServerEvent::Partial { text } => {
                // Display only. Partials are revised as the recognizer gets more
                // context; injecting one would type a word the user did not say.
                let _ = app.emit("weldspeak://partial", text);
                Vec::new()
            }

            ServerEvent::Transcript { .. } | ServerEvent::Pong => Vec::new(),

            ServerEvent::Result { text, .. } => session.handle(SessionEvent::Result { text }),

            ServerEvent::Error { code, message, .. } => {
                session.handle(SessionEvent::Failed { code, message })
            }
        }
    };

    perform(app, actions);
}

fn send(app: &AppHandle, message: Outbound) {
    let state = app.state::<AppState>();

    // Clone the sender out and release the lock before sending: holding a mutex
    // across a send invites a deadlock the first time a receiver runs on this
    // same thread.
    let sender = state.outbound.lock().ok().and_then(|slot| slot.clone());

    if let Some(sender) = sender {
        let _ = sender.send(message);
    }
}

/// Close the socket, stop streaming, and return the overlay to rest.
fn teardown(app: &AppHandle) {
    let state = app.state::<AppState>();

    if let Ok(capture) = state.capture.lock() {
        if let Some(capture) = capture.as_ref() {
            capture.disarm();
        }
    }

    if let Ok(mut outbound) = state.outbound.lock() {
        if let Some(sender) = outbound.take() {
            let _ = sender.send(Outbound::Close);
        }
    }

    let _ = app.emit("weldspeak://done", ());
}

/// Drive the session into its failed state and tell the user.
fn fail(app: &AppHandle, message: &str) {
    let actions = {
        let state = app.state::<AppState>();
        let Ok(mut session) = state.session.lock() else {
            return;
        };
        session.handle(SessionEvent::Failed {
            code: weldspeak_protocol::ErrorCode::Internal,
            message: message.to_string(),
        })
    };

    perform(app, actions);
}
