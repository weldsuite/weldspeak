//! Commands the settings window calls.

use reqwest::Method;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use weldspeak_core::auth::now_secs;

use crate::settings::Settings;
use crate::{api, auth, hotkey, inject, AppState};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgSummary {
    pub org_id: String,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryTerm {
    pub id: String,
    pub scope: String,
    pub term: String,
    pub sounds_like: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MeResponse {
    email: Option<String>,
    orgs: Vec<OrgSummary>,
}

#[derive(Deserialize)]
struct TermsResponse {
    terms: Vec<DictionaryTerm>,
}

struct SessionContext {
    api_base: String,
    token: String,
    org_id: Option<String>,
}

fn session_context(app: &AppHandle) -> Result<SessionContext, String> {
    let state = app.state::<AppState>();
    let api_base = state
        .settings
        .lock()
        .map_err(|_| "settings unavailable")?
        .api_base
        .clone();
    let org_id = state
        .settings
        .lock()
        .map_err(|_| "settings unavailable")?
        .org_id
        .clone();
    let token = state
        .auth
        .lock()
        .map_err(|_| "session unavailable")?
        .access_token(now_secs())
        .map(str::to_owned)
        .ok_or_else(|| "Sign in to manage your dictionary.".to_string())?;
    Ok(SessionContext {
        api_base,
        token,
        org_id,
    })
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
pub async fn get_status(app: AppHandle) -> Status {
    let can_inject = inject::can_synthesise_input();
    let Ok(ctx) = session_context(&app) else {
        return Status {
            signed_in: false,
            email: None,
            orgs: Vec::new(),
            can_inject,
        };
    };

    match api::json::<MeResponse, ()>(
        &ctx.api_base,
        &ctx.token,
        Method::GET,
        "/api/me",
        ctx.org_id.as_deref(),
        None,
    )
    .await
    {
        Ok(me) => Status {
            signed_in: true,
            email: me.email,
            orgs: me.orgs,
            can_inject,
        },
        Err(error) => {
            tracing::warn!(%error, "could not load account");
            Status {
                signed_in: true,
                email: None,
                orgs: Vec::new(),
                can_inject,
            }
        }
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
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|error| format!("could not open the browser: {error}"))?;
        Ok(())
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

#[tauri::command]
pub async fn list_dictionary(app: AppHandle) -> Result<Vec<DictionaryTerm>, String> {
    let ctx = session_context(&app)?;
    let response = api::json::<TermsResponse, ()>(
        &ctx.api_base,
        &ctx.token,
        Method::GET,
        "/api/dictionary",
        ctx.org_id.as_deref(),
        None,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(response.terms)
}

#[tauri::command]
pub async fn add_dictionary_term(
    app: AppHandle,
    term: String,
    sounds_like: Option<String>,
) -> Result<DictionaryTerm, String> {
    let trimmed = term.trim().to_string();
    if trimmed.is_empty() {
        return Err("Type a word or phrase to add.".into());
    }
    let ctx = session_context(&app)?;
    api::json::<DictionaryTerm, _>(
        &ctx.api_base,
        &ctx.token,
        Method::POST,
        "/api/dictionary",
        ctx.org_id.as_deref(),
        Some(&serde_json::json!({
            "term": trimmed,
            "scope": "user",
            "soundsLike": sounds_like.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        })),
    )
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_dictionary_term(app: AppHandle, id: String) -> Result<(), String> {
    let ctx = session_context(&app)?;
    api::send::<()>(
        &ctx.api_base,
        &ctx.token,
        Method::DELETE,
        &format!("/api/dictionary/{id}"),
        ctx.org_id.as_deref(),
        None,
    )
    .await
    .map_err(|error| error.to_string())
}
