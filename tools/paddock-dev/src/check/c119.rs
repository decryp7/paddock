//! C119: no workflow job runs `./dev` before its toolchain is set up (ER-9, ER-26). `./dev` hashes its own sources with `rustc -vV` and
//! builds itself with that compiler, so a step that runs it before `.gitea/actions/setup-rust` (or any action that goes through
//! `conformance/ci/kit.sh`, whose first act is `./dev --ensure-toolchain`) can hash and build with another compiler than the job's later
//! steps, or find none (run 8034; the CI-in-Rust WP-3 review: five root jobs called `./dev ci run` with no setup-rust). Judged per job of
//! `.gitea/workflows/*.yml`: the first step whose `run:` script runs `./dev` is a finding unless an earlier step of the job `uses:` a
//! local action that sets the toolchain up (an action whose text names `kit.sh`, `--ensure-toolchain` or `actions/setup-rust`) or runs
//! `kit.sh` / `--ensure-toolchain` itself. A justified case carries `conformance: dev-before-toolchain — <why>` on the line or above.
//! Report-only for now (ER-41).
//!
//! The port of `conformance/src/checks/C119-dev-before-toolchain.mjs`, finding for finding (`tests/it/check_c119.rs` compares them).

use super::finding::Finding;
use super::inline_waiver::{CommentView, InlineWaiver, Kind};
use super::jsre::rx;
use super::record::Precondition;
use super::{Check, CheckOutput, Control, Ctx};
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

static SETS_UP: LazyLock<Regex> = LazyLock::new(|| rx("kit\\.sh|--ensure-toolchain|actions/setup-rust"));
static RUNS_DEV: LazyLock<Regex> = LazyLock::new(|| rx("(?:^|[\\s;&|(])\\./dev(?:\\s|$)"));
static KIT_LINE: LazyLock<Regex> = LazyLock::new(|| rx("kit\\.sh|--ensure-toolchain"));
static JOBS_KEY: LazyLock<Regex> = LazyLock::new(|| rx("^jobs:\\s*(?:#.*)?$"));
static JOB_LINE: LazyLock<Regex> = LazyLock::new(|| rx("^ {2}[A-Za-z0-9_-]+:\\s*(?:#.*)?$"));
static STEPS_LINE: LazyLock<Regex> = LazyLock::new(|| rx("^ {4}steps:\\s*(?:#.*)?$"));
static USES_LOCAL: LazyLock<Regex> = LazyLock::new(|| rx("^uses:\\s*\\./\\.gitea/actions/([\\w-]+)"));
static RUN: LazyLock<Regex> = LazyLock::new(|| rx("^run:\\s*(.*)$"));
static COMMENT_TAIL: LazyLock<Regex> = LazyLock::new(|| rx("\\s+#.*$"));
static BLOCK_SCALAR: LazyLock<Regex> = LazyLock::new(|| rx("^[|>][+-]?[0-9]*\\s*$"));
static DASH_LEAD: LazyLock<Regex> = LazyLock::new(|| rx("^\\s*-\\s+"));

const FIX: &str = "Add `- uses: ./.gitea/actions/setup-rust` before the first step that runs `./dev` (the job then hashes and builds the tool with the pinned compiler). A justified case carries `# conformance: dev-before-toolchain — <why>`.";

/// The local actions that set the toolchain up: those whose text goes through kit.sh or names setup-rust.
pub fn toolchain_actions(actions: &[(String, String)]) -> BTreeSet<String> {
    let mut names: BTreeSet<String> = BTreeSet::from(["setup-rust".to_string()]);
    for (name, text) in actions {
        if SETS_UP.is_match(text) {
            names.insert(name.clone());
        }
    }
    names
}

/// One run of `./dev` before the toolchain: the 1-based line, the job and the waiver comment.
#[derive(Debug, PartialEq, Eq)]
pub struct Early {
    /// The 1-based line.
    pub line: usize,
    /// The job id.
    pub job: String,
    /// The waiver comment near the run.
    pub waived: InlineWaiver,
}

