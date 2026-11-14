//! `./dev check --pre-push` (ADR 0038 §3, docs/plans/ci-in-rust.md WP-2, cut to ER-9's fast gates): the gates CI would run for
//! what this push changes, run here first, in parallel, bounded by the machine's load (ER-24).
//!
//! - [`gates`]: the plan and the changed files become gates (pure).
//! - the conformance command comes from `conformance/paddock.ci.json` ([`crate::ci_run::contract::conformance_args_at`]): the one copy
//!   of the check list, which CI's conformance and train-gates jobs run through `./dev ci run conformance paddock` (ADR 0038 §1).
//! - [`exec`]: running them, with a fake for tests.
//!
//! What is checked. With `--refs-from-stdin` (the hook) exactly the commit being pushed: it is checked out in a throw-away worktree
//! (locked, named by the run's pid and start time, under `PADDOCK_PREPUSH_WORKTREES` or the project's worktree root, never under the
//! home folder; leftovers of ended runs are swept at the next start; builds go to a kept folder per workspace beside it)
//! and every gate runs there, so a dirty tree or another checked-out commit cannot change the answer. Without it, the working tree
//! as it is (a person's run). Only workspaces that hold a changed file themselves get Clippy and format; the ones reached through
//! dependencies are CI's (ER-9 overlap), and the Linux Clippy in the CI image is opt-in (`--linux`).
//!
//! One table at the end: what ran, what was skipped and why, what could not be measured (ER-33: never a pass), and for each
//! failure one line with its cause and the next step. Exit 0 all passed, accepted or skipped; 5 something failed; 6 only "not
//! measured" (a gate that could not run, was cut off by the 15-minute budget or by a timeout blocks the push); 7 a check is broken
//! (beats 5 and 6); 130 the person stopped it (a second Ctrl+C stops at once). The gates the table does not run but CI does are
//! listed under it ([`gates::NOT_RUN`]). One named override exists so nobody reaches for `LEFTHOOK=0`:
//! `PADDOCK_ACCEPT_NOT_MEASURED=<full row ids>` (or `--accept-not-measured`) lets those not-measured rows pass the push; it is
//! announced at the top of every run it is set in, shown in the table and logged next to the gate logs.

pub mod budget;
pub mod exec;
pub mod gates;
pub mod heavy_tool;
pub mod known;
pub mod outcome;

use crate::plan::{Request, plan_with_graph};
use crate::plan_repo::git_text;
use clap::Args;
use exec::{BUDGET, Kind, QUEUE_MAX, RealExec, Row, RunOpts, run_all_timed, workers};
use gates::{Change, Facts};
use paddock_cli::{CliError, Exit, Report};
use paddock_env::winpath;
use paddock_repo_graph::GitHistory;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The arguments of `./dev check --pre-push`.
#[derive(Args)]
pub struct PrePushArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// The base every push is measured from. Default: the nearest commit of the push's remote that the pushed commit contains (the
    /// remote ref's old commit, or a remote-tracking ref the remote still has there or later, read live with `git ls-remote`), so
    /// every commit the remote lacks is checked; origin/main when none is usable. A person's run measures HEAD the same way. Naming
    /// it also turns off the known-commit shortcut.
    #[arg(long)]
    pub base: Option<String>,
    /// The remote being pushed to (git's first pre-push argument; the hook passes it): the only remote evidence and bases are read from.
    #[arg(long)]
    pub remote: Option<String>,
    /// Only the gates whose row starts with one of these words (for example `fmt,clippy`); the hook always runs them all.
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,
    /// Print the gates and their commands; run nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Take these repository-relative files as the change instead of asking git.
    #[arg(long, value_delimiter = ',')]
    pub files: Vec<String>,
    /// Read the refs being pushed from standard input (git's pre-push format: `<local ref> <local sha> <remote ref> <remote sha>`
    /// per line) and check exactly that commit in a throw-away worktree, not the working tree. The hook passes it.
    #[arg(long)]
    pub refs_from_stdin: bool,
    /// Also run the Linux Clippy of the shared crates in the CI image (off by default: CI covers it, and it needs Docker).
    #[arg(long)]
    pub linux: bool,
    /// Let these not-measured rows (their full ids, for example `clippy-windows apps/dyno`) pass the push; each use is logged.
    /// Also read from PADDOCK_ACCEPT_NOT_MEASURED (comma separated).
    #[arg(long, value_delimiter = ',')]
    pub accept_not_measured: Vec<String>,
}

const GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// A pushed commit checked out in a folder of its own: locked while this run lives, unlocked and removed when it is dropped.
struct Detached {
    main: PathBuf,
    path: PathBuf,
}

/// Folder name of a run's worktree: `pp-<pid>-<start of that process in microseconds>-<sha>`. Pid plus start time is the identity
/// of the run (rule 53): a later process that gets the same pid has another start time, so its leftovers are never mistaken for live.
fn tree_name(pid: u32, start_micros: u64, sha: &str) -> String {
    format!("pp-{pid}-{start_micros}-{}", &sha[..sha.len().min(12)])
}

