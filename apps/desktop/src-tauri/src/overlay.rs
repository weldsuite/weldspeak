//! The on-screen listening pill.
//!
//! Wispr-style feedback: the moment the dictation key goes down, a floating
//! bar appears above the taskbar and shows that the microphone is live. It
//! never takes focus — the caret stays in whatever the user was typing into.

use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::window::Color;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Size, WebviewWindow};

/// Waveform only — the Wispr-sized capsule. Grows when partials or notices need type.
const COMPACT_SIZE: LogicalSize<f64> = LogicalSize {
    width: 96.0,
    height: 48.0,
};

use crate::audio::Capture;
use crate::AppState;

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("overlay")
}

fn set_live(app: &AppHandle, live: bool) {
    app.state::<AppState>()
        .overlay_live
        .store(live, Ordering::Relaxed);
}

fn reveal(app: &AppHandle, window: &WebviewWindow, size: LogicalSize<f64>) {
    let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
    let _ = window.set_size(Size::Logical(size));
    position_over_cursor(app, window, size);
    let _ = window.set_ignore_cursor_events(true);
    let _ = window.set_always_on_top(true);
    let _ = window.show();
}

fn notice_size(message: &str) -> LogicalSize<f64> {
    if message.is_empty() {
        return COMPACT_SIZE;
    }
    let width = (92.0 + message.len() as f64 * 6.8).clamp(132.0, 320.0);
    LogicalSize {
        width,
        height: 48.0,
    }
}

/// Place the pill on the monitor under the cursor, above the taskbar, and make
/// it click-through so it cannot steal the user's typing.
pub fn prepare(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = window(app) else {
        return Ok(());
    };

    let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
    let _ = window.set_ignore_cursor_events(true);
    let _ = window.set_size(Size::Logical(COMPACT_SIZE));
    position_over_cursor(app, &window, COMPACT_SIZE);
    spawn_level_ticker(app.clone());
    Ok(())
}

/// Key is down: the user should see that they are talking, immediately.
pub fn appear_listening(app: &AppHandle) {
    set_live(app, true);
    if let Some(window) = window(app) {
        reveal(app, &window, COMPACT_SIZE);
    }
    let _ = app.emit("weldspeak://listening", ());
}

/// Stretch the capsule just enough for the last few recognised words.
pub fn show_partial(app: &AppHandle, preview: &str) {
    let shown = last_words(preview, 6);
    if let Some(window) = window(app) {
        let size = notice_size(&shown);
        let _ = window.set_size(Size::Logical(size));
        position_over_cursor(app, &window, size);
    }
}

fn last_words(text: &str, n: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let start = words.len().saturating_sub(n);
    words[start..].join(" ")
}

pub fn show_notice(app: &AppHandle, message: &str) {
    set_live(app, false);
    if let Some(window) = window(app) {
        reveal(app, &window, notice_size(message));
    }
    let _ = app.emit("weldspeak://notice", message);
    hide_later(app, Duration::from_secs(4));
}

/// Same as [`show_notice`], but the pill stays until the caller hides it.
/// Used while an update is downloading so a 4-second flash is not the last
/// thing the user sees of the process.
pub fn show_status(app: &AppHandle, message: &str) {
    set_live(app, false);
    if let Some(window) = window(app) {
        reveal(app, &window, notice_size(message));
    }
    let _ = app.emit("weldspeak://notice", message);
}

fn hide_later(app: &AppHandle, after: Duration) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(after).await;
        if !app
            .state::<AppState>()
            .overlay_live
            .load(Ordering::Relaxed)
        {
            hide(&app);
        }
    });
}

pub fn dismiss(app: &AppHandle) {
    set_live(app, false);
    let _ = app.emit("weldspeak://done", ());
    hide(app);
}

fn hide(app: &AppHandle) {
    if let Some(window) = window(app) {
        let _ = window.hide();
    }
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
            let level = state
                .capture
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().map(Capture::current_level))
                .unwrap_or(0.0);
            let _ = app.emit("weldspeak://level", level);
        }
    });
}

fn position_over_cursor(app: &AppHandle, window: &WebviewWindow, logical: LogicalSize<f64>) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|point| window.monitor_from_point(point.x, point.y).ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten());

    let Some(monitor) = monitor else {
        return;
    };

    // Use the size we just asked for. `outer_size` lags a frame behind `set_size`,
    // which would leave a growing notice off-centre.
    let scale = monitor.scale_factor();
    let width = (logical.width * scale).round() as i32;
    let height = (logical.height * scale).round() as i32;
    let origin = monitor.position();
    let area = monitor.size();
    let x = origin.x + (area.width as i32 - width) / 2;
    let y = origin.y + area.height as i32 - height - 56;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}
