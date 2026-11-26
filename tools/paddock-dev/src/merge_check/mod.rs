//! `./dev merge check`: did a merge silently drop one side's work? (Rule 48 / the 2026-11-05 merge check, ported from
//! `scripts/merge-regression-check.mjs` and extended on 2026-11-10.) Four detectors read the merge's parents and its result
//! tree:
//!
//! 1. files that go backwards against the first parent ([`backwards`], the port): a stale tree brought an old blob back
//!    (b48df1818), or the merge changed a file the branch never touched; answered by `Reverted: <path>: <reason>`;
//! 2. hunks a parent added since the merge base that the result lacks ([`hunks`]); answered by `Hunks-dropped: <path>: <reason>`;
//! 3. tests a parent has that the result lacks, three-way ([`lost_tests`]); answered by `Tests-removed: <name>: <reason>`;
//! 4. files taken whole from one parent although the other changed them too ([`whole_file`]); answered by `Hunks-dropped`;
//!    (and, report-only, a member built on a merge the target reverted: [`reverted_base`], a `::warning::` per merge);
//! 5. a stale workspace-hack, which no merge message can excuse ([`hakari`]): the fix is `cargo hakari generate` (the check compares the
//!    generated file only; refreshing the lockfiles is the author's step and is not verified).
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
mod gates;
mod generated;
mod generator_step;
pub(crate) mod git;
mod hakari;
mod hunks;
mod lost_tests;
mod reland;
pub(crate) mod reverted_base;
mod stale_base;
mod static_rows;
pub mod sync_member;
mod testnames;
mod trailers;
mod whole_file;

use git::{AutoMerge, Git, short};
use paddock_cli::outcome::{Sha, TreeId};
use paddock_cli::{CheckId, CliError, Exit, Outcome, Reason, Report, Subject, ToolVersion, UnixSeconds, Unmeasured, measure};
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
    /// `crates/paddock-workspace-hack` is not what `cargo hakari generate` writes for the result.
    WorkspaceHack,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Backwards => "backwards",
            Kind::DroppedHunk => "dropped-hunk",
            Kind::LostTest => "lost-test",
            Kind::WholeFile => "whole-file",
            Kind::WorkspaceHack => "workspace-hack",
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

/// A check's result: the shared [`Outcome`] (ADR 0089; a pass is made only by `measure`, with evidence about the merge) and the
/// merge-specific findings behind it, which carry what the shared finding does not (kind, file, excerpts).
#[derive(Debug)]
pub struct Analysis {
    /// Clean or findings with evidence, or not measured, or broken.
    pub outcome: Outcome,
    /// The findings, in the order of the detectors (empty unless the outcome is `Findings`).
    pub found: Vec<Finding>,
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
    /// The merge commit, when it exists (a branch checked before it is merged has none).
    pub commit: Option<String>,
    /// What to call the first parent (the target) and the second (the member) in messages: their names, or short hashes.
    pub target_name: String,
    /// See `target_name`.
    pub member_name: String,
}

impl Merge {
    /// A merge of `parents` (first parent first) with result `tree`: no message, no merge commit yet, and the parents' short ids as
    /// their names. Every construction site starts here and sets what it knows with the `with_*` methods, so a new field gets its
    /// default in one place and no site breaks.
    pub fn new(parents: Vec<String>, tree: impl Into<String>, label: impl Into<String>) -> Self {
        let name_of = |i: usize| parents.get(i).map(|p| short(p).to_string()).unwrap_or_default();
        let (target_name, member_name) = (name_of(0), name_of(1));
        Merge {
            parents,
            tree: tree.into(),
            message: String::new(),
            label: label.into(),
            commit: None,
            target_name,
            member_name,
        }
    }

    /// The merge commit's message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// The merge commit, when it exists.
    pub fn with_commit(mut self, commit: Option<String>) -> Self {
        self.commit = commit;
        self
    }

    /// The names of the target (first parent) and the member (second parent) for messages.
    pub fn with_names(mut self, target: impl Into<String>, member: impl Into<String>) -> Self {
        self.target_name = target.into();
        self.member_name = member.into();
        self
    }

