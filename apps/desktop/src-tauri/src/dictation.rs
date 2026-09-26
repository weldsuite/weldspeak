//! Driving one dictation.
//!
//! The state machine in `weldspeak-core` decides *what* should happen; this
//! module does it. Keeping the two apart is what lets the awkward timing cases
//! — a tap released before the socket opens, Escape landing mid-cleanup, a
//! result arriving after a cancel — be tested without a microphone.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;
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

/// Keep recording this long after the key comes up.
///
/// People let go as the last syllable is still leaving their mouth, and a few
/// tens of milliseconds more sit in the device buffer and the resampler.
/// Sending `stop` on the instant of release cut that tail off — the most
/// common way a dictation lost its last word.
pub const RELEASE_TAIL: Duration = Duration::from_millis(250);

/// Bumped by every release, press and cancel so a stale deferred stop can tell
/// it has been overtaken.
static STOP_GENERATION: AtomicU64 = AtomicU64::new(0);
/// A release is waiting out its tail; the next press continues the dictation.
static STOP_PENDING: AtomicBool = AtomicBool::new(false);
/// Bumped per dictation so a slow context read cannot land on the next one.
static CONTEXT_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Hotkey pressed: open a session.
pub fn begin(app: &AppHandle) {
    // Pressed again while the previous release was still in its tail: this is
    // the same dictation continuing (a double-tap into hands-free, or a pause
    // mid-thought). Drop the pending stop and keep streaming.
    let state = app.state::<AppState>();
    if STOP_PENDING.swap(false, Ordering::SeqCst) {
        STOP_GENERATION.fetch_add(1, Ordering::SeqCst);
        let continuing = state
            .session
            .lock()
            .map(|session| !session.is_idle())
            .unwrap_or(false);
        // If the session failed during the tail there is nothing to continue;
        // fall through and start a fresh one.
        if continuing {
            return;
        }
    }

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

    // Retain speech from this instant, before anything slow runs. Pausing media
    // goes through the OS media session and can take long enough for the first
    // words to age out of the idle pre-roll.
    if let Ok(capture) = state.capture.lock() {
        if let Some(capture) = capture.as_ref() {
            capture.hold();
        }
    }
    crate::learn::invalidate();
    // Read what is around the cursor now, while the target field still has
    // focus and before the dictation changes it.
    capture_context(app);
    // Show the pill before the socket is up. Without this, a held key looks
    // like nothing happened — the Wispr Flow complaint.
    crate::overlay::appear_listening(app);
    crate::media::pause_if_enabled(app);
    perform(app, actions);
}

/// Read the cursor context in the background and park it for `stop`.
///
/// Wispr Flow skips context it cannot read quickly, and so does this: the read
/// never delays the dictation, and one that finishes after the key is released
/// is dropped.
fn capture_context(app: &AppHandle) {
    let generation = CONTEXT_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let state = app.state::<AppState>();
    if let Ok(mut slot) = state.field_context.lock() {
        *slot = None;
    }
    let enabled = state
        .settings
        .lock()
        .map(|settings| settings.use_context)
        .unwrap_or(false);
    if !enabled {
        return;
    }

    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("weldspeak-context".into())
        .spawn(move || {
            let context = read_context(&app);
            if CONTEXT_GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            if let Ok(mut slot) = app.state::<AppState>().field_context.lock() {
                *slot = context;
            }
        });
}

fn read_context(app: &AppHandle) -> Option<weldspeak_protocol::FieldContext> {
    // Accessibility is read on the main thread on macOS, as everywhere else in
    // this app; UI Automation on Windows must stay off the UI thread.
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = app.run_on_main_thread(move || {
            let _ = tx.send(crate::inject::focused_context());
        });
        rx.recv_timeout(Duration::from_millis(500)).ok().flatten()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        crate::inject::focused_context()
    }
}

/// Hotkey released: keep listening for `tail`, then finish and wait for the
/// text.
pub fn end(app: &AppHandle, tail: Duration) {
    let generation = STOP_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    STOP_PENDING.store(true, Ordering::SeqCst);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(tail).await;
        let for_main = app.clone();
        let _ = app.run_on_main_thread(move || {
            if STOP_GENERATION.load(Ordering::SeqCst) != generation
                || !STOP_PENDING.swap(false, Ordering::SeqCst)
            {
                return;
            }
            finish(&for_main);
        });
    });
}

