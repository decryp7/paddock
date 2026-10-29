//! The size index of a disk scan: a compact arena of folders (and the notable files), with sizes that add up from the
//! leaves to the root at every moment, so a half-finished or cancelled scan is still a consistent tree.
//!
//! Memory is bounded: a node is about 80 bytes plus its name, files below [`ScanLimits::notable_bytes`] are folded into
//! their folder, and past [`ScanLimits::max_nodes`] no new node is made (the index says so with [`DiskIndex::truncated`]).
//! Queries return one level at a time, paged, with the small children folded into one "other" bucket.

use super::reason::UnreadableReason;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;

mod rings;
pub use rings::{DiskRings, RingNode, MAX_RINGS, MAX_RING_NODES};

const NONE: u32 = u32::MAX;
/// The most nodes any scan may hold, whatever the Setting says (about 400 MB at the bound).
pub const HARD_MAX_NODES: u32 = 5_000_000;
/// The most children one page returns.
pub const MAX_PAGE: u32 = 256;
/// Names longer than this many bytes are cut (no supported file system allows more: 255 UTF-8 bytes, or 255 UTF-16 units).
const MAX_NAME: usize = 1024;

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

/// The two sizes of a file or a sum of files: what it occupies on the volume and what a program reading it would get.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sizes {
    /// Bytes allocated on the volume (blocks, clusters); this is what the tree is sorted by.
    pub on_disk: u64,
    /// Bytes of content (the length). Smaller than `on_disk` for small files, larger for sparse or compressed ones.
    pub logical: u64,
}

