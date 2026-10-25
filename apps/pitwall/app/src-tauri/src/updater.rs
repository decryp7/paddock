//! Pitwall updating itself with the Tauri updater (minisign-signed, opt-in). Wired but inert in unsigned builds.
//!
//! The public key comes from the build (CI secret `TAURI_UPDATER_PUBKEY`, read with `option_env!`) or from
//! `plugins.updater.pubkey` in `tauri.conf.json` (empty in the repository). A build without a key never checks and says
//! "Updates are not available in this build". Checks happen only when the user asks. After an install the app restarts;
//! the server restarts itself once it is idle and finds its own program replaced. (Adapted from Pitcrew's updater.)

use pitwall_core::mgmt::{CommandError, ErrorCode};
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_updater::UpdaterExt;

/// The update found by the last check, kept for the install.
#[derive(Default)]
pub struct Pending(Mutex<Option<tauri_plugin_updater::Update>>);

/// The update public key given to this build. Cargo rebuilds when it changes (build.rs).
const BUILD_PUBKEY: Option<&str> = option_env!("TAURI_UPDATER_PUBKEY");

/// `AppUpdateStatus` in `api.types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateStatus {
    /// This build can update itself.
    pub available: bool,
    /// Why not.
    pub reason: Option<String>,
    /// The running version.
    pub current_version: String,
}

/// An update found by a check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// Its version.
    pub version: String,
    /// Release notes.
    pub notes: Option<String>,
}

/// The plugin. Its own IPC commands are granted to no window: the UI goes through Pitwall's commands.
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new().build()
}

/// The key to verify updates with: the build's, else the configured one; `None` when neither looks like a Tauri updater
/// public key (base64 of a minisign public key file).
pub fn resolve_pubkey(build: Option<&str>, config: Option<&str>) -> Option<String> {
    let pick = |k: Option<&str>| k.map(str::trim).filter(|k| !k.is_empty()).map(str::to_string);
    let key = pick(build).or_else(|| pick(config))?;
    looks_like_public_key(&key).then_some(key)
}

/// A minisign public key file is two lines (~ 100 bytes); Tauri stores it base64-encoded. This is a sanity check against
/// pasting the wrong secret (e.g. the private key, which is much longer); the plugin verifies for real.
fn looks_like_public_key(key: &str) -> bool {
    (80..=400).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
}

fn key<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let configured = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|c| c.get("pubkey"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    resolve_pubkey(BUILD_PUBKEY, configured.as_deref())
}

/// Whether this build can update itself.
pub fn status<R: Runtime>(app: &AppHandle<R>) -> AppUpdateStatus {
    status_for(key(app).is_some(), app.package_info().version.to_string())
}

fn status_for(has_key: bool, version: String) -> AppUpdateStatus {
    AppUpdateStatus {
        available: has_key,
        reason: (!has_key).then(|| "Updates are not available in this build.".to_string()),
        current_version: version,
    }
}

/// Checks for a release (one HTTPS GET of latest.json; signatures are verified at install).
pub async fn check<R: Runtime>(app: &AppHandle<R>) -> Result<Option<UpdateInfo>, CommandError> {
    let Some(k) = key(app) else {
        return Err(CommandError::new(
            ErrorCode::NotAvailable,
            "Updates are not available in this build.",
        ));
    };
    let updater = app
        .updater_builder()
        .pubkey(k)
        .build()
        .map_err(|e| CommandError::new(ErrorCode::Internal, format!("the updater could not be set up: {e}")))?;
    let found = updater
        .check()
        .await
        .map_err(|e| CommandError::new(ErrorCode::Offline, format!("checking for updates failed: {e}")))?;
    let info = found.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        notes: u.body.clone(),
    });
    *app.state::<Pending>().0.lock().unwrap_or_else(|p| p.into_inner()) = found;
    Ok(info)
}

/// Downloads, verifies and installs the update found by [`check`], then restarts the app (the server keeps serving).
pub async fn install<R: Runtime>(app: &AppHandle<R>) -> Result<(), CommandError> {
    let pending = app.state::<Pending>().0.lock().unwrap_or_else(|p| p.into_inner()).take();
    let Some(update) = pending else {
        return Err(CommandError::new(ErrorCode::BadRequest, "check for updates first"));
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| CommandError::new(ErrorCode::Internal, format!("the update could not be installed: {e}")))?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    // The shape of a Tauri updater public key (base64 of "untrusted comment: ...\nRW..."), not a real key.
    const KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDAwMDAwMDAwMDAwMDAwMDAKUldRQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=";

    #[test]
    fn the_build_key_wins_and_junk_is_refused() {
        assert_eq!(resolve_pubkey(Some(KEY), Some("other")).as_deref(), Some(KEY));
        assert_eq!(resolve_pubkey(Some("  "), Some(KEY)).as_deref(), Some(KEY));
        assert_eq!(resolve_pubkey(None, Some("")), None);
        assert_eq!(resolve_pubkey(Some("not a key!"), None), None);
        assert_eq!(resolve_pubkey(Some(&"A".repeat(2000)), None), None, "a private key is far longer");
    }

    #[test]
    fn a_build_without_a_key_says_so_and_the_wire_shape_is_camel_case() {
        let s = status_for(false, "0.1.0".into());
        assert!(!s.available);
        assert_eq!(s.reason.as_deref(), Some("Updates are not available in this build."));
        assert!(status_for(true, "0.1.0".into()).available);
        let v = serde_json::to_value(s).unwrap();
        assert_eq!(v["currentVersion"], "0.1.0");
    }
}
