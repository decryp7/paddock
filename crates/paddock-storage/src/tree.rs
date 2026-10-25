//! The size tree of a category's folder (from Dyno's disk-space map): bounded in depth (levels kept), in items kept per folder (the
//! smallest are folded into one line), in entries looked at and in time; never follows links; stays on the starting volume. When a
//! bound is hit, or the user cancels, the tree says so and its sizes are lower bounds.

use crate::model::SizeNode;
use crate::run::Run;
use crate::walk::{Budget, MAX_DEPTH, device};
use std::path::Path;

/// Levels of children kept below the root.
pub const TREE_DEPTH: usize = 3;
/// Children kept per folder; the rest are folded into one node.
pub const TREE_PER_LEVEL: usize = 12;

struct Walk<'a> {
    budget: &'a mut Budget,
    run: &'a dyn Run,
    volume: u64,
    capped: bool,
}

/// The size tree of `root` named `name`, and whether the walk was cut short.
pub fn size_tree(root: &Path, name: &str, budget: &mut Budget, run: &dyn Run) -> (SizeNode, bool) {
    let volume = std::fs::symlink_metadata(root).map_or(0, |m| device(&m));
    let mut walk = Walk {
        budget,
        run,
        volume,
        capped: false,
    };
    let node = dir_node(&mut walk, root, name, 0);
    (node, walk.capped)
}

fn dir_node(w: &mut Walk<'_>, path: &Path, name: &str, level: usize) -> SizeNode {
    let mut node = SizeNode {
        name: name.to_string(),
        bytes: 0,
        folded: 0,
        children: Vec::new(),
    };
    let Ok(entries) = std::fs::read_dir(path) else {
        return node;
    };
    let keep = level < TREE_DEPTH;
    for entry in entries.flatten() {
        if w.run.cancelled() || !w.budget.take() {
            w.capped = true;
            break;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let ft = meta.file_type();
        if ft.is_file() {
            node.bytes += meta.len();
            if keep {
                node.children.push(SizeNode {
                    name: file_name,
                    bytes: meta.len(),
                    folded: 0,
                    children: Vec::new(),
                });
            }
        } else if ft.is_dir() && device(&meta) == w.volume {
            if level >= MAX_DEPTH {
                w.capped = true;
                continue;
            }
            let child = dir_node(w, &entry.path(), &file_name, level + 1);
            node.bytes += child.bytes;
            if keep {
                node.children.push(child);
            }
        }
    }
    node.children
        .sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    if node.children.len() > TREE_PER_LEVEL {
        let rest = node.children.split_off(TREE_PER_LEVEL);
        node.children.push(SizeNode {
            name: String::new(),
            bytes: rest.iter().map(|c| c.bytes).sum(),
            folded: rest.len() as u64,
            children: Vec::new(),
        });
    }
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::Recorder;
    use crate::walk::tests::{standard, write};

    fn fixture() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        for (p, n) in [
            ("a/x", 100),
            ("a/y/z", 50),
            ("b", 10),
            ("c/d/e/f/deep", 1000),
        ] {
            write(&t.path().join(p), n);
        }
        t
    }

    #[test]
    fn sizes_add_up_biggest_first_and_the_depth_is_bounded() {
        let t = fixture();
        let (n, capped) = size_tree(t.path(), "Cache", &mut standard(), &());
        assert!(!capped);
        assert_eq!((n.name.as_str(), n.bytes), ("Cache", 1160));
        assert_eq!(n.children[0].name, "c", "biggest first");
        let c = &n.children[0];
        assert_eq!(
            (
                c.bytes,
                c.children[0].name.as_str(),
                c.children[0].children[0].name.as_str()
            ),
            (1000, "d", "e")
        );
        assert!(
            c.children[0].children[0].children.is_empty(),
            "three levels are kept, the rest only counted"
        );
        assert_eq!(c.children[0].children[0].bytes, 1000);
    }

    #[test]
    fn the_smallest_items_of_a_busy_folder_are_folded_into_one_line() {
        let t = tempfile::tempdir().unwrap();
        for i in 0..TREE_PER_LEVEL + 5 {
            write(&t.path().join(format!("f{i:02}")), i + 1);
        }
        let (n, _) = size_tree(t.path(), "Busy", &mut standard(), &());
        assert_eq!(n.children.len(), TREE_PER_LEVEL + 1);
        let fold = n.children.last().unwrap();
        assert_eq!((fold.folded, fold.name.as_str()), (5, ""));
        assert_eq!(fold.bytes, (1..=5).sum::<usize>() as u64);
        assert_eq!(
            n.bytes,
            (1..=TREE_PER_LEVEL + 5).sum::<usize>() as u64,
            "the fold keeps the total"
        );
    }

    #[test]
    fn a_used_up_budget_or_cancel_says_the_sizes_are_lower_bounds() {
        let t = fixture();
        let mut tiny = Budget::new(
            2,
            std::time::Duration::from_secs(60),
            std::sync::Arc::new(paddock_jobs::time::SystemClock),
        );
        assert!(size_tree(t.path(), "x", &mut tiny, &()).1);
        let cancelled = Recorder::new();
        cancelled.cancel();
        let (n, capped) = size_tree(t.path(), "x", &mut standard(), &cancelled);
        assert!(capped && n.bytes == 0);
        assert_eq!(
            size_tree(&t.path().join("missing"), "gone", &mut standard(), &())
                .0
                .bytes,
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn links_are_not_followed_and_a_folder_nested_too_deep_is_capped() {
        let t = fixture();
        let other = tempfile::tempdir().unwrap();
        write(&other.path().join("huge"), 5000);
        std::os::unix::fs::symlink(other.path(), t.path().join("shortcut")).unwrap();
        assert_eq!(size_tree(t.path(), "x", &mut standard(), &()).0.bytes, 1160);
        let deep = tempfile::tempdir().unwrap();
        let mut p = deep.path().to_path_buf();
        for _ in 0..MAX_DEPTH + 2 {
            p = p.join("d");
        }
        write(&p.join("f"), 1);
        assert!(size_tree(deep.path(), "x", &mut standard(), &()).1);
    }
}
