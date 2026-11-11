//! The inspected tree: the files under a root listed once and read on demand, as `openTree` in `conformance/src/util/fs.mjs`
//! does (a file-system walk that skips build output and dependencies, not `git ls-files`: the two hosts see the same files).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Folders no check looks into.
const SKIP_DIRS: [&str; 8] = [
    "node_modules",
    ".git",
    "target",
    "dist",
    ".angular",
    "coverage",
    "test-results",
    "playwright-report",
];

/// A read-only view of a checkout.
pub struct Tree {
    root: PathBuf,
    files: RefCell<Option<Rc<Vec<String>>>>,
    cache: RefCell<HashMap<String, Option<Rc<String>>>>,
    /// Folders (relative POSIX paths, `.` for the root) whose entries the listing could not read: the tree is then not completely
    /// known.
    unlisted: RefCell<Vec<String>>,
    /// A tree held in memory (a check's positive control), instead of a folder.
    memory: Option<BTreeMap<String, String>>,
}

impl Tree {
    /// Opens `root`; nothing is read yet.
    pub fn open(root: &Path) -> Tree {
        Tree {
            root: root.to_path_buf(),
            files: RefCell::new(None),
            cache: RefCell::new(HashMap::new()),
            unlisted: RefCell::new(Vec::new()),
            memory: None,
        }
    }

    /// A tree made of the given `(path, text)` pairs, for a check's positive control: no file system involved.
    pub fn in_memory(files: &[(&str, &str)]) -> Tree {
        Tree {
            memory: Some(files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect()),
            ..Tree::open(Path::new(""))
        }
    }

    /// An in-memory tree whose listing reports the given folders as unreadable, as a locked folder would (for tests of the host).
    #[cfg(test)]
    pub fn in_memory_with_unlisted(files: &[(&str, &str)], unlisted: &[&str]) -> Tree {
        let t = Tree::in_memory(files);
        *t.unlisted.borrow_mut() = unlisted.iter().map(|d| d.to_string()).collect();
        t
    }

    /// Whether every folder and entry was listed (a read error is counted, never skipped silently).
    pub fn listing_complete(&self) -> bool {
        self.unlisted().is_empty()
    }

    /// The folders the listing could not read, sorted (empty when the listing is complete).
    pub fn unlisted(&self) -> Vec<String> {
        self.files();
        let mut v = self.unlisted.borrow().clone();
        v.sort();
        v.dedup();
        v
    }

    /// The root folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every file (relative POSIX paths, sorted), listed once. Symbolic links are neither files nor folders here.
    pub fn files(&self) -> Rc<Vec<String>> {
        if let Some(f) = self.files.borrow().as_ref() {
            return f.clone();
        }
        let mut out = Vec::new();
        match &self.memory {
            Some(m) => out.extend(m.keys().cloned()),
            None => walk(&self.root, "", &mut out, &self.unlisted),
        }
        out.sort();
        let rc = Rc::new(out);
        *self.files.borrow_mut() = Some(rc.clone());
        rc
    }

    /// The file's text (invalid UTF-8 becomes U+FFFD, as Node does), or `None` when it cannot be read.
    pub fn read(&self, rel: &str) -> Option<Rc<String>> {
        if let Some(hit) = self.cache.borrow().get(rel) {
            return hit.clone();
        }
        if let Some(m) = &self.memory {
            return m.get(rel).map(|t| Rc::new(t.clone()));
        }
        let text = std::fs::read(self.root.join(rel))
            .ok()
            .map(|b| Rc::new(String::from_utf8_lossy(&b).into_owned()));
        self.cache.borrow_mut().insert(rel.to_string(), text.clone());
        text
    }
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>, unlisted: &RefCell<Vec<String>>) {
    let here = || {
        unlisted
            .borrow_mut()
            .push(if prefix.is_empty() { ".".into() } else { prefix.to_string() })
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        here();
        return;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            here();
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            here();
            continue;
        };
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if kind.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk(&entry.path(), &rel, out, unlisted);
            }
        } else if kind.is_file() {
            out.push(rel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_sorted_skips_build_output_and_reads_lossily() {
        let d = tempfile::tempdir().unwrap();
        for (p, c) in [
            ("b/x.rs", "x"),
            ("a.rs", "a"),
            ("target/t.rs", "t"),
            ("node_modules/n/i.rs", "n"),
            ("c/dist/d.rs", "d"),
        ] {
            let f = d.path().join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, c).unwrap();
        }
        std::fs::write(d.path().join("bad.rs"), [b'a', 0xff]).unwrap();
        let t = Tree::open(d.path());
        assert_eq!(*t.files(), vec!["a.rs", "b/x.rs", "bad.rs"]);
        assert_eq!(t.read("bad.rs").unwrap().as_str(), "a\u{fffd}");
        assert!(t.read("missing.rs").is_none());
        assert!(t.listing_complete());
    }

    #[cfg(unix)]
    #[test]
    fn an_unlistable_folder_is_counted_not_skipped() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("locked");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("a.rs"), "x").unwrap();
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable_anyway = std::fs::read_dir(&sub).is_ok(); // root ignores the mode (CI containers)
        let t = Tree::open(d.path());
        let complete = t.listing_complete();
        let unlisted = t.unlisted();
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            complete, readable_anyway,
            "the locked folder must make the listing incomplete unless it can be read"
        );
        assert_eq!(
            unlisted,
            if readable_anyway {
                Vec::<String>::new()
            } else {
                vec!["locked".to_string()]
            }
        );
    }

    #[test]
    fn an_in_memory_tree_lists_and_reads() {
        let t = Tree::in_memory(&[("b.rs", "b"), ("a/x.rs", "x")]);
        assert_eq!(*t.files(), vec!["a/x.rs", "b.rs"]);
        assert_eq!(t.read("b.rs").unwrap().as_str(), "b");
        assert!(t.listing_complete());
    }
}
