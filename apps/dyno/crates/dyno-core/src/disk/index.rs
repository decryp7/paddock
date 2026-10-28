//! The size index of a disk scan: a compact arena of folders (and the notable files), with sizes that add up from the
//! leaves to the root at every moment, so a half-finished or cancelled scan is still a consistent tree.
//!
//! Memory is bounded: a node is about 80 bytes plus its name, files below [`ScanLimits::notable_bytes`] are folded into
//! their folder, and past [`ScanLimits::max_nodes`] no new node is made (the index says so with [`DiskIndex::truncated`]).
//! Queries return one level at a time, paged, with the small children folded into one "other" bucket.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

const NONE: u32 = u32::MAX;
/// The most nodes any scan may hold, whatever the Setting says (about 400 MB at the bound).
pub const HARD_MAX_NODES: u32 = 5_000_000;
/// The most children one page returns.
pub const MAX_PAGE: u32 = 256;
/// Names longer than this many bytes are cut (no file system allows more).
const MAX_NAME: usize = 255;

/// What a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    /// A folder that was (or is being) read.
    Folder,
    /// A file big enough to have its own node.
    File,
    /// Another volume mounted inside the scanned one; not entered.
    MountPoint,
    /// A folder the OS would not let us read; its size is not in the total.
    Unreadable,
}

/// The bounds of one index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanLimits {
    /// The most nodes (Setting `disk.scan.folders`).
    pub max_nodes: u32,
    /// Files of at least this many bytes get their own node; smaller ones are folded into their folder.
    pub notable_bytes: u64,
    /// The most multi-link files remembered to count each only once; further ones are counted per name.
    pub max_hard_links: u32,
}

impl Default for ScanLimits {
    fn default() -> Self {
        ScanLimits {
            max_nodes: 2_000_000,
            notable_bytes: 10 * 1024 * 1024,
            max_hard_links: 1_000_000,
        }
    }
}

impl ScanLimits {
    /// The limits clamped to what is allowed (at least one node, at most [`HARD_MAX_NODES`]).
    pub fn validated(self) -> ScanLimits {
        ScanLimits {
            max_nodes: self.max_nodes.clamp(1, HARD_MAX_NODES),
            ..self
        }
    }
}

/// A node's id inside one index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

/// Why a query could not be answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IndexError {
    /// No such node (a stale id from another scan).
    #[error("that folder is not in this scan")]
    NoSuchNode,
}

struct Node {
    parent: u32,
    first_child: u32,
    next_sibling: u32,
    name_off: u32,
    name_len: u16,
    kind: NodeKind,
    complete: bool,
    /// Folders: the listing itself (1 until finished) plus the child folders not complete yet.
    open: u32,
    child_count: u32,
    bytes: u64,
    items: u64,
    loose_bytes: u64,
    loose_items: u64,
    newest: i64,
}

/// A node as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskNode {
    /// Id inside the scan.
    pub id: u32,
    /// File or folder name (the root: the volume or folder name).
    pub name: String,
    /// What it is.
    pub kind: NodeKind,
    /// Bytes on the volume, children included. Only grows during a scan.
    pub bytes: u64,
    /// Files and folders inside, recursively.
    pub items: u64,
    /// Newest modification inside.
    pub newest_change: Timestamp,
    /// Whether everything inside has been read.
    pub complete: bool,
    /// Whether there is anything inside to open.
    pub has_children: bool,
}

/// A sum of folded things.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folded {
    /// How many nodes (or files) are in it.
    pub count: u64,
    /// Their bytes.
    pub bytes: u64,
    /// Their items.
    pub items: u64,
}

/// One page of one level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskChildren {
    /// The folder asked about.
    pub node: DiskNode,
    /// This page of its children, biggest first.
    pub children: Vec<DiskNode>,
    /// Children below the minimum share, folded into one bucket (all pages).
    pub other: Folded,
    /// Small files directly inside the folder that have no node of their own.
    pub loose: Folded,
    /// How many children are shown across all pages (the ones at or above the minimum share).
    pub shown: u32,
    /// The offset of this page.
    pub offset: u32,
    /// The offset of the next page, `None` on the last.
    pub next_offset: Option<u32>,
}

