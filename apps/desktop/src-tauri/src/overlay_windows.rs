//! Win32 listening pill. No webview, click-through, never activates.

use crate::native_settings::theme;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;
use tauri::AppHandle;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, EndPaint,
    FillRect, FillRgn, GetMonitorInfoW, GetStockObject, InvalidateRect, MonitorFromPoint,
    SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TextOutW, BLACK_PEN, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE, FW_SEMIBOLD, HBRUSH, HDC,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, OUT_TT_PRECIS, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, GetWindowLongPtrW, MoveWindow, RegisterClassW,
    SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow, CS_HREDRAW,
    CS_VREDRAW, GWL_EXSTYLE, HTTRANSPARENT, HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, WM_DESTROY, WM_ERASEBKGND, WM_NCHITTEST, WM_PAINT,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};

use super::{current_level, notice_lock, PHASE};

static HWND_BITS: AtomicIsize = AtomicIsize::new(0);
static SIZE: Mutex<(i32, i32)> = Mutex::new((72, 30));
static BAR_ENV: Mutex<f32> = Mutex::new(0.0);

fn rgb(c: (u8, u8, u8)) -> COLORREF {
    COLORREF(u32::from(c.2) << 16 | u32::from(c.1) << 8 | u32::from(c.0))
}

fn module() -> HINSTANCE {
    unsafe { HINSTANCE(GetModuleHandleW(None).expect("module").0) }
}

fn hwnd() -> HWND {
    HWND(HWND_BITS.load(Ordering::Relaxed) as *mut core::ffi::c_void)
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
            72,
            30,
            HWND::default(),
            windows::Win32::UI::WindowsAndMessaging::HMENU::default(),
            module(),
            None,
        )
        .expect("overlay window");
        // Soft glass opacity — WS_EX_LAYERED needs an alpha before the pill shows.
        let _ = SetLayeredWindowAttributes(window, COLORREF(0), 236, LWA_ALPHA);
        HWND_BITS.store(window.0 as isize, Ordering::Relaxed);
        let _ = SetWindowLongPtrW(window, GWL_EXSTYLE, GetWindowLongPtrW(window, GWL_EXSTYLE));
        round_region(window, 72, 30);
    }
    Ok(())
}

fn round_region(window: HWND, w: i32, h: i32) {
    unsafe {
        let region = CreateRoundRectRgn(0, 0, w + 1, h + 1, h, h);
        let _ = SetWindowRgn(window, region, true);
    }
}

pub fn show(app: &AppHandle, w: i32, h: i32) {
    if let Ok(mut size) = SIZE.lock() {
        *size = (w, h);
    }
    let Some((x, y)) = super::position_over_cursor(app, w, h) else {
        return;
    };
    let window = hwnd();
    if window.0.is_null() {
        return;
    }
    unsafe {
        let _ = SetLayeredWindowAttributes(window, COLORREF(0), 236, LWA_ALPHA);
        round_region(window, w, h);
        let _ = MoveWindow(window, x, y, w, h, true);
        let _ = SetWindowPos(
            window,
            HWND_TOPMOST,
            x,
            y,
            w,
            h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        let _ = InvalidateRect(window, None, true);
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

pub fn cursor_monitor_rect(_app: &AppHandle) -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let mut pt = POINT::default();
        GetCursorPos(&mut pt).ok()?;
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
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

fn paint(window: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(window, &mut ps);
        let (w, h) = SIZE.lock().map(|s| *s).unwrap_or((72, 30));
        let bg = CreateSolidBrush(rgb(theme::OVERLAY_BG_RGB));
        let rect = RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        };
        let _ = FillRect(hdc, &rect, bg);
        let _ = DeleteObject(bg);
        let phase = PHASE.load(Ordering::Relaxed);
        if phase == 3 {
            let text = notice_lock().lock().map(|g| g.clone()).unwrap_or_default();
            draw_notice(hdc, &text, h);
        } else if phase == 1 || phase == 2 {
            draw_bars(hdc, w, h, phase == 2);
        }
        let _ = EndPaint(window, &ps);
    }
}

fn draw_notice(hdc: HDC, text: &str, h: i32) {
    unsafe {
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, rgb(theme::OVERLAY_TEXT_RGB));
        let font = CreateFontW(
            12,
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.0.into(),
            OUT_TT_PRECIS.0.into(),
            CLIP_DEFAULT_PRECIS.0.into(),
            CLEARTYPE_QUALITY.0.into(),
            u32::from(DEFAULT_PITCH.0) | u32::from(FF_DONTCARE.0),
            w!("Segoe UI"),
        );
        let old = SelectObject(hdc, font);
        let wide: Vec<u16> = text.encode_utf16().collect();
        let _ = TextOutW(hdc, 12, (h / 2) - 7, &wide);
        SelectObject(hdc, old);
        let _ = DeleteObject(font);
    }
}

fn draw_bars(hdc: HDC, w: i32, h: i32, thinking: bool) {
    let app = match super::APP.get() {
        Some(app) => app,
        None => return,
    };
    let raw = current_level(app);
    let db = 20.0 * (raw.max(1e-5)).log10();
    let voice = ((db + 48.0) / 40.0).clamp(0.0, 1.0);
    let mut env = BAR_ENV.lock().unwrap_or_else(|e| e.into_inner());
    *env = if voice > *env {
        voice
    } else {
        *env * 0.72 + voice * 0.28
    };
    let envelope = *env;
    drop(env);

    let color = if thinking {
        theme::OVERLAY_MUTED_RGB
    } else {
        theme::OVERLAY_LISTEN_RGB
    };
    unsafe {
        let brush = CreateSolidBrush(rgb(color));
        let old_pen = SelectObject(hdc, GetStockObject(BLACK_PEN));
        let gap = 3;
        let bar_w = 3;
        let count = 5;
        let total = count * bar_w + (count - 1) * gap;
        let start_x = (w - total) / 2;
        let max_h = (h as f32 - 10.0).max(10.0);
        for i in 0..count {
            let wobble = 0.32 + 0.68 * ((i as f32 * 1.41 + envelope * 2.4).sin().abs());
            let floor = if thinking { 0.18 } else { 0.14 };
            let frac = (floor + envelope * wobble).clamp(floor, 1.0);
            let bh = (max_h * frac) as i32;
            let x = start_x + i * (bar_w + gap);
            let y = (h - bh) / 2;
            let rgn = CreateRoundRectRgn(x, y, x + bar_w, y + bh.max(bar_w), bar_w, bar_w);
            let _ = FillRgn(hdc, rgn, brush);
            let _ = DeleteObject(rgn);
        }
        SelectObject(hdc, old_pen);
        let _ = DeleteObject(brush);
    }
}
