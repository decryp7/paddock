//! The Tauri adapter of `paddock-packs` (built with the `tauri` feature; plan X23 follow-up).
//!
//! - [`Setup`] names the app's pack kinds ([`crate::view::kind`]) and the `paddock-health` entry to keep current;
//!   [`install`] manages the [`Packs`] registry. `paddock-shell` calls it from `Builder::packs`.
//! - [`COMMANDS`](crate::COMMANDS): `packs_status` (every kind now) and `packs_reload` (reads the folders again on the
//!   blocking pool and answers with the result).
//! - Every real change of a kind (a hot reload, a reload command) goes to every window on [`EVENT`] as a [`PacksView`],
//!   and to the health entry: `down` with the first problem while a pack is refused, `ok` when none is. Its Repair is
//!   "reload", served by the shell and never by a pack (rule 19).

use crate::Health;
use crate::view::{KindHandle, PacksView};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager as _, Runtime, State};
use paddock_health::{Cause, Health as Registry, RepairAction};
use std::sync::Arc;

/// The event every change is sent on.
pub const EVENT: &str = crate::EVENT_NAME;

/// What the app sets up.
#[derive(Default)]
pub struct Setup {
    kinds: Vec<Arc<dyn KindHandle>>,
    health: Option<String>,
}

impl Setup {
    /// No kinds yet.
    pub fn new() -> Setup {
        Setup::default()
    }

    /// Adds a kind (`paddock_packs::view::kind("cleanup", "Cleanup packs", store)`).
    #[must_use]
    pub fn kind(mut self, kind: Arc<dyn KindHandle>) -> Setup {
        self.kinds.push(kind);
        self
    }

    /// Keeps the health entry `service` current and makes "reload" its repair (declare it in the app's health services).
    #[must_use]
    pub fn health(mut self, service: &str) -> Setup {
        self.health = Some(service.to_string());
        self
    }
}

/// The managed state: every kind the app registered.
pub struct Packs {
    kinds: Vec<Arc<dyn KindHandle>>,
}

impl Packs {
    /// Every kind now.
    pub fn view(&self) -> PacksView {
        PacksView {
            kinds: self.kinds.iter().map(|k| k.view()).collect(),
        }
    }

    /// Reads every kind's folder again (blocking); whether any set changed.
    pub fn reload(&self) -> bool {
        self.kinds
            .iter()
            .fold(false, |changed, k| k.reload() | changed)
    }
}

fn health_cause(view: &PacksView) -> Option<Cause> {
    match view.health() {
        Health::Degraded { message } => Some(
            Cause::new(message)
                .hint("Fix or remove the pack file in the packs folder; changes are picked up by themselves, or choose Retry.")
                .code("packs.refused"),
        ),
        Health::Ok | Health::Notes { .. } => None,
    }
}

fn report(registry: &Registry, service: &str, view: &PacksView) {
    match health_cause(view) {
        Some(cause) => {
            registry.down(service, cause, Some(RepairAction::Retry));
        }
        None => {
            registry.ok(service);
        }
    }
}

/// Manages the registry, wires the events and the health entry, and reports the state after the first load. Call it
/// after the app's first `reload_now`, so the first look is real.
pub fn install<R: Runtime>(app: &AppHandle<R>, setup: Setup) -> Arc<Packs> {
    if let Some(existing) = app.try_state::<Arc<Packs>>() {
        return existing.inner().clone();
    }
    let Setup { kinds, health } = setup;
    let packs = Arc::new(Packs { kinds });
    let registry = app.try_state::<Arc<Registry>>().map(|h| h.inner().clone());
    let wired = match (health, registry) {
        (Some(id), Some(r)) if r.get(&id).is_some() => Some((id, r)),
        (Some(id), Some(_)) => {
            log::error!(service = id.as_str(); "the packs health entry is not declared in the app's health services");
            None
        }
        (Some(id), None) => {
            log::warn!(service = id.as_str(); "the packs health entry is set but no health registry is managed");
            None
        }
        (None, _) => None,
    };
    if let Some((id, r)) = &wired {
        let weak = Arc::downgrade(&packs);
        r.set_repair(id, move || {
            let Some(p) = weak.upgrade() else {
                return Err(Cause::new("The app is closing."));
            };
            p.reload();
            health_cause(&p.view()).map_or(Ok(()), Err)
        });
        report(r, id, &packs.view());
    }
    for kind in &packs.kinds {
        let handle = app.clone();
        let state = Arc::downgrade(&packs);
        let wired = wired.clone();
        kind.observe(Box::new(move || {
            let Some(p) = state.upgrade() else { return };
            let view = p.view();
            if let Some((id, r)) = &wired {
                report(r, id, &view);
            }
            // A failed emit means the app is closing; nothing is left to tell.
            let _ = handle.emit(EVENT, &view);
        }));
    }
    app.manage(packs.clone());
    packs
}

type Managed<'a> = State<'a, Arc<Packs>>;

/// Every pack kind now.
#[::tauri::command]
pub async fn packs_status(state: Managed<'_>) -> Result<PacksView, String> {
    Ok(state.view())
}

/// Reads every pack folder again on the blocking pool and answers with the result (the event follows when a set changed).
#[::tauri::command]
pub async fn packs_reload(state: Managed<'_>) -> Result<PacksView, String> {
    let packs = state.inner().clone();
    ::tauri::async_runtime::spawn_blocking(move || {
        packs.reload();
        packs.view()
    })
    .await
    .map_err(|_| "Reloading the packs was interrupted.".to_string())
}

/// The invoke handler of [`COMMANDS`](crate::COMMANDS); wrap it in the shell's guard.
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![packs_status, packs_reload]
}

#[cfg(test)]
mod tests;
