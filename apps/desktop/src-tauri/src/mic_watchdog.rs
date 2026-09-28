//! Noticing a dead microphone stream and rebuilding it.
//!
//! The stream is opened once and kept open for the pre-roll, so when it dies —
//! a headset unplugged, a Bluetooth link dropped, a driver that stops calling
//! back — nothing reopens it, and every dictation afterwards is silent until
//! the app restarts. This module watches for that and climbs a short ladder:
//!
//!   1. Rebuild on the device the user asked for (their saved microphone, or
//!      the system default), which is all a wedged driver needs.
//!   2. Fail over to the default, then to any other attached input, skipping
//!      loopback and virtual devices the user never chose.
//!   3. Only when nothing works, tell the user.
//!
//! Failing over never touches the saved setting. It is a stand-in until the
//! user's own microphone comes back, and the watchdog switches back when it
//! does.
//!
//! A stream in use is never rebuilt: a dictation in progress is left alone and
//! the watchdog acts once it ends. The exception is hotkey-down on a stream
//! already known to be dead, where there is nothing left to protect and
//! rebuilding at once is the only way the take is not lost.
//!
//! The decisions — when a stream counts as dead, which device is next, when to
//! go back — live in `weldspeak_core::mic`, where they are tested.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use weldspeak_core::mic::{self, Health, Liveness, Reclaim};

use crate::audio::{self, Capture};
use crate::AppState;

/// How often the watchdog looks. Cheap — two atomic loads and a lock — and
/// frequent enough that a dead stream is usually caught before the next
/// dictation.
const TICK: Duration = Duration::from_millis(500);

/// How long a rebuilt stream on the user's own device has to deliver audio.
/// Generous, because a Bluetooth headset switching profile can take seconds,
/// and failing over away from it would stick until it is unplugged.
const PROBE_PREFERRED: Duration = Duration::from_secs(4);

/// How long a fallback device has to deliver audio before the next is tried.
const PROBE_FALLBACK: Duration = Duration::from_secs(2);

/// How often to check, while failed over, whether the user's microphone is
/// back. Each check enumerates devices, which is not free on every host.
const RECLAIM_EVERY: Duration = Duration::from_secs(3);

/// At most one "using another microphone" notice in this window, so a device
/// that keeps dropping does not keep popping the overlay.
const NOTICE_EVERY: Duration = Duration::from_secs(60);

const NO_MICROPHONE: &str =
    "WeldSpeak can't hear a microphone. Check it is connected, or choose one in Settings.";

/// A recovery is in flight. Only one ladder runs at a time; the watchdog and
/// hotkey-down both start them.
static RECOVERING: AtomicBool = AtomicBool::new(false);
/// The watchdog's latest verdict: the stream is dead or missing. Read at
/// hotkey-down, which cannot wait for the next tick.
static DEAD: AtomicBool = AtomicBool::new(false);
static MEMORY: Mutex<Memory> = Mutex::new(Memory {
    failover: None,
    silent_take: None,
    zero_rebuilt: None,
    last_notice: None,
});

/// What recovery remembers between attempts.
struct Memory {
    /// Running on a stand-in; watching for the user's microphone to return.
    failover: Option<Reclaim>,
    /// A capture whose last take was pure digital silence.
    silent_take: Option<u64>,
    /// The device already rebuilt once for digital silence. A second silent
    /// take on it is most likely a mute switch, which is the user's business.
    zero_rebuilt: Option<String>,
    last_notice: Option<Instant>,
}

fn memory() -> std::sync::MutexGuard<'static, Memory> {
    MEMORY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Why a recovery started.
#[derive(Debug, Clone, Copy)]
enum Cause {
    /// The device reported an error.
    Error,
    /// No readings for this long.
    Stalled(Duration),
    /// No capture at all: the microphone never opened, or the last attempt
    /// found nothing.
    Absent,
    /// A whole take of exact zeros. Earns one same-device rebuild only.
    SilentTake,
    /// The hotkey went down on a stream already known to be dead.
    HotkeyDown,
}

impl Cause {
    /// For the log: how long a stalled stream had gone without audio.
    fn silent_ms(self) -> Option<u64> {
        match self {
            Cause::Stalled(silent_for) => Some(silent_for.as_millis() as u64),
            _ => None,
        }
    }
}

enum Outcome {
    Recovered {
        device: String,
        stand_in: bool,
    },
    Exhausted,
    /// Settings replaced the microphone mid-ladder, a dictation began where
    /// one may not be interrupted, or another ladder was already running.
    Abandoned,
}

/// Start the watchdog for the app's lifetime.
pub fn spawn(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("weldspeak-mic-watchdog".into())
        .spawn(move || watch(app));
}

