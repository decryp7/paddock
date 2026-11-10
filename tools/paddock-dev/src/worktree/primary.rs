//! `check primary`: the probe of the primary checkout (ADR 0083 §3). The coordinator runs it at the start of a session,
//! before and after every landing batch and after any cleanup.
//!
//! It checks that the primary is on `main` at `main`'s tip with no local-only commit, has no tracked change, sparse
//! checkout or half-done merge, rebase or cherry-pick, lists untracked entries outside an allow-list, and compares the
//! locked worktrees with the owner map in both directions. Agent liveness is measured only against a list the coordinator
//! passes (`--live <file>`); without it the probe prints `liveness: not measured` and calls no lock stale (ER-33).
//! A check that could not run is reported as `not measured` and fails the probe: it never counts as a pass.

use super::git::{Repo, WorktreeInfo, worktrees};
use super::inventory::STALE_LOCK_SECS;
use super::note::{Note, now_secs};

use super::owners::{OwnerMap, OwnerStore, same_path};
use paddock_cli::{CliError, Exit, Report};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::Path;

/// The branch the primary checkout stays on.
pub const MAIN_BRANCH: &str = "main";
/// The remote branch `main` may only trail.
pub const REMOTE_MAIN: &str = "origin/main";
/// Untracked entries that live in the primary on purpose (tool folders), by path prefix.
pub const DEFAULT_ALLOWED_UNTRACKED: [&str; 3] = [".claude/densepack-vault/", ".serena/", "node_modules"];

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The primary or the ownership is wrong; fails the probe.
    Error,
    /// A check could not run; fails the probe (it is not a pass).
    NotMeasured,
    /// Worth a look; does not fail the probe.
    Warning,
}

/// One difference, with the next step (ER-27).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// A stable id (`branch`, `local-commits`, `tracked`, `owner-missing`, ...).
    pub id: &'static str,
    /// How serious.
    pub severity: Severity,
    /// What is wrong.
    pub message: String,
    /// What to do.
    pub next: String,
}

impl Finding {
    fn new(id: &'static str, severity: Severity, message: impl Into<String>, next: impl Into<String>) -> Finding {
        Finding {
            id,
            severity,
            message: message.into(),
            next: next.into(),
        }
    }

    /// The annotation line.
    pub fn line(&self) -> String {
        let level = match self.severity {
            Severity::Warning => "warning",
            _ => "error",
        };
        let measured = if self.severity == Severity::NotMeasured {
            " (not measured)"
        } else {
            ""
        };
        format!("::{level}::{}{measured}: {} Next: {}", self.id, self.message, self.next)
    }
}

/// `check primary`'s options.
pub struct Options<'a> {
    /// The known-untracked file (default in the git directory).
    pub baseline: Option<&'a Path>,
    /// Record the current untracked entries as the baseline.
    pub accept_untracked: bool,
    /// A file with one live `agent_id` per line.
    pub live: Option<&'a Path>,
    /// Allowed untracked prefixes beyond [`DEFAULT_ALLOWED_UNTRACKED`].
    pub allow_untracked: &'a [String],
}

/// Untracked entries (`?? path`) that no allow-list prefix covers.
///
/// Git prints an untracked folder as one `dir/` entry. A folder that only contains an allowed path (`.claude/` holding
/// `.claude/densepack-vault/`) is opened with `expand` (its untracked files) so the rest of it is still judged.
pub fn unexpected_untracked(status: &str, allowed: &[String], expand: &dyn Fn(&str) -> Vec<String>) -> Vec<String> {
    let covered = |p: &str| {
        allowed.iter().any(|a| {
            let a = a.trim_end_matches('/');
            p.trim_end_matches('/') == a || p.starts_with(&format!("{a}/"))
        })
    };
    let mut out = Vec::new();
    for p in status
        .lines()
        .filter_map(|l| l.strip_prefix("?? "))
        .map(|p| p.trim_matches('"').to_string())
    {
        let holds_allowed = p.ends_with('/') && allowed.iter().any(|a| a.starts_with(&p));
        let entries = if holds_allowed { expand(&p) } else { vec![p] };
        out.extend(entries.into_iter().filter(|e| !covered(e)));
    }
    out
}

