//! A pushed commit this tool or CI has already proven. Never "a remote branch holds it": only recorded evidence for that exact sha
//! counts (ER-33), and the result is its own verdict, `known_commit`, that says nothing was re-checked.
//!
//! Evidence, in this order:
//! - a pass record this tool wrote for the sha ([`write_record`]: on every complete passing run, under the git common dir so every
//!   worktree sees it), valid while the sha's tree id is the recorded one;
//! - a complete green CI run on the sha, read through the one shared reader (`./dev ci verdict`), and only when the push's own
//!   remote (`--remote`, git's `$1`) still has that sha as a branch tip, confirmed with `git ls-remote`.
//!
//! It never applies to `--files`, an explicit `--base`, `--only` or `--dry-run` runs.

use super::PrePushArgs;
use super::outcome::GitRepo;
use crate::plan_repo::git_text;
use paddock_cli::outcome::Sha;
use paddock_cli::{CheckId, Evidence as Proof, Exit, Outcome, Report, ToolVersion, UnixSeconds};
use serde_json::{Value, json};
use std::path::Path;

/// The CI workflow whose complete green run counts.
pub const CI_WORKFLOW: &str = "ci.yml";

/// What proves a commit.
#[derive(Debug, Clone)]
pub enum Evidence {
    /// A pass record written by this tool.
    Record {
        /// `<sha12>@<unix seconds>`.
        id: String,
        /// The record, adopted: a `known-commit` outcome that the commit's tree was re-read live for.
        outcome: Outcome,
    },
    /// A complete green CI run.
    CiRun {
        /// The run id.
        run: u64,
        /// The run as evidence, adopted the same way.
        outcome: Outcome,
    },
}

impl Evidence {
    fn kind(&self) -> &'static str {
        match self {
            Evidence::Record { .. } => "record",
            Evidence::CiRun { .. } => "ci_run",
        }
    }
    fn id(&self) -> String {
        match self {
            Evidence::Record { id, .. } => id.clone(),
            Evidence::CiRun { run, .. } => run.to_string(),
        }
    }
    fn text(&self) -> String {
        match self {
            Evidence::Record { id, .. } => format!("this tool's own pass record {id}"),
            Evidence::CiRun { run, .. } => format!("a complete green {CI_WORKFLOW} run {run} on this exact commit"),
        }
    }
    /// The adopted outcome (`known-commit`).
    pub fn outcome(&self) -> &Outcome {
        match self {
            Evidence::Record { outcome, .. } | Evidence::CiRun { outcome, .. } => outcome,
        }
    }
}

/// The lookups that leave the machine, behind a trait so tests never touch a network (ER-14).
pub trait Lookups {
    /// The output of `git ls-remote <remote>` (None: it could not be read).
    fn ls_remote(&self, remote: &str) -> Option<String>;
    /// The id of the complete green run of [`CI_WORKFLOW`] on the sha, or why there is none.
    fn ci_green(&self, sha: &str) -> Result<u64, String>;
}

/// Whether this run may use evidence at all: the hook's push of commits, with no override of what is measured.
pub fn applicable(args: &PrePushArgs, pushed: &[String]) -> bool {
    args.refs_from_stdin && args.files.is_empty() && args.base.is_none() && args.only.is_empty() && !args.dry_run && !pushed.is_empty()
}

fn record_path(state: &Path, sha: &str) -> std::path::PathBuf {
    state.join("records").join(format!("{sha}.json"))
}

fn tree_of(root: &Path, sha: &str) -> Option<String> {
    git_text(root, &["rev-parse", "--verify", "--quiet", &format!("{sha}^{{tree}}")]).map(|t| t.trim().to_string())
}

/// The record of this exact sha: its sha matches, and its `evidence` (a serialized `paddock_cli::Evidence` of a pass of this tool)
/// is about this commit and adopts against the tree the repository has now (`Evidence::adopt`, ADR 0089 amendment).
fn read_record(root: &Path, state: &Path, sha: &str) -> Option<Evidence> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(record_path(state, sha)).ok()?).ok()?;
    if v["sha"] != sha {
        return None;
    }
    let proof = Proof::from_json(v.get("evidence")?).ok()?;
    if !matches!(proof.subject(), paddock_cli::Subject::Commit { sha: s, .. } if s.as_str() == sha) {
        return None;
    }
    match Proof::adopt(proof, &GitRepo(root)) {
        outcome @ Outcome::KnownCommit(_) => Some(Evidence::Record {
            id: v["id"].as_str().unwrap_or_default().to_string(),
            outcome,
        }),
        _ => None,
    }
}