/// The microphone was (re)opened from settings. Recovery starts afresh, and
/// if the saved device was missing and the default stood in, that counts as
/// failing over: the saved device is taken back when it appears.
pub fn on_reopened(capture: &Capture, requested: Option<&str>) {
    DEAD.store(false, Ordering::SeqCst);
    let fell_back = requested.is_some_and(|name| !name.is_empty() && name != capture.device_name());
    let mut memory = memory();
    memory.failover = fell_back.then(|| Reclaim::new(None));
    memory.silent_take = None;
}

/// Hotkey-down. If the stream is already known to be dead, rebuild it now,
/// in the background, while the socket opens — that takes a few hundred
/// milliseconds anyway, and the rebuilt capture joins the take as soon as it
/// is up. Nothing is probed: the user is speaking, so the first device that
/// opens is taken, and the watchdog judges it once the dictation is over.
pub fn on_hotkey_down(app: &AppHandle) {
    let dead = DEAD.load(Ordering::SeqCst)
        || app
            .state::<AppState>()
            .capture
            .lock()
            .map(|slot| slot.as_ref().is_none_or(Capture::errored))
            .unwrap_or(false);
    if !dead || RECOVERING.load(Ordering::SeqCst) {
        return;
    }

    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("weldspeak-mic-recover".into())
        .spawn(move || {
            if let Outcome::Exhausted = recover(&app, Cause::HotkeyDown) {
                let for_main = app.clone();
                let _ = app.run_on_main_thread(move || {
                    crate::dictation::abandon(&for_main, NO_MICROPHONE);
                });
            }
        });
}

/// A dictation ended. Remember a take of pure digital silence so the watchdog
/// can rebuild the stream before the next one.
pub fn after_take(capture: &Capture) {
    let mut memory = memory();
    if capture.take_was_digitally_silent() {
        memory.silent_take = Some(capture.id());
    } else if capture.heard_signal() {
        memory.zero_rebuilt = None;
    }
}

fn watch(app: AppHandle) {
    let mut liveness: Option<(u64, Liveness)> = None;
    let mut failures: u32 = 0;
    let mut next_attempt = Instant::now();
    let mut last_reclaim_look = Instant::now();
    // Whether a stream has worked since the user was last told about an
    // outage. A machine that has never had a microphone is not told on every
    // launch; it finds out when it presses the hotkey.
    let mut had_working = false;
    let mut warned = false;

    loop {
        std::thread::sleep(TICK);
        let now = Instant::now();
        let state = app.state::<AppState>();

        let snapshot = state.capture.lock().ok().and_then(|slot| {
            slot.as_ref()
                .map(|capture| (capture.id(), capture.latest_chunk_db().0, capture.errored()))
        });

        let (cause, health) = match snapshot {
            None => {
                liveness = None;
                (Some(Cause::Absent), None)
            }
            Some((id, seq, errored)) => {
                let live = match &mut liveness {
                    Some((seen, live)) if *seen == id => live,
                    slot => &mut slot.insert((id, Liveness::new(now, seq))).1,
                };
                let health = live.observe(now, seq);
                let cause = match health {
                    _ if errored => Some(Cause::Error),
                    Health::Stalled { silent_for } => Some(Cause::Stalled(silent_for)),
                    Health::Alive | Health::Starting => None,
                };
                (cause, Some((id, health)))
            }
        };
        DEAD.store(cause.is_some(), Ordering::SeqCst);

        if !is_idle(&app) || RECOVERING.load(Ordering::SeqCst) {
            continue;
        }

        if let Some(cause) = cause {
            if now < next_attempt {
                continue;
            }
            match recover(&app, cause) {
                Outcome::Recovered { device, stand_in } => {
                    failures = 0;
                    if stand_in && had_working {
                        notify_rate_limited(
                            &app,
                            &format!("Microphone stopped responding. Using {device} for now."),
                        );
                    }
                }
                Outcome::Exhausted => {
                    failures = failures.saturating_add(1);
                    next_attempt = Instant::now() + mic::retry_delay(failures);
                    if !warned {
                        warned = true;
                        set_tray_warning(&app, true);
                        if had_working {
                            had_working = false;
                            notify_if_idle(&app, NO_MICROPHONE);
                        }
                    }
                }
                Outcome::Abandoned => {}
            }
            continue;
        }

        let Some((id, health)) = health else {
            continue;
        };
        if health == Health::Alive {
            had_working = true;
            failures = 0;
            next_attempt = now;
            if warned {
                warned = false;
                set_tray_warning(&app, false);
            }
        }

        // A take of pure zeros on a live stream: one rebuild in place. A
        // take from a capture since replaced says nothing about this one.
        let silent_take = memory().silent_take.take().is_some_and(|take| take == id);
        if silent_take {
            rebuild_after_silent_take(&app);
            continue;
        }

        if now.duration_since(last_reclaim_look) >= RECLAIM_EVERY {
            last_reclaim_look = now;
            reclaim_if_back(&app);
        }
    }
}

