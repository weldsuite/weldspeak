//! Keeping the microphone alive.
//!
//! The microphone is opened once and held for the whole session, which is what
//! makes the pre-roll possible — and also what makes a dead stream so costly.
//! A headset unplugged, a Bluetooth link dropped, or a driver that quietly stops
//! delivering buffers leaves an open stream that produces nothing, and every
//! dictation after it is silent until the app restarts.
//!
//! The desktop crate watches the stream and rebuilds it. The decisions it makes
//! along the way — is this stream dead or just starting, which device to try
//! next, when to go back to the one the user chose — are policy with no
//! platform in them, so they live here where the edge cases can be tested
//! without unplugging anything.

use std::time::{Duration, Instant};

/// A stream that has delivered audio and then goes this long without another
/// reading is dead.
///
/// The stream stays open between dictations to feed the pre-roll, so buffers
/// arrive continuously — every 10 ms or so on every host — whether anyone is
/// speaking or not. A second and a half without one is far outside scheduling
/// jitter, and short enough to catch a dead stream before the user's next
/// dictation most of the time.
pub const STALL_AFTER: Duration = Duration::from_millis(1_500);

/// How long a freshly opened stream may take to deliver its first reading.
///
/// Longer than [`STALL_AFTER`] because starting is slower than running:
/// a Bluetooth headset switching into its headset profile can take a couple of
/// seconds before the first buffer arrives, and judging it dead in that window
/// would fail over away from a device that was about to work.
pub const START_GRACE: Duration = Duration::from_secs(3);

/// A gap this long between two looks means the watcher itself was not running
/// — the machine slept — rather than that the stream stalled.
///
/// Streams stop across sleep and usually resume on wake. Without this, the
/// first look after waking would see seconds of silence and rebuild a stream
/// that was about to start again.
pub const SUSPEND_GAP: Duration = Duration::from_secs(5);

/// A dictation at least this long in which every sample was exactly zero is
/// treated as a stream worth rebuilding.
///
/// Real microphones are never digitally silent — even a quiet room has a noise
/// floor — so a whole utterance of exact zeros usually means a stream that is
/// running but disconnected from the hardware. A hardware mute switch produces
/// the same thing, which is why this only ever earns one same-device rebuild,
/// never a switch to another microphone.
pub const SILENT_TAKE_MIN: Duration = Duration::from_secs(1);

/// Device-name fragments that mark an input as a loopback or virtual device.
///
/// These capture what the computer is playing, not what the user is saying.
/// Falling back to one would "work" — audio arrives, the stream looks healthy
/// — while dictating the user's music or meeting instead of their voice. They
/// stay usable when chosen explicitly; they are only skipped when WeldSpeak is
/// picking on the user's behalf.
const LOOPBACK_MARKERS: &[&str] = &[
    "stereo mix",
    "what u hear",
    "wave out mix",
    "line in",
    "line-in",
    "loopback",
    "cable output",
    "vb-audio",
    "voicemeeter",
    "virtual",
    "blackhole",
    "soundflower",
    "monitor of",
];

/// Whether `name` looks like a loopback or virtual input rather than a
/// microphone.
///
/// A marker only counts at the start of a word, so "Headline Input" is not a
/// line input.
pub fn is_loopback_name(name: &str) -> bool {
    let name = name.to_lowercase();
    LOOPBACK_MARKERS
        .iter()
        .any(|marker| starts_a_word(&name, marker))
}

fn starts_a_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(at, _)| {
        haystack[..at]
            .chars()
            .next_back()
            .is_none_or(|before| !before.is_alphanumeric())
    })
}

/// The device the user asked for, when it is attached.
///
/// A saved name is only a target while that device is present; "System
/// default" follows whatever the host currently reports as default.
pub fn reclaim_target(
    requested: Option<&str>,
    default: Option<&str>,
    attached: &[String],
) -> Option<String> {
    match requested.filter(|name| !name.is_empty()) {
        Some(name) => attached
            .iter()
            .any(|device| device == name)
            .then(|| name.to_string()),
        None => default.map(str::to_string),
    }
}

