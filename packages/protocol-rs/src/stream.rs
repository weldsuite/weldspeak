//! The dictation WebSocket protocol.
//!
//! Mirrors `packages/protocol/src/stream.ts`. One socket carries JSON text
//! frames for control and binary frames for raw `linear16` PCM. Binary frames
//! are only meaningful between a `start` and a `stop`/`cancel`.

use serde::{Deserialize, Serialize};

/// What surrounds the cursor in the field being dictated into.
///
/// Mirrors `FieldContext` in stream.ts. Cleanup uses it to continue a
/// sentence and spell names already on screen; the server never stores it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FieldContext {
    /// Text just before the cursor, at most [`MAX_CONTEXT_BEFORE`] characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Text just after the cursor, at most [`MAX_CONTEXT_AFTER`] characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Focused window title; in a browser it names the site.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_title: Option<String>,
}

pub const MAX_CONTEXT_BEFORE: usize = 1_500;
pub const MAX_CONTEXT_AFTER: usize = 500;
pub const MAX_WINDOW_TITLE: usize = 200;

impl FieldContext {
    /// Keep the text nearest the cursor and drop empty parts. `None` when
    /// nothing useful is left, so the frame carries no empty object.
    pub fn trimmed(self) -> Option<Self> {
        fn keep(text: Option<String>) -> Option<String> {
            text.filter(|text| !text.trim().is_empty())
        }
        let before = keep(self.before).map(|text| last_chars(&text, MAX_CONTEXT_BEFORE));
        let after = keep(self.after).map(|text| text.chars().take(MAX_CONTEXT_AFTER).collect());
        let window_title = keep(self.window_title)
            .map(|text| text.trim().chars().take(MAX_WINDOW_TITLE).collect());
        if before.is_none() && after.is_none() && window_title.is_none() {
            return None;
        }
        Some(Self {
            before,
            after,
            window_title,
        })
    }
}

fn last_chars(text: &str, count: usize) -> String {
    let total = text.chars().count();
    text.chars().skip(total.saturating_sub(count)).collect()
}

/// Frames sent by the desktop client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientFrame {
    /// Opens a dictation session. Must precede any audio.
    #[serde(rename = "start", rename_all = "camelCase")]
    Start {
        sample_rate: u32,
        encoding: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        locale: Option<String>,
        /// Active organization. The server re-verifies membership; a
        /// client-supplied value is never trusted on its own.
        #[serde(skip_serializing_if = "Option::is_none")]
        org_id: Option<String>,
        /// Vocabulary boosts: personal terms merged with the org glossary.
        #[serde(skip_serializing_if = "Option::is_none")]
        keyterms: Option<Vec<String>>,
        /// Focused application, for history and diagnostics only.
        #[serde(skip_serializing_if = "Option::is_none")]
        app_name: Option<String>,
        /// Run the cleanup pass. When false the raw transcript is returned.
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<bool>,
        /// Store the transcript in history. Only `false` changes anything:
        /// the server already defaults to keeping it unless the org says not.
        #[serde(skip_serializing_if = "Option::is_none")]
        retain: Option<bool>,
    },
    /// Sent on hotkey release. The server finalizes and replies `result`.
    #[serde(rename = "stop")]
    Stop {
        /// Cursor context captured at hotkey-down, for cleanup only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context: Option<FieldContext>,
    },
    /// Sent on Escape. The server discards the utterance; no `result` follows.
    #[serde(rename = "cancel")]
    Cancel,
    #[serde(rename = "ping")]
    Ping,
}

/// Why a session failed. `retryable` on [`ServerEvent::Error`] says whether a
/// retry is worth attempting.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    OrgForbidden,
    QuotaExceeded,
    BadRequest,
    UpstreamFailed,
    Internal,
}

/// Events sent by the Worker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerEvent {
    /// The `start` was accepted; the server is ready for audio.
    #[serde(rename = "ready", rename_all = "camelCase")]
    Ready { session_id: String },

    /// Interim result. Replaces the previous partial rather than appending.
    /// Display-only — never inject a partial, it will be revised.
    #[serde(rename = "partial")]
    Partial { text: String },

    /// The recognizer's final, before cleanup.
    #[serde(rename = "transcript")]
    Transcript { text: String },

    /// Terminal success. `text` is what gets injected.
    #[serde(rename = "result", rename_all = "camelCase")]
    Result {
        text: String,
        raw: String,
        /// False when cleanup was disabled, failed, or missed its deadline.
        formatted: bool,
        duration_ms: u64,
    },

    /// Terminal failure.
    #[serde(rename = "error")]
    Error {
        code: ErrorCode,
        message: String,
        retryable: bool,
    },

    #[serde(rename = "pong")]
    Pong,
}

