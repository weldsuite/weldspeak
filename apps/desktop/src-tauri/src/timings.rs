//! Where the time goes in one dictation.
//!
//! "It felt slow" is not something anyone can act on. The server reports its
//! own stages on `result`; this records the client's side — the key going
//! down, the socket opening, `ready`, the release, `stop`, the result, the
//! text landing — and logs both together as one line once the text is in, so
//! a slow dictation can be pinned on the network, the recognizer, cleanup, or
//! injection without a debugger attached.
//!
//! One dictation runs at a time, so one record is enough. A generation number
//! taken at hotkey-down keeps a late event from an abandoned dictation — a
//! socket still connecting, a result after a cancel — off the next one's
//! record.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

static GENERATION: AtomicU64 = AtomicU64::new(0);
static CURRENT: Mutex<Option<Marks>> = Mutex::new(None);

/// A moment in a dictation after hotkey-down.
#[derive(Debug, Clone, Copy)]
pub enum Stage {
    /// The WebSocket handshake completed.
    Connected,
    /// The server said `ready`.
    Ready,
    /// The key came up. A double-tap into hands-free releases more than once;
    /// the last release before `stop` counts.
    Released,
    /// `stop` went out, after the release tail.
    StopSent,
}

#[derive(Debug, Clone)]
struct Marks {
    generation: u64,
    key_down: Instant,
    connected: Option<Instant>,
    ready: Option<Instant>,
    released: Option<Instant>,
    stop_sent: Option<Instant>,
    result: Option<Instant>,
    injected: Option<Instant>,
    server: Option<BTreeMap<String, u64>>,
}

impl Marks {
    fn new(generation: u64, key_down: Instant) -> Self {
        Self {
            generation,
            key_down,
            connected: None,
            ready: None,
            released: None,
            stop_sent: None,
            result: None,
            injected: None,
            server: None,
        }
    }
}

/// Hotkey-down on a new dictation. Returns its generation.
pub fn start() -> u64 {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    if let Ok(mut current) = CURRENT.lock() {
        *current = Some(Marks::new(generation, Instant::now()));
    }
    generation
}

/// The dictation in progress, or the last one.
pub fn current() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

pub fn mark(generation: u64, stage: Stage) {
    with_marks(generation, |marks| {
        let now = Some(Instant::now());
        match stage {
            Stage::Connected => marks.connected = now,
            Stage::Ready => marks.ready = now,
            Stage::Released => marks.released = now,
            Stage::StopSent => marks.stop_sent = now,
        }
    });
}

/// The `result` arrived and is being injected, with the server's own stage
/// timings when it sent them.
pub fn result(generation: u64, server: Option<BTreeMap<String, u64>>) {
    with_marks(generation, |marks| {
        marks.result = Some(Instant::now());
        marks.server = server;
    });
}

/// The text is in. Logs the dictation's timings and forgets them.
pub fn injected(generation: u64) {
    let Some(mut marks) = CURRENT
        .lock()
        .ok()
        .and_then(|mut current| current.take_if(|marks| marks.generation == generation))
    else {
        return;
    };
    marks.injected = Some(Instant::now());
    let (client, server) = summarise(&marks);
    tracing::info!(%client, %server, "dictation timings");
}

fn with_marks(generation: u64, update: impl FnOnce(&mut Marks)) {
    if let Ok(mut current) = CURRENT.lock() {
        if let Some(marks) = current
            .as_mut()
            .filter(|marks| marks.generation == generation)
        {
            update(marks);
        }
    }
}

/// Render the client stages and the server's as `name=ms` lists, the
/// server's in its own names so the two can be grepped side by side.
///
/// A stage that never happened — a quick tap has no separate release tail, a
/// clipboard-only paste still counts as injected — is left out rather than
/// shown as zero.
fn summarise(marks: &Marks) -> (String, String) {
    let between = |from: Option<Instant>, to: Option<Instant>| -> Option<u64> {
        Some(to?.saturating_duration_since(from?).as_millis() as u64)
    };
    let down = Some(marks.key_down);
    let stages = [
        ("keyToConnectMs", between(down, marks.connected)),
        ("keyToReadyMs", between(down, marks.ready)),
        ("tailMs", between(marks.released, marks.stop_sent)),
        ("stopToResultMs", between(marks.stop_sent, marks.result)),
        ("releaseToResultMs", between(marks.released, marks.result)),
        ("injectMs", between(marks.result, marks.injected)),
        (
            "totalAfterReleaseMs",
            between(marks.released, marks.injected),
        ),
        ("totalMs", between(down, marks.injected)),
    ];
    let client = join(
        stages
            .iter()
            .filter_map(|(name, ms)| ms.map(|ms| (*name, ms))),
    );
    let server = marks
        .server
        .as_ref()
        .filter(|server| !server.is_empty())
        .map(|server| join(server.iter().map(|(name, ms)| (name.as_str(), *ms))))
        .unwrap_or_else(|| "-".into());
    (client, server)
}

fn join<'a>(pairs: impl Iterator<Item = (&'a str, u64)>) -> String {
    pairs
        .map(|(name, ms)| format!("{name}={ms}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(base: Instant, ms: u64) -> Option<Instant> {
        Some(base + Duration::from_millis(ms))
    }

    #[test]
    fn summarises_client_and_server_stages() {
        let t0 = Instant::now();
        let mut marks = Marks::new(1, t0);
        marks.connected = at(t0, 180);
        marks.ready = at(t0, 410);
        marks.released = at(t0, 3_000);
        marks.stop_sent = at(t0, 3_250);
        marks.result = at(t0, 4_150);
        marks.injected = at(t0, 4_170);
        marks.server = Some(BTreeMap::from([
            ("sttMs".to_string(), 640),
            ("cleanupMs".to_string(), 410),
        ]));

        let (client, server) = summarise(&marks);
        assert_eq!(
            client,
            "keyToConnectMs=180 keyToReadyMs=410 tailMs=250 stopToResultMs=900 \
             releaseToResultMs=1150 injectMs=20 totalAfterReleaseMs=1170 totalMs=4170"
        );
        assert_eq!(server, "cleanupMs=410 sttMs=640");
    }

    #[test]
    fn leaves_out_stages_that_did_not_happen() {
        // A quick tap: released before `ready`, and a server with no timings.
        let t0 = Instant::now();
        let mut marks = Marks::new(1, t0);
        marks.ready = at(t0, 400);
        marks.stop_sent = at(t0, 400);
        marks.result = at(t0, 1_000);
        marks.injected = at(t0, 1_010);

        let (client, server) = summarise(&marks);
        assert_eq!(
            client,
            "keyToReadyMs=400 stopToResultMs=600 injectMs=10 totalMs=1010"
        );
        assert_eq!(server, "-");
    }

    #[test]
    fn a_stale_generation_does_not_touch_the_current_record() {
        let generation = start();
        mark(generation.wrapping_sub(1), Stage::Ready);
        mark(generation, Stage::Connected);
        let current = CURRENT.lock().unwrap().clone().unwrap();
        assert!(current.ready.is_none());
        assert!(current.connected.is_some());
    }
}
