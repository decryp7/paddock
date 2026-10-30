//! The indexer: snapshot, fingerprint, plan, extract, and one transaction that swaps the new generation in (ADR 0005).

use std::collections::BTreeMap;

use crate::facts::FileFacts;
use pacenotes_core::{FileId, FileIdAllocator, Fingerprint, Generation, RepoPath};
use paddock_jobs::{CancelReason, CancelToken, Counter, JobError, Workers, par_map_ordered};

use crate::denylist::{Denylist, redact};
use crate::facts::{SourceError, SymbolSource};
use crate::fs::{FsError, WorkspaceFs};
use crate::ignore::{Ignore, NoIgnore};
use crate::plan::{Current, plan_changes};
use crate::progress::{EpochClock, NoProgress, Progress, SystemEpoch};
use crate::snapshot::{SnapshotLimits, snapshot_with};
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
    /// Files not read at all because their size and modification time matched (ADR 0006).
    pub shortcut_hits: usize,
    /// Withheld by the secret denylist (shown to the user as "withheld").
    pub denied: u32,
    /// Left out by the project's ignore rules.
    pub ignored: u32,
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
    /// Files the worker crashed or hung on in this run, with the reason: quarantined until their content changes.
    pub newly_quarantined: Vec<(RepoPath, String)>,
    /// The walk stopped at its entry limit.
    pub walk_truncated: bool,
}

/// How long a file's modification time must precede the moment it was read for the shortcut to trust it (git's "racy" rule): an
/// edit within the timestamp's granularity after the read would otherwise be missed.
pub const RACY_NS: i64 = 2_000_000_000;

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
    /// Live progress (the engine adapts the job reporter to it).
    pub progress: &'a dyn Progress,
    /// Threads for the read and hash pass (bounded by the disk).
    pub read_workers: Workers,
    /// Threads for the extraction pass (one worker process each, bounded by cores and memory).
    pub parse_workers: Workers,
    /// The clock that stamps when a file was read.
    pub clock: &'a dyn EpochClock,
    /// The project's ignore rules (`.gitignore`); nothing is ignored by default.
    pub ignore: &'a dyn Ignore,
}

fn fs_error(e: FsError) -> IndexError {
    match e {
        FsError::Cancelled => IndexError::Cancelled,
        other => IndexError::Fs(other.to_string()),
    }
}

fn cancelled() -> JobError {
    JobError::Cancelled(CancelReason::User)
}

fn job_error(e: JobError) -> IndexError {
    match e {
        JobError::Cancelled(_) => IndexError::Cancelled,
        other => IndexError::Store(other.to_string()),
    }
}

/// Adds to the current phase of a [`Progress`].
struct Tick<'p>(&'p dyn Progress);

impl Counter for Tick<'_> {
    fn add(&self, n: u64) {
        self.0.add(n);
    }
}

/// What the read pass found for one file.
enum Read {
    TooLarge,
    Unreadable,
    /// Not read: the stamp matched.
    Same {
        path: RepoPath,
        fingerprint: Fingerprint,
        size: u64,
    },
    /// Read and hashed.
    Hashed {
        path: RepoPath,
        fingerprint: Fingerprint,
        size: u64,
        read_at_ns: i64,
    },
}

/// A file the extraction pass works on.
struct Work {
    path: RepoPath,
    file_id: FileId,
    fingerprint: Fingerprint,
    modified_ns: Option<i64>,
}

/// What the extraction pass found for one file.
struct Extracted {
    work: Work,
    size: u64,
    fingerprint: Fingerprint,
    read_at_ns: i64,
    facts: Option<FileFacts>,
    failure: Option<String>,
    crash: Option<String>,
    unreadable: bool,
}