    /// What a check of this merge is about: the merge commit and its tree, or (before it exists) the two parents joined by `+`.
    pub fn subject(&self) -> Subject {
        let sha = self.commit.clone().unwrap_or_else(|| self.parents.join("+"));
        Subject::Commit {
            sha: Sha::new(sha),
            tree: TreeId::new(self.tree.clone()),
        }
    }
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

/// Runs the detectors over `merge`, as one measurement: the shared outcome comes only from `measure`, so a merge is clean only
/// when every detector ran, and the findings (and a panic, which is a broken check) cannot be turned into a pass.
pub fn analyze(git: &Git, merge: &Merge) -> Analysis {
    let found = std::cell::RefCell::new(Vec::new());
    let outcome = measure(
        CheckId::new("merge-check"),
        merge.subject(),
        ToolVersion::new(concat!("paddock-dev ", env!("CARGO_PKG_VERSION"))),
        UnixSeconds::now(),
        || {
            let list = run(git, merge).map_err(|why| Unmeasured::NotMeasured(Reason::new(why)))?;
            let shared = list
                .iter()
                .map(|f| paddock_cli::Finding::failing(format!("[{}] {}: {}", f.kind.name(), f.subject, f.detail)))
                .collect();
            *found.borrow_mut() = list;
            Ok(shared)
        },
    );
    Analysis {
        outcome,
        found: found.into_inner(),
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
        if !git.has_commit(p)? {
            return Err(format!(
                "the parent {} is not in this clone (a shallow clone? run `git fetch --unshallow`)",
                git::short(p)
            ));
        }
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
    // Not a merge's own loss but a result that no longer generates: every merged branch can be right and the sum stale.
    match hakari::check(git, tree, &result_blobs, &hakari_program()) {
        Ok(found) => out.extend(found),
        // Alone, what hakari could not say is the merge's not-measured (exit 6); beside the detectors' findings (which fail, 5) it
        // is reported with them, never as a pass (ADR 0089).
        Err(why) if out.is_empty() => return Err(why),
        Err(why) => out.push(Finding {
            kind: Kind::WorkspaceHack,
            subject: "cargo hakari generate --diff".into(),
            detail: format!("NOT MEASURED: {why}"),
            excerpts: Vec::new(),
        }),
    }
    Ok(out)
}

/// The program that runs `hakari`: `cargo`, or `PADDOCK_MERGE_CHECK_CARGO` (tests point it at a stand-in).
fn hakari_program() -> String {
    std::env::var("PADDOCK_MERGE_CHECK_CARGO").unwrap_or_else(|_| "cargo".into())
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
    /// Run the member gate (`./dev check --pre-push --member-gate --base <merge>^1`) on the merge (the commit form only; the checkout must be
    /// the merge result). ADR 0098 WP-1; report-only until its first green run ([`gates::ENFORCED`]).
    pub gates: bool,
    /// Write the merge message with the `Reverted:` lines a re-land explains to this file ([`reland`]; findings only).
    pub write_reverted: Option<PathBuf>,
    /// `--old-base-anyway <reason>`: let a member that lacks the train's base through, and print the reason ([`stale_base`]).
    pub old_base_anyway: Option<String>,
    /// `--gates-base <ref>`: the base the member gate measures from, instead of the merge's first parent. A member's gate plans only
    /// that member's delta; the final gate of a train with `--gates-base origin/main` plans the whole train's rows, so the rows the push
    /// hook and the train-audits guard will ask for already have a pass on this tree and are reused, not run again (user, 2026-11-26).
    pub gates_base: Option<String>,
}

/// The step for `--write-reverted`: the lines a re-land explains, written with the message to `file`; says plainly when it tied none.
fn write_reverted_step(git: &Git, merge: &Merge, found: &[Finding], file: &std::path::Path) -> generator_step::Step {
    let step = |text: Vec<String>, data: serde_json::Value, exit| generator_step::Step { text, data, exit };
    let lines = match reland::lines(git, merge, found) {
        Ok(l) => l,
        Err(why) => {
            return step(
                vec![format!(
                    "::warning::--write-reverted could not read the history: {why}. Next step: write the `Reverted:` lines by hand."
                )],
                json!({"outcome": "not measured", "why": why}),
                Exit::Ok,
            );
        }
    };
    if lines.is_empty() {
        return step(
            vec![format!(
                "--write-reverted: no finding ties to a re-land of this member (a commit `revert: reapply ...` or `Reapply \"...\"` that undoes a revert of the target and changes the file); nothing written to {}.",
                file.display()
            )],
            json!({"outcome": "none", "lines": []}),
            Exit::Ok,
        );
    }
    let head = if merge.commit.is_some() {
        merge.message.trim_end().to_string()
    } else {
        format!("Merge branch '{}' into {}", merge.member_name, merge.target_name)
    };
    let text = format!("{head}\n\n{}\n", lines.join("\n"));
    if let Err(e) = std::fs::write(file, &text) {
        return step(
            vec![format!(
                "::error::--write-reverted could not write {}: {e}. Next step: choose a folder you can write to.",
                file.display()
            )],
            json!({"outcome": "not measured", "why": e.to_string()}),
            Exit::NotMeasured,
        );
    }
    let left = found.len() - lines.len().min(found.len());
    let mut out = vec![format!(
        "--write-reverted: {} of {} finding(s) are the re-land's own; message with their `Reverted:` lines written to {} (use it with `git commit --amend -F` or `git merge -F`, then run the check again; the other {left} need a person):",
        lines.len(),
        found.len(),
        file.display()
    )];
    out.extend(lines.iter().map(|l| format!("  {l}")));
    step(
        out,
        json!({"outcome": "written", "file": file.display().to_string(), "lines": lines}),
        Exit::Ok,
    )
}

fn resolve(git: &Git, req: &Request) -> Result<Merge, CliError> {
    let gap = |e: String| CliError::new(Exit::NotMeasured, not_measured(&e));
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
            let commit = git
                .text(&["rev-parse", &format!("{c}^{{commit}}")])
                .ok()
                .map(|t| t.trim().to_string());
            Ok(Merge::new(ids, tree, format!("merge {}", short(c)))
                .with_message(message)
                .with_commit(commit))
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
                .map_err(|e| {
                    let mut err = gap(format!("{b} does not merge cleanly into {t}, so there is no result to check: {e}"));
                    // The usual cause is a member built on a merge the target reverted: say so (report-only warnings).
                    let names = reverted_base::Names {
                        target: t,
                        member: b,
                        merged: false,
                    };
                    if let Some(step) = reverted_base::of_refs(git, &format!("{b} into {t}"), &first, &second, &names) {
                        err.message.push('\n');
                        err.message.push_str(&step.text.join("\n"));
                    }
                    err
                })?;
            let tree = tree.lines().next().unwrap_or("").trim().to_string();
            Ok(Merge::new(vec![first, second], tree, format!("{b} into {t}")).with_names(t.clone(), b.clone()))
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
    let hack = found.iter().filter(|f| f.kind == Kind::WorkspaceHack).count();
    let mut next = Vec::new();
    if hack < found.len() {
        next.push("restore what was dropped, or if it was meant, say so in the merge message with `Hunks-dropped: <path>: <reason>`, `Reverted: <path>: <reason>` or `Tests-removed: <name>: <reason>`".to_string());
    }
    if hack > 0 {
        next.push("run `cargo hakari generate` in crates/ and refresh the lockfiles that list a changed crate (a merge message cannot excuse a stale workspace-hack; if cargo-hakari could not run, install it with `cargo install --locked cargo-hakari` and run `cargo hakari generate --diff` yourself)".to_string());
    }
    lines.push(format!(
        "::error::{label}: {} finding(s). Next step: {}, then run it again.",
        found.len(),
        next.join("; and ")
    ));
    lines.join("\n")
}

fn clean_report(label: &str, result: &serde_json::Value) -> Report {
    Report::new(
        json!({"outcome": "clean", "merge": label, "result": result}),
        format!("merge check: {label}: nothing a parent added is missing from the result."),
    )
}

/// The steps after the detectors (the generator checks, the member gate), each under its own key of the JSON.
#[derive(Default)]
struct Steps(Vec<(&'static str, generator_step::Step)>);

impl Steps {
    fn push(&mut self, key: &'static str, step: Option<generator_step::Step>) {
        if let Some(s) = step {
            self.0.push((key, s));
        }
    }

    /// The steps' lines, one after the other.
    fn text(&self) -> Option<String> {
        (!self.0.is_empty()).then(|| self.0.iter().map(|(_, s)| s.text.join("\n")).collect::<Vec<_>>().join("\n"))
    }

    /// A stale file or a failing gate beats a step that could not look; `Ok` when none says more.
    fn exit(&self) -> Exit {
        let all: Vec<Exit> = self.0.iter().map(|(_, s)| s.exit).collect();
        [Exit::Findings, Exit::NotMeasured]
            .into_iter()
            .find(|e| all.contains(e))
            .unwrap_or(Exit::Ok)
    }

    fn data(&self) -> Vec<(&'static str, serde_json::Value)> {
        self.0.iter().map(|(k, s)| (*k, s.data.clone())).collect()
    }
}

/// Runs the check: the merge's label and its findings, or why it could not look.
/// The merge's label, its findings, the steps after the detectors and the outcome as the wire object (with its evidence).
type Checked = (String, Vec<Finding>, Steps, serde_json::Value);

/// Runs `a` and `b` at the same time and returns both results: the member gate beside the generated-file check, so the integrator waits
/// for the longer one. Each closure runs on a thread of its own inside one scope, so neither outlives the call.
fn both<A: Send, B: Send>(a: impl FnOnce() -> A + Send, b: impl FnOnce() -> B + Send) -> (A, B) {
    std::thread::scope(|s| {
        let hb = s.spawn(b);
        let ra = a();
        let rb = hb.join().unwrap_or_else(|e| std::panic::resume_unwind(e));
        (ra, rb)
    })
}

fn check(req: &Request) -> Result<Checked, CliError> {
    if req.old_base_anyway.as_deref().is_some_and(|r| r.trim().is_empty()) {
        return Err(CliError::usage("--old-base-anyway needs a reason.")
            .with_hint("Say why, for example --old-base-anyway \"hotfix, the newer main work is unrelated\"."));
    }
    let root = crate::gates::command::repo_root(req.root.as_deref())?;
    let git = Git::new(&root);
    let merge = resolve(&git, req)?;
    let analysis = analyze(&git, &merge);
    let result = analysis.outcome.json();
    match analysis.outcome {
        Outcome::NotMeasured(why) => Err(CliError::new(Exit::NotMeasured, not_measured(why.as_str()))),
        Outcome::CheckBroken(why) => Err(CliError::new(Exit::CheckBroken, not_measured(why.as_str()))),
        // Nothing here makes a known commit or a not-applicable result: a merge is always measured by `measure` above.
        Outcome::KnownCommit(_) | Outcome::NotApplicable(_) => Err(CliError::new(
            Exit::NotMeasured,
            not_measured("the merge check returned a result it does not make"),
        )),
        Outcome::Clean(_) | Outcome::Findings(_) => {
            let found = analysis.found;
            // The generated files are checked in the checkout, so only a merge that exists as a commit has one to check.
            // The generated-file check (`./dev gen check`) and the member gate read the merge's checkout and neither writes it, so with
            // `--gates` on a merge commit they run at the same time, not one after the other (user, 2026-11-26): the integrator waits
            // for the longer of the two instead of their sum ([`both`]).
            // (Each closure has its own `Git`: its answer caches are not shared between threads.)
            let gen_step = || {
                let git = Git::new(&root);
                generator_step::run(&root, &git, &merge, &crate::generators::run::Real::new(&root), &|l| {
                    crate::log_text::eline(l)
                })
            };
            let gates_step = || {
                let git = Git::new(&root);
                gates::run(&root, &git, &merge, &gates::Real, gates::ENFORCED, req.gates_base.as_deref())
            };
            let (mut pre_gen, mut pre_gates) = (None, None);
            if req.generators && req.gates && req.commit.is_some() {
                let (g, ga) = both(gen_step, gates_step);
                (pre_gen, pre_gates) = (Some(g), Some(ga));
            }
            let step = if let Some(s) = pre_gen.take() {
                s
            } else if !req.generators {
                Some(generator_step::not_run("--no-generators"))
            } else if req.commit.is_none() {
                Some(generator_step::not_run(
                    "a branch is checked before it is merged, so there is no checkout of the result; run `./dev merge check <merge-commit>` after merging",
                ))
            } else {
                gen_step()
            };
            let mut steps = Steps::default();
            steps.push("generators", step);
            // Report-only (ER-41): a member built on a merge the target reverted; never a finding, never an exit code.
            steps.push("reverted-base", reverted_base::run(&git, &merge));
            // A member that lacks the train's base is refused (stale_base::ENFORCED, user 2026-11-25) unless --old-base-anyway names a
            // reason; a train with no declared base is only noticed.
            if let [train, member] = merge.parents.as_slice() {
                match stale_base::problem(&git, train, member, &merge.target_name, &merge.member_name) {
                    Ok(Some(text)) => match (&req.old_base_anyway, stale_base::ENFORCED) {
                        (Some(reason), true) => steps.push(
                            "stale-base",
                            Some(generator_step::Step {
                                text: vec![stale_base::overridden(&text, reason)],
                                data: json!({"outcome": "overridden", "reason": reason}),
                                exit: Exit::Ok,
                            }),
                        ),
                        (None, true) => return Err(CliError::new(Exit::Findings, text)),
                        (_, false) => steps.push(
                            "stale-base",
                            Some(generator_step::Step {
                                text: vec![text],
                                data: json!({"outcome": "warning"}),
                                exit: Exit::Ok,
                            }),
                        ),
                    },
                    Ok(None) => {}
                    Err(why) => steps.push(
                        "stale-base",
                        Some(generator_step::Step {
                            text: vec![format!(
                                "::warning::stale-base check NOT MEASURED: {why}. Next step: run it again; this check is report-only."
                            )],
                            data: json!({"outcome": "not measured", "why": why}),
                            exit: Exit::Ok,
                        }),
                    ),
                }
            }
            if let Some(Ok(Some(notice))) = merge.parents.first().map(|t| stale_base::unknown(&git, t, &merge.target_name)) {
                steps.push(
                    "stale-base",
                    Some(generator_step::Step {
                        text: vec![notice],
                        data: json!({"outcome": "not run"}),
                        exit: Exit::Ok,
                    }),
                );
            }
            if let (Some(file), false) = (&req.write_reverted, found.is_empty()) {
                steps.push("write-reverted", Some(write_reverted_step(&git, &merge, &found, file)));
            }
            if req.gates {
                steps.push(
                    "gates",
                    Some(if req.commit.is_none() {
                        gates::not_run("a branch is checked before it is merged, so there is no checkout of the result; run `./dev merge check --gates <merge-commit>` after merging")
                    } else {
                        pre_gates.take().unwrap_or_else(gates_step)
                    }),
                );
            }
            Ok((merge.label, found, steps, result))
        }
    }
}

/// `./dev merge check`. Detector findings are an error (exit 5), so the text goes to standard error like every other failure.
/// A stale generated file is exit 5 and a generator check that could not look is exit 6, both with their `::error::` lines; the
/// member gate (`--gates`) adds its own rows and, once enforced, the same two exits.
pub fn command(req: &Request) -> Result<Report, CliError> {
    let (label, found, steps, result) = check(req)?;
    let lines = steps.text();
    let exit = steps.exit();
    if !found.is_empty() {
        let mut text = findings_text(&label, &found);
        if let Some(l) = lines {
            text.push('\n');
            text.push_str(&l);
        }
        return Err(CliError::new(Exit::Findings, text));
    }
    let mut report = clean_report(&label, &result);
    if let Some(l) = lines {
        report.text.push('\n');
        report.text.push_str(&l);
    }
    for (k, v) in steps.data() {
        report.data[k] = v;
    }
    Ok(if exit == Exit::Ok { report } else { report.with_exit(exit) })
}

/// `./dev merge check --json`: one object on standard output. Findings are `"ok": false, "kind": "findings"` with the
/// structured list (`kind`, `subject`, `detail`), and the text goes to standard error as without `--json`. `"generators"` holds
/// the generator checks' results when they ran, `"gates"` the member gate's.
pub fn command_json(req: &Request, out: &mut dyn std::io::Write, err: &mut dyn std::io::Write) -> i32 {
    match check(req) {
        Err(e) => paddock_cli::finish(paddock_cli::Format::Json, Err(e), out, err).code(),
        Ok((label, found, steps, result)) if found.is_empty() => {
            let exit = steps.exit();
            let mut report = clean_report(&label, &result);
            if let Some(l) = steps.text() {
                report.text.push('\n');
                report.text.push_str(&l);
            }
            for (k, v) in steps.data() {
                report.data[k] = v;
            }
            let report = if exit == Exit::Ok { report } else { report.with_exit(exit) };
            paddock_cli::finish(paddock_cli::Format::Json, Ok(report), out, err).code()
        }
        Ok((label, found, steps, result)) => {
            let mut text = findings_text(&label, &found);
            if let Some(l) = steps.text() {
                text.push('\n');
                text.push_str(&l);
            }
            let mut object = json!({
                "ok": false,
                "kind": "findings",
                "exit": Exit::Findings.code(),
                "merge": label,
                "result": result,
                "error": text,
                "findings": found.iter().map(|f| json!({"kind": f.kind.name(), "subject": f.subject, "detail": f.detail, "excerpts": f.excerpts})).collect::<Vec<_>>(),
                "generators": serde_json::Value::Null,
            });
            for (k, v) in steps.data() {
                object[k] = v;
            }
            let _ = writeln!(out, "{object}");
            let _ = writeln!(err, "error: {text}");
            Exit::Findings.code()
        }
    }
}

#[cfg(test)]
mod merge_tests {
    use super::{Merge, both};
    use std::sync::{Condvar, Mutex};
    use std::time::Duration;

    /// The member gate and the generated-file check run at the same time (user, 2026-11-26): each closure announces that it started and
    /// waits (bounded only as a hang detector, ER-12) until the other has started too. A serial `both` would never let the second start
    /// while the first waits, so the wait would end at its bound and the assertion below would fail: the test sees overlap by construction,
    /// not by timing. The results come back in call order.
    #[test]
    fn both_runs_the_two_closures_at_the_same_time_and_returns_their_results_in_order() {
        let started = (Mutex::new(0u32), Condvar::new());
        let meet = |me: &'static str| {
            let mut n = started.0.lock().unwrap();
            *n += 1;
            started.1.notify_all();
            let (n, timeout) = started.1.wait_timeout_while(n, Duration::from_secs(60), |n| *n < 2).unwrap();
            (me, *n, timeout.timed_out())
        };
        let (a, b) = both(|| meet("generators"), || meet("gates"));
        assert_eq!((a.0, b.0), ("generators", "gates"), "the results keep the call order");
        assert!(!a.2 && !b.2, "each saw the other start while it was itself in flight: {a:?} {b:?}");
        assert_eq!((a.1, b.1), (2, 2));
    }

    /// A panic in either closure reaches the caller (a failed step is never silently dropped).
    #[test]
    #[should_panic(expected = "the gates step broke")]
    fn a_panic_in_the_second_closure_reaches_the_caller() {
        let _ = both(|| 1, || -> u8 { panic!("the gates step broke") });
    }

    #[test]
    fn a_new_merge_names_its_parents_by_short_id_and_has_no_message_or_commit() {
        let m = Merge::new(vec!["1111111111111111".into(), "22222".into()], "tree", "label");
        assert_eq!((m.target_name.as_str(), m.member_name.as_str()), ("111111111", "22222"));
        assert_eq!(
            (m.message.as_str(), m.commit.as_deref(), m.tree.as_str(), m.label.as_str()),
            ("", None, "tree", "label")
        );
    }

    #[test]
    fn the_with_methods_set_only_their_own_field() {
        let m = Merge::new(vec!["a".into(), "b".into()], "t", "l")
            .with_message("msg")
            .with_commit(Some("c".into()))
            .with_names("main", "feat/x");
        assert_eq!((m.message.as_str(), m.commit.as_deref()), ("msg", Some("c")));
        assert_eq!((m.target_name.as_str(), m.member_name.as_str()), ("main", "feat/x"));
        assert_eq!(m.parents, ["a", "b"]);
    }
}