/// `(pid, start)` out of a [`tree_name`], or `None` for a folder that is not ours.
fn parse_tree_name(name: &str) -> Option<(u32, u64)> {
    let rest = name.strip_prefix("pp-")?;
    let mut parts = rest.splitn(3, '-');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// The names under `trees` whose run has ended: `alive(pid, start)` says whether that process still runs (it errs towards "yes").
fn stale_trees(names: &[String], alive: &dyn Fn(u32, u64) -> bool) -> Vec<String> {
    names
        .iter()
        .filter(|n| parse_tree_name(n).is_some_and(|(pid, start)| !alive(pid, start)))
        .cloned()
        .collect()
}

/// Whether the run that made a worktree may still be running: its pid is alive with a start time near the recorded one. A start time
/// that could not be read (0) or a process table that cannot be read counts as running: the folder is kept (rule 53).
fn run_may_be_alive(pid: u32, start_micros: u64) -> bool {
    start_micros == 0 || paddock_proc::ProcessId::new(pid, paddock_proc::StartTime::from_micros(start_micros)).may_be_alive()
}

/// Removes the throw-away worktrees of runs that were killed (a second Ctrl+C, a crash, a power cut). Only our own: the entries of
/// git's `worktrees/` admin folder whose `gitdir` points at a `pp-*` folder directly under `trees`. For each one whose run ended:
/// unlock, `git worktree remove --force`, and when its folder was already gone (so git cannot remove it) delete just that entry's admin
/// folder. Never `git worktree prune`: that drops the record of every unlocked worktree whose folder is missing, such as an agent's on a
/// drive that is not mounted.
fn sweep_stale(main: &Path, trees: &Path, alive: &dyn Fn(u32, u64) -> bool) -> usize {
    let Some(common) = git_text(main, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).map(|t| PathBuf::from(t.trim())) else {
        return 0;
    };
    let admin = common.join("worktrees");
    let mut swept = 0;
    for entry in std::fs::read_dir(&admin).into_iter().flatten().flatten() {
        let Ok(gitdir) = std::fs::read_to_string(entry.path().join("gitdir")) else {
            continue;
        };
        let wt = PathBuf::from(gitdir.trim()).parent().map(Path::to_path_buf).unwrap_or_default();
        let ours = wt
            .parent()
            .and_then(|p| winpath::real(p).ok())
            .is_some_and(|p| Some(p) == winpath::real(trees).ok())
            && wt
                .file_name()
                .is_some_and(|n| stale_trees(&[n.to_string_lossy().to_string()], alive).len() == 1);
        if !ours {
            continue;
        }
        let p = wt.to_string_lossy().to_string();
        let _ = git_text(main, &["worktree", "unlock", &p]);
        let _ = git_text(main, &["worktree", "remove", "--force", &p]);
        if !wt.exists() && entry.path().exists() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
        swept += 1;
    }
    swept
}

/// The folder pre-push worktrees and kept build folders go under: `PADDOCK_PREPUSH_WORKTREES`, else a `pre-push` folder in the
/// project's worktree root (the one `./dev worktree` uses: git config `paddock.worktreeRoot`, then `PADDOCK_WORKTREE_ROOT`). Never
/// under the home folder (ER-14, as heavy.sh refuses a build folder there).
fn work_base(main: &Path, env: &HashMap<String, String>) -> Result<PathBuf, CliError> {
    let base = if let Some(v) = env.get("PADDOCK_PREPUSH_WORKTREES").filter(|v| !v.is_empty()) {
        PathBuf::from(v)
    } else if let Some(v) = git_text(main, &["config", "--get", "paddock.worktreeRoot"])
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        PathBuf::from(v).join("pre-push")
    } else if let Some(v) = env.get("PADDOCK_WORKTREE_ROOT").filter(|v| !v.is_empty()) {
        PathBuf::from(v).join("pre-push")
    } else {
        return Err(CliError::failure("There is no folder for the pre-push worktrees.").with_hint(
            "Set PADDOCK_PREPUSH_WORKTREES, or `git config paddock.worktreeRoot <folder>` once (the root `./dev worktree` uses).",
        ));
    };
    refuse_home(&base, env.get("HOME").map(String::as_str))?;
    Ok(base)
}

/// A folder inside the home folder is refused: builds and checkouts do not go on the internal disk (ER-14). A relative path and a
/// literal `~` are refused (they depend on where the tool runs); the folder is created, then it and `home` are resolved (symlinks
/// followed) before they are compared, so a link into home cannot hide it.
fn refuse_home(path: &Path, home: Option<&str>) -> Result<(), CliError> {
    let hint = "Use a work disk: set PADDOCK_PREPUSH_WORKTREES (or paddock.worktreeRoot) to an absolute folder outside your home.";
    if !path.is_absolute() || path.to_string_lossy().starts_with('~') {
        return Err(CliError::refused(format!("{} is not an absolute path.", path.display())).with_hint(hint));
    }
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(CliError::refused(format!("{} contains `..`.", path.display())).with_hint(hint));
    }
    // Resolve the nearest ancestor that exists (symlinks followed) and put the rest of the path on it: the check runs before
    // anything is created, so a refused folder is never made.
    let ancestor = path.ancestors().find(|a| a.exists()).unwrap_or(path);
    let rest = path.strip_prefix(ancestor).unwrap_or(Path::new(""));
    let resolved = winpath::real(ancestor)
        .map_err(|e| CliError::failure(format!("{} cannot be resolved: {e}.", ancestor.display())))?
        .join(rest);
    if let Some(h) = home.filter(|h| !h.is_empty() && *h != "/") {
        let home_resolved = winpath::real(Path::new(h)).unwrap_or_else(|_| PathBuf::from(h));
        if resolved.starts_with(&home_resolved) {
            return Err(CliError::refused(format!(
                "{} is inside the home folder {}.",
                resolved.display(),
                home_resolved.display()
            ))
            .with_hint(hint));
        }
    }
    std::fs::create_dir_all(path).map_err(|e| CliError::failure(format!("{} cannot be created: {e}.", path.display())))?;
    Ok(())
}

impl Detached {
    /// `git worktree add --detach --lock` of `sha` under `trees`, with the main checkout's `node_modules` linked in (conformance needs it).
    fn new(main: &Path, trees: &Path, sha: &str) -> Result<Detached, CliError> {
        let pid = std::process::id();
        let start = paddock_proc::ProcessId::of(pid).map_or(0, |p| p.started().as_micros());
        let path = trees.join(tree_name(pid, start, sha));
        std::fs::create_dir_all(trees).map_err(|e| CliError::failure(format!("{} cannot be created: {e}.", trees.display())))?;
        let p = path.to_string_lossy().to_string();
        let reason = format!("pre-push {pid}");
        git_text(main, &["worktree", "add", "--detach", "--lock", "--reason", &reason, &p, sha]).ok_or_else(|| {
            CliError::failure(format!("The pushed commit {sha} could not be checked out in {p}."))
                .with_hint("Run `git worktree add --detach <folder> <sha>` by hand to see why.")
        })?;
        let this = Detached {
            main: main.to_path_buf(),
            path,
        };
        #[cfg(unix)]
        {
            let nm = main.join("node_modules");
            if nm.exists() {
                let _ = std::os::unix::fs::symlink(&nm, this.path.join("node_modules"));
            }
        }
        Ok(this)
    }
}

impl Drop for Detached {
    fn drop(&mut self) {
        let p = self.path.to_string_lossy().to_string();
        let _ = git_text(&self.main, &["worktree", "unlock", &p]);
        let _ = git_text(&self.main, &["worktree", "remove", "--force", &p]);
    }
}

/// The lookups that leave the machine: git's `ls-remote` and the shared CI verdict reader.
struct RealLookups {
    root: PathBuf,
}

impl known::Lookups for RealLookups {
    fn ls_remote(&self, remote: &str) -> Option<String> {
        git_text(&self.root, &["ls-remote", "--", remote])
    }
    fn ci_green(&self, sha: &str) -> Result<u64, String> {
        match crate::ci_runs::ci_verdict(known::CI_WORKFLOW, sha) {
            crate::ci_runs::verdict::Verdict::Green { run } => Ok(run),
            crate::ci_runs::verdict::Verdict::Red { reason, .. } => Err(format!("red or partial: {reason}")),
            crate::ci_runs::verdict::Verdict::NotMeasured(why) => Err(format!("not measured: {why}")),
        }
    }
}

/// The command.
pub fn command(args: &PrePushArgs) -> Result<Report, CliError> {
    let root = match &args.root {
        Some(r) => r.clone(),
        None => {
            let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
            git_text(&cwd, &["rev-parse", "--show-toplevel"])
                .map(|t| PathBuf::from(t.trim()))
                .ok_or_else(|| CliError::failure("This is not a git checkout.").with_hint("Run it inside the repository."))?
        }
    };
    let stdin = if args.refs_from_stdin { read_stdin()? } else { String::new() };
    command_in(args, &root, &stdin, &RealLookups { root: root.clone() })
}