/// The two-way comparison of locked worktrees and owner-map entries, and the lock ages.
pub fn ownership_findings(list: &[WorktreeInfo], owners: &OwnerMap, now: u64) -> Vec<Finding> {
    let mut out = Vec::new();
    let primary = list.first().map(|w| w.path.replace('\\', "/")).unwrap_or_default();
    for w in list.iter().skip(1) {
        let k = w.path.replace('\\', "/");
        if super::git::is_under(&k, &primary) {
            out.push(Finding::new(
                "nested-worktree",
                Severity::Error,
                format!("{k} is inside the primary checkout's folder."),
                "Move it out (`git worktree move`) after its agent ended; every worktree belongs under the worktree root.",
            ));
        }
        let Some(reason) = &w.locked else { continue };
        // `git worktree add --lock` without --reason stores git's own placeholder text.
        if reason.trim().is_empty() || reason.trim() == "added with --lock" {
            out.push(Finding::new(
                "owner-note-missing",
                Severity::Error,
                format!("{k} is locked with no owner note."),
                "The coordinator re-locks it with a note (`git worktree lock --reason owner=<role>:<task> <path>`) or releases it.",
            ));
        }
        if owners.get(&k).is_none() {
            out.push(Finding::new(
                "owner-missing",
                Severity::Error,
                format!("{k} is locked but no agent has claimed it."),
                format!("The agent runs `./dev worktree claim {k}`, or the coordinator runs `./dev worktree release {k}`."),
            ));
        }
        if let Some(created) = Note::parse(reason).and_then(|n| n.created) {
            let age = now.saturating_sub(created);
            if age > STALE_LOCK_SECS {
                out.push(Finding::new(
                    "lock-old",
                    Severity::Warning,
                    format!("{k} has been locked for {} days.", age / 86_400),
                    "Check that its agent is still working; release it after the agent's completion notice (a lock is a hint, never removed automatically).",
                ));
            }
        }
    }
    for (path, e) in owners.iter() {
        let locked = list
            .iter()
            .skip(1)
            .any(|w| same_path(&w.path.replace('\\', "/"), path) && w.locked.is_some());
        if !locked {
            out.push(Finding::new(
                "owner-orphan",
                Severity::Error,
                format!("The owner map names {path} (agent {}), which is not a locked worktree.", e.agent_id),
                format!("The coordinator clears it with `./dev worktree reassign {path}`."),
            ));
        }
    }
    out
}

/// Entries of the owner map whose agent is not in `live`.
pub fn not_live(owners: &OwnerMap, live: &BTreeSet<String>) -> Vec<Finding> {
    owners
        .iter()
        .filter(|(_, e)| !live.contains(&e.agent_id))
        .map(|(path, e)| {
            Finding::new(
                "agent-not-live",
                Severity::Warning,
                format!("{path} is claimed by agent {}, which is not in the live list.", e.agent_id),
                format!(
                    "If it ended, the coordinator releases the worktree (`./dev worktree release {path}`); a note is a hint, not proof."
                ),
            )
        })
        .collect()
}

