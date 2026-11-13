//! Recent lists owned by Rust: an entry is recorded only after a granted open or save succeeded ([`Recents::record`] takes the
//! [`GrantedPath`] as proof), so reopening one from a list (the welcome page, the Recent projects dialog, the tray) is the
//! user's earlier choice and is granted again on use ([`crate::Files::regrant`]).
//!
//! The lists live in one versioned file in the config folder (`recent.json`; a missing, damaged or newer file reads as empty,
//! by `paddock-env`) and are app-wide: every window shows the same lists. An app declares its lists ([`ListSpec`]: an id and a
//! cap); the family's usual two are [`FILES`] and [`PROJECTS`].

use crate::grants::GrantedPath;
use crate::path::MAX_PATH_LEN;
use paddock_env::VersionedFile;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

/// The file's name in the config folder.
pub const FILE: &str = "recent.json";
/// The file's format version.
pub const VERSION: u32 = 1;
/// The longest display name kept, in characters.
pub const MAX_NAME: usize = 120;

/// A recent list an app keeps: an id (letters, digits, dashes) and how many entries it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListSpec {
    /// The id the UI names the list by.
    pub id: &'static str,
    /// Entries kept; the oldest go first.
    pub max: usize,
}

/// Recent files and folders: the family's default list.
pub const FILES: ListSpec = ListSpec {
    id: "files",
    max: 8,
};
/// Recent projects: the family's default list.
pub const PROJECTS: ListSpec = ListSpec {
    id: "projects",
    max: 10,
};

/// One entry of a list.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentEntry {
    /// The absolute path as it was granted.
    pub path: String,
    /// A display name (a project's), when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub name: Option<String>,
    /// When it was last opened or saved, in milliseconds since the Unix epoch.
    #[cfg_attr(test, ts(type = "number"))]
    pub opened_at: u64,
    /// It was a folder when it was recorded (the UI draws the folder glyph).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(test, ts(as = "Option<bool>", optional))]
    pub folder: bool,
}

/// A list with its id, newest first.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentList {
    /// The list's id.
    pub id: String,
    /// The entries, newest first.
    pub entries: Vec<RecentEntry>,
}

/// Every list, in the order the app declared them (camelCase on the wire).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentLists {
    /// One entry per declared list.
    pub lists: Vec<RecentList>,
}

impl RecentLists {
    /// The entries of list `id` (empty for an unknown list).
    pub fn entries(&self, id: &str) -> &[RecentEntry] {
        self.lists
            .iter()
            .find(|l| l.id == id)
            .map_or(&[], |l| l.entries.as_slice())
    }
}

/// The recent lists of an app (managed state; thread-safe).
pub struct Recents {
    specs: Vec<ListSpec>,
    file: Option<VersionedFile<RecentLists>>,
    lists: Mutex<RecentLists>,
    clock: fn() -> u64,
}

impl Recents {
    /// The lists kept in `dir` (none: in memory only), read once. Entries of lists the app no longer declares are dropped.
    pub fn open(dir: Option<&Path>, specs: &[ListSpec]) -> Recents {
        let file = dir.map(|d| VersionedFile::<RecentLists>::new(d.join(FILE), VERSION));
        let loaded = file.as_ref().map(|f| {
            let loaded = f.load();
            if let Some(problem) = &loaded.problem {
                log::warn!("the recent lists could not be used: {problem}");
            }
            loaded.value
        });
        let mut lists = fixed(loaded.unwrap_or_default(), specs);
        for l in &mut lists.lists {
            l.entries.shrink_to_fit();
        }
        Recents {
            specs: specs.to_vec(),
            file,
            lists: Mutex::new(lists),
            clock: now_ms,
        }
    }

    /// Replaces the clock (milliseconds since the Unix epoch); for tests.
    #[must_use]
    pub fn with_clock(mut self, clock: fn() -> u64) -> Recents {
        self.clock = clock;
        self
    }