impl<'a> Indexer<'a> {
    /// An indexer with default limits, no progress, one thread per pass and the system clock.
    pub fn new(fs: &'a dyn WorkspaceFs, source: &'a dyn SymbolSource, denylist: &'a Denylist) -> Indexer<'a> {
        static NO_PROGRESS: NoProgress = NoProgress;
        static SYSTEM: SystemEpoch = SystemEpoch;
        static NO_IGNORE: NoIgnore = NoIgnore;
        Indexer {
            fs,
            source,
            denylist,
            limits: IndexLimits::default(),
            progress: &NO_PROGRESS,
            read_workers: Workers::one(),
            parse_workers: Workers::one(),
            clock: &SYSTEM,
            ignore: &NO_IGNORE,
        }
    }

    /// Brings the store up to date with the workspace. All changes are one transaction; a cancel or a failure before the commit
    /// leaves the previous generation as it was. Phases: Finding files, Reading files, Reading code, Saving the index.
    pub fn run(&self, store: &mut IndexStore, cancel: &CancelToken) -> Result<IndexReport, IndexError> {
        let mut report = IndexReport::default();
        self.progress.phase("walk", "Finding files", None);
        let snap = snapshot_with(self.fs, self.denylist, self.ignore, &self.limits.snapshot, cancel, self.progress).map_err(|e| {
            if cancel.is_cancelled() {
                IndexError::Cancelled
            } else {
                fs_error(e)
            }
        })?;
        self.progress.set_total(snap.files.len() as u64);
        report.denied = snap.denied;
        report.links = snap.links;
        report.ignored = snap.ignored;
        report.skipped_other = snap.special + snap.bad_names + snap.too_deep + snap.unreadable;
        report.walk_truncated = snap.truncated;

        let previous = store.known_files()?;
        let stamps = store.stamps()?;

        // pass 1: fingerprint every file, reading only those whose stamp changed
        self.progress.phase("read", "Reading files", Some(snap.files.len() as u64));
        let results = par_map_ordered(
            snap.files.clone(),
            self.read_workers,
            cancel,
            &Tick(self.progress),
            |(path, size)| {
                if size > self.limits.max_file_bytes {
                    return Ok(Read::TooLarge);
                }
                let modified = snap.modified.get(&path).copied();
                if let (Some(stamp), Some(known)) = (
                    stamps.get(&path),
                    previous.binary_search_by(|k| k.path.cmp(&path)).ok().map(|i| &previous[i]),
                ) && stamp.size == size
                    && modified.is_some()
                    && stamp.modified_ns == modified
                    && stamp
                        .read_at_ns
                        .zip(modified)
                        .is_some_and(|(read, m)| m.saturating_add(RACY_NS) <= read)
                {
                    return Ok(Read::Same {
                        path,
                        fingerprint: known.fingerprint,
                        size,
                    });
                }
                match self.fs.read(&path, self.limits.max_file_bytes) {
                    Ok(bytes) => {
                        // a Cancel that arrived while the file was being read: do not hash it
                        if cancel.is_cancelled() {
                            return Err(cancelled());
                        }
                        let read_at_ns = self.clock.now_ns();
                        Ok(Read::Hashed {
                            path,
                            fingerprint: Fingerprint::of_content(&bytes),
                            size: bytes.len() as u64,
                            read_at_ns,
                        })
                    }
                    Err(FsError::TooLarge { .. }) => Ok(Read::TooLarge),
                    Err(_) => Ok(Read::Unreadable),
                }
            },
        )
        .map_err(job_error)?;

        let mut current = Vec::with_capacity(results.len());
        let mut sizes: BTreeMap<RepoPath, u64> = BTreeMap::new();
        let mut hashed_at: BTreeMap<RepoPath, i64> = BTreeMap::new();
        for r in results {
            match r {
                Read::TooLarge => report.too_large += 1,
                Read::Unreadable => report.unreadable += 1,
                Read::Same { path, fingerprint, size } => {
                    report.shortcut_hits += 1;
                    sizes.insert(path.clone(), size);
                    current.push(Current { path, fingerprint });
                }
                Read::Hashed {
                    path,
                    fingerprint,
                    size,
                    read_at_ns,
                } => {
                    sizes.insert(path.clone(), size);
                    hashed_at.insert(path.clone(), read_at_ns);
                    current.push(Current { path, fingerprint });
                }
            }
        }

        let plan = plan_changes(&previous, &current);
        report.unchanged = plan.unchanged.len();
        report.added = plan.added.len();
        report.modified = plan.modified.len();
        report.renamed = plan.renamed.len();
        report.deleted = plan.deleted.len();

        let id_of: BTreeMap<&RepoPath, FileId> = previous.iter().map(|k| (&k.path, k.file_id)).collect();
        let fingerprints: BTreeMap<&RepoPath, Fingerprint> = current.iter().map(|c| (&c.path, c.fingerprint)).collect();

        // files read again and found unchanged get a fresh stamp, so the next run skips them
        let restamps: Vec<(FileId, Option<i64>, i64)> = plan
            .unchanged
            .iter()
            .filter_map(|p| hashed_at.get(p).map(|read| (id_of[p], snap.modified.get(p).copied(), *read)))
            .collect();

        if report.added + report.modified + report.renamed + report.deleted == 0 {
            store.restamp(&restamps)?;
            return Ok(report);
        }

        let mut ids = FileIdAllocator::resume(store.next_file_sequence()?);
        let mut batch = Batch::default();
        for path in &plan.deleted {
            batch.deleted.push(id_of[path]);
        }
        for (from, to) in &plan.renamed {
            batch.renamed.push((id_of[from], to.clone()));
        }

        // pass 2: extract the facts of what is new or changed (one worker process per thread)
        let mut work = Vec::with_capacity(plan.added.len() + plan.modified.len());
        for path in &plan.added {
            let file_id = ids.issue().map_err(|e| IndexError::Store(e.to_string()))?;
            work.push(Work {
                path: path.clone(),
                file_id,
                fingerprint: fingerprints[path],
                modified_ns: snap.modified.get(path).copied(),
            });
        }
        for path in &plan.modified {
            work.push(Work {
                path: path.clone(),
                file_id: id_of[path],
                fingerprint: fingerprints[path],
                modified_ns: snap.modified.get(path).copied(),
            });
        }
        work.sort_by(|a, b| a.path.cmp(&b.path));
        self.progress.phase("extract", "Reading code", Some(work.len() as u64));
        let extracted = par_map_ordered(work, self.parse_workers, cancel, &Tick(self.progress), |w| {
            let size = sizes[&w.path];
            let bytes = match self.fs.read(&w.path, self.limits.max_file_bytes) {
                Ok(b) => b,
                Err(_) => {
                    let (fingerprint, read_at_ns) = (w.fingerprint, self.clock.now_ns());
                    return Ok(Extracted {
                        work: w,
                        size,
                        fingerprint,
                        read_at_ns,
                        facts: None,
                        failure: None,
                        crash: None,
                        unreadable: true,
                    });
                }
            };
            let read_at_ns = self.clock.now_ns();
            let fingerprint = Fingerprint::of_content(&bytes);
            let size = bytes.len() as u64;
            let (facts, failure, crash) = match self.source.extract(&w.path, &bytes, cancel) {
                Ok(facts) => (facts, None, None),
                Err(SourceError::Cancelled) => return Err(cancelled()),
                Err(SourceError::Failed(why)) => (None, Some(why), None),
                Err(SourceError::Crashed(why)) => (None, None, Some(why)),
            };
            Ok(Extracted {
                work: w,
                size,
                fingerprint,
                read_at_ns,
                facts,
                failure,
                crash,
                unreadable: false,
            })
        })
        .map_err(job_error)?;

        for e in extracted {
            if e.unreadable {
                report.unreadable += 1;
            }
            if let Some(why) = e.failure {
                report.extract_failure_count += 1;
                if report.extract_failures.len() < self.limits.max_failures_named {
                    report.extract_failures.push((e.work.path.clone(), why));
                }
            }
            if let Some(why) = e.crash {
                report.newly_quarantined.push((e.work.path.clone(), why.clone()));
                batch.quarantined.push((e.work.path.clone(), e.fingerprint, why));
            }
            if e.facts.is_none() {
                report.tier1_only += 1;
            }
            batch.upserts.push(Upsert {
                file_id: e.work.file_id,
                path: e.work.path,
                fingerprint: e.fingerprint,
                size: e.size,
                facts: e.facts,
                modified_ns: e.work.modified_ns,
                read_at_ns: e.read_at_ns,
            });
        }
        batch.next_file_sequence = ids.next_sequence();
        if cancel.is_cancelled() {
            return Err(IndexError::Cancelled);
        }
        self.progress.phase("save", "Saving the index", None);
        report.generation = Some(store.commit(&batch)?);
        store.restamp(&restamps)?;
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
