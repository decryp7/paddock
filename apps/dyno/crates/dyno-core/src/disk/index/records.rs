//! The index as bytes: a versioned, packed record stream that a saved disk map is kept in (ADR 0029 section 3).
//!
//! The stream is one header, then one fixed-size little-endian record per node in arena order, then the names. The links
//! (`first_child`, `next_sibling`), the child counts, the name offsets and `open` are rebuilt on reading, not stored.
//! [`RecordWriter`] cuts the stream into chunks without holding it whole; [`RecordDecoder`] is the inverse: it takes the
//! bytes in any pieces, checks every bound before it allocates from a count it read, and returns a typed
//! [`RecordError`] for any input, never a panic. The compression and the 4 MiB chunking belong to the store (the agent).
//!
//! A change to `Node` (a field added, a width changed) must bump [`RECORD_FORMAT`]; a test pins [`RECORD_LEN`].

use super::{DiskIndex, Node, NodeKind, ScanLimits, HARD_MAX_NODES, MAX_NAME, NONE};
use crate::disk::reason::UnreadableReason;

/// The layout number of the stream.
pub const RECORD_FORMAT: u32 = 1;
/// Bytes of the header.
pub const HEADER_LEN: usize = 69;
/// Bytes of one node record: parent 4, name length 2, kind 1, reason 1, flags 1 and seven 8-byte fields.
pub const RECORD_LEN: usize = 65;
const MAGIC: [u8; 4] = *b"DYIX";

/// Why a stream is not an index. Every reading error is one of these; none is a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    /// The stream does not start with the index magic.
    #[error("not a saved disk map")]
    BadMagic,
    /// A layout number this build does not know.
    #[error("saved with a record format this version does not read")]
    UnknownFormat,
    /// The header names no nodes, more than the hard limit or the stored limits, or more names than nodes could carry.
    #[error("the saved map claims more than any scan holds")]
    OverCap,
    /// The header disagrees with the row it was stored in (node count or length).
    #[error("the saved map's header disagrees with its row")]
    HeaderMismatch,
    /// The stream ended before the header said it would.
    #[error("the saved map is cut short")]
    Truncated,
    /// More bytes than the row says were stored.
    #[error("the saved map is longer than stored")]
    TooLong,
    /// A node's parent is not an earlier node, or cannot have children.
    #[error("a saved folder points at a parent that cannot be one")]
    BadParent,
    /// A name length past the allowed name size, or the names do not add up to the name block.
    #[error("a saved name is out of range")]
    BadName,
    /// An unknown kind, reason or flag.
    #[error("a saved node has an unknown kind, reason or flag")]
    BadField,
    /// The sizes stored do not add up from the leaves, or disagree with the header's totals.
    #[error("the saved sizes do not add up")]
    Totals,
}

/// What the store knows about the stream before it reads it: the row's columns. The header is checked against them before
/// anything is allocated from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordBounds {
    /// The row's `record_format`.
    pub format: u32,
    /// The row's `nodes`.
    pub nodes: u64,
    /// The row's `stored_bytes`: the length of the whole uncompressed stream.
    pub stored_bytes: u64,
}

fn reason_code(r: Option<UnreadableReason>) -> u8 {
    match r {
        None => 0,
        Some(UnreadableReason::NeedsFullDiskAccess) => 1,
        Some(UnreadableReason::NeedsAdmin) => 2,
        Some(UnreadableReason::Protected) => 3,
        Some(UnreadableReason::Other) => 4,
    }
}

fn reason_from(code: u8) -> Result<Option<UnreadableReason>, RecordError> {
    Ok(match code {
        0 => None,
        1 => Some(UnreadableReason::NeedsFullDiskAccess),
        2 => Some(UnreadableReason::NeedsAdmin),
        3 => Some(UnreadableReason::Protected),
        4 => Some(UnreadableReason::Other),
        _ => return Err(RecordError::BadField),
    })
}

fn kind_code(k: NodeKind) -> u8 {
    match k {
        NodeKind::Folder => 0,
        NodeKind::File => 1,
        NodeKind::MountPoint => 2,
        NodeKind::Unreadable => 3,
    }
}

fn kind_from(code: u8) -> Result<NodeKind, RecordError> {
    Ok(match code {
        0 => NodeKind::Folder,
        1 => NodeKind::File,
        2 => NodeKind::MountPoint,
        3 => NodeKind::Unreadable,
        _ => return Err(RecordError::BadField),
    })
}