/// The JWT travels as a WebSocket subprotocol rather than a header.
///
/// Browsers cannot set headers on a WebSocket handshake. This client could,
/// but using the subprotocol keeps a future browser client possible without a
/// second auth path on the server.
pub const WS_SUBPROTOCOL_PREFIX: &str = "weldspeak.token.";

/// Build the subprotocol value carrying `token`.
pub fn encode_token_subprotocol(token: &str) -> String {
    format!("{WS_SUBPROTOCOL_PREFIX}{token}")
}

/// Recover a token from a `Sec-WebSocket-Protocol` list, if present.
pub fn decode_token_subprotocol(header: &str) -> Option<&str> {
    header.split(',').find_map(|raw| {
        raw.trim()
            .strip_prefix(WS_SUBPROTOCOL_PREFIX)
            .filter(|token| !token.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_frame_serializes_to_the_typescript_shape() {
        let frame = ClientFrame::Start {
            sample_rate: 16_000,
            encoding: "linear16".into(),
            locale: Some("en".into()),
            org_id: Some("org_123".into()),
            keyterms: Some(vec!["Inconel 625".into()]),
            app_name: None,
            format: Some(true),
            retain: Some(false),
        };

        let json: serde_json::Value = serde_json::to_value(&frame).unwrap();
        assert_eq!(json["type"], "start");
        assert_eq!(json["sampleRate"], 16_000);
        assert_eq!(json["orgId"], "org_123");
        assert_eq!(json["keyterms"][0], "Inconel 625");
        assert_eq!(json["retain"], false);
        // Absent optionals are omitted rather than sent as null.
        assert!(json.get("appName").is_none());
    }

    #[test]
    fn unit_frames_carry_only_a_type() {
        assert_eq!(
            serde_json::to_string(&ClientFrame::Stop { context: None }).unwrap(),
            r#"{"type":"stop"}"#
        );
    }

    #[test]
    fn stop_carries_cursor_context_in_the_typescript_shape() {
        let frame = ClientFrame::Stop {
            context: FieldContext {
                before: Some("Hi Aysha, ".into()),
                after: None,
                window_title: Some("Inbox - Gmail".into()),
            }
            .trimmed(),
        };
        assert_eq!(
            serde_json::to_string(&frame).unwrap(),
            r#"{"type":"stop","context":{"before":"Hi Aysha, ","windowTitle":"Inbox - Gmail"}}"#
        );
        let parsed: ClientFrame = serde_json::from_str(r#"{"type":"stop"}"#).unwrap();
        assert_eq!(parsed, ClientFrame::Stop { context: None });
    }

    #[test]
    fn context_keeps_the_text_nearest_the_cursor() {
        let context = FieldContext {
            before: Some(format!("{}END", "x".repeat(MAX_CONTEXT_BEFORE))),
            after: Some(format!("START{}", "y".repeat(MAX_CONTEXT_AFTER))),
            window_title: Some("   ".into()),
        }
        .trimmed()
        .unwrap();
        assert!(context.before.as_deref().unwrap().ends_with("END"));
        assert_eq!(context.before.unwrap().chars().count(), MAX_CONTEXT_BEFORE);
        assert!(context.after.as_deref().unwrap().starts_with("START"));
        assert_eq!(context.window_title, None);
        assert_eq!(FieldContext::default().trimmed(), None);
    }

    #[test]
    fn server_events_deserialize_from_the_typescript_shape() {
        let event: ServerEvent = serde_json::from_str(
            r#"{"type":"result","text":"Hello.","raw":"hello","formatted":true,"durationMs":1200}"#,
        )
        .unwrap();

        match event {
            ServerEvent::Result {
                text,
                formatted,
                duration_ms,
                ..
            } => {
                assert_eq!(text, "Hello.");
                assert!(formatted);
                assert_eq!(duration_ms, 1200);
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[test]
    fn error_codes_use_snake_case_on_the_wire() {
        let event: ServerEvent = serde_json::from_str(
            r#"{"type":"error","code":"quota_exceeded","message":"cap reached","retryable":false}"#,
        )
        .unwrap();

        match event {
            ServerEvent::Error { code, .. } => assert_eq!(code, ErrorCode::QuotaExceeded),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn token_survives_a_subprotocol_round_trip() {
        let encoded = encode_token_subprotocol("abc.def.ghi");
        assert_eq!(decode_token_subprotocol(&encoded), Some("abc.def.ghi"));
    }

    #[test]
    fn token_is_found_among_other_offered_subprotocols() {
        let header = format!("chat, {}, superchat", encode_token_subprotocol("tok"));
        assert_eq!(decode_token_subprotocol(&header), Some("tok"));
    }

    #[test]
    fn missing_or_empty_token_decodes_to_none() {
        assert_eq!(decode_token_subprotocol("chat, superchat"), None);
        assert_eq!(decode_token_subprotocol("weldspeak.token."), None);
    }
}
