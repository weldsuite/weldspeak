//! Windows helpers for push-to-talk.
//!
//! `GetAsyncKeyState` is the hold detector. A `WH_KEYBOARD_LL` callback is the
//! wrong tool here: Windows will skip or silently unhook it when the callback
//! is slow, and WebView2 / Chrome often never deliver it at all — which is
//! exactly when someone is sitting in Settings changing the key.

use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

const VK_ESCAPE: u16 = 0x1B;

pub fn is_down(vk: u16) -> bool {
    down(vk)
}

pub fn is_escape_down() -> bool {
    down(VK_ESCAPE)
}

fn down(vk: u16) -> bool {
    // High bit is the current physical state. Safe to call from any thread.
    unsafe { GetAsyncKeyState(i32::from(vk)) as u16 & 0x8000 != 0 }
}
