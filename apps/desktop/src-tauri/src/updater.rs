//! Check GitHub Releases and install a newer desktop build.
//!
//! The app looks at the rolling `desktop` release. CI overwrites that release
//! on every successful installer run. A launch check installs quietly; Settings
//! also has a button for the same path.

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

use crate::overlay;

static IN_FLIGHT: AtomicBool = AtomicBool::new(false);

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match run(&app).await {
            Ok(true) => {
                tracing::info!("installed an update; restarting");
                app.restart();
            }
            Ok(false) => tracing::debug!("already on the latest desktop build"),
            Err(error) => tracing::warn!(%error, "could not check for updates"),
        }
    });
}

/// Check for a newer build and install it if one exists.
///
/// Returns a short status for the Settings button. If an update was installed
/// this process restarts and the caller never sees the Ok.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<String, String> {
    match run(&app).await {
        Ok(true) => {
            app.restart();
        }
        Ok(false) => Ok("You're on the latest version.".into()),
        Err(error) => Err(error),
    }
}

async fn run(app: &AppHandle) -> Result<bool, String> {
    if IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return Err("An update is already in progress.".into());
    }

    let result = check_and_install(app).await;
    IN_FLIGHT.store(false, Ordering::SeqCst);
    result.map_err(explain)
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
