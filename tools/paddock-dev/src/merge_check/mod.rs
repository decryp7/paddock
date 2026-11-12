//! `./dev merge check`: did a merge silently drop one side's work? (Rule 48 / the 2026-11-05 merge check, ported from
//! `scripts/merge-regression-check.mjs` and extended on 2026-11-10.) Four detectors read the merge's parents and its result
//! tree:
//!
//! 1. files that go backwards against the first parent ([`backwards`], the port): a stale tree brought an old blob back
//!    (b48df1818), or the merge changed a file the branch never touched; answered by `Reverted: <path>: <reason>`;
//! 2. hunks a parent added since the merge base that the result lacks ([`hunks`]); answered by `Hunks-dropped: <path>: <reason>`;
//! 3. tests a parent has that the result lacks, three-way ([`lost_tests`]); answered by `Tests-removed: <name>: <reason>`;
//! 4. files taken whole from one parent although the other changed them too ([`whole_file`]); answered by `Hunks-dropped`.
//!
//! What it covers. 087570da4 restored what merge f94934ec5 dropped: a merge-level drop, found by detectors 2 and 3 (and 1 for
//! two of its files). 85f326139, 439861fcf and 359be91b4 are commit-level drops (single-parent commits that restored work an
//! earlier commit lost) and b48df1818 is a commit built on a stale tree: this check looks at a merge's parents against its
//! result, so a per-commit run covers the first three; the last is caught when its branch is merged (detector 1).
//!
//! Rule for filters: a filter may only remove findings for paths git reports as cleanly merged (not in the conflicted list of
//! `git merge-tree --write-tree`, and equal to its tree), and git's own merge is the arbiter for every detector that has a
//! filter: a conflicted path is never skipped, whatever its blob (a binary conflict taken as ours has parent 1's blob).
//!
//! Principle: every judgement is a three-way content judgement (base, mine, other, result), never a name or a count alone, and
//! each mechanism is tested with the others switched off (a test per detector that the others cannot satisfy). Git's own merge
//! of the two parents is the arbiter where one is needed: a file whose result equals it was merged, not taken.
//!
//! The gate must also run per commit, not only per merge: 85f326139, 439861fcf and 359be91b4 are commit-level drops that a
//! merge-level check cannot see.
//!
//! Octopus merges are not supported: they are reported as not measured (git has no automatic merge of more than two parents to
//! ask, and the history has none).
//!
//! Outcomes follow ADR 0089: clean, findings (exit 5, an error object in JSON mode) and not measured (a missing parent, a
//! shallow clone, an unparsable file, a git failure). Never a silent pass: whatever the check could not read is reported as
//! not measured. Generated files ([`generated`]) are skipped by all three detectors. Not done: `git cat-file --batch` (the
//! shared process runner closes standard input).

mod backwards;
mod diff;
mod generated;
mod generator_step;
pub(crate) mod git;
mod hunks;
mod lost_tests;
mod testnames;
mod trailers;
mod whole_file;

use git::{AutoMerge, Git, short};
use paddock_cli::{CliError, Exit, Report};
use serde_json::json;
use std::path::PathBuf;

/// What kind of problem a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A file goes backwards against the first parent (a stale tree, a revert).
    Backwards,
    /// A parent's hunk is missing from the result.
    DroppedHunk,
    /// A parent's test is missing from the result.
    LostTest,
    /// The result is one parent's version of a file the other parent changed too.
    WholeFile,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Backwards => "backwards",
            Kind::DroppedHunk => "dropped-hunk",
            Kind::LostTest => "lost-test",
            Kind::WholeFile => "whole-file",
        }
    }
}

/// One thing the merge lost.
#[derive(Debug, Clone)]
pub struct Finding {
    /// The detector.
    pub kind: Kind,
    /// The file, or the test name.
    pub subject: String,
    /// What is wrong and how to give a reason.
    pub detail: String,
    /// Lines that show it (where a dropped hunk was, and what it said); empty when the detail says it all.
    pub excerpts: Vec<String>,
}

/// The result of a check (ADR 0089).
#[derive(Debug)]
pub enum Outcome {
    /// Measured, nothing lost.
    Clean,
    /// Measured, something lost.
    Findings(Vec<Finding>),
    /// The check could not look; the text says why.
    NotMeasured(String),
}