/// The arena.
pub struct DiskIndex {
    nodes: Vec<Node>,
    names: Vec<u8>,
    limits: ScanLimits,
    truncated: bool,
    skipped_folders: u64,
}

fn clamp_name(name: &str) -> &str {
    if name.len() <= MAX_NAME {
        return name;
    }
    let mut end = MAX_NAME;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

impl DiskIndex {
    /// An index with a root folder called `root_name` (its listing is open until [`finish_listing`](Self::finish_listing)).
    pub fn new(root_name: &str, limits: ScanLimits) -> DiskIndex {
        let mut i = DiskIndex {
            nodes: Vec::new(),
            names: Vec::new(),
            limits: limits.validated(),
            truncated: false,
            skipped_folders: 0,
        };
        let name = i.intern(root_name);
        i.nodes.push(Node {
            parent: NONE,
            first_child: NONE,
            next_sibling: NONE,
            name_off: name.0,
            name_len: name.1,
            kind: NodeKind::Folder,
            complete: false,
            open: 1,
            child_count: 0,
            bytes: 0,
            items: 0,
            loose_bytes: 0,
            loose_items: 0,
            newest: 0,
        });
        i
    }

    fn intern(&mut self, name: &str) -> (u32, u16) {
        let name = clamp_name(name);
        let off = u32::try_from(self.names.len()).unwrap_or(u32::MAX);
        self.names.extend_from_slice(name.as_bytes());
        (off, u16::try_from(name.len()).unwrap_or(u16::MAX))
    }

    /// The root.
    pub fn root(&self) -> NodeId {
        NodeId(0)
    }

    /// How many nodes there are.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Never empty: the root always exists.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Whether a limit stopped nodes from being made.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Folders left out because the node limit was reached.
    pub fn skipped_folders(&self) -> u64 {
        self.skipped_folders
    }

    /// The limits in force.
    pub fn limits(&self) -> ScanLimits {
        self.limits
    }

    /// The root's totals as (bytes, items).
    pub fn totals(&self) -> (u64, u64) {
        let r = &self.nodes[0];
        (r.bytes, r.items)
    }

    /// Whether the whole tree is complete (every listing finished, nothing unreadable).
    pub fn is_complete(&self) -> bool {
        self.nodes[0].complete
    }

    fn room(&mut self) -> bool {
        let full = self.nodes.len() >= self.limits.max_nodes as usize || self.names.len() >= (u32::MAX as usize) - MAX_NAME;
        if full {
            self.truncated = true;
        }
        !full
    }

    fn bump(&mut self, from: u32, bytes: u64, items: u64, mtime: i64) {
        let mut at = from;
        while at != NONE {
            let n = &mut self.nodes[at as usize];
            n.bytes = n.bytes.saturating_add(bytes);
            n.items = n.items.saturating_add(items);
            n.newest = n.newest.max(mtime);
            at = n.parent;
        }
    }

    fn link(&mut self, parent: NodeId, node: Node) -> NodeId {
        let id = u32::try_from(self.nodes.len()).unwrap_or(NONE);
        let mut node = node;
        node.parent = parent.0;
        let p = &mut self.nodes[parent.0 as usize];
        node.next_sibling = p.first_child;
        p.first_child = id;
        p.child_count += 1;
        self.nodes.push(node);
        NodeId(id)
    }

    fn blank(&mut self, name: &str, kind: NodeKind) -> Node {
        let (off, len) = self.intern(name);
        Node {
            parent: NONE,
            first_child: NONE,
            next_sibling: NONE,
            name_off: off,
            name_len: len,
            kind,
            complete: false,
            open: 0,
            child_count: 0,
            bytes: 0,
            items: 0,
            loose_bytes: 0,
            loose_items: 0,
            newest: 0,
        }
    }

    /// Adds a folder under `parent`, to be listed later. `None` when the node limit is reached (the folder is counted as
    /// skipped and must not be walked).
    pub fn add_folder(&mut self, parent: NodeId, name: &str) -> Option<NodeId> {
        if !self.room() {
            self.skipped_folders += 1;
            return None;
        }
        let mut n = self.blank(name, NodeKind::Folder);
        n.open = 1;
        let id = self.link(parent, n);
        self.nodes[parent.0 as usize].open += 1;
        self.bump(parent.0, 0, 1, 0);
        Some(id)
    }

    /// Adds a mount point under `parent`: another volume, shown but not entered, complete with no size.
    pub fn add_mount_point(&mut self, parent: NodeId, name: &str) -> bool {
        if !self.room() {
            return false;
        }
        let mut n = self.blank(name, NodeKind::MountPoint);
        n.complete = true;
        self.link(parent, n);
        self.bump(parent.0, 0, 1, 0);
        true
    }

    /// Marks a folder as unreadable: it stays incomplete for good, and so do its ancestors.
    pub fn mark_unreadable(&mut self, node: NodeId) {
        self.nodes[node.0 as usize].kind = NodeKind::Unreadable;
    }

    /// Adds a file under `parent`. A notable file gets a node; any other is folded into `parent`'s loose totals.
    pub fn add_file(&mut self, parent: NodeId, name: &str, bytes: u64, mtime: Timestamp) {
        if bytes >= self.limits.notable_bytes && self.room() {
            let mut n = self.blank(name, NodeKind::File);
            n.complete = true;
            n.bytes = bytes;
            n.items = 1;
            n.newest = mtime.0;
            self.link(parent, n);
            self.bump(parent.0, bytes, 1, mtime.0);
        } else {
            self.add_folded_files(parent, bytes, 1, mtime);
        }
    }

    /// Adds files that were already summed by the caller (small ones, folded into `parent`).
    pub fn add_folded_files(&mut self, parent: NodeId, bytes: u64, items: u64, newest: Timestamp) {
        let p = &mut self.nodes[parent.0 as usize];
        p.loose_bytes = p.loose_bytes.saturating_add(bytes);
        p.loose_items = p.loose_items.saturating_add(items);
        self.bump(parent.0, bytes, items, newest.0);
    }

    /// Ends the listing of a folder: it is complete once every child folder is too.
    pub fn finish_listing(&mut self, node: NodeId) {
        let mut at = node.0;
        while at != NONE {
            let n = &mut self.nodes[at as usize];
            n.open = n.open.saturating_sub(1);
            if n.open > 0 || n.complete || n.kind == NodeKind::Unreadable {
                return;
            }
            n.complete = true;
            at = n.parent;
        }
    }

    fn info(&self, id: u32) -> DiskNode {
        let n = &self.nodes[id as usize];
        DiskNode {
            id,
            name: self.name(id),
            kind: n.kind,
            bytes: n.bytes,
            items: n.items,
            newest_change: Timestamp(n.newest),
            complete: n.complete,
            has_children: n.child_count > 0 || n.loose_items > 0,
        }
    }

    fn name(&self, id: u32) -> String {
        let n = &self.nodes[id as usize];
        let s = n.name_off as usize;
        String::from_utf8_lossy(&self.names[s..s + n.name_len as usize]).into_owned()
    }

    /// One node's summary.
    pub fn node(&self, id: NodeId) -> Result<DiskNode, IndexError> {
        if (id.0 as usize) >= self.nodes.len() {
            return Err(IndexError::NoSuchNode);
        }
        Ok(self.info(id.0))
    }

    /// The names from the root down to `id` (the root's own name first).
    pub fn path_names(&self, id: NodeId) -> Result<Vec<String>, IndexError> {
        if (id.0 as usize) >= self.nodes.len() {
            return Err(IndexError::NoSuchNode);
        }
        let mut out = Vec::new();
        let mut at = id.0;
        while at != NONE {
            out.push(self.name(at));
            at = self.nodes[at as usize].parent;
        }
        out.reverse();
        Ok(out)
    }

    /// One page of the children of `id`, biggest first. Children below `min_fraction` of the folder's size (0 shows all)
    /// are folded into [`DiskChildren::other`]; `limit` is clamped to 1..=[`MAX_PAGE`].
    pub fn children(&self, id: NodeId, offset: u32, limit: u32, min_fraction: f64) -> Result<DiskChildren, IndexError> {
        if (id.0 as usize) >= self.nodes.len() {
            return Err(IndexError::NoSuchNode);
        }
        let parent = &self.nodes[id.0 as usize];
        let mut kids: Vec<u32> = Vec::with_capacity(parent.child_count as usize);
        let mut at = parent.first_child;
        while at != NONE {
            kids.push(at);
            at = self.nodes[at as usize].next_sibling;
        }
        kids.sort_unstable_by(|a, b| {
            let (x, y) = (&self.nodes[*a as usize], &self.nodes[*b as usize]);
            y.bytes.cmp(&x.bytes).then_with(|| self.name(*a).cmp(&self.name(*b))).then(a.cmp(b))
        });
        let min_bytes = if min_fraction > 0.0 {
            (parent.bytes as f64 * min_fraction.min(1.0)).ceil() as u64
        } else {
            0
        };
        // With a minimum share, a child with no bytes is always folded.
        let floor = if min_fraction > 0.0 { min_bytes.max(1) } else { 0 };
        let shown = kids.iter().take_while(|k| self.nodes[**k as usize].bytes >= floor).count();
        let other = kids[shown..].iter().fold(Folded::default(), |f, k| {
            let n = &self.nodes[*k as usize];
            Folded {
                count: f.count + 1,
                bytes: f.bytes.saturating_add(n.bytes),
                items: f.items.saturating_add(n.items),
            }
        });
        let limit = limit.clamp(1, MAX_PAGE) as usize;
        let start = (offset as usize).min(shown);
        let end = (start + limit).min(shown);
        let next_offset = (end < shown).then(|| u32::try_from(end).unwrap_or(u32::MAX));
        Ok(DiskChildren {
            node: self.info(id.0),
            children: kids[start..end].iter().map(|k| self.info(*k)).collect(),
            other,
            loose: Folded {
                count: parent.loose_items,
                bytes: parent.loose_bytes,
                items: parent.loose_items,
            },
            shown: u32::try_from(shown).unwrap_or(u32::MAX),
            offset: u32::try_from(start).unwrap_or(u32::MAX),
            next_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    fn limits(max_nodes: u32) -> ScanLimits {
        ScanLimits {
            max_nodes,
            notable_bytes: MB,
            max_hard_links: 10,
        }
    }

    /// root -> a (big 5 MB file, 10 small bytes), b (3 MB file), c (empty), loose 7 bytes in root.
    fn sample() -> DiskIndex {
        let mut i = DiskIndex::new("vol", limits(100));
        let r = i.root();
        let a = i.add_folder(r, "a").unwrap();
        i.add_file(a, "big.bin", 5 * MB, Timestamp(50));
        i.add_file(a, "small", 10, Timestamp(60));
        let b = i.add_folder(r, "b").unwrap();
        i.add_file(b, "mid.bin", 3 * MB, Timestamp(70));
        let c = i.add_folder(r, "c").unwrap();
        i.add_file(r, "loose", 7, Timestamp(5));
        for f in [a, b, c] {
            i.finish_listing(f);
        }
        i
    }

    #[test]
    fn sizes_add_up_at_every_level() {
        let i = sample();
        let page = i.children(i.root(), 0, 10, 0.0).unwrap();
        assert_eq!(page.node.bytes, 8 * MB + 17);
        let sum: u64 = page.children.iter().map(|c| c.bytes).sum::<u64>() + page.other.bytes + page.loose.bytes;
        assert_eq!(sum, page.node.bytes, "children + other + loose = parent");
        assert_eq!(page.children[0].name, "a", "biggest first");
        assert_eq!(page.children[1].name, "b");
        assert_eq!(
            page.loose,
            Folded {
                count: 1,
                bytes: 7,
                items: 1
            }
        );
        assert_eq!(page.node.newest_change, Timestamp(70));
        assert_eq!(i.totals(), (8 * MB + 17, 7), "a, b, c, big, small, mid, loose");
    }

    #[test]
    fn small_files_fold_and_notable_files_get_nodes() {
        let i = sample();
        let a = i.children(i.root(), 0, 10, 0.0).unwrap().children[0].id;
        let page = i.children(NodeId(a), 0, 10, 0.0).unwrap();
        assert_eq!(page.children.len(), 1);
        assert_eq!(page.children[0].kind, NodeKind::File);
        assert_eq!(page.loose.bytes, 10);
        assert!(page.node.has_children);
        assert!(!page.children[0].has_children);
        assert!(i.children(NodeId(a), 0, 10, 0.0).unwrap().children[0].complete);
    }

    #[test]
    fn the_other_bucket_folds_children_below_the_minimum_share() {
        let i = sample();
        let page = i.children(i.root(), 0, 10, 0.5).unwrap();
        assert_eq!(page.shown, 1, "only a is at least half");
        assert_eq!(page.children.len(), 1);
        assert_eq!(page.other.count, 2, "b and the empty c");
        assert_eq!(page.other.bytes, 3 * MB);
        let all = page.children[0].bytes + page.other.bytes + page.loose.bytes;
        assert_eq!(all, page.node.bytes);
        let page = i.children(i.root(), 0, 10, 1.5).unwrap();
        assert_eq!(page.shown, 0, "a share above 100 percent folds everything");
        assert_eq!(page.other.count, 3);
    }

    #[test]
    fn pages_cover_the_level_once() {
        let mut i = DiskIndex::new("r", limits(1000));
        let r = i.root();
        for n in 0..10 {
            let f = i.add_folder(r, &format!("f{n}")).unwrap();
            i.add_file(f, "x", 2 * MB + n, Timestamp(1));
            i.finish_listing(f);
        }
        let mut seen = Vec::new();
        let mut offset = 0;
        loop {
            let p = i.children(r, offset, 4, 0.0).unwrap();
            assert!(p.children.len() <= 4);
            seen.extend(p.children.iter().map(|c| c.name.clone()));
            match p.next_offset {
                Some(n) => offset = n,
                None => break,
            }
        }
        assert_eq!(seen.len(), 10);
        assert_eq!(seen[0], "f9", "biggest first");
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 10, "no child twice");
        let past = i.children(r, 99, 4, 0.0).unwrap();
        assert!(past.children.is_empty() && past.next_offset.is_none());
        let zero = i.children(r, 0, 0, 0.0).unwrap();
        assert_eq!(zero.children.len(), 1, "a limit of 0 is raised to 1");
        let huge = i.children(r, 0, 100_000, 0.0).unwrap();
        assert_eq!(huge.children.len(), 10, "the page is capped at MAX_PAGE");
    }

    #[test]
    fn completeness_flows_up_and_unreadable_folders_stay_open() {
        let mut i = DiskIndex::new("r", limits(100));
        let r = i.root();
        let a = i.add_folder(r, "a").unwrap();
        let b = i.add_folder(a, "b").unwrap();
        assert!(!i.is_complete());
        i.finish_listing(a);
        assert!(!i.node(NodeId(a.0)).unwrap().complete, "b is still open");
        i.finish_listing(b);
        assert!(i.node(a).unwrap().complete);
        i.finish_listing(r);
        assert!(i.is_complete());

        let mut j = DiskIndex::new("r", limits(100));
        let r = j.root();
        let locked = j.add_folder(r, "Library").unwrap();
        j.mark_unreadable(locked);
        j.finish_listing(locked);
        j.finish_listing(r);
        assert!(!j.is_complete(), "an unreadable folder keeps the volume incomplete");
        let kids = j.children(r, 0, 10, 0.0).unwrap();
        assert_eq!(kids.children[0].kind, NodeKind::Unreadable);
        assert!(!kids.children[0].complete);
    }

    #[test]
    fn mount_points_are_shown_but_not_entered() {
        let mut i = DiskIndex::new("r", limits(100));
        let r = i.root();
        assert!(i.add_mount_point(r, "Volumes"));
        i.finish_listing(r);
        let p = i.children(r, 0, 10, 0.0).unwrap();
        assert_eq!(p.children[0].kind, NodeKind::MountPoint);
        assert!(p.children[0].complete);
        assert!(i.is_complete());
    }

    #[test]
    fn the_node_limit_is_reported_and_keeps_sums_right() {
        let mut i = DiskIndex::new("r", limits(3));
        let r = i.root();
        let a = i.add_folder(r, "a").unwrap();
        assert!(i.add_folder(r, "b").is_some());
        assert!(!i.truncated());
        assert!(i.add_folder(r, "c").is_none(), "the root and two folders fill three nodes");
        assert!(i.truncated());
        assert_eq!(i.skipped_folders(), 1);
        i.add_file(a, "big", 9 * MB, Timestamp(1));
        let p = i.children(a, 0, 10, 0.0).unwrap();
        assert!(p.children.is_empty(), "no room for a node: folded into loose");
        assert_eq!(p.loose.bytes, 9 * MB);
        assert_eq!(i.totals().0, 9 * MB);
        assert!(!i.add_mount_point(r, "m"));
        assert_eq!(i.len(), 3);
        assert!(!i.is_empty());
    }

    #[test]
    fn limits_are_clamped() {
        assert_eq!(
            ScanLimits {
                max_nodes: 0,
                ..ScanLimits::default()
            }
            .validated()
            .max_nodes,
            1
        );
        assert_eq!(
            ScanLimits {
                max_nodes: u32::MAX,
                ..ScanLimits::default()
            }
            .validated()
            .max_nodes,
            HARD_MAX_NODES
        );
        let d = ScanLimits::default();
        assert_eq!(
            (d.max_nodes, d.notable_bytes),
            (2_000_000, 10 * MB),
            "the real defaults, pinned once"
        );
        assert_eq!(DiskIndex::new("r", d).limits(), d);
    }

    #[test]
    fn paths_names_and_stale_ids() {
        let i = sample();
        let kids = i.children(i.root(), 0, 10, 0.0).unwrap();
        let a = NodeId(kids.children[0].id);
        let big = NodeId(i.children(a, 0, 1, 0.0).unwrap().children[0].id);
        assert_eq!(i.path_names(big).unwrap(), ["vol", "a", "big.bin"]);
        assert_eq!(i.node(big).unwrap().name, "big.bin");
        assert_eq!(i.node(NodeId(9999)), Err(IndexError::NoSuchNode));
        assert_eq!(i.path_names(NodeId(9999)), Err(IndexError::NoSuchNode));
        assert_eq!(i.children(NodeId(9999), 0, 1, 0.0), Err(IndexError::NoSuchNode));
        assert_eq!(IndexError::NoSuchNode.to_string(), "that folder is not in this scan");
        assert!(i.children(big, 0, 10, 0.0).unwrap().children.is_empty());
    }

    #[test]
    fn long_names_are_cut_on_a_character_boundary() {
        let mut i = DiskIndex::new("r", limits(10));
        let long = "é".repeat(200);
        let f = i.add_folder(i.root(), &long).unwrap();
        let name = i.node(f).unwrap().name;
        assert!(name.len() <= 255 && name.chars().all(|c| c == 'é'));
        assert_eq!(clamp_name("short"), "short");
    }

    #[test]
    fn the_wire_shape_is_camel_case() {
        let i = sample();
        let v = serde_json::to_value(i.children(i.root(), 0, 2, 0.0).unwrap()).unwrap();
        assert!(v["nextOffset"].is_u64() && v["node"]["newestChange"].is_i64() && v["children"][0]["hasChildren"].is_boolean());
        assert_eq!(v["children"][0]["kind"], "folder");
    }
}
