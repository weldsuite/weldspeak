//! Signing in, and keeping the session alive.
//!
//! Clerk cannot run here — its session tokens live about a minute and refresh
//! through browser cookies on our own domain. So the app hands the user to a
//! real browser, where Clerk works normally, and collects tokens the Worker
//! mints on its behalf.
//!
//! Rendering Clerk's sign-in inside the Tauri webview is the obvious shortcut
//! and a trap: the webview runs on `tauri://localhost`, which fights Clerk's
//! cookie and allowed-origin model. The system browser is the supported path.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use weldspeak_core::auth::{now_secs, Tokens};

use crate::AppState;

/// How long a refresh may take before it counts as unreachable. Callers wait
/// on it — the Hub at launch, a dictation at hotkey-down — so a dead network
/// must fail in seconds rather than hang.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(10);

/// Longest the keeper sleeps between checks. A laptop's timers may not count
/// time spent asleep, so after waking it re-checks within minutes instead of
/// trusting a 55-minute sleep that started yesterday.
const KEEPER_MAX_SLEEP: Duration = Duration::from_secs(5 * 60);

/// Retry spacing while the server is unreachable.
const RETRY_MIN: Duration = Duration::from_secs(15);
const RETRY_MAX: Duration = Duration::from_secs(5 * 60);

