//! Native macOS listening pill — NSPanel, no WKWebView.
//!
//! Geometry, colours and the bar model are shared with Windows through
//! `overlay.rs`. The bars are still drawn as block glyphs in a label; a
//! drawn capsule (as on Windows) needs a custom NSView.

use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSFont, NSPanel, NSScreen, NSTextAlignment, NSTextField,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};
use tauri::AppHandle;

use super::{
    notice_text, phase, sample_bars, BAR_COUNT, BOTTOM_MARGIN, NOTICE_MAX_W, NOTICE_PAD, PILL_H,
    PILL_W,
};

/// Tallest a bar gets (2 px × 5 × 1.5), for mapping lengths onto glyphs.
const MAX_BAR: f32 = 15.0;

static PANEL_PTR: AtomicIsize = AtomicIsize::new(0);

fn mtm() -> MainThreadMarker {
    // `MainThreadMarker::new()` requires the `NSThread` feature, which this
    // crate does not enable. Every caller of `mtm()` in this module already
    // runs on the main thread (panel setup or AppKit event handling).
    // Safety: only called from AppKit setup / main-thread overlay code.
    unsafe { MainThreadMarker::new_unchecked() }
}

fn panel() -> Option<Retained<NSPanel>> {
    let bits = PANEL_PTR.load(Ordering::Relaxed);
    if bits == 0 {
        return None;
    }
    // Safety: created on the main thread and only used there.
    unsafe { Retained::retain(bits as *mut NSPanel) }
}

fn rgb(c: (u8, u8, u8)) -> Retained<NSColor> {
    unsafe {
        NSColor::colorWithCalibratedRed_green_blue_alpha(
            c.0 as f64 / 255.0,
            c.1 as f64 / 255.0,
            c.2 as f64 / 255.0,
            1.0,
        )
    }
}

pub fn create(_app: &AppHandle) -> tauri::Result<()> {
    let mtm = mtm();
    let rect = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(PILL_W as f64, PILL_H as f64),
    );
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let panel = unsafe {
        NSPanel::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            rect,
            style,
            NSBackingStoreType::NSBackingStoreBuffered,
            false,
        )
    };
    unsafe {
        panel.setFloatingPanel(true);
        panel.setHidesOnDeactivate(false);
        panel.setReleasedWhenClosed(false);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
    }
    panel.setOpaque(false);
    panel.setHasShadow(true);
    panel.setIgnoresMouseEvents(true);
    panel.setLevel(3); // NSFloatingWindowLevel
    panel.setBackgroundColor(Some(&rgb(crate::native_settings::theme::OVERLAY_BG_RGB)));

    let content = panel.contentView().expect("content view");
    let label = unsafe { NSTextField::new(mtm) };
    unsafe {
        label.setEditable(false);
        label.setBezeled(false);
        label.setDrawsBackground(false);
        label.setSelectable(false);
        label.setAlignment(NSTextAlignment::Center);
        label.setTextColor(Some(&rgb(
            crate::native_settings::theme::OVERLAY_LISTEN_RGB,
        )));
        label.setFont(Some(&NSFont::boldSystemFontOfSize(11.0)));
        label.setFrame(NSRect::new(
            NSPoint::new(8.0, 6.0),
            NSSize::new(PILL_W as f64 - 16.0, PILL_H as f64 - 12.0),
        ));
        label.setStringValue(&NSString::from_str(""));
        content.addSubview(&label);
    }

    LABEL.with(|slot| *slot.borrow_mut() = Some(label));
    PANEL_PTR.store(Retained::as_ptr(&panel) as isize, Ordering::Relaxed);
    std::mem::forget(panel);
    Ok(())
}

thread_local! {
    static LABEL: RefCell<Option<Retained<NSTextField>>> = const { RefCell::new(None) };
}