/// The lookups of one run, `git ls-remote` read once per remote: the base choice and the known-commit lookup ask the same question.
struct OnceLookups<'a> {
    inner: &'a dyn known::Lookups,
    listings: std::sync::Mutex<HashMap<String, Option<String>>>,
}

impl known::Lookups for OnceLookups<'_> {
    fn ls_remote(&self, remote: &str) -> Option<String> {
        if let Ok(m) = self.listings.lock()
            && let Some(l) = m.get(remote)
        {
            return l.clone();
        }
        let l = self.inner.ls_remote(remote);
        if let Ok(mut m) = self.listings.lock() {
            m.insert(remote.to_string(), l.clone());
        }
        l
    }
    fn ci_green(&self, sha: &str) -> Result<u64, String> {
        self.inner.ci_green(sha)
    }
}

/// [`command`] with the repository, git's pre-push lines and the outside lookups given.
fn command_in(args: &PrePushArgs, root: &Path, stdin: &str, look: &dyn known::Lookups) -> Result<Report, CliError> {
    let look = &OnceLookups {
        inner: look,
        listings: Default::default(),
    };
    let root = root.to_path_buf();
    let env: HashMap<String, String> = std::env::vars().collect();
    let accept = accepted_ids(&args.accept_not_measured, &env);
    let stop = Arc::new(AtomicBool::new(false));
    stop_on_signals(&stop);
    let state = state_dir(&root);
    let pushes = if args.refs_from_stdin { parse_push(stdin) } else { Vec::new() };
    let pushed: Vec<String> = pushes.iter().map(|p| p.sha.clone()).collect();
    // Every pushed commit's base comes first: a pass record counts only when it was measured from that base or an older one.
    let bases: Vec<(String, String)> = match &args.base {
        Some(b) => vec![(b.clone(), format!("--base {b} was given"))],
        None if pushes.is_empty() => vec![person_base(&root, args.remote.as_deref(), DEFAULT_BASE, look)],
        None => pushes
            .iter()
            .map(|p| choose_base(&root, p, args.remote.as_deref(), DEFAULT_BASE, look))
            .collect(),
    };
    let (base, base_why) = bases[0].clone();
    let mut lookup_notes = Vec::new();
    if known::applicable(args, &pushed) {
        let measured: Vec<&str> = bases.iter().map(|(b, _)| b.as_str()).collect();
        let (found, trail) = known::find(&root, &state, args.remote.as_deref(), &pushed, &measured, look);
        log_known(&state, &pushed, args.remote.as_deref(), &trail);
        if let Some(ev) = found {
            return Ok(known::report(&pushed, args.remote.as_deref(), &ev));
        }
        lookup_notes = trail;
    }
    let (change, mut notes, extra) = discover(&root, &base, &args.files, &pushed)?;
    notes.insert(0, format!("base: {base} ({base_why})"));
    notes.extend(lookup_notes.into_iter().map(|l| format!("known-commit lookup: {l}")));
    // An empty diff passes on its own only for a person's run with no pushed commit; a push is always measured (or known).
    if pushed.is_empty() && change.files.is_empty() && change.ranges.is_empty() && extra.is_empty() {
        return Ok(Report::new(
            json!({"gates": [], "changed": 0, "base": base, "outcome": outcome::nothing_to_check(&base).json()}),
            format!("pre-push: base {base} ({base_why}); nothing changed since; nothing to check."),
        ));
    }
    if !accept.is_empty() {
        notes.push(format!(
            "OVERRIDE SET: PADDOCK_ACCEPT_NOT_MEASURED / --accept-not-measured names {}; a not-measured row with exactly one of these ids passes the push, and each use is logged in {}",
            accept.join(", "),
            state.join("overrides.log").display()
        ));
    }
    // Where the gates run: the throw-away worktree of the pushed commit (unless that commit is HEAD of a clean tree, whose
    // content is the same), or the working tree for a person's run.
    let head_sha = git_text(&root, &["rev-parse", "HEAD"]).map(|t| t.trim().to_string());
    let clean = git_text(&root, &["status", "--porcelain"]).is_some_and(|t| t.trim().is_empty());
    let _checkout;
    let mut kept_targets = None;
    let work = match pushed.first() {
        Some(sha) if head_sha.as_deref() == Some(sha.as_str()) && clean => {
            notes.push(format!(
                "checking the pushed commit {} (it is HEAD of a clean tree)",
                &sha[..sha.len().min(12)]
            ));
            root.clone()
        }
        Some(sha) => {
            let base = work_base(&root, &env)?;
            let trees = base.join("trees");
            let swept = sweep_stale(&root, &trees, &run_may_be_alive);
            if swept > 0 {
                notes.push(format!("removed {swept} leftover pre-push worktree(s) of runs that ended"));
            }
            let c = Detached::new(&root, &trees, sha)?;
            notes.push(format!(
                "checking the pushed commit {} in a throw-away worktree ({}), removed afterwards; builds go to {}",
                &sha[..sha.len().min(12)],
                c.path.display(),
                base.join("targets").display()
            ));
            kept_targets = Some(base.join("targets").to_string_lossy().to_string());
            let p = c.path.clone();
            _checkout = c;
            p
        }
        None => {
            notes.push("checking the working tree as it is, not a pushed commit (the hook checks the commit being pushed)".into());
            root.clone()
        }
    };
    let real = RealExec {
        root: work.clone(),
        env: env.clone(),
        logs: state.join("logs"),
        stop: Arc::clone(&stop),
        built_heavy: Default::default(),
        budget_end: std::time::Instant::now() + BUDGET,
        priority: pushes.iter().any(|p| budget::is_landing_ref(&p.remote_ref)),
    };
    let queue = queue_max(&env);
    let history = GitHistory::new(&work, GIT_TIMEOUT);
    let req = Request {
        files: change.files.clone(),
        all: false,
        explain: false,
        root: None,
        base: None,
        head: "HEAD".into(),
    };
    let (_, plan) = plan_with_graph(&work, &history, &req).map_err(|e| {
        CliError::failure(format!("The planner could not plan this change: {e}."))
            .with_hint("Run `./dev plan --files <a,b>` to see the same error.")
    })?;
    let mut facts = read_facts(&work, &plan, &env);
    // A dry run lists and runs nothing, so it needs no install; a real run in a throw-away tree on Windows has no node_modules.
    facts.conformance_env = if args.dry_run {
        None
    } else {
        node_env_problem(cfg!(windows) && work != root, &|p| std::fs::read_to_string(work.join(p)).ok())
    };
    facts.linux = args.linux;
    facts.kept_targets = kept_targets;
    let mut planned = gates::plan_gates(&plan, &change, &facts);
    if !args.only.is_empty() {
        planned.retain(|p| {
            let id = match p {
                gates::Planned::Run(g) => &g.id,
                gates::Planned::Skip { id, .. } | gates::Planned::NotMeasured { id, .. } | gates::Planned::Broken { id, .. } => id,
            };
            args.only.iter().any(|w| id.split(' ').next() == Some(w.as_str()))
        });
    }
    if args.dry_run {
        return Ok(dry_run(&planned));
    }
    let load = exec::Exec::load(&real);
    let n = workers(load, std::thread::available_parallelism().map_or(1, |n| n.get()));
    let mut overridden = String::new();
    if env.get("PADDOCK_HEAVY").map(|v| v.trim()) == Some("off") && !env.contains_key(exec::TEST_HEAVY_OFF) {
        overridden =
            "OVERRIDE SET: PADDOCK_HEAVY=off is not allowed outside the tests; every compile (clippy, xwin, Linux) is not measured\n"
                .into();
    } else if exec::heavy_prefix(&env, &|p| exec::Exec::has_tool(&real, p)).is_empty() {
        notes.push("no heavy.sh or heavy on PATH: the compiles (clippy, xwin, Linux) run through the heavy built from tools/heavy (cached under ~/.paddock-dev/bin); if that build fails they are not measured (ER-24)".into());
    }
    let head = format!(
        "{overridden}pre-push: base {} ({}); {} changed file(s); {} gate(s) at a time (1-minute load {}); budget {} min of run time (time queued for the heavy slot behind other agents is reported apart and bounded on its own: {} min for the whole push; {}); logs in {}\n{}",
        base,
        base_why,
        change.files.len(),
        n,
        load.map_or("unknown".to_string(), |l| format!("{l:.1}")),
        BUDGET.as_secs() / 60,
        queue.as_secs() / 60,
        if real.priority {
            "priority lane: this push lands on a train, main or a release tag"
        } else {
            "normal lane"
        },
        real.logs.display(),
        notes.iter().map(|n| format!("pre-push: {n}")).collect::<Vec<_>>().join("\n")
    );
    // What the run is about (the commit and its tree) is the evidence of every pass, so it is known before the first gate runs.
    let checked = pushed.first().cloned().or_else(|| if clean { head_sha.clone() } else { None });
    let subject = outcome::subject_of(&root, checked.as_deref());
    let (mut rows, timings) = run_all_timed(
        &planned,
        &real,
        &RunOpts {
            workers: n,
            budget: BUDGET,
            queue,
            stop: &stop,
            subject: &subject,
        },
    );
    rows.extend(extra_ref_rows(&extra));
    accept_not_measured(&mut rows, &accept, &state);
    log_queue_times(&state, checked.as_deref(), &base, &timings);
    let mut report = render_timed(head.trim_end(), &rows, &timings);
    if stop.load(Ordering::SeqCst) {
        report = report.with_exit(Exit::Interrupted);
    }
    // A complete passing run is the evidence a later push of the same commit may rest on (known::find).
    // The run's own outcome, with the commit and tree it is about: what a consumer reads (Outcome::from_wire), never the exit code.
    let (shared, run_outcome) = outcome::fold(&rows, &subject);
    debug_assert!(
        report.exit == Exit::Interrupted || report.exit == shared.exit(),
        "the table's exit and the shared verdict's exit disagree"
    );
    report.data["outcome"] = run_outcome.json();
    report.data["outcomes"] = shared.json();
    if let Some(sha) =
        checked.filter(|_| known::recordable(args, accept.len(), extra.len(), report.exit) && outcome::leaves_record(&run_outcome))
    {
        let passed: Vec<String> = rows
            .iter()
            .filter(|(_, v)| v.kind() == Kind::Passed)
            .map(|(id, _)| id.clone())
            .collect();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if let Err(e) = known::write_record(&root, &state, &sha, &base, &passed, now, &run_outcome) {
            eprintln!("::warning::pre-push: the pass record of {sha} was not written: {e}. A later push of it is checked again.");
        }
    }
    Ok(report)
}

