//! Native listening pill — no webview.
//!
//! Modelled on Wispr Flow's bottom-docked pill, measured from its shipped
//! stylesheet rather than eyeballed:
//!
//! - Listening: a 73 × 30 black capsule with a 1 px `#30302f` rim.
//! - Ten 2 px bars, 2 px apart. Each bar is a 2 px dot scaled vertically by
//!   `max(1, 5 × level) × envelope × ripple`, so silence reads as a row of
//!   breathing dots and speech lifts the middle bars most.
//! - Envelope: `1 − d² / 48`, `d` = distance from the centre bar.
//! - Ripple: a 1 s ease-in-out loop through ×1, 1.2, 1.5, 1.1, 1.3, 1,
//!   staggered 0.1 s per bar outward from the centre.
//! - Level: loudness per 40 ms in dBFS, mapped onto a 20 dB window above an
//!   adaptive noise floor, averaged per 150 ms and eased per frame.
//!
//! The adaptive floor is what makes the bars move for every microphone. A
//! fixed dB window left quiet mics looking dead and loud ones pinned.
//!
//! Drawing is a small anti-aliased rasteriser shared by the platforms, so the
//! capsule edge and bar ends are smooth rather than GDI's stair-stepped
//! regions.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

use crate::audio::Capture;
use crate::native_settings::theme;
use crate::AppState;

/// Listening capsule, logical pixels.
pub(crate) const PILL_W: f32 = 73.0;
pub(crate) const PILL_H: f32 = 30.0;
/// Horizontal padding either side of notice text.
pub(crate) const NOTICE_PAD: f32 = 14.0;
/// Widest a notice may grow.
pub(crate) const NOTICE_MAX_W: f32 = 420.0;
/// Gap between the pill and the bottom of the work area (above the taskbar).
pub(crate) const BOTTOM_MARGIN: f32 = 14.0;

/// Bars drawn while listening / thinking.
pub(crate) const BAR_COUNT: usize = 10;
const BAR_W: f32 = 2.0;
const BAR_GAP: f32 = 2.0;
/// Unscaled bar length; everything else multiplies this.
const BAR_BASE: f32 = 2.0;
/// `--audio-scale` gain in Wispr Flow's dictation mode.
const LEVEL_GAIN: f32 = 5.0;
/// Loudness above the noise floor that counts as full scale.
const LEVEL_RANGE_DB: f32 = 20.0;
/// Lowest the adaptive floor may fall.
const FLOOR_MIN_DB: f32 = -60.0;
/// Window over which readings are averaged before the bars chase them.
const AVERAGE_WINDOW: Duration = Duration::from_millis(150);
/// Fraction of the previous value kept per 60 Hz frame.
const EASE_RETAIN: f32 = 0.85;
/// Ripple keyframes: (phase, multiplier).
const RIPPLE: [(f32, f32); 6] = [
    (0.0, 1.0),
    (0.2, 1.2),
    (0.4, 1.5),
    (0.8, 1.1),
    (0.9, 1.3),
    (1.0, 1.0),
];
const RIPPLE_PERIOD_S: f32 = 1.0;
const RIPPLE_STAGGER_S: f32 = 0.1;

/// 0 idle (hidden), 1 listening, 2 thinking, 3 notice.
static PHASE: AtomicU8 = AtomicU8::new(0);
static NOTICE: OnceLock<Mutex<String>> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
static WAVE: OnceLock<Mutex<WaveState>> = OnceLock::new();

/// Everything the waveform remembers between frames.
struct WaveState {
    /// Adaptive noise floor, dBFS. Starts at 0 each dictation and falls to the
    /// quietest reading seen, so the pre-roll's room tone sets it.
    floor_db: f32,
    last_seq: u32,
    /// Scaled readings since the last average.
    window: Vec<f32>,
    window_started: Instant,
    /// Level the bars are easing toward.
    target: f32,
    /// Eased level actually drawn.
    level: f32,
    last_frame: Instant,
    /// Ripple clock origin.
    epoch: Instant,
}

