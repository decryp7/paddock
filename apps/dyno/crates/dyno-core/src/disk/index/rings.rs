//! Several levels of a scan in one answer, for the sunburst: the nested children under a folder down to a depth, the small ones
//! of each level folded into that level's "other", and the whole answer capped. It replaces many one-level reads.

use super::{DiskIndex, DiskNode, Folded, IndexError, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// The deepest ring count one answer may have.
pub const MAX_RINGS: u8 = 6;
/// The most nodes one answer may hold.
pub const MAX_RING_NODES: u32 = 2_000;

/// A folder or file with what is shown inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RingNode {
    /// The node itself.
    pub node: DiskNode,
    /// The children shown, biggest first (empty for a file, an unread folder, or a ring that was not asked for).
    pub children: Vec<RingNode>,
    /// The children not shown because they are below the minimum share or past the node cap.
    pub other: Folded,
    /// Small files directly inside that have no node of their own.
    pub loose: Folded,
}

/// The answer: the centre with its nested rings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskRings {
    /// The centre and everything shown under it.
    pub root: RingNode,
    /// How many nodes are in the answer, the centre included.
    pub count: u32,
    /// The node cap left out children that were big enough to show.
    pub capped: bool,
}

struct Built {
    node: DiskNode,
    kids: Vec<usize>,
    other: Folded,
    loose: Folded,
}

fn fold(f: Folded, n: &DiskNode) -> Folded {
    Folded {
        count: f.count + 1,
        bytes: f.bytes.saturating_add(n.bytes),
        logical_bytes: f.logical_bytes.saturating_add(n.logical_bytes),
        items: f.items.saturating_add(n.items),
    }
}