/// One line per run in `queue-times.log` beside the gate logs: the whole push's heavy queue and run time, the measurement the
/// per-push queue allowance ([`QUEUE_MAX`]) is to be revisited with (its basis so far is per step, ADR 0102 amendment).
fn log_queue_times(state: &Path, sha: Option<&str>, base: &str, timings: &std::collections::BTreeMap<String, budget::Timing>) {
    use std::io::Write;
    let queued: f64 = timings.values().map(|t| t.queued).sum();
    let ran: f64 = timings.values().map(|t| t.ran).sum();
    let line = format!(
        "{} sha={} base={} heavy_steps={} queued_s={queued:.1} ran_s={ran:.1}\n",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        sha.unwrap_or("worktree"),
        base,
        timings.len()
    );
    let _ = std::fs::create_dir_all(state);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join("queue-times.log"))
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// The push's queue allowance (all its heavy steps' waits for the slot together): `PADDOCK_QUEUE_MAX` (seconds, at least 1) or
/// [`QUEUE_MAX`].
fn queue_max(env: &HashMap<String, String>) -> Duration {
    env.get("PADDOCK_QUEUE_MAX")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s >= 1)
        .map_or(QUEUE_MAX, Duration::from_secs)
}

/// One not-measured row per pushed commit beyond the first: a run checks a single commit, so the others are never passed.
fn extra_ref_rows(extra: &[String]) -> Vec<(String, Row)> {
    extra
        .iter()
        .map(|sha| {
            (
                format!("pushed commit {}", &sha[..sha.len().min(12)]),
                Row::not_measured(
                    "a run checks one pushed commit and this push has more than one ref",
                    "push the refs one at a time (`git push origin <ref>`), so each is checked",
                ),
            )
        })
        .collect()
}

/// The override's ids: the option and the environment variable, comma separated, trimmed.
fn accepted_ids(option: &[String], env: &HashMap<String, String>) -> Vec<String> {
    let from_env = env
        .get("PADDOCK_ACCEPT_NOT_MEASURED")
        .map(|v| v.split(',').map(|w| w.trim().to_string()).collect::<Vec<_>>())
        .unwrap_or_default();
    option.iter().cloned().chain(from_env).filter(|w| !w.is_empty()).collect()
}

/// The exit code of a second Ctrl+C or SIGTERM: 128 + SIGINT.
#[cfg(unix)]
const SECOND_SIGNAL_EXIT: i32 = 130;

#[cfg(unix)]
pub(crate) fn stop_on_signals(stop: &Arc<AtomicBool>) {
    use signal_hook::consts::{SIGINT, SIGTERM};
    for sig in [SIGINT, SIGTERM] {
        // Registered first, so it sees the flag the second signal finds set: the first signal only sets the flag (the gates end,
        // the table prints), the second one leaves at once.
        let _ = signal_hook::flag::register_conditional_shutdown(sig, SECOND_SIGNAL_EXIT, Arc::clone(stop));
        let _ = signal_hook::flag::register(sig, Arc::clone(stop));
    }
}

/// Windows: Ctrl+C reaches the whole console group; the handler sets the same flag so the table prints and the gates end.
#[cfg(windows)]
pub(crate) fn stop_on_signals(stop: &Arc<AtomicBool>) {
    let flag = Arc::clone(stop);
    let _ = ctrlc::set_handler(move || {
        if flag.swap(true, Ordering::SeqCst) {
            std::process::exit(130);
        }
    });
}