const F_COMPLETE: u8 = 1;
const F_PARTIAL: u8 = 2;
const F_UNREAD: u8 = 4;

impl DiskIndex {
    /// The whole stream in memory (tests and benches; the store uses [`RecordWriter`] to stay bounded).
    pub fn to_records(&self) -> Vec<u8> {
        let mut w = RecordWriter::new(self);
        let mut out = Vec::with_capacity(usize::try_from(w.total_len()).unwrap_or(0));
        while let Some(c) = w.next_chunk(4 * 1024 * 1024) {
            out.extend_from_slice(&c);
        }
        out
    }

    /// The index from a whole stream (the inverse of [`to_records`](Self::to_records)); see [`RecordDecoder`] for the bounds.
    pub fn from_records(bytes: &[u8], bounds: RecordBounds) -> Result<DiskIndex, RecordError> {
        let mut d = RecordDecoder::new(bounds);
        d.push(bytes)?;
        d.finish()
    }
}

/// Cuts an index into the record stream, a chunk at a time, reading the index in place.
pub struct RecordWriter<'a> {
    index: &'a DiskIndex,
    /// Next node to write; `nodes.len()` once the records are done.
    next_node: usize,
    /// Next byte of the name block (the names in arena order are the block itself).
    next_name: usize,
    header_done: bool,
    /// The part of an item that did not fit the last chunk.
    carry: Vec<u8>,
}

impl<'a> RecordWriter<'a> {
    /// A writer over `index`.
    pub fn new(index: &'a DiskIndex) -> RecordWriter<'a> {
        RecordWriter {
            index,
            next_node: 0,
            next_name: 0,
            header_done: false,
            carry: Vec::new(),
        }
    }

    /// The length of the whole stream.
    pub fn total_len(&self) -> u64 {
        HEADER_LEN as u64 + self.index.nodes.len() as u64 * RECORD_LEN as u64 + self.index.names.len() as u64
    }

    /// The next chunk of at most `max` bytes (full except the last), `None` when the stream is done.
    pub fn next_chunk(&mut self, max: usize) -> Option<Vec<u8>> {
        let max = max.max(1);
        let mut out: Vec<u8> = Vec::with_capacity(max.min(1 << 20));
        if !self.carry.is_empty() {
            let n = self.carry.len().min(max);
            out.extend(self.carry.drain(..n));
        }
        if !self.header_done {
            self.header_done = true;
            let h = self.header();
            self.put(&mut out, &h, max);
        }
        while out.len() < max && self.next_node < self.index.nodes.len() {
            let r = self.record(self.next_node);
            self.next_node += 1;
            self.put(&mut out, &r, max);
        }
        if out.len() < max && self.next_node >= self.index.nodes.len() && self.next_name < self.index.names.len() {
            let take = (max - out.len()).min(self.index.names.len() - self.next_name);
            out.extend_from_slice(&self.index.names[self.next_name..self.next_name + take]);
            self.next_name += take;
        }
        (!out.is_empty()).then_some(out)
    }

    /// Appends `item` to `out`, keeping what does not fit for the next chunk.
    fn put(&mut self, out: &mut Vec<u8>, item: &[u8], max: usize) {
        let room = max.saturating_sub(out.len());
        let n = item.len().min(room);
        out.extend_from_slice(&item[..n]);
        self.carry.extend_from_slice(&item[n..]);
    }

    fn header(&self) -> Vec<u8> {
        let i = self.index;
        let mut h = Vec::with_capacity(HEADER_LEN);
        h.extend_from_slice(&MAGIC);
        h.extend_from_slice(&RECORD_FORMAT.to_le_bytes());
        h.extend_from_slice(&(i.nodes.len() as u32).to_le_bytes());
        h.extend_from_slice(&(i.names.len() as u64).to_le_bytes());
        h.extend_from_slice(&i.limits.max_nodes.to_le_bytes());
        h.extend_from_slice(&i.limits.notable_bytes.to_le_bytes());
        h.extend_from_slice(&i.limits.max_hard_links.to_le_bytes());
        h.push(u8::from(i.truncated));
        h.extend_from_slice(&i.skipped_folders.to_le_bytes());
        h.extend_from_slice(&i.needing_fda.to_le_bytes());
        h.extend_from_slice(&i.nodes[0].bytes.to_le_bytes());
        h.extend_from_slice(&i.nodes[0].items.to_le_bytes());
        debug_assert_eq!(h.len(), HEADER_LEN);
        h
    }

    fn record(&self, at: usize) -> [u8; RECORD_LEN] {
        let n = &self.index.nodes[at];
        let mut r = [0u8; RECORD_LEN];
        let mut p = 0;
        let mut put = |b: &[u8]| {
            r[p..p + b.len()].copy_from_slice(b);
            p += b.len();
        };
        put(&n.parent.to_le_bytes());
        put(&n.name_len.to_le_bytes());
        put(&[kind_code(n.kind), reason_code(n.reason)]);
        put(&[(u8::from(n.complete) * F_COMPLETE) | (u8::from(n.partial) * F_PARTIAL) | (u8::from(n.unread) * F_UNREAD)]);
        for v in [n.bytes, n.logical, n.items, n.loose_bytes, n.loose_logical, n.loose_items] {
            put(&v.to_le_bytes());
        }
        put(&n.newest.to_le_bytes());
        debug_assert_eq!(p, RECORD_LEN);
        r
    }
}

/// What the header said, once it passed its checks.
struct Header {
    nodes: u32,
    name_bytes: u64,
    limits: ScanLimits,
    truncated: bool,
    skipped: u64,
    needing_fda: u64,
    root_bytes: u64,
    root_items: u64,
}

#[derive(PartialEq, Eq)]
enum Stage {
    Header,
    Records,
    Names,
    Done,
}

/// Reads the record stream in pieces of any size. Memory grows with the bytes actually read, never from a count in the header:
/// the header is first checked against the [`RecordBounds`] (the row's columns) and the hard limits.
pub struct RecordDecoder {
    bounds: RecordBounds,
    stage: Stage,
    read: u64,
    /// An item cut by the end of a piece.
    pending: Vec<u8>,
    header: Option<Header>,
    nodes: Vec<Node>,
    names: Vec<u8>,
    /// The sum of the name lengths of the records so far.
    named: u64,
}

fn le_u32(b: &[u8]) -> u32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[..4]);
    u32::from_le_bytes(a)
}

