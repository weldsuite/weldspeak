//! Win32 listening pill. No webview, click-through, never activates.
//!
//! The pill is a per-pixel-alpha layered window: `overlay::rasterize` draws an
//! anti-aliased capsule into a 32-bit DIB and `UpdateLayeredWindow` hands it
//! to the compositor. That replaces the old window region + GDI fill, which
//! gave the capsule stair-stepped edges and flickered at 60 Hz.
//!
//! Sizes are logical pixels scaled by the DPI of the monitor under the cursor;
//! the process is per-monitor DPI aware, so an unscaled pill shrank to a
//! sliver on a 150 % laptop screen.

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;
use tauri::AppHandle;
use windows::core::w;
use windows::Win32::Foundation::{
    COLORREF, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetDC,
    GetMonitorInfoW, GetTextExtentPoint32W, MonitorFromPoint, ReleaseDC, SelectObject, SetBkMode,
    SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, BLENDFUNCTION, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS,
    DT_CENTER, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_MEDIUM,
    HBITMAP, HBRUSH, HDC, HFONT, HGDIOBJ, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    OUT_TT_PRECIS, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, RegisterClassW, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW, HTTRANSPARENT, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_NCHITTEST, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::{
    bar_rgb, notice_text, phase, rasterize, sample_bars, Content, BOTTOM_MARGIN, NOTICE_MAX_W,
    NOTICE_PAD, PILL_H, PILL_W,
};

/// Logical point size of notice text.
const NOTICE_FONT_PT: f32 = 12.0;

static HWND_BITS: AtomicIsize = AtomicIsize::new(0);

/// Where the pill currently sits, in physical pixels, and its scale.
#[derive(Clone, Copy)]
struct Geometry {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    scale: f32,
}

static GEOMETRY: Mutex<Option<Geometry>> = Mutex::new(None);

fn module() -> HINSTANCE {
    unsafe { HINSTANCE(GetModuleHandleW(None).expect("module").0) }
}

fn hwnd() -> HWND {
    HWND(HWND_BITS.load(Ordering::Relaxed) as *mut c_void)
}

fn px(logical: f32, scale: f32) -> i32 {
    (logical * scale).round() as i32
}

pub fn create(_app: &AppHandle) -> tauri::Result<()> {
    unsafe {
        let class = w!("WeldSpeakOverlay");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: module(),
            lpszClassName: class,
            hbrBackground: HBRUSH::default(),
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        let window = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!(""),
            WS_POPUP,
            0,
            0,
            PILL_W as i32,
            PILL_H as i32,
            HWND::default(),
            windows::Win32::UI::WindowsAndMessaging::HMENU::default(),
            module(),
            None,
        )
        .expect("overlay window");
        HWND_BITS.store(window.0 as isize, Ordering::Relaxed);
    }
    Ok(())
}

/// Show (or resize) the pill for the current phase.
pub fn show(app: &AppHandle) {
    let window = hwnd();
    if window.0.is_null() {
        return;
    }
    let scale = cursor_monitor().map(monitor_scale).unwrap_or(1.0);
    let height = px(PILL_H, scale);
    let width = if phase() == 3 {
        // Size notices to the text actually drawn rather than a per-character
        // guess, which clipped wide glyphs and padded narrow ones.
        measure_notice(&notice_text(), scale)
            .map(|text| {
                (text + 2 * px(NOTICE_PAD, scale)).clamp(height * 3, px(NOTICE_MAX_W, scale))
            })
            .unwrap_or(px(PILL_W, scale))
    } else {
        px(PILL_W, scale)
    };
    let Some((x, y)) = super::position_over_cursor(app, width, height, px(BOTTOM_MARGIN, scale))
    else {
        return;
    };

    if let Ok(mut geometry) = GEOMETRY.lock() {
        *geometry = Some(Geometry {
            x,
            y,
            width,
            height,
            scale,
        });
    }
    repaint(app);
    unsafe {
        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            window,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        );
    }
}

pub fn hide() {
    let window = hwnd();
    if !window.0.is_null() {
        unsafe {
            let _ = ShowWindow(window, SW_HIDE);
        }
    }
}

/// Draw the current frame and push it to the compositor. Safe from any
/// thread: `UpdateLayeredWindow` sends no messages to the window.
pub fn repaint(app: &AppHandle) {
    let window = hwnd();
    let Some(geometry) = GEOMETRY.lock().ok().and_then(|g| *g) else {
        return;
    };
    if window.0.is_null() || geometry.width <= 0 || geometry.height <= 0 {
        return;
    }
    let (w, h) = (geometry.width as usize, geometry.height as usize);

    let pixels = match phase() {
        3 => {
            let mask = render_notice_mask(&notice_text(), w, h, geometry.scale);
            rasterize(w, h, geometry.scale, &Content::Mask(&mask))
        }
        p @ (1 | 2) => rasterize(
            w,
            h,
            geometry.scale,
            &Content::Bars {
                lengths: sample_bars(app),
                rgb: bar_rgb(p == 2),
            },
        ),
        _ => return,
    };
    present(window, geometry, &pixels);
}