impl WaveState {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            floor_db: 0.0,
            last_seq: 0,
            window: Vec::with_capacity(8),
            window_started: now,
            target: 0.0,
            level: 0.0,
            last_frame: now,
            epoch: now,
        }
    }

    /// Fold in the latest microphone reading if it is new.
    fn observe(&mut self, seq: u32, db: f32) {
        if seq == 0 || seq == self.last_seq {
            return;
        }
        self.last_seq = seq;
        if db < self.floor_db {
            self.floor_db = db.max(FLOOR_MIN_DB);
        }
        self.window
            .push(((db - self.floor_db) / LEVEL_RANGE_DB).clamp(0.0, 1.0));
    }

    /// Advance one frame and return the eased level.
    fn step(&mut self, now: Instant, live: bool) -> f32 {
        if !live {
            // Thinking: the bars settle to their resting size but keep rippling.
            self.target = 0.0;
            self.window.clear();
        } else if now.duration_since(self.window_started) >= AVERAGE_WINDOW {
            if !self.window.is_empty() {
                self.target = self.window.iter().sum::<f32>() / self.window.len() as f32;
                self.window.clear();
            }
            self.window_started = now;
        }

        let frames = now.duration_since(self.last_frame).as_secs_f32() * 60.0;
        self.last_frame = now;
        let retain = EASE_RETAIN.powf(frames.clamp(0.0, 6.0));
        self.level = self.level * retain + self.target * (1.0 - retain);
        self.level
    }
}

fn notice_lock() -> &'static Mutex<String> {
    NOTICE.get_or_init(|| Mutex::new(String::new()))
}

fn wave_lock() -> &'static Mutex<WaveState> {
    WAVE.get_or_init(|| Mutex::new(WaveState::new()))
}

fn set_live(app: &AppHandle, live: bool) {
    app.state::<AppState>()
        .overlay_live
        .store(live, Ordering::Relaxed);
}

/// Current phase, for the platform renderers.
pub(crate) fn phase() -> u8 {
    PHASE.load(Ordering::Relaxed)
}

pub(crate) fn notice_text() -> String {
    notice_lock().lock().map(|g| g.clone()).unwrap_or_default()
}

pub fn prepare(app: &AppHandle) -> tauri::Result<()> {
    let _ = APP.set(app.clone());
    platform::create(app)?;
    spawn_ticker(app.clone());
    Ok(())
}

pub fn appear_listening(app: &AppHandle) {
    reset_wave();
    set_live(app, true);
    PHASE.store(1, Ordering::Relaxed);
    platform::show(app);
}

pub fn appear_thinking(app: &AppHandle) {
    set_live(app, false);
    PHASE.store(2, Ordering::Relaxed);
    platform::show(app);
}

pub fn show_notice(app: &AppHandle, message: &str) {
    show_status(app, message);
    hide_later(app, Duration::from_secs(4));
}

pub fn show_status(app: &AppHandle, message: &str) {
    set_live(app, false);
    if let Ok(mut guard) = notice_lock().lock() {
        *guard = message.to_string();
    }
    PHASE.store(3, Ordering::Relaxed);
    platform::show(app);
}

pub fn dismiss(app: &AppHandle) {
    set_live(app, false);
    PHASE.store(0, Ordering::Relaxed);
    platform::hide();
}

fn hide_later(app: &AppHandle, after: Duration) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(after).await;
        if phase() == 3 {
            dismiss(&app);
        }
    });
}

fn spawn_ticker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(16));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            // Thinking animates too: the bars keep rippling while the text is
            // cleaned up, as Wispr Flow's do.
            if matches!(phase(), 1 | 2) {
                platform::repaint(&app);
            }
        }
    });
}

fn latest_reading(app: &AppHandle) -> Option<(u32, f32)> {
    app.state::<AppState>()
        .capture
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(Capture::latest_chunk_db))
}

fn reset_wave() {
    if let Ok(mut wave) = wave_lock().lock() {
        let last_seq = wave.last_seq;
        *wave = WaveState::new();
        // Readings taken before this dictation must not count toward it.
        wave.last_seq = last_seq;
    }
}

/// Advance the waveform one frame and return each bar's length in logical
/// pixels.
pub(crate) fn sample_bars(app: &AppHandle) -> [f32; BAR_COUNT] {
    let live = phase() == 1;
    let reading = if live { latest_reading(app) } else { None };
    let mut wave = wave_lock().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((seq, db)) = reading {
        wave.observe(seq, db);
    }
    let now = Instant::now();
    let level = wave.step(now, live);
    let t = now.duration_since(wave.epoch).as_secs_f32();
    bar_lengths(level, t)
}

/// Bar lengths for an eased `level` (0–1) at ripple time `t` seconds.
pub(crate) fn bar_lengths(level: f32, t: f32) -> [f32; BAR_COUNT] {
    let scale = (LEVEL_GAIN * level).max(1.0);
    let centre = (BAR_COUNT as f32 - 1.0) / 2.0;
    let half = BAR_COUNT.div_ceil(2);
    let mut out = [0.0; BAR_COUNT];
    for (i, slot) in out.iter_mut().enumerate() {
        let distance = (centre - i as f32).abs();
        let envelope = (1.0 - distance * distance / 48.0).max(0.0);
        let offset = if i < half {
            i as f32
        } else {
            i as f32 - BAR_COUNT as f32
        };
        let phase = ((t - RIPPLE_STAGGER_S * offset) / RIPPLE_PERIOD_S).rem_euclid(1.0);
        *slot = BAR_BASE * scale * envelope * ripple(phase);
    }
    out
}