fn primary_findings(repo: &Repo) -> Vec<Finding> {
    let mut out = Vec::new();
    let git = repo.git();
    let head_ref = git
        .run(&["symbolic-ref", "-q", "HEAD"])
        .ok()
        .filter(|o| o.ok)
        .map(|o| o.stdout.trim().to_string());
    let main_ref = format!("refs/heads/{MAIN_BRANCH}");
    match &head_ref {
        None => out.push(Finding::new(
            "branch",
            Severity::Error,
            format!("The primary checkout has a detached HEAD; it must be on {MAIN_BRANCH}."),
            format!("Delegate the repair to the integrator; the primary returns to {MAIN_BRANCH} with `git switch {MAIN_BRANCH}` once nothing is lost."),
        )),
        Some(r) if *r != main_ref => out.push(Finding::new(
            "branch",
            Severity::Error,
            format!("The primary checkout is on {}, not {MAIN_BRANCH}.", r.trim_start_matches("refs/heads/")),
            format!("Check what was committed there, then `git switch {MAIN_BRANCH}`; agents work only in their own worktrees."),
        )),
        Some(_) => {}
    }
    let head = git.text(&["rev-parse", "HEAD"]).map(|s| s.trim().to_string()).unwrap_or_default();
    let main_sha = git
        .text(&["rev-parse", "--verify", "--quiet", &main_ref])
        .map(|s| s.trim().to_string())
        .ok();
    match &main_sha {
        None => out.push(Finding::new(
            "branch",
            Severity::Error,
            format!("There is no local {MAIN_BRANCH} branch."),
            "Create it from origin/main in the integrator's worktree.",
        )),
        Some(m) if head_ref.is_none() && *m != head => out.push(Finding::new(
            "head-not-main",
            Severity::Error,
            format!("HEAD {} is not {MAIN_BRANCH}'s tip {}.", short(&head), short(m)),
            "Find the commits made on this HEAD (`git reflog`) before anything moves it.",
        )),
        Some(_) => {}
    }
    if main_sha.is_some() {
        match git.run(&["rev-parse", "--verify", "--quiet", &format!("{REMOTE_MAIN}^{{commit}}")]) {
            Ok(o) if o.ok => {
                if !git
                    .succeeds(&["merge-base", "--is-ancestor", &main_ref, REMOTE_MAIN])
                    .unwrap_or(false)
                {
                    let n = git
                        .text(&["rev-list", "--count", &format!("{REMOTE_MAIN}..{main_ref}")])
                        .unwrap_or_default();
                    out.push(Finding::new(
                        "local-commits",
                        Severity::Error,
                        format!("{MAIN_BRANCH} has {} commit(s) that {REMOTE_MAIN} lacks.", n.trim()),
                        format!("Inspect `git log {REMOTE_MAIN}..{MAIN_BRANCH}`; move them to a branch in a worktree, then reset {MAIN_BRANCH} with the coordinator's fast-forward."),
                    ));
                }
            }
            _ => out.push(Finding::new(
                "local-commits",
                Severity::NotMeasured,
                format!("{REMOTE_MAIN} was not found, so local-only commits on {MAIN_BRANCH} could not be checked."),
                "Run `git fetch origin` in the primary, then the probe again.",
            )),
        }
    }
    match git.text(&["status", "--porcelain=v1", "--untracked-files=no"]) {
        Ok(s) if s.trim().is_empty() => {}
        Ok(s) => {
            let lines: Vec<&str> = s.lines().collect();
            out.push(Finding::new(
                "tracked",
                Severity::Error,
                format!(
                    "{} tracked file(s) changed in the primary: {}.",
                    lines.len(),
                    lines.iter().take(5).map(|l| l.trim()).collect::<Vec<_>>().join(", ")
                ),
                "Find which agent wrote there; move the change to its worktree and restore the primary with the coordinator's tools.",
            ));
        }
        Err(e) => out.push(Finding::new(
            "tracked",
            Severity::NotMeasured,
            e.to_string(),
            "Run `git status` in the primary.",
        )),
    }
    for (file, what) in [
        ("MERGE_HEAD", "merge"),
        ("CHERRY_PICK_HEAD", "cherry-pick"),
        ("REVERT_HEAD", "revert"),
        ("rebase-merge", "rebase"),
        ("rebase-apply", "rebase or am"),
    ] {
        if repo.common_dir.join(file).exists() {
            out.push(Finding::new(
                "in-progress",
                Severity::Error,
                format!("A {what} is in progress in the primary."),
                format!(
                    "Finish or abort it in the integrator's worktree, not here (`git {} --abort` as a last resort).",
                    what.split(' ').next().unwrap_or("merge")
                ),
            ));
        }
    }
    if git
        .text(&["config", "--get", "core.sparseCheckout"])
        .is_ok_and(|s| s.trim() == "true")
    {
        out.push(Finding::new(
            "sparse",
            Severity::Error,
            "The primary checkout is a sparse checkout.",
            "Run `git sparse-checkout disable` in the primary; sparse cones belong in agent worktrees.",
        ));
    }
    out
}

/// The untracked entries of the primary that the allow-list does not cover.
fn untracked_outside_allow(repo: &Repo, allow: &[String]) -> Result<Vec<String>, String> {
    let git = repo.git();
    let status = git
        .text(&["status", "--porcelain=v1", "--untracked-files=normal"])
        .map_err(|e| e.to_string())?;
    let expand = |dir: &str| -> Vec<String> {
        git.text(&["ls-files", "--others", "--exclude-standard", "--", dir])
            .map(|t| t.lines().map(str::to_string).collect())
            .unwrap_or_default()
    };
    Ok(unexpected_untracked(&status, allow, &expand))
}

/// The baseline file: one known untracked entry per line (`#` starts a comment).
fn read_baseline(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Entries that neither the baseline nor one of its folders names.
pub fn not_in_baseline<'a>(entries: &'a [String], baseline: &[String]) -> Vec<&'a String> {
    entries
        .iter()
        .filter(|e| !baseline.iter().any(|b| super::git::is_under(e, b)))
        .collect()
}

