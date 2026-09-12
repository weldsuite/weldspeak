//! Windows helpers for push-to-talk.
//!
//! `GetAsyncKeyState` is the hold detector. A `WH_KEYBOARD_LL` callback is the
//! wrong tool here: Windows will skip or silently unhook it when the callback
//! is slow, and WebView2 / Chrome often never deliver it at all — which is
//! exactly when someone is sitting in Settings changing the key.

use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

use super::PttKey;

const VK_LCONTROL: u32 = 0xA2;
const VK_RCONTROL: u32 = 0xA3;
const VK_LMENU: u32 = 0xA4;
const VK_RMENU: u32 = 0xA5;
const VK_F8: u32 = 0x77;
const VK_F13: u32 = 0x7C;

pub fn is_down(key: PttKey) -> bool {
    match key {
        PttKey::ControlRight => down(VK_RCONTROL),
        PttKey::ControlLeft => down(VK_LCONTROL),
        PttKey::AltRight => down(VK_RMENU),
        PttKey::AltLeft => down(VK_LMENU),
        PttKey::F8 => down(VK_F8),
        PttKey::F13 => down(VK_F13),
    }
}

fn down(vk: u32) -> bool {
    // High bit is the current physical state. Safe to call from any thread.
    unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
}
