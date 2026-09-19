//! Check GitHub Releases and install a newer desktop build.
//!
//! The app looks at the rolling `desktop` release. CI overwrites that release
//! on every successful installer run. A launch check installs quietly; the Hub
//! also polls availability and shows an **Update** control for the same path.
//!
//! Signing: Desktop installers need repo secrets `TAURI_SIGNING_PRIVATE_KEY`
//! and optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The matching public key
//! lives in `tauri.conf.json` → `plugins.updater.pubkey`. Without the private
//! key, CI skips updater artifacts and `latest.json` cannot be published.

use std::sync::atomic::{AtomicBool, Ordering};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

use crate::overlay;

static IN_FLIGHT: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub available_version: Option<String>,
}

impl UpdateInfo {
    fn current(app: &AppHandle) -> Self {
        Self {
            available: false,
            current_version: app.package_info().version.to_string(),
            available_version: None,
        }
    }
}

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match run_install(&app).await {
            Ok(true) => {
                tracing::info!("installed an update; restarting");
                app.restart();
            }
            Ok(false) => {
                tracing::debug!("already on the latest desktop build");
                let _ = app.emit("weldspeak://update-status", UpdateInfo::current(&app));
            }
            Err(error) => tracing::warn!(%error, "could not check for updates"),
        }
    });
}

/// Probe `latest.json` without downloading. Safe to call while the Hub is open.
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<UpdateInfo, String> {
    let info = probe(&app).await?;
    let _ = app.emit("weldspeak://update-status", &info);
    Ok(info)
}

/// Check for a newer build and install it if one exists.
///
/// Returns a short status for the Settings button. If an update was installed
/// this process restarts and the caller never sees the Ok.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<String, String> {
    match run_install(&app).await {
        Ok(true) => {
            app.restart();
        }
        Ok(false) => {
            let info = UpdateInfo::current(&app);
            let _ = app.emit("weldspeak://update-status", &info);
            Ok("You're on the latest version.".into())
        }
        Err(error) => Err(error),
    }
}

async fn run_install(app: &AppHandle) -> Result<bool, String> {
    if IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return Err("An update is already in progress.".into());
    }

    let result = check_and_install(app).await;
    IN_FLIGHT.store(false, Ordering::SeqCst);
    result.map_err(explain)
}

async fn probe(app: &AppHandle) -> Result<UpdateInfo, String> {
    let current_version = app.package_info().version.to_string();
    let Some(update) = app
        .updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| explain(error.to_string()))?
    else {
        return Ok(UpdateInfo {
            available: false,
            current_version,
            available_version: None,
        });
    };

    Ok(UpdateInfo {
        available: true,
        current_version,
        available_version: Some(update.version),
    })
}

async fn check_and_install(app: &AppHandle) -> Result<bool, String> {
    let Some(update) = app
        .updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };

    let _ = app.emit(
        "weldspeak://update-status",
        UpdateInfo {
            available: true,
            current_version: app.package_info().version.to_string(),
            available_version: Some(update.version.clone()),
        },
    );

    overlay::show_status(app, &format!("Updating WeldSpeak to {}…", update.version));

    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|error| error.to_string())?;

    Ok(true)
}

fn explain(error: String) -> String {
    if error.contains("valid release JSON") {
        "Could not read the update feed. Try again after the next desktop build lands.".into()
    } else {
        error
    }
}
