//! What state a tracked folder is in, and so what `clean` may do with it.
//!
//! [`classify`] is pure: it takes the facts and answers one state. [`inspect`] gathers the facts (git, the owner map, the
//! record, the clock). Anything that cannot be established counts against removal: a missing answer is never "safe".

use super::record::{self, Kind, Record};
use crate::worktree::git::{Git, Repo, key, worktrees};
use crate::worktree::owners::OwnerStore;
use std::path::{Path, PathBuf};

/// The state of a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum State {
    /// Locked, owned, a train, or used within the live window: never touched.
    Live,
    /// Its branch is merged into main or a train, and nothing in it is unsaved.
    Merged,
    /// Clean, and every commit is on a remote.
    StalePushed,
    /// Dirty, has unpushed commits, or something about it could not be established.
    Unsaved,
    /// No owner record.
    Orphan,
}

impl State {
    /// The word in the report and the index.
    pub fn word(self) -> &'static str {
        match self {
            State::Live => "live",
            State::Merged => "merged",
            State::StalePushed => "stale-pushed",
            State::Unsaved => "unsaved",
            State::Orphan => "orphan",
        }
    }

    /// Reads the word back.
    pub fn parse(w: &str) -> Option<State> {
        Some(match w {
            "live" => State::Live,
            "merged" => State::Merged,
            "stale-pushed" => State::StalePushed,
            "unsaved" => State::Unsaved,
            "orphan" => State::Orphan,
            _ => return None,
        })
    }

    /// The states `clean --apply` may remove.
    pub fn reclaimable(self) -> bool {
        matches!(self, State::Merged | State::StalePushed)
    }
}

/// What is known about one folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// The folder has an owner record.
    pub has_record: bool,
    /// Locked, claimed by an agent, a train worktree, or used within the live window.
    pub live: bool,
    /// The folder is a git worktree of a repository that could be read.
    pub git_ok: bool,
    /// Uncommitted or untracked files (`None`: not established).
    pub dirty: Option<bool>,
    /// Commits that are on no remote (`None`: not established).
    pub unpushed: Option<bool>,
    /// HEAD is in main or a train (`None`: not established).
    pub merged: Option<bool>,
    /// The folder holds no git work at all (a build folder or scratch with nothing linked): there is nothing to lose.
    pub no_git_work: bool,
}

/// The one state the facts give.
pub fn classify(f: &Facts) -> State {
    if !f.has_record {
        return State::Orphan;
    }
    if f.live {
        return State::Live;
    }
    if f.no_git_work {
        return State::StalePushed;
    }
    if !f.git_ok || f.dirty != Some(false) || f.unpushed != Some(false) {
        return State::Unsaved;
    }
    if f.merged == Some(true) {
        State::Merged
    } else {
        State::StalePushed
    }
}

/// What the gathering needs from outside: the clock and the folders processes are in.
pub struct Context {
    /// Now, seconds since the epoch.
    pub now: u64,
    /// The live window in seconds.
    pub live_secs: u64,
    /// The current folders of running processes (`None`: could not be read).
    pub process_folders: Option<Vec<PathBuf>>,
}

/// The state, the last-used time and a reason for a folder.
#[derive(Debug, Clone)]
pub struct Inspected {
    /// The state.
    pub state: State,
    /// The later of the record's last use and the folder's own modification time.
    pub last_used: u64,
    /// The facts the state came from.
    pub facts: Facts,
}

fn mtime(dir: &Path) -> u64 {
    std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// What git says about a worktree folder.
struct GitFacts {
    live: bool,
    git_ok: bool,
    dirty: Option<bool>,
    unpushed: Option<bool>,
    merged: Option<bool>,
}

fn git_facts(dir: &Path, rec: &Record, ctx: &Context) -> GitFacts {
    let mut f = GitFacts {
        live: false,
        git_ok: false,
        dirty: None,
        unpushed: None,
        merged: None,
    };
    let Ok(repo) = Repo::discover(dir) else { return f };
    let me = key(dir);
    let Ok(list) = worktrees(&repo) else { return f };
    let Some(entry) = list.iter().find(|w| key(Path::new(&w.path)) == me) else {
        return f;
    };
    f.git_ok = true;
    let git = Git::at(dir);
    let branch = entry.branch.clone().unwrap_or_default();
    // Locked, claimed by an agent (an unreadable owner map counts as claimed), or a train: never touched.
    let owned = OwnerStore::open(&repo.common_dir).read().map_or(true, |m| m.get(&me).is_some());
    f.live = entry.locked.is_some() || owned || branch.starts_with("train/") || rec.owner.starts_with("integrator");
    f.dirty = git
        .text(&[
            "status",
            "--porcelain",
            "--untracked-files=normal",
            "--",
            ".",
            &format!(":(exclude){}", record::FILE),
        ])
        .ok()
        .map(|s| !s.trim().is_empty());
    f.unpushed = git
        .text(&["rev-list", "--count", "HEAD", "--not", "--remotes"])
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|n| n > 0);
    f.merged = git
        .text(&[
            "for-each-ref",
            "--contains",
            "HEAD",
            "--format=%(refname)",
            "refs/remotes/origin/main",
            "refs/remotes/origin/train",
        ])
        .ok()
        .map(|s| !s.trim().is_empty());
    if let Some(folders) = &ctx.process_folders {
        f.live |= crate::worktree::procs::any_inside(folders, dir);
    }
    f
}

