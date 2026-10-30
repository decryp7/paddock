//! Per-file and per-range authorship (package C2b): who wrote each line of a file at a revision, renames followed, for the ownership
//! figures. gix's blame diffs blobs, which ADR 0004 had refused outright; the amendment of C2b says what makes it safe here and the trap
//! test proves it: the repository is opened isolated as always, blame reads only objects from the object database (never the worktree,
//! the index or a filter), and a `diff.*.textconv` or `filter.*` that the repository's own config or attributes name is never run.

use std::collections::BTreeMap;

use pacenotes_core::ports::HistoryError;
use pacenotes_core::{CommitHash, RepoPath, Rev};
use paddock_jobs::CancelToken;

use crate::history::{GixHistory, MAX_AUTHOR_BYTES};

/// The most hunks one blame returns.
pub const MAX_BLAME_HUNKS: usize = 50_000;

/// A run of lines written by one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameHunk {
    /// The first line in the blamed revision (1-based).
    pub first_line: u32,
    /// The last line in the blamed revision (1-based, inclusive).
    pub last_line: u32,
    /// The commit that introduced them.
    pub commit: CommitHash,
    /// The author's email (bounded), as the log gives it.
    pub author: String,
    /// The commit time, seconds since the Unix epoch.
    pub time: i64,
    /// The file's name in that commit when it differs from the blamed one (the file was renamed since).
    pub source_path: Option<RepoPath>,
}

impl BlameHunk {
    /// How many lines the hunk covers.
    pub fn lines(&self) -> u32 {
        self.last_line - self.first_line + 1
    }
}

/// The authorship of a file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Blame {
    /// The hunks, in line order.
    pub hunks: Vec<BlameHunk>,
    /// Set when the list was cut at [`MAX_BLAME_HUNKS`].
    pub truncated: bool,
}

impl Blame {
    /// Lines per author, most lines first (ties by author), for the ownership figures.
    pub fn lines_by_author(&self) -> Vec<(String, u32)> {
        let mut by: BTreeMap<&str, u32> = BTreeMap::new();
        for h in &self.hunks {
            *by.entry(h.author.as_str()).or_insert(0) += h.lines();
        }
        let mut out: Vec<(String, u32)> = by.into_iter().map(|(a, n)| (a.to_owned(), n)).collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }
}

fn backend(e: impl std::fmt::Display) -> HistoryError {
    HistoryError::Backend(e.to_string())
}

impl GixHistory {
    /// Who wrote each line of `path` at `rev`, or of lines `range` (1-based, inclusive) only. Renames are followed. `since` (seconds
    /// since the epoch) stops the walk at commits older than that; lines whose origin lies further back are attributed to the last commit the walk reached. gix's blame cannot be interrupted, so `cancel` is looked at before it starts; `since` and the
    /// line range are what bound the work on a large history.
    pub fn blame(
        &self,
        rev: &Rev,
        path: &RepoPath,
        range: Option<(u32, u32)>,
        since: Option<i64>,
        cancel: &CancelToken,
    ) -> Result<Blame, HistoryError> {
        self.blame_limited(rev, path, range, since, MAX_BLAME_HUNKS, cancel)
    }

    /// As [`blame`](Self::blame), keeping at most `max_hunks` hunks (the answer says it was cut).
    pub fn blame_limited(
        &self,
        rev: &Rev,
        path: &RepoPath,
        range: Option<(u32, u32)>,
        since: Option<i64>,
        max_hunks: usize,
        cancel: &CancelToken,
    ) -> Result<Blame, HistoryError> {
        if cancel.is_cancelled() {
            return Err(HistoryError::Cancelled);
        }
        let Rev::Commit(hash) = rev else {
            return Err(HistoryError::UnknownRev(format!(
                "{rev} (the working tree is served by the index, not by history)"
            )));
        };
        let id = gix::ObjectId::from_hex(hash.as_str().as_bytes()).map_err(|_| HistoryError::UnknownRev(rev.to_string()))?;
        let repo = self.thread_local_repo();
        repo.find_commit(id).map_err(|_| HistoryError::UnknownRev(rev.to_string()))?;
        let ranges = match range {
            Some((a, b)) if a >= 1 && b >= a => gix::blame::BlameRanges::from_one_based_inclusive_range(a..=b).map_err(backend)?,
            Some(_) => {
                return Err(HistoryError::Backend(
                    "the line range must be 1-based with the end not before the start".to_owned(),
                ));
            }
            None => gix::blame::BlameRanges::default(),
        };
        let options = gix::repository::blame_file::Options {
            diff_algorithm: Some(gix::diff::blob::Algorithm::Myers),
            ranges,
            since: since.map(|s| gix::date::Time::new(s, 0)),
            rewrites: Some(gix::diff::Rewrites::default()),
        };
        let outcome = repo.blame_file(path.as_str().into(), id, options).map_err(|e| {
            let text = e.to_string();
            if text.contains("wasn't found") {
                HistoryError::NotFound(path.clone())
            } else {
                backend(text)
            }
        })?;
        let mut entries = outcome.entries;
        entries.sort_by_key(|e| e.start_in_blamed_file);
        let mut blame = Blame::default();
        let mut who: BTreeMap<gix::ObjectId, (String, i64)> = BTreeMap::new();
        for e in entries {
            if blame.hunks.len() >= max_hunks {
                blame.truncated = true;
                break;
            }
            let (author, time) = match who.get(&e.commit_id) {
                Some(known) => known.clone(),
                None => {
                    let commit = repo.find_commit(e.commit_id).map_err(backend)?;
                    let mut author = commit.author().map_err(backend)?.email.to_string();
                    crate::history::cut_text(&mut author, MAX_AUTHOR_BYTES);
                    let time = commit.time().map_err(backend)?.seconds;
                    who.insert(e.commit_id, (author.clone(), time));
                    (author, time)
                }
            };
            let first = e.start_in_blamed_file + 1;
            blame.hunks.push(BlameHunk {
                first_line: first,
                last_line: first + e.len.get() - 1,
                commit: CommitHash::parse(&e.commit_id.to_string()).map_err(backend)?,
                author,
                time,
                source_path: e
                    .source_file_name
                    .as_deref()
                    .and_then(|n| std::str::from_utf8(n).ok())
                    .and_then(|n| RepoPath::parse(n).ok()),
            });
        }
        Ok(blame)
    }
}
