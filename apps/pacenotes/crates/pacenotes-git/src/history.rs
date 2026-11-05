//! [`GixHistory`]: the `History` port over gix, and the commit log.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pacenotes_core::ports::{ChangeKind, ChangeSet, FileChange, History, HistoryError};
use pacenotes_core::{CommitHash, RepoPath, Rev};
use paddock_jobs::CancelToken;

use crate::churn::CommitChanges;

/// The most commits one [`GixHistory::log`] call walks, whatever the caller asks for.
pub const MAX_LOG_COMMITS: usize = 100_000;
/// The most files recorded for one commit; a larger commit is recorded cut at this many.
pub const MAX_FILES_PER_COMMIT: usize = 10_000;

/// The longest author text kept.
pub const MAX_AUTHOR_BYTES: usize = 128;

pub(crate) fn cut_text(s: &mut String, max: usize) {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
}

/// Whether the walk ended at the record limit (`true`), or why it failed: a cancel is [`HistoryError::Cancelled`] and an
/// error of the diff that we did not cause by stopping it is a backend error.
fn walk_outcome<E: std::fmt::Display>(stop: Stop, result: Result<(), E>) -> Result<bool, HistoryError> {
    match (stop, result) {
        (Stop::Cancelled, _) => Err(HistoryError::Cancelled),
        (Stop::No, Err(e)) => Err(backend(e)),
        (stop, _) => Ok(stop == Stop::Limit),
    }
}

/// What a bounded tree walk found.
struct Walked {
    changes: Vec<FileChange>,
    skipped_names: u32,
    cut: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    No,
    Limit,
    Cancelled,
}

/// A tree-diff visitor that records changes like gix's recorder and stops the walk at a record limit or a cancel.
struct Bounded<'a> {
    recorder: gix::diff::tree::Recorder,
    max_records: usize,
    cancel: &'a CancelToken,
    stop: Stop,
}

impl gix::diff::tree::Visit for Bounded<'_> {
    fn pop_front_tracked_path_and_set_current(&mut self) {
        self.recorder.pop_front_tracked_path_and_set_current();
    }

    fn push_back_tracked_path_component(&mut self, component: &gix::bstr::BStr) {
        self.recorder.push_back_tracked_path_component(component);
    }

    fn push_path_component(&mut self, component: &gix::bstr::BStr) {
        self.recorder.push_path_component(component);
    }

    fn pop_path_component(&mut self) {
        self.recorder.pop_path_component();
    }

    fn visit(&mut self, change: gix::diff::tree::visit::Change) -> gix::diff::tree::visit::Action {
        if self.cancel.is_cancelled() {
            self.stop = Stop::Cancelled;
            return std::ops::ControlFlow::Break(());
        }
        if self.recorder.records.len() >= self.max_records {
            self.stop = Stop::Limit;
            return std::ops::ControlFlow::Break(());
        }
        self.recorder.visit(change)
    }
}

/// A repository opened by [`open`](crate::open). Cheap to share: every call takes its own thread-local handle.
pub struct GixHistory {
    repo: gix::ThreadSafeRepository,
    root: PathBuf,
}

pub(crate) fn backend(e: impl std::fmt::Display) -> HistoryError {
    HistoryError::Backend(e.to_string())
}

fn live(cancel: &CancelToken) -> Result<(), HistoryError> {
    if cancel.is_cancelled() {
        Err(HistoryError::Cancelled)
    } else {
        Ok(())
    }
}

/// The commit a revision names. A working-tree revision is not served here: this port never touches the working tree.
pub(crate) fn commit_id(rev: &Rev) -> Result<gix::ObjectId, HistoryError> {
    match rev {
        Rev::Commit(hash) => gix::ObjectId::from_hex(hash.as_str().as_bytes()).map_err(|_| HistoryError::UnknownRev(rev.to_string())),
        Rev::WorkingTree(_) => Err(HistoryError::UnknownRev(format!(
            "{rev} (the working tree is served by the index, not by history)"
        ))),
    }
}

pub(crate) fn hash_of(id: gix::ObjectId) -> Result<CommitHash, HistoryError> {
    CommitHash::parse(&id.to_string()).map_err(backend)
}

impl GixHistory {
    pub(crate) fn new(repo: gix::ThreadSafeRepository, root: PathBuf) -> GixHistory {
        GixHistory { repo, root }
    }

    pub(crate) fn thread_local_repo(&self) -> gix::Repository {
        self.repo.to_thread_local()
    }