/// The folder for this checkout's gate logs and override log: `<git common dir>/paddock-pre-push`.
fn state_dir(root: &Path) -> PathBuf {
    git_text(root, &["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .map(|t| PathBuf::from(t.trim()))
        .unwrap_or_else(|| root.join(".git"))
        .join("paddock-pre-push")
}

/// The most of git's pre-push input that is read (a push of thousands of refs is not a pre-push gate's case).
const STDIN_LIMIT: u64 = 1 << 20;

/// Standard input, for `--refs-from-stdin`: refused when it is a terminal (nothing would ever end it) and bounded.
fn read_stdin() -> Result<String, CliError> {
    use std::io::{IsTerminal, Read};
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(
            CliError::usage("--refs-from-stdin reads git's pre-push lines, and standard input is a terminal.")
                .with_hint("Let git run it (the pre-push hook passes the lines), or leave the flag out to check the working tree."),
        );
    }
    let mut text = String::new();
    stdin
        .lock()
        .take(STDIN_LIMIT)
        .read_to_string(&mut text)
        .map_err(|e| CliError::failure(format!("Standard input could not be read: {e}.")))?;
    Ok(text)
}

/// The base a push is measured from when `--base` is not given.
const DEFAULT_BASE: &str = "origin/main";

/// One ref of a push, from a line of git's pre-push input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pushed {
    /// The local ref (`refs/heads/x`).
    pub local_ref: String,
    /// The commit being pushed.
    pub sha: String,
    /// The ref on the remote.
    pub remote_ref: String,
    /// The commit the remote ref has now: all zeros for a new ref.
    pub remote_sha: String,
}

/// The refs of a push that send a commit (deletions are left out), once per commit.
pub fn parse_push(input: &str) -> Vec<Pushed> {
    let mut out: Vec<Pushed> = Vec::new();
    for l in input.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if let [local_ref, sha, remote_ref, remote_sha] = f[..]
            && sha.chars().any(|c| c != '0')
            && !out.iter().any(|p| p.sha == sha)
        {
            out.push(Pushed {
                local_ref: local_ref.into(),
                sha: sha.into(),
                remote_ref: remote_ref.into(),
                remote_sha: remote_sha.into(),
            });
        }
    }
    out
}

/// Where a push is measured from, and why (printed at the top of the run, ER-27): one definition, as git's pre-push.sample has it.
/// The commits a push must check are the ones the remote lacks; every commit of the remote that the pushed commit contains bounds
/// them, so the base is the nearest such commit ([`nearest_base`]): the remote ref's old commit (git's own pre-push line says the
/// remote has it) or a remote-tracking ref of the push's remote that the pushed commit contains and the remote still has (read live
/// with `git ls-remote`). `fallback` (origin/main) when none is. The base comes back as a commit id, resolved once, so the diff, the
/// known-commit lookup and the pass record all use the same commit. An empty diff is never a pass on its own: a candidate at the
/// pushed commit itself is skipped.
pub fn choose_base(root: &Path, p: &Pushed, remote: Option<&str>, fallback: &str, look: &dyn known::Lookups) -> (String, String) {
    choose_base_with(root, p, remote, fallback, look, false)
}

/// The base of a person's run (no push to look at): HEAD measured as a push of the current branch to `remote` (default origin) would
/// be, so the run ahead of a push plans what the push will. A candidate at HEAD itself is fine here (the run checks the working
/// tree's uncommitted changes, and an empty diff is "nothing to check").
pub fn person_base(root: &Path, remote: Option<&str>, fallback: &str, look: &dyn known::Lookups) -> (String, String) {
    let remote = remote.filter(|r| !r.is_empty() && !r.starts_with('-')).unwrap_or("origin");
    let Some(head) = git_text(root, &["rev-parse", "--verify", "--quiet", "HEAD"]).map(|t| t.trim().to_string()) else {
        let (b, w) = resolved(root, fallback, "a person's run with no HEAD commit".into());
        return (b, w);
    };
    let branch = git_text(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]).map(|t| t.trim().to_string());
    let name = branch.as_deref().map_or_else(|| "HEAD".to_string(), |b| format!("refs/heads/{b}"));
    let p = Pushed {
        local_ref: name.clone(),
        sha: head,
        remote_ref: name,
        remote_sha: "0".repeat(40),
    };
    let (base, why) = choose_base_with(root, &p, Some(remote), fallback, look, true);
    let what = branch.map_or_else(|| "a detached HEAD".to_string(), |b| format!("branch {b}"));
    (base, format!("a person's run: HEAD of {what} measured as its push would be; {why}"))
}

/// `fallback` as a commit id when it names one here (else the name itself, and `discover` says it cannot be read), with `why`.
fn resolved(root: &Path, fallback: &str, why: String) -> (String, String) {
    let sha = git_text(root, &["rev-parse", "--verify", "--quiet", &format!("{fallback}^{{commit}}")]).map(|t| t.trim().to_string());
    match sha {
        Some(s) => (s.clone(), format!("{why}; measuring from {fallback} ({})", &s[..s.len().min(12)])),
        None => (
            fallback.to_string(),
            format!("{why}; measuring from {fallback}, which is not a commit here"),
        ),
    }
}

fn choose_base_with(
    root: &Path,
    p: &Pushed,
    remote: Option<&str>,
    fallback: &str,
    look: &dyn known::Lookups,
    empty_ok: bool,
) -> (String, String) {
    let short = |s: &str| s[..s.len().min(12)].to_string();
    let mut prefix = String::new();
    let mut old = None;
    if p.remote_sha.chars().any(|c| c != '0') {
        if git_text(root, &["cat-file", "-e", &format!("{}^{{commit}}", p.remote_sha)]).is_some() {
            old = Some(p.remote_sha.clone());
        } else {
            prefix = format!(
                "{} is on the remote at {}, which is not here (fetch it); ",
                p.remote_ref,
                short(&p.remote_sha)
            );
        }
    }
    let (b, w) = nearest_base(root, p, remote, fallback, look, old.as_deref(), empty_ok);
    (b, format!("{prefix}{w}"))
}

/// One base candidate: how many of the pushed commit's commits it leaves new, its tie-break rank (0 the old commit, 1 the tracking
/// upstream, 2 any other), what it is (for the log) and its commit.
struct Candidate {
    dist: u32,
    rank: u8,
    label: String,
    sha: String,
}