fn le_u64(b: &[u8]) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[..8]);
    u64::from_le_bytes(a)
}

impl RecordDecoder {
    /// A decoder for a stream stored with `bounds`.
    pub fn new(bounds: RecordBounds) -> RecordDecoder {
        RecordDecoder {
            bounds,
            stage: Stage::Header,
            read: 0,
            pending: Vec::new(),
            header: None,
            nodes: Vec::new(),
            names: Vec::new(),
            named: 0,
        }
    }

    /// Takes the next piece of the stream.
    pub fn push(&mut self, mut data: &[u8]) -> Result<(), RecordError> {
        self.read = self.read.saturating_add(data.len() as u64);
        if self.read > self.bounds.stored_bytes {
            return Err(RecordError::TooLong);
        }
        while !data.is_empty() {
            match self.stage {
                Stage::Header => {
                    let Some(item) = self.item(&mut data, HEADER_LEN) else { break };
                    self.read_header(&item[..HEADER_LEN])?;
                }
                Stage::Records => {
                    let Some(item) = self.item(&mut data, RECORD_LEN) else { break };
                    self.read_record(&item[..RECORD_LEN])?;
                    if self.header.as_ref().is_some_and(|h| self.nodes.len() as u64 == u64::from(h.nodes)) {
                        self.stage = Stage::Names;
                        if self.header.as_ref().is_some_and(|h| h.name_bytes == 0) {
                            self.stage = Stage::Done;
                        }
                    }
                }
                Stage::Names => {
                    let want = self.header.as_ref().map_or(0, |h| h.name_bytes) - self.names.len() as u64;
                    let take = usize::try_from(want).unwrap_or(usize::MAX).min(data.len());
                    self.names.extend_from_slice(&data[..take]);
                    data = &data[take..];
                    if self.names.len() as u64 == self.header.as_ref().map_or(0, |h| h.name_bytes) {
                        self.stage = Stage::Done;
                    }
                }
                Stage::Done => return Err(RecordError::TooLong),
            }
        }
        Ok(())
    }

