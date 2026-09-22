//! Native listening pill — no webview.
//!
//! A small rounded window above the taskbar. Click-through, never focused.
//! Bars follow the microphone; notices can widen the capsule.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Manager, PhysicalPosition};

use crate::audio::Capture;
use crate::AppState;

/// Wispr-like thin pill: wider than the 52×22 shrink so the waveform can
/// breathe, still compact in height (72×30 was oversized next to the cursor).
const COMPACT_W: i32 = 84;
const COMPACT_H: i32 = 22;

/// Bars drawn while listening / thinking.
pub(crate) const BAR_COUNT: usize = 7;

/// 0 idle (hidden), 1 listening, 2 thinking, 3 notice.
static PHASE: AtomicU8 = AtomicU8::new(0);
static NOTICE: OnceLock<Mutex<String>> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
static WAVE: OnceLock<Mutex<WaveState>> = OnceLock::new();

#[derive(Clone, Copy)]
struct WaveState {
    envelope: f32,
    bars: [f32; BAR_COUNT],
    /// Continuous phase so bars keep flowing when the mic level is steady.
    phase: f32,
}

impl WaveState {
    const fn idle() -> Self {
        Self {
            envelope: 0.12,
            bars: [0.16; BAR_COUNT],
            phase: 0.0,
        }
    }
}

fn notice_lock() -> &'static Mutex<String> {
    NOTICE.get_or_init(|| Mutex::new(String::new()))
}

fn wave_lock() -> &'static Mutex<WaveState> {
    WAVE.get_or_init(|| Mutex::new(WaveState::idle()))
}

fn set_live(app: &AppHandle, live: bool) {
    app.state::<AppState>()
        .overlay_live
        .store(live, Ordering::Relaxed);
}

fn notice_size(message: &str) -> (i32, i32) {
    if message.is_empty() {
        return (COMPACT_W, COMPACT_H);
    }
    let width = (88.0 + message.len() as f64 * 5.8).clamp(110.0, 260.0) as i32;
    (width, COMPACT_H.max(22))
}

pub fn prepare(app: &AppHandle) -> tauri::Result<()> {
    let _ = APP.set(app.clone());
    platform::create(app)?;
    spawn_level_ticker(app.clone());
    Ok(())
}

pub fn appear_listening(app: &AppHandle) {
    reset_wave();
    set_live(app, true);
    PHASE.store(1, Ordering::Relaxed);
    platform::show(app, COMPACT_W, COMPACT_H);
}

pub fn appear_thinking(app: &AppHandle) {
    set_live(app, false);
    PHASE.store(2, Ordering::Relaxed);
    platform::show(app, COMPACT_W, COMPACT_H);
}

pub fn show_notice(app: &AppHandle, message: &str) {
    set_live(app, false);
    if let Ok(mut guard) = notice_lock().lock() {
        *guard = message.to_string();
    }
    PHASE.store(3, Ordering::Relaxed);
    let (w, h) = notice_size(message);
    platform::show(app, w, h);
    hide_later(app, Duration::from_secs(4));
}

pub fn show_status(app: &AppHandle, message: &str) {
    set_live(app, false);
    if let Ok(mut guard) = notice_lock().lock() {
        *guard = message.to_string();
    }
    PHASE.store(3, Ordering::Relaxed);
    let (w, h) = notice_size(message);
    platform::show(app, w, h);
}

pub fn dismiss(app: &AppHandle) {
    set_live(app, false);
    PHASE.store(0, Ordering::Relaxed);
    reset_wave();
    platform::hide();
}

fn hide_later(app: &AppHandle, after: Duration) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(after).await;
        if !app.state::<AppState>().overlay_live.load(Ordering::Relaxed) {
            dismiss(&app);
        }
    });
}

fn spawn_level_ticker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // ~60 Hz redraw; bar heights are interpolated so motion stays fluid.
        let mut interval = tokio::time::interval(Duration::from_millis(16));
        loop {
            interval.tick().await;
            let state = app.state::<AppState>();
            if !state.overlay_live.load(Ordering::Relaxed) {
                continue;
            }
            platform::repaint();
        }
    });
}

