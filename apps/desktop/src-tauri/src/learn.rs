//! Learn vocabulary from what the user does after a dictation.
//!
//! Two sources: distinctive names in the inserted text itself, and the edit
//! they type when the recognizer got a word wrong. Both land in the personal
//! dictionary so cleanup and the next utterance see them.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use weldspeak_core::auth::now_secs;
use weldspeak_core::learn::{self, Correction};

use crate::snippets::Snippet;
use crate::AppState;

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// How long after an insertion an edit still counts as correcting it.
const WATCH_FOR: Duration = Duration::from_secs(30);
/// Quiet time after the last edit before the correction is judged final.
const SETTLE: Duration = Duration::from_millis(2_500);
const POLL: Duration = Duration::from_millis(12);
const FIELD_POLL: Duration = Duration::from_millis(400);

/// Drop an in-flight watch, e.g. when the next dictation starts.
pub fn invalidate() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// Watch for an edit of `inserted`, and add names from it to the dictionary.
pub fn after_inject(app: &AppHandle, inserted: String) {
    let terms = learn::glossary_candidates(&inserted);
    if !terms.is_empty() {
        persist(app, Vec::new(), terms);
    }

    let gen = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::Builder::new()
        .name("weldspeak-learn".into())
        .spawn(move || watch_loop(app, inserted, gen))
        .ok();
}

/// Follow the field the dictation went into until the user stops editing it.
///
/// The first reading that contains the dictation is the baseline: the field
/// as it was the moment the paste landed. Diffing later readings against it
/// isolates exactly what the user changed, wherever the cursor was. Keystrokes
/// are a fallback for fields that cannot be read at all.
fn watch_loop(app: AppHandle, inserted: String, gen: u64) {
    let started = Instant::now();
    let target_app = crate::inject::focused_app_name();
    let needle = inserted.trim().to_string();

    let mut last_edit = None;
    let mut backspaces = 0usize;
    let mut typed = String::new();
    let mut undid = false;
    let mut prev_keys = [false; 256];
    let mut baseline = None::<String>;
    let mut last_field = None::<String>;
    let mut last_field_at = Instant::now();
    let mut field_poll_at = Instant::now();

    while GENERATION.load(Ordering::Relaxed) == gen && started.elapsed() < WATCH_FOR {
        let (down_now, chars) = poll_keys(&mut prev_keys);
        if down_now.backspace {
            backspaces = backspaces.saturating_add(1);
            last_edit = Some(Instant::now());
        }
        if down_now.delete {
            last_edit = Some(Instant::now());
        }
        if down_now.undo {
            undid = true;
            typed.clear();
            last_edit = Some(Instant::now());
        }
        if !chars.is_empty() {
            typed.push_str(&chars);
            last_edit = Some(Instant::now());
        }

        if field_poll_at.elapsed() >= FIELD_POLL {
            field_poll_at = Instant::now();
            // Switching apps ends the correction window: whatever field has
            // focus now is not the one the dictation went into.
            if crate::inject::focused_app_name() != target_app {
                break;
            }
            if let Some(field) = read_field(&app) {
                if baseline.is_none() && field.contains(&needle) {
                    baseline = Some(field.clone());
                }
                if last_field.as_deref() != Some(&field) {
                    let edited = match baseline.as_deref() {
                        Some(base) => base != field,
                        None => !field.contains(&needle),
                    };
                    last_field = Some(field);
                    last_field_at = Instant::now();
                    if edited {
                        last_edit = Some(Instant::now());
                    }
                }
            }
        }

        if let Some(edited_at) = last_edit {
            if edited_at.elapsed() >= SETTLE && last_field_at.elapsed() >= SETTLE {
                break;
            }
        }

        std::thread::sleep(POLL);
    }

    if GENERATION.load(Ordering::Relaxed) != gen || last_edit.is_none() {
        return;
    }

    let found = match (baseline.as_deref(), last_field.as_deref()) {
        // The field was readable: its diff is the truth, and keystrokes (which
        // cannot tell where the cursor was) would only add guesses.
        (Some(before), Some(after)) => learn::from_field_change(&inserted, before, after),
        (None, Some(after)) => learn::from_edit(&inserted, after),
        _ => learn::from_keystrokes(&inserted, backspaces, &typed, undid)
            .into_iter()
            .collect(),
    };

    if found.is_empty() {
        return;
    }

    persist(&app, found, Vec::new());
}

struct DownNow {
    backspace: bool,
    delete: bool,
    undo: bool,
}

fn poll_keys(prev: &mut [bool; 256]) -> (DownNow, String) {
    #[cfg(target_os = "windows")]
    {
        windows_poll_keys(prev)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = prev;
        (
            DownNow {
                backspace: false,
                delete: false,
                undo: false,
            },
            String::new(),
        )
    }
}