/// Devices to try, in order, when the stream has died.
///
/// The first is the device the user asked for — their saved microphone if it
/// is attached, otherwise the system default, exactly what opening the
/// microphone normally picks — so a stream that merely wedged is rebuilt where
/// it was. After that come the system default and then every other attached
/// input by name, skipping loopback and virtual devices unless the user chose
/// that very device.
pub fn failover_order(
    requested: Option<&str>,
    default: Option<&str>,
    attached: &[String],
) -> Vec<String> {
    // The user's own choice goes first, loopback or not. When their saved
    // device is gone there is no choice to honour, and the default below
    // stands in like any other fallback.
    let mut order: Vec<String> = reclaim_target(requested, default, attached)
        .into_iter()
        .collect();

    let mut others: Vec<&str> = attached.iter().map(String::as_str).collect();
    others.sort_unstable();
    for name in default.into_iter().chain(others) {
        if !is_loopback_name(name) && !order.iter().any(|seen| seen == name) {
            order.push(name.to_string());
        }
    }
    order
}

/// Whether a stream is delivering audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Readings are arriving.
    Alive,
    /// Newly opened and still within [`START_GRACE`].
    Starting,
    /// No reading for longer than the stream is allowed.
    Stalled { silent_for: Duration },
}

impl Health {
    pub fn is_stalled(self) -> bool {
        matches!(self, Health::Stalled { .. })
    }
}

/// Judges one stream's liveness from the level meter's sequence number, which
/// advances once per reading while the device is delivering audio.
///
/// Readings are a sequence rather than a timestamp so the audio callback does
/// nothing more than it already does; the watcher supplies the clock.
#[derive(Debug, Clone)]
pub struct Liveness {
    last_seq: u32,
    /// When the sequence last moved, or `None` if it has not since opening.
    last_advance: Option<Instant>,
    /// When the stream opened — or, after a suspend, when watching resumed.
    started: Instant,
    last_look: Instant,
}

impl Liveness {
    /// Start watching a stream whose meter currently reads `seq`.
    pub fn new(now: Instant, seq: u32) -> Self {
        Self {
            last_seq: seq,
            last_advance: None,
            started: now,
            last_look: now,
        }
    }

    /// Take a reading and judge the stream.
    pub fn observe(&mut self, now: Instant, seq: u32) -> Health {
        let gap = now.saturating_duration_since(self.last_look);
        self.last_look = now;

        if seq != self.last_seq {
            self.last_seq = seq;
            self.last_advance = Some(now);
            return Health::Alive;
        }

        if gap >= SUSPEND_GAP {
            // Asleep, not stalled. Give the stream the same grace as a fresh
            // open to come back on its own.
            self.started = now;
            self.last_advance = None;
            return Health::Starting;
        }

        match self.last_advance {
            Some(at) => {
                let silent_for = now.saturating_duration_since(at);
                if silent_for >= STALL_AFTER {
                    Health::Stalled { silent_for }
                } else {
                    Health::Alive
                }
            }
            None => {
                let silent_for = now.saturating_duration_since(self.started);
                if silent_for >= START_GRACE {
                    Health::Stalled { silent_for }
                } else {
                    Health::Starting
                }
            }
        }
    }
}

/// When to go back to the user's microphone after failing over.
///
/// Only when the target *changes* — the saved device reappears, or the system
/// default moves — not merely because it is listed. A device can stay listed
/// while delivering nothing (a Bluetooth headset that dropped its link but not
/// its endpoint), and switching back to it on every look would bounce between
/// a dead microphone and a working one.
#[derive(Debug, Clone)]
pub struct Reclaim {
    seen: Option<String>,
}

impl Reclaim {
    /// Start watching from the target as it stands at the moment of failover.
    pub fn new(target_now: Option<String>) -> Self {
        Self { seen: target_now }
    }

    /// The device to go back to, if the target has changed since the last look
    /// and is not the one already in use.
    pub fn observe(&mut self, target_now: Option<String>, current: &str) -> Option<String> {
        if target_now == self.seen {
            return None;
        }
        self.seen.clone_from(&target_now);
        target_now.filter(|target| target != current)
    }
}

/// Wait before the next recovery attempt after `failures` in a row.
///
/// The first attempt is immediate. After that the wait doubles to a cap, so a
/// machine with no working microphone at all is probed every half-minute
/// rather than hammered — each attempt opens devices and logs a line.
pub fn retry_delay(failures: u32) -> Duration {
    match failures {
        0 => Duration::ZERO,
        n => Duration::from_secs((1u64 << n.min(5)).min(30)),
    }
}

