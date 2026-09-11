//! The on-screen listening pill.
//!
//! Wispr-style feedback: the moment the dictation key goes down, a floating
//! bar appears above the taskbar and shows that the microphone is live. It
//! never takes focus — the caret stays in whatever the user was typing into.

use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};

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

fn reveal(app: &AppHandle, window: &WebviewWindow) {
    position_over_cursor(app, window);
    let _ = window.set_ignore_cursor_events(true);
    let _ = window.set_always_on_top(true);
    let _ = window.show();
}

/// Place the pill on the monitor under the cursor, above the taskbar, and make
/// it click-through so it cannot steal the user's typing.
pub fn prepare(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = window(app) else {
        return Ok(());
    };

    let _ = window.set_ignore_cursor_events(true);
    position_over_cursor(app, &window);
    spawn_level_ticker(app.clone());
    Ok(())
}

/// Key is down: the user should see that they are talking, immediately.
pub fn appear_listening(app: &AppHandle) {
    set_live(app, true);
    if let Some(window) = window(app) {
        reveal(app, &window);
    }
    let _ = app.emit("weldspeak://listening", ());
}

pub fn show_notice(app: &AppHandle, message: &str) {
    set_live(app, false);
    if let Some(window) = window(app) {
        reveal(app, &window);
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
        reveal(app, &window);
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
        let mut interval = tokio::time::interval(Duration::from_millis(50));
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

fn position_over_cursor(app: &AppHandle, window: &WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|point| window.monitor_from_point(point.x, point.y).ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten());

    let Some(monitor) = monitor else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };

    // Bottom centre, with room above the Windows taskbar.
    let origin = monitor.position();
    let area = monitor.size();
    let x = origin.x + (area.width as i32 - size.width as i32) / 2;
    let y = origin.y + area.height as i32 - size.height as i32 - 72;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}