fn indent_of(l: &str) -> usize {
    l.chars().count() - l.trim_start().chars().count()
}

/// The `./dev` runs before any toolchain setup in one workflow text, one per job at most.
pub fn dev_before_toolchain(text: &str, setup: &BTreeSet<String>) -> Vec<Early> {
    let lines: Vec<&str> = text.split('\n').collect();
    let view = CommentView::of(text, Kind::Yaml);
    let mut out = Vec::new();
    let (mut in_jobs, mut in_steps, mut ready) = (false, false, false);
    let mut job: Option<String> = None;
    let mut script_indent: Option<usize> = None;
    let check = |script: &str, i: usize, job: &str, ready: &mut bool, out: &mut Vec<Early>| {
        if script.trim_start().starts_with('#') {
            return;
        }
        if KIT_LINE.is_match(script) {
            *ready = true;
            return;
        }
        if *ready || !RUNS_DEV.is_match(script) {
            return;
        }
        let waived = view.waiver(i + 1, "dev-before-toolchain");
        out.push(Early {
            line: i + 1,
            job: job.to_string(),
            waived,
        });
        *ready = true;
    };
    for (i, raw) in lines.iter().enumerate() {
        let l = raw.strip_suffix('\r').unwrap_or(raw);
        let blank = l.trim().is_empty();
        if !blank && indent_of(l) == 0 && !l.trim_start().starts_with('#') {
            in_jobs = JOBS_KEY.is_match(l);
            job = None;
            in_steps = false;
            script_indent = None;
            continue;
        }
        if !in_jobs || blank {
            continue;
        }
        let ind = indent_of(l);
        if let Some(si) = script_indent
            && ind > si
        {
            if !l.trim_start().starts_with('#') {
                check(l, i, job.as_deref().unwrap_or(""), &mut ready, &mut out);
            }
            continue;
        }
        script_indent = None;
        if ind == 2 && JOB_LINE.is_match(l) {
            job = Some(l.trim().split(':').next().unwrap_or("").to_string());
            in_steps = false;
            ready = false;
            continue;
        }
        let Some(j) = job.clone() else { continue };
        if ind == 4 && STEPS_LINE.is_match(l) {
            in_steps = true;
            continue;
        }
        if ind == 4 && !l.trim_start().starts_with('#') {
            in_steps = false;
            continue;
        }
        if !in_steps {
            continue;
        }
        let body = DASH_LEAD.replace(l, "").trim_start().to_string();
        if let Some(u) = USES_LOCAL.captures(&body)
            && setup.contains(&u[1])
        {
            ready = true;
            continue;
        }
        if let Some(r) = RUN.captures(&body) {
            let v = COMMENT_TAIL.replace(&r[1], "").to_string();
            if BLOCK_SCALAR.is_match(&v) {
                script_indent = Some(ind);
                continue;
            }
            check(&v, i, &j, &mut ready, &mut out);
        }
    }
    out
}

/// The check.
pub struct C119;

fn workflow(f: &str) -> bool {
    f.strip_prefix(".gitea/workflows/")
        .is_some_and(|r| !r.contains('/') && (r.ends_with(".yml") || r.ends_with(".yaml")))
}

fn action_name(f: &str) -> Option<&str> {
    let r = f.strip_prefix(".gitea/actions/")?;
    let (name, file) = r.split_once('/')?;
    (file == "action.yml" || file == "action.yaml").then_some(name)
}

