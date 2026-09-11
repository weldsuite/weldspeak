//! Authenticated calls to the WeldSpeak API from the desktop app.

use anyhow::{anyhow, Result};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::time::Duration;

pub async fn json<T: DeserializeOwned, B: Serialize>(
    api_base: &str,
    token: &str,
    method: Method,
    path: &str,
    org_id: Option<&str>,
    body: Option<&B>,
) -> Result<T> {
    let url = format!("{}{}", api_base.trim_end_matches('/'), path);
    let mut request = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?
        .request(method, url)
        .bearer_auth(token);

    if let Some(org_id) = org_id.filter(|id| !id.is_empty()) {
        request = request.header("X-WeldSpeak-Org", org_id);
    }
    if let Some(body) = body {
        request = request.json(body);
    }

    let response = request.send().await?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        if let Ok(body) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(message) = body.get("message").and_then(|value| value.as_str()) {
                return Err(anyhow!("{message}"));
            }
        }
        return Err(anyhow!("HTTP {status}: {text}"));
    }

    Ok(response.json().await?)
}

/// Like `json`, for endpoints that return an empty or untyped body.
pub async fn send<B: Serialize>(
    api_base: &str,
    token: &str,
    method: Method,
    path: &str,
    org_id: Option<&str>,
    body: Option<&B>,
) -> Result<()> {
    let _: serde_json::Value = json(api_base, token, method, path, org_id, body).await?;
    Ok(())
}