    /// The canonical workspace root this history was opened for.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn commit<'r>(repo: &'r gix::Repository, rev: &Rev) -> Result<gix::Commit<'r>, HistoryError> {
        let id = commit_id(rev)?;
        repo.find_commit(id).map_err(|_| HistoryError::UnknownRev(rev.to_string()))
    }

    /// The files that changed between two trees, at most about `limit` of them, plus how many have names that are not valid
    /// [`RepoPath`]s and whether the walk stopped at the limit. Renames are found by identical blob ids. Reads trees only; the
    /// walk is bounded (it stops after `2 * limit + 2` raw changes) and checks `cancel` at every change.
    fn tree_changes(
        repo: &gix::Repository,
        old: Option<&gix::Tree<'_>>,
        new: &gix::Tree<'_>,
        limit: usize,
        cancel: &CancelToken,
    ) -> Result<Walked, HistoryError> {
        let empty = repo.empty_tree();
        let old = old.unwrap_or(&empty);
        let mut bounded = Bounded {
            recorder: gix::diff::tree::Recorder::default(),
            max_records: limit.saturating_mul(2).saturating_add(2),
            cancel,
            stop: Stop::No,
        };
        let result = gix::diff::tree(
            gix::objs::TreeRefIter::from_bytes(&old.data, old.id.kind()),
            gix::objs::TreeRefIter::from_bytes(&new.data, new.id.kind()),
            gix::diff::tree::State::default(),
            &repo.objects,
            &mut bounded,
        );
        let cut = walk_outcome(bounded.stop, result)?;

        use gix::diff::tree::recorder::Change;
        let is_file = |mode: &gix::objs::tree::EntryMode| mode.is_blob() || mode.is_link();
        let mut skipped = 0u32;
        let mut parse = |path: &gix::bstr::BString| {
            let parsed = RepoPath::parse(&path.to_string()).ok();
            if parsed.is_none() {
                skipped = skipped.saturating_add(1);
            }
            parsed
        };
        // exact renames: a deleted and an added entry with the same blob id
        let mut deleted: BTreeMap<gix::ObjectId, Vec<RepoPath>> = BTreeMap::new();
        let mut added: Vec<(gix::ObjectId, RepoPath)> = Vec::new();
        let mut out: Vec<FileChange> = Vec::new();
        for change in bounded.recorder.records {
            match change {
                Change::Modification { entry_mode, path, .. } if is_file(&entry_mode) => {
                    if let Some(path) = parse(&path) {
                        out.push(FileChange {
                            path,
                            kind: ChangeKind::Modified,
                        });
                    }
                }
                Change::Deletion { entry_mode, oid, path, .. } if is_file(&entry_mode) => {
                    if let Some(path) = parse(&path) {
                        deleted.entry(oid).or_default().push(path);
                    }
                }
                Change::Addition { entry_mode, oid, path, .. } if is_file(&entry_mode) => {
                    if let Some(path) = parse(&path) {
                        added.push((oid, path));
                    }
                }
                _ => {}
            }
        }
        for paths in deleted.values_mut() {
            paths.sort();
            paths.reverse(); // pop() takes the smallest first
        }
        added.sort_by(|a, b| a.1.cmp(&b.1));
        for (oid, path) in added {
            match deleted.get_mut(&oid).and_then(Vec::pop) {
                Some(from) => out.push(FileChange {
                    path,
                    kind: ChangeKind::Renamed { from },
                }),
                None => out.push(FileChange {
                    path,
                    kind: ChangeKind::Added,
                }),
            }
        }
        for path in deleted.into_values().flatten() {
            out.push(FileChange {
                path,
                kind: ChangeKind::Deleted,
            });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Walked {
            changes: out,
            skipped_names: skipped,
            cut,
        })
    }

    /// What changed in each commit on the first-parent line from `from`, newest first, at most `max_commits` (and at most
    /// [`MAX_LOG_COMMITS`]). A merge commit shows what it brought in against its first parent. Stops at cancel with
    /// [`HistoryError::Cancelled`].
    pub fn log(&self, from: &Rev, max_commits: usize, cancel: &CancelToken) -> Result<Vec<CommitChanges>, HistoryError> {
        live(cancel)?;
        let repo = self.repo.to_thread_local();
        let start = commit_id(from)?;
        repo.find_commit(start).map_err(|_| HistoryError::UnknownRev(from.to_string()))?;
        let walk = repo.rev_walk([start]).first_parent_only().all().map_err(backend)?;
        let mut out = Vec::new();
        for info in walk.take(max_commits.min(MAX_LOG_COMMITS)) {
            live(cancel)?;
            let info = info.map_err(backend)?;
            let commit = repo.find_commit(info.id).map_err(backend)?;
            let tree = commit.tree().map_err(backend)?;
            let parent_tree = match commit.parent_ids().next() {
                Some(parent) => Some(repo.find_commit(parent).map_err(backend)?.tree().map_err(backend)?),
                None => None,
            };
            let walked = Self::tree_changes(&repo, parent_tree.as_ref(), &tree, MAX_FILES_PER_COMMIT, cancel)?;
            let truncated = walked.cut || walked.changes.len() > MAX_FILES_PER_COMMIT;
            let skipped_names = walked.skipped_names;
            let mut files: Vec<RepoPath> = walked.changes.into_iter().map(|c| c.path).collect();
            files.truncate(MAX_FILES_PER_COMMIT);
            let mut author = commit.author().map_err(backend)?.email.to_string();
            cut_text(&mut author, MAX_AUTHOR_BYTES);
            out.push(CommitChanges {
                commit: hash_of(info.id)?,
                time: commit.time().map_err(backend)?.seconds,
                author,
                files,
                skipped_names,
                truncated,
            });
        }
        Ok(out)
    }
}

