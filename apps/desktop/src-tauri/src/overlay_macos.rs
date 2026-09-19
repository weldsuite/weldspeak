//! Native macOS listening pill — NSPanel, no WKWebView.

use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSFont, NSPanel, NSScreen, NSTextAlignment, NSTextField,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};
use tauri::AppHandle;

use super::{notice_lock, sample_bars, PHASE};

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

pub fn create(_app: &AppHandle) -> tauri::Result<()> {
    let mtm = mtm();
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(84.0, 22.0));
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
    let (br, bg, bb) = crate::native_settings::theme::OVERLAY_BG_RGB;
    let panel_bg = unsafe {
        NSColor::colorWithCalibratedRed_green_blue_alpha(
            br as f64 / 255.0,
            bg as f64 / 255.0,
            bb as f64 / 255.0,
            0.90,
        )
    };
    panel.setBackgroundColor(Some(&panel_bg));

    let content = panel.contentView().expect("content view");
    let label = unsafe { NSTextField::new(mtm) };
    unsafe {
        label.setEditable(false);
        label.setBezeled(false);
        label.setDrawsBackground(false);
        label.setSelectable(false);
        label.setAlignment(NSTextAlignment::Center);
        let (lr, lg, lb) = crate::native_settings::theme::OVERLAY_LISTEN_RGB;
        let label_fg = NSColor::colorWithCalibratedRed_green_blue_alpha(
            lr as f64 / 255.0,
            lg as f64 / 255.0,
            lb as f64 / 255.0,
            1.0,
        );
        label.setTextColor(Some(&label_fg));
        label.setFont(Some(&NSFont::boldSystemFontOfSize(11.0)));
        label.setFrame(NSRect::new(NSPoint::new(8.0, 2.0), NSSize::new(68.0, 18.0)));
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

fn waveform_glyphs(heights: &[f32], thinking: bool) -> String {
    // Twelve steps (was eight) so smoothed bar heights don't quantize as hard.
    const STEPS: [&str; 12] = ["▁", "▂", "▂", "▃", "▄", "▄", "▅", "▆", "▆", "▇", "▇", "█"];
    let _ = thinking;
    heights
        .iter()
        .map(|&frac| {
            let idx = ((frac * (STEPS.len() - 1) as f32).round() as usize).min(STEPS.len() - 1);
            STEPS[idx]
        })
        .collect::<Vec<_>>()
        .join("")
}

fn update_label(app: Option<&AppHandle>) {
    LABEL.with(|slot| {
        let borrow = slot.borrow();
        let Some(label) = borrow.as_ref() else {
            return;
        };
        let phase = PHASE.load(Ordering::Relaxed);
        let (text, orange) = if phase == 3 {
            (
                notice_lock().lock().map(|g| g.clone()).unwrap_or_default(),
                false,
            )
        } else if phase == 1 || phase == 2 {
            let thinking = phase == 2;
            let heights = if let Some(app) = app {
                sample_bars(app, thinking)
            } else {
                [0.2; super::BAR_COUNT]
            };
            (waveform_glyphs(&heights, thinking), !thinking)
        } else {
            (String::new(), true)
        };
        let color = if orange {
            let (r, g, b) = crate::native_settings::theme::OVERLAY_LISTEN_RGB;
            unsafe {
                NSColor::colorWithCalibratedRed_green_blue_alpha(
                    r as f64 / 255.0,
                    g as f64 / 255.0,
                    b as f64 / 255.0,
                    1.0,
                )
            }
        } else {
            let (r, g, b) = crate::native_settings::theme::OVERLAY_TEXT_RGB;
            unsafe {
                NSColor::colorWithCalibratedRed_green_blue_alpha(
                    r as f64 / 255.0,
                    g as f64 / 255.0,
                    b as f64 / 255.0,
                    1.0,
                )
            }
        };
        unsafe {
            label.setTextColor(Some(&color));
            label.setStringValue(&NSString::from_str(&text));
        }
    });
}

pub fn show(app: &AppHandle, w: i32, h: i32) {
    let Some(panel) = panel() else {
        return;
    };
    let Some((x, y)) = super::position_over_cursor(app, w, h) else {
        return;
    };
    // Cocoa origin is bottom-left.
    panel.setFrame_display(
        NSRect::new(
            NSPoint::new(x as f64, y as f64),
            NSSize::new(w as f64, h as f64),
        ),
        true,
    );
    LABEL.with(|slot| {
        if let Some(label) = slot.borrow().as_ref() {
            unsafe {
                label.setFrame(NSRect::new(
                    NSPoint::new(8.0, 2.0),
                    NSSize::new((w as f64) - 16.0, (h as f64) - 4.0),
                ));
            }
        }
    });
    update_label(Some(app));
    unsafe {
        panel.orderFrontRegardless();
    }
}

pub fn hide() {
    if let Some(panel) = panel() {
        panel.orderOut(None);
    }
}

pub fn repaint() {
    let app = super::APP.get();
    update_label(app);
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