fn waveform_glyphs(lengths: &[f32; BAR_COUNT]) -> String {
    const STEPS: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    lengths
        .iter()
        .map(|&len| {
            let frac = (len / MAX_BAR).clamp(0.0, 1.0);
            let idx = ((frac * (STEPS.len() - 1) as f32).round() as usize).min(STEPS.len() - 1);
            STEPS[idx]
        })
        .collect()
}

fn update_label(app: &AppHandle) {
    LABEL.with(|slot| {
        let borrow = slot.borrow();
        let Some(label) = borrow.as_ref() else {
            return;
        };
        let (text, color) = match phase() {
            3 => (
                notice_text(),
                crate::native_settings::theme::OVERLAY_TEXT_RGB,
            ),
            p @ (1 | 2) => (waveform_glyphs(&sample_bars(app)), super::bar_rgb(p == 2)),
            _ => (
                String::new(),
                crate::native_settings::theme::OVERLAY_LISTEN_RGB,
            ),
        };
        unsafe {
            label.setTextColor(Some(&rgb(color)));
            label.setStringValue(&NSString::from_str(&text));
        }
    });
}

pub fn show(app: &AppHandle) {
    let for_main = app.clone();
    let _ = app.run_on_main_thread(move || show_on_main(&for_main));
}

fn show_on_main(app: &AppHandle) {
    let Some(panel) = panel() else {
        return;
    };
    let height = PILL_H as i32;
    let width = if phase() == 3 {
        let estimate = notice_text().chars().count() as f32 * 7.0 + 2.0 * NOTICE_PAD;
        estimate.clamp(PILL_H * 3.0, NOTICE_MAX_W) as i32
    } else {
        PILL_W as i32
    };
    let Some((x, y)) = super::position_over_cursor(app, width, height, BOTTOM_MARGIN as i32) else {
        return;
    };
    // Cocoa origin is bottom-left.
    panel.setFrame_display(
        NSRect::new(
            NSPoint::new(x as f64, y as f64),
            NSSize::new(width as f64, height as f64),
        ),
        true,
    );
    LABEL.with(|slot| {
        if let Some(label) = slot.borrow().as_ref() {
            unsafe {
                label.setFrame(NSRect::new(
                    NSPoint::new(8.0, 6.0),
                    NSSize::new((width as f64) - 16.0, (height as f64) - 12.0),
                ));
            }
        }
    });
    update_label(app);
    unsafe {
        panel.orderFrontRegardless();
    }
}

pub fn hide() {
    // Callers may be on any thread; AppKit must be touched on the main one.
    if let Some(app) = super::APP.get() {
        let _ = app.run_on_main_thread(|| {
            if let Some(panel) = panel() {
                panel.orderOut(None);
            }
        });
    }
}

/// Called ~60 times a second from the ticker task. The label lives in a
/// main-thread thread-local, so the update must hop there — updating from the
/// ticker thread found no label and the bars never moved.
pub fn repaint(app: &AppHandle) {
    let for_main = app.clone();
    let _ = app.run_on_main_thread(move || update_label(&for_main));
}

pub fn cursor_monitor_rect(_app: &AppHandle) -> Option<(i32, i32, i32, i32)> {
    let mtm = mtm();
    let mouse = unsafe { NSEvent::mouseLocation() };
    let screens = NSScreen::screens(mtm);
    for i in 0..screens.count() {
        let screen = unsafe { screens.objectAtIndex(i) };
        let frame = screen.frame();
        if mouse.x >= frame.origin.x
            && mouse.x <= frame.origin.x + frame.size.width
            && mouse.y >= frame.origin.y
            && mouse.y <= frame.origin.y + frame.size.height
        {
            let vis = screen.visibleFrame();
            return Some((
                vis.origin.x as i32,
                vis.origin.y as i32,
                vis.size.width as i32,
                vis.size.height as i32,
            ));
        }
    }
    let vis = NSScreen::mainScreen(mtm)?.visibleFrame();
    Some((
        vis.origin.x as i32,
        vis.origin.y as i32,
        vis.size.width as i32,
        vis.size.height as i32,
    ))
}