    /// The next whole item of `len` bytes, taken from `data` (and the pending part), or `None` when `data` ended inside it.
    fn item(&mut self, data: &mut &[u8], len: usize) -> Option<[u8; HEADER_LEN]> {
        let mut out = [0u8; HEADER_LEN];
        if self.pending.is_empty() && data.len() >= len {
            let (head, rest) = data.split_at(len);
            *data = rest;
            out[..len].copy_from_slice(head);
            return Some(out);
        }
        let need = len - self.pending.len();
        let take = need.min(data.len());
        self.pending.extend_from_slice(&data[..take]);
        *data = &data[take..];
        if self.pending.len() == len {
            out[..len].copy_from_slice(&self.pending);
            self.pending.clear();
            return Some(out);
        }
        None
    }

    fn read_header(&mut self, b: &[u8]) -> Result<(), RecordError> {
        if b[..4] != MAGIC {
            return Err(RecordError::BadMagic);
        }
        let format = le_u32(&b[4..]);
        if format != RECORD_FORMAT || self.bounds.format != RECORD_FORMAT {
            return Err(RecordError::UnknownFormat);
        }
        let nodes = le_u32(&b[8..]);
        let name_bytes = le_u64(&b[12..]);
        let limits = ScanLimits {
            max_nodes: le_u32(&b[20..]),
            notable_bytes: le_u64(&b[24..]),
            max_hard_links: le_u32(&b[32..]),
        };
        if nodes == 0 || nodes > HARD_MAX_NODES || limits.max_nodes == 0 || limits.max_nodes > HARD_MAX_NODES || nodes > limits.max_nodes {
            return Err(RecordError::OverCap);
        }
        if name_bytes > u64::from(nodes) * MAX_NAME as u64 {
            return Err(RecordError::OverCap);
        }
        // Checked against the row before anything is read into memory from these counts.
        if u64::from(nodes) != self.bounds.nodes {
            return Err(RecordError::HeaderMismatch);
        }
        let total = HEADER_LEN as u64 + u64::from(nodes) * RECORD_LEN as u64 + name_bytes;
        if total != self.bounds.stored_bytes {
            return Err(RecordError::HeaderMismatch);
        }
        let flag = b[36];
        if flag > 1 {
            return Err(RecordError::BadField);
        }
        self.header = Some(Header {
            nodes,
            name_bytes,
            limits,
            truncated: flag == 1,
            skipped: le_u64(&b[37..]),
            needing_fda: le_u64(&b[45..]),
            root_bytes: le_u64(&b[53..]),
            root_items: le_u64(&b[61..]),
        });
        self.stage = Stage::Records;
        Ok(())
    }

    fn read_record(&mut self, b: &[u8]) -> Result<(), RecordError> {
        let at = self.nodes.len();
        let parent = le_u32(b);
        let name_len = u16::from_le_bytes([b[4], b[5]]);
        let kind = kind_from(b[6])?;
        let reason = reason_from(b[7])?;
        let flags = b[8];
        if flags & !(F_COMPLETE | F_PARTIAL | F_UNREAD) != 0 {
            return Err(RecordError::BadField);
        }
        if usize::from(name_len) > MAX_NAME {
            return Err(RecordError::BadName);
        }
        if at == 0 {
            if parent != NONE || !matches!(kind, NodeKind::Folder | NodeKind::Unreadable) {
                return Err(RecordError::BadParent);
            }
        } else {
            let p = self.nodes.get(parent as usize).ok_or(RecordError::BadParent)?;
            if !matches!(p.kind, NodeKind::Folder | NodeKind::Unreadable) {
                return Err(RecordError::BadParent);
            }
        }
        let name_off = self.named;
        self.named += u64::from(name_len);
        let name_block = self.header.as_ref().map_or(0, |h| h.name_bytes);
        if self.named > name_block {
            return Err(RecordError::BadName);
        }
        self.nodes.push(Node {
            parent,
            first_child: NONE,
            next_sibling: NONE,
            name_off: u32::try_from(name_off).map_err(|_| RecordError::BadName)?,
            name_len,
            kind,
            reason,
            complete: flags & F_COMPLETE != 0,
            partial: flags & F_PARTIAL != 0,
            unread: flags & F_UNREAD != 0,
            open: 0,
            child_count: 0,
            bytes: le_u64(&b[9..]),
            logical: le_u64(&b[17..]),
            items: le_u64(&b[25..]),
            loose_bytes: le_u64(&b[33..]),
            loose_logical: le_u64(&b[41..]),
            loose_items: le_u64(&b[49..]),
            newest: i64::from_le_bytes([b[57], b[58], b[59], b[60], b[61], b[62], b[63], b[64]]),
        });
        Ok(())
    }