/// Ripple multiplier at `phase` (0–1), eased within each keyframe segment.
fn ripple(phase: f32) -> f32 {
    for pair in RIPPLE.windows(2) {
        let (p0, v0) = pair[0];
        let (p1, v1) = pair[1];
        if phase <= p1 {
            let u = ((phase - p0) / (p1 - p0)).clamp(0.0, 1.0);
            let eased = u * u * (3.0 - 2.0 * u);
            return v0 + (v1 - v0) * eased;
        }
    }
    RIPPLE[RIPPLE.len() - 1].1
}

/// What goes inside the capsule.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) enum Content<'a> {
    Bars {
        lengths: [f32; BAR_COUNT],
        rgb: (u8, u8, u8),
    },
    /// Text coverage, one byte per pixel, same size as the surface.
    Mask(&'a [u8]),
}

/// Rasterise the pill into premultiplied BGRA, `width × height` physical
/// pixels at `scale` physical pixels per logical pixel.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn rasterize(width: usize, height: usize, scale: f32, content: &Content) -> Vec<u8> {
    let mut pixels = vec![0u8; width * height * 4];
    let (w, h) = (width as f32, height as f32);
    let radius = h / 2.0;
    let rim = scale.max(1.0);
    let (fill, edge) = (theme::OVERLAY_BG_RGB, theme::OVERLAY_BORDER_RGB);

    // Bar rectangles, physical pixels: (centre x, half width, half length).
    let bars: Vec<(f32, f32, f32)> = match content {
        Content::Bars { lengths, .. } => {
            let bar_w = BAR_W * scale;
            let pitch = (BAR_W + BAR_GAP) * scale;
            let total = pitch * BAR_COUNT as f32 - BAR_GAP * scale;
            let left = (w - total) / 2.0 + bar_w / 2.0;
            lengths
                .iter()
                .enumerate()
                .map(|(i, len)| {
                    let length = (len * scale).clamp(scale, h - 8.0 * scale);
                    (left + pitch * i as f32, bar_w / 2.0, length / 2.0)
                })
                .collect()
        }
        Content::Mask(_) => Vec::new(),
    };
    let bar_rgb = match content {
        Content::Bars { rgb, .. } => *rgb,
        Content::Mask(_) => theme::OVERLAY_TEXT_RGB,
    };
    let bar_radius = 0.5 * scale;

    for y in 0..height {
        let py = y as f32 + 0.5;
        for x in 0..width {
            let px = x as f32 + 0.5;
            let d = rounded_rect_sdf(px - w / 2.0, py - h / 2.0, w / 2.0, h / 2.0, radius);
            let outer = coverage(d);
            if outer <= 0.0 {
                continue;
            }
            let inner = coverage(d + rim);
            let mut rgb = mix(edge, fill, inner);

            let ink = match content {
                Content::Mask(mask) => f32::from(mask[y * width + x]) / 255.0,
                Content::Bars { .. } => bars
                    .iter()
                    .map(|&(cx, hw, hl)| {
                        coverage(rounded_rect_sdf(
                            px - cx,
                            py - h / 2.0,
                            hw,
                            hl,
                            bar_radius.min(hw),
                        ))
                    })
                    .fold(0.0, f32::max),
            };
            if ink > 0.0 {
                rgb = mix(rgb, to_f32(bar_rgb), ink * inner);
            }

            let i = (y * width + x) * 4;
            pixels[i] = (rgb.2 * outer).round() as u8;
            pixels[i + 1] = (rgb.1 * outer).round() as u8;
            pixels[i + 2] = (rgb.0 * outer).round() as u8;
            pixels[i + 3] = (255.0 * outer).round() as u8;
        }
    }
    pixels
}

