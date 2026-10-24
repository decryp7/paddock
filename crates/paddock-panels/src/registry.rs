//! Which pop-outs exist (the layer's own record, never the webview's word) and where each view's pop-out was last
//! seen.
//!
//! A pop-out belongs to a host window: its label is `p-<host>-<n>` (`paddock_commands::labels`), numbered per run, at
//! most [`MAX_POPOUTS`] per host. The record is the single source of "is this window a pop-out, of which panel, of which
//! host".

use crate::panel::{Panel, valid_view_id};
use crate::wire::{PanelError, PanelErrorCode};
use paddock_commands::labels::{Label, POPOUT_PREFIX};
use paddock_env::VersionedFile;
use paddock_window::bounds::SavedBounds;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// More pop-outs per host window than this are refused.
pub const MAX_POPOUTS: usize = 12;
/// The most views whose place is remembered.
pub const MAX_REMEMBERED: usize = 64;
/// The version of the remembered-bounds file.
pub const MEMORY_VERSION: u32 = 1;

/// One pop-out.
#[derive(Debug, Clone, PartialEq)]
pub struct PopOut {
    pub host: String,
    pub panel: Panel,
    pub title: String,
    pub keep_on_top: bool,
    /// Where it was last seen (updated while it moves and is resized).
    pub last: Option<SavedBounds>,
}

/// The pop-outs of the app, keyed by label.
#[derive(Default)]
pub struct PopOuts {
    map: Mutex<BTreeMap<String, PopOut>>,
    next: AtomicU32,
}

fn number_of(label: &str) -> u32 {
    match Label::parse(label) {
        Some(Label::Popout { n, .. }) => n,
        _ => u32::MAX,
    }
}

impl PopOuts {
    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, PopOut>> {
        self.map.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn label(&self, host: &str) -> String {
        let n = self.next.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        format!("{POPOUT_PREFIX}{host}-{n}")
    }

    /// The label of the pop-out showing `panel` for `host`, if there is one.
    pub fn find(&self, host: &str, panel: &Panel) -> Option<String> {
        self.lock()
            .iter()
            .find(|(_, e)| e.host == host && &e.panel == panel)
            .map(|(l, _)| l.clone())
    }

    /// Records a new pop-out and gives it its label. Only a top-level window hosts pop-outs, and at most
    /// [`MAX_POPOUTS`] of them.
    pub fn reserve(&self, entry: PopOut) -> Result<String, PanelError> {
        if !matches!(
            Label::parse(&entry.host),
            Some(Label::Main | Label::Window(_))
        ) {
            return Err(PanelError::new(
                PanelErrorCode::NotAllowed,
                "only an application window can pop panels out",
            ));
        }
        let mut map = self.lock();
        if map.values().filter(|e| e.host == entry.host).count() >= MAX_POPOUTS {
            return Err(PanelError::new(
                PanelErrorCode::TooMany,
                format!(
                    "At most {MAX_POPOUTS} panels can be popped out of one window; dock one back first"
                ),
            ));
        }
        let label = self.label(&entry.host);
        map.insert(label.clone(), entry);
        Ok(label)
    }

    /// A label for the rebuilt window of a pop-out (the old one is still recorded, so the cap does not apply).
    pub fn reserve_replacing(&self, entry: PopOut) -> String {
        let label = self.label(&entry.host);
        self.lock().insert(label.clone(), entry);
        label
    }

    /// Forgets a pop-out.
    pub fn remove(&self, label: &str) -> Option<PopOut> {
        self.lock().remove(label)
    }

    /// The record of a pop-out.
    pub fn get(&self, label: &str) -> Option<PopOut> {
        self.lock().get(label).cloned()
    }

    /// Labels of the host's pop-outs, oldest first.
    pub fn of_host(&self, host: &str) -> Vec<String> {
        let mut labels: Vec<String> = self
            .lock()
            .iter()
            .filter(|(_, e)| e.host == host)
            .map(|(l, _)| l.clone())
            .collect();
        labels.sort_by_key(|l| number_of(l));
        labels
    }

    /// Remembers where a pop-out is now.
    pub fn set_last(&self, label: &str, bounds: Option<SavedBounds>) {
        if let Some(e) = self.lock().get_mut(label) {
            e.last = bounds;
        }
    }
}

/// What the file keeps: one entry per view, damaged entries dropped one by one when it is read.
#[derive(Debug, Default, Serialize, Deserialize)]
struct MemoryFile {
    #[serde(default)]
    views: BTreeMap<String, serde_json::Value>,
}

/// Where each view's pop-out was last seen, kept in a small versioned file (missing, damaged or newer: no memory).
#[derive(Default)]
pub struct Memory {
    file: Option<VersionedFile<MemoryFile>>,
    map: Mutex<BTreeMap<String, SavedBounds>>,
}

impl Memory {
    /// Memory kept in memory only (nothing survives a restart).
    pub fn in_memory() -> Self {
        Memory::default()
    }

