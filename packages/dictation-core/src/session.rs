//! The dictation state machine.
//!
//! One utterance, from hotkey-down to injected text. Modelling it explicitly
//! rather than as a scattering of booleans is what keeps the awkward cases
//! honest — and they are all timing races:
//!
//!   - A quick tap releases the hotkey before the server has said `ready`, so
//!     the stop has to be remembered and sent once it arrives.
//!   - Escape can land at any point, including while the cleanup pass is
//!     already running, and must not inject text afterwards.
//!   - A `result` can arrive after a cancel, and must be dropped rather than
//!     typed into whatever the user has since clicked on.
//!
//! Each of those is a bug that only shows up under a fast user, which is to say
//! under every user eventually.

use weldspeak_protocol::ErrorCode;

/// Where a dictation currently is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum State {
    /// Nothing happening. The microphone is still open, filling the pre-roll.
    #[default]
    Idle,
    /// Hotkey down; opening the socket and waiting for `ready`.
    Arming {
        /// The user already let go. Send `stop` the moment the server is ready.
        stop_pending: bool,
    },
    /// Streaming audio.
    Recording,
    /// Hotkey released; waiting for the transcript and cleanup.
    Finalizing,
    /// Text in hand, being injected into the focused application.
    Injecting,
    /// Terminal failure. Carries what to tell the user.
    Failed { code: ErrorCode, message: String },
}

/// Something that happened, from the user, the network, or the injector.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    HotkeyDown,
    HotkeyUp,
    /// The server accepted the session.
    Ready,
    /// A final transcript and cleaned text arrived.
    Result {
        text: String,
    },
    /// The user pressed Escape.
    Cancel,
    /// The injector finished.
    Injected,
    Failed {
        code: ErrorCode,
        message: String,
    },
}

/// What the caller should do about a transition.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Open the WebSocket and send the `start` frame.
    OpenSocket,
    /// Begin handing captured frames to the transport.
    StartStreaming,
    /// Send the `stop` frame.
    SendStop,
    /// Send `cancel` and close.
    SendCancel,
    /// Type this text into the focused application.
    Inject { text: String },
    /// Close the socket and return to idle.
    Teardown,
    /// Show a message to the user.
    Notify { message: String },
}

/// The state machine. Deliberately owns no I/O — it decides, the caller acts.
#[derive(Debug, Default)]
pub struct Session {
    state: State,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Whether audio frames should currently be sent.
    ///
    /// True only while recording: frames captured during `Arming` are held by
    /// the framer, and anything after `stop` would arrive past the end of the
    /// utterance.
    pub fn is_streaming(&self) -> bool {
        matches!(self.state, State::Recording)
    }

    /// Whether a new dictation can start.
    pub fn is_idle(&self) -> bool {
        matches!(self.state, State::Idle | State::Failed { .. })
    }

