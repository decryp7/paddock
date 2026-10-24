//! The shell's own command: `shell_version`.

use crate::builder::AppIdentity;
use serde::Serialize;
use tauri::{Runtime, State};

/// What `shell_version` answers: the plugin's version (the UI package must carry the same one) and the app's identity.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionReply {
    /// The `paddock-shell` crate's version (lockstep with `@decryp7/paddock-ui`).
    pub shell: String,
    pub app: AppIdentity,
}

impl VersionReply {
    /// The reply for `app` from this build of the shell.
    pub fn of(app: &AppIdentity) -> VersionReply {
        VersionReply {
            shell: crate::VERSION.to_string(),
            app: app.clone(),
        }
    }
}

/// The plugin's version and the app's identity. Every window (and the tray popover) may ask.
#[tauri::command]
pub async fn shell_version<R: Runtime>(
    _app: tauri::AppHandle<R>,
    identity: State<'_, AppIdentity>,
) -> Result<VersionReply, String> {
    Ok(VersionReply::of(&identity))
}

/// The invoke handler of [`OWN_COMMANDS`](crate::OWN_COMMANDS).
pub(crate) fn invoke_handler<R: Runtime>()
-> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![shell_version]
}
