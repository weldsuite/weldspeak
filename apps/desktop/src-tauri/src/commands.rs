//! Commands the settings window calls.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use weldspeak_core::auth::now_secs;

use crate::settings::Settings;
use crate::{auth, hotkey, inject, AppState};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub signed_in: bool,
    pub email: Option<String>,
    pub orgs: Vec<OrgSummary>,
    /// False on macOS until Accessibility is granted. Checked live rather than
    /// cached: the permission can be revoked while the app runs, and resets
    /// whenever the app's signature changes.
    pub can_inject: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgSummary {
    pub org_id: String,
    pub name: String,
    pub role: String,
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    state
        .settings
        .lock()
        .map(|settings| settings.clone())
        .map_err(|_| "settings unavailable".into())
}

/// Apply a partial update.
///
/// A patch rather than a whole object so two settings panes cannot clobber each
/// other's changes, and so a client written against an older shape does not
/// reset fields it has never heard of.
#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: serde_json::Value,
) -> Result<Settings, String> {
    let path = crate::settings::path_for(&app)?;
    let previous_accelerator;
    let next = {
        let mut settings = state.settings.lock().map_err(|_| "settings unavailable")?;
        previous_accelerator = settings.hotkey.accelerator.clone();

        let mut merged = serde_json::to_value(&*settings).map_err(|e| e.to_string())?;
        if let (Some(target), Some(source)) = (merged.as_object_mut(), patch.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }

        *settings = serde_json::from_value(merged).map_err(|e| e.to_string())?;
        settings
            .save(&path)
            .map_err(|error| error.to_string())?;
        settings.clone()
    };

    if next.hotkey.accelerator != previous_accelerator {
        crate::reregister_hotkey(&app);
    }

    Ok(next)
}

/// Check a hotkey, returning a human-readable reason if it will not work.
///
/// The rule lives in Rust rather than the UI because it is platform knowledge —
/// notably that macOS never delivers the Fn key to applications.
#[tauri::command]
pub fn validate_hotkey(accelerator: String) -> Option<String> {
    hotkey::validate_for_push_to_talk(&accelerator).err()
}

#[tauri::command]
pub fn open_permission_settings() -> Result<(), String> {
    inject::open_permission_settings().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Status {
    let signed_in = state
        .auth
        .lock()
        .map(|auth| auth.access_token(now_secs()).is_some())
        .unwrap_or(false);

    Status {
        signed_in,
        email: None,
        orgs: Vec::new(),
        can_inject: inject::can_synthesise_input(),
    }
}

/// Begin the device authorization grant, open the system browser, and return
/// the short code the user must confirm.
///
/// The browser is opened here rather than from the webview: the opener plugin's
/// URL ACL is easy to get wrong, and a failed `openUrl` from JS looks like the
/// Sign in button does nothing.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInStarted {
    pub verify_url: String,
    pub user_code: String,
}

#[tauri::command]
pub async fn begin_sign_in(app: AppHandle) -> Result<SignInStarted, String> {
    let api_base = {
        let state = app.state::<AppState>();
        let settings = state.settings.lock().map_err(|_| "settings unavailable")?;
        settings.api_base.clone()
    };

    let grant = auth::start_device_flow(&api_base)
        .await
        .map_err(|error| error.to_string())?;

    let verify_url = grant.verify_url.clone();
    let user_code = grant.user_code.clone();

    open_in_browser(&verify_url)?;

    // Poll in the background so the settings window stays responsive while the
    // user signs in.
    tauri::async_runtime::spawn(async move {
        match auth::await_approval(&api_base, &grant).await {
            Ok(tokens) => {
                if let Err(error) = auth::save_refresh_token(&tokens.refresh_token) {
                    tracing::error!(?error, "could not persist credentials");
                }

                let state = app.state::<AppState>();
                if let Ok(mut store) = state.auth.lock() {
                    store.accept(tokens);
                }

                use tauri::Emitter;
                let _ = app.emit("weldspeak://signed-in", ());
            }
            Err(error) => {
                tracing::warn!(%error, "sign-in did not complete");
                crate::overlay::show_notice(&app, &error.to_string());
            }
        }
    });

    Ok(SignInStarted {
        verify_url,
        user_code,
    })
}

/// Open `url` in the user's default browser without going through the webview.
fn open_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| format!("could not open the browser: {error}"))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|error| format!("could not open the browser: {error}"))?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = url;
        Err("opening a browser is not supported on this platform".into())
    }
}

#[tauri::command]
pub fn sign_out(state: State<'_, AppState>) -> Result<(), String> {
    if let Ok(mut auth) = state.auth.lock() {
        auth.clear();
    }
    auth::clear_refresh_token().map_err(|error| error.to_string())
}
