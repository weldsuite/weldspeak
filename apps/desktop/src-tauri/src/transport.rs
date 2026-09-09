//! The dictation WebSocket.
//!
//! Opened on hotkey-*down*, not on the first audio frame. TLS negotiation and
//! waking the Durable Object take a few hundred milliseconds, and doing that
//! while the user is drawing breath costs nothing — doing it after they stop
//! speaking would be most of the latency budget.

use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use weldspeak_protocol::{encode_token_subprotocol, ClientFrame, ServerEvent};

/// Messages the session sends into the transport.
pub enum Outbound {
    Control(ClientFrame),
    Audio(Vec<u8>),
    Close,
}

/// Open a dictation session and pump it until it ends.
///
/// Returns once the socket closes. Server events are forwarded to `events`; the
/// caller drives the state machine with them.
pub async fn run(
    api_base: &str,
    access_token: &str,
    org_id: Option<&str>,
    mut outbound: UnboundedReceiver<Outbound>,
    events: UnboundedSender<ServerEvent>,
) -> Result<()> {
    let url = url_for(api_base, org_id)?;

    let mut request = url.as_str().into_client_request()?;
    // The token travels as a subprotocol rather than a header. A Rust client
    // could set a header, but keeping to the subprotocol means a future browser
    // client — which cannot set headers on a WebSocket handshake — needs no
    // second code path on the server.
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_str(&encode_token_subprotocol(access_token))?,
    );

    let (socket, _) = tokio_tungstenite::connect_async(request).await?;
    let (mut sink, mut stream) = socket.split();

    loop {
        tokio::select! {
            message = outbound.recv() => match message {
                Some(Outbound::Control(frame)) => {
                    sink.send(Message::Text(serde_json::to_string(&frame)?)).await?;
                }
                Some(Outbound::Audio(bytes)) => {
                    sink.send(Message::Binary(bytes)).await?;
                }
                // Sender dropped or an explicit close: shut down cleanly so the
                // server can release the Durable Object rather than waiting for
                // a timeout.
                Some(Outbound::Close) | None => {
                    let _ = sink.send(Message::Close(None)).await;
                    return Ok(());
                }
            },

            message = stream.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<ServerEvent>(&text) {
                        Ok(event) => {
                            let terminal = matches!(
                                event,
                                ServerEvent::Result { .. } | ServerEvent::Error { .. }
                            );
                            if events.send(event).is_err() || terminal {
                                return Ok(());
                            }
                        }
                        // An unrecognised event is not fatal: the server may be
                        // newer than this client, and dropping it is safer than
                        // abandoning a dictation in progress.
                        Err(error) => tracing::warn!(?error, %text, "unrecognised server event"),
                    }
                }
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.into()),
            },
        }
    }
}

/// Build the stream URL, converting the API's scheme to its WebSocket form.
fn url_for(api_base: &str, org_id: Option<&str>) -> Result<url::Url> {
    let base = api_base
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);

    let mut url = url::Url::parse(&format!("{}/v1/stream", base.trim_end_matches('/')))
        .map_err(|error| anyhow!("invalid API base URL {api_base}: {error}"))?;

    if let Some(org_id) = org_id {
        url.query_pairs_mut().append_pair("org", org_id);
    }

    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrades_the_scheme_to_websocket() {
        assert_eq!(
            url_for("https://api.weldspeak.io", None).unwrap().as_str(),
            "wss://api.weldspeak.io/v1/stream",
        );
        assert_eq!(
            url_for("http://localhost:8787", None).unwrap().as_str(),
            "ws://localhost:8787/v1/stream",
        );
    }

    #[test]
    fn carries_the_active_organization() {
        let url = url_for("https://api.weldspeak.io", Some("org_acme")).unwrap();
        assert_eq!(url.query(), Some("org=org_acme"));
    }

    #[test]
    fn tolerates_a_trailing_slash() {
        assert_eq!(
            url_for("https://api.weldspeak.io/", None).unwrap().path(),
            "/v1/stream",
        );
    }
}
