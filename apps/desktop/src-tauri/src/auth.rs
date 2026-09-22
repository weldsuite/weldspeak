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
use weldspeak_core::auth::Tokens;

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