    fn lock(&self) -> MutexGuard<'_, RecentLists> {
        self.lists
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The lists now.
    pub fn list(&self) -> RecentLists {
        self.lock().clone()
    }

    /// Whether `path` (as the webview sends it) is in list `id`.
    pub fn has(&self, id: &str, path: &str) -> bool {
        self.lock().entries(id).iter().any(|e| same(&e.path, path))
    }

    /// Puts a granted path at the front of list `id` (deduplicated, capped); whether the lists changed. An unknown list or a
    /// path that cannot be kept changes nothing. `name` is the display name (a project's); empty gives the file's stem.
    pub fn record(&self, id: &str, granted: &GrantedPath, name: Option<&str>) -> bool {
        self.record_entry(id, &granted.text(), name, granted.path().is_dir())
    }

    #[cfg(test)]
    pub(crate) fn record_path(&self, id: &str, path: &str, name: Option<&str>) -> bool {
        self.record_entry(id, path, name, false)
    }

    fn record_entry(&self, id: &str, path: &str, name: Option<&str>, folder: bool) -> bool {
        let Some(spec) = self.specs.iter().find(|s| s.id == id) else {
            log::warn!(List = id; "a path was recorded in a recent list the app does not declare");
            return false;
        };
        if !keepable(path) {
            return false;
        }
        let name = name.map(|n| clean_name(n, path)).filter(|n| !n.is_empty());
        let entry = RecentEntry {
            path: path.to_string(),
            name,
            opened_at: (self.clock)(),
            folder,
        };
        self.change(id, |entries| {
            entries.retain(|e| !same(&e.path, path));
            entries.insert(0, entry);
            entries.truncate(spec.max);
        })
    }

    /// Takes `path` out of list `id`; whether it was there.
    #[must_use = "whether the entry was there and the lists were rewritten"]
    pub fn remove(&self, id: &str, path: &str) -> bool {
        self.change(id, |entries| entries.retain(|e| !same(&e.path, path)))
    }

    /// Empties list `id`; whether it had entries.
    #[must_use = "whether the list had entries and was rewritten"]
    pub fn clear(&self, id: &str) -> bool {
        self.change(id, Vec::clear)
    }

    /// Applies `f` to list `id`; when it changed, writes the lists and says so.
    fn change(&self, id: &str, f: impl FnOnce(&mut Vec<RecentEntry>)) -> bool {
        let snapshot = {
            let mut all = self.lock();
            let Some(list) = all.lists.iter_mut().find(|l| l.id == id) else {
                return false;
            };
            let before = list.entries.clone();
            f(&mut list.entries);
            if list.entries == before {
                return false;
            }
            all.clone()
        };
        if let Some(file) = &self.file
            && let Err(e) = file.save(&snapshot)
        {
            log::warn!("could not save the recent lists: {e}");
        }
        true
    }
}

/// Paths compare as written, ignoring ASCII case on Windows and macOS (case-insensitive file systems).
fn same(a: &str, b: &str) -> bool {
    same_on(a, b, cfg!(any(windows, target_os = "macos")))
}

fn same_on(a: &str, b: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// Milliseconds since the Unix epoch (0 if the clock is before it).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn keepable(path: &str) -> bool {
    path.len() <= MAX_PATH_LEN && !path.contains('\0') && Path::new(path).is_absolute()
}

fn clean_name(name: &str, path: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME)
        .collect();
    if name.trim().is_empty() {
        Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().chars().take(MAX_NAME).collect())
            .unwrap_or_default()
    } else {
        name
    }
}