fn current_level(app: &AppHandle) -> f32 {
    app.state::<AppState>()
        .capture
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(Capture::current_level))
        .unwrap_or(0.0)
}

fn reset_wave() {
    if let Ok(mut wave) = wave_lock().lock() {
        *wave = WaveState::idle();
    }
}

fn voice_from_raw(raw: f32) -> f32 {
    let db = 20.0 * (raw.max(1e-5)).log10();
    ((db + 48.0) / 40.0).clamp(0.0, 1.0)
}

/// Soft attack + softer release — no instant envelope jumps.
fn smooth_toward(current: f32, target: f32, attack: f32, release: f32) -> f32 {
    let alpha = if target > current { attack } else { release };
    current + (target - current) * alpha
}

/// Advance shared waveform state and return smoothed per-bar heights (0..=1).
pub(crate) fn sample_bars(app: &AppHandle, thinking: bool) -> [f32; BAR_COUNT] {
    let voice = voice_from_raw(current_level(app));
    let mut wave = wave_lock().lock().unwrap_or_else(|e| e.into_inner());

    wave.envelope = smooth_toward(wave.envelope, voice, 0.34, 0.10);
    // Drift phase with speech energy so idle bars still breathe a little.
    let drift = 0.085 + wave.envelope * 0.22;
    wave.phase = (wave.phase + drift) % (std::f32::consts::TAU * 8.0);

    let floor = if thinking { 0.16 } else { 0.12 };
    for i in 0..BAR_COUNT {
        let wobble = 0.30
            + 0.70
                * ((i as f32 * 1.31 + wave.phase + wave.envelope * 0.9)
                    .sin()
                    .abs());
        let target = (floor + wave.envelope * wobble).clamp(floor, 1.0);
        wave.bars[i] = smooth_toward(wave.bars[i], target, 0.40, 0.16);
    }

    wave.bars
}

fn position_over_cursor(app: &AppHandle, width: i32, height: i32) -> Option<(i32, i32)> {
    platform::cursor_monitor_rect(app).map(|(x, y, w, h)| {
        let px = x + (w - width) / 2;
        // Windows work-area coords are top-left; Cocoa visibleFrame is bottom-left.
        #[cfg(target_os = "macos")]
        let py = {
            let _ = (h, height);
            y + 56
        };
        #[cfg(not(target_os = "macos"))]
        let py = y + h - height - 56;
        (px, py)
    })
}

#[allow(dead_code)]
fn physical_position(x: i32, y: i32) -> PhysicalPosition<i32> {
    PhysicalPosition::new(x, y)
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
    pub fn show(_app: &AppHandle, _w: i32, _h: i32) {}
    pub fn hide() {}
    pub fn repaint() {}
    pub fn cursor_monitor_rect(_app: &AppHandle) -> Option<(i32, i32, i32, i32)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{smooth_toward, BAR_COUNT, COMPACT_H, COMPACT_W};

    #[test]
    fn compact_pill_is_wider_than_height() {
        const _: () = assert!(COMPACT_W > COMPACT_H);
        const _: () = assert!(COMPACT_W >= 76);
        const _: () = assert!(COMPACT_H <= 24);
        assert_eq!(BAR_COUNT, 7);
        // Touch the values so the test still exercises the public constants.
        assert_eq!(COMPACT_W, 84);
        assert_eq!(COMPACT_H, 22);
    }

    #[test]
    fn smooth_toward_eases_both_directions() {
        let up = smooth_toward(0.2, 0.9, 0.34, 0.10);
        assert!(up > 0.2 && up < 0.9);
        let down = smooth_toward(0.9, 0.2, 0.34, 0.10);
        assert!(down < 0.9 && down > 0.2);
        assert!((up - 0.2) > (0.9 - down)); // attack faster than release
    }
}