impl Sizes {
    /// A file whose content fills its allocation exactly (what a source that can tell only one size reports).
    pub fn same(bytes: u64) -> Sizes {
        Sizes {
            on_disk: bytes,
            logical: bytes,
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

impl Node {
    /// Whether this node is a refused (or partly refused) folder that Full Disk Access would open.
    fn needs_fda(&self) -> bool {
        self.reason == Some(UnreadableReason::NeedsFullDiskAccess) && (self.kind == NodeKind::Unreadable || self.partial)
    }
}

struct Node {
    parent: u32,
    first_child: u32,
    next_sibling: u32,
    name_off: u32,
    name_len: u16,
    kind: NodeKind,
    /// Why an unreadable folder was refused (kept when it is reopened, so a retry cut short still says why).
    reason: Option<UnreadableReason>,
    complete: bool,
    /// Something directly inside could not be read (but the folder itself could be listed): never complete.
    partial: bool,
    /// A folder a cut-short retry made and did not get to read: incomplete, but not refused by anyone.
    unread: bool,
    /// Folders: the listing itself (1 until finished) plus the child folders not complete yet.
    open: u32,
    child_count: u32,
    bytes: u64,
    logical: u64,
    items: u64,
    loose_bytes: u64,
    loose_logical: u64,
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
    /// Bytes of content, children included (what the files add up to when read).
    pub logical_bytes: u64,
    /// Files and folders inside, recursively.
    pub items: u64,
    /// Newest modification inside.
    pub newest_change: Timestamp,
    /// Whether everything inside has been read.
    pub complete: bool,
    /// Whether there is anything inside to open.
    pub has_children: bool,
    /// Why an unreadable folder could not be read; null for everything else (always present: the binary protocol has no optional fields).
    pub unreadable_reason: Option<UnreadableReason>,
}

/// A sum of folded things.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folded {
    /// How many nodes (or files) are in it.
    pub count: u64,
    /// Their bytes on the volume.
    pub bytes: u64,
    /// Their bytes of content.
    pub logical_bytes: u64,
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
    /// Refused (or partly refused) folders whose reason is Full Disk Access, kept up to date by every change of a node's state.
    needing_fda: u64,
}

/// What a name too long to be real is stored as. No supported file system has such a name, so this is never a path to walk; it
/// is stored whole rather than cut, because a cut could split a character and the bytes are rebuilt into a path unchecked.
const TOO_LONG: &[u8] = b"(name too long)";

fn clamp_name(name: &[u8]) -> &[u8] {
    if name.len() > MAX_NAME {
        TOO_LONG
    } else {
        name
    }
}

impl DiskIndex {
    /// An index with a root folder called `root_name` (its listing is open until [`finish_listing`](Self::finish_listing)).
    pub fn new(root_name: impl AsRef<OsStr>, limits: ScanLimits) -> DiskIndex {
        let mut i = DiskIndex {
            nodes: Vec::new(),
            names: Vec::new(),
            limits: limits.validated(),
            truncated: false,
            skipped_folders: 0,
            needing_fda: 0,
        };
        let name = i.intern(root_name.as_ref());
        i.nodes.push(Node {
            parent: NONE,
            first_child: NONE,
            next_sibling: NONE,
            name_off: name.0,
            name_len: name.1,
            kind: NodeKind::Folder,
            reason: None,
            complete: false,
            partial: false,
            unread: false,
            open: 1,
            child_count: 0,
            bytes: 0,
            logical: 0,
            items: 0,
            loose_bytes: 0,
            loose_logical: 0,
            loose_items: 0,
            newest: 0,
        });
        i
    }

    fn intern(&mut self, name: &OsStr) -> (u32, u16) {
        let name = clamp_name(name.as_encoded_bytes());
        let off = u32::try_from(self.names.len()).unwrap_or(u32::MAX);
        self.names.extend_from_slice(name);
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

    fn bump(&mut self, from: u32, sizes: Sizes, items: u64, mtime: i64) {
        let mut at = from;
        while at != NONE {
            let n = &mut self.nodes[at as usize];
            n.bytes = n.bytes.saturating_add(sizes.on_disk);
            n.logical = n.logical.saturating_add(sizes.logical);
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

    fn blank(&mut self, name: &OsStr, kind: NodeKind) -> Node {
        let (off, len) = self.intern(name);
        Node {
            parent: NONE,
            first_child: NONE,
            next_sibling: NONE,
            name_off: off,
            name_len: len,
            kind,
            reason: None,
            complete: false,
            partial: false,
            unread: false,
            open: 0,
            child_count: 0,
            bytes: 0,
            logical: 0,
            items: 0,
            loose_bytes: 0,
            loose_logical: 0,
            loose_items: 0,
            newest: 0,
        }
    }

    /// Adds a folder under `parent`, to be listed later. `None` when the node limit is reached (the folder is counted as
    /// skipped and must not be walked).
    pub fn add_folder(&mut self, parent: NodeId, name: impl AsRef<OsStr>) -> Option<NodeId> {
        if !self.room() {
            self.skipped_folders += 1;
            return None;
        }
        let mut n = self.blank(name.as_ref(), NodeKind::Folder);
        n.open = 1;
        let id = self.link(parent, n);
        self.nodes[parent.0 as usize].open += 1;
        self.bump(parent.0, Sizes::default(), 1, 0);
        Some(id)
    }

    /// Adds a mount point under `parent`: another volume, shown but not entered, complete with no size.
    pub fn add_mount_point(&mut self, parent: NodeId, name: impl AsRef<OsStr>) -> bool {
        if !self.room() {
            return false;
        }
        let mut n = self.blank(name.as_ref(), NodeKind::MountPoint);
        n.complete = true;
        self.link(parent, n);
        self.bump(parent.0, Sizes::default(), 1, 0);
        true
    }

    /// Marks a folder as unreadable: it stays incomplete for good, and so do its ancestors.
    pub fn mark_unreadable(&mut self, node: NodeId, reason: UnreadableReason) {
        self.change(node, |n| {
            n.kind = NodeKind::Unreadable;
            n.reason = Some(reason);
        });
    }

    /// Applies `f` to a node and keeps the Full Disk Access counter in step with whether the node counts.
    fn change(&mut self, node: NodeId, f: impl FnOnce(&mut Node)) {
        let n = &mut self.nodes[node.0 as usize];
        let was = n.needs_fda();
        f(n);
        match (was, n.needs_fda()) {
            (false, true) => self.needing_fda += 1,
            (true, false) => self.needing_fda = self.needing_fda.saturating_sub(1),
            _ => {}
        }
    }

    /// Marks a reopened folder as unreadable again with the reason it had (a retry was cut short before it was read).
    pub fn restore_unreadable(&mut self, node: NodeId) {
        self.change(node, |n| {
            n.kind = NodeKind::Unreadable;
            n.reason.get_or_insert(UnreadableReason::Other);
        });
    }

    /// Opens every unreadable folder for another try and returns their ids. Each one is a folder being listed again, so
    /// [`finish_listing`](Self::finish_listing) completes it and its ancestors when the second try succeeds, and
    /// [`mark_unreadable`](Self::mark_unreadable) leaves it as it was when it fails again. A folder that has anything in it
    /// (it was not refused at the door) is left alone: listing it again would count its files twice.
    pub fn reopen_unreadable(&mut self) -> Vec<NodeId> {
        let mut out = Vec::new();
        for id in 0..self.nodes.len() {
            let n = &self.nodes[id];
            if n.kind == NodeKind::Unreadable && n.child_count == 0 && n.loose_items == 0 {
                let node = NodeId(u32::try_from(id).unwrap_or(NONE));
                self.change(node, |n| {
                    n.kind = NodeKind::Folder;
                    n.open = 1;
                });
                out.push(node);
            }
        }
        out
    }

    /// Marks a folder that was made but not read yet (a retry was cut short): it stays incomplete, and is not counted as
    /// refused. [`reopen_unread`](Self::reopen_unread) opens it again.
    pub fn mark_unread(&mut self, node: NodeId) {
        self.nodes[node.0 as usize].unread = true;
    }

    /// Opens every folder marked unread for another try and returns their ids.
    pub fn reopen_unread(&mut self) -> Vec<NodeId> {
        let mut out = Vec::new();
        for (id, n) in self.nodes.iter_mut().enumerate() {
            if n.unread && n.child_count == 0 && n.loose_items == 0 {
                n.unread = false;
                n.open = 1;
                out.push(NodeId(u32::try_from(id).unwrap_or(NONE)));
            }
        }
        out
    }

    /// Marks a folder as listed but with something in it that could not be read: its size is a lower bound, it stays incomplete
    /// for good (it cannot be read again without counting its files twice), and so do its ancestors.
    pub fn mark_partial(&mut self, node: NodeId, reason: UnreadableReason) {
        self.change(node, |n| {
            n.partial = true;
            n.reason = Some(reason);
        });
    }

    /// How many folders (refused, or partly refused) the OS would read if Full Disk Access were given.
    pub fn needing_full_disk_access(&self) -> u64 {
        self.needing_fda
    }

    /// Adds a file under `parent`. A notable file gets a node; any other is folded into `parent`'s loose totals.
    pub fn add_file(&mut self, parent: NodeId, name: impl AsRef<OsStr>, sizes: Sizes, mtime: Timestamp) {
        if sizes.on_disk >= self.limits.notable_bytes && self.room() {
            let mut n = self.blank(name.as_ref(), NodeKind::File);
            n.complete = true;
            n.bytes = sizes.on_disk;
            n.logical = sizes.logical;
            n.items = 1;
            n.newest = mtime.0;
            self.link(parent, n);
            self.bump(parent.0, sizes, 1, mtime.0);
        } else {
            self.add_folded_files(parent, sizes, 1, mtime);
        }
    }

    /// Adds files that were already summed by the caller (small ones, folded into `parent`).
    pub fn add_folded_files(&mut self, parent: NodeId, sizes: Sizes, items: u64, newest: Timestamp) {
        let p = &mut self.nodes[parent.0 as usize];
        p.loose_bytes = p.loose_bytes.saturating_add(sizes.on_disk);
        p.loose_logical = p.loose_logical.saturating_add(sizes.logical);
        p.loose_items = p.loose_items.saturating_add(items);
        self.bump(parent.0, sizes, items, newest.0);
    }

    /// Ends the listing of a folder: it is complete once every child folder is too.
    pub fn finish_listing(&mut self, node: NodeId) {
        let mut at = node.0;
        while at != NONE {
            let n = &mut self.nodes[at as usize];
            n.open = n.open.saturating_sub(1);
            if n.open > 0 || n.complete || n.partial || n.unread || n.kind == NodeKind::Unreadable {
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
            logical_bytes: n.logical,
            items: n.items,
            newest_change: Timestamp(n.newest),
            complete: n.complete,
            has_children: n.child_count > 0 || n.loose_items > 0,
            unreadable_reason: if n.kind == NodeKind::Unreadable { n.reason } else { None },
        }
    }

    fn name_bytes(&self, id: u32) -> &[u8] {
        let n = &self.nodes[id as usize];
        let s = n.name_off as usize;
        &self.names[s..s + n.name_len as usize]
    }

    /// The name for display: what cannot be shown as text is replaced, so never use it to rebuild a path.
    fn name(&self, id: u32) -> String {
        String::from_utf8_lossy(self.name_bytes(id)).into_owned()
    }

    /// One node's summary.
    pub fn node(&self, id: NodeId) -> Result<DiskNode, IndexError> {
        if (id.0 as usize) >= self.nodes.len() {
            return Err(IndexError::NoSuchNode);
        }
        Ok(self.info(id.0))
    }

    /// The names from the root down to `id` (the root's own name first) exactly as the file system gave them, as the bytes of
    /// `OsStr::as_encoded_bytes`. Unlike the display names, they rebuild the path of a name that is not valid text.
    pub fn path_components(&self, id: NodeId) -> Result<Vec<Vec<u8>>, IndexError> {
        if (id.0 as usize) >= self.nodes.len() {
            return Err(IndexError::NoSuchNode);
        }
        let mut out = Vec::new();
        let mut at = id.0;
        while at != NONE {
            out.push(self.name_bytes(at).to_vec());
            at = self.nodes[at as usize].parent;
        }
        out.reverse();
        Ok(out)
    }

    /// The ids of the children of `id`, biggest first (equal sizes by name bytes, then by id).
    fn sorted_kids(&self, id: u32) -> Vec<u32> {
        let parent = &self.nodes[id as usize];
        let mut kids: Vec<u32> = Vec::with_capacity(parent.child_count as usize);
        let mut at = parent.first_child;
        while at != NONE {
            kids.push(at);
            at = self.nodes[at as usize].next_sibling;
        }
        kids.sort_unstable_by(|a, b| {
            let (x, y) = (&self.nodes[*a as usize], &self.nodes[*b as usize]);
            y.bytes
                .cmp(&x.bytes)
                .then_with(|| self.name_bytes(*a).cmp(self.name_bytes(*b)))
                .then(a.cmp(b))
        });
        kids
    }

    /// One page of the children of `id`, biggest first. Children below `min_fraction` of the folder's size (0 shows all)
    /// are folded into [`DiskChildren::other`]; `limit` is clamped to 1..=[`MAX_PAGE`].
    pub fn children(&self, id: NodeId, offset: u32, limit: u32, min_fraction: f64) -> Result<DiskChildren, IndexError> {
        if (id.0 as usize) >= self.nodes.len() {
            return Err(IndexError::NoSuchNode);
        }
        let parent = &self.nodes[id.0 as usize];
        let kids = self.sorted_kids(id.0);
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
                logical_bytes: f.logical_bytes.saturating_add(n.logical),
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
                logical_bytes: parent.loose_logical,
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
        i.add_file(a, "big.bin", Sizes::same(5 * MB), Timestamp(50));
        i.add_file(a, "small", Sizes::same(10), Timestamp(60));
        let b = i.add_folder(r, "b").unwrap();
        i.add_file(b, "mid.bin", Sizes::same(3 * MB), Timestamp(70));
        let c = i.add_folder(r, "c").unwrap();
        i.add_file(r, "loose", Sizes::same(7), Timestamp(5));
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
                logical_bytes: 7,
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
            let f = i.add_folder(r, format!("f{n}")).unwrap();
            i.add_file(f, "x", Sizes::same(2 * MB + n), Timestamp(1));
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
        j.mark_unreadable(locked, UnreadableReason::NeedsAdmin);
        j.finish_listing(locked);
        j.finish_listing(r);
        assert!(!j.is_complete(), "an unreadable folder keeps the volume incomplete");
        let kids = j.children(r, 0, 10, 0.0).unwrap();
        assert_eq!(kids.children[0].kind, NodeKind::Unreadable);
        assert!(!kids.children[0].complete);
    }

    #[test]
    fn an_unreadable_folder_carries_its_reason_to_the_ui_and_keeps_it_when_a_retry_is_cut_short() {
        let mut i = DiskIndex::new("v", ScanLimits::default());
        let root = i.root();
        let locked = i.add_folder(root, "Mail").unwrap();
        let fine = i.add_folder(root, "ok").unwrap();
        i.mark_unreadable(locked, UnreadableReason::NeedsFullDiskAccess);
        i.finish_listing(fine);
        let kids = i.children(root, 0, 10, 0.0).unwrap().children;
        let reason = |name: &str| kids.iter().find(|k| k.name == name).unwrap().unreadable_reason;
        assert_eq!((reason("Mail"), reason("ok")), (Some(UnreadableReason::NeedsFullDiskAccess), None));
        assert_eq!(i.reopen_unreadable(), vec![locked]);
        i.restore_unreadable(locked);
        let kids = i.children(root, 0, 10, 0.0).unwrap().children;
        assert_eq!(
            kids.iter().find(|k| k.name == "Mail").unwrap().unreadable_reason,
            Some(UnreadableReason::NeedsFullDiskAccess)
        );
    }

    #[test]
    fn the_full_disk_access_count_follows_every_change_of_state_without_scanning() {
        use UnreadableReason::{NeedsAdmin, NeedsFullDiskAccess};
        let mut i = DiskIndex::new("v", ScanLimits::default());
        let root = i.root();
        let (a, b, c) = (
            i.add_folder(root, "a").unwrap(),
            i.add_folder(root, "b").unwrap(),
            i.add_folder(root, "c").unwrap(),
        );
        assert_eq!(i.needing_full_disk_access(), 0);
        i.mark_unreadable(a, NeedsFullDiskAccess);
        i.mark_unreadable(b, NeedsAdmin);
        assert_eq!(i.needing_full_disk_access(), 1, "only the Full Disk Access reason counts");
        i.mark_partial(c, NeedsFullDiskAccess);
        assert_eq!(i.needing_full_disk_access(), 2, "a partly refused folder counts too");
        i.mark_unreadable(a, NeedsFullDiskAccess);
        assert_eq!(i.needing_full_disk_access(), 2, "marking again does not count twice");
        i.mark_unreadable(a, NeedsAdmin);
        assert_eq!(i.needing_full_disk_access(), 1, "a changed reason leaves the count");
        i.mark_unreadable(a, NeedsFullDiskAccess);
        let reopened = i.reopen_unreadable();
        assert_eq!(reopened.len(), 2, "a and b were refused at the door");
        assert_eq!(
            i.needing_full_disk_access(),
            1,
            "a reopened folder is not refused now; c stays partly refused"
        );
        i.restore_unreadable(a);
        assert_eq!(i.needing_full_disk_access(), 2, "a cut-short retry puts it back with its reason");
    }

    #[test]
    fn unreadable_folders_can_be_opened_again_and_complete_the_tree() {
        let mut i = DiskIndex::new("r", limits(100));
        let r = i.root();
        let locked = i.add_folder(r, "Library").unwrap();
        let fine = i.add_folder(r, "fine").unwrap();
        i.add_file(fine, "x", Sizes::same(9), Timestamp(1));
        i.mark_unreadable(locked, UnreadableReason::NeedsAdmin);
        for n in [locked, fine, r] {
            i.finish_listing(n);
        }
        assert!(!i.is_complete());
        assert_eq!(i.reopen_unreadable(), vec![locked], "only the refused folder is opened again");
        i.add_file(locked, "mail", Sizes::same(40), Timestamp(2));
        i.finish_listing(locked);
        assert!(i.is_complete(), "the second try completes the folder and everything above it");
        assert_eq!(i.totals().0, 49);
        assert!(i.reopen_unreadable().is_empty(), "nothing is left to retry");

        // A second refusal leaves the folder as it was.
        let mut j = DiskIndex::new("r", limits(100));
        let r = j.root();
        let locked = j.add_folder(r, "Library").unwrap();
        j.mark_unreadable(locked, UnreadableReason::NeedsAdmin);
        j.finish_listing(locked);
        j.finish_listing(r);
        assert_eq!(j.reopen_unreadable(), vec![locked]);
        j.mark_unreadable(locked, UnreadableReason::NeedsAdmin);
        j.finish_listing(locked);
        assert!(!j.is_complete());
        assert_eq!(j.reopen_unreadable(), vec![locked], "it can be tried again later");
    }

    #[test]
    fn an_unread_folder_is_incomplete_not_refused_and_opens_again() {
        let mut i = DiskIndex::new("r", limits(100));
        let r = i.root();
        let a = i.add_folder(r, "a").unwrap();
        i.mark_unread(a);
        i.finish_listing(a);
        i.finish_listing(r);
        assert!(!i.is_complete());
        assert_eq!(i.node(a).unwrap().kind, NodeKind::Folder, "not shown as refused");
        assert!(i.reopen_unreadable().is_empty(), "nobody refused it");
        assert_eq!(i.reopen_unread(), vec![a]);
        i.finish_listing(a);
        assert!(i.is_complete(), "read at last");
        assert!(i.reopen_unread().is_empty());
    }

    #[test]
    fn a_partly_unreadable_folder_keeps_the_tree_incomplete() {
        let mut i = DiskIndex::new("r", limits(100));
        let r = i.root();
        let a = i.add_folder(r, "a").unwrap();
        i.add_file(a, "x", Sizes::same(5), Timestamp(1));
        i.mark_partial(a, UnreadableReason::NeedsAdmin);
        i.finish_listing(a);
        i.finish_listing(r);
        assert!(!i.node(a).unwrap().complete && !i.is_complete());
        assert!(
            i.reopen_unreadable().is_empty(),
            "a partial folder is not retried: its files are counted"
        );
        assert_eq!(i.node(a).unwrap().kind, NodeKind::Folder, "it is still a folder you can open");
    }

    #[test]
    fn a_folder_with_contents_is_not_opened_again() {
        let mut i = DiskIndex::new("r", limits(100));
        let r = i.root();
        let a = i.add_folder(r, "a").unwrap();
        i.add_file(a, "tiny", Sizes::same(1), Timestamp(1));
        i.mark_unreadable(a, UnreadableReason::NeedsAdmin);
        assert!(i.reopen_unreadable().is_empty(), "its files are already counted");
    }

    #[test]
    fn both_sizes_are_kept_and_add_up() {
        let mut i = DiskIndex::new("r", limits(100));
        let r = i.root();
        let a = i.add_folder(r, "a").unwrap();
        // A sparse file occupies less than its length; a tiny one occupies a whole block.
        i.add_file(
            a,
            "sparse.img",
            Sizes {
                on_disk: 3 * MB,
                logical: 50 * MB,
            },
            Timestamp(1),
        );
        i.add_file(
            a,
            "tiny",
            Sizes {
                on_disk: 4096,
                logical: 10,
            },
            Timestamp(2),
        );
        i.finish_listing(a);
        i.finish_listing(r);
        let top = i.children(r, 0, 10, 0.0).unwrap();
        assert_eq!((top.node.bytes, top.node.logical_bytes), (3 * MB + 4096, 50 * MB + 10));
        let inner = i.children(NodeId(top.children[0].id), 0, 10, 0.0).unwrap();
        assert_eq!((inner.children[0].bytes, inner.children[0].logical_bytes), (3 * MB, 50 * MB));
        assert_eq!((inner.loose.bytes, inner.loose.logical_bytes), (4096, 10));
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
        i.add_file(a, "big", Sizes::same(9 * MB), Timestamp(1));
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
        let names: Vec<String> = i
            .path_components(big)
            .unwrap()
            .iter()
            .map(|n| String::from_utf8(n.clone()).unwrap())
            .collect();
        assert_eq!(names, ["vol", "a", "big.bin"]);
        assert_eq!(i.node(big).unwrap().name, "big.bin");
        assert_eq!(i.node(NodeId(9999)), Err(IndexError::NoSuchNode));
        assert_eq!(i.path_components(NodeId(9999)), Err(IndexError::NoSuchNode));
        assert_eq!(i.children(NodeId(9999), 0, 1, 0.0), Err(IndexError::NoSuchNode));
        assert_eq!(IndexError::NoSuchNode.to_string(), "that folder is not in this scan");
        assert!(i.children(big, 0, 10, 0.0).unwrap().children.is_empty());
    }

    #[test]
    fn names_are_kept_as_the_file_system_gave_them() {
        let mut i = DiskIndex::new("r", limits(10));
        let odd = unsafe_free_odd_name();
        let f = i.add_folder(i.root(), &odd).unwrap();
        assert_eq!(
            i.path_components(f).unwrap()[1],
            odd.as_encoded_bytes(),
            "exact bytes for rebuilding the path"
        );
        assert!(
            i.node(f).unwrap().name.contains('\u{fffd}') || odd.to_str().is_some(),
            "display text may replace what is not text"
        );
        let long = "é".repeat(500);
        let g = i.add_folder(i.root(), &long).unwrap();
        assert_eq!(
            i.path_components(g).unwrap()[1].len(),
            1000,
            "a long name is kept whole up to 1024 bytes"
        );
        let absurd = "é".repeat(600);
        let h = i.add_folder(i.root(), &absurd).unwrap();
        assert_eq!(i.node(h).unwrap().name, "(name too long)", "never cut in the middle of a character");
    }

    /// A name that is not valid text where the platform allows one (Unix); a plain one elsewhere.
    fn unsafe_free_odd_name() -> std::ffi::OsString {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            std::ffi::OsString::from_vec(vec![b'a', 0xff, b'b'])
        }
        #[cfg(not(unix))]
        {
            std::ffi::OsString::from("ab")
        }
    }

    #[test]
    fn children_of_equal_size_are_ordered_by_name_bytes() {
        let mut i = DiskIndex::new("r", limits(10));
        let r = i.root();
        for n in ["b", "a", "c"] {
            let f = i.add_folder(r, n).unwrap();
            i.finish_listing(f);
        }
        let names: Vec<String> = i.children(r, 0, 10, 0.0).unwrap().children.into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["a", "b", "c"]);
    }

    #[test]
    fn the_wire_shape_is_camel_case() {
        let i = sample();
        let v = serde_json::to_value(i.children(i.root(), 0, 2, 0.0).unwrap()).unwrap();
        assert!(
            v["node"]["logicalBytes"].is_u64()
                && v["nextOffset"].is_u64()
                && v["node"]["newestChange"].is_i64()
                && v["children"][0]["hasChildren"].is_boolean()
        );
        assert_eq!(v["children"][0]["kind"], "folder");
    }
}