fn rebuild_after_silent_take(app: &AppHandle) {
    let Some(device) = current_device(app) else {
        return;
    };
    {
        let mut memory = memory();
        if memory.zero_rebuilt.as_deref() == Some(device.as_str()) {
            tracing::debug!(%device, "another digitally silent take; leaving the microphone (muted?)");
            return;
        }
        memory.zero_rebuilt = Some(device);
    }
    recover(app, Cause::SilentTake);
}

/// While failed over, go back to the user's microphone once it returns.
fn reclaim_if_back(app: &AppHandle) {
    if memory().failover.is_none() {
        return;
    }
    let Some(current) = current_device(app) else {
        return;
    };
    let requested = requested_microphone(app);
    let (default, attached) = audio::input_device_names();
    let target = mic::reclaim_target(requested.as_deref(), default.as_deref(), &attached);

    let back = memory()
        .failover
        .as_mut()
        .and_then(|reclaim| reclaim.observe(target, &current));
    let Some(device) = back else {
        return;
    };
    // Held like a recovery, so a hotkey pressed mid-switch waits for this
    // capture rather than starting a ladder of its own.
    if RECOVERING.swap(true, Ordering::SeqCst) {
        return;
    }
    tracing::info!(%device, stand_in = %current, "microphone is back; switching to it");
    crate::reopen_microphone(app);
    crate::dictation::adopt_capture(app);
    RECOVERING.store(false, Ordering::SeqCst);
}

/// Climb the ladder once. Logs one line for the whole attempt.
fn recover(app: &AppHandle, cause: Cause) -> Outcome {
    if RECOVERING.swap(true, Ordering::SeqCst) {
        return Outcome::Abandoned;
    }
    struct Release;
    impl Drop for Release {
        fn drop(&mut self) {
            RECOVERING.store(false, Ordering::SeqCst);
        }
    }
    let _release = Release;

    let started = Instant::now();
    let state = app.state::<AppState>();
    let Some(frames) = state.frames.lock().ok().and_then(|guard| guard.clone()) else {
        return Outcome::Abandoned;
    };
    let requested = requested_microphone(app);
    let (default, attached) = audio::input_device_names();
    let candidates = match cause {
        // Rebuilt where it is, whichever device that is: digital silence is
        // no reason to move the user to another microphone.
        Cause::SilentTake => current_device(app).into_iter().collect(),
        _ => mic::failover_order(requested.as_deref(), default.as_deref(), &attached),
    };
    if candidates.is_empty() {
        // A desktop with no microphone at all retries every half-minute for
        // as long as the app runs; that is not an error worth a line each time.
        tracing::debug!(?cause, "microphone recovery: no input devices attached");
        return Outcome::Exhausted;
    }

    let mut expected = current_id(app);
    let mut tried: Vec<String> = Vec::new();

    for (rung, device) in candidates.into_iter().enumerate() {
        let busy = !is_idle(app);
        // Digital silence is a hunch, not a dead stream: never worth
        // interrupting a dictation for.
        if busy && matches!(cause, Cause::SilentTake) {
            tracing::info!(?cause, "microphone rebuild deferred: dictation in progress");
            return Outcome::Abandoned;
        }
        if current_id(app) != expected {
            tracing::info!(
                ?cause,
                "microphone recovery superseded by a settings change"
            );
            return Outcome::Abandoned;
        }

        // Close the old stream before opening the next, as reopening does:
        // some drivers refuse a second open of the same device. Dropped
        // outside the lock, since closing joins the audio thread.
        let old = state.capture.lock().ok().and_then(|mut slot| slot.take());
        drop(old);
        expected = None;

        let capture = match Capture::start_exact(frames.clone(), device.clone()) {
            Ok(capture) => capture,
            Err(error) => {
                tried.push(format!("{device}: {error}"));
                continue;
            }
        };
        let id = capture.id();
        let seq = capture.latest_chunk_db().0;
        {
            let Ok(mut slot) = state.capture.lock() else {
                return Outcome::Abandoned;
            };
            if slot.is_some() {
                // Settings opened a microphone while this one was starting.
                tracing::info!(
                    ?cause,
                    "microphone recovery superseded by a settings change"
                );
                return Outcome::Abandoned;
            }
            *slot = Some(capture);
        }
        expected = Some(id);
        crate::dictation::adopt_capture(app);

        let verdict = if busy {
            Probe::Busy
        } else {
            let limit = if rung == 0 {
                PROBE_PREFERRED
            } else {
                PROBE_FALLBACK
            };
            probe(app, id, seq, limit)
        };
        match verdict {
            Probe::Audio | Probe::Busy => {
                let stand_in = rung > 0
                    || requested
                        .as_deref()
                        .is_some_and(|name| !name.is_empty() && name != device);
                tracing::info!(
                    ?cause,
                    silent_ms = ?cause.silent_ms(),
                    %device,
                    rung,
                    stand_in,
                    unprobed = matches!(verdict, Probe::Busy),
                    ?tried,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "microphone recovered"
                );
                let mut memory = memory();
                memory.failover = stand_in.then(|| {
                    Reclaim::new(mic::reclaim_target(
                        requested.as_deref(),
                        default.as_deref(),
                        &attached,
                    ))
                });
                memory.silent_take = None;
                DEAD.store(false, Ordering::SeqCst);
                return Outcome::Recovered { device, stand_in };
            }
            Probe::Superseded => {
                tracing::info!(
                    ?cause,
                    "microphone recovery superseded by a settings change"
                );
                return Outcome::Abandoned;
            }
            Probe::Errored => tried.push(format!("{device}: stream error")),
            Probe::Silent => tried.push(format!("{device}: no audio")),
        }
    }

    tracing::error!(
        ?cause,
        silent_ms = ?cause.silent_ms(),
        ?tried,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "microphone recovery failed: no working input"
    );
    Outcome::Exhausted
}