/// Gathers the facts of a folder and classifies it. `rec` is its record (`None`: an orphan).
pub fn inspect(dir: &Path, rec: Option<&Record>, ctx: &Context) -> Inspected {
    let Some(rec) = rec else {
        return Inspected {
            state: State::Orphan,
            last_used: mtime(dir),
            facts: Facts::default(),
        };
    };
    let last_used = rec.last_used.max(mtime(dir));
    let recent = ctx.now.saturating_sub(last_used) < ctx.live_secs;
    let mut facts = Facts {
        has_record: true,
        live: recent,
        ..Facts::default()
    };
    // A worktree is judged by its own git state; a build folder or scratch by the worktree it serves, when it names one.
    let judged: Option<PathBuf> = match rec.kind {
        Kind::Worktree => Some(dir.to_path_buf()),
        _ if !rec.link.is_empty() && Path::new(&rec.link).is_dir() => Some(PathBuf::from(&rec.link)),
        _ => None,
    };
    match judged {
        Some(wt) => {
            let linked = if wt == dir { None } else { record::read(&wt).ok().flatten() };
            let g = git_facts(&wt, linked.as_ref().unwrap_or(rec), ctx);
            facts.live |= g.live;
            facts.git_ok = g.git_ok;
            facts.dirty = g.dirty;
            facts.unpushed = g.unpushed;
            facts.merged = g.merged;
        }
        None => facts.no_git_work = rec.kind != Kind::Worktree,
    }
    Inspected {
        state: classify(&facts),
        last_used,
        facts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Facts {
        Facts {
            has_record: true,
            git_ok: true,
            dirty: Some(false),
            unpushed: Some(false),
            merged: Some(false),
            ..Facts::default()
        }
    }

    #[test]
    fn no_record_is_an_orphan_whatever_else_is_true() {
        assert_eq!(
            classify(&Facts {
                has_record: false,
                live: true,
                ..base()
            }),
            State::Orphan
        );
    }

    #[test]
    fn live_beats_every_other_state() {
        assert_eq!(
            classify(&Facts {
                live: true,
                dirty: Some(true),
                ..base()
            }),
            State::Live
        );
    }

    #[test]
    fn dirty_unpushed_or_unknown_is_unsaved() {
        for f in [
            Facts {
                dirty: Some(true),
                ..base()
            },
            Facts {
                unpushed: Some(true),
                ..base()
            },
            Facts { dirty: None, ..base() },
            Facts { unpushed: None, ..base() },
            Facts { git_ok: false, ..base() },
            Facts {
                merged: Some(true),
                dirty: Some(true),
                ..base()
            },
        ] {
            assert_eq!(classify(&f), State::Unsaved, "{f:?}");
        }
    }

    #[test]
    fn clean_and_pushed_is_merged_or_stale_pushed() {
        assert_eq!(
            classify(&Facts {
                merged: Some(true),
                ..base()
            }),
            State::Merged
        );
        assert_eq!(
            classify(&Facts {
                merged: Some(false),
                ..base()
            }),
            State::StalePushed
        );
        assert_eq!(classify(&Facts { merged: None, ..base() }), State::StalePushed);
    }

    #[test]
    fn a_build_folder_with_no_worktree_and_no_use_is_stale_pushed() {
        let f = Facts {
            has_record: true,
            no_git_work: true,
            ..Facts::default()
        };
        assert_eq!(classify(&f), State::StalePushed);
        assert_eq!(classify(&Facts { live: true, ..f }), State::Live);
    }

    #[test]
    fn only_merged_and_stale_pushed_are_reclaimable() {
        for (s, want) in [
            (State::Live, false),
            (State::Merged, true),
            (State::StalePushed, true),
            (State::Unsaved, false),
            (State::Orphan, false),
        ] {
            assert_eq!(s.reclaimable(), want);
            assert_eq!(State::parse(s.word()), Some(s));
        }
    }
}