/// The lists as read: exactly the declared lists in their order, bounded, with only absolute paths.
fn fixed(read: RecentLists, specs: &[ListSpec]) -> RecentLists {
    RecentLists {
        lists: specs
            .iter()
            .map(|spec| {
                let mut entries: Vec<RecentEntry> = read
                    .lists
                    .iter()
                    .find(|l| l.id == spec.id)
                    .map(|l| {
                        l.entries
                            .iter()
                            .filter(|e| keepable(&e.path))
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                entries.truncate(spec.max);
                RecentList {
                    id: spec.id.to_string(),
                    entries,
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grants::{Access, GrantSource, PathGrants};

    fn abs(dir: &Path, name: &str) -> String {
        dir.join(name).to_string_lossy().into_owned()
    }

    fn open(dir: &Path) -> Recents {
        Recents::open(Some(dir), &[FILES, PROJECTS]).with_clock(|| 1_700_000_000_000)
    }

    #[test]
    fn lists_are_deduplicated_capped_and_kept() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        let r = open(d);
        assert_eq!(r.list().lists.len(), 2, "every declared list exists, empty");
        assert!(r.record_path("files", &abs(d, "a"), None));
        assert!(r.record_path("files", &abs(d, "b"), None));
        assert!(
            r.record_path("files", &abs(d, "a"), None),
            "reopening moves to the front"
        );
        let paths = |r: &Recents| {
            r.list()
                .entries("files")
                .iter()
                .map(|e| e.path.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(paths(&r), vec![abs(d, "a"), abs(d, "b")]);
        assert!(
            !r.record_path("files", "relative", None),
            "only absolute paths are kept"
        );
        assert!(!r.record_path("files", &format!("{}\0", abs(d, "x")), None));
        assert!(
            !r.record_path("nope", &abs(d, "a"), None),
            "an undeclared list changes nothing"
        );
        for i in 0..20 {
            r.record_path("files", &abs(d, &format!("f{i}")), None);
        }
        assert_eq!(r.list().entries("files").len(), FILES.max);
        assert!(
            r.has("files", &abs(d, "f19"))
                && !r.has("files", &abs(d, "a"))
                && !r.has("nope", &abs(d, "f19"))
        );
        // Kept across loads.
        let again = open(d);
        assert_eq!(again.list(), r.list());
        assert!(again.remove("files", &abs(d, "f19")));
        assert!(!again.remove("files", &abs(d, "f19")));
        assert!(!again.remove("nope", &abs(d, "f18")));
        assert!(again.clear("files"));
        assert!(!again.clear("files"), "nothing to clear");
        assert!(open(d).list().entries("files").is_empty());
    }

    #[test]
    fn names_are_cleaned_and_default_to_the_file_stem() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        let r = open(d);
        let name = |r: &Recents| r.list().entries("projects")[0].name.clone();
        r.record_path("projects", &abs(d, "alpha.proj"), Some("  "));
        assert_eq!(
            name(&r).as_deref(),
            Some("alpha"),
            "an empty name is the file's stem"
        );
        r.record_path("projects", &abs(d, "alpha.proj"), Some("My\nproject"));
        assert_eq!(
            name(&r).as_deref(),
            Some("Myproject"),
            "control characters are dropped"
        );
        r.record_path("projects", &abs(d, "alpha.proj"), Some(&"n".repeat(500)));
        assert_eq!(name(&r).unwrap().chars().count(), MAX_NAME);
        r.record_path("projects", &abs(d, "alpha.proj"), None);
        assert_eq!(name(&r), None, "no name given");
        assert_eq!(
            r.list().entries("projects")[0].opened_at,
            1_700_000_000_000,
            "the injected clock"
        );
        for i in 0..15 {
            r.record_path(
                "projects",
                &abs(d, &format!("p{i}.proj")),
                Some(&format!("P{i}")),
            );
        }
        assert_eq!(r.list().entries("projects").len(), PROJECTS.max);
        assert_eq!(name(&r).as_deref(), Some("P14"));
    }

    #[test]
    fn recording_takes_a_granted_path_as_proof() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("a.log");
        std::fs::write(&file, "x").unwrap();
        let granted = PathGrants::default()
            .grant("main", &file, Access::Read, GrantSource::Dialog)
            .unwrap();
        let r = Recents::open(None, &[FILES]);
        assert!(r.record("files", &granted, None));
        assert!(r.has("files", &granted.text()));
        assert!(!r.list().entries("files")[0].folder);
        let dir = PathGrants::default()
            .grant("main", t.path(), Access::Read, GrantSource::Dialog)
            .unwrap();
        r.record("files", &dir, None);
        assert!(
            r.list().entries("files")[0].folder,
            "a folder is recorded as one"
        );
        assert_eq!(r.list().entries("files").len(), 2);
    }

    #[test]
    fn a_damaged_or_newer_file_reads_as_empty_and_undeclared_lists_go() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::write(d.join(FILE), "{ not json").unwrap();
        assert!(open(d).list().entries("files").is_empty());
        let path = abs(d, "x");
        std::fs::write(
            d.join(FILE),
            format!(r#"{{"version":99,"lists":[{{"id":"files","entries":[{{"path":{path:?},"openedAt":1}}]}}]}}"#),
        )
        .unwrap();
        let newer = open(d);
        assert!(
            newer.list().entries("files").is_empty(),
            "a newer file is not read"
        );
        assert!(
            newer.record_path("files", &abs(d, "y"), None),
            "memory still works"
        );
        assert!(
            std::fs::read_to_string(d.join(FILE))
                .unwrap()
                .contains("\"version\":99"),
            "the newer file is left untouched"
        );
        // Old entries of lists the app dropped, relative paths and surplus entries are cleaned on load.
        let entries: Vec<String> = (0..12)
            .map(|i| {
                format!(
                    r#"{{"path":{:?},"openedAt":{i}}}"#,
                    abs(d, &format!("e{i}"))
                )
            })
            .chain([r#"{"path":"relative","openedAt":1}"#.to_string()])
            .collect();
        std::fs::write(
            d.join(FILE),
            format!(
                r#"{{"version":1,"lists":[{{"id":"old","entries":[{{"path":{path:?},"openedAt":1}}]}},{{"id":"files","entries":[{}]}}]}}"#,
                entries.join(",")
            ),
        )
        .unwrap();
        let cleaned = open(d).list();
        assert_eq!(
            cleaned
                .lists
                .iter()
                .map(|l| l.id.as_str())
                .collect::<Vec<_>>(),
            ["files", "projects"]
        );
        assert_eq!(cleaned.entries("files").len(), FILES.max);
        assert!(
            cleaned
                .entries("files")
                .iter()
                .all(|e| e.path.starts_with(&*d.to_string_lossy()))
        );
        assert_eq!(
            Recents::open(None, &[FILES]).list().entries("files").len(),
            0
        );
    }

    #[test]
    fn a_failed_save_is_logged_and_the_change_still_counts() {
        let t = tempfile::tempdir().unwrap();
        // The config folder is gone: the write fails, the list in memory is right.
        let gone = t.path().join("gone");
        let r = Recents::open(Some(&gone), &[FILES]);
        assert!(r.record_path("files", &abs(t.path(), "a"), None));
        assert!(r.has("files", &abs(t.path(), "a")));
    }

    #[test]
    fn paths_compare_by_the_file_systems_case_rule() {
        assert!(same_on("/A/b", "/a/B", true) && !same_on("/A/b", "/a/B", false));
        assert!(same_on("/a", "/a", false) && !same_on("/a", "/b", true));
    }

    #[test]
    fn a_poisoned_lock_recovers() {
        let r = std::sync::Arc::new(Recents::open(None, &[FILES]));
        let r2 = r.clone();
        let _ = std::thread::spawn(move || {
            let _hold = r2.lock();
            panic!("poison");
        })
        .join();
        let absolute = if cfg!(windows) { r"C:\a" } else { "/a" };
        assert!(r.record_path("files", absolute, None));
    }
}
