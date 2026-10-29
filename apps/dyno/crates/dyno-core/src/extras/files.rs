//! The large and duplicate file finder: the pure part. The agent walks the chosen folder (bounded), hashes the files that
//! could be duplicates and hands the results here. Finding deletes nothing, and neither does anything else in Dyno.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Smallest size filter the user may ask for (below this, duplicates are noise: empty files, icons).
pub const MIN_BYTES_FLOOR: u64 = 1024 * 1024;
/// Most files listed in each result list.
pub const MAX_LISTED: usize = 200;
/// Most duplicate groups listed.
pub const MAX_GROUPS: usize = 100;
/// Most files one scan looks at before it stops and says so.
pub const MAX_SCANNED: usize = 500_000;
/// Deepest folder level the scan enters (the chosen folder is level 0).
pub const MAX_DEPTH: u32 = 48;
/// Most bytes the scan reads while comparing the content of files that could be duplicates.
pub const MAX_HASH_BYTES: u64 = 64 * 1024 * 1024 * 1024;

/// Which limit stopped a scan early. The scan still answers with what it found so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FinderLimit {
    /// [`MAX_SCANNED`] files and folders were looked at.
    Files,
    /// Folders deeper than [`MAX_DEPTH`] levels were left out.
    Depth,
    /// [`MAX_HASH_BYTES`] were read comparing contents; some duplicates may be missing.
    HashBytes,
}

/// The limit that stopped a scan, with the sentence the person reads (the engine words it; the UI only shows it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitHit {
    /// Which limit.
    pub limit: FinderLimit,
    /// What happened and what to do about it.
    pub message: String,
}

impl From<FinderLimit> for LimitHit {
    fn from(limit: FinderLimit) -> LimitHit {
        LimitHit {
            message: limit.message(),
            limit,
        }
    }
}

impl FinderLimit {
    /// What to tell the person, ending with what to do.
    pub fn message(self) -> String {
        match self {
            FinderLimit::Files => format!(
                "Stopped after {MAX_SCANNED} files and folders. Choose a smaller folder to see everything in it."
            ),
            FinderLimit::Depth => format!("Folders more than {MAX_DEPTH} levels down were left out. Choose a folder nearer to them."),
            FinderLimit::HashBytes => format!(
                "Stopped comparing contents after {} GiB. Some duplicates may be missing; choose a smaller folder or a larger minimum size.",
                MAX_HASH_BYTES / (1024 * 1024 * 1024)
            ),
        }
    }
}

/// What to look for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FinderMode {
    /// The biggest files.
    Large,
    /// Files with identical content.
    Duplicates,
    /// Both.
    #[default]
    Both,
}

/// The request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinderQuery {
    /// The folder the user picked (absolute).
    pub folder: String,
    /// What to look for.
    #[serde(default)]
    pub mode: FinderMode,
    /// Ignore files smaller than this many bytes (at least [`MIN_BYTES_FLOOR`]).
    pub min_bytes: u64,
}

impl FinderQuery {
    /// The query with the size floor applied, or why it is refused.
    pub fn validated(mut self) -> Result<FinderQuery, String> {
        if self.folder.trim().is_empty() || self.folder.len() > 4096 || self.folder.contains('\0') {
            return Err("Choose a folder to look in.".into());
        }
        self.min_bytes = self.min_bytes.max(MIN_BYTES_FLOOR);
        Ok(self)
    }
}

/// One file in a result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundFile {
    /// Id within the result (what the user's selection refers to).
    pub id: String,
    /// Full path.
    pub path: String,
    /// Size.
    pub bytes: u64,
    /// Last modified.
    pub modified: Timestamp,
}

/// Files with the same content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    /// Size of each copy.
    pub bytes_each: u64,
    /// What removing all but one copy would free.
    pub wasted_bytes: u64,
    /// The copies, oldest first (the oldest is the one to keep by default).
    pub files: Vec<FoundFile>,
}

/// The finder's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FinderResult {
    /// Id of this result; a removal refers to it and it expires after an hour.
    pub scan_id: String,
    /// The folder as resolved.
    pub folder: String,
    /// The biggest files, biggest first.
    pub large: Vec<FoundFile>,
    /// Duplicate groups, most wasted first.
    pub duplicates: Vec<DuplicateGroup>,
    /// Files looked at.
    pub scanned: u32,
    /// The limit that stopped the scan early, when one did; more exists.
    pub limit: Option<LimitHit>,
    /// Total wasted by the listed duplicate groups.
    pub wasted_bytes: u64,
}

/// A file the walk found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walked {
    /// Full path.
    pub path: String,
    /// Size.
    pub bytes: u64,
    /// Last modified.
    pub modified: Timestamp,
}

/// The biggest files at or above `min_bytes`.
pub fn largest(files: &[Walked], min_bytes: u64, id_prefix: &str) -> Vec<FoundFile> {
    let mut v: Vec<&Walked> = files.iter().filter(|f| f.bytes >= min_bytes).collect();
    v.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    v.truncate(MAX_LISTED);
    v.into_iter()
        .enumerate()
        .map(|(i, f)| found(f, format!("{id_prefix}-l{i}")))
        .collect()
}

