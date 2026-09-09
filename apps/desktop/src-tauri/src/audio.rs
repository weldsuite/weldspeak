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
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use weldspeak_core::{Frame, Framer, Resampler};
use weldspeak_protocol::audio::SAMPLE_RATE;

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
    _shutdown: Sender<()>,
}

struct Pipeline {
    resampler: Resampler,
    framer: Framer,
}

impl Capture {
    /// Open the default input device and begin conditioning audio.
    ///
    /// Frames are sent to `frames` only while armed; before that they feed the
    /// pre-roll buffer and are discarded as they age out.
    pub fn start(frames: Sender<Frame>) -> Result<Self> {
        let (shutdown, shutdown_rx) = channel::<()>();
        // The audio thread reports whether the device opened, so a missing or
        // refused microphone surfaces here rather than as silence later.
        let (ready, ready_rx) = channel::<Result<Arc<Mutex<Pipeline>>>>();

        std::thread::Builder::new()
            .name("weldspeak-audio".into())
            .spawn(move || {
                let stream = match Self::open(frames) {
                    Ok((stream, shared)) => {
                        let _ = ready.send(Ok(shared));
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

        let shared = ready_rx.recv()??;
        Ok(Self { shared, _shutdown: shutdown })
    }

    /// Open the default input device. Runs on the audio thread.
    fn open(frames: Sender<Frame>) -> Result<(Stream, Arc<Mutex<Pipeline>>)> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow!("no microphone available"))?;

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
            resampler: Resampler::new(
                config.sample_rate.0,
                config.channels as usize,
                SAMPLE_RATE,
            ),
            framer: Framer::new(),
        }));

        let stream = Self::build_stream(&device, &config, sample_format, shared.clone(), frames)?;
        stream.play()?;

        Ok((stream, shared))
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
        frames: Sender<Frame>,
    ) -> Result<Stream> {
        // An error on the audio thread must not take the process down: the user
        // may simply have unplugged a headset mid-sentence.
        let on_error = |error| tracing::error!(?error, "audio stream error");

        let stream = match format {
            SampleFormat::F32 => device.build_input_stream(
                config,
                move |data: &[f32], _| process(&shared, &frames, data),
                on_error,
                None,
            )?,
            SampleFormat::I16 => {
                let convert = |data: &[i16]| -> Vec<f32> {
                    data.iter().map(|s| s.to_sample::<f32>()).collect()
                };
                device.build_input_stream(
                    config,
                    move |data: &[i16], _| process(&shared, &frames, &convert(data)),
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
                    move |data: &[u16], _| process(&shared, &frames, &convert(data)),
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
fn process(shared: &Arc<Mutex<Pipeline>>, frames: &Sender<Frame>, samples: &[f32]) {
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