impl History for GixHistory {
    fn head(&self, cancel: &CancelToken) -> Result<Rev, HistoryError> {
        live(cancel)?;
        let repo = self.repo.to_thread_local();
        let id = repo
            .head_id()
            .map_err(|_| HistoryError::UnknownRev("HEAD (the repository has no commits)".to_owned()))?;
        Ok(Rev::Commit(hash_of(id.detach())?))
    }

    fn read_at(&self, rev: &Rev, path: &RepoPath, max_bytes: u64, cancel: &CancelToken) -> Result<Vec<u8>, HistoryError> {
        live(cancel)?;
        let repo = self.repo.to_thread_local();
        let tree = Self::commit(&repo, rev)?.tree().map_err(backend)?;
        let entry = tree
            .lookup_entry_by_path(path.as_str())
            .map_err(backend)?
            .filter(|e| e.mode().is_blob() || e.mode().is_link())
            .ok_or_else(|| HistoryError::NotFound(path.clone()))?;
        let id = entry.object_id();
        // the header gives the size without reading the content
        let size = repo.find_header(id).map_err(backend)?.size();
        if size > max_bytes {
            return Err(HistoryError::TooLarge {
                path: path.clone(),
                limit: max_bytes,
            });
        }
        live(cancel)?;
        let blob = repo.find_object(id).map_err(backend)?;
        Ok(blob.data.clone())
    }

    fn changed_between(&self, from: &Rev, to: &Rev, limit: usize, cancel: &CancelToken) -> Result<ChangeSet, HistoryError> {
        live(cancel)?;
        let repo = self.repo.to_thread_local();
        let old = Self::commit(&repo, from)?.tree().map_err(backend)?;
        let new = Self::commit(&repo, to)?.tree().map_err(backend)?;
        live(cancel)?;
        let walked = Self::tree_changes(&repo, Some(&old), &new, limit, cancel)?;
        let truncated = walked.cut || walked.changes.len() > limit;
        let mut changes = walked.changes;
        changes.truncate(limit);
        Ok(ChangeSet {
            changes,
            truncated,
            skipped_names: walked.skipped_names,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gix::diff::tree::Visit as _;
    use gix::diff::tree::visit::Change;

    fn change() -> Change {
        Change::Deletion {
            entry_mode: gix::objs::tree::EntryKind::Blob.into(),
            oid: gix::ObjectId::null(gix::hash::Kind::Sha1),
            relation: None,
        }
    }

    #[test]
    fn the_walk_stops_at_the_record_limit_and_at_a_cancel() {
        let live = CancelToken::new();
        let mut b = Bounded {
            recorder: gix::diff::tree::Recorder::default(),
            max_records: 2,
            cancel: &live,
            stop: Stop::No,
        };
        assert!(b.visit(change()).is_continue() && b.visit(change()).is_continue());
        assert!(b.visit(change()).is_break() && b.stop == Stop::Limit);

        let dead = CancelToken::new();
        dead.cancel();
        let mut b = Bounded {
            recorder: gix::diff::tree::Recorder::default(),
            max_records: 10,
            cancel: &dead,
            stop: Stop::No,
        };
        assert!(b.visit(change()).is_break() && b.stop == Stop::Cancelled);
        assert!(b.recorder.records.is_empty());
    }

    #[test]
    fn a_stopped_walk_is_a_cut_a_cancel_or_a_backend_error() {
        assert_eq!(walk_outcome(Stop::No, Ok::<(), String>(())), Ok(false));
        assert_eq!(walk_outcome(Stop::Limit, Err("the delegate cancelled".to_owned())), Ok(true));
        assert_eq!(
            walk_outcome(Stop::Cancelled, Err("the delegate cancelled".to_owned())),
            Err(HistoryError::Cancelled)
        );
        assert!(matches!(
            walk_outcome(Stop::No, Err("bad tree".to_owned())),
            Err(HistoryError::Backend(_))
        ));
    }

    #[test]
    fn author_text_is_cut_on_a_character_boundary() {
        let mut s = format!("a{}", "é".repeat(100));
        cut_text(&mut s, MAX_AUTHOR_BYTES);
        assert!(s.len() <= MAX_AUTHOR_BYTES && s.chars().skip(1).all(|c| c == 'é'));
        let mut short = "a@b".to_owned();
        cut_text(&mut short, MAX_AUTHOR_BYTES);
        assert_eq!(short, "a@b");
    }
}