fn untracked_findings(repo: &Repo, allow: &[String], baseline_file: &Path, accept: bool) -> Vec<Finding> {
    let entries = match untracked_outside_allow(repo, allow) {
        Ok(e) => e,
        Err(e) => {
            return vec![Finding::new(
                "untracked",
                Severity::NotMeasured,
                e,
                "Run `git status` in the primary.",
            )];
        }
    };
    if accept {
        let mut text = String::from("# Known untracked entries of the primary checkout (`./dev check primary --accept-untracked`).\n");
        for e in &entries {
            text.push_str(e);
            text.push('\n');
        }
        if let Err(e) = std::fs::write(baseline_file, text) {
            return vec![Finding::new(
                "untracked",
                Severity::NotMeasured,
                format!("Cannot write {}: {e}.", baseline_file.display()),
                "Check the folder's permissions.",
            )];
        }
        return Vec::new();
    }
    let baseline = read_baseline(baseline_file);
    let new = not_in_baseline(&entries, &baseline);
    if new.is_empty() {
        return Vec::new();
    }
    vec![Finding::new(
        "untracked-new",
        Severity::Error,
        format!(
            "{} new untracked entr(ies) in the primary: {}.",
            new.len(),
            new.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ),
        format!(
            "Find which agent wrote them (a write into the primary leaves untracked files) and move or delete them; if they belong there, record them with `./dev check primary --accept-untracked` (baseline {}).",
            baseline_file.display()
        ),
    )]
}

fn short(sha: &str) -> &str {
    sha.get(..9).unwrap_or(sha)
}

/// Runs the probe.
pub fn check(repo: &Repo, opts: &Options<'_>) -> Result<Report, CliError> {
    let live: Option<BTreeSet<String>> = match opts.live {
        None => None,
        Some(f) => {
            let text = std::fs::read_to_string(f).map_err(|e| {
                CliError::failure(format!("Cannot read the live-agent list {}: {e}.", f.display()))
                    .with_hint("Write one agent_id per line from the coordinator's own agent list, or leave --live out.")
            })?;
            Some(
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .map(str::to_string)
                    .collect(),
            )
        }
    };
    let mut allow: Vec<String> = DEFAULT_ALLOWED_UNTRACKED.iter().map(|s| s.to_string()).collect();
    allow.extend(opts.allow_untracked.iter().cloned());
    let mut findings = primary_findings(repo);
    let default_baseline = repo.common_dir.join("paddock-primary-untracked.txt");
    findings.extend(untracked_findings(
        repo,
        &allow,
        opts.baseline.unwrap_or(&default_baseline),
        opts.accept_untracked,
    ));
    match (worktrees(repo), OwnerStore::open(&repo.common_dir).read()) {
        (Ok(list), Ok(owners)) => {
            findings.extend(ownership_findings(&list, &owners, now_secs()));
            if let Some(live) = &live {
                findings.extend(not_live(&owners, live));
            }
        }
        (Err(e), _) => findings.push(Finding::new(
            "ownership",
            Severity::NotMeasured,
            e.to_string(),
            "Run `git worktree list` in the primary.",
        )),
        (_, Err(e)) => findings.push(Finding::new(
            "ownership",
            Severity::NotMeasured,
            e.to_string(),
            "Check the owner map file named above.",
        )),
    }
    let failing = findings.iter().filter(|f| f.severity != Severity::Warning).count();
    let liveness = match &live {
        Some(l) => format!("liveness: measured against {} live agent(s)", l.len()),
        None => "liveness: not measured".to_string(),
    };
    let mut text = String::new();
    for f in &findings {
        text.push_str(&f.line());
        text.push('\n');
    }
    text.push_str(&format!(
        "primary {}: {}\n{liveness}\n",
        repo.primary.display(),
        if failing == 0 {
            "ok".to_string()
        } else {
            format!("{failing} problem(s)")
        }
    ));
    let only_unmeasured = findings.iter().all(|f| f.severity != Severity::Error) && failing > 0;
    let data = json!({
        "not_measured_only": only_unmeasured,
        "primary": repo.primary.to_string_lossy(),
        "healthy": failing == 0,
        "problems": failing,
        "liveness": if live.is_some() { "measured" } else { "not measured" },
        "findings": findings.iter().map(|f| json!({
            "id": f.id,
            "severity": match f.severity { Severity::Error => "error", Severity::NotMeasured => "not measured", Severity::Warning => "warning" },
            "message": f.message,
            "next": f.next,
        })).collect::<Vec<_>>(),
    });
    let report = Report::new(data, text);
    Ok(if failing == 0 { report } else { report.with_exit(Exit::Findings) })
}