enum Probe {
    /// Readings arrived.
    Audio,
    /// A dictation started on this capture; it is kept without judgement.
    Busy,
    Silent,
    Errored,
    /// Someone else replaced the capture.
    Superseded,
}

/// Wait up to `limit` for a freshly opened capture to deliver audio.
fn probe(app: &AppHandle, id: u64, seq: u32, limit: Duration) -> Probe {
    let deadline = Instant::now() + limit;
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let reading = app
            .state::<AppState>()
            .capture
            .lock()
            .ok()
            .and_then(|slot| {
                slot.as_ref()
                    .map(|capture| (capture.id(), capture.latest_chunk_db().0, capture.errored()))
            });
        match reading {
            Some((current, _, _)) if current != id => return Probe::Superseded,
            None => return Probe::Superseded,
            Some((_, _, true)) => return Probe::Errored,
            // Two readings, not one: a device can flush a single stale
            // buffer on start and then deliver nothing.
            Some((_, now, _)) if now.wrapping_sub(seq) >= 2 => return Probe::Audio,
            Some(_) => {}
        }
        if !is_idle(app) {
            return Probe::Busy;
        }
        if Instant::now() >= deadline {
            return Probe::Silent;
        }
    }
}

/// No dictation in progress and no Settings change waiting on one.
fn is_idle(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let idle = state
        .session
        .lock()
        .map(|session| session.is_idle())
        .unwrap_or(false);
    idle && !state.mic_dirty.load(Ordering::SeqCst)
}

fn current_id(app: &AppHandle) -> Option<u64> {
    app.state::<AppState>()
        .capture
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(Capture::id))
}

fn current_device(app: &AppHandle) -> Option<String> {
    app.state::<AppState>()
        .capture
        .lock()
        .ok()
        .and_then(|slot| {
            slot.as_ref()
                .map(|capture| capture.device_name().to_string())
        })
}

fn requested_microphone(app: &AppHandle) -> Option<String> {
    app.state::<AppState>()
        .settings
        .lock()
        .ok()
        .and_then(|settings| settings.microphone.clone())
}

/// Show a notice unless a dictation has started; the listening pill matters
/// more than news about the microphone.
fn notify_if_idle(app: &AppHandle, message: &str) {
    if is_idle(app) {
        crate::overlay::show_notice(app, message);
    }
}

fn notify_rate_limited(app: &AppHandle, message: &str) {
    {
        let mut memory = memory();
        if memory
            .last_notice
            .is_some_and(|at| at.elapsed() < NOTICE_EVERY)
        {
            return;
        }
        memory.last_notice = Some(Instant::now());
    }
    notify_if_idle(app, message);
}

/// The tray tooltip says when there is no working microphone, for as long as
/// that lasts — the overlay notice is gone after a few seconds.
fn set_tray_warning(app: &AppHandle, warning: bool) {
    if let Some(tray) = app.tray_by_id("main") {
        let tooltip = if warning {
            "WeldSpeak — no working microphone"
        } else {
            "WeldSpeak"
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }
}
