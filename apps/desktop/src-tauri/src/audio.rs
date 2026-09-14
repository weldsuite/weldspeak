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
use std::sync::atomic::{AtomicU32, Ordering};
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
    level: Arc<AtomicU32>,
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
        let (ready, ready_rx) = channel::<Result<(Arc<Mutex<Pipeline>>, Arc<AtomicU32>)>>();
        let level = Arc::new(AtomicU32::new(0));
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

    /// Instantaneous microphone loudness, 0.0–1.0, for the listening waveform.
    pub fn current_level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Open the chosen input device. Runs on the audio thread.
    fn open(
        frames: Sender<Frame>,
        level: Arc<AtomicU32>,
        preferred: Option<&str>,
    ) -> Result<(Stream, Arc<Mutex<Pipeline>>, Arc<AtomicU32>)> {
        let device = pick_input_device(preferred)?;

        let supported = device.default_input_config()?;
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

    /// Begin sending frames, returning the retained pre-roll to send first.
    pub fn arm(&self) -> Vec<Frame> {
        self.shared
            .lock()
            .map(|mut pipeline| pipeline.framer.arm())
            .unwrap_or_default()
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
        level: Arc<AtomicU32>,
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

/// Runs on the audio callback thread: resample, frame, hand off.
///
/// This thread has a hard deadline — overrunning it produces an audible glitch
/// — so it does no I/O and never blocks. The channel send is non-blocking and a
/// full channel drops the frame rather than stalling capture.
fn process(
    shared: &Arc<Mutex<Pipeline>>,
    level: &Arc<AtomicU32>,
    frames: &Sender<Frame>,
    samples: &[f32],
) {
    // Peak with instant attack and a short release so the overlay can track
    // speech. Raw RMS of conversational mic input is ~0.02 and would look like
    // silence if drawn linearly.
    let peak = samples
        .iter()
        .fold(0.0f32, |max, sample| max.max(sample.abs()));
    let rms = rms_f32(samples);
    let instant = peak.max(rms * 1.8);
    let previous = f32::from_bits(level.load(Ordering::Relaxed));
    let next = if instant > previous {
        instant
    } else {
        previous * 0.86 + instant * 0.14
    };
    level.store(next.to_bits(), Ordering::Relaxed);

    let Ok(mut pipeline) = shared.try_lock() else {
        // The lock is only held briefly by arm/disarm. Skipping a callback is
        // better than blocking the audio thread waiting for it.
        return;
    };

    let resampled = pipeline.resampler.push(samples);
    for frame in pipeline.framer.push(&resampled) {
        if frames.send(frame).is_err() {
            // The receiver is gone; the session has ended.
            return;
        }
    }
}

/// Root-mean-square loudness of a buffer, clamped to 0..=1.
fn rms_f32(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|sample| sample * sample).sum();
    (sum / samples.len() as f32).sqrt().min(1.0)
}

#[cfg(test)]
mod tests {
    use super::rms_f32;

    #[test]
    fn silence_is_zero() {
        assert_eq!(rms_f32(&[0.0, 0.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn a_full_scale_tone_is_loud() {
        assert!(rms_f32(&[1.0, -1.0, 1.0, -1.0]) > 0.9);
    }
}
