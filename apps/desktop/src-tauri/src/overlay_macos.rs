//! Native macOS listening pill — NSPanel, no WKWebView.
//!
//! The capsule and bars come from the same anti-aliased rasteriser as
//! Windows (`overlay::rasterize`), drawn at the screen's backing scale and
//! handed to Core Animation as the contents of a layer-backed view. The panel
//! itself is transparent, so only the capsule shows. That replaces a square
//! black panel with block glyphs (▁▂▃) standing in for the bars.
//!
//! Notices draw an empty capsule and put an AppKit label over it, so the text
//! gets the system font and subpixel positioning for free.

use core_foundation::base::TCFType;
use core_foundation::data::{CFData, CFDataRef};
use core_foundation::string::CFStringRef;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSFont, NSFontWeightMedium, NSLineBreakMode, NSPanel,
    NSScreen, NSTextAlignment, NSTextField, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
use tauri::AppHandle;

use super::{
    bar_rgb, notice_text, phase, rasterize, sample_bars, Content, BOTTOM_MARGIN, NOTICE_MAX_W,
    NOTICE_PAD, PILL_H, PILL_W,
};

/// Point size of notice text, matching the Windows pill.
const NOTICE_FONT_PT: f64 = 12.0;

/// `kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little`: the
/// premultiplied BGRA that `rasterize` produces.
const BGRA_PREMULTIPLIED: u32 = 2 | (2 << 12);

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    static kCGColorSpaceSRGB: CFStringRef;
    fn CGColorSpaceCreateWithName(name: CFStringRef) -> *mut c_void;
    fn CGColorSpaceRelease(space: *mut c_void);
    fn CGDataProviderCreateWithCFData(data: CFDataRef) -> *mut c_void;
    fn CGDataProviderRelease(provider: *mut c_void);
    fn CGImageCreate(
        width: usize,
        height: usize,
        bits_per_component: usize,
        bits_per_pixel: usize,
        bytes_per_row: usize,
        space: *mut c_void,
        bitmap_info: u32,
        provider: *mut c_void,
        decode: *const f64,
        should_interpolate: bool,
        intent: i32,
    ) -> *mut c_void;
    fn CGImageRelease(image: *mut c_void);
}

static PANEL_PTR: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    static PILL: RefCell<Option<Retained<NSView>>> = const { RefCell::new(None) };
    static LABEL: RefCell<Option<Retained<NSTextField>>> = const { RefCell::new(None) };
}

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
        // Transparent window: the rasterised capsule is the only thing drawn.
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
    }
    panel.setOpaque(false);
    // No shadow, as on Windows. AppKit traces it from the previous frame, so
    // it lagged a notice's new width.
    panel.setHasShadow(false);
    panel.setIgnoresMouseEvents(true);
    panel.setLevel(3); // NSFloatingWindowLevel

    let content = panel.contentView().expect("content view");

    // A plain layer-backed view does no drawing of its own, so AppKit leaves
    // the layer contents we set alone.
    let pill = unsafe { NSView::initWithFrame(mtm.alloc(), rect) };
    pill.setWantsLayer(true);

    let label = unsafe { NSTextField::new(mtm) };
    unsafe {
        label.setEditable(false);
        label.setBezeled(false);
        label.setDrawsBackground(false);
        label.setSelectable(false);
        label.setAlignment(NSTextAlignment::Center);
        label.setUsesSingleLineMode(true);
        label.setLineBreakMode(NSLineBreakMode::NSLineBreakByTruncatingTail);
        label.setTextColor(Some(&rgb(
            crate::native_settings::theme::OVERLAY_TEXT_RGB,
        )));
        label.setFont(Some(&NSFont::systemFontOfSize_weight(
            NOTICE_FONT_PT,
            NSFontWeightMedium,
        )));
        label.setStringValue(&NSString::from_str(""));
        label.setHidden(true);
        content.addSubview(&pill);
        content.addSubview(&label);
    }

    PILL.with(|slot| *slot.borrow_mut() = Some(pill));
    LABEL.with(|slot| *slot.borrow_mut() = Some(label));
    PANEL_PTR.store(Retained::as_ptr(&panel) as isize, Ordering::Relaxed);
    std::mem::forget(panel);
    Ok(())
}

/// Width of `text` in the notice font, in points.
fn measure_notice(text: &str) -> Option<f64> {
    LABEL.with(|slot| {
        let borrow = slot.borrow();
        let label = borrow.as_ref()?;
        unsafe {
            label.setStringValue(&NSString::from_str(text));
            Some(label.sizeThatFits(NSSize::new(f64::MAX, f64::MAX)).width)
        }
    })
}