impl DiskIndex {
    /// The levels under `id` down to `rings` deep (clamped to 1..=[`MAX_RINGS`]). A child is shown when it has at least
    /// `min_share` of the *centre's* bytes (0 shows all, a child with no bytes is then folded); the answer holds at most
    /// `max_nodes` nodes (1..=[`MAX_RING_NODES`]), taken level by level, biggest first, so a cap cuts the smallest outer
    /// ones. What is not shown is folded into each folder's `other`. One pass over at most the visited folders' children.
    pub fn rings(&self, id: NodeId, rings: u8, min_share: f64, max_nodes: u32) -> Result<DiskRings, IndexError> {
        if (id.0 as usize) >= self.nodes.len() {
            return Err(IndexError::NoSuchNode);
        }
        let (rings, budget) = (rings.clamp(1, MAX_RINGS), max_nodes.clamp(1, MAX_RING_NODES) as usize);
        let centre = self.info(id.0);
        let floor = if min_share > 0.0 {
            ((centre.bytes as f64 * min_share.min(1.0)).ceil() as u64).max(1)
        } else {
            0
        };
        let loose_of = |i: u32| {
            let n = &self.nodes[i as usize];
            Folded {
                count: n.loose_items,
                bytes: n.loose_bytes,
                logical_bytes: n.loose_logical,
                items: n.loose_items,
            }
        };
        let mut built = vec![Built {
            node: centre,
            kids: Vec::new(),
            other: Folded::default(),
            loose: loose_of(id.0),
        }];
        let mut queue = VecDeque::from([(0usize, id.0, 0u8)]);
        let mut capped = false;
        while let Some((at, node, depth)) = queue.pop_front() {
            if depth >= rings {
                continue;
            }
            let mut other = Folded::default();
            for kid in self.sorted_kids(node) {
                let info = self.info(kid);
                if info.bytes < floor {
                    other = fold(other, &info);
                } else if built.len() >= budget {
                    capped = true;
                    other = fold(other, &info);
                } else {
                    built.push(Built {
                        loose: loose_of(kid),
                        node: info,
                        kids: Vec::new(),
                        other: Folded::default(),
                    });
                    let child = built.len() - 1;
                    built[at].kids.push(child);
                    queue.push_back((child, kid, depth + 1));
                }
            }
            built[at].other = other;
        }
        let count = u32::try_from(built.len()).unwrap_or(u32::MAX);
        // Children were pushed after their parents, so building from the back finishes every child before its parent.
        let mut done: Vec<Option<RingNode>> = (0..built.len()).map(|_| None).collect();
        for (i, b) in built.into_iter().enumerate().rev() {
            let children = b.kids.iter().filter_map(|k| done[*k].take()).collect();
            done[i] = Some(RingNode {
                node: b.node,
                children,
                other: b.other,
                loose: b.loose,
            });
        }
        let root = done.swap_remove(0).ok_or(IndexError::NoSuchNode)?;
        Ok(DiskRings { root, count, capped })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::index::{ScanLimits, Sizes};
    use crate::time::Timestamp;

    const MB: u64 = 1024 * 1024;

    fn limits() -> ScanLimits {
        ScanLimits {
            max_nodes: 1000,
            notable_bytes: MB,
            max_hard_links: 10,
        }
    }

    /// root: a (x 30 MB, y 10 MB, tiny 5 B), b (c (z 8 MB)), d (1 MB), loose 7 B
    fn sample() -> DiskIndex {
        let mut i = DiskIndex::new("vol", limits());
        let r = i.root();
        let a = i.add_folder(r, "a").unwrap();
        i.add_file(a, "x", Sizes::same(30 * MB), Timestamp(1));
        i.add_file(a, "y", Sizes::same(10 * MB), Timestamp(1));
        i.add_file(a, "tiny", Sizes::same(5), Timestamp(1));
        let b = i.add_folder(r, "b").unwrap();
        let c = i.add_folder(b, "c").unwrap();
        i.add_file(c, "z", Sizes::same(8 * MB), Timestamp(1));
        let d = i.add_folder(r, "d").unwrap();
        i.add_file(d, "w", Sizes::same(MB), Timestamp(1));
        i.add_file(r, "loose", Sizes::same(7), Timestamp(1));
        for f in [c, b, a, d, r] {
            i.finish_listing(f);
        }
        i
    }

    fn names(n: &RingNode) -> Vec<&str> {
        n.children.iter().map(|c| c.node.name.as_str()).collect()
    }

    #[test]
    fn levels_are_nested_biggest_first_down_to_the_asked_depth() {
        let i = sample();
        let two = i.rings(i.root(), 2, 0.0, 100).unwrap();
        assert_eq!(names(&two.root), ["a", "b", "d"]);
        assert_eq!(names(&two.root.children[0]), ["x", "y"]);
        assert_eq!(names(&two.root.children[1]), ["c"], "b's child c is ring 2");
        assert!(two.root.children[1].children[0].children.is_empty(), "z is ring 3: not asked for");
        assert_eq!(two.root.children[0].loose.bytes, 5, "tiny files are the folder's loose bytes");
        assert_eq!(two.root.loose.bytes, 7);
        assert_eq!((two.count, two.capped), (8, false), "root, a, b, d, x, y, c, w");
        let three = i.rings(i.root(), 3, 0.0, 100).unwrap();
        assert_eq!(names(&three.root.children[1].children[0]), ["z"]);
    }

    #[test]
    fn small_children_are_folded_by_share_of_the_centre_and_sizes_add_up_per_level() {
        let i = sample();
        // 5% of 49 MB is 2.45 MB: x, y and c's 8 MB stay; d (1 MB) and the tiny file fold.
        let r = i.rings(i.root(), 3, 0.05, 100).unwrap();
        assert_eq!(names(&r.root), ["a", "b"]);
        assert_eq!(
            (r.root.other.count, r.root.other.bytes),
            (1, MB),
            "d is folded into the centre's other"
        );
        for n in [&r.root, &r.root.children[0]] {
            let shown: u64 = n.children.iter().map(|c| c.node.bytes).sum();
            assert_eq!(
                shown + n.other.bytes + n.loose.bytes,
                n.node.bytes,
                "children + other + loose = the folder"
            );
        }
        let all = i.rings(i.root(), 1, 1.0, 100).unwrap();
        assert!(
            all.root.children.is_empty() && all.root.other.count == 3,
            "a share of 1 shows nothing but folds all"
        );
    }

    #[test]
    fn the_node_cap_cuts_the_smallest_outer_nodes_and_says_so() {
        let i = sample();
        let capped = i.rings(i.root(), 3, 0.0, 4).unwrap();
        assert_eq!(capped.count, 4);
        assert!(capped.capped);
        assert_eq!(names(&capped.root), ["a", "b", "d"], "ring 1 comes first");
        assert!(capped.root.children.iter().all(|c| c.children.is_empty()), "no room for ring 2");
        let a = &capped.root.children[0];
        assert_eq!(a.other.count, 2, "x and y did not fit and are folded into a's other");
        let one = i.rings(i.root(), 6, 0.0, 1).unwrap();
        assert_eq!((one.count, one.root.children.len(), one.root.other.count), (1, 0, 3));
    }

    #[test]
    fn limits_are_clamped_and_unknown_nodes_refused() {
        let i = sample();
        assert_eq!(
            i.rings(i.root(), 0, 0.0, 0).unwrap().count,
            1,
            "zero rings and zero nodes become one each"
        );
        assert_eq!(
            i.rings(i.root(), 200, 0.0, u32::MAX).unwrap().count,
            9,
            "the depth is clamped to 6, the cap to 2000"
        );
        assert_eq!(i.rings(NodeId(9999), 1, 0.0, 10), Err(IndexError::NoSuchNode));
    }

    #[test]
    fn a_partial_scan_gives_its_partial_tree() {
        let mut i = DiskIndex::new("vol", limits());
        let r = i.root();
        let a = i.add_folder(r, "a").unwrap();
        i.add_file(a, "x", Sizes::same(3 * MB), Timestamp(1));
        let locked = i.add_folder(r, "locked").unwrap();
        i.mark_unreadable(locked);
        // Nothing finished: a is still being listed.
        let out = i.rings(r, 2, 0.0, 10).unwrap();
        assert!(!out.root.node.complete);
        assert_eq!(names(&out.root), ["a", "locked"]);
        assert!(!out.root.children[0].node.complete);
        assert_eq!(out.root.children[1].node.kind, crate::disk::index::NodeKind::Unreadable);
    }

    #[test]
    fn the_wire_shape_is_camel_case() {
        let i = sample();
        let v = serde_json::to_value(i.rings(i.root(), 1, 0.0, 10).unwrap()).unwrap();
        assert!(v["root"]["children"][0]["node"]["logicalBytes"].is_u64() && v["capped"].is_boolean() && v["count"].is_u64());
        assert!(v["root"]["other"]["logicalBytes"].is_u64());
    }
}