/// The nearest commit of the remote that the pushed commit contains. The candidates are the remote ref's old commit (when the
/// pushed commit contains it: a force push does not) and the remote-tracking refs of `remote` the pushed commit contains (`git
/// for-each-ref --merged`), each only when the live listing still has that branch at that commit or a later one (a ref the remote
/// deleted or force-pushed since the last fetch is stale and its commits may be gone from the remote); when the remote cannot be
/// read, only the remote-tracking refs that the remote-tracking main or the upstream holds (a deleted branch's ref is not), said
/// so. Of the candidates only the ones no other candidate contains can be nearest
/// (`git merge-base --independent`, one process however many branches the remote has); of those the fewest new commits wins, on a
/// tie the lower rank. A candidate at the pushed commit itself is skipped unless `empty_ok` (a person's run).
fn nearest_base(
    root: &Path,
    p: &Pushed,
    remote: Option<&str>,
    fallback: &str,
    look: &dyn known::Lookups,
    old: Option<&str>,
    empty_ok: bool,
) -> (String, String) {
    let started = std::time::Instant::now();
    let git = |args: &[&str]| git_text(root, args);
    let short = |s: &str| s[..s.len().min(12)].to_string();
    let named = remote.filter(|r| !r.is_empty() && !r.starts_with('-'));
    let tracking_of = named.unwrap_or("origin");
    let mut notes: Vec<String> = Vec::new();
    let listing = match named {
        None => {
            notes.push(format!(
                "no remote was named for this push, so the remote-tracking refs of {tracking_of} are not checked live"
            ));
            None
        }
        Some(r) => {
            let l = look.ls_remote(r);
            if l.is_none() {
                notes.push(format!(
                    "git ls-remote {r} could not be read, so the remote-tracking refs are not checked live"
                ));
            }
            l
        }
    };
    let live: HashMap<String, String> = listing
        .as_deref()
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .filter_map(|(sha, n)| {
            n.trim()
                .strip_prefix("refs/heads/")
                .map(|n| (n.to_string(), sha.trim().to_string()))
        })
        .collect();
    let upstream = p
        .local_ref
        .strip_prefix("refs/heads/")
        .and_then(|b| git(&["rev-parse", "--symbolic-full-name", &format!("{b}@{{upstream}}")]))
        .and_then(|t| t.trim().strip_prefix(&format!("refs/remotes/{tracking_of}/")).map(str::to_string));
    // (sha, rank, label) of every usable candidate.
    let mut usable: Vec<(String, u8, String)> = Vec::new();
    let mut stale: Vec<String> = Vec::new();
    if let Some(sha) = old {
        if git(&["merge-base", "--is-ancestor", sha, &p.sha]).is_some() {
            usable.push((
                sha.to_string(),
                0,
                format!("{} on the remote: its old commit {}", p.remote_ref, short(sha)),
            ));
        } else {
            notes.push(format!(
                "{} is on the remote at {}, which the pushed commit does not contain (a force push)",
                p.remote_ref,
                short(sha)
            ));
        }
    }
    // What a ref that cannot be checked live must be held by: the remote-tracking main and the upstream.
    let anchors: Vec<String> = ["main".to_string()]
        .into_iter()
        .chain(upstream.clone())
        .filter_map(|b| {
            git(&[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/remotes/{tracking_of}/{b}^{{commit}}"),
            ])
        })
        .map(|t| t.trim().to_string())
        .collect();
    let merged = format!("--merged={}", p.sha);
    let prefix = format!("refs/remotes/{tracking_of}/");
    for line in git(&["for-each-ref", &merged, "--format=%(objectname) %(refname:strip=3)", &prefix])
        .unwrap_or_default()
        .lines()
    {
        let Some((sha, name)) = line.split_once(' ') else { continue };
        if name == "HEAD" {
            continue;
        }
        let verified = match (&listing, live.get(name)) {
            // Not checked live: only a commit main or the upstream holds is surely still on the remote (a deleted branch's ref
            // is not), so only those count.
            (None, _) if anchors.iter().any(|a| git(&["merge-base", "--is-ancestor", sha, a]).is_some()) => {
                Some("remote-tracking ref, not checked live; main or the upstream holds it")
            }
            (None, _) => None,
            (Some(_), Some(now)) if now == sha => Some("remote-tracking ref, the remote has it there (live)"),
            (Some(_), Some(now))
                if git(&["cat-file", "-e", &format!("{now}^{{commit}}")]).is_some()
                    && git(&["merge-base", "--is-ancestor", sha, now]).is_some() =>
            {
                Some("remote-tracking ref, the remote has moved on from it (live)")
            }
            _ => None,
        };
        let Some(how) = verified else {
            stale.push(name.to_string());
            continue;
        };
        let rank = if upstream.as_deref() == Some(name) { 1 } else { 2 };
        let up = if rank == 1 { ", its tracking upstream" } else { "" };
        usable.push((sha.to_string(), rank, format!("{tracking_of}/{name} ({how}{up})")));
    }
    if !stale.is_empty() {
        stale.sort();
        notes.push(format!(
            "skipped {} remote-tracking ref(s) the remote may no longer have at that commit or later (deleted or force-pushed since the last fetch, or not checkable live and held by neither main nor the upstream): {}",
            stale.len(),
            stale.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if !empty_ok && usable.iter().any(|(sha, ..)| *sha == p.sha) {
        let at: Vec<String> = usable.iter().filter(|(sha, ..)| *sha == p.sha).map(|(_, _, l)| l.clone()).collect();
        usable.retain(|(sha, ..)| *sha != p.sha);
        notes.push(format!(
            "the pushed commit is already on {} but no evidence for it was found, so the nearest base that leaves something new is used",
            at.join(", ")
        ));
    }
    let mut shas: Vec<&str> = usable.iter().map(|(s, ..)| s.as_str()).collect();
    shas.sort_unstable();
    shas.dedup();
    let independent: Vec<String> = if shas.len() > 1 {
        let mut args = vec!["merge-base", "--independent"];
        args.extend(shas.iter().copied());
        git(&args).map_or_else(
            || shas.iter().map(|s| s.to_string()).collect(),
            |t| t.lines().map(|l| l.trim().to_string()).collect(),
        )
    } else {
        shas.iter().map(|s| s.to_string()).collect()
    };
    let mut cands: Vec<Candidate> = Vec::new();
    for sha in &independent {
        let Some(dist) = git(&["rev-list", "--count", &format!("{sha}..{}", p.sha)]).and_then(|t| t.trim().parse::<u32>().ok()) else {
            continue;
        };
        if let Some((_, rank, label)) = usable
            .iter()
            .filter(|(s, ..)| s == sha)
            .min_by(|a, b| (a.1, &a.2).cmp(&(b.1, &b.2)))
        {
            cands.push(Candidate {
                dist,
                rank: *rank,
                label: label.clone(),
                sha: sha.clone(),
            });
        }
    }
    cands.sort_by_key(|c| (c.dist, c.rank));
    let others: Vec<String> = cands.iter().skip(1).take(2).map(|c| format!("{} {}", c.label, c.dist)).collect();
    let took = format!(
        "{} candidate(s) of {tracking_of} the pushed commit contains, {} not contained by another, counted in {} ms{}",
        usable.len(),
        cands.len(),
        started.elapsed().as_millis(),
        if others.is_empty() {
            String::new()
        } else {
            format!("; next nearest: {}", others.join(", "))
        }
    );
    let notes = if notes.is_empty() {
        String::new()
    } else {
        format!("{}; ", notes.join("; "))
    };
    match cands.first() {
        Some(c) => (
            c.sha.clone(),
            format!("{notes}{} leaves the fewest new commits, {} ({took})", c.label, c.dist),
        ),
        None => resolved(
            root,
            fallback,
            format!("{notes}no commit of {tracking_of} that the pushed commit contains is usable ({took})"),
        ),
    }
}

/// The commits a push sends, from git's pre-push input: the local sha of every ref that is not a deletion.
pub fn parse_refs(input: &str) -> Vec<String> {
    let mut shas: Vec<String> = Vec::new();
    for sha in input
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .filter(|sha| sha.chars().any(|c| c != '0'))
    {
        if !shas.iter().any(|s| s == sha) {
            shas.push(sha.to_string());
        }
    }
    shas
}

/// Turns the not-measured rows a person named (by full row id) into accepted ones and logs each use: when, who, which row and why.
pub fn accept_not_measured(rows: &mut [(String, Row)], accept: &[String], state: &Path) {
    for (id, v) in rows.iter_mut() {
        if v.kind() != Kind::NotMeasured {
            continue;
        }
        let reason = v.reason();
        if !accept.iter().any(|a| a == id) {
            continue;
        }
        let line = format!(
            "{} user={} gate={id} reason={reason}\n",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            std::env::var("USER").unwrap_or_default()
        );
        let _ = std::fs::create_dir_all(state);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(state.join("overrides.log"))
        {
            let _ = std::io::Write::write_all(&mut f, line.as_bytes());
        }
        v.excused_by = Some(std::env::var("USER").unwrap_or_else(|_| "a person".into()));
    }
}

fn dry_run(planned: &[gates::Planned]) -> Report {
    let mut text = String::new();
    let mut data = Vec::new();
    for p in planned {
        match p {
            gates::Planned::Run(g) => {
                text.push_str(&format!("run   {:<34} {}\n", g.id, g.rerun()));
                data.push(json!({"id": g.id, "action": "run", "command": g.rerun(), "heavy": g.heavy}));
            }
            gates::Planned::Skip { id, reason } => {
                text.push_str(&format!("skip  {id:<34} {reason}\n"));
                data.push(json!({"id": id, "action": "skip", "reason": reason}));
            }
            gates::Planned::NotMeasured { id, reason, .. } => {
                text.push_str(&format!("NOT MEASURED {id:<31} {reason}\n"));
                data.push(json!({"id": id, "action": "not-measured", "reason": reason}));
            }
            gates::Planned::Broken { id, cause, .. } => {
                text.push_str(&format!("BROKEN {id:<33} {cause}\n"));
                data.push(json!({"id": id, "action": "broken", "reason": cause}));
            }
        }
    }
    Report::new(json!({ "dry_run": true, "gates": data }), text)
}

/// The table, one line per row, then what CI runs that this gate does not, and the exit code (see the module doc). A failure's line
/// carries its cause and next step.
pub fn render(head: &str, rows: &[(String, Row)]) -> Report {
    render_timed(head, rows, &std::collections::BTreeMap::new())
}

/// [`render`] with the queue and run time of the rows that went through `heavy`: a pass or a failure shows both, apart (ER-27).
pub fn render_timed(head: &str, rows: &[(String, Row)], timings: &std::collections::BTreeMap<String, budget::Timing>) -> Report {
    let both = |id: &str, secs: f64| match timings.get(id) {
        Some(t) => format!("{:.1} s run, {:.1} s queued", t.ran, t.queued),
        None => format!("{secs:.1} s"),
    };
    let mut text = format!("{head}\n");
    let (mut ok, mut fail, mut skip, mut unmeasured, mut broken, mut accepted) = (0, 0, 0, 0, 0, 0);
    let mut data = Vec::new();
    let (mut unmeasured_ids, mut failed_ids): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    for (id, v) in rows {
        let reason = v.reason();
        let (line, kind) = match v.kind() {
            Kind::Passed => {
                ok += 1;
                (format!("ok            {id:<34} {}", both(id, v.secs)), "passed")
            }
            Kind::Skipped => {
                skip += 1;
                (format!("skip          {id:<34} {reason}"), "skipped")
            }
            Kind::NotMeasured => {
                unmeasured += 1;
                unmeasured_ids.push(id);
                (format!("NOT MEASURED  {id:<34} {reason}. Next: {}", v.next), "not measured")
            }
            Kind::Accepted => {
                accepted += 1;
                (
                    format!("ACCEPTED      {id:<34} not measured ({reason}); named by its full id in PADDOCK_ACCEPT_NOT_MEASURED, logged"),
                    "accepted",
                )
            }
            Kind::Failed => {
                fail += 1;
                failed_ids.push(id);
                let shown = if v.tail.is_empty() {
                    String::new()
                } else {
                    format!("\n    {}", v.tail.replace('\n', "\n    "))
                };
                (
                    format!(
                        "FAIL          {id:<34} {}\n  ::error::{id}: {reason} Next step: {}{shown}",
                        both(id, v.secs),
                        v.next
                    ),
                    "failed",
                )
            }
            Kind::Broken => {
                broken += 1;
                (format!("CHECK BROKEN  {id:<34} {reason}. Next: {}", v.next), "broken")
            }
        };
        text.push_str(&line);
        text.push('\n');
        let mut row = json!({"id": id, "verdict": kind});
        if let Some(t) = timings.get(id) {
            row["run_secs"] = json!(t.ran);
            row["queued_secs"] = json!(t.queued);
        }
        data.push(row);
    }
    text.push_str("CI also runs, and this gate does not:\n");
    for (what, why) in gates::NOT_RUN {
        text.push_str(&format!("  - {what}: {why}\n"));
    }
    text.push_str(&format!(
        "pre-push: {ok} passed, {fail} failed, {unmeasured} not measured, {accepted} accepted, {broken} broken, {skip} skipped. {}",
        if fail + unmeasured + broken > 0 {
            "Not proven: do not push.".to_string()
        } else if accepted > 0 {
            format!("Pushed with {accepted} accepted not-measured row(s): those are not proven.")
        } else {
            "Safe to push.".to_string()
        }
    ));
    // The ids once more, on the last lines: output that gets cut (a hook's long table) still ends with what the author has to act on.
    if fail > 0 {
        text.push_str(&format!("\nFAILED ({fail}): {}", failed_ids.join("; ")));
    }
    if unmeasured > 0 {
        text.push_str(&format!("\nNOT MEASURED ({unmeasured}): {}", unmeasured_ids.join("; ")));
    }
    // 7 beats 5 and 6, and 5 beats 6: the worst finding decides.
    let exit = if broken > 0 {
        Exit::CheckBroken
    } else if fail > 0 {
        Exit::Findings
    } else if unmeasured > 0 {
        Exit::NotMeasured
    } else {
        Exit::Ok
    };
    let not_run: Vec<&str> = gates::NOT_RUN.iter().map(|(w, _)| *w).collect();
    Report::new(
        json!({ "gates": data, "passed": ok, "failed": fail, "not_measured": unmeasured, "accepted": accepted, "broken": broken, "skipped": skip, "ci_only": not_run }),
        text,
    )
    .with_exit(exit)
}

/// The change, notes for the header and the pushed commits beyond the first. Without pushed refs: files since the merge base
/// (committed, modified, untracked) and the range to HEAD. With them: the files and range of the first pushed commit only (the run
/// checks that commit in a worktree of its own, so a dirty tree is not part of it).
#[allow(clippy::type_complexity)]
fn discover(root: &Path, base: &str, files: &[String], pushed: &[String]) -> Result<(Change, Vec<String>, Vec<String>), CliError> {
    let lines = |t: Option<String>| {
        t.unwrap_or_default()
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let merge_base = |tip: &str| {
        git_text(root, &["merge-base", base, tip])
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                CliError::failure(format!("There is no merge base of {base} and {tip}.")).with_hint(format!(
                    "Run `git fetch origin` or pass --base <ref> (this check measures the push from {base})."
                ))
            })
    };
    let count = |mb: &str, tip: &str| {
        git_text(root, &["rev-list", "--count", &format!("{mb}..{tip}")])
            .and_then(|t| t.trim().parse::<u32>().ok())
            .unwrap_or(0)
    };
    // Every file a commit of the range touches, also one a later commit changes back: the tree diff alone would miss it, and the
    // commit is still one the push sends.
    let commit_files = |mb: &str, tip: &str| {
        git_text(root, &["log", "--no-merges", "--name-only", "--format=", &format!("{mb}..{tip}")])
            .map(|t| lines(Some(t)))
            .ok_or_else(|| CliError::failure(format!("git log {mb}..{tip} failed.")).with_hint("Run it by hand to see why."))
    };
    let mut notes = Vec::new();
    let mut changed: Vec<String> = Vec::new();
    let mut ranges = Vec::new();
    let mut extra = Vec::new();
    if let Some((tip, rest)) = pushed.split_first() {
        extra.extend(rest.iter().cloned());
        let mb = merge_base(tip)?;
        notes.push(format!(
            "measured from {base} (merge base {mb}): every file the commits {mb}..{tip} change, one by one and in all"
        ));
        let diff = git_text(root, &["diff", "--name-only", &mb, tip])
            .ok_or_else(|| CliError::failure(format!("git diff {mb} {tip} failed.")).with_hint("Run it by hand to see why."))?;
        changed.extend(lines(Some(diff)));
        changed.extend(commit_files(&mb, tip)?);
        if count(&mb, tip) > 0 {
            ranges.push(format!("{mb}..{tip}"));
        }
        let dirty = lines(git_text(root, &["status", "--porcelain"]));
        if !dirty.is_empty() {
            notes.push(format!(
                "the working tree has {} uncommitted change(s); they are not pushed and play no part in the check",
                dirty.len()
            ));
        }
    } else {
        let mb = merge_base("HEAD")?;
        if files.is_empty() {
            let tracked = git_text(root, &["diff", "--name-only", &mb])
                .ok_or_else(|| CliError::failure(format!("git diff against {mb} failed.")).with_hint("Run it by hand to see why."))?;
            changed.extend(lines(Some(tracked)));
            changed.extend(commit_files(&mb, "HEAD")?);
            changed.extend(lines(git_text(root, &["ls-files", "--others", "--exclude-standard"])));
            let dirty = lines(git_text(root, &["status", "--porcelain"]));
            if !dirty.is_empty() {
                notes.push(format!(
                    "the working tree has {} uncommitted change(s); they are checked here but a push does not send them",
                    dirty.len()
                ));
            }
        } else {
            changed.extend(files.iter().cloned());
        }
        if count(&mb, "HEAD") > 0 {
            ranges.push(format!("{mb}..HEAD"));
        }
    }
    changed.sort();
    changed.dedup();
    Ok((Change { files: changed, ranges }, notes, extra))
}

/// What the repository says that the plan does not: the conformance contract's check list, the apps that lint Windows, the CI image.
fn read_facts(root: &Path, plan: &Value, env: &HashMap<String, String>) -> Facts {
    let ci = crate::ci_run::contract::conformance_args_at(root);
    let mut windows = std::collections::BTreeSet::new();
    for ws in gates::workspaces(plan) {
        let Some(app) = ws.strip_prefix("apps/").and_then(|r| r.split('/').next()) else {
            continue;
        };
        let contract = std::fs::read_to_string(root.join(format!("apps/{app}/paddock.ci.json")))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        if contract.is_some_and(|c| c["windowsClippy"] == true) {
            windows.insert(ws);
        }
    }
    let image = std::fs::read_to_string(root.join("ci-image/VERSION"))
        .map(|v| format!("dev.decryptology.net/decryp7/paddock-ci:v{}", v.trim()))
        .map_err(|e| format!("ci-image/VERSION cannot be read: {e}"));
    let mut tests: Vec<String> = std::fs::read_dir(root.join("apps/pitwall/scripts/ci"))
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.ends_with(".test.mjs"))
                .map(|n| format!("scripts/ci/{n}"))
                .collect()
        })
        .unwrap_or_default();
    tests.sort();
    Facts {
        conformance: Some(ci),
        conformance_env: None,
        windows_clippy: windows,
        image: Some(image),
        linux: false,
        kept_targets: None,
        pitwall_tests: tests,
        dev_exe: crate::tool_cache::self_exe().map_or_else(|_| "./dev".into(), |p| p.to_string_lossy().to_string()),
        target_dir: env.get("CARGO_TARGET_DIR").cloned(),
    }
}

