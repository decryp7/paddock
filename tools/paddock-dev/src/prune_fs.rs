//! The thin edge of `dev host prune`: reads a folder into the pure plan's [`DirNode`] snapshot and removes what the plan
//! names. Nothing here decides anything ([`crate::prune`] does); everything is bounded and no symlink is ever followed.
//!
//! # Reusable names
//!
//! A path can name something else by the time it is removed (a folder replaced by a symlink between the scan and the
//! removal). Before every removal each component below the root is looked at again without following links, and the folder's
//! device and inode must still be the scanned ones (where the platform has them); `remove_dir_all` does not follow a symlink
//! at its own path either. The window that remains is the few microseconds between that look and the removal, and it
//! can only be used by someone who already owns the folder.

use crate::prune::{Candidate, DirNode, FileId, MAX_DEPTH, MAX_DIRS, MAX_ENTRIES, PruneError, RelPath};
use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};

/// What a path is, looked at without following a symlink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Nothing is there (or it cannot be looked at).
    Missing,
    /// A symlink.
    Symlink,
    /// A folder.
    Dir,
    /// A file or anything else.
    Other,
}

/// What `path` is, without following a symlink.
pub fn kind(path: &Path) -> Kind {
    match fs::symlink_metadata(path) {
        Err(_) => Kind::Missing,
        Ok(m) if m.file_type().is_symlink() => Kind::Symlink,
        Ok(m) if m.is_dir() => Kind::Dir,
        Ok(_) => Kind::Other,
    }
}

#[cfg(unix)]
fn sizes(m: &Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (m.len(), m.blocks().saturating_mul(512))
}

#[cfg(not(unix))]
fn sizes(m: &Metadata) -> (u64, u64) {
    (m.len(), m.len())
}

