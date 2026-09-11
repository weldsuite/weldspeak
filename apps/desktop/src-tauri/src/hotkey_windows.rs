//! Push-to-talk on Windows via a low-level keyboard hook.
//!
//! `RegisterHotKey` (what Tauri's shortcut plugin uses) does not deliver a
//! modifier held on its own. Wispr-style dictation needs exactly that, so we
//! watch `WH_KEYBOARD_LL` on a dedicated thread with its own message loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tauri::AppHandle;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::PttKey;

static APP: OnceLock<AppHandle> = OnceLock::new();
static HELD: AtomicBool = AtomicBool::new(false);

const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_LCONTROL: u32 = 0xA2;
const VK_RCONTROL: u32 = 0xA3;
const VK_LMENU: u32 = 0xA4;
const VK_RMENU: u32 = 0xA5;
const VK_F8: u32 = 0x77;
const VK_F13: u32 = 0x7C;
/// Bit 0 of `KBDLLHOOKSTRUCT.flags`.
const LLKHF_EXTENDED: u32 = 0x01;

pub fn install(app: AppHandle) {
    if APP.set(app).is_err() {
        return;
    }

    std::thread::Builder::new()
        .name("weldspeak-ptt".into())
        .spawn(|| unsafe { message_loop() })
        .expect("failed to start the push-to-talk hook");
}

fn matches(vk: u32, flags: u32, key: PttKey) -> bool {
    let extended = flags & LLKHF_EXTENDED != 0;
    match key {
        PttKey::ControlRight => vk == VK_RCONTROL || (vk == VK_CONTROL && extended),
        PttKey::ControlLeft => vk == VK_LCONTROL || (vk == VK_CONTROL && !extended),
        PttKey::AltRight => vk == VK_RMENU || (vk == VK_MENU && extended),
        PttKey::AltLeft => vk == VK_LMENU || (vk == VK_MENU && !extended),
        PttKey::F8 => vk == VK_F8,
        PttKey::F13 => vk == VK_F13,
    }
}

fn dispatch(down: bool) {
    let Some(app) = APP.get() else {
        return;
    };
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if down {
            crate::dictation::begin(&app);
        } else {
            crate::dictation::end(&app);
        }
    });
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 {
        // SAFETY: WH_KEYBOARD_LL delivers a KBDLLHOOKSTRUCT pointer in lParam.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if let Some(key) = super::current_key() {
            if matches(info.vkCode, info.flags.0, key) {
                let message = wparam.0 as u32;
                let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
                let up = message == WM_KEYUP || message == WM_SYSKEYUP;
                if down {
                    if !HELD.swap(true, Ordering::SeqCst) {
                        dispatch(true);
                    }
                    return LRESULT(1);
                }
                if up {
                    if HELD.swap(false, Ordering::SeqCst) {
                        dispatch(false);
                    }
                    return LRESULT(1);
                }
            }
        }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe fn message_loop() {
    let module = match GetModuleHandleW(None) {
        Ok(module) => module,
        Err(error) => {
            tracing::error!(%error, "could not start the push-to-talk hook");
            return;
        }
    };

    // The hook lives for the process lifetime; the module handle only has to
    // remain valid for this call. `Option<&HINSTANCE>` is what windows 0.58
    // asks for here.
    let instance = HINSTANCE::from(module);
    let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(&instance), 0) {
        Ok(hook) => hook,
        Err(error) => {
            tracing::error!(%error, "could not install the push-to-talk hook");
            return;
        }
    };

    tracing::info!("push-to-talk keyboard hook installed");

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    let _ = UnhookWindowsHookEx(hook);
}
