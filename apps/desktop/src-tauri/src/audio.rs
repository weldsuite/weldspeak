//! Microphone capture.
//!
//! The microphone stays open for the whole session, not just while the hotkey
//! is down. That is what makes the pre-roll possible: by the time the key
//! registers, the first syllable has already been spoken, and it is sitting in
//! the ring buffer.
//!
//! Keeping the stream open also avoids the device-start latency that would
//! otherwise sit at the front of every dictation — on macOS, opening an input
//! device can take a couple of hundred milliseconds, which is most of the
//! latency budget spent before a word is captured.

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, Stream, StreamConfig};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use weldspeak_core::{Frame, Framer, Resampler};
use weldspeak_protocol::audio::SAMPLE_RATE;

/// An input device the settings window can offer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Microphone {
    pub name: String,
    pub is_default: bool,
}

/// Input devices currently attached. An empty list means the host would not
/// enumerate them; the UI still offers "System default".
pub fn list_input_devices() -> Vec<Microphone> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|device| device.name().ok());
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };

    let mut listed: Vec<Microphone> = devices
        .filter_map(|device| {
            let name = device.name().ok()?;
            let is_default = default_name.as_deref() == Some(name.as_str());
            Some(Microphone { name, is_default })
        })
        .collect();
    listed.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
    listed
}

/// Resolve a saved device name, falling back to the system default if it is
/// missing, empty, or the headset has been unplugged.
fn pick_input_device(preferred: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = preferred.filter(|name| !name.is_empty()) {
        if let Ok(devices) = host.input_devices() {
            for device in devices {
                if device.name().ok().as_deref() == Some(name) {
                    return Ok(device);
                }
            }
        }
        tracing::warn!(name, "saved microphone not found; using system default");
    }
    host.default_input_device()
        .ok_or_else(|| anyhow!("no microphone available"))
}