#[cfg(unix)]
fn identity(m: &Metadata) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    Some((m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn identity(_: &Metadata) -> Option<FileId> {
    None
}

/// A hard link beyond the first is counted once, as `du` does (unix; elsewhere every name counts).
#[cfg(unix)]
fn extra_link(m: &Metadata, seen: &mut HashSet<FileId>) -> bool {
    use std::os::unix::fs::MetadataExt;
    m.nlink() > 1 && !seen.insert((m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn extra_link(_: &Metadata, _: &mut HashSet<FileId>) -> bool {
    false
}

/// A folder read into a snapshot.
#[derive(Debug)]
pub struct Scan {
    /// The snapshot.
    pub tree: DirNode,
    /// Entries that could not be read (permissions, a race); they are not in the sizes.
    pub unreadable: u64,
}

struct Limits {
    dirs: usize,
    entries: u64,
    unreadable: u64,
    seen: HashSet<FileId>,
}

fn walk(path: &Path, name: String, depth: usize, st: &mut Limits) -> Result<DirNode, PruneError> {
    let meta = fs::symlink_metadata(path).map_err(|e| PruneError::Io(format!("{}: {e}", path.display())))?;
    let (apparent, allocated) = sizes(&meta);
    let mut node = DirNode {
        name,
        apparent,
        allocated,
        id: identity(&meta),
        dirs: Vec::new(),
    };
    st.dirs += 1;
    if st.dirs > MAX_DIRS {
        return Err(PruneError::TooLarge("more folders than the scan accepts"));
    }
    if depth >= MAX_DEPTH {
        return Err(PruneError::TooLarge("folders nested deeper than the scan accepts"));
    }
    let Ok(entries) = fs::read_dir(path) else {
        st.unreadable += 1;
        return Ok(node);
    };
    for entry in entries {
        let Ok(entry) = entry else {
            st.unreadable += 1;
            continue;
        };
        st.entries += 1;
        if st.entries > MAX_ENTRIES {
            return Err(PruneError::TooLarge("more entries than the scan accepts"));
        }
        let Ok(meta) = entry.metadata() else {
            st.unreadable += 1;
            continue;
        };
        if meta.is_dir() && !meta.file_type().is_symlink() {
            node.dirs.push(walk(
                &entry.path(),
                entry.file_name().to_string_lossy().into_owned(),
                depth + 1,
                st,
            )?);
        } else if !extra_link(&meta, &mut st.seen) {
            let (a, b) = sizes(&meta);
            node.apparent = node.apparent.saturating_add(a);
            node.allocated = node.allocated.saturating_add(b);
        }
    }
    node.dirs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(node)
}

/// Reads `root` and everything below it, never following a symlink.
pub fn scan(root: &Path) -> Result<Scan, PruneError> {
    let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut st = Limits {
        dirs: 0,
        entries: 0,
        unreadable: 0,
        seen: HashSet::new(),
    };
    let tree = walk(root, name, 0, &mut st)?;
    Ok(Scan {
        tree,
        unreadable: st.unreadable,
    })
}

/// The folders directly in `parent` whose names `wanted` accepts, sorted by name, with their sizes. A symlink is listed
/// (with `symlink` set and no size) when it points to a folder; a missing `parent` lists nothing.
pub fn list_dirs(parent: &Path, wanted: &dyn Fn(&str) -> bool) -> Result<Vec<Candidate>, PruneError> {
    let Ok(entries) = fs::read_dir(parent) else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !wanted(&name) {
            continue;
        }
        let path = entry.path();
        let candidate = match kind(&path) {
            Kind::Dir => Candidate {
                bytes: crate::prune::measure(&scan(&path)?.tree, &Default::default()).apparent,
                path,
                name,
                symlink: false,
            },
            Kind::Symlink if fs::metadata(&path).is_ok_and(|m| m.is_dir()) => Candidate {
                path,
                name,
                bytes: 0,
                symlink: true,
            },
            _ => continue,
        };
        found.push(candidate);
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

fn io(path: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", path.display())
}

/// Removes the folder at `rel` below `root` when every component is still a real folder (not a symlink) and the last one
/// still has the scanned identity `id`.
pub fn remove_rel(root: &Path, rel: &RelPath, id: Option<FileId>) -> Result<(), String> {
    let mut cur: PathBuf = root.to_path_buf();
    if kind(&cur) != Kind::Dir {
        return Err(format!("{} is no longer a folder; left alone", cur.display()));
    }
    for part in rel {
        cur.push(part);
        if kind(&cur) != Kind::Dir {
            return Err(format!("{} is no longer a folder; left alone", cur.display()));
        }
    }
    let now = fs::symlink_metadata(&cur).map_err(|e| io(&cur, e))?;
    if id.is_some() && identity(&now) != id {
        return Err(format!("{} is not the folder that was scanned; left alone", cur.display()));
    }
    fs::remove_dir_all(&cur).map_err(|e| io(&cur, e))
}

/// The scanned identity of the folder at `rel`.
pub fn id_of(tree: &DirNode, rel: &[String]) -> Option<FileId> {
    let mut node = tree;
    for name in rel {
        node = node.dirs.iter().find(|d| &d.name == name)?;
    }
    node.id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prune::{Unit, measure};
    use std::collections::BTreeSet;

    fn put(root: &Path, rel: &str, len: usize) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![7u8; len]).unwrap();
    }

    #[test]
    fn a_scan_sums_sizes_and_finds_the_folders() {
        let t = tempfile::tempdir().unwrap();
        put(t.path(), "a/x", 3000);
        put(t.path(), "a/b/y", 5000);
        put(t.path(), "z", 100);
        let s = scan(t.path()).unwrap();
        assert_eq!(s.unreadable, 0);
        let names: Vec<_> = s.tree.dirs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["a"]);
        assert!(measure(&s.tree, &BTreeSet::new()).apparent >= 8100);
        assert!(measure(&s.tree, &BTreeSet::new()).units(Unit::Kb) >= 8);
        assert!(id_of(&s.tree, &["a".into(), "b".into()]).is_some() == cfg!(unix));
        assert_eq!(id_of(&s.tree, &["nope".into()]), None);
    }

    #[test]
    fn a_scan_never_follows_a_symlink() {
        let t = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        put(outside.path(), "big", 1_000_000);
        put(t.path(), "f", 10);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), t.path().join("link")).unwrap();
            let s = scan(t.path()).unwrap();
            assert!(s.tree.dirs.is_empty(), "the link is not a folder of the tree");
            assert!(measure(&s.tree, &BTreeSet::new()).apparent < 100_000);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_hard_link_is_counted_once() {
        let t = tempfile::tempdir().unwrap();
        put(t.path(), "one", 50_000);
        fs::hard_link(t.path().join("one"), t.path().join("two")).unwrap();
        let s = scan(t.path()).unwrap();
        assert!(measure(&s.tree, &BTreeSet::new()).apparent < 60_000);
    }

    #[test]
    fn the_scan_is_bounded() {
        let t = tempfile::tempdir().unwrap();
        let mut deep = t.path().to_path_buf();
        for _ in 0..MAX_DEPTH + 1 {
            deep.push("d");
        }
        if fs::create_dir_all(&deep).is_ok() {
            assert_eq!(
                scan(t.path()).unwrap_err(),
                PruneError::TooLarge("folders nested deeper than the scan accepts")
            );
        }
    }

    #[test]
    fn a_missing_root_is_an_error_of_the_scan_and_a_kind_of_its_own() {
        let t = tempfile::tempdir().unwrap();
        let gone = t.path().join("gone");
        assert!(matches!(scan(&gone), Err(PruneError::Io(_))));
        assert_eq!(kind(&gone), Kind::Missing);
        put(t.path(), "f", 1);
        assert_eq!((kind(t.path()), kind(&t.path().join("f"))), (Kind::Dir, Kind::Other));
    }

    #[test]
    fn listing_filters_by_name_and_flags_links() {
        let t = tempfile::tempdir().unwrap();
        put(t.path(), ".app-target-b/x", 10);
        put(t.path(), ".app-target-a/x", 20);
        put(t.path(), "other/x", 30);
        put(t.path(), ".app-target-file", 1);
        let l = list_dirs(t.path(), &|n| n.starts_with(".app-target")).unwrap();
        assert_eq!(
            l.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            [".app-target-a", ".app-target-b"]
        );
        assert!(l.iter().all(|c| !c.symlink && c.bytes >= 10));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(t.path().join("other"), t.path().join(".app-target-link")).unwrap();
            std::os::unix::fs::symlink(t.path().join("nowhere"), t.path().join(".app-target-dead")).unwrap();
            let l = list_dirs(t.path(), &|n| n.starts_with(".app-target")).unwrap();
            let link = l.iter().find(|c| c.name == ".app-target-link").unwrap();
            assert!(link.symlink && link.bytes == 0);
            assert!(l.iter().all(|c| c.name != ".app-target-dead"), "a dangling link is not a folder");
        }
        assert!(list_dirs(&t.path().join("none"), &|_| true).unwrap().is_empty());
    }

    #[test]
    fn removal_checks_every_component_and_the_identity() {
        let t = tempfile::tempdir().unwrap();
        put(t.path(), "a/incremental/f", 10);
        let scanned = scan(t.path()).unwrap();
        let rel: RelPath = vec!["a".into(), "incremental".into()];
        let id = id_of(&scanned.tree, &rel);
        assert!(
            remove_rel(t.path(), &vec!["a".into(), "nope".into()], None)
                .unwrap_err()
                .contains("no longer a folder")
        );
        if cfg!(unix) {
            assert!(
                remove_rel(t.path(), &rel, Some((1, 1)))
                    .unwrap_err()
                    .contains("not the folder that was scanned")
            );
        }
        assert!(t.path().join("a/incremental/f").exists(), "nothing was removed by the refusals");
        remove_rel(t.path(), &rel, id).unwrap();
        assert!(!t.path().join("a/incremental").exists() && t.path().join("a").exists());
    }

    #[cfg(unix)]
    #[test]
    fn removal_refuses_a_component_swapped_for_a_symlink() {
        let t = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        put(outside.path(), "incremental/canary", 1);
        std::os::unix::fs::symlink(outside.path(), t.path().join("a")).unwrap();
        let err = remove_rel(t.path(), &vec!["a".into(), "incremental".into()], None).unwrap_err();
        assert!(err.contains("no longer a folder"), "{err}");
        assert!(outside.path().join("incremental/canary").exists());
    }
}
