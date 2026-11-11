//! `dev worktree` and `dev check primary` (ADR 0083, RT-3): one tool makes, claims and removes agent worktrees, so that a
//! worktree is always locked with an owner note and bound to its agent, and no cleanup removes a locked one.
//!
//! - [`ops`]: `add`, `claim`, `reassign`, `release`, `drop`, each under the owner map's lock;
//! - [`inventory`]: `list` and `clean` (only unlocked, merged, clean, unused worktrees; `git worktree remove` without force);
//! - [`primary`]: the probe of the primary checkout;
//! - [`owners`]: the owner map `<git-common-dir>/paddock/owners.json`, which the guard hook (RT-7) reuses;
//! - [`note`], [`git`], [`procs`]: the lock note and UTC time, git and the repository, and the folders processes are in.
//!
//! The engine reads no Paddock path: the base ref and the main branch are the constants of [`primary`] and the default of
//! `--base`, so it can move to a generic tool (RT plan, "Generic code stays generic").

/// The exit code of `check primary` when its only problems are checks that could not run (5 is a found problem).
pub const EXIT_NOT_MEASURED: i32 = 6;

pub mod git;
pub mod inventory;
pub mod note;
pub mod ops;
pub mod owners;
pub mod primary;
pub mod procs;

use clap::{Args, Subcommand};
use paddock_cli::{CliError, Report};
use std::path::PathBuf;

/// `dev worktree ...`
#[derive(Subcommand)]
pub enum WorktreeCommand {
    /// Make a worktree under the worktree root: always locked, with an owner note, a full checkout (`--profile gates`), on a
    /// new branch or attached to an existing one (local or remote); refuses an existing path, one in the owner
    /// map, or one inside another checkout. Not yet printed: CARGO_TARGET_DIR and the e2e port (they come with RT-6).
    Add(AddArgs),
    /// Bind a locked worktree to the calling agent (refuses an unknown, unlocked or other-owned one).
    Claim(PathArgs),
    /// Coordinator only: clear a worktree's owner entry and keep its lock.
    Reassign(PathArgs),
    /// Unlock a worktree and forget its owner (its owner, or the coordinator).
    Release(PathArgs),
    /// Remove your own worktree, or any unlocked unowned one (after `release`), whoever calls; refuses unpushed commits and
    /// uncommitted changes.
    Drop(PathArgs),
    /// Every worktree: branch, lock note, owner, lock age, merged, dirty and in use.
    List(ListArgs),
    /// Remove worktrees that are unlocked, merged, clean and not in use (never rm, never force). Does not remove build
    /// folders yet (RT-6).
    Clean(CleanArgs),
}

/// Who is calling and where the repository is.
///
/// The agent id is **self-declared** (an argument or PADDOCK_AGENT_ID) until the guard hook (RT-7) passes the real
/// `agent_id`: these commands keep honest agents from each other's worktrees, they do not stop a caller that lies.
#[derive(Args)]
pub struct Common {
    /// The repository (default: the current folder's).
    #[arg(long, global = true)]
    repo: Option<PathBuf>,
    /// The calling agent's id, self-declared until RT-7's hook passes it (default: PADDOCK_AGENT_ID; none means the coordinator).
    #[arg(long, global = true)]
    agent: Option<String>,
    /// The calling agent's type (default: PADDOCK_AGENT_TYPE).
    #[arg(long, global = true)]
    agent_type: Option<String>,
}

/// What `add` checks out. One profile today: the enum is the place for another one (a named cone someone measured and a gate
/// reads in full), so `--profile` does not change shape when it comes; until then `--sparse` is the only other choice.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Profile {
    /// A full checkout: nothing a gate reads is missing.
    Gates,
}

/// `add`'s arguments.
#[derive(Args)]
pub struct AddArgs {
    /// The folder name (one component); the worktree is made at <root>/<name>.
    name: String,
    /// The commit-ish to start from (for example origin/main).
    #[arg(long)]
    base: String,
    /// The new branch (default: the name). Not with --detach.
    #[arg(short = 'b', long = "branch", conflicts_with = "detach")]
    branch: Option<String>,
    /// What it is for: <role>:<task>, for example implementer:RT-3.
    #[arg(long)]
    owner: String,
    /// A review copy at the base: no branch.
    #[arg(long)]
    detach: bool,
    /// What to check out: `gates` (the default) is a full checkout, everything conformance, `affected.mjs`, `gates check`
    /// and `dev plan` read. Measurements: docs/benchmarks.md, "Full against sparse worktree checkout".
    #[arg(long, value_enum, default_value = "gates", conflicts_with = "sparse")]
    profile: Profile,
    /// Sparse cone folders (comma separated). Only with --detach, for a review copy that reads a few folders. A review copy
    /// that runs the gates (conformance, `affected.mjs`, `gates check`, `dev plan`) needs no --sparse: leave it out.
    #[arg(long, value_delimiter = ',')]
    sparse: Vec<String>,
    /// The folder all agent worktrees go under (default: git config paddock.worktreeRoot, then PADDOCK_WORKTREE_ROOT).
    /// The tool refuses a path inside the primary checkout or another worktree.
    #[arg(long)]
    root: Option<PathBuf>,
    #[command(flatten)]
    common: Common,
}

