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

/// Wispr-small capsule — 72×30 still read large next to the cursor.
const COMPACT_W: i32 = 52;
const COMPACT_H: i32 = 22;

/// 0 idle (hidden), 1 listening, 2 thinking, 3 notice.
static PHASE: AtomicU8 = AtomicU8::new(0);
static NOTICE: OnceLock<Mutex<String>> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();

fn notice_lock() -> &'static Mutex<String> {
    NOTICE.get_or_init(|| Mutex::new(String::new()))
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
    let width = (72.0 + message.len() as f64 * 5.8).clamp(100.0, 240.0) as i32;
    (width, COMPACT_H.max(22))
}

pub fn prepare(app: &AppHandle) -> tauri::Result<()> {
    let _ = APP.set(app.clone());
    platform::create(app)?;
    spawn_level_ticker(app.clone());
    Ok(())
}

pub fn appear_listening(app: &AppHandle) {
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
