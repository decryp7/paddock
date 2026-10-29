//! The folder snapshot: which files the index may look at, without reading any of them.

use pacenotes_core::RepoPath;
use paddock_jobs::CancelToken;

use crate::denylist::Denylist;
use crate::fs::{EntryKind, FsError, WorkspaceFs};
use crate::progress::{NoProgress, Progress};

/// Bounds of a walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotLimits {
    /// Entries visited before the walk stops (and says so).
    pub max_entries: usize,
    /// Folder nesting before a folder is skipped (and counted).
    pub max_depth: usize,
}

impl Default for SnapshotLimits {
    fn default() -> Self {
        SnapshotLimits {
            max_entries: 1_000_000,
            max_depth: 64,
        }
    }
}

/// The files found and what was left out, counted so nothing disappears silently.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Regular files that may be read, sorted by path.
    pub files: Vec<(RepoPath, u64)>,
    /// The modification time of each file in `files`, when the file system gave one (nanoseconds since the Unix epoch).
    pub modified: std::collections::BTreeMap<RepoPath, i64>,
    /// Entries withheld by the secret denylist (including every `.git` entry).
    pub denied: u32,
    /// Links (symbolic links, junctions) skipped, whatever they point at.
    pub links: u32,
    /// Sockets, devices and other special files skipped.
    pub special: u32,
    /// Names that are not valid paths (not UTF-8, a backslash, a control character).
    pub bad_names: u32,
    /// Folders deeper than the limit.
    pub too_deep: u32,
    /// Folders that could not be listed.
    pub unreadable: u32,
    /// True when `max_entries` stopped the walk.
    pub truncated: bool,
}

/// Walks the workspace with `fs`, skipping denied names, links and special files. Never reads a file's content.
pub fn snapshot(fs: &dyn WorkspaceFs, denylist: &Denylist, limits: &SnapshotLimits, cancel: &CancelToken) -> Result<Snapshot, FsError> {
    snapshot_with(fs, denylist, limits, cancel, &NoProgress)
}

/// [`snapshot`] that reports each folder's entries to `progress` as it goes (a growing count: the total is not known yet).
pub fn snapshot_with(
    fs: &dyn WorkspaceFs,
    denylist: &Denylist,
    limits: &SnapshotLimits,
    cancel: &CancelToken,
    progress: &dyn Progress,
) -> Result<Snapshot, FsError> {
    let mut out = Snapshot::default();
    let mut stack: Vec<(Option<RepoPath>, usize)> = vec![(None, 0)];
    let mut seen = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        if cancel.is_cancelled() {
            return Err(FsError::Io("cancelled".to_owned()));
        }
        let entries = match fs.list(dir.as_ref()) {
            Ok(e) => e,
            Err(FsError::Io(_) | FsError::Refused(_) | FsError::NotFound) if dir.is_some() => {
                out.unreadable += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
        progress.add(entries.len() as u64);
        for entry in entries {
            seen += 1;
            if seen > limits.max_entries {
                out.truncated = true;
                out.files.sort();
                return Ok(out);
            }
            let text = match &dir {
                Some(d) => format!("{d}/{}", entry.name),
                None => entry.name.clone(),
            };
            let Ok(path) = RepoPath::parse(&text) else {
                out.bad_names += 1;
                continue;
            };
            if denylist.check(&path).is_some() {
                out.denied += 1;
                continue;
            }
            match entry.kind {
                EntryKind::Link => out.links += 1,
                EntryKind::Other => out.special += 1,
                EntryKind::File(size) => {
                    if let Some(ns) = entry.modified_ns {
                        out.modified.insert(path.clone(), ns);
                    }
                    out.files.push((path, size));
                }
                EntryKind::Dir if depth + 1 > limits.max_depth => out.too_deep += 1,
                EntryKind::Dir => stack.push((Some(path), depth + 1)),
            }
        }
    }
    out.files.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::DirEntry;
    use std::collections::BTreeMap;

    /// A file system of folders given as text: `dir` -> entries.
    struct Fake(BTreeMap<String, Vec<DirEntry>>);

    impl WorkspaceFs for Fake {
        fn list(&self, dir: Option<&RepoPath>) -> Result<Vec<DirEntry>, FsError> {
            let key = dir.map_or(String::new(), |d| d.as_str().to_owned());
            self.0.get(&key).cloned().ok_or(FsError::Io("unreadable".into()))
        }

        fn read(&self, _: &RepoPath, _: u64) -> Result<Vec<u8>, FsError> {
            panic!("a snapshot never reads a file");
        }
    }

    fn e(name: &str, kind: EntryKind) -> DirEntry {
        DirEntry {
            name: name.to_owned(),
            kind,
            modified_ns: Some(7),
        }
    }

    fn fake() -> Fake {
        let mut m = BTreeMap::new();
        m.insert(
            String::new(),
            vec![
                e("b.rs", EntryKind::File(3)),
                e("a.rs", EntryKind::File(4)),
                e(".env", EntryKind::File(9)),
                e(".git", EntryKind::Dir),
                e("src", EntryKind::Dir),
                e("link", EntryKind::Link),
                e("sock", EntryKind::Other),
                e("bad\\name", EntryKind::File(1)),
                e("gone", EntryKind::Dir),
            ],
        );
        m.insert(
            "src".to_owned(),
            vec![
                e("lib.rs", EntryKind::File(10)),
                e("keys.pem", EntryKind::File(2)),
                e("deep", EntryKind::Dir),
            ],
        );
        m.insert("src/deep".to_owned(), vec![e("x.rs", EntryKind::File(1))]);
        Fake(m)
    }

    #[test]
    fn files_are_sorted_and_everything_left_out_is_counted() {
        let s = snapshot(&fake(), &Denylist::new(), &SnapshotLimits::default(), &CancelToken::new()).unwrap();
        let names: Vec<_> = s.files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(names, ["a.rs", "b.rs", "src/deep/x.rs", "src/lib.rs"]);
        assert_eq!(
            (s.denied, s.links, s.special, s.bad_names, s.unreadable, s.too_deep),
            (3, 1, 1, 1, 1, 0)
        );
        assert!(!s.truncated);
        assert_eq!(s.modified.len(), 4, "every file carries its modification time");
    }

    #[test]
    fn the_walk_is_bounded_in_entries_and_depth() {
        let cut = snapshot(
            &fake(),
            &Denylist::new(),
            &SnapshotLimits {
                max_entries: 3,
                max_depth: 64,
            },
            &CancelToken::new(),
        )
        .unwrap();
        assert!(cut.truncated && cut.files.len() <= 3);
        let shallow = snapshot(
            &fake(),
            &Denylist::new(),
            &SnapshotLimits {
                max_entries: 100,
                max_depth: 1,
            },
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(shallow.too_deep, 1);
        assert!(shallow.files.iter().all(|(p, _)| p.as_str() != "src/deep/x.rs"));
    }

    #[test]
    fn a_cancelled_walk_and_an_unreadable_root_are_errors() {
        let dead = CancelToken::new();
        dead.cancel();
        assert!(snapshot(&fake(), &Denylist::new(), &SnapshotLimits::default(), &dead).is_err());
        let broken = Fake(BTreeMap::new());
        assert_eq!(
            snapshot(&broken, &Denylist::new(), &SnapshotLimits::default(), &CancelToken::new()),
            Err(FsError::Io("unreadable".into()))
        );
    }
}
