//! Platform-independent dictation logic.
//!
//! Everything here compiles and is tested on any host, which is the point: the
//! parts of a dictation client most likely to be subtly wrong — sample-rate
//! conversion, frame timing, the pre-roll buffer, the session state machine —
//! are exactly the parts that would otherwise only be exercisable by speaking
//! into a Mac.
//!
//! The Tauri crate in `apps/desktop/src-tauri` supplies what genuinely cannot
//! be portable: microphone access, global hotkeys, and text injection.

pub mod auth;
pub mod frame;
pub mod inject;
pub mod learn;
pub mod resample;
pub mod session;

pub use auth::{AuthState, AuthStore, Tokens};
pub use learn::Correction;
pub use frame::{Frame, Framer};
pub use inject::{plan, plan_or_clipboard_only, Fallback, Method, Plan, Preference};
pub use resample::Resampler;
pub use session::{Action, Event, Session, State};