/// Keychain service name. Keyed per application, not per user: the OS scopes
/// the entry to the logged-in account already.
const KEYCHAIN_SERVICE: &str = "io.weldspeak.desktop";
const KEYCHAIN_ACCOUNT: &str = "refresh-token";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceStartRequest {
    platform: String,
    label: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verify_url: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenPair {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum PollResponse {
    AuthorizationPending,
    SlowDown { interval: u64 },
    Expired,
    Denied,
    Approved { tokens: TokenPair },
}

/// Why signing in ended without tokens.
#[derive(Debug, thiserror::Error)]
pub enum SignInError {
    #[error("the code expired before it was approved")]
    Expired,
    #[error("the request was denied")]
    Denied,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Begin a device authorization grant.
pub async fn start_device_flow(api_base: &str) -> Result<DeviceStart> {
    let label = hostname().unwrap_or_else(|| "Unknown device".into());

    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?
        .post(format!(
            "{}/auth/device/start",
            api_base.trim_end_matches('/')
        ))
        .json(&DeviceStartRequest {
            platform: platform_name().into(),
            label,
        })
        .send()
        .await?
        .error_for_status()?;

    Ok(response.json().await?)
}

/// Poll until the user approves in the browser, or the grant ends.
///
/// Honours the server's interval, including a `slow_down` instruction: polling
/// faster than asked gets a client throttled, and there is nothing to gain —
/// the user is reading a screen, not racing.
pub async fn await_approval(
    api_base: &str,
    grant: &DeviceStart,
) -> std::result::Result<Tokens, SignInError> {
    let client = reqwest::Client::new();
    let url = format!("{}/auth/device/poll", api_base.trim_end_matches('/'));

    let mut interval = Duration::from_secs(grant.interval.max(1));
    let deadline = std::time::Instant::now() + Duration::from_secs(grant.expires_in);

    while std::time::Instant::now() < deadline {
        tokio::time::sleep(interval).await;

        let response = client
            .post(&url)
            .json(&serde_json::json!({ "deviceCode": grant.device_code }))
            .send()
            .await
            .map_err(|error| SignInError::Other(error.into()))?;

        let parsed: PollResponse = response
            .json()
            .await
            .map_err(|error| SignInError::Other(error.into()))?;

        match parsed {
            PollResponse::AuthorizationPending => {}
            PollResponse::SlowDown { interval: next } => {
                interval = Duration::from_secs(next.max(1));
            }
            PollResponse::Expired => return Err(SignInError::Expired),
            PollResponse::Denied => return Err(SignInError::Denied),
            PollResponse::Approved { tokens } => {
                return Ok(Tokens::from_response(
                    tokens.access_token,
                    tokens.refresh_token,
                    tokens.expires_in,
                ));
            }
        }
    }

    Err(SignInError::Expired)
}

/// Exchange a refresh token for a new pair.
///
/// The boolean says whether the failure is final. A refusal (4xx) means the
/// token was revoked or reused, or the user was removed from their
/// organization — retrying cannot help. Anything else is worth another attempt,
/// because otherwise every tunnel and every flaky café network would sign
/// people out mid-sentence.
pub async fn refresh(api_base: &str, refresh_token: &str) -> std::result::Result<Tokens, bool> {
    let response = reqwest::Client::new()
        .post(format!("{}/auth/refresh", api_base.trim_end_matches('/')))
        .timeout(REFRESH_TIMEOUT)
        .json(&serde_json::json!({ "refreshToken": refresh_token }))
        .send()
        .await
        .map_err(|_| false)?;

    if !response.status().is_success() {
        let fatal = response.status().is_client_error();
        return Err(fatal);
    }

    let tokens: TokenPair = response.json().await.map_err(|_| false)?;

    Ok(Tokens::from_response(
        tokens.access_token,
        tokens.refresh_token,
        tokens.expires_in,
    ))
}

/// What a caller can do with the session right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    /// A usable access token.
    Token(String),
    /// Credentials are saved but the server could not be reached. The user is
    /// still signed in; asking them to sign in again would be wrong.
    Offline,
    /// No credentials, or the server refused them.
    SignedOut,
}

/// A usable access token, refreshing first when one is due.
///
/// Everything that talks to the API goes through here. Only the refresh token
/// survives a restart, so at launch there is no access token until a refresh
/// completes; reading the store directly in that window is what made the Hub
/// show "not signed in" for a few seconds. Waiting here instead also covers
/// expiry: an access token lives an hour, and a long-running app renews it on
/// demand if the keeper has not already.
pub async fn access(app: &AppHandle) -> Access {
    if let Some(token) = fresh_token(app) {
        return Access::Token(token);
    }

    // Refresh tokens are single-use: two refreshes at once would present the
    // same one twice, and the server treats reuse as theft and revokes the
    // device. Whoever waited here finds the other's result below.
    let state = app.state::<AppState>();
    let _guard = state.refresh_lock.lock().await;
    if let Some(token) = fresh_token(app) {
        return Access::Token(token);
    }
    refresh_session(app).await
}

/// The stored access token, unless it is due for renewal.
fn fresh_token(app: &AppHandle) -> Option<String> {
    let state = app.state::<AppState>();
    let store = state.auth.lock().ok()?;
    store
        .tokens()
        .filter(|tokens| !tokens.needs_refresh(now_secs()))
        .map(|tokens| tokens.access_token.clone())
}

/// The stored access token if it still works, due for renewal or not.
fn valid_token(app: &AppHandle) -> Option<String> {
    let state = app.state::<AppState>();
    let store = state.auth.lock().ok()?;
    store.access_token(now_secs()).map(str::to_owned)
}

/// Exchange the saved refresh token. Call with `refresh_lock` held.
async fn refresh_session(app: &AppHandle) -> Access {
    let Some(refresh_token) = load_refresh_token() else {
        // Signed in this run but the keychain write failed: the token in
        // memory is all there is, and it is good until it expires.
        return valid_token(app).map_or(Access::SignedOut, Access::Token);
    };

    let state = app.state::<AppState>();
    let Ok(api_base) = state
        .settings
        .lock()
        .map(|settings| settings.api_base.clone())
    else {
        return Access::Offline;
    };
    let restoring = state
        .auth
        .lock()
        .map(|store| store.tokens().is_none())
        .unwrap_or(false);

    match refresh(&api_base, &refresh_token).await {
        Ok(tokens) => {
            // Refresh tokens are single-use. If we fail to persist the
            // replacement, the next launch will present the old one and the
            // server will revoke the device.
            if let Err(error) = save_refresh_token(&tokens.refresh_token) {
                tracing::error!(?error, "could not persist credentials after refresh");
            }
            let token = tokens.access_token.clone();
            if let Ok(mut store) = state.auth.lock() {
                store.accept(tokens);
            }

            if restoring {
                // The first refresh of a launch is the session coming back.
                // The Hub asks for its status again; no "Signed in" toast,
                // since the user did nothing.
                let _ = app.emit("weldspeak://session-changed", ());
                crate::native_settings::on_signed_in();
            }
            Access::Token(token)
        }
        Err(fatal) => {
            // Fatal means the server refused: revoked, reused, or the user was
            // removed from their organization. Anything else is a network
            // problem worth retrying rather than signing out over.
            if let Ok(mut store) = state.auth.lock() {
                store.refresh_failed(now_secs(), fatal);
            }
            if fatal {
                tracing::warn!("the server refused the saved session; signing out");
                let _ = clear_refresh_token();
                let _ = app.emit("weldspeak://session-changed", ());
                return Access::SignedOut;
            }
            tracing::warn!("could not reach the server to renew the session");
            valid_token(app).map_or(Access::Offline, Access::Token)
        }
    }
}

/// Keep the session alive in the background.
///
/// Restores the saved session at launch, then renews the access token a few
/// minutes before it expires, so the hotkey never waits on a refresh. When
/// the server is unreachable it retries with backoff instead of signing out.
pub fn spawn_keeper(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut backoff = RETRY_MIN;
        let mut had_token = false;
        loop {
            let access = access(&app).await;

            // Words learned while signed out or offline wait for a session;
            // push them once one is back (launch, sign-in, reconnect).
            let has_token = matches!(access, Access::Token(_));
            if has_token && !had_token {
                crate::learn::flush_to_dictionary(&app).await;
            }
            had_token = has_token;

            let due_in = {
                let state = app.state::<AppState>();
                let store = state.auth.lock();
                store.ok().and_then(|store| {
                    store
                        .tokens()
                        .map(|tokens| tokens.refresh_delay(now_secs()))
                })
            };

            let wait = match (access, due_in) {
                // Renewed, or not yet due: sleep until it is.
                (Access::Token(_), Some(due)) if !due.is_zero() => {
                    backoff = RETRY_MIN;
                    due
                }
                // Signed out: nothing to renew. Sign-in stores fresh tokens,
                // which the next check picks up.
                (Access::SignedOut, _) => KEEPER_MAX_SLEEP,
                // Offline, or a renewal that failed while the old token still
                // works: try again soon, backing off.
                _ => {
                    let wait = backoff;
                    backoff = (backoff * 2).min(RETRY_MAX);
                    wait
                }
            };
            tokio::time::sleep(wait.min(KEEPER_MAX_SLEEP)).await;
        }
    });
}

/// Store the refresh token in the OS keychain.
///
/// Only the refresh token is persisted. The access token is short-lived and
/// cheap to re-obtain, so writing it to disk would add exposure for no benefit.
pub fn save_refresh_token(token: &str) -> Result<()> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)?
        .set_password(token)
        .map_err(|error| anyhow!("could not save credentials to the keychain: {error}"))
}

/// Read the stored refresh token, if there is one.
pub fn load_refresh_token() -> Option<String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .ok()?
        .get_password()
        .ok()
}

/// Remove the stored refresh token, on sign-out.
pub fn clear_refresh_token() -> Result<()> {
    match keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)?.delete_credential() {
        Ok(()) => Ok(()),
        // Already absent is the desired end state, not a failure.
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(anyhow!("could not clear credentials: {error}")),
    }
}

const fn platform_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "other"
    }
}

/// The machine's name, so the user can tell their devices apart in the dashboard.
fn hostname() -> Option<String> {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .filter(|name| !name.is_empty())
}