/// A command that takes one worktree path.
#[derive(Args)]
pub struct PathArgs {
    /// The worktree.
    path: PathBuf,
    #[command(flatten)]
    common: Common,
}

/// `list`'s arguments.
#[derive(Args)]
pub struct ListArgs {
    /// What "merged" means.
    #[arg(long, default_value = primary::REMOTE_MAIN)]
    base: String,
    #[command(flatten)]
    common: Common,
}

/// `clean`'s arguments.
#[derive(Args)]
pub struct CleanArgs {
    /// What "merged" means.
    #[arg(long, default_value = primary::REMOTE_MAIN)]
    base: String,
    /// Say what would be removed and remove nothing.
    #[arg(long)]
    dry_run: bool,
    #[command(flatten)]
    common: Common,
}

/// `dev check primary`'s arguments.
#[derive(Args)]
pub struct PrimaryArgs {
    /// A file with one live agent_id per line (from the coordinator's own list); without it liveness is not measured.
    #[arg(long)]
    live: Option<PathBuf>,
    /// An untracked path prefix that may live in the primary (repeatable).
    #[arg(long)]
    allow_untracked: Vec<String>,
    /// The file of known untracked entries (default: paddock-primary-untracked.txt in the git directory). An untracked
    /// entry in neither it nor the allow-list is an error.
    #[arg(long)]
    baseline: Option<PathBuf>,
    /// Write the current untracked entries to the baseline (the coordinator does this after looking at them).
    #[arg(long)]
    accept_untracked: bool,
    /// The repository (default: the current folder's).
    #[arg(long)]
    repo: Option<PathBuf>,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn repo_of(repo: Option<&PathBuf>) -> Result<git::Repo, CliError> {
    let from = match repo {
        Some(r) => r.clone(),
        None => std::env::current_dir().map_err(|e| CliError::failure(format!("Cannot read the current folder: {e}.")))?,
    };
    git::Repo::discover(&from)
        .map_err(|e| CliError::failure(e.to_string()).with_hint("Run it inside the repository, or pass --repo <folder>."))
}

/// Runs a `worktree` command.
pub fn command(c: WorktreeCommand) -> Result<Report, CliError> {
    let (common, ..) = match &c {
        WorktreeCommand::Add(a) => (&a.common, ()),
        WorktreeCommand::Claim(a) | WorktreeCommand::Reassign(a) | WorktreeCommand::Release(a) | WorktreeCommand::Drop(a) => {
            (&a.common, ())
        }
        WorktreeCommand::List(a) => (&a.common, ()),
        WorktreeCommand::Clean(a) => (&a.common, ()),
    };
    let repo = repo_of(common.repo.as_ref())?;
    let caller = ops::Caller::resolve(common.agent.as_deref(), common.agent_type.as_deref(), &env)?;
    match c {
        WorktreeCommand::Add(a) => {
            match a.profile {
                // The only profile: a full checkout, which `add` makes whenever no `--sparse` is given.
                Profile::Gates => {}
            }
            let root = ops::resolve_root(&repo, a.root.as_deref(), &env)?;
            ops::add(
                &repo,
                &caller,
                &ops::AddRequest {
                    name: &a.name,
                    root: &root,
                    branch: if a.detach {
                        None
                    } else {
                        Some(a.branch.as_deref().unwrap_or(&a.name))
                    },
                    base: &a.base,
                    owner: &a.owner,
                    sparse: &a.sparse,
                },
            )
        }
        WorktreeCommand::Claim(a) => ops::claim(&repo, &caller, &a.path),
        WorktreeCommand::Reassign(a) => ops::reassign(&repo, &caller, &a.path),
        WorktreeCommand::Release(a) => ops::release(&repo, &caller, &a.path),
        WorktreeCommand::Drop(a) => ops::drop_own(&repo, &caller, &a.path),
        WorktreeCommand::List(a) => inventory::list(&repo, &a.base),
        WorktreeCommand::Clean(a) => inventory::clean(&repo, &a.base, a.dry_run),
    }
}

/// `check primary`.
pub fn check_primary(a: &PrimaryArgs) -> Result<Report, CliError> {
    let repo = repo_of(a.repo.as_ref())?;
    primary::check(
        &repo,
        &primary::Options {
            live: a.live.as_deref(),
            allow_untracked: &a.allow_untracked,
            baseline: a.baseline.as_deref(),
            accept_untracked: a.accept_untracked,
        },
    )
}