/// Draw the current frame into the pill view's layer.
fn paint(app: &AppHandle) {
    let Some(panel) = panel() else {
        return;
    };
    let size = panel.frame().size;
    let scale = panel.backingScaleFactor().max(1.0);
    let width = (size.width * scale).round() as usize;
    let height = (size.height * scale).round() as usize;
    if width == 0 || height == 0 {
        return;
    }

    let pixels = match phase() {
        // The label draws the text; the capsule behind it is empty.
        3 => rasterize(
            width,
            height,
            scale as f32,
            &Content::Mask(&vec![0; width * height]),
        ),
        p @ (1 | 2) => rasterize(
            width,
            height,
            scale as f32,
            &Content::Bars {
                lengths: sample_bars(app),
                rgb: bar_rgb(p == 2),
            },
        ),
        _ => return,
    };

    PILL.with(|slot| {
        if let Some(pill) = slot.borrow().as_ref() {
            set_layer_image(pill, width, height, &pixels);
        }
    });
}

/// Wrap premultiplied BGRA pixels in a CGImage and make it the view's layer
/// contents. The layer stretches it over the view's bounds, so an image at
/// backing resolution lands one pixel per device pixel.
fn set_layer_image(view: &NSView, width: usize, height: usize, pixels: &[u8]) {
    let data = CFData::from_buffer(pixels);
    unsafe {
        let space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
        let provider = CGDataProviderCreateWithCFData(data.as_concrete_TypeRef());
        let image = CGImageCreate(
            width,
            height,
            8,
            32,
            width * 4,
            space,
            BGRA_PREMULTIPLIED,
            provider,
            std::ptr::null(),
            false,
            0, // kCGRenderingIntentDefault
        );
        CGDataProviderRelease(provider);
        CGColorSpaceRelease(space);
        if image.is_null() {
            return;
        }

        let layer: *mut AnyObject = msg_send![view, layer];
        if !layer.is_null() {
            // Swap the frame in without Core Animation's implicit cross-fade,
            // which would smear the bars across frames.
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let _: () = msg_send![layer, setContents: image.cast::<AnyObject>()];
            let _: () = msg_send![class!(CATransaction), commit];
        }
        // The layer holds its own reference.
        CGImageRelease(image);
    }
}

pub fn show(app: &AppHandle) {
    let for_main = app.clone();
    let _ = app.run_on_main_thread(move || show_on_main(&for_main));
}

fn show_on_main(app: &AppHandle) {
    let Some(panel) = panel() else {
        return;
    };
    let notice = phase() == 3;
    let height = PILL_H as f64;
    let text_width = if notice {
        measure_notice(&notice_text())
    } else {
        None
    };
    let width = match text_width {
        Some(text) => {
            (text.ceil() + 2.0 * NOTICE_PAD as f64).clamp(height * 3.0, NOTICE_MAX_W as f64)
        }
        None => PILL_W as f64,
    };
    let Some((x, y)) =
        super::position_over_cursor(app, width as i32, height as i32, BOTTOM_MARGIN as i32)
    else {
        return;
    };
    // Cocoa origin is bottom-left.
    panel.setFrame_display(
        NSRect::new(NSPoint::new(x as f64, y as f64), NSSize::new(width, height)),
        true,
    );

    let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
    PILL.with(|slot| {
        if let Some(pill) = slot.borrow().as_ref() {
            unsafe { pill.setFrame(bounds) };
        }
    });
    LABEL.with(|slot| {
        if let Some(label) = slot.borrow().as_ref() {
            unsafe {
                // NSTextField has no vertical centring, so size it to one
                // line and centre that inside the capsule.
                let line = label.sizeThatFits(NSSize::new(f64::MAX, f64::MAX)).height;
                let pad = NOTICE_PAD as f64;
                label.setFrame(NSRect::new(
                    NSPoint::new(pad, ((height - line) / 2.0).round()),
                    NSSize::new((width - 2.0 * pad).max(0.0), line),
                ));
            }
            label.setHidden(!notice);
        }
    });

    paint(app);
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

/// Called ~60 times a second from the ticker task. The pill view lives in a
/// main-thread thread-local, so the frame must be drawn there.
pub fn repaint(app: &AppHandle) {
    let for_main = app.clone();
    let _ = app.run_on_main_thread(move || paint(&for_main));
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