    /// The index, once the whole stream has been pushed: rebuilds the links and checks that the sizes add up.
    pub fn finish(mut self) -> Result<DiskIndex, RecordError> {
        if self.stage != Stage::Done {
            return Err(RecordError::Truncated);
        }
        let Some(h) = self.header.take() else {
            return Err(RecordError::Truncated);
        };
        if self.named != h.name_bytes {
            return Err(RecordError::BadName);
        }
        // The links, as `link` made them: each child in arena order becomes its parent's first child.
        for id in 1..self.nodes.len() {
            let parent = self.nodes[id].parent as usize;
            let first = self.nodes[parent].first_child;
            self.nodes[id].next_sibling = first;
            self.nodes[parent].first_child = id as u32;
            self.nodes[parent].child_count += 1;
        }
        for id in 0..self.nodes.len() {
            let n = &self.nodes[id];
            if n.kind == NodeKind::File {
                continue;
            }
            let (mut bytes, mut logical, mut items, mut newest) = (n.loose_bytes, n.loose_logical, n.loose_items, i64::MIN);
            let mut at = n.first_child;
            while at != NONE {
                let c = &self.nodes[at as usize];
                bytes = bytes.saturating_add(c.bytes);
                logical = logical.saturating_add(c.logical);
                items = items.saturating_add(c.items).saturating_add(u64::from(c.kind != NodeKind::File));
                newest = newest.max(c.newest);
                at = c.next_sibling;
            }
            if n.bytes != bytes || n.logical != logical || n.items != items || n.newest < newest {
                return Err(RecordError::Totals);
            }
        }
        let needing = self
            .nodes
            .iter()
            .filter(|n| n.reason == Some(UnreadableReason::NeedsFullDiskAccess) && (n.kind == NodeKind::Unreadable || n.partial))
            .count() as u64;
        let root = &self.nodes[0];
        if root.bytes != h.root_bytes || root.items != h.root_items || needing != h.needing_fda {
            return Err(RecordError::Totals);
        }
        Ok(DiskIndex {
            nodes: self.nodes,
            names: self.names,
            limits: h.limits,
            truncated: h.truncated,
            skipped_folders: h.skipped,
            needing_fda: needing,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::index::Sizes;
    use crate::time::Timestamp;
    use std::ffi::OsString;

    const MB: u64 = 1024 * 1024;

    fn limits() -> ScanLimits {
        ScanLimits {
            max_nodes: 1000,
            notable_bytes: MB,
            max_hard_links: 10,
        }
    }

    /// A tree with every kind of node and flag: folders, big and loose files, a mount point, a refused folder, a partly
    /// refused one, a name that is not text.
    fn rich() -> DiskIndex {
        let mut i = DiskIndex::new("vol", limits());
        let r = i.root();
        let a = i.add_folder(r, "a").unwrap();
        i.add_file(
            a,
            "big.bin",
            Sizes {
                on_disk: 5 * MB,
                logical: 4 * MB,
            },
            Timestamp(50),
        );
        i.add_file(a, "small", Sizes::same(10), Timestamp(60));
        let b = i.add_folder(a, "b").unwrap();
        i.add_file(b, "mid.bin", Sizes::same(3 * MB), Timestamp(70));
        i.finish_listing(b);
        i.finish_listing(a);
        let locked = i.add_folder(r, "Library").unwrap();
        i.mark_unreadable(locked, UnreadableReason::NeedsFullDiskAccess);
        i.finish_listing(locked);
        let part = i.add_folder(r, "part").unwrap();
        i.mark_partial(part, UnreadableReason::NeedsFullDiskAccess);
        i.add_file(part, "x", Sizes::same(2 * MB), Timestamp(9));
        i.finish_listing(part);
        let un = i.add_folder(r, "unread").unwrap();
        i.mark_unread(un);
        i.add_mount_point(r, "mnt");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let odd = OsString::from_vec(vec![b'o', 0xff, b'd']);
            i.add_file(r, odd, Sizes::same(2 * MB), Timestamp(3));
        }
        i.add_file(r, "loose", Sizes::same(7), Timestamp(5));
        i.finish_listing(r);
        i
    }

    fn bounds_of(bytes: &[u8], i: &DiskIndex) -> RecordBounds {
        RecordBounds {
            format: RECORD_FORMAT,
            nodes: i.len() as u64,
            stored_bytes: bytes.len() as u64,
        }
    }

    /// Every answer the engine gives over the two indexes agrees.
    fn same_answers(a: &DiskIndex, b: &DiskIndex) {
        assert_eq!(a.len(), b.len());
        assert_eq!(a.truncated(), b.truncated());
        assert_eq!(a.skipped_folders(), b.skipped_folders());
        assert_eq!(a.limits(), b.limits());
        assert_eq!(a.totals(), b.totals());
        assert_eq!(a.is_complete(), b.is_complete());
        assert_eq!(a.needing_full_disk_access(), b.needing_full_disk_access());
        for id in 0..a.len() as u32 {
            let id = super::super::NodeId(id);
            assert_eq!(a.children(id, 0, 256, 0.0), b.children(id, 0, 256, 0.0));
            assert_eq!(a.path_components(id), b.path_components(id));
            assert_eq!(a.rings(id, 3, 0.0, 2000), b.rings(id, 3, 0.0, 2000));
        }
    }

    #[test]
    fn a_round_trip_gives_the_same_answers_and_pins_the_layout() {
        let i = rich();
        let bytes = i.to_records();
        let names = i.names.len();
        assert_eq!(bytes.len(), HEADER_LEN + i.len() * RECORD_LEN + names);
        assert_eq!(RECORD_LEN, 65, "a change to Node's fields must bump RECORD_FORMAT and this size");
        assert_eq!(HEADER_LEN, 69);
        let back = DiskIndex::from_records(&bytes, bounds_of(&bytes, &i)).unwrap();
        same_answers(&i, &back);
        assert_eq!(back.to_records(), bytes, "writing it again gives the same bytes");
        assert!(!back.is_complete(), "the unreadable folders keep it incomplete");
    }

    #[test]
    fn chunks_of_any_size_and_pieces_of_any_size_read_the_same() {
        let i = rich();
        let whole = i.to_records();
        for max in [1usize, 7, 64, 65, 66, 1000, 4096] {
            let mut w = RecordWriter::new(&i);
            assert_eq!(w.total_len(), whole.len() as u64);
            let mut d = RecordDecoder::new(bounds_of(&whole, &i));
            let mut joined = Vec::new();
            while let Some(c) = w.next_chunk(max) {
                assert!(c.len() <= max);
                joined.extend_from_slice(&c);
                d.push(&c).unwrap();
            }
            assert_eq!(joined, whole, "chunk size {max}");
            same_answers(&i, &d.finish().unwrap());
        }
    }

    #[test]
    fn a_tree_of_millions_of_identical_empty_files_is_legitimate() {
        // Highly compressible, and small here, but the shape is the point: many equal records read fine (no ratio limit).
        let mut i = DiskIndex::new(
            "r",
            ScanLimits {
                max_nodes: 50_000,
                notable_bytes: 0,
                max_hard_links: 1,
            },
        );
        let r = i.root();
        for _ in 0..20_000 {
            i.add_file(r, "same", Sizes::same(0), Timestamp(1));
        }
        i.finish_listing(r);
        let bytes = i.to_records();
        let back = DiskIndex::from_records(&bytes, bounds_of(&bytes, &i)).unwrap();
        same_answers_light(&i, &back);
    }

    fn same_answers_light(a: &DiskIndex, b: &DiskIndex) {
        assert_eq!(a.len(), b.len());
        assert_eq!(a.totals(), b.totals());
        assert_eq!(a.children(a.root(), 0, 256, 0.0), b.children(b.root(), 0, 256, 0.0));
    }

    fn good() -> (Vec<u8>, RecordBounds) {
        let i = rich();
        let b = i.to_records();
        let bounds = bounds_of(&b, &i);
        (b, bounds)
    }

    fn set_u64(b: &mut [u8], at: usize, v: u64) {
        b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }

    fn rec(n: usize) -> usize {
        HEADER_LEN + n * RECORD_LEN
    }

    #[test]
    fn broken_streams_give_typed_errors() {
        let (good, bounds) = good();
        let read = |b: &[u8], bounds: RecordBounds| DiskIndex::from_records(b, bounds).err();
        let mut cases: Vec<(&str, Vec<u8>, RecordBounds, RecordError)> = Vec::new();
        let mut add = |name: &'static str, b: Vec<u8>, bounds: RecordBounds, e: RecordError| cases.push((name, b, bounds, e));

        add("empty", vec![], bounds, RecordError::Truncated);
        add("cut in the header", good[..30].to_vec(), bounds, RecordError::Truncated);
        add("cut in the records", good[..rec(2) + 10].to_vec(), bounds, RecordError::Truncated);
        add("cut in the names", good[..good.len() - 1].to_vec(), bounds, RecordError::Truncated);
        let mut longer = good.clone();
        longer.push(0);
        add("a trailing byte", longer, bounds, RecordError::TooLong);

        let mut b = good.clone();
        b[0] = b'X';
        add("bad magic", b, bounds, RecordError::BadMagic);
        let mut b = good.clone();
        b[4] = 9;
        add("unknown format", b, bounds, RecordError::UnknownFormat);
        add(
            "row format unknown",
            good.clone(),
            RecordBounds { format: 7, ..bounds },
            RecordError::UnknownFormat,
        );

        let mut b = good.clone();
        b[8..12].copy_from_slice(&(HARD_MAX_NODES + 1).to_le_bytes());
        add("count over the hard cap", b, bounds, RecordError::OverCap);
        let mut b = good.clone();
        b[8..12].copy_from_slice(&5000u32.to_le_bytes());
        add("count over the stored limit", b, bounds, RecordError::OverCap);
        let mut b = good.clone();
        b[8..12].copy_from_slice(&0u32.to_le_bytes());
        add("zero nodes", b, bounds, RecordError::OverCap);
        let mut b = good.clone();
        b[8..12].copy_from_slice(&(bounds.nodes as u32 + 1).to_le_bytes());
        add("a header count larger than the row's nodes", b, bounds, RecordError::HeaderMismatch);
        let mut b = good.clone();
        set_u64(&mut b, 12, u64::MAX);
        add("names past any possible size", b, bounds, RecordError::OverCap);
        let mut b = good.clone();
        let names = le_u64(&b[12..]);
        set_u64(&mut b, 12, names + 1);
        add("name bytes that disagree with the row", b, bounds, RecordError::HeaderMismatch);
        add(
            "a row that disagrees on the length",
            good.clone(),
            RecordBounds {
                stored_bytes: bounds.stored_bytes + 1,
                ..bounds
            },
            RecordError::HeaderMismatch,
        );

        // Records: 1 is "a" (a child of the root).
        let mut b = good.clone();
        b[rec(1)..rec(1) + 4].copy_from_slice(&5u32.to_le_bytes());
        add("a parent after its child", b, bounds, RecordError::BadParent);
        let mut b = good.clone();
        b[rec(1)..rec(1) + 4].copy_from_slice(&1u32.to_le_bytes());
        add("a node its own parent", b, bounds, RecordError::BadParent);
        let mut b = good.clone();
        b[rec(0)..rec(0) + 4].copy_from_slice(&0u32.to_le_bytes());
        add("a root with a parent", b, bounds, RecordError::BadParent);
        let mut b = good.clone();
        b[rec(1) + 6] = 1;
        add("a file with children", b, bounds, RecordError::BadParent);
        let mut b = good.clone();
        b[rec(1) + 4..rec(1) + 6].copy_from_slice(&((MAX_NAME + 1) as u16).to_le_bytes());
        add("a name past the cap", b, bounds, RecordError::BadName);
        let mut b = good.clone();
        b[rec(1) + 4..rec(1) + 6].copy_from_slice(&200u16.to_le_bytes());
        add("names that overrun the block", b, bounds, RecordError::BadName);
        let mut b = good.clone();
        b[rec(1) + 6] = 9;
        add("an unknown kind", b, bounds, RecordError::BadField);
        let mut b = good.clone();
        b[rec(1) + 7] = 9;
        add("an unknown reason", b, bounds, RecordError::BadField);
        let mut b = good.clone();
        b[rec(1) + 8] = 0x80;
        add("an unknown flag", b, bounds, RecordError::BadField);
        let mut b = good.clone();
        b[36] = 2;
        add("an unknown truncated flag", b, bounds, RecordError::BadField);
        let mut b = good.clone();
        let v = le_u64(&b[rec(1) + 9..]);
        set_u64(&mut b, rec(1) + 9, v + 1);
        add("a node's bytes that do not add up", b, bounds, RecordError::Totals);
        let mut b = good.clone();
        set_u64(&mut b, 53, 1);
        add("a root total that disagrees with the header", b, bounds, RecordError::Totals);
        let mut b = good.clone();
        set_u64(&mut b, 45, 99);
        add("a Full Disk Access count that disagrees", b, bounds, RecordError::Totals);

        for (name, b, bounds, want) in cases {
            assert_eq!(read(&b, bounds), Some(want), "{name}");
        }
    }

    #[test]
    fn a_stream_longer_than_the_row_says_is_refused_while_it_is_read() {
        let (good, bounds) = good();
        let mut d = RecordDecoder::new(RecordBounds {
            stored_bytes: 100,
            ..bounds
        });
        assert_eq!(d.push(&good), Err(RecordError::TooLong), "refused before the records are parsed");
    }

    #[test]
    fn no_allocation_follows_a_header_count() {
        // A header that claims the most nodes, with a row that agrees, but only a header's worth of bytes: the decoder holds
        // the header and nothing sized from it.
        let i = rich();
        let mut h = i.to_records();
        h.truncate(HEADER_LEN);
        h[8..12].copy_from_slice(&HARD_MAX_NODES.to_le_bytes());
        h[20..24].copy_from_slice(&HARD_MAX_NODES.to_le_bytes());
        set_u64(&mut h, 12, 0);
        let total = HEADER_LEN as u64 + u64::from(HARD_MAX_NODES) * RECORD_LEN as u64;
        let mut d = RecordDecoder::new(RecordBounds {
            format: RECORD_FORMAT,
            nodes: u64::from(HARD_MAX_NODES),
            stored_bytes: total,
        });
        d.push(&h).unwrap();
        assert_eq!(d.nodes.capacity(), 0);
        assert_eq!(d.names.capacity(), 0);
        assert!(matches!(d.finish(), Err(RecordError::Truncated)));
    }

    #[test]
    fn garbage_and_flipped_bytes_never_panic() {
        // A fixed generator, so a failure repeats: random bytes, and the good stream with each byte flipped.
        let (good, bounds) = good();
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..200 {
            let len = (next() % 600) as usize;
            let junk: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let _ = DiskIndex::from_records(
                &junk,
                RecordBounds {
                    stored_bytes: len as u64,
                    ..bounds
                },
            );
        }
        for at in 0..good.len() {
            let mut b = good.clone();
            b[at] ^= 1 << (next() % 8);
            let _ = DiskIndex::from_records(&b, bounds);
        }
    }

    mod prop {
        use super::*;
        use proptest::prelude::*;

        /// A random tree: each step adds a folder or a file under an earlier folder, some unreadable, some partial.
        fn build(steps: &[(u8, u8, u32)]) -> DiskIndex {
            let mut i = DiskIndex::new(
                "root",
                ScanLimits {
                    max_nodes: 60,
                    notable_bytes: 100,
                    max_hard_links: 5,
                },
            );
            let mut folders = vec![i.root()];
            for (n, (op, pick, size)) in steps.iter().enumerate() {
                let parent = folders[*pick as usize % folders.len()];
                match op % 6 {
                    0 | 1 => {
                        if let Some(f) = i.add_folder(parent, format!("f{n}")) {
                            folders.push(f);
                        }
                    }
                    2 | 3 => i.add_file(
                        parent,
                        format!("file{n}"),
                        Sizes::same(u64::from(*size)),
                        Timestamp(i64::from(*size)),
                    ),
                    4 => i.mark_partial(parent, UnreadableReason::Protected),
                    _ => {
                        i.add_mount_point(parent, format!("m{n}"));
                    }
                }
            }
            for f in folders.iter().rev() {
                i.finish_listing(*f);
            }
            i
        }

        proptest! {
            #[test]
            fn any_generated_tree_round_trips(steps in proptest::collection::vec((any::<u8>(), any::<u8>(), 0u32..5000), 0..120)) {
                let i = build(&steps);
                let bytes = i.to_records();
                let back = DiskIndex::from_records(&bytes, bounds_of(&bytes, &i)).unwrap();
                same_answers(&i, &back);
            }

            #[test]
            fn any_bytes_are_an_error_not_a_panic(junk in proptest::collection::vec(any::<u8>(), 0..400)) {
                let b = RecordBounds { format: RECORD_FORMAT, nodes: 1, stored_bytes: junk.len() as u64 };
                let _ = DiskIndex::from_records(&junk, b);
            }
        }
    }
}