/// A merge to check: its parents (first parent first), the result tree and the message that may carry reasons.
#[derive(Debug)]
pub struct Merge {
    /// Parent commits.
    pub parents: Vec<String>,
    /// The tree of the merge result.
    pub tree: String,
    /// The merge commit's message (empty when checking a branch before merging).
    pub message: String,
    /// A name for output.
    pub label: String,
}

/// What every detector reads: the repository, the merge's parents, its result tree and message, and the generated files.
pub struct Ctx<'a> {
    git: &'a Git<'a>,
    parents: &'a [String],
    tree: &'a str,
    message: &'a str,
    generated: &'a generated::Generated,
    /// The diff from each parent to the result, in parent order.
    diffs: &'a [Vec<diff::FileDiff>],
    /// Git's own merge of the two parents, and the result's files.
    auto: &'a AutoMerge,
    result_blobs: &'a std::collections::HashMap<String, String>,
}

impl Ctx<'_> {
    /// Whether git merged `path` cleanly and the result is exactly that: nothing a parent added can be missing from it. A path
    /// git lists as conflicted is never clean, whatever its blob is (a binary conflict's blob is parent 1's).
    fn merged_cleanly(&self, path: &str) -> bool {
        !self.auto.conflicted.contains(path) && self.auto.blobs.get(path) == self.result_blobs.get(path)
    }
}

/// Runs the detectors over `merge`.
pub fn analyze(git: &Git, merge: &Merge) -> Outcome {
    match run(git, merge) {
        Ok(f) if f.is_empty() => Outcome::Clean,
        Ok(f) => Outcome::Findings(f),
        Err(why) => Outcome::NotMeasured(why),
    }
}

fn run(git: &Git, merge: &Merge) -> Result<Vec<Finding>, String> {
    if merge.parents.len() > 2 {
        return Err("octopus merges are not supported (git has no automatic merge of more than two parents to ask)".into());
    }
    // A shallow clone's history is cut: a parent, a merge base or an earlier blob may be missing, and what is missing cannot
    // be told from what is not there.
    if git.is_shallow()? {
        return Err("this clone is shallow, so parents, merge bases and earlier blobs may be missing (run `git fetch --unshallow`)".into());
    }
    for p in &merge.parents {
        git.text(&["cat-file", "-e", &format!("{p}^{{commit}}")])
            .map_err(|e| format!("a parent is not in this clone (a shallow clone? run `git fetch --unshallow`): {e}"))?;
    }
    let first = merge.parents.first().ok_or("the merge has no parents")?;
    let tree = merge.tree.as_str();
    // One diff per parent, shared by the three detectors; the first parent's also serves the port.
    let diffs = merge.parents.iter().map(|p| diff_of(git, p, tree)).collect::<Result<Vec<_>, _>>()?;
    let paths: std::collections::BTreeSet<String> = diffs
        .iter()
        .flatten()
        .flat_map(|f| [f.path.clone(), f.old().to_string()])
        .filter(|p| !generated::by_name(p))
        .collect();
    let generated = generated::Generated::load(git, tree, &paths.into_iter().collect::<Vec<_>>())?;
    let mut out = Vec::new();
    let blobs = merge.parents.iter().map(|p| git.blob_ids(p)).collect::<Result<Vec<_>, _>>()?;
    let result_blobs = git.blob_ids(tree)?;
    // Git's own merge of two parents: what a file is when nothing but the three-way merge decides it.
    let auto = git.auto_merge(&merge.parents[0], &merge.parents[1])?;
    let cx = Ctx {
        git,
        parents: &merge.parents,
        tree,
        message: &merge.message,
        generated: &generated,
        diffs: &diffs,
        auto: &auto,
        result_blobs: &result_blobs,
    };
    for (i, parent) in merge.parents.iter().enumerate() {
        // The merge base of the first parent with the second; of a later parent with the first (an octopus has one per parent).
        let other = merge.parents.get(if i == 0 { 1 } else { i }).ok_or("the merge has one parent")?;
        let base = git
            .text(&["merge-base", first, other])
            .map_err(|e| format!("no merge base for parent {}: {e}", i + 1))?;
        let base = base.trim();
        let base_blobs = git.blob_ids(base)?;
        let listings = whole_file::Listings {
            parents: &blobs,
            base: &base_blobs,
            result: &result_blobs,
            auto: &auto,
        };
        out.extend(whole_file::check(&merge.parents, i, &listings, &merge.message, &generated));
        if i > 0 {
            out.extend(backwards::check(&cx, parent, base, &diffs[0])?);
        }
        out.extend(hunks::check(&cx, i, base, &diffs[i])?);
        out.extend(lost_tests::check(&cx, i, base, &diffs[i])?);
    }
    Ok(out)
}

