//! The disk-space map's data (read-only): a folder's size tree, how it is bounded for the UI, and the check that a
//! folder path names no `.` or `..` segment.

pub mod index;
pub mod scan;
pub mod source;
pub mod volume;

use serde::{Deserialize, Serialize};

/// A folder's size with its biggest children (the disk-space treemap).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeNode {
    /// Name.
    pub name: String,
    /// Full path.
    pub path: String,
    /// Bytes, including children.
    pub bytes: u64,
    /// Biggest children.
    pub children: Vec<SizeNode>,
}

/// Whether `path` has a `.` or `..` segment. `Path::components` reads `..` as a parent step only on ordinary paths: in a Windows
/// verbatim path (`\\?\C:\..`) it is a plain name, so this checks the names too and refuses both forms.
pub fn has_dot_segment(path: &std::path::Path) -> bool {
    use std::path::Component;
    path.components().any(|c| match c {
        Component::ParentDir | Component::CurDir => true,
        Component::Normal(n) => n == ".." || n == ".",
        _ => false,
    })
}

/// Bounds a size tree for the UI: at most `per_level` children per node (the rest folded into "Other") and `depth`
/// levels.
pub fn prune(mut node: SizeNode, per_level: usize, depth: usize) -> SizeNode {
    node.children.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    if depth == 0 {
        node.children.clear();
        return node;
    }
    if node.children.len() > per_level {
        let rest: u64 = node.children[per_level..].iter().map(|c| c.bytes).sum();
        let count = node.children.len() - per_level;
        node.children.truncate(per_level);
        node.children.push(SizeNode {
            name: format!("{count} other items"),
            path: String::new(),
            bytes: rest,
            children: vec![],
        });
    }
    node.children = node.children.into_iter().map(|c| prune(c, per_level, depth - 1)).collect();
    node
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_segments_are_found_in_any_spelling() {
        use std::path::Path;
        assert!(has_dot_segment(Path::new("/a/../b")));
        assert!(!has_dot_segment(Path::new("/a/b..c/.hidden/..x")));
    }

    #[test]
    fn size_trees_are_bounded() {
        let leaf = |n: &str, b| SizeNode {
            name: n.into(),
            path: format!("/{n}"),
            bytes: b,
            children: vec![],
        };
        let mut deep = leaf("a", 100);
        deep.children = vec![leaf("x", 60), leaf("y", 30), leaf("z", 10)];
        let mut root = leaf("root", 200);
        root.children = vec![leaf("b", 50), deep, leaf("c", 40), leaf("d", 10)];
        let p = prune(root, 2, 1);
        assert_eq!(p.children.len(), 3);
        assert_eq!(p.children[0].name, "a");
        assert!(p.children[0].children.is_empty(), "depth 1");
        assert_eq!(p.children[2].name, "2 other items");
        assert_eq!(p.children[2].bytes, 50);
    }
}