    /// Apply an event and return the actions it implies.
    pub fn handle(&mut self, event: Event) -> Vec<Action> {
        match (&self.state, event) {
            // --- starting ---
            (State::Idle | State::Failed { .. }, Event::HotkeyDown) => {
                self.state = State::Arming {
                    stop_pending: false,
                };
                vec![Action::OpenSocket]
            }

            (State::Arming { stop_pending }, Event::Ready) => {
                if *stop_pending {
                    // The user tapped and released faster than the socket came
                    // up. The pre-roll holds what they said, so finalize at once
                    // rather than dropping the utterance.
                    self.state = State::Finalizing;
                    vec![Action::StartStreaming, Action::SendStop]
                } else {
                    self.state = State::Recording;
                    vec![Action::StartStreaming]
                }
            }

            // Released before the server was ready: remember it.
            (State::Arming { .. }, Event::HotkeyUp) => {
                self.state = State::Arming { stop_pending: true };
                vec![]
            }

            // --- finishing ---
            (State::Recording, Event::HotkeyUp) => {
                self.state = State::Finalizing;
                vec![Action::SendStop]
            }

            (State::Finalizing, Event::Result { text }) => {
                if text.trim().is_empty() {
                    // Silence, or a hotkey brushed by accident. Injecting an
                    // empty string is pointless; saying nothing is correct.
                    self.state = State::Idle;
                    vec![Action::Teardown]
                } else {
                    self.state = State::Injecting;
                    vec![Action::Inject { text }]
                }
            }

            (State::Injecting, Event::Injected) => {
                self.state = State::Idle;
                vec![Action::Teardown]
            }

            // --- cancelling ---
            (State::Arming { .. } | State::Recording | State::Finalizing, Event::Cancel) => {
                self.state = State::Idle;
                vec![Action::SendCancel, Action::Teardown]
            }

            // Too late: the text is already going in. Cancelling here would
            // leave half a sentence in the document, which is worse than the
            // whole one. The user has undo.
            (State::Injecting, Event::Cancel) => vec![],

            // --- failure ---
            (_, Event::Failed { code, message }) => {
                let notify = message.clone();
                self.state = State::Failed { code, message };
                vec![Action::Teardown, Action::Notify { message: notify }]
            }

            // --- events that no longer apply ---
            //
            // A `result` arriving after a cancel is the dangerous one: acting on
            // it would type into whatever the user moved on to. Everything that
            // does not match a transition above is dropped.
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(text: &str) -> Event {
        Event::Result { text: text.into() }
    }

    /// Drive a session through a normal dictation.
    fn recording_session() -> Session {
        let mut session = Session::new();
        session.handle(Event::HotkeyDown);
        session.handle(Event::Ready);
        session
    }

    #[test]
    fn a_normal_dictation_runs_start_to_finish() {
        let mut session = Session::new();

        assert_eq!(session.handle(Event::HotkeyDown), vec![Action::OpenSocket]);
        assert_eq!(session.handle(Event::Ready), vec![Action::StartStreaming]);
        assert!(session.is_streaming());

        assert_eq!(session.handle(Event::HotkeyUp), vec![Action::SendStop]);
        assert!(!session.is_streaming());

        assert_eq!(
            session.handle(result("The weld looks good.")),
            vec![Action::Inject {
                text: "The weld looks good.".into()
            }],
        );

        assert_eq!(session.handle(Event::Injected), vec![Action::Teardown]);
        assert!(session.is_idle());
    }

    #[test]
    fn a_quick_tap_still_produces_a_dictation() {
        // Released before the socket came up. The pre-roll holds what was said,
        // so this must finalize rather than silently drop the utterance.
        let mut session = Session::new();

        session.handle(Event::HotkeyDown);
        assert_eq!(session.handle(Event::HotkeyUp), vec![]);
        assert_eq!(session.state(), &State::Arming { stop_pending: true });

        assert_eq!(
            session.handle(Event::Ready),
            vec![Action::StartStreaming, Action::SendStop],
        );
        assert_eq!(session.state(), &State::Finalizing);

        assert_eq!(
            session.handle(result("quick note")),
            vec![Action::Inject {
                text: "quick note".into()
            }],
        );
    }

    #[test]
    fn escape_while_recording_sends_a_cancel() {
        let mut session = recording_session();

        assert_eq!(
            session.handle(Event::Cancel),
            vec![Action::SendCancel, Action::Teardown],
        );
        assert!(session.is_idle());
    }

    #[test]
    fn escape_while_finalizing_still_cancels() {
        let mut session = recording_session();
        session.handle(Event::HotkeyUp);

        assert_eq!(
            session.handle(Event::Cancel),
            vec![Action::SendCancel, Action::Teardown],
        );
    }

    #[test]
    fn a_result_arriving_after_a_cancel_is_never_injected() {
        // The response was already in flight when the user hit Escape. Typing it
        // now would put a sentence into whatever they moved on to.
        let mut session = recording_session();
        session.handle(Event::HotkeyUp);
        session.handle(Event::Cancel);

        assert_eq!(session.handle(result("should not appear")), vec![]);
        assert!(session.is_idle());
    }

    #[test]
    fn escape_during_injection_is_ignored() {
        // Half a sentence in the document is worse than the whole one, and the
        // user still has undo.
        let mut session = recording_session();
        session.handle(Event::HotkeyUp);
        session.handle(result("already going in"));

        assert_eq!(session.handle(Event::Cancel), vec![]);
        assert_eq!(session.state(), &State::Injecting);
    }

    #[test]
    fn an_empty_result_injects_nothing() {
        for empty in ["", "   ", "\n"] {
            let mut session = recording_session();
            session.handle(Event::HotkeyUp);

            assert_eq!(session.handle(result(empty)), vec![Action::Teardown]);
            assert!(session.is_idle());
        }
    }

    #[test]
    fn audio_is_only_streamed_while_recording() {
        let mut session = Session::new();
        assert!(!session.is_streaming());

        session.handle(Event::HotkeyDown);
        assert!(!session.is_streaming(), "not until the server is ready");

        session.handle(Event::Ready);
        assert!(session.is_streaming());

        session.handle(Event::HotkeyUp);
        assert!(!session.is_streaming(), "not after stop");
    }

    #[test]
    fn a_failure_reports_and_returns_to_a_usable_state() {
        let mut session = recording_session();

        let actions = session.handle(Event::Failed {
            code: ErrorCode::QuotaExceeded,
            message: "Monthly limit reached".into(),
        });

        assert_eq!(
            actions,
            vec![
                Action::Teardown,
                Action::Notify {
                    message: "Monthly limit reached".into()
                },
            ],
        );

        // Failed is terminal for this utterance but must not wedge the app.
        assert!(session.is_idle());
        assert_eq!(session.handle(Event::HotkeyDown), vec![Action::OpenSocket]);
    }

    #[test]
    fn a_second_hotkey_press_mid_dictation_is_ignored() {
        // Key repeat, or a user leaning on the key. Restarting here would drop
        // the utterance already in progress.
        let mut session = recording_session();

        assert_eq!(session.handle(Event::HotkeyDown), vec![]);
        assert_eq!(session.state(), &State::Recording);
    }

    #[test]
    fn a_stray_hotkey_release_while_idle_does_nothing() {
        let mut session = Session::new();
        assert_eq!(session.handle(Event::HotkeyUp), vec![]);
        assert!(session.is_idle());
    }

    #[test]
    fn a_late_ready_after_cancel_is_ignored() {
        let mut session = Session::new();
        session.handle(Event::HotkeyDown);
        session.handle(Event::Cancel);

        assert_eq!(session.handle(Event::Ready), vec![]);
        assert!(session.is_idle());
    }
}