/// Records that the gates passed on this sha: `{id, sha, tree, base, gates, tool_version, time, evidence}`, where `evidence` is the
/// pass's own `paddock_cli::Evidence` (taken from the run's `Clean` outcome, so only a real pass can leave one).
pub fn write_record(root: &Path, state: &Path, sha: &str, base: &str, gates: &[String], now: u64, pass: &Outcome) -> Result<(), String> {
    let Outcome::Clean(passed) = pass else {
        return Err("the run did not end in a pass, so there is nothing to record".into());
    };
    let tree = tree_of(root, sha).ok_or_else(|| format!("the tree of {sha} cannot be read"))?;
    let rec = json!({
        "id": format!("{}@{now}", &sha[..sha.len().min(12)]), "sha": sha, "tree": tree, "base": base, "gates": gates,
        "tool_version": env!("CARGO_PKG_VERSION"), "time": now, "evidence": passed.evidence().to_json(),
    });
    let path = record_path(state, sha);
    std::fs::create_dir_all(path.parent().unwrap_or(state)).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::write(&path, rec.to_string()).map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether a finished run may leave a record: it ran every gate (no `--only`, `--files`, dry run or override), for one commit, and
/// everything passed or was skipped by the plan.
pub fn recordable(args: &PrePushArgs, accepted: usize, extra: usize, exit: Exit) -> bool {
    args.files.is_empty() && args.base.is_none() && args.only.is_empty() && !args.dry_run && accepted == 0 && extra == 0 && exit == Exit::Ok
}

/// The evidence for every pushed sha (each needs its own), and the trail of what was looked at, for the log (ER-27).
pub fn find(
    root: &Path,
    state: &Path,
    remote: Option<&str>,
    pushed: &[String],
    look: &dyn Lookups,
) -> (Option<Vec<Evidence>>, Vec<String>) {
    let mut trail = Vec::new();
    let mut ls: Option<Option<String>> = None;
    let mut all = Vec::new();
    for sha in pushed {
        let short = &sha[..sha.len().min(12)];
        if let Some(e) = read_record(root, state, sha) {
            trail.push(format!("{short}: pass record {} matches the commit and its tree", e.id()));
            all.push(e);
            continue;
        }
        trail.push(format!("{short}: no valid pass record"));
        let Some(remote) = remote.filter(|r| !r.is_empty() && !r.starts_with('-')) else {
            trail.push(format!("{short}: no usable --remote, so no CI lookup"));
            return (None, trail);
        };
        let listing = ls.get_or_insert_with(|| look.ls_remote(remote));
        let tip = listing
            .as_ref()
            .is_some_and(|l| l.lines().any(|x| x.split_whitespace().next() == Some(sha.as_str())));
        trail.push(format!(
            "{short}: git ls-remote {remote} (live): {}",
            match listing {
                None => "unreadable",
                Some(_) if tip => "the commit is a branch tip",
                Some(_) => "the commit is not a branch tip",
            }
        ));
        if !tip {
            return (None, trail);
        }
        match look.ci_green(sha) {
            Ok(run) => {
                trail.push(format!("{short}: {CI_WORKFLOW} run {run} is complete and green"));
                let proof = Proof::ci_run(
                    CheckId::new(format!("ci:{CI_WORKFLOW}")),
                    CI_WORKFLOW,
                    paddock_cli::outcome::RunId(run),
                    Sha::new(sha.as_str()),
                    ToolVersion::new("paddock-dev ci verdict"),
                    UnixSeconds::now(),
                );
                match Proof::adopt(proof, &GitRepo(root)) {
                    outcome @ Outcome::KnownCommit(_) => all.push(Evidence::CiRun { run, outcome }),
                    other => {
                        trail.push(format!("{short}: the CI run could not be adopted ({})", other.detail()));
                        return (None, trail);
                    }
                }
            }
            Err(why) => {
                trail.push(format!("{short}: no complete green CI run ({why})"));
                return (None, trail);
            }
        }
    }
    (Some(all), trail)
}

/// The verdict `known_commit`: text and JSON say that nothing was re-checked and by what evidence, never "passed".
pub fn report(pushed: &[String], remote: Option<&str>, ev: &[Evidence]) -> Report {
    let what: Vec<String> = pushed
        .iter()
        .zip(ev)
        .map(|(s, e)| format!("{} by {}", &s[..s.len().min(12)], e.text()))
        .collect();
    let data = json!({
        "verdict": "known_commit", "not_rechecked": true, "gates": [], "changed": 0, "remote": remote,
        "commits": pushed.iter().zip(ev).map(|(s, e)| json!({"sha": s, "evidence": {"kind": e.kind(), "id": e.id()}, "outcome": e.outcome().json()})).collect::<Vec<_>>(),
        "outcome": ev.first().map(|e| e.outcome().json()),
    });
    Report::new(data, format!("pre-push: not re-checked: known commit, {}.", what.join("; "))).with_exit(Exit::Ok)
}
