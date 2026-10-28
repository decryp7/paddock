//! The disk-space map: a bounded size tree of one folder. Bounded in depth (levels shown), in entries looked at and in
//! time; never follows links; on Unix stays on the starting volume. When a bound is hit the tree says so.

use super::walk::Budget;
use dyno_core::disk::{has_dot_segment, prune, SizeNode};
use std::path::{Path, PathBuf};

/// Levels of children returned.
pub const DEPTH: usize = 3;
/// Children kept per folder (the rest are folded into one "other items" node).
pub const PER_LEVEL: usize = 12;
/// The most entries looked at.
pub const MAX_ENTRIES: usize = 200_000;
/// The longest the walk may take.
pub const MAX_SECS: u64 = 5;

/// Why a path cannot be mapped.
pub fn check(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    if !p.is_absolute() || has_dot_segment(p) {
        return Err("Give the full path of a folder.".into());
    }
    let real = std::fs::canonicalize(p).map_err(|_| "That folder does not exist or cannot be read.".to_string())?;
    if !real.is_dir() {
        return Err("That is a file; choose a folder.".into());
    }
    Ok(real)
}

/// The folder mapped when none is given: the system drive on Windows, `/` elsewhere.
pub fn default_root() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("SystemDrive").map_or_else(|| PathBuf::from("C:\\"), |d| PathBuf::from(format!("{}\\", d.to_string_lossy())))
    } else {
        PathBuf::from("/")
    }
}

#[cfg(unix)]
fn device(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.dev()
}

#[cfg(not(unix))]
fn device(_meta: &std::fs::Metadata) -> u64 {
    0
}

struct Walk {
    budget: Budget,
    dev: u64,
    /// Levels below the root still kept as nodes.
    keep: usize,
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map_or_else(|| p.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

fn node(path: &Path, depth: usize, w: &mut Walk) -> SizeNode {
    let mut n = SizeNode {
        name: name_of(path),
        path: path.to_string_lossy().into_owned(),
        bytes: 0,
        children: vec![],
    };
    let Ok(rd) = std::fs::read_dir(path) else { return n };
    for e in rd.flatten() {
        if !w.budget.spend() {
            return n;
        }
        let p = e.path();
        let Ok(meta) = p.symlink_metadata() else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if device(&meta) != w.dev {
                continue;
            }
            let child = node(&p, depth + 1, w);
            n.bytes = n.bytes.saturating_add(child.bytes);
            if depth < w.keep {
                n.children.push(child);
            }
        } else {
            n.bytes = n.bytes.saturating_add(meta.len());
            if depth < w.keep {
                n.children.push(SizeNode {
                    name: e.file_name().to_string_lossy().into_owned(),
                    path: p.to_string_lossy().into_owned(),
                    bytes: meta.len(),
                    children: vec![],
                });
            }
        }
    }
    n
}

/// The size tree of `root` (already checked with [`check`]).
pub fn size_tree(root: &Path, budget: Budget) -> SizeNode {
    let dev = root.symlink_metadata().map_or(0, |m| device(&m));
    let mut w = Walk { budget, dev, keep: DEPTH };
    let mut tree = node(root, 0, &mut w);
    if w.budget.exhausted() {
        tree.children.push(SizeNode {
            name: "More not counted: the time or entry limit was reached".into(),
            path: String::new(),
            bytes: 0,
            children: vec![],
        });
    }
    prune(tree, PER_LEVEL, DEPTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        for (p, n) in [("a/x", 100), ("a/y/z", 50), ("b", 10), ("c/d/e/f/deep", 1000)] {
            let path = t.path().join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, vec![0u8; n]).unwrap();
        }
        t
    }

    #[test]
    fn sizes_add_up_and_depth_is_bounded() {
        let t = tree();
        let root = std::fs::canonicalize(t.path()).unwrap();
        let n = size_tree(&root, Budget::new(MAX_ENTRIES, 5));
        assert_eq!(n.bytes, 1160);
        assert_eq!(n.children[0].name, "c", "biggest first");
        let c = &n.children[0];
        assert_eq!(c.bytes, 1000);
        assert_eq!(c.children[0].name, "d");
        assert_eq!(c.children[0].children[0].name, "e");
        assert!(
            c.children[0].children[0].children.is_empty(),
            "three levels shown, the rest only counted"
        );
    }

    #[test]
    fn a_used_up_budget_is_said_out_loud() {
        let t = tree();
        let n = size_tree(&std::fs::canonicalize(t.path()).unwrap(), Budget::new(2, 5));
        assert!(n.children.iter().any(|c| c.name.contains("not counted")));
    }

    #[cfg(unix)]
    #[test]
    fn links_are_not_followed() {
        let t = tree();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("huge"), vec![0u8; 5000]).unwrap();
        std::os::unix::fs::symlink(other.path(), t.path().join("shortcut")).unwrap();
        let n = size_tree(&std::fs::canonicalize(t.path()).unwrap(), Budget::new(MAX_ENTRIES, 5));
        assert_eq!(n.bytes, 1160);
    }

    #[test]
    fn paths_are_validated() {
        let t = tree();
        assert!(check("relative/path").is_err());
        assert!(check(&format!("{}/a/../b", t.path().display())).is_err());
        assert!(check(&format!("{}/missing", t.path().display())).is_err());
        assert!(check(&format!("{}/b", t.path().display())).unwrap_err().contains("file"));
        assert!(check(&t.path().display().to_string()).is_ok());
        assert!(default_root().is_absolute());
    }
}