    /// Memory kept in `file` (read now).
    pub fn in_file(path: impl Into<std::path::PathBuf>) -> Self {
        let file = VersionedFile::<MemoryFile>::new(path, MEMORY_VERSION);
        let loaded = file.load();
        if let Some(problem) = &loaded.problem {
            log::warn!(target: "paddock_panels", Problem = format!("{problem:?}").as_str(); "Pop-out places not restored");
        }
        let map = loaded
            .value
            .views
            .into_iter()
            .filter(|(k, _)| valid_view_id(k))
            .filter_map(|(k, v)| {
                Some((
                    k,
                    serde_json::from_value::<SavedBounds>(v).ok()?.sanitized()?,
                ))
            })
            .take(MAX_REMEMBERED)
            .collect();
        Memory {
            file: Some(file),
            map: Mutex::new(map),
        }
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, SavedBounds>> {
        self.map.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Where the view's pop-out was last seen.
    pub fn get(&self, view: &str) -> Option<SavedBounds> {
        self.lock().get(view).copied()
    }

    /// Remembers `bounds` for `view` and writes the file; the oldest-named entry goes when there are too many. A write
    /// that fails is logged (the place is still remembered for this run).
    pub fn put(&self, view: &str, bounds: SavedBounds) {
        if !valid_view_id(view) {
            return;
        }
        let snapshot = {
            let mut map = self.lock();
            if !map.contains_key(view)
                && map.len() >= MAX_REMEMBERED
                && let Some(first) = map.keys().next().cloned()
            {
                map.remove(&first);
            }
            map.insert(view.to_string(), bounds);
            map.clone()
        };
        let Some(file) = &self.file else { return };
        let views = snapshot
            .into_iter()
            .filter_map(|(k, b)| Some((k, serde_json::to_value(b).ok()?)))
            .collect();
        if let Err(e) = file.save(&MemoryFile { views }) {
            log::warn!(target: "paddock_panels", Error = e.to_string().as_str(); "Pop-out places not saved");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(host: &str, panel: &str) -> PopOut {
        PopOut {
            host: host.into(),
            panel: Panel::parse(panel).unwrap(),
            title: "T".into(),
            keep_on_top: false,
            last: None,
        }
    }

    fn saved(x: f64) -> SavedBounds {
        crate::place::remember(x, 20.0, 500.0, 300.0, 2.0).unwrap()
    }

    #[test]
    fn labels_are_per_host_and_pop_outs_are_found_by_panel() {
        let p = PopOuts::default();
        assert_eq!(p.reserve(entry("main", "pn.code")).unwrap(), "p-main-1");
        assert_eq!(p.reserve(entry("w-2", "pn.code")).unwrap(), "p-w-2-2");
        assert_eq!(p.reserve(entry("main", "pn.code#a")).unwrap(), "p-main-3");
        let code = Panel::parse("pn.code").unwrap();
        assert_eq!(p.find("main", &code).as_deref(), Some("p-main-1"));
        assert_eq!(p.find("w-2", &code).as_deref(), Some("p-w-2-2"));
        assert_eq!(p.find("w-3", &code), None);
        assert_eq!(p.of_host("main"), ["p-main-1", "p-main-3"]);
        assert_eq!(p.get("p-main-3").unwrap().panel.key(), Some("a"));
        assert!(p.remove("p-main-1").is_some() && p.remove("p-main-1").is_none());
        assert_eq!(p.of_host("main"), ["p-main-3"]);
    }

    #[test]
    fn only_application_windows_host_pop_outs_and_only_twelve_each() {
        let p = PopOuts::default();
        for host in ["p-main-1", "d-main-1", "tray", "nope", ""] {
            assert_eq!(
                p.reserve(entry(host, "pn.code")).unwrap_err().code,
                PanelErrorCode::NotAllowed,
                "{host}"
            );
        }
        for i in 0..MAX_POPOUTS {
            p.reserve(entry("main", &format!("pn.v{i}"))).unwrap();
        }
        let e = p.reserve(entry("main", "pn.one-too-many")).unwrap_err();
        assert_eq!(e.code, PanelErrorCode::TooMany);
        assert!(e.message.contains("At most 12"), "{e}");
        p.reserve(entry("w-2", "pn.code")).unwrap();
        // The cap does not apply to a rebuilt window.
        let again = p.reserve_replacing(entry("main", "pn.v0"));
        assert_eq!(p.of_host("main").len(), MAX_POPOUTS + 1);
        assert_eq!(p.of_host("main").last(), Some(&again));
        // The most recent label sorts by number, not by text.
        for _ in 0..12 {
            p.reserve_replacing(entry("w-2", "pn.x"));
        }
        let w2 = p.of_host("w-2");
        assert!(
            w2.windows(2).all(|w| number_of(&w[0]) < number_of(&w[1])),
            "{w2:?}"
        );
        assert_eq!(number_of("main"), u32::MAX);
    }

    #[test]
    fn where_a_pop_out_is_gets_recorded() {
        let p = PopOuts::default();
        let l = p.reserve(entry("main", "pn.code")).unwrap();
        p.set_last(&l, Some(saved(5.0)));
        assert_eq!(p.get(&l).unwrap().last, Some(saved(5.0)));
        p.set_last("p-main-99", None);
    }

    #[test]
    fn a_poisoned_lock_is_recovered() {
        let p = std::sync::Arc::new(PopOuts::default());
        let q = p.clone();
        let _ = std::thread::spawn(move || {
            let _g = q.map.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert!(p.reserve(entry("main", "pn.code")).is_ok());
    }

    #[test]
    fn places_are_remembered_per_view_and_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("panels.json");
        let m = Memory::in_file(&path);
        assert!(m.get("pn.code").is_none());
        m.put("pn.code", saved(10.0));
        m.put("pn.log", saved(20.0));
        m.put("not a view", saved(30.0));
        assert_eq!(m.get("pn.code"), Some(saved(10.0)));
        let again = Memory::in_file(&path);
        assert_eq!(again.get("pn.log"), Some(saved(20.0)));
        assert!(again.get("not a view").is_none());
        // The in-memory kind remembers for this run only.
        let mem = Memory::in_memory();
        mem.put("pn.code", saved(1.0));
        assert_eq!(mem.get("pn.code"), Some(saved(1.0)));
    }

    #[test]
    fn a_damaged_newer_or_unwritable_file_means_no_memory_and_never_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("panels.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(Memory::in_file(&path).get("pn.code").is_none());
        std::fs::write(&path, r#"{"version":99,"views":{}}"#).unwrap();
        let newer = Memory::in_file(&path);
        newer.put("pn.code", saved(1.0));
        assert_eq!(newer.get("pn.code"), Some(saved(1.0)), "kept for this run");
        assert!(
            std::fs::read_to_string(&path).unwrap().contains("99"),
            "the newer file is untouched"
        );
        // One bad entry does not cost the others.
        std::fs::write(
            &path,
            r#"{"version":1,"views":{"pn.ok":{"x":1,"y":2,"width":300,"height":200,"scale":1},"pn.bad":{"x":"a"},"Bad Key":{"x":1,"y":2,"width":3,"height":4}}}"#,
        )
        .unwrap();
        let some = Memory::in_file(&path);
        assert!(
            some.get("pn.ok").is_some()
                && some.get("pn.bad").is_none()
                && some.get("Bad Key").is_none()
        );
        // A folder where the file should be: the write fails, the place is still known.
        let blocked = dir.path().join("blocked");
        std::fs::create_dir(&blocked).unwrap();
        let m = Memory::in_file(&blocked);
        m.put("pn.code", saved(1.0));
        assert!(m.get("pn.code").is_some());
    }

    #[test]
    fn only_the_most_recent_views_are_remembered() {
        let m = Memory::in_memory();
        for i in 0..MAX_REMEMBERED + 5 {
            m.put(&format!("pn.v{i:03}"), saved(i as f64));
        }
        assert_eq!(m.lock().len(), MAX_REMEMBERED);
        assert!(m.get("pn.v000").is_none());
        assert!(m.get(&format!("pn.v{:03}", MAX_REMEMBERED + 4)).is_some());
        m.put(&format!("pn.v{:03}", MAX_REMEMBERED + 4), saved(0.0));
        assert_eq!(
            m.lock().len(),
            MAX_REMEMBERED,
            "replacing an entry evicts nothing"
        );
    }
}
