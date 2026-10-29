//! The indexer: snapshot, fingerprint, plan, extract, and one transaction that swaps the new generation in (ADR 0005).

use std::collections::BTreeMap;

use pacenotes_core::{FileId, FileIdAllocator, Fingerprint, Generation, RepoPath};
use paddock_jobs::CancelToken;

use crate::denylist::{Denylist, redact};
use crate::facts::{SourceError, SymbolSource};
use crate::fs::{FsError, WorkspaceFs};
use crate::plan::{Current, plan_changes};
use crate::snapshot::{SnapshotLimits, snapshot};
use crate::store::{Batch, IndexStore, Upsert};

/// Why a run or a read failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IndexError {
    /// Cancelled by the caller; nothing was written.
    #[error("cancelled")]
    Cancelled,
    /// The store failed.
    #[error("index store: {0}")]
    Store(String),
    /// The workspace could not be walked.
    #[error("workspace: {0}")]
    Fs(String),
    /// The path is on the secret denylist; its content is not served.
    #[error("{0} is withheld (secret denylist)")]
    Denied(RepoPath),
}

/// Limits of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexLimits {
    /// Files larger than this are counted in the report, not read, not indexed (default 2 MiB).
    pub max_file_bytes: u64,
    /// Bounds of the walk.
    pub snapshot: SnapshotLimits,
    /// The most extractor failures named in the report (the rest are counted).
    pub max_failures_named: usize,
}

impl Default for IndexLimits {
    fn default() -> Self {
        IndexLimits {
            max_file_bytes: 2 * 1024 * 1024,
            snapshot: SnapshotLimits::default(),
            max_failures_named: 100,
        }
    }
}

/// What a run did, with everything it left out counted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexReport {
    /// The generation written, or `None` when nothing changed.
    pub generation: Option<Generation>,
    /// Files added.
    pub added: usize,
    /// Files whose content changed.
    pub modified: usize,
    /// Files that moved and kept their ids.
    pub renamed: usize,
    /// Files that disappeared.
    pub deleted: usize,
    /// Files that did not change.
    pub unchanged: usize,
    /// Withheld by the secret denylist (shown to the user as "withheld").
    pub denied: u32,
    /// Links skipped.
    pub links: u32,
    /// Special files, bad names, folders too deep or unreadable.
    pub skipped_other: u32,
    /// Files over the size limit.
    pub too_large: usize,
    /// Files that could not be read.
    pub unreadable: usize,
    /// Files the source does not parse: listed, tier 1.
    pub tier1_only: usize,
    /// The first files the extractor failed on, with the reason.
    pub extract_failures: Vec<(RepoPath, String)>,
    /// How many extractor failures there were in all.
    pub extract_failure_count: usize,
    /// The walk stopped at its entry limit.
    pub walk_truncated: bool,
}

/// Runs the pure half of an index over a workspace.
pub struct Indexer<'a> {
    /// The workspace.
    pub fs: &'a dyn WorkspaceFs,
    /// Where facts come from (the parser worker; never in process).
    pub source: &'a dyn SymbolSource,
    /// The secret denylist.
    pub denylist: &'a Denylist,
    /// Limits.
    pub limits: IndexLimits,
}

fn fs_error(e: FsError) -> IndexError {
    IndexError::Fs(e.to_string())
}