/// A top-down 32-bit DIB and the memory DC it is selected into.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    len: usize,
}

impl Surface {
    fn new(width: usize, height: usize) -> Option<Self> {
        unsafe {
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    // Negative height: rows run top to bottom, matching `rasterize`.
                    biHeight: -(height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let dc = CreateCompatibleDC(HDC::default());
            if dc.is_invalid() {
                return None;
            }
            let mut bits: *mut c_void = std::ptr::null_mut();
            let Ok(bitmap) =
                CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, HANDLE::default(), 0)
            else {
                let _ = DeleteDC(dc);
                return None;
            };
            if bits.is_null() {
                let _ = DeleteObject(bitmap);
                let _ = DeleteDC(dc);
                return None;
            }
            let old = SelectObject(dc, bitmap);
            Some(Self {
                dc,
                bitmap,
                old,
                bits: bits.cast(),
                len: width * height * 4,
            })
        }
    }

    fn bytes(&mut self) -> &mut [u8] {
        // Safety: `bits` points at the DIB's `len` bytes for this surface's life.
        unsafe { std::slice::from_raw_parts_mut(self.bits, self.len) }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.dc);
        }
    }
}

fn present(window: HWND, geometry: Geometry, pixels: &[u8]) {
    let Some(mut surface) = Surface::new(geometry.width as usize, geometry.height as usize) else {
        return;
    };
    surface.bytes().copy_from_slice(pixels);
    unsafe {
        let screen = GetDC(HWND::default());
        let origin = POINT {
            x: geometry.x,
            y: geometry.y,
        };
        let size = SIZE {
            cx: geometry.width,
            cy: geometry.height,
        };
        let source = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            window,
            screen,
            Some(&origin),
            Some(&size),
            surface.dc,
            Some(&source),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        ReleaseDC(HWND::default(), screen);
    }
}

fn notice_font(scale: f32) -> HFONT {
    unsafe {
        CreateFontW(
            -px(NOTICE_FONT_PT, scale),
            0,
            0,
            0,
            FW_MEDIUM.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.0.into(),
            OUT_TT_PRECIS.0.into(),
            CLIP_DEFAULT_PRECIS.0.into(),
            // Greyscale anti-aliasing: the glyph coverage becomes the alpha
            // mask. ClearType's colour fringes would not survive that.
            ANTIALIASED_QUALITY.0.into(),
            u32::from(DEFAULT_PITCH.0) | u32::from(FF_DONTCARE.0),
            w!("Segoe UI Variable Text"),
        )
    }
}

fn measure_notice(text: &str, scale: f32) -> Option<i32> {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let surface = Surface::new(1, 1)?;
    unsafe {
        let font = notice_font(scale);
        let old = SelectObject(surface.dc, font);
        let mut size = SIZE::default();
        let ok = GetTextExtentPoint32W(surface.dc, &wide, &mut size).as_bool();
        SelectObject(surface.dc, old);
        let _ = DeleteObject(font);
        ok.then_some(size.cx)
    }
}

/// White-on-black text, read back as per-pixel coverage.
fn render_notice_mask(text: &str, width: usize, height: usize, scale: f32) -> Vec<u8> {
    let Some(mut surface) = Surface::new(width, height) else {
        return vec![0; width * height];
    };
    unsafe {
        let font = notice_font(scale);
        let old = SelectObject(surface.dc, font);
        SetBkMode(surface.dc, TRANSPARENT);
        SetTextColor(surface.dc, COLORREF(0x00ff_ffff));
        let pad = px(NOTICE_PAD, scale);
        let mut rect = RECT {
            left: pad,
            top: 0,
            right: width as i32 - pad,
            bottom: height as i32,
        };
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let _ = DrawTextW(
            surface.dc,
            &mut wide,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(surface.dc, old);
        let _ = DeleteObject(font);
    }
    // GDI leaves alpha at zero; the green channel carries the coverage.
    surface
        .bytes()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| px[1])
        .collect()
}

fn cursor_monitor() -> Option<HMONITOR> {
    unsafe {
        let mut pt = POINT::default();
        GetCursorPos(&mut pt).ok()?;
        Some(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST))
    }
}

fn monitor_scale(monitor: HMONITOR) -> f32 {
    let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
    unsafe {
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_err() {
            return 1.0;
        }
    }
    (dpi_x as f32 / 96.0).clamp(1.0, 4.0)
}

pub fn cursor_monitor_rect(_app: &AppHandle) -> Option<(i32, i32, i32, i32)> {
    let monitor = cursor_monitor()?;
    unsafe {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let r = info.rcWork;
        Some((r.left, r.top, r.right - r.left, r.bottom - r.top))
    }
}

unsafe extern "system" fn wnd_proc(
    window: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        _ => unsafe { DefWindowProcW(window, msg, wparam, lparam) },
    }
}