/// Why conformance's npm install is not usable (the environment is not ready), or None when it is. The throw-away worktree links
/// this checkout's `node_modules` in (Unix only); a stale install fails inside conformance as "yaml cannot be loaded", which reads as
/// a finding, so each dependency's version in `node_modules/.package-lock.json` is compared with `package-lock.json` first.
fn node_env_problem(windows_throwaway: bool, read: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    if windows_throwaway {
        return Some("the throw-away worktree has no node_modules (it is linked in on Unix only, not on Windows)".into());
    }
    let json = |p: &str| read(p).and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let Some(installed) = json("node_modules/.package-lock.json") else {
        return Some("node_modules/.package-lock.json cannot be read (nothing is installed here)".into());
    };
    let (Some(lock), Some(manifest)) = (json("package-lock.json"), json("conformance/package.json")) else {
        return Some("package-lock.json or conformance/package.json cannot be read".into());
    };
    let names = ["dependencies", "devDependencies"]
        .iter()
        .filter_map(|k| manifest[k].as_object())
        .flat_map(|o| o.keys().cloned())
        .collect::<Vec<_>>();
    for name in names {
        for place in [format!("node_modules/{name}"), format!("conformance/node_modules/{name}")] {
            let Some(want) = lock["packages"][&place]["version"].as_str() else {
                continue;
            };
            let have = installed["packages"][&place]["version"].as_str();
            if have != Some(want) {
                return Some(format!(
                    "{place} is {} in node_modules but {want} in package-lock.json",
                    have.map_or("missing".to_string(), str::to_string)
                ));
            }
        }
    }
    None
}

/// One line per decision about a known commit, in the state folder: the shas, the remote, how fresh the ref lookup was and which
/// evidence decided (ER-27). A failed write is not an error of the push.
fn log_known(state: &Path, pushed: &[String], remote: Option<&str>, trail: &[String]) {
    use std::io::Write;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let line = format!(
        "{now} shas={} remote={} | {}\n",
        pushed.join(","),
        remote.unwrap_or("-"),
        trail.join(" | ")
    );
    let _ = std::fs::create_dir_all(state);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join("known-commits.log"))
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

#[cfg(test)]
mod tests;
