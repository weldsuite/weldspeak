//! Check GitHub Releases and install a newer desktop build.
//!
//! The app looks at the rolling `desktop` release. CI overwrites that release
//! on every successful installer run, so holding the dictation key is the only
//! thing a user should have to do after the first install.

use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

use crate::overlay;

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match check_and_install(&app).await {
            Ok(true) => {
                tracing::info!("installed an update; restarting");
                app.restart();
            }
            Ok(false) => tracing::debug!("already on the latest desktop build"),
            Err(error) => tracing::warn!(%error, "could not check for updates"),
        }
    });
}

async fn check_and_install(app: &AppHandle) -> anyhow::Result<bool> {
    let Some(update) = app.updater()?.check().await? else {
        return Ok(false);
    };

    overlay::show_status(
        app,
        &format!("Updating WeldSpeak to {}…", update.version),
    );

    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await?;

    Ok(true)
}
