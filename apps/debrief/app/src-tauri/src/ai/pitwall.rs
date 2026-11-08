//! Pitwall as the AI engine, the Tauri side (ADR 0059, ADR 0070): the key in the OS credential store, the background
//! look at Pitwall, the commands of Settings ▸ AI, and what a change means for the status event and service health.
//! The client lives in `debrief_ai::engine::pitwall`.

use super::commands::core;
use super::core::AiCore;
use super::error::{AiCmdError, AiResult};
use super::AiStatus;
use super::STATUS_EVENT;
use debrief_ai::engine::pitwall::{KeyStore, Pitwall, PitwallEvent, PitwallView};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Runtime};

/// The credential-store entry of Debrief's Pitwall key.
pub const KEYRING_SERVICE: &str = "Debrief";
pub const KEYRING_ACCOUNT: &str = "Pitwall API key";
/// How often Pitwall is looked at.
const PROBE_EVERY: Duration = Duration::from_secs(15);

/// The OS credential store (macOS Keychain, Windows Credential Manager, Secret Service). Only the app uses it; tests
/// use `MemoryKeys`.
pub struct OsKeys;

impl OsKeys {
    fn entry() -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).map_err(|e| format!("the credential store is not available: {e}"))
    }
}

impl KeyStore for OsKeys {
    fn get(&self) -> Option<String> {
        Self::entry()
            .ok()?
            .get_password()
            .ok()
            .and_then(|k| debrief_ai::engine::pitwall::keys::parse_key(&k))
    }

    fn set(&self, key: &str) -> Result<(), String> {
        let k = debrief_ai::engine::pitwall::keys::parse_key(key).ok_or("the key is not usable")?;
        Self::entry()?
            .set_password(&k)
            .map_err(|e| format!("the key could not be stored: {e}"))
    }

    fn delete(&self) -> Result<(), String> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("the key could not be deleted: {e}")),
        }
    }
}

/// The real Pitwall side: this user's Pitwall folder, the well-known port and the OS credential store.
pub fn system() -> Arc<Pitwall> {
    Pitwall::new(
        debrief_ai::engine::pitwall::Locator::system(),
        Arc::new(OsKeys),
        debrief_ai::engine::pitwall::Tuning::default(),
    )
}

/// Every window hears the new status; service health follows it.
pub(crate) fn announce<R: Runtime>(app: &AppHandle<R>, core: &AiCore) {
    let status = core.status();
    let _ = app.emit(STATUS_EVENT, &status);
    crate::health::commands::refresh_ai_engine(app, &status);
}

/// Listens to Pitwall and looks at it in the background: at once, then every [`PROBE_EVERY`]. Never on the UI thread.
pub fn start<R: Runtime>(app: &AppHandle<R>, core: Arc<AiCore>) {
    let Some(pw) = core.pitwall().cloned() else {
        return;
    };
    let (a, c) = (app.clone(), Arc::downgrade(&core));
    pw.set_listener(Arc::new(move |_: PitwallEvent| {
        let Some(core) = c.upgrade() else { return };
        announce(&a, &core);
    }));
    let weak = Arc::downgrade(&core);
    let a = app.clone();
    let spawned = std::thread::Builder::new().name("debrief-pitwall".into()).spawn(move || {
        let mut first = true;
        loop {
            let Some(core) = weak.upgrade() else { return };
            if let Some(pw) = core.pitwall() {
                pw.probe();
            }
            if first {
                first = false;
                announce(&a, &core);
            }
            drop(core);
            std::thread::sleep(PROBE_EVERY);
        }
    });
    if let Err(e) = spawned {
        log::warn!("Pitwall is not looked for in this run: {e}");
    }
}

fn pitwall_of(core: &AiCore) -> AiResult<Arc<Pitwall>> {
    core.pitwall()
        .cloned()
        .ok_or_else(|| AiCmdError::unavailable("Pitwall cannot be used in this run of Debrief."))
}

/// Starts pairing with Pitwall (the user chose Connect); the code and the outcome follow as status events.
#[tauri::command]
pub async fn ai_pitwall_connect<R: Runtime>(app: AppHandle<R>) -> AiResult<PitwallView> {
    let core = core(&app)?;
    let pw = pitwall_of(&core)?;
    let view = crate::sync::blocking("connecting to Pitwall", move || -> AiResult<PitwallView> { Ok(pw.connect(true)?) }).await?;
    announce(&app, &core);
    Ok(view)
}

/// Stops a pairing that waits for approval.
#[tauri::command]
pub async fn ai_pitwall_cancel<R: Runtime>(app: AppHandle<R>) -> AiResult<bool> {
    let core = core(&app)?;
    let stopped = pitwall_of(&core)?.cancel_pairing();
    announce(&app, &core);
    Ok(stopped)
}

/// Deletes Debrief's Pitwall key.
#[tauri::command]
pub async fn ai_pitwall_forget<R: Runtime>(app: AppHandle<R>) -> AiResult<AiStatus> {
    let core = core(&app)?;
    let pw = pitwall_of(&core)?;
    crate::sync::blocking("forgetting Pitwall", move || -> AiResult<PitwallView> { Ok(pw.forget()?) }).await?;
    announce(&app, &core);
    Ok(core.status())
}

/// Looks at Pitwall now.
#[tauri::command]
pub async fn ai_pitwall_refresh<R: Runtime>(app: AppHandle<R>) -> AiResult<AiStatus> {
    let core = core(&app)?;
    let pw = pitwall_of(&core)?;
    crate::sync::blocking("looking for Pitwall", move || -> AiResult<PitwallView> { Ok(pw.probe()) }).await?;
    announce(&app, &core);
    Ok(core.status())
}
