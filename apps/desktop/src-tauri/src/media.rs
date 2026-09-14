//! Quiet other audio while the user is speaking.
//!
//! Wispr Flow pauses or mutes music and videos for the length of a dictation
//! so the microphone is not fighting playback, then puts them back. On Windows
//! that is: pause anything the system media controls know about, and mute any
//! remaining playback sessions (Chrome, games) that do not expose those
//! controls.

use tauri::{AppHandle, Manager};

use crate::AppState;

#[derive(Default)]
pub struct MediaPause {
    paused_apps: Vec<String>,
    /// Windows WASAPI sessions muted for the duration of a dictation.
    /// Unused on macOS, where only AppleScript pause/play is available.
    #[allow(dead_code)]
    muted_pids: Vec<u32>,
}

pub fn pause_if_enabled(app: &AppHandle) {
    let enabled = app
        .state::<AppState>()
        .settings
        .lock()
        .map(|settings| settings.pause_media)
        .unwrap_or(false);
    if !enabled {
        return;
    }

    let paused = platform::pause();
    if let Ok(mut slot) = app.state::<AppState>().media.lock() {
        *slot = paused;
    }
}

pub fn resume(app: &AppHandle) {
    let paused = app
        .state::<AppState>()
        .media
        .lock()
        .ok()
        .map(|mut slot| std::mem::take(&mut *slot))
        .unwrap_or_default();
    platform::resume(paused);
}

#[cfg(target_os = "windows")]
mod platform {
    use super::MediaPause;
    use windows::core::Interface;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    };
    use windows::Win32::Foundation::TRUE;
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, IAudioSessionControl, IAudioSessionControl2, IAudioSessionEnumerator,
        IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    pub fn pause() -> MediaPause {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        MediaPause {
            paused_apps: pause_smtc(),
            muted_pids: mute_other_sessions(),
        }
    }

    pub fn resume(state: MediaPause) {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        unmute_sessions(&state.muted_pids);
        resume_smtc(&state.paused_apps);
    }

    fn pause_smtc() -> Vec<String> {
        let Ok(manager) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .and_then(|op| op.get())
        else {
            return Vec::new();
        };
        let Ok(sessions) = manager.GetSessions() else {
            return Vec::new();
        };
        let mut paused = Vec::new();
        let count = sessions.Size().unwrap_or(0);
        for index in 0..count {
            let Ok(session) = sessions.GetAt(index) else {
                continue;
            };
            let playing = session
                .GetPlaybackInfo()
                .ok()
                .and_then(|info| info.PlaybackStatus().ok())
                .is_some_and(|status| {
                    status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing
                });
            if !playing {
                continue;
            }
            let id = session
                .SourceAppUserModelId()
                .map(|value| value.to_string())
                .unwrap_or_default();
            if session.TryPauseAsync().and_then(|op| op.get()).is_ok() {
                paused.push(id);
            }
        }
        paused
    }

    fn resume_smtc(apps: &[String]) {
        if apps.is_empty() {
            return;
        }
        let Ok(manager) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .and_then(|op| op.get())
        else {
            return;
        };
        let Ok(sessions) = manager.GetSessions() else {
            return;
        };
        let count = sessions.Size().unwrap_or(0);
        for index in 0..count {
            let Ok(session) = sessions.GetAt(index) else {
                continue;
            };
            let id = session
                .SourceAppUserModelId()
                .map(|value| value.to_string())
                .unwrap_or_default();
            if apps.iter().any(|app| app == &id) {
                let _ = session.TryPlayAsync().and_then(|op| op.get());
            }
        }
    }

    fn mute_other_sessions() -> Vec<u32> {
        let us = std::process::id();
        let mut muted = Vec::new();
        let Ok(enumerator) = (unsafe {
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        }) else {
            return muted;
        };
        let Ok(device) = (unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia) })
        else {
            return muted;
        };
        let Ok(manager) = (unsafe { device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) })
        else {
            return muted;
        };
        let Ok(collection) = (unsafe { manager.GetSessionEnumerator() }) else {
            return muted;
        };
        let Ok(count) = (unsafe { collection.GetCount() }) else {
            return muted;
        };
        for index in 0..count {
            if mute_session(&collection, index, us) {
                if let Some(pid) = session_pid(&collection, index) {
                    muted.push(pid);
                }
            }
        }
        muted
    }

    fn mute_session(collection: &IAudioSessionEnumerator, index: i32, us: u32) -> bool {
        let Some(pid) = session_pid(collection, index) else {
            return false;
        };
        if pid == us {
            return false;
        }
        let Ok(control) = (unsafe { collection.GetSession(index) }) else {
            return false;
        };
        let Ok(volume) = control.cast::<ISimpleAudioVolume>() else {
            return false;
        };
        let already = unsafe { volume.GetMute() }.unwrap_or_default();
        if already.as_bool() {
            return false;
        }
        unsafe { volume.SetMute(TRUE, std::ptr::null()) }.is_ok()
    }

    fn session_pid(collection: &IAudioSessionEnumerator, index: i32) -> Option<u32> {
        let control: IAudioSessionControl = unsafe { collection.GetSession(index) }.ok()?;
        let control2: IAudioSessionControl2 = control.cast().ok()?;
        unsafe { control2.GetProcessId() }.ok()
    }

    fn unmute_sessions(pids: &[u32]) {
        if pids.is_empty() {
            return;
        }
        let Ok(enumerator) = (unsafe {
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        }) else {
            return;
        };
        let Ok(device) = (unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia) })
        else {
            return;
        };
        let Ok(manager) = (unsafe { device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) })
        else {
            return;
        };
        let Ok(collection) = (unsafe { manager.GetSessionEnumerator() }) else {
            return;
        };
        let Ok(count) = (unsafe { collection.GetCount() }) else {
            return;
        };
        for index in 0..count {
            let Some(pid) = session_pid(&collection, index) else {
                continue;
            };
            if !pids.contains(&pid) {
                continue;
            }
            let Ok(control) = (unsafe { collection.GetSession(index) }) else {
                continue;
            };
            let Ok(volume) = control.cast::<ISimpleAudioVolume>() else {
                continue;
            };
            let _ = unsafe { volume.SetMute(windows::Win32::Foundation::FALSE, std::ptr::null()) };
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::MediaPause;

    pub fn pause() -> MediaPause {
        #[cfg(target_os = "macos")]
        {
            pause_macos()
        }
        #[cfg(not(target_os = "macos"))]
        {
            MediaPause::default()
        }
    }

    pub fn resume(state: MediaPause) {
        #[cfg(target_os = "macos")]
        {
            resume_macos(state);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = state;
        }
    }

    #[cfg(target_os = "macos")]
    fn pause_macos() -> MediaPause {
        let mut paused = Vec::new();
        for app in ["Music", "Spotify", "Safari", "TV"] {
            if tell(app, "pause") {
                paused.push(app.into());
            }
        }
        MediaPause {
            paused_apps: paused,
            muted_pids: Vec::new(),
        }
    }

    #[cfg(target_os = "macos")]
    fn resume_macos(state: MediaPause) {
        for app in state.paused_apps {
            tell(&app, "play");
        }
    }

    #[cfg(target_os = "macos")]
    fn tell(app: &str, command: &str) -> bool {
        std::process::Command::new("osascript")
            .args(["-e", &format!("tell application \"{app}\" to {command}")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}