/// Whether a finished dictation's audio suggests a stream that is running but
/// disconnected: long enough to judge, and not one non-zero sample in it.
pub fn is_suspect_silence(audio: Duration, heard_signal: bool) -> bool {
    !heard_signal && audio >= SILENT_TAKE_MIN
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn recognises_loopback_and_virtual_inputs() {
        for name in [
            "Stereo Mix (Realtek(R) Audio)",
            "Line In (High Definition Audio Device)",
            "CABLE Output (VB-Audio Virtual Cable)",
            "VoiceMeeter Output (VB-Audio VoiceMeeter VAIO)",
            "BlackHole 2ch",
            "Soundflower (2ch)",
            "Monitor of Built-in Audio Analog Stereo",
            "Loopback Audio",
            "What U Hear (Sound Blaster)",
        ] {
            assert!(is_loopback_name(name), "{name} should be loopback");
        }
    }

    #[test]
    fn leaves_real_microphones_alone() {
        for name in [
            "Microphone (Realtek(R) Audio)",
            "MacBook Pro Microphone",
            "Headset (AirPods Pro Hands-Free)",
            "Microphone (NVIDIA Broadcast)",
            "Headline Input Array",
            "Microphone Array (Intel® Smart Sound Technology)",
        ] {
            assert!(!is_loopback_name(name), "{name} should not be loopback");
        }
    }

    #[test]
    fn rebuilds_on_the_saved_microphone_first() {
        let attached = names(&["Laptop Mic", "USB Headset", "Webcam Mic"]);
        assert_eq!(
            failover_order(Some("USB Headset"), Some("Laptop Mic"), &attached),
            names(&["USB Headset", "Laptop Mic", "Webcam Mic"]),
        );
    }

    #[test]
    fn a_missing_saved_microphone_falls_to_the_default() {
        let attached = names(&["Laptop Mic", "Webcam Mic"]);
        assert_eq!(
            failover_order(Some("USB Headset"), Some("Laptop Mic"), &attached),
            names(&["Laptop Mic", "Webcam Mic"]),
        );
    }

    #[test]
    fn system_default_users_start_from_the_default() {
        let attached = names(&["Webcam Mic", "Laptop Mic"]);
        assert_eq!(
            failover_order(None, Some("Laptop Mic"), &attached),
            names(&["Laptop Mic", "Webcam Mic"]),
        );
        // An empty saved name is how Settings spells "System default".
        assert_eq!(
            failover_order(Some(""), Some("Laptop Mic"), &attached),
            names(&["Laptop Mic", "Webcam Mic"]),
        );
    }

    #[test]
    fn never_fails_over_onto_a_loopback_device() {
        let attached = names(&[
            "Stereo Mix (Realtek)",
            "Laptop Mic",
            "CABLE Output (VB-Audio)",
        ]);
        assert_eq!(
            failover_order(Some("USB Headset"), Some("Stereo Mix (Realtek)"), &attached),
            names(&["Laptop Mic"]),
        );
    }

    #[test]
    fn a_loopback_device_the_user_chose_is_still_rebuilt() {
        let attached = names(&["Stereo Mix (Realtek)", "Laptop Mic"]);
        assert_eq!(
            failover_order(Some("Stereo Mix (Realtek)"), Some("Laptop Mic"), &attached),
            names(&["Stereo Mix (Realtek)", "Laptop Mic"]),
        );
        // Choosing "System default" when the OS default is a loopback device is
        // the user's choice too.
        assert_eq!(
            failover_order(None, Some("Stereo Mix (Realtek)"), &attached),
            names(&["Stereo Mix (Realtek)", "Laptop Mic"]),
        );
    }

    #[test]
    fn an_unenumerable_host_still_offers_its_default() {
        assert_eq!(
            failover_order(Some("USB Headset"), Some("Laptop Mic"), &[]),
            names(&["Laptop Mic"]),
        );
        assert!(failover_order(None, None, &[]).is_empty());
    }

    #[test]
    fn a_steady_stream_is_alive() {
        let t0 = Instant::now();
        let mut live = Liveness::new(t0, 0);
        for step in 1..20u32 {
            let now = t0 + Duration::from_millis(500) * step;
            assert_eq!(live.observe(now, step), Health::Alive);
        }
    }

    #[test]
    fn a_stream_that_stops_is_stalled_after_the_limit() {
        let t0 = Instant::now();
        let mut live = Liveness::new(t0, 0);
        assert_eq!(
            live.observe(t0 + Duration::from_millis(500), 7),
            Health::Alive
        );
        assert_eq!(
            live.observe(t0 + Duration::from_millis(1_000), 7),
            Health::Alive
        );
        assert_eq!(
            live.observe(t0 + Duration::from_millis(1_500), 7),
            Health::Alive
        );
        assert!(live
            .observe(t0 + Duration::from_millis(2_000), 7)
            .is_stalled());
    }

    #[test]
    fn a_new_stream_gets_a_start_grace() {
        let t0 = Instant::now();
        let mut live = Liveness::new(t0, 0);
        assert_eq!(
            live.observe(t0 + Duration::from_millis(2_000), 0),
            Health::Starting
        );
        assert_eq!(
            live.observe(t0 + Duration::from_millis(2_500), 0),
            Health::Starting
        );
        assert!(live.observe(t0 + START_GRACE, 0).is_stalled());
    }

    #[test]
    fn a_slow_starter_that_begins_is_alive() {
        let t0 = Instant::now();
        let mut live = Liveness::new(t0, 3);
        assert_eq!(
            live.observe(t0 + Duration::from_millis(2_500), 3),
            Health::Starting
        );
        assert_eq!(
            live.observe(t0 + Duration::from_millis(2_900), 4),
            Health::Alive
        );
    }

    #[test]
    fn waking_from_sleep_is_not_a_stall() {
        let t0 = Instant::now();
        let mut live = Liveness::new(t0, 0);
        assert_eq!(
            live.observe(t0 + Duration::from_millis(500), 1),
            Health::Alive
        );
        // The watcher did not run for a minute: the machine slept.
        let woke = t0 + Duration::from_secs(60);
        assert_eq!(live.observe(woke, 1), Health::Starting);
        assert_eq!(
            live.observe(woke + Duration::from_millis(500), 1),
            Health::Starting
        );
        // Still nothing once the grace is up: now it really is dead.
        assert!(live.observe(woke + START_GRACE, 1).is_stalled());
    }

    #[test]
    fn digital_silence_is_not_a_stall() {
        // The meter advances on silence too; liveness is about buffers
        // arriving, not about what is in them. A muted mic is the user's call.
        let t0 = Instant::now();
        let mut live = Liveness::new(t0, 0);
        for step in 1..10u32 {
            assert_eq!(
                live.observe(t0 + Duration::from_millis(500) * step, step),
                Health::Alive
            );
        }
    }

    #[test]
    fn goes_back_when_the_saved_microphone_reappears() {
        let mut reclaim = Reclaim::new(None);
        assert_eq!(reclaim.observe(None, "Laptop Mic"), None);
        assert_eq!(
            reclaim.observe(Some("USB Headset".into()), "Laptop Mic"),
            Some("USB Headset".into()),
        );
        // Once is enough; staying listed is not a reason to switch again.
        assert_eq!(
            reclaim.observe(Some("USB Headset".into()), "Laptop Mic"),
            None
        );
    }

    #[test]
    fn a_listed_but_dead_microphone_is_not_reclaimed_until_it_changes() {
        // Failed over away from a headset that is still listed.
        let mut reclaim = Reclaim::new(Some("USB Headset".into()));
        assert_eq!(
            reclaim.observe(Some("USB Headset".into()), "Laptop Mic"),
            None
        );
        // Unplugged and plugged back in.
        assert_eq!(reclaim.observe(None, "Laptop Mic"), None);
        assert_eq!(
            reclaim.observe(Some("USB Headset".into()), "Laptop Mic"),
            Some("USB Headset".into()),
        );
    }

    #[test]
    fn does_not_reclaim_the_device_already_in_use() {
        let mut reclaim = Reclaim::new(Some("Laptop Mic".into()));
        assert_eq!(
            reclaim.observe(Some("Webcam Mic".into()), "Webcam Mic"),
            None
        );
    }

    #[test]
    fn retries_back_off_to_half_a_minute() {
        let secs: Vec<u64> = (0..8).map(|n| retry_delay(n).as_secs()).collect();
        assert_eq!(secs, vec![0, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(retry_delay(u32::MAX), Duration::from_secs(30));
    }

    #[test]
    fn only_a_whole_take_of_exact_zeros_is_suspect() {
        assert!(is_suspect_silence(Duration::from_secs(2), false));
        assert!(!is_suspect_silence(Duration::from_secs(2), true));
        // A tap is too short to judge.
        assert!(!is_suspect_silence(Duration::from_millis(400), false));
    }
}
