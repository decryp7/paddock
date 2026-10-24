//! Native glue of the updater's adapter: what only a real desktop app does (quit, start the app again, hand the package to
//! `tauri-plugin-updater`). Each function makes one platform call or hands over to the plugin; reviewed by hand and left
//! out of the coverage job (crates/README.md).

use ::tauri::{AppHandle, Manager, Runtime};

/// Quits the app to update; `RunEvent::Exit` then runs `on_exit`, which installs.
pub(super) fn quit<R: Runtime>(app: &AppHandle<R>) {
    log::info!("Restarting to update");
    app.exit(0);
}

/// Starts this app's binary again when `action` asks for it (after an install, or after a failed one so the user sees
/// what happened).
pub(super) fn relaunch_after<R: Runtime>(app: &AppHandle<R>, action: crate::ExitAction) {
    if !matches!(
        action,
        crate::ExitAction::Installed { relaunch: true }
            | crate::ExitAction::Failed { relaunch: true }
    ) {
        return;
    }
    match ::tauri::process::current_binary(&app.env()) {
        Ok(exe) => {
            if let Err(e) = std::process::Command::new(exe).spawn() {
                log::error!("Could not start the app again after the update: {e}");
            }
        }
        Err(e) => log::error!("Could not start the app again after the update: {e}"),
    }
}

#[cfg(feature = "install")]
pub use plugin::PluginInstaller;

#[cfg(feature = "install")]
mod plugin {
    use crate::{Installer, UpdateError};
    use ::tauri::{AppHandle, Runtime};
    use std::path::Path;
    use tauri_plugin_updater::UpdaterExt;

    /// Installs through `tauri-plugin-updater` (registered by the app): it restores the update from the release information
    /// as received (requiring the signed version), verifies the package's signature again and installs it.
    pub struct PluginInstaller<R: Runtime> {
        app: AppHandle<R>,
        key: String,
    }

    impl<R: Runtime> PluginInstaller<R> {
        /// `key` is the same public key the updater was given.
        pub fn new(app: AppHandle<R>, key: &str) -> Self {
            PluginInstaller {
                app,
                key: key.trim().to_string(),
            }
        }
    }

    impl<R: Runtime> Installer for PluginInstaller<R> {
        fn install(
            &self,
            file: &Path,
            manifest: &serde_json::Value,
            relaunch: bool,
        ) -> Result<(), UpdateError> {
            let failed = |detail: String| UpdateError::InstallFailed { detail };
            let updater = self
                .app
                .updater_builder()
                .pubkey(self.key.clone())
                .build()
                .map_err(|e| failed(e.to_string()))?;
            let update = updater
                .restore_update(manifest.clone())
                .map_err(|e| failed(e.to_string()))?
                .ok_or_else(|| failed("the update is no longer newer than this version".into()))?;
            let bytes = std::fs::read(file).map_err(|e| failed(e.to_string()))?;
            update
                .restart_after_install(relaunch)
                .install(bytes)
                .map_err(|e| match e {
                    tauri_plugin_updater::Error::Minisign(_)
                    | tauri_plugin_updater::Error::SignedVersionMismatch { .. }
                    | tauri_plugin_updater::Error::MissingSignedVersion => {
                        UpdateError::SignatureInvalid {
                            detail: e.to_string(),
                        }
                    }
                    other => failed(other.to_string()),
                })
        }
    }
}
