//! Win32 listening pill. No webview, click-through, never activates.
//!
//! Sizes arrive from `overlay.rs` in logical pixels and are scaled here by the
//! DPI of the monitor under the cursor: the process is per-monitor DPI aware,
//! so without scaling the pill shrank to a sliver on a 150% laptop screen.
//! Painting goes through an off-screen bitmap so the ~60 Hz waveform redraw
//! does not flicker.

use crate::native_settings::theme;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;
use tauri::AppHandle;
use windows::core::w;
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
    CreateRoundRectRgn, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, GetDC,
    GetMonitorInfoW, GetStockObject, GetTextExtentPoint32W, InvalidateRect, MonitorFromPoint,
    ReleaseDC, RoundRect, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_END_ELLIPSIS, DT_NOPREFIX,
    DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_MEDIUM, HBRUSH, HDC, HFONT, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, NULL_PEN, OUT_TT_PRECIS, PAINTSTRUCT, PS_SOLID, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, RegisterClassW, SetLayeredWindowAttributes,
    SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, HTTRANSPARENT, HWND_TOPMOST, LWA_ALPHA,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, WM_DESTROY, WM_ERASEBKGND,
    WM_NCHITTEST, WM_PAINT, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::{notice_lock, sample_bars, BAR_COUNT, PHASE};

/// Near-opaque: enough to read over busy backgrounds, a hint of glass.
const ALPHA: u8 = 242;
/// Logical point size of notice text.
const NOTICE_FONT_PT: f32 = 12.0;
/// Logical horizontal padding either side of notice text.
const NOTICE_PAD: f32 = 14.0;

static HWND_BITS: AtomicIsize = AtomicIsize::new(0);

/// Physical width, height, and the scale they were computed with.
static GEOMETRY: Mutex<(i32, i32, f32)> = Mutex::new((84, 22, 1.0));

fn rgb(c: (u8, u8, u8)) -> COLORREF {
    COLORREF(u32::from(c.2) << 16 | u32::from(c.1) << 8 | u32::from(c.0))
}

fn module() -> HINSTANCE {
    unsafe { HINSTANCE(GetModuleHandleW(None).expect("module").0) }
}

fn hwnd() -> HWND {
    HWND(HWND_BITS.load(Ordering::Relaxed) as *mut core::ffi::c_void)
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
            84,
            22,
            HWND::default(),
            windows::Win32::UI::WindowsAndMessaging::HMENU::default(),
            module(),
            None,
        )
        .expect("overlay window");
        // WS_EX_LAYERED needs an alpha before the pill shows.
        let _ = SetLayeredWindowAttributes(window, COLORREF(0), ALPHA, LWA_ALPHA);
        HWND_BITS.store(window.0 as isize, Ordering::Relaxed);
        round_region(window, 84, 22);
    }
    Ok(())
}

fn round_region(window: HWND, w: i32, h: i32) {
    unsafe {
        let region = CreateRoundRectRgn(0, 0, w + 1, h + 1, h, h);
        // The system owns the region after this call; it must not be deleted.
        let _ = SetWindowRgn(window, region, true);
    }
}

/// Show the pill. `w` and `h` are logical pixels.
pub fn show(app: &AppHandle, w: i32, h: i32) {
    let window = hwnd();
    if window.0.is_null() {
        return;
    }
    let scale = cursor_monitor().map(monitor_scale).unwrap_or(1.0);
    let height = px(h as f32, scale);
    let mut width = px(w as f32, scale);

    // Size notices to the text actually drawn rather than a per-character guess,
    // which clipped wide glyphs and padded narrow ones.
    if PHASE.load(Ordering::Relaxed) == 3 {
        let text = notice_lock().lock().map(|g| g.clone()).unwrap_or_default();
        if let Some(text_width) = measure_notice(window, &text, scale) {
            let padded = text_width + 2 * px(NOTICE_PAD, scale);
            width = padded.clamp(height * 3, px(420.0, scale));
        }
    }

    if let Ok(mut geometry) = GEOMETRY.lock() {
        *geometry = (width, height, scale);
    }
    let Some((x, y)) = super::position_over_cursor(app, width, height) else {
        return;
    };
    unsafe {
        round_region(window, width, height);
        let _ = SetWindowPos(
            window,
            HWND_TOPMOST,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        let _ = InvalidateRect(window, None, false);
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

pub fn repaint() {
    let window = hwnd();
    if !window.0.is_null() {
        unsafe {
            let _ = InvalidateRect(window, None, false);
        }
    }
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
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(window);
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => unsafe { DefWindowProcW(window, msg, wparam, lparam) },
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
            CLEARTYPE_QUALITY.0.into(),
            u32::from(DEFAULT_PITCH.0) | u32::from(FF_DONTCARE.0),
            w!("Segoe UI Variable Text"),
        )
    }
}

fn measure_notice(window: HWND, text: &str, scale: f32) -> Option<i32> {
    let wide: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        let hdc = GetDC(window);
        if hdc.is_invalid() {
            return None;
        }
        let font = notice_font(scale);
        let old = SelectObject(hdc, font);
        let mut size = SIZE::default();
        let ok = GetTextExtentPoint32W(hdc, &wide, &mut size).as_bool();
        SelectObject(hdc, old);
        let _ = DeleteObject(font);
        ReleaseDC(window, hdc);
        ok.then_some(size.cx)
    }
}

