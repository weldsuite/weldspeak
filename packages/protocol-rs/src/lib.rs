//! Wire protocol shared between the WeldSpeak desktop client and the Worker.
//!
//! This crate mirrors `packages/protocol` (TypeScript). The two are kept in
//! step by hand — the surface is small enough that a code generator would be
//! more machinery than it earns — so any change here needs the matching change
//! there, and vice versa.

pub mod audio;
pub mod stream;

pub use audio::{
    bytes_to_ms, BYTES_PER_SAMPLE, CHANNELS, ENCODING, FRAME_BYTES, FRAME_MS, FRAME_SAMPLES,
    PREROLL_FRAMES, PREROLL_MS, SAMPLE_RATE,
};
pub use stream::{
    decode_token_subprotocol, encode_token_subprotocol, ClientFrame, ErrorCode, ServerEvent,
    WS_SUBPROTOCOL_PREFIX,
};