#[cfg(target_os = "windows")]
fn windows_poll_keys(prev: &mut [bool; 256]) -> (DownNow, String) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, GetKeyboardState, MapVirtualKeyW, ToUnicode, MAPVK_VK_TO_VSC,
    };

    const VK_BACK: u32 = 0x08;
    const VK_DELETE: u32 = 0x2E;
    const VK_CONTROL: u32 = 0x11;
    const VK_SHIFT: u32 = 0x10;
    const VK_MENU: u32 = 0x12;
    const VK_LWIN: u32 = 0x5B;
    const VK_RWIN: u32 = 0x5C;
    const VK_ESCAPE: u32 = 0x1B;
    const VK_Z: u32 = 0x5A;

    fn down(vk: u32) -> bool {
        unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
    }

    let ctrl = down(VK_CONTROL);
    let mut now = DownNow {
        backspace: false,
        delete: false,
        undo: false,
    };
    let mut typed = String::new();

    let backspace = down(VK_BACK);
    if backspace && !prev[VK_BACK as usize] {
        now.backspace = true;
    }
    prev[VK_BACK as usize] = backspace;

    let delete = down(VK_DELETE);
    if delete && !prev[VK_DELETE as usize] {
        now.delete = true;
    }
    prev[VK_DELETE as usize] = delete;

    let z = down(VK_Z);
    if ctrl && z && !prev[VK_Z as usize] {
        now.undo = true;
    }
    prev[VK_Z as usize] = z;

    if ctrl || down(VK_MENU) || down(VK_LWIN) || down(VK_RWIN) || down(VK_ESCAPE) {
        return (now, typed);
    }

    let mut keyboard = [0u8; 256];
    let _ = unsafe { GetKeyboardState(&mut keyboard) };

    for vk in 0x20u32..=0xFE {
        if matches!(
            vk,
            VK_SHIFT | VK_CONTROL | VK_MENU | VK_LWIN | VK_RWIN | VK_BACK | VK_DELETE
        ) {
            continue;
        }
        let held = down(vk);
        let was = prev[vk as usize];
        prev[vk as usize] = held;
        if !held || was {
            continue;
        }
        let scan = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_VSC) };
        let mut buf = [0u16; 8];
        let n = unsafe { ToUnicode(vk, scan, Some(&keyboard), &mut buf, 0) };
        if n > 0 {
            typed.push_str(&String::from_utf16_lossy(&buf[..n as usize]));
        }
    }

    (now, typed)
}

fn read_field(app: &AppHandle) -> Option<String> {
    // UI Automation calls into other processes and can stall; it belongs on
    // this watcher thread, never on the UI thread.
    #[cfg(target_os = "windows")]
    {
        let _ = app;
        crate::inject::focused_text()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = app.run_on_main_thread(move || {
            let _ = tx.send(crate::inject::focused_text());
        });
        rx.recv_timeout(Duration::from_millis(200)).ok().flatten()
    }
}

fn persist(app: &AppHandle, corrections: Vec<Correction>, terms: Vec<String>) {
    let path = crate::settings::path_for(app).ok();
    let mut uploaded = Vec::new();
    {
        let state = app.state::<AppState>();
        let Ok(mut settings) = state.settings.lock() else {
            return;
        };
        for correction in corrections {
            uploaded.push((correction.heard.clone(), correction.meant.clone()));
            learn::merge(&mut settings.corrections, correction);
        }
        for term in terms {
            uploaded.push((String::new(), term.clone()));
            learn::merge_term(&mut settings.pending_terms, &term);
        }
        if let Some(path) = path {
            let _ = settings.save(&path);
        }
    }

    if uploaded.is_empty() {
        return;
    }

    if let Some((_, meant)) = uploaded.iter().find(|(heard, _)| !heard.is_empty()) {
        crate::overlay::show_notice(app, &format!("Added “{meant}” to your dictionary"));
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        flush_to_dictionary(&app).await;
    });
}

/// Apply learned replacements to a transcript, longest first.
pub fn apply(text: &str, corrections: &[Correction]) -> String {
    let snippets: Vec<Snippet> = corrections
        .iter()
        .map(|correction| Snippet {
            trigger: correction.heard.clone(),
            expansion: correction.meant.clone(),
        })
        .collect();
    crate::snippets::expand(text, &snippets)
}

pub async fn flush_to_dictionary(app: &AppHandle) {
    let (api_base, org_id, corrections, pending) = {
        let state = app.state::<AppState>();
        let Ok(settings) = state.settings.lock() else {
            return;
        };
        (
            settings.api_base.clone(),
            settings.org_id.clone(),
            settings.corrections.clone(),
            settings.pending_terms.clone(),
        )
    };

    let token = {
        let state = app.state::<AppState>();
        state
            .auth
            .lock()
            .ok()
            .and_then(|auth| auth.access_token(now_secs()).map(str::to_owned))
    };
    let Some(token) = token else {
        return;
    };

    let mut ok_terms = Vec::new();
    for term in pending {
        if push_term(app, &api_base, &token, org_id.as_deref(), "", &term).await {
            ok_terms.push(term);
        }
    }
    for correction in corrections.iter().take(40) {
        let _ = push_term(
            app,
            &api_base,
            &token,
            org_id.as_deref(),
            &correction.heard,
            &correction.meant,
        )
        .await;
    }

    if ok_terms.is_empty() {
        return;
    }
    let path = crate::settings::path_for(app).ok();
    if let Ok(mut settings) = app.state::<AppState>().settings.lock() {
        settings
            .pending_terms
            .retain(|term| !ok_terms.iter().any(|ok| ok.eq_ignore_ascii_case(term)));
        if let Some(path) = path {
            let _ = settings.save(&path);
        }
    }
}

async fn push_term(
    _app: &AppHandle,
    api_base: &str,
    token: &str,
    org_id: Option<&str>,
    heard: &str,
    meant: &str,
) -> bool {
    let body = serde_json::json!({
        "heard": if heard.is_empty() { serde_json::Value::Null } else { heard.into() },
        "meant": meant,
    });
    crate::api::json::<serde_json::Value, _>(
        api_base,
        token,
        reqwest::Method::POST,
        "/api/dictionary/learn",
        org_id,
        Some(&body),
    )
    .await
    .is_ok()
}