fn paint(window: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let screen = BeginPaint(window, &mut ps);
        let (w, h, scale) = GEOMETRY.lock().map(|g| *g).unwrap_or((84, 22, 1.0));

        let buffer = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let old_bitmap = SelectObject(buffer, bitmap);

        draw_capsule(buffer, w, h);
        match PHASE.load(Ordering::Relaxed) {
            1 => draw_bars(buffer, w, h, scale, false),
            2 => draw_bars(buffer, w, h, scale, true),
            3 => {
                let text = notice_lock().lock().map(|g| g.clone()).unwrap_or_default();
                draw_notice(buffer, &text, w, h, scale);
            }
            _ => {}
        }

        let _ = BitBlt(screen, 0, 0, w, h, buffer, 0, 0, SRCCOPY);
        SelectObject(buffer, old_bitmap);
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(buffer);
        let _ = EndPaint(window, &ps);
    }
}

fn draw_capsule(hdc: HDC, w: i32, h: i32) {
    unsafe {
        let fill = CreateSolidBrush(rgb(theme::OVERLAY_BG_RGB));
        let edge = CreatePen(PS_SOLID, 1, rgb(theme::OVERLAY_BORDER_RGB));
        let old_brush = SelectObject(hdc, fill);
        let old_pen = SelectObject(hdc, edge);
        let _ = RoundRect(hdc, 0, 0, w, h, h, h);
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        let _ = DeleteObject(fill);
        let _ = DeleteObject(edge);
    }
}

fn draw_notice(hdc: HDC, text: &str, w: i32, h: i32, scale: f32) {
    unsafe {
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, rgb(theme::OVERLAY_TEXT_RGB));
        let font = notice_font(scale);
        let old = SelectObject(hdc, font);
        let pad = px(NOTICE_PAD, scale);
        let mut rect = RECT {
            left: pad,
            top: 0,
            right: w - pad,
            bottom: h,
        };
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let _ = DrawTextW(
            hdc,
            &mut wide,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(hdc, old);
        let _ = DeleteObject(font);
    }
}

fn draw_bars(hdc: HDC, w: i32, h: i32, scale: f32, thinking: bool) {
    let Some(app) = super::APP.get() else {
        return;
    };
    let heights = sample_bars(app, thinking);

    let color = if thinking {
        theme::OVERLAY_MUTED_RGB
    } else {
        theme::OVERLAY_LISTEN_RGB
    };
    unsafe {
        let brush = CreateSolidBrush(rgb(color));
        let old_brush = SelectObject(hdc, brush);
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        let bar_w = px(2.0, scale).max(2);
        let gap = px(3.0, scale);
        let count = BAR_COUNT as i32;
        let total = count * bar_w + (count - 1) * gap;
        let start_x = (w - total) / 2;
        let max_h = (h - px(8.0, scale)).max(bar_w * 2) as f32;
        for (i, frac) in heights.iter().enumerate() {
            let bh = ((max_h * frac).round() as i32).max(bar_w);
            let x = start_x + i as i32 * (bar_w + gap);
            let y = (h - bh) / 2;
            // NULL_PEN leaves the right/bottom edge undrawn, hence the +1.
            let _ = RoundRect(hdc, x, y, x + bar_w + 1, y + bh + 1, bar_w, bar_w);
        }
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
        let _ = DeleteObject(brush);
    }
}