impl Check for C119 {
    fn id(&self) -> &'static str {
        "C119"
    }

    fn rule(&self) -> u32 {
        3
    }

    fn scope(&self) -> super::CheckScope {
        super::CheckScope::Root
    }

    fn mode(&self) -> super::Mode {
        super::Mode::ReportOnly
    }

    fn title(&self) -> &'static str {
        "no workflow job runs ./dev before its toolchain is set up"
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    ".gitea/workflows/bad.yml",
                    "jobs:\n  a:\n    steps:\n      - uses: actions/checkout@v4\n      - run: ./dev ci run x y\n",
                ),
                (
                    ".gitea/workflows/good.yml",
                    "jobs:\n  a:\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n      - run: ./dev ci run x y\n",
                ),
                (
                    ".gitea/workflows/waived.yml",
                    "jobs:\n  a:\n    steps:\n      # conformance: dev-before-toolchain — the image's compiler is the job's only one\n      - run: ./dev ci run x y\n",
                ),
            ],
            expect: vec![(".gitea/workflows/bad.yml", 5)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        if ctx.config().app() != "paddock" {
            return CheckOutput::not_applicable("the check judges the Paddock repository (--app paddock)");
        }
        let files = ctx.files();
        let workflows: Vec<&String> = files.iter().filter(|f| workflow(f)).collect();
        let actions: Vec<(String, String)> = files
            .iter()
            .filter_map(|f| action_name(f).map(|n| (n.to_string(), ctx.read(f).map(|t| t.to_string()).unwrap_or_default())))
            .collect();
        let setup = toolchain_actions(&actions);
        let (mut findings, mut probes, mut completed) = (Vec::new(), 0, true);
        for f in workflows {
            let Some(text) = ctx.read(f) else {
                completed = false;
                continue;
            };
            probes += 1;
            for e in dev_before_toolchain(&text, &setup) {
                let message = |bare: &str| {
                    format!(
                        "job `{}` runs ./dev before its toolchain is set up{bare} (ER-9: ./dev hashes and builds itself with the job's compiler)",
                        e.job
                    )
                };
                let site = Finding::warn("C119", 3, f, e.line, message(""), FIX);
                findings.push(e.waived.apply(site, || {
                    Finding::warn("C119", 3, f, e.line, message(" (the waiver comment has no reason)"), FIX)
                }));
            }
        }
        CheckOutput {
            findings,
            probes,
            completed,
            precondition: Precondition {
                met: ctx.tree_files().iter().any(|f| workflow(f)),
                detail: "the tree has a workflow file".into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> BTreeSet<String> {
        toolchain_actions(&[
            ("kept".into(), "run: bash conformance/ci/kit.sh host x".into()),
            ("plain".into(), "run: echo".into()),
        ])
    }

    fn lines(text: &str) -> Vec<usize> {
        dev_before_toolchain(text, &setup()).iter().map(|e| e.line).collect()
    }

    #[test]
    fn a_dev_run_before_the_toolchain_is_found_once_per_job_and_a_setup_action_clears_it() {
        let bad = "jobs:\n  a:\n    steps:\n      - uses: actions/checkout@v4\n      - run: ./dev x\n      - run: ./dev y\n  b:\n    steps:\n      - uses: ./.gitea/actions/kept\n      - run: ./dev x\n";
        assert_eq!(lines(bad), vec![5]);
        let ok = "jobs:\n  a:\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n      - run: |\n          ./dev x\n";
        assert!(lines(ok).is_empty());
        let block = "jobs:\n  a:\n    steps:\n      - uses: ./.gitea/actions/plain\n      - name: n\n        run: |\n          set -e\n          ./dev x\n";
        assert_eq!(lines(block), vec![8]);
    }

    #[test]
    fn a_waiver_needs_a_reason_and_a_comment_or_another_tool_is_no_run() {
        let w =
            "jobs:\n  a:\n    steps:\n      # conformance: dev-before-toolchain — the image has the only compiler\n      - run: ./dev x\n";
        let found = dev_before_toolchain(w, &setup());
        assert_eq!(found[0].waived, InlineWaiver::Reason("the image has the only compiler".into()));
        let bare = "jobs:\n  a:\n    steps:\n      # conformance: dev-before-toolchain\n      - run: ./dev x\n";
        assert_eq!(dev_before_toolchain(bare, &setup())[0].waived, InlineWaiver::NoReason);
        assert!(lines("jobs:\n  a:\n    steps:\n      # ./dev x\n      - run: ./devx y\n      - run: echo ./dev\n").len() == 1);
    }
}
