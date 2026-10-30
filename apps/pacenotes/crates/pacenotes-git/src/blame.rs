//! Per-file and per-range authorship (package C2b): who wrote each line of a file at a revision, renames followed, for the ownership
//! figures. gix's blame diffs blobs, which ADR 0004 had refused outright; its C2b amendment says what makes it safe here, in two layers
//! that this module is the only place to touch:
//!
//! 1. the repository is opened isolated and with reduced trust (see [`crate::open`]), which drops `diff.*`, `filter.*` and
//!    `core.attributesFile`, and `core.bigFileThreshold` is pinned there;
//! 2. the diff resource cache is built here, by hand, in mode "to git" with **no worktree roots** and an **empty attribute stack**: no
//!    `.gitattributes` from anywhere (not the tree, not the index, not `info/attributes`) can switch a file to "binary", turn on a filter
//!    or name a text conversion, and the index is never read.
//!
//! The object database is reached through [`Guarded`], which checks the cancel token and a budget on every lookup, refuses a blob
//! over the cap from its header before reading it, and counts objects read as the progress figure.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use pacenotes_core::ports::{Blame, BlameError, BlameHunk, BlameRequest, Blamer};
use pacenotes_core::{RepoPath, Rev};
use paddock_jobs::CancelToken;

use crate::history::{GixHistory, MAX_AUTHOR_BYTES, commit_id, cut_text, hash_of};

/// Why the guarded object database stopped a lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Cancelled = 1,
    Objects = 2,
    Blob = 3,
}

#[derive(Debug, thiserror::Error)]
enum Stopped {
    #[error("the blame was cancelled")]
    Cancelled,
    #[error("the blame read more objects than its limit")]
    Objects,
    #[error("the blame met a file larger than its limit")]
    Blob,
}

/// The object database with a leash.
struct Guarded<'a, O> {
    inner: &'a O,
    cancel: &'a CancelToken,
    max_objects: u64,
    max_blob: u64,
    objects: AtomicU64,
    stop: AtomicU8,
    stopped_size: AtomicU64,
    progress: &'a dyn Fn(u64),
}

impl<O> Guarded<'_, O> {
    /// The typed error for a lookup that failed: the guard's own reason if it stopped it, else the backend's words.
    fn error(&self, request: &BlameRequest, e: &gix::Error) -> BlameError {
        match self.reason() {
            Some(Stop::Cancelled) => BlameError::Cancelled,
            Some(Stop::Objects) => BlameError::Limit {
                what: "objects read",
                got: self.objects.load(Ordering::Relaxed),
                max: request.limits.max_objects,
            },
            Some(Stop::Blob) => BlameError::Limit {
                what: "file size in bytes",
                got: self.stopped_size.load(Ordering::SeqCst),
                max: request.limits.max_blob_bytes,
            },
            None => BlameError::Backend(e.to_string()),
        }
    }

    fn halt(&self, why: Stop) -> gix::Error {
        self.stop.store(why as u8, Ordering::SeqCst);
        gix::Error::from_error(match why {
            Stop::Cancelled => Stopped::Cancelled,
            Stop::Objects => Stopped::Objects,
            Stop::Blob => Stopped::Blob,
        })
    }

    fn tick(&self) -> Result<(), gix::Error> {
        if self.cancel.is_cancelled() {
            return Err(self.halt(Stop::Cancelled));
        }
        let n = self.objects.fetch_add(1, Ordering::Relaxed) + 1;
        if n > self.max_objects {
            return Err(self.halt(Stop::Objects));
        }
        if n.is_multiple_of(64) {
            (self.progress)(n);
        }
        Ok(())
    }

    fn reason(&self) -> Option<Stop> {
        match self.stop.load(Ordering::SeqCst) {
            1 => Some(Stop::Cancelled),
            2 => Some(Stop::Objects),
            3 => Some(Stop::Blob),
            _ => None,
        }
    }
}

impl<O: gix::objs::FindHeader> gix::objs::FindHeader for Guarded<'_, O> {
    fn try_header(&self, id: &gix::oid) -> Result<Option<gix::objs::Header>, gix::Error> {
        self.inner.try_header(id)
    }
}

impl<O: gix::objs::Find + gix::objs::FindHeader> gix::objs::Find for Guarded<'_, O> {
    fn try_find<'b>(&self, id: &gix::oid, buffer: &'b mut Vec<u8>) -> Result<Option<gix::objs::Data<'b>>, gix::Error> {
        self.tick()?;
        // the header first: a blob over the cap is refused before a byte of it is read or inflated
        if let Some(h) = self.inner.try_header(id)?
            && h.kind == gix::objs::Kind::Blob
            && h.size > self.max_blob
        {
            self.stopped_size.store(h.size, Ordering::SeqCst);
            return Err(self.halt(Stop::Blob));
        }
        self.inner.try_find(id, buffer)
    }
}

