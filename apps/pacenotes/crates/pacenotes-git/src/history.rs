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

/// A repository opened by [`open`](crate::open). Cheap to share: every call takes its own thread-local handle.
pub struct GixHistory {
    repo: gix::ThreadSafeRepository,
    root: PathBuf,
}

fn backend(e: impl std::fmt::Display) -> HistoryError {
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
fn commit_id(rev: &Rev) -> Result<gix::ObjectId, HistoryError> {
    match rev {
        Rev::Commit(hash) => gix::ObjectId::from_hex(hash.as_str().as_bytes()).map_err(|_| HistoryError::UnknownRev(rev.to_string())),
        Rev::WorkingTree(_) => Err(HistoryError::UnknownRev(format!(
            "{rev} (the working tree is served by the index, not by history)"
        ))),
    }
}

fn hash_of(id: gix::ObjectId) -> Result<CommitHash, HistoryError> {
    CommitHash::parse(&id.to_string()).map_err(backend)
}

impl GixHistory {
    pub(crate) fn new(repo: gix::ThreadSafeRepository, root: PathBuf) -> GixHistory {
        GixHistory { repo, root }
    }

    /// The canonical workspace root this history was opened for.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn commit<'r>(repo: &'r gix::Repository, rev: &Rev) -> Result<gix::Commit<'r>, HistoryError> {
        let id = commit_id(rev)?;
        repo.find_commit(id).map_err(|_| HistoryError::UnknownRev(rev.to_string()))
    }

    /// The files that changed between a commit's first parent and the commit (all of its files for a root commit), cut at
    /// `limit`; renames are found by identical blob ids. Reads trees only.
    fn tree_changes(repo: &gix::Repository, old: Option<&gix::Tree<'_>>, new: &gix::Tree<'_>) -> Result<Vec<FileChange>, HistoryError> {
        let empty = repo.empty_tree();
        let old = old.unwrap_or(&empty);
        let mut recorder = gix::diff::tree::Recorder::default();
        gix::diff::tree(
            gix::objs::TreeRefIter::from_bytes(&old.data, old.id.kind()),
            gix::objs::TreeRefIter::from_bytes(&new.data, new.id.kind()),
            gix::diff::tree::State::default(),
            &repo.objects,
            &mut recorder,
        )
        .map_err(backend)?;

        use gix::diff::tree::recorder::Change;
        let is_file = |mode: &gix::objs::tree::EntryMode| mode.is_blob() || mode.is_link();
        // exact renames: a deleted and an added entry with the same blob id
        let mut deleted: BTreeMap<gix::ObjectId, Vec<RepoPath>> = BTreeMap::new();
        let mut added: Vec<(gix::ObjectId, RepoPath)> = Vec::new();
        let mut out: Vec<FileChange> = Vec::new();
        for change in recorder.records {
            match change {
                Change::Modification { entry_mode, path, .. } if is_file(&entry_mode) => {
                    if let Ok(path) = RepoPath::parse(&path.to_string()) {
                        out.push(FileChange {
                            path,
                            kind: ChangeKind::Modified,
                        });
                    }
                }
                Change::Deletion { entry_mode, oid, path, .. } if is_file(&entry_mode) => {
                    if let Ok(path) = RepoPath::parse(&path.to_string()) {
                        deleted.entry(oid).or_default().push(path);
                    }
                }
                Change::Addition { entry_mode, oid, path, .. } if is_file(&entry_mode) => {
                    if let Ok(path) = RepoPath::parse(&path.to_string()) {
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
        Ok(out)
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
            let mut files: Vec<RepoPath> = Self::tree_changes(&repo, parent_tree.as_ref(), &tree)?
                .into_iter()
                .map(|c| c.path)
                .collect();
            files.truncate(MAX_FILES_PER_COMMIT);
            let author = commit.author().map_err(backend)?;
            out.push(CommitChanges {
                commit: hash_of(info.id)?,
                time: commit.time().map_err(backend)?.seconds,
                author: author.email.to_string(),
                files,
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
        let mut changes = Self::tree_changes(&repo, Some(&old), &new)?;
        let truncated = changes.len() > limit;
        changes.truncate(limit);
        Ok(ChangeSet { changes, truncated })
    }
}