impl Indexer<'_> {
    /// Brings the store up to date with the workspace. All changes are one transaction; a cancel or a failure before the commit
    /// leaves the previous generation as it was.
    pub fn run(&self, store: &mut IndexStore, cancel: &CancelToken) -> Result<IndexReport, IndexError> {
        let mut report = IndexReport::default();
        let snap = snapshot(self.fs, self.denylist, &self.limits.snapshot, cancel).map_err(|e| {
            if cancel.is_cancelled() {
                IndexError::Cancelled
            } else {
                fs_error(e)
            }
        })?;
        report.denied = snap.denied;
        report.links = snap.links;
        report.skipped_other = snap.special + snap.bad_names + snap.too_deep + snap.unreadable;
        report.walk_truncated = snap.truncated;

        // pass 1: fingerprint every file (read, hash, drop: memory stays one file)
        let mut current = Vec::with_capacity(snap.files.len());
        let mut sizes: BTreeMap<RepoPath, u64> = BTreeMap::new();
        for (path, size) in &snap.files {
            if cancel.is_cancelled() {
                return Err(IndexError::Cancelled);
            }
            if *size > self.limits.max_file_bytes {
                report.too_large += 1;
                continue;
            }
            match self.fs.read(path, self.limits.max_file_bytes) {
                Ok(bytes) => {
                    sizes.insert(path.clone(), bytes.len() as u64);
                    current.push(Current {
                        path: path.clone(),
                        fingerprint: Fingerprint::of_content(&bytes),
                    });
                }
                Err(FsError::TooLarge { .. }) => report.too_large += 1,
                Err(_) => report.unreadable += 1,
            }
        }

        let previous = store.known_files()?;
        let plan = plan_changes(&previous, &current);
        report.unchanged = plan.unchanged.len();
        report.added = plan.added.len();
        report.modified = plan.modified.len();
        report.renamed = plan.renamed.len();
        report.deleted = plan.deleted.len();
        if report.added + report.modified + report.renamed + report.deleted == 0 {
            return Ok(report);
        }

        let id_of: BTreeMap<&RepoPath, FileId> = previous.iter().map(|k| (&k.path, k.file_id)).collect();
        let fingerprints: BTreeMap<&RepoPath, Fingerprint> = current.iter().map(|c| (&c.path, c.fingerprint)).collect();
        let mut ids = FileIdAllocator::resume(store.next_file_sequence()?);
        let mut batch = Batch::default();
        for path in &plan.deleted {
            batch.deleted.push(id_of[path]);
        }
        for (from, to) in &plan.renamed {
            batch.renamed.push((id_of[from], to.clone()));
        }
        // pass 2: extract the facts of what is new or changed
        let changed = plan
            .added
            .iter()
            .map(|p| (p, None))
            .chain(plan.modified.iter().map(|p| (p, Some(id_of[p]))));
        for (path, existing) in changed {
            if cancel.is_cancelled() {
                return Err(IndexError::Cancelled);
            }
            let file_id = match existing {
                Some(id) => id,
                None => ids.issue().map_err(|e| IndexError::Store(e.to_string()))?,
            };
            let facts = match self.fs.read(path, self.limits.max_file_bytes) {
                Ok(bytes) => match self.source.extract(path, &bytes, cancel) {
                    Ok(facts) => facts,
                    Err(SourceError::Cancelled) => return Err(IndexError::Cancelled),
                    Err(SourceError::Failed(why)) => {
                        report.extract_failure_count += 1;
                        if report.extract_failures.len() < self.limits.max_failures_named {
                            report.extract_failures.push((path.clone(), why));
                        }
                        None
                    }
                },
                Err(_) => {
                    report.unreadable += 1;
                    None
                }
            };
            if facts.is_none() {
                report.tier1_only += 1;
            }
            batch.upserts.push(Upsert {
                file_id,
                path: path.clone(),
                fingerprint: fingerprints[path],
                size: sizes[path],
                facts,
            });
        }
        batch.next_file_sequence = ids.next_sequence();
        if cancel.is_cancelled() {
            return Err(IndexError::Cancelled);
        }
        report.generation = Some(store.commit(&batch)?);
        Ok(report)
    }

    /// The text of the chunks of a file, redacted, at most 32 KiB each. This is the read API a client sees: a denied path is
    /// refused before the file is opened, and what is returned has passed [`redact`].
    pub fn chunk_texts(&self, store: &IndexStore, path: &RepoPath) -> Result<Vec<String>, IndexError> {
        if self.denylist.check(path).is_some() {
            return Err(IndexError::Denied(path.clone()));
        }
        let chunks = store.chunks_of(path)?;
        let bytes = self.fs.read(path, self.limits.max_file_bytes).map_err(fs_error)?;
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().collect();
        let mut out = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            let (first, last) = (chunk.first_line, chunk.last_line);
            let from = (first as usize).saturating_sub(1).min(lines.len());
            let to = (last as usize).min(lines.len()).max(from);
            let mut piece = lines[from..to].join("\n");
            if piece.len() > 32 * 1024 {
                let mut end = 32 * 1024;
                while !piece.is_char_boundary(end) {
                    end -= 1;
                }
                piece.truncate(end);
            }
            out.push(redact(&piece).into_owned());
        }
        Ok(out)
    }
}