fn found(f: &Walked, id: String) -> FoundFile {
    FoundFile {
        id,
        path: f.path.clone(),
        bytes: f.bytes,
        modified: f.modified,
    }
}

/// Indexes of files that share a size with another file (the only ones worth hashing), grouped by size, biggest first.
pub fn same_size_groups(files: &[Walked], min_bytes: u64) -> Vec<Vec<usize>> {
    let mut by_size: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (i, f) in files.iter().enumerate().filter(|(_, f)| f.bytes >= min_bytes) {
        by_size.entry(f.bytes).or_default().push(i);
    }
    by_size.into_iter().rev().map(|(_, g)| g).filter(|g| g.len() > 1).collect()
}

/// Duplicate groups from files that were hashed: `hashed` holds `(file index, content digest)`. Files that share a
/// digest *and* a size are duplicates. Groups are ordered by wasted bytes, copies by age.
pub fn duplicate_groups(files: &[Walked], hashed: &[(usize, [u8; 32])], id_prefix: &str) -> Vec<DuplicateGroup> {
    let mut by_digest: BTreeMap<(u64, [u8; 32]), Vec<usize>> = BTreeMap::new();
    for (i, d) in hashed {
        if let Some(f) = files.get(*i) {
            by_digest.entry((f.bytes, *d)).or_default().push(*i);
        }
    }
    let mut groups: Vec<DuplicateGroup> = by_digest
        .into_iter()
        .filter(|(_, g)| g.len() > 1)
        .map(|((bytes, _), mut g)| {
            g.sort_by(|a, b| {
                files[*a]
                    .modified
                    .cmp(&files[*b].modified)
                    .then_with(|| files[*a].path.cmp(&files[*b].path))
            });
            DuplicateGroup {
                bytes_each: bytes,
                wasted_bytes: bytes.saturating_mul(g.len() as u64 - 1),
                files: g.into_iter().map(|i| found(&files[i], String::new())).collect(),
            }
        })
        .collect();
    groups.sort_by(|a, b| {
        b.wasted_bytes
            .cmp(&a.wasted_bytes)
            .then_with(|| a.files[0].path.cmp(&b.files[0].path))
    });
    groups.truncate(MAX_GROUPS);
    for (gi, g) in groups.iter_mut().enumerate() {
        for (fi, f) in g.files.iter_mut().enumerate() {
            f.id = format!("{id_prefix}-d{gi}-{fi}");
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(path: &str, bytes: u64, modified: i64) -> Walked {
        Walked {
            path: path.into(),
            bytes,
            modified: Timestamp(modified),
        }
    }

    const MB: u64 = 1024 * 1024;

    #[test]
    fn the_query_has_a_size_floor_and_a_folder() {
        let q = FinderQuery {
            folder: "/a".into(),
            mode: FinderMode::Both,
            min_bytes: 5,
        };
        assert_eq!(q.clone().validated().unwrap().min_bytes, MIN_BYTES_FLOOR);
        assert!(FinderQuery {
            folder: " ".into(),
            ..q.clone()
        }
        .validated()
        .is_err());
        assert!(FinderQuery {
            folder: "a\0b".into(),
            ..q
        }
        .validated()
        .is_err());
    }

    #[test]
    fn large_files_are_biggest_first_with_ties_by_path() {
        let files = vec![w("/b", 5 * MB, 1), w("/a", 5 * MB, 1), w("/c", 9 * MB, 1), w("/small", 10, 1)];
        let l = largest(&files, MB, "s1");
        assert_eq!(l.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["/c", "/a", "/b"]);
        assert_eq!(l[0].id, "s1-l0");
    }

    #[test]
    fn only_files_sharing_a_size_are_candidates() {
        let files = vec![
            w("/a", 2 * MB, 1),
            w("/b", 2 * MB, 2),
            w("/c", 3 * MB, 3),
            w("/d", 4 * MB, 4),
            w("/e", 4 * MB, 5),
            w("/f", 4 * MB, 6),
        ];
        let g = same_size_groups(&files, MB);
        assert_eq!(g, vec![vec![3, 4, 5], vec![0, 1]]);
        assert!(same_size_groups(&files, 10 * MB).is_empty());
    }

    #[test]
    fn duplicates_need_equal_digests_and_keep_the_oldest_first() {
        let files = vec![
            w("/a", 2 * MB, 30),
            w("/b", 2 * MB, 10),
            w("/c", 2 * MB, 20),
            w("/d", 4 * MB, 1),
            w("/e", 4 * MB, 2),
        ];
        let same = [1u8; 32];
        let other = [2u8; 32];
        let hashed = vec![(0, same), (1, same), (2, other), (3, same), (4, same)];
        let g = duplicate_groups(&files, &hashed, "s");
        assert_eq!(g.len(), 2);
        // The 4 MB pair wastes more than the 2 MB pair.
        assert_eq!(g[0].wasted_bytes, 4 * MB);
        assert_eq!(g[1].files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["/b", "/a"]);
        assert_eq!(g[1].files[0].id, "s-d1-0");
        // Same digest but different size is never a duplicate.
        let g = duplicate_groups(&files, &[(0, same), (3, same)], "s");
        assert!(g.is_empty());
        // An index outside the list is ignored.
        assert!(duplicate_groups(&files, &[(99, same), (98, same)], "s").is_empty());
    }
}
