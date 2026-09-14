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

use super::{notice_lock, PHASE};

static PANEL_PTR: AtomicIsize = AtomicIsize::new(0);

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("AppKit overlay on the main thread")
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
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(72.0, 34.0));
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
    panel.setOpaque(true);
    panel.setHasShadow(true);
    panel.setIgnoresMouseEvents(true);
    panel.setLevel(3); // NSFloatingWindowLevel
    panel.setBackgroundColor(Some(&unsafe {
        NSColor::colorWithCalibratedRed_green_blue_alpha(0.09, 0.086, 0.086, 0.92)
    }));

    let content = panel.contentView().expect("content view");
    let label = unsafe { NSTextField::new(mtm) };
    unsafe {
        label.setEditable(false);
        label.setBezeled(false);
        label.setDrawsBackground(false);
        label.setSelectable(false);
        label.setAlignment(NSTextAlignment::Center);
        label.setTextColor(Some(&NSColor::whiteColor()));
        label.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        label.setFrame(NSRect::new(NSPoint::new(8.0, 6.0), NSSize::new(56.0, 22.0)));
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
                    NSPoint::new(10.0, 6.0),
                    NSSize::new((w as f64) - 20.0, 22.0),
                ));
            }
            let phase = PHASE.load(Ordering::Relaxed);
            let text = if phase == 3 {
                notice_lock().lock().map(|g| g.clone()).unwrap_or_default()
            } else if phase == 2 {
                "…".into()
            } else {
                String::new()
            };
            unsafe {
                label.setStringValue(&NSString::from_str(&text));
            }
        }
    });
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
    LABEL.with(|slot| {
        if let Some(label) = slot.borrow().as_ref() {
            if PHASE.load(Ordering::Relaxed) == 1 {
                unsafe {
                    label.setStringValue(&NSString::from_str(""));
                }
            }
        }
    });
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