fn diff_of(git: &Git, from: &str, to: &str) -> Result<Vec<diff::FileDiff>, String> {
    let mut args = vec!["diff"];
    args.extend(diff::DIFF_FLAGS);
    args.extend([from, to]);
    diff::parse(&git.text(&args)?)
}

/// Command line of `./dev merge check`.
pub struct Request {
    /// The repository root (default: found from the current folder).
    pub root: Option<PathBuf>,
    /// A merge commit.
    pub commit: Option<String>,
    /// A branch about to be merged, with `target`.
    pub branch: Option<String>,
    /// The branch it would be merged into.
    pub target: Option<String>,
    /// Run the generator checks of `docs/generators.toml` that the merge makes due (the commit form only; the checkout must be
    /// the merge result). `--no-generators` turns it off.
    pub generators: bool,
}

fn resolve(git: &Git, req: &Request) -> Result<Merge, CliError> {
    let gap = |e: String| CliError::failure(not_measured(&e));
    match (&req.commit, &req.branch, &req.target) {
        (Some(c), None, None) => {
            let line = git.text(&["rev-list", "--parents", "-n", "1", c]).map_err(gap)?;
            let ids: Vec<String> = line.split_whitespace().skip(1).map(str::to_string).collect();
            if ids.len() < 2 {
                let shallow = git.text(&["rev-parse", "--is-shallow-repository"]).map_err(gap)?;
                if shallow.trim() == "true" {
                    return Err(gap(format!(
                        "{c} shows no parents because this clone is shallow (run `git fetch --unshallow`)"
                    )));
                }
                return Err(CliError::usage(format!("{c} is not a merge commit."))
                    .with_hint("Give a merge commit, or --branch <b> --target <t> before merging."));
            }
            let tree = git.text(&["rev-parse", &format!("{c}^{{tree}}")]).map_err(gap)?.trim().to_string();
            let message = git.text(&["show", "-s", "--format=%B", c]).map_err(gap)?;
            Ok(Merge {
                parents: ids,
                tree,
                message,
                label: format!("merge {}", short(c)),
            })
        }
        (None, Some(b), Some(t)) => {
            let first = git
                .text(&["rev-parse", "--verify", &format!("{t}^{{commit}}")])
                .map_err(gap)?
                .trim()
                .to_string();
            let second = git
                .text(&["rev-parse", "--verify", &format!("{b}^{{commit}}")])
                .map_err(gap)?
                .trim()
                .to_string();
            let tree = git
                .text(&["merge-tree", "--write-tree", "--no-messages", &first, &second])
                .map_err(|e| gap(format!("{b} does not merge cleanly into {t}, so there is no result to check: {e}")))?;
            let tree = tree.lines().next().unwrap_or("").trim().to_string();
            Ok(Merge {
                parents: vec![first, second],
                tree,
                message: String::new(),
                label: format!("{b} into {t}"),
            })
        }
        _ => Err(CliError::usage("Give one merge commit, or --branch and --target together.")
            .with_hint("`./dev merge check <merge-commit>` or `./dev merge check --branch <b> --target <t>`.")),
    }
}

/// The `::error::` line for a check that could not look (ER-27, ADR 0089).
fn not_measured(why: &str) -> String {
    format!(
        "::error::merge check NOT MEASURED: {why}. Next step: fix what is named, then run it again; a merge that was not measured is not a merge that passed."
    )
}

/// The findings as text: a header, one line per finding in one format, and the `::error::` line with the next step.
fn findings_text(label: &str, found: &[Finding]) -> String {
    let mut lines = vec![format!("merge check: {label}: {} finding(s)", found.len())];
    for f in found {
        lines.push(format!("  [{}] {}: {}", f.kind.name(), f.subject, f.detail));
        lines.extend(f.excerpts.iter().map(|e| format!("      {e}")));
    }
    lines.push(format!(
        "::error::{label}: {} finding(s). Next step: restore what was dropped, or if it was meant, say so in the merge message with `Hunks-dropped: <path>: <reason>`, `Reverted: <path>: <reason>` or `Tests-removed: <name>: <reason>`, then run it again.",
        found.len()
    ));
    lines.join("\n")
}

