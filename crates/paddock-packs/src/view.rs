//! The type-erased look at a [`PackStore`], for the adapters and the UI: what a window shows without knowing the app's
//! pack type.

use crate::{Health, Origin, PackKind, PackStore, Problem, Snapshot};
use serde::Serialize;
use std::sync::Arc;

/// One loaded pack as the UI lists it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackEntry {
    /// The file name (or the built-in pack's name).
    pub name: String,
    /// Built-in or the user's.
    pub origin: Origin,
}

/// One kind of pack (cleanup categories, model catalogue, ...) as the UI shows it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindView {
    /// The kind's id (`cleanup`).
    pub id: String,
    /// The kind's name for the user ("Cleanup packs").
    pub label: String,
    /// Counts up with every real change.
    pub revision: u32,
    /// The packs in use.
    pub packs: Vec<PackEntry>,
    /// Packs refused, with what to fix.
    pub problems: Vec<Problem>,
    /// Keys that were ignored.
    pub warnings: Vec<Problem>,
    /// The one-line summary.
    pub health: Health,
}

/// Every kind the app registered.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PacksView {
    /// The kinds, in the order the app registered them.
    pub kinds: Vec<KindView>,
}

impl PacksView {
    /// The worst health across the kinds: the first degraded one, else the first with notes, else ok.
    pub fn health(&self) -> Health {
        let pick = |want: fn(&Health) -> bool| {
            self.kinds
                .iter()
                .map(|k| &k.health)
                .find(|h| want(h))
                .cloned()
        };
        pick(|h| matches!(h, Health::Degraded { .. }))
            .or_else(|| pick(|h| matches!(h, Health::Notes { .. })))
            .unwrap_or(Health::Ok)
    }
}

/// One registered kind, seen without its pack type.
pub trait KindHandle: Send + Sync {
    /// How the kind looks now.
    fn view(&self) -> KindView;
    /// Reads the packs again; whether the set changed.
    fn reload(&self) -> bool;
    /// Calls `f` after every real change.
    fn observe(&self, f: Box<dyn Fn() + Send + Sync>);
}

struct Registered<K: PackKind> {
    id: String,
    label: String,
    store: Arc<PackStore<K>>,
}

impl<K: PackKind> KindHandle for Registered<K> {
    fn view(&self) -> KindView {
        view_of(&self.id, &self.label, &self.store.current())
    }

    fn reload(&self) -> bool {
        self.store.reload()
    }

    fn observe(&self, f: Box<dyn Fn() + Send + Sync>) {
        self.store.observe(move |_| f());
    }
}

fn view_of<P>(id: &str, label: &str, s: &Snapshot<P>) -> KindView {
    KindView {
        id: id.to_string(),
        label: label.to_string(),
        revision: u32::try_from(s.revision).unwrap_or(u32::MAX),
        packs: s
            .packs
            .iter()
            .map(|p| PackEntry {
                name: p.name.clone(),
                origin: p.origin,
            })
            .collect(),
        problems: s.problems.clone(),
        warnings: s.warnings.clone(),
        health: s.health(),
    }
}

/// Registers `store` as the kind `id` ("cleanup") shown to the user as `label`.
pub fn kind<K: PackKind>(id: &str, label: &str, store: Arc<PackStore<K>>) -> Arc<dyn KindHandle> {
    Arc::new(Registered {
        id: id.to_string(),
        label: label.to_string(),
        store,
    })
}