/// The lines of a blamed file (a last line without a newline counts).
fn line_count(blob: &[u8]) -> u32 {
    let n = blob.iter().filter(|b| **b == b'\n').count() + usize::from(blob.last().is_some_and(|b| *b != b'\n'));
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl Blamer for GixHistory {
    fn blame(
        &self,
        rev: &Rev,
        path: &RepoPath,
        request: &BlameRequest,
        progress: &dyn Fn(u64),
        cancel: &CancelToken,
    ) -> Result<Blame, BlameError> {
        if cancel.is_cancelled() {
            return Err(BlameError::Cancelled);
        }
        let Ok(id) = commit_id(rev) else {
            return Err(BlameError::UnknownRev(rev.to_string()));
        };
        let repo = self.thread_local_repo();
        let commit = repo.find_commit(id).map_err(|_| BlameError::UnknownRev(rev.to_string()))?;
        // is the file there? looked up in the commit's tree, so a missing path is `NotFound` deterministically, whatever words gix's own
        // errors use (and a damaged object database stays a backend error)
        let entry = commit
            .tree()
            .and_then(|tree| tree.lookup_entry_by_path(path.as_str()))
            .map_err(|e| BlameError::Backend(e.to_string()))?
            .filter(|e| e.mode().is_blob());
        let Some(entry) = entry else {
            return Err(BlameError::NotFound(path.clone()));
        };
        let blob_id = entry.object_id();
        let ranges = match request.range {
            Some((a, b)) if a >= 1 && b >= a => {
                gix::blame::BlameRanges::from_one_based_inclusive_range(a..=b).map_err(|e| BlameError::Backend(e.to_string()))?
            }
            Some((first, last)) => return Err(BlameError::InvalidRange { first, last }),
            None => gix::blame::BlameRanges::default(),
        };
        let options = gix::blame::Options {
            diff_algorithm: gix::diff::blob::Algorithm::Myers,
            ranges,
            since: request.since.map(|s| gix::date::Time::new(s, 0)),
            rewrites: Some(gix::diff::Rewrites::default()),
            debug_track_path: false,
        };
        // an attribute stack with nothing in it: no globals, no `info/attributes`, no `.gitattributes` from any tree or index
        let attributes = gix::worktree::stack::state::Attributes::new(
            gix::attrs::Search::default(),
            None,
            gix::worktree::stack::state::attributes::Source::IdMapping,
            gix::attrs::search::MetadataCollection::default(),
        );
        let stack = gix::worktree::Stack::new(
            repo.git_dir(),
            gix::worktree::stack::State::AttributesStack(attributes),
            gix::glob::pattern::Case::Sensitive,
            Vec::new(),
            Vec::new(),
        );
        let mut cache = gix::diff::resource_cache(
            &repo,
            gix::diff::blob::pipeline::Mode::ToGit,
            stack,
            gix::diff::blob::pipeline::WorktreeRoots::default(),
        )
        .map_err(|e| BlameError::Backend(e.to_string()))?;
        let guard = Guarded {
            inner: &repo.objects,
            cancel,
            max_objects: request.limits.max_objects,
            max_blob: request.limits.max_blob_bytes,
            objects: AtomicU64::new(0),
            stop: AtomicU8::new(0),
            stopped_size: AtomicU64::new(0),
            progress,
        };
        // the blamed blob is read once, through the guard (so the cap, the budget and the cancel apply), before the walk: the range is
        // checked against its real line count and a binary file is refused here, not after a walk that would be thrown away
        let mut buf = Vec::new();
        let blob = match gix::objs::Find::try_find(&guard, &blob_id, &mut buf) {
            Ok(Some(data)) => data.data.to_vec(),
            Ok(None) => return Err(BlameError::NotFound(path.clone())),
            Err(e) => return Err(guard.error(request, &e)),
        };
        // git's own test for binary: a NUL in the first 8000 bytes. gix then gives the whole file to the oldest commit, which is not an
        // attribution, so it is refused.
        if blob.iter().take(8000).any(|b| *b == 0) {
            return Err(BlameError::Binary(path.clone()));
        }
        let lines = line_count(&blob);
        if let Some((first, last)) = request.range
            && last > lines
        {
            return Err(BlameError::RangeOutsideFile { first, last, lines });
        }
        drop(blob);
        let outcome = gix::blame::file(
            &guard,
            gix::blame::Start::Commit(id),
            None,
            &mut cache,
            path.as_str().into(),
            options,
        );
        let objects = guard.objects.load(Ordering::Relaxed);
        progress(objects);
        let outcome = match outcome {
            Ok(o) => o,
            Err(e) => return Err(guard.error(request, &e)),
        };
        let mut entries = outcome.entries;
        entries.sort_by_key(|e| e.start_in_blamed_file);
        let mut blame = Blame {
            objects_read: objects,
            ..Blame::default()
        };
        let mut who: BTreeMap<gix::ObjectId, (String, i64)> = BTreeMap::new();
        for e in entries {
            if blame.hunks.len() >= request.limits.max_hunks {
                blame.truncated = true;
                break;
            }
            let (author, time) = match who.get(&e.commit_id) {
                Some(known) => known.clone(),
                None => {
                    let commit = repo.find_commit(e.commit_id).map_err(|e| BlameError::Backend(e.to_string()))?;
                    let mut author = commit.author().map_err(|e| BlameError::Backend(e.to_string()))?.email.to_string();
                    cut_text(&mut author, MAX_AUTHOR_BYTES);
                    let time = commit.time().map_err(|e| BlameError::Backend(e.to_string()))?.seconds;
                    who.insert(e.commit_id, (author.clone(), time));
                    (author, time)
                }
            };
            let first = e.start_in_blamed_file + 1;
            let source_path = match e.source_file_name.as_deref() {
                None => None,
                Some(name) => match std::str::from_utf8(name).ok().and_then(|n| RepoPath::parse(n).ok()) {
                    Some(p) => Some(p),
                    None => {
                        blame.unnamed_sources += 1;
                        None
                    }
                },
            };
            blame.hunks.push(BlameHunk {
                first_line: first,
                last_line: first + e.len.get() - 1,
                commit: hash_of(e.commit_id).map_err(|e| BlameError::Backend(e.to_string()))?,
                author,
                time,
                source_path,
            });
        }
        Ok(blame)
    }
}