/// Signed distance from a point (relative to the rectangle's centre) to a
/// rounded rectangle with half extents `hw` × `hh`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn rounded_rect_sdf(x: f32, y: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = x.abs() - (hw - r);
    let qy = y.abs() - (hh - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn coverage(distance: f32) -> f32 {
    (0.5 - distance).clamp(0.0, 1.0)
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn to_f32(c: (u8, u8, u8)) -> (f32, f32, f32) {
    (f32::from(c.0), f32::from(c.1), f32::from(c.2))
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn mix<A: Into<Rgb>, B: Into<Rgb>>(a: A, b: B, t: f32) -> (f32, f32, f32) {
    let (a, b) = (a.into().0, b.into().0);
    (
        a.0 + (b.0 - a.0) * t,
        a.1 + (b.1 - a.1) * t,
        a.2 + (b.2 - a.2) * t,
    )
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
struct Rgb((f32, f32, f32));

impl From<(u8, u8, u8)> for Rgb {
    fn from(c: (u8, u8, u8)) -> Self {
        Rgb(to_f32(c))
    }
}

impl From<(f32, f32, f32)> for Rgb {
    fn from(c: (f32, f32, f32)) -> Self {
        Rgb(c)
    }
}

/// Bar colour for the current phase: white while the mic is live, dimmed
/// while thinking (Wispr Flow draws idle bars at 40 % white).
pub(crate) fn bar_rgb(thinking: bool) -> (u8, u8, u8) {
    if thinking {
        theme::OVERLAY_MUTED_RGB
    } else {
        theme::OVERLAY_LISTEN_RGB
    }
}

/// Top-left of a `width × height` pill centred at the bottom of the work area
/// under the cursor. `margin` is in the same units as the rectangle.
fn position_over_cursor(
    app: &AppHandle,
    width: i32,
    height: i32,
    margin: i32,
) -> Option<(i32, i32)> {
    platform::cursor_monitor_rect(app).map(|(x, y, w, h)| {
        let px = x + (w - width) / 2;
        // Windows work-area coords are top-left; Cocoa visibleFrame is bottom-left.
        #[cfg(target_os = "macos")]
        let py = {
            let _ = (h, height);
            y + margin
        };
        #[cfg(not(target_os = "macos"))]
        let py = y + h - height - margin;
        (px, py)
    })
}

#[cfg(target_os = "windows")]
#[path = "overlay_windows.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "overlay_macos.rs"]
mod platform;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    use tauri::AppHandle;
    pub fn create(_app: &AppHandle) -> tauri::Result<()> {
        Ok(())
    }
    pub fn show(_app: &AppHandle) {}
    pub fn hide() {}
    pub fn repaint(_app: &AppHandle) {}
    pub fn cursor_monitor_rect(_app: &AppHandle) -> Option<(i32, i32, i32, i32)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_draws_resting_dots() {
        let bars = bar_lengths(0.0, 0.0);
        assert_eq!(bars.len(), 10);
        // Scale floors at 1, so every bar is at most the 2 px base × 1.5 ripple.
        assert!(bars.iter().all(|&b| b > 0.0 && b <= BAR_BASE * 1.5 + 1e-4));
    }

    #[test]
    fn speech_lifts_the_middle_most() {
        let bars = bar_lengths(1.0, 0.0);
        let middle = bars[4].max(bars[5]);
        assert!(middle > bars[0] * 1.3, "{bars:?}");
        // Full level: 2 px × 5 × ~1 × ripple, well above the resting dots.
        assert!(middle >= 9.0, "{bars:?}");
    }

    #[test]
    fn ripple_follows_its_keyframes() {
        assert!((ripple(0.0) - 1.0).abs() < 1e-6);
        assert!((ripple(0.4) - 1.5).abs() < 1e-6);
        assert!((ripple(1.0) - 1.0).abs() < 1e-6);
        assert!(ripple(0.3) > 1.2 && ripple(0.3) < 1.5);
    }

    #[test]
    fn floor_adapts_to_the_room() {
        let mut wave = WaveState::new();
        // Room tone at −50 dB sets the floor; speech 20 dB above reads as full.
        wave.observe(1, -50.0);
        wave.observe(2, -30.0);
        assert_eq!(wave.window, vec![0.0, 1.0]);
        // A stale sequence number is ignored.
        wave.observe(2, -10.0);
        assert_eq!(wave.window.len(), 2);
    }

    #[test]
    fn floor_never_drops_below_minus_sixty() {
        let mut wave = WaveState::new();
        wave.observe(1, -200.0);
        assert_eq!(wave.floor_db, FLOOR_MIN_DB);
    }

    #[test]
    fn capsule_is_opaque_inside_and_clear_at_the_corners() {
        let (w, h) = (73, 30);
        let pixels = rasterize(
            w,
            h,
            1.0,
            &Content::Bars {
                lengths: [2.0; BAR_COUNT],
                rgb: (255, 255, 255),
            },
        );
        let alpha = |x: usize, y: usize| pixels[(y * w + x) * 4 + 3];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(w / 2, 2), 255);
        // Centre of the middle gap is black fill; a bar centre is white.
        let px = |x: usize, y: usize| pixels[(y * w + x) * 4];
        assert!(px(20, h / 2) < 40, "fill should be near black");
    }
}
