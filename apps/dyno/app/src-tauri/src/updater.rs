//! Updating Dyno with the Tauri updater (minisign-signed, opt-in). The public key comes from the build (CI secret
//! `TAURI_UPDATER_PUBKEY`, read with `option_env!`) or from `plugins.updater.pubkey` in `tauri.conf.json` (empty in
//! the repository). A build without a key never checks and says so in About.

use crate::dto::AppInfo;
use tauri::{AppHandle, Runtime};

/// The update public key given to this build. Cargo rebuilds when it changes (build.rs).
const BUILD_PUBKEY: Option<&str> = option_env!("TAURI_UPDATER_PUBKEY");

/// The plugin. Its own IPC commands are granted to no window: the UI goes through Dyno's commands.
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new().build()
}

/// The key to verify updates with: the build's, else the configured one; `None` when neither looks like a Tauri
/// updater public key (base64 of a minisign public key file).
pub fn resolve_pubkey(build: Option<&str>, config: Option<&str>) -> Option<String> {
    let pick = |k: Option<&str>| k.map(str::trim).filter(|k| !k.is_empty()).map(str::to_string);
    let key = pick(build).or_else(|| pick(config))?;
    looks_like_public_key(&key).then_some(key)
}

/// A minisign public key file is two lines (~ 100 bytes); Tauri stores it base64-encoded. A sanity check against
/// pasting the wrong secret (the private key is much longer); the plugin verifies for real.
fn looks_like_public_key(key: &str) -> bool {
    (80..=400).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
}

/// What About shows for this build.
pub fn app_info<R: Runtime>(app: &AppHandle<R>) -> AppInfo {
    let configured = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|c| c.get("pubkey"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    info_for(
        resolve_pubkey(BUILD_PUBKEY, configured.as_deref()).is_some(),
        app.package_info().version.to_string(),
    )
}

fn info_for(has_key: bool, version: String) -> AppInfo {
    AppInfo {
        version,
        os: crate::dto::os_name(),
        updates_available: has_key,
        updates_reason: (!has_key).then(|| "Updates are not available in this build.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The shape of a Tauri updater public key (base64 of "untrusted comment: …\nRW…"), not a real key.
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
    fn a_build_without_a_key_says_so() {
        let i = info_for(false, "0.1.0".into());
        assert!(!i.updates_available);
        assert_eq!(i.updates_reason.as_deref(), Some("Updates are not available in this build."));
        assert!(info_for(true, "0.1.0".into()).updates_available);
    }
}