fn clean_report(label: &str) -> Report {
    Report::new(
        json!({"outcome": "clean", "merge": label}),
        format!("merge check: {label}: nothing a parent added is missing from the result."),
    )
}

/// Runs the check: the merge's label and its findings, or why it could not look.
fn check(req: &Request) -> Result<(String, Vec<Finding>, Option<generator_step::Step>), CliError> {
    let root = crate::gates::command::repo_root(req.root.as_deref())?;
    let git = Git::new(&root);
    let merge = resolve(&git, req)?;
    match analyze(&git, &merge) {
        // TODO(ADR 0089 WP-1): exit 6 (not measured) once `paddock_cli::Exit` has it; until then it is Failure (1).
        Outcome::NotMeasured(why) => Err(CliError::failure(not_measured(&why))),
        outcome => {
            let found = match outcome {
                Outcome::Findings(f) => f,
                _ => Vec::new(),
            };
            // The generated files are checked in the checkout, so only a merge that exists as a commit has one to check.
            let step = if !req.generators {
                Some(generator_step::not_run("--no-generators"))
            } else if req.commit.is_none() {
                Some(generator_step::not_run(
                    "a branch is checked before it is merged, so there is no checkout of the result; run `./dev merge check <merge-commit>` after merging",
                ))
            } else {
                generator_step::run(&root, &git, &merge, &crate::generators::run::Real, &|l| eprintln!("{l}"))
            };
            Ok((merge.label, found, step))
        }
    }
}

/// `./dev merge check`. Detector findings are an error (exit 5), so the text goes to standard error like every other failure.
/// A stale generated file is exit 5 and a generator check that could not look is exit 6, both with their `::error::` lines.
pub fn command(req: &Request) -> Result<Report, CliError> {
    let (label, found, step) = check(req)?;
    let lines = step.as_ref().map(|s| s.text.join("\n"));
    let exit = step.as_ref().map_or(Exit::Ok, |s| s.exit);
    if !found.is_empty() {
        let mut text = findings_text(&label, &found);
        if let Some(l) = lines {
            text.push('\n');
            text.push_str(&l);
        }
        return Err(CliError::new(Exit::Findings, text));
    }
    let mut report = clean_report(&label);
    if let Some(s) = step {
        report.text.push('\n');
        report.text.push_str(&lines.unwrap_or_default());
        report.data["generators"] = s.data;
    }
    Ok(if exit == Exit::Ok { report } else { report.with_exit(exit) })
}

/// `./dev merge check --json`: one object on standard output. Findings are `"ok": false, "kind": "findings"` with the
/// structured list (`kind`, `subject`, `detail`), and the text goes to standard error as without `--json`. `"generators"` holds
/// the generator checks' results when they ran.
pub fn command_json(req: &Request, out: &mut dyn std::io::Write, err: &mut dyn std::io::Write) -> i32 {
    match check(req) {
        Err(e) => paddock_cli::finish(paddock_cli::Format::Json, Err(e), out, err).code(),
        Ok((label, found, step)) if found.is_empty() => {
            let exit = step.as_ref().map_or(Exit::Ok, |s| s.exit);
            let mut report = clean_report(&label);
            if let Some(s) = step {
                report.text.push('\n');
                report.text.push_str(&s.text.join("\n"));
                report.data["generators"] = s.data;
            }
            let report = if exit == Exit::Ok { report } else { report.with_exit(exit) };
            paddock_cli::finish(paddock_cli::Format::Json, Ok(report), out, err).code()
        }
        Ok((label, found, step)) => {
            let mut text = findings_text(&label, &found);
            if let Some(s) = &step {
                text.push('\n');
                text.push_str(&s.text.join("\n"));
            }
            let object = json!({
                "ok": false,
                "kind": "findings",
                "exit": Exit::Findings.code(),
                "merge": label,
                "error": text,
                "findings": found.iter().map(|f| json!({"kind": f.kind.name(), "subject": f.subject, "detail": f.detail, "excerpts": f.excerpts})).collect::<Vec<_>>(),
                "generators": step.map(|s| s.data),
            });
            let _ = writeln!(out, "{object}");
            let _ = writeln!(err, "error: {text}");
            Exit::Findings.code()
        }
    }
}