fn finish(app: &AppHandle) {
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
    STOP_PENDING.store(false, Ordering::SeqCst);
    STOP_GENERATION.fetch_add(1, Ordering::SeqCst);
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
                // Whatever the context read produced by now goes with the stop;
                // a read still stuck on a slow app is simply left out.
                let context = app
                    .state::<AppState>()
                    .field_context
                    .lock()
                    .ok()
                    .and_then(|mut slot| slot.take());
                send(app, Outbound::Control(ClientFrame::Stop { context }));
                let _ = app.emit("weldspeak://thinking", ());
                crate::overlay::appear_thinking(app);
            }
            Action::SendCancel => {
                send(app, Outbound::Control(ClientFrame::Cancel));
                teardown(app);
            }
            Action::Inject { text } => {
                let (preference, snippets, corrections) = {
                    let state = app.state::<AppState>();
                    let settings = state.settings.lock().ok();
                    let preference = settings
                        .as_ref()
                        .map(|settings| settings.injection.into())
                        .unwrap_or_default();
                    let snippets = settings
                        .as_ref()
                        .map(|settings| settings.snippets.clone())
                        .unwrap_or_default();
                    let corrections = settings
                        .map(|settings| settings.corrections.clone())
                        .unwrap_or_default();
                    (preference, snippets, corrections)
                };
                let text = crate::snippets::expand(&text, &snippets);
                let text = crate::learn::apply(&text, &corrections);
                remember_transcript(app, &text);
                crate::native_settings::on_history_changed(app, &text);
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
                crate::overlay::show_notice(app, &message);
            }
        }
    }
}

fn open_socket(app: &AppHandle) {
    let state = app.state::<AppState>();

    let (api_base, org_id, locale, format, keep_history) = {
        let Ok(settings) = state.settings.lock() else {
            return;
        };
        (
            settings.api_base.clone(),
            settings.org_id.clone(),
            settings.locale.clone(),
            settings.clean_up_text,
            settings.keep_history,
        )
    };

    let Some(access_token) = state.auth.lock().ok().and_then(|auth| {
        auth.access_token(weldspeak_core::auth::now_secs())
            .map(str::to_owned)
    }) else {
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
        // Read now, on hotkey-down: the app in front is the one the text is
        // going into, and cleanup styles a prompt differently from an email.
        app_name: crate::inject::focused_app_name(),
        format: Some(format),
        // "Keep my dictations" off: ask the server not to store this one.
        retain: Some(keep_history),
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

    // Arming hands back everything held since hotkey-down (idle lead-in plus
    // speech spoken while waiting for `ready`). It is queued before any live
    // frame can be, so the recognizer hears the utterance in order.
    let sender = state.outbound.lock().ok().and_then(|slot| slot.clone());
    if let Ok(capture) = state.capture.lock() {
        if let Some(capture) = capture.as_ref() {
            capture.arm_with(|frame| {
                if let Some(sender) = &sender {
                    let _ = sender.send(Outbound::Audio(frame));
                }
            });
        }
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

            ServerEvent::Partial { .. } => {
                // Wispr-style: the pill is waveform only. Partials are never
                // shown — they would stretch the capsule as the user talks.
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

    crate::overlay::dismiss(app);
    crate::media::resume(app);
    if state.mic_dirty.load(std::sync::atomic::Ordering::SeqCst) {
        crate::reopen_microphone(app);
    }
}

fn remember_transcript(app: &AppHandle, text: &str) {
    let state = app.state::<AppState>();
    if let Ok(mut last) = state.last_transcript.lock() {
        *last = Some(text.to_string());
    }
    let words = text.split_whitespace().count() as u64;
    let path = crate::settings::path_for(app).ok();
    if let Ok(mut settings) = state.settings.lock() {
        settings.words_dictated = settings.words_dictated.saturating_add(words);
        if let Some(path) = path {
            let _ = settings.save(&path);
        }
    };
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