/// A running capture, conditioning device audio into wire-ready frames.
///
/// `cpal::Stream` is `!Send` — on macOS it is bound to the thread that created
/// it — so it cannot live in Tauri's managed state alongside the rest of the
/// app. Instead a dedicated thread owns the stream for its lifetime, and this
/// handle holds only what genuinely crosses threads: the shared pipeline and a
/// shutdown signal. Dropping the handle closes the channel, which ends the
/// thread, which drops the stream.
pub struct Capture {
    shared: Arc<Mutex<Pipeline>>,
    level: Arc<AtomicU64>,
    shutdown: Option<Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        // Close the stream before another Capture::start opens the same (or
        // another) device — WASAPI will refuse a second exclusive open.
        drop(self.shutdown.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Pipeline {
    resampler: Resampler,
    framer: Framer,
    meter: Meter,
}

/// Length of one loudness reading. Matches Wispr Flow's 40 ms audio chunks,
/// which is what its waveform is tuned against.
const METER_CHUNK_MS: usize = 40;

/// Accumulates device samples into one dBFS reading per [`METER_CHUNK_MS`].
struct Meter {
    sum_squares: f64,
    count: usize,
    chunk: usize,
    seq: u32,
}

impl Meter {
    fn new(sample_rate: u32, channels: usize) -> Self {
        Self {
            sum_squares: 0.0,
            count: 0,
            chunk: (sample_rate as usize * channels * METER_CHUNK_MS / 1000).max(1),
            seq: 0,
        }
    }

    /// Feed samples; publishes a reading each time a chunk completes.
    fn push(&mut self, samples: &[f32], level: &AtomicU64) {
        for &sample in samples {
            self.sum_squares += f64::from(sample) * f64::from(sample);
            self.count += 1;
            if self.count == self.chunk {
                let rms = (self.sum_squares / self.count as f64).sqrt();
                let db = (20.0 * (rms + 1e-10).log10()) as f32;
                self.seq = self.seq.wrapping_add(1);
                level.store(pack_level(self.seq, db), Ordering::Relaxed);
                self.sum_squares = 0.0;
                self.count = 0;
            }
        }
    }
}

fn pack_level(seq: u32, db: f32) -> u64 {
    (u64::from(seq) << 32) | u64::from(db.to_bits())
}

fn unpack_level(bits: u64) -> (u32, f32) {
    ((bits >> 32) as u32, f32::from_bits(bits as u32))
}

impl Capture {
    /// Open an input device and begin conditioning audio.
    ///
    /// `preferred` is a cpal device name from settings; `None` or a name that
    /// is no longer attached uses the system default. Frames are sent to
    /// `frames` only while armed; before that they feed the pre-roll buffer
    /// and are discarded as they age out.
    pub fn start(frames: Sender<Frame>, preferred: Option<String>) -> Result<Self> {
        let (shutdown, shutdown_rx) = channel::<()>();
        // The audio thread reports whether the device opened, so a missing or
        // refused microphone surfaces here rather than as silence later.
        let (ready, ready_rx) = channel::<Result<(Arc<Mutex<Pipeline>>, Arc<AtomicU64>)>>();
        // Seq 0 at -120 dBFS reads as "no audio yet" rather than full scale.
        let level = Arc::new(AtomicU64::new(pack_level(0, -120.0)));
        let level_for_thread = Arc::clone(&level);

        let thread = std::thread::Builder::new()
            .name("weldspeak-audio".into())
            .spawn(move || {
                let stream = match Self::open(frames, level_for_thread, preferred.as_deref()) {
                    Ok((stream, shared, level)) => {
                        let _ = ready.send(Ok((shared, level)));
                        stream
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };

                // Park until the handle is dropped. The stream must outlive this
                // scope, so it is held here rather than returned.
                let _ = shutdown_rx.recv();
                drop(stream);
            })?;

        match ready_rx.recv()? {
            Ok((shared, _thread_level)) => Ok(Self {
                shared,
                level,
                shutdown: Some(shutdown),
                thread: Some(thread),
            }),
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }

    /// Loudness of the most recent 40 ms of input in dBFS, with a sequence
    /// number that changes once per chunk so the waveform can tell a new
    /// reading from the same one sampled twice.
    pub fn latest_chunk_db(&self) -> (u32, f32) {
        unpack_level(self.level.load(Ordering::Relaxed))
    }

    /// Open the chosen input device. Runs on the audio thread.
    fn open(
        frames: Sender<Frame>,
        level: Arc<AtomicU64>,
        preferred: Option<&str>,
    ) -> Result<(Stream, Arc<Mutex<Pipeline>>, Arc<AtomicU64>)> {
        let device = pick_input_device(preferred)?;

        let mut supported = device.default_input_config()?;
        // Prefer 48 kHz (or 44.1) when the device offers it — more headroom for
        // the anti-aliasing resampler than a low native rate.
        if let Ok(configs) = device.supported_input_configs() {
            let preferred = configs
                .filter(|range| range.channels() >= 1)
                .filter_map(|range| {
                    let max = range.max_sample_rate().0;
                    let min = range.min_sample_rate().0;
                    let rate = [48_000, 44_100, 32_000, 16_000]
                        .into_iter()
                        .find(|r| *r >= min && *r <= max)?;
                    Some(range.with_sample_rate(cpal::SampleRate(rate)))
                })
                .max_by_key(|cfg| cfg.sample_rate().0);
            if let Some(better) = preferred {
                if better.sample_rate().0 > supported.sample_rate().0 {
                    supported = better;
                }
            }
        }
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();

        tracing::info!(
            device = device.name().unwrap_or_else(|_| "unknown".into()),
            rate = config.sample_rate.0,
            channels = config.channels,
            "opening microphone"
        );

        let shared = Arc::new(Mutex::new(Pipeline {
            resampler: Resampler::new(config.sample_rate.0, config.channels as usize, SAMPLE_RATE),
            framer: Framer::new(),
            meter: Meter::new(config.sample_rate.0, config.channels as usize),
        }));

        let stream = Self::build_stream(
            &device,
            &config,
            sample_format,
            shared.clone(),
            Arc::clone(&level),
            frames,
        )?;
        stream.play()?;

        Ok((stream, shared, level))
    }

    /// Begin retaining frames until [`Self::arm`] (hotkey-down).
    pub fn hold(&self) {
        if let Ok(mut pipeline) = self.shared.lock() {
            pipeline.framer.hold();
        }
    }

    /// Begin sending frames, handing the retained pre-roll to `send` first.
    ///
    /// `send` runs while the pipeline lock is held. The audio callback needs
    /// that lock to emit a live frame, so no live frame can be queued ahead of
    /// or among the pre-roll; arming and then sending after the lock was
    /// released let one slip in and scrambled the start of the utterance.
    pub fn arm_with(&self, mut send: impl FnMut(Frame)) {
        if let Ok(mut pipeline) = self.shared.lock() {
            for frame in pipeline.framer.arm() {
                send(frame);
            }
        }
    }

    /// Stop sending frames and drop any partial frame.
    pub fn disarm(&self) {
        if let Ok(mut pipeline) = self.shared.lock() {
            pipeline.framer.disarm();
        }
    }

    fn build_stream(
        device: &cpal::Device,
        config: &StreamConfig,
        format: SampleFormat,
        shared: Arc<Mutex<Pipeline>>,
        level: Arc<AtomicU64>,
        frames: Sender<Frame>,
    ) -> Result<Stream> {
        // An error on the audio thread must not take the process down: the user
        // may simply have unplugged a headset mid-sentence.
        let on_error = |error| tracing::error!(?error, "audio stream error");

        let stream = match format {
            SampleFormat::F32 => device.build_input_stream(
                config,
                {
                    let level = Arc::clone(&level);
                    move |data: &[f32], _| process(&shared, &level, &frames, data)
                },
                on_error,
                None,
            )?,
            SampleFormat::I16 => {
                let convert = |data: &[i16]| -> Vec<f32> {
                    data.iter().map(|s| s.to_sample::<f32>()).collect()
                };
                device.build_input_stream(
                    config,
                    move |data: &[i16], _| process(&shared, &level, &frames, &convert(data)),
                    on_error,
                    None,
                )?
            }
            SampleFormat::U16 => {
                let convert = |data: &[u16]| -> Vec<f32> {
                    data.iter().map(|s| s.to_sample::<f32>()).collect()
                };
                device.build_input_stream(
                    config,
                    move |data: &[u16], _| process(&shared, &level, &frames, &convert(data)),
                    on_error,
                    None,
                )?
            }
            other => return Err(anyhow!("unsupported sample format: {other:?}")),
        };

        Ok(stream)
    }
}

/// Runs on the audio callback thread: meter, resample, frame, hand off.
///
/// Audio is passed through untouched. Per-buffer make-up gain used to sit here;
/// it re-chose a gain for every ~10 ms buffer from that buffer's own peak, so
/// the level jumped between 1× and 4× at buffer edges and quiet syllables and
/// room noise were lifted to the same level as speech. Wispr Flow asks for the
/// raw microphone (no AGC, noise suppression or echo cancellation) and leaves
/// loudness to the recognizer, which is trained on exactly that.
fn process(
    shared: &Arc<Mutex<Pipeline>>,
    level: &Arc<AtomicU64>,
    frames: &Sender<Frame>,
    samples: &[f32],
) {
    // Blocking is deliberate. The only other holders are hold/arm/disarm,
    // which take microseconds; `try_lock` here used to discard the whole
    // callback's audio when it lost that race, leaving a hole mid-sentence.
    let Ok(mut pipeline) = shared.lock() else {
        return;
    };

    pipeline.meter.push(samples, level);
    let resampled = pipeline.resampler.push(samples);
    for frame in pipeline.framer.push(&resampled) {
        if frames.send(frame).is_err() {
            // The receiver is gone; the session has ended.
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{pack_level, unpack_level, Meter};
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn level_packing_round_trips() {
        assert_eq!(unpack_level(pack_level(7, -32.5)), (7, -32.5));
    }

    #[test]
    fn meter_reports_one_reading_per_40_ms() {
        // 1 kHz mono → 40 samples per reading.
        let level = AtomicU64::new(pack_level(0, -120.0));
        let mut meter = Meter::new(1_000, 1);
        meter.push(&[0.5; 39], &level);
        assert_eq!(unpack_level(level.load(Ordering::Relaxed)).0, 0);
        meter.push(&[0.5; 1], &level);
        let (seq, db) = unpack_level(level.load(Ordering::Relaxed));
        assert_eq!(seq, 1);
        // RMS 0.5 ≈ −6 dBFS.
        assert!((db + 6.02).abs() < 0.05, "got {db}");
    }

    #[test]
    fn silence_reads_as_very_quiet() {
        let level = AtomicU64::new(0);
        let mut meter = Meter::new(1_000, 1);
        meter.push(&[0.0; 40], &level);
        assert!(unpack_level(level.load(Ordering::Relaxed)).1 < -150.0);
    }
}
