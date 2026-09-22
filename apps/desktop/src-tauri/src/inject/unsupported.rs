//! Stub for platforms WeldSpeak does not target.
//!
//! Linux is not a supported target, but keeping the crate compiling there means
//! `cargo check` and clippy work on a developer's machine without a Mac or a
//! Windows box, which is worth the few lines.

use anyhow::{bail, Result};

pub fn can_synthesise_input() -> bool {
    false
}

pub fn open_permission_settings() -> Result<()> {
    Ok(())
}

pub fn type_text(_text: &str) -> Result<()> {
    bail!("text injection is not implemented on this platform")
}

pub fn send_paste_shortcut() -> Result<()> {
    bail!("text injection is not implemented on this platform")
}

pub fn focused_text() -> Option<String> {
    None
}

pub fn focused_app_name() -> Option<String> {
    None
}

pub fn focused_context() -> Option<weldspeak_protocol::stream::FieldContext> {
    None
}
