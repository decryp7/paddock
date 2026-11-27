//! `mutants gate`: mutation-testing of the diff, ci.yml's `mutants` job (user decision 127, 2026-11-27; moved from the member gate to CI the same day).
//!
//! The class it catches: a test or a guard that never reaches the production code it claims to check (a landing test that rebuilt the
//! planner's wiring itself; `config::removed`, a guard nothing called; a producer test that looked at an action `ci.yml` does not use).
//! `cargo mutants --in-diff` changes each function in the changed lines of `tools/paddock-dev` and `crates/` in turn (a return value
//! replaced, an operator swapped) and runs the package's tests: a mutant no test fails on is MISSED, and the function it names is a
//! finding. Timeouts and unviable mutants are counted apart and never read as caught (ER-33).
//!
//! It starts report-only (ER-41): [`MODE`] makes every finding a `::warning::` and the exit 0; the review date of that phase is 2026-12-11 (ER-41: flip
//! [`MODE`] to Gate or delete the job). Whatever the mode, a thing that could not be measured (the pinned tool is
//! not installed, the unmutated tree fails its tests, the run was cut short) is printed as `not measured` and counted, never as a
//! clean run; as a gate it exits `NotMeasured`, and a failed positive control exits `CheckBroken`.
//!
//! The positive control runs first, on every run ([`CONTROL_LIB`]): a one-function crate whose test does not reach the second
//! function must show that function as MISSED and the first as caught, else the row says `check broken` and looks at nothing.
//!
//! Everything that starts a program goes through [`Tool`], so the parsing, the cap and the control are tested on canned output and the
//! real cargo-mutants only in the row. The mutants run in place in a checkout of the committed head (a local `git clone` with its whole history) under the run's own scratch folder `<dev-targets>/mutants/run-<pid>-<time>`: cargo-mutants' default copy
//! of one workspace folder cannot run these tests (they read the repository around the workspace, and the `../../crates` path
//! dependencies of dev-dependencies are not rewritten), and mutating the member's own worktree would break the rows that run beside
//! this one. The folder has an owner record and is removed through [`crate::safe_remove`] when the run ends, whatever its outcome.

use crate::locks_guard::Mode;
use paddock_cli::{CliError, Exit, Report};
use paddock_proc::{Limits, command, run_bounded};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Whether a finding fails the run. Report-only (ER-41) until the first green train run of ci.yml's `mutants` job on the real tree is
/// recorded here, by run id, in the same change that sets this to [`Mode::Gate`] and removes the job's `continue-on-error`. Review date
/// 2026-12-11: flip it, or delete the job.
pub const MODE: Mode = Mode::ReportOnly;

/// The one cargo-mutants version the row runs (ER-26: an upgrade is one deliberate change plus a full run). It is not installed from
/// here (ER-45: a probe installs nothing): a different version is `not measured`, with the install command in the message.
pub const PIN: &str = "27.1.0";

/// The Cargo workspaces whose changed functions are mutated, with the folder cargo-mutants runs in.
pub const WORKSPACES: &[&str] = &["tools/paddock-dev", "crates"];

/// The nextest profile the mutants' test runs use: CI's own `ci` profile (both workspaces define it), so a mutant is judged by the
/// tests CI runs, with their bounds.
pub const NEXTEST_PROFILE: &str = "ci";

/// The hang detector of one mutant's build or test run, in seconds (cargo-mutants `--timeout`, `--build-timeout`): never a speed limit
/// (ER-12). The measured test run is about 150 s (nextest's `ci` profile bounds each test itself) and the build 8 s to 51 s; 900 s is
/// six times the longest (docs/benchmarks.md, "Mutation testing of the diff in the member gate").
pub const MUTANT_TIMEOUT_SECS: u64 = 900;

/// The most mutants one run tests, over all workspaces. A diff with more runs one slice of consecutive mutants in source order
/// (cargo-mutants `--shard 0/k`, `k = ceil(n / cap)`) and the rest is reported as not measured. Measured (docs/benchmarks.md, "Mutation
/// testing of the diff in the member gate"): in `tools/paddock-dev` the unmutated build and tests take 4 minutes and each mutant 2.5 to
/// 4 minutes (8 s of build, the package's tests the rest), so 12 mutants took 3162 s on a busy machine (train y's `fix/gate-row-stall`);
/// in `crates` a mutant took 13 s. 12 fits the run limit and the 60-minute row cap of a push step.
pub const MAX_MUTANTS: usize = 12;

/// The most mutants one planned shard (`--shard k/n`, a leg of ci.yml's `mutants` matrix) tests: a leg has a job of its own, so it is
/// held by the run limit and not by a push step's row cap. Measured on the trial of 2026-11-27 (docs/benchmarks.md): a shard of the
/// four-way split saw 25 mutants for the whole paddock-dev diff of this branch, which the cap of 12 refused; eight legs of at most
/// this many hold such a diff.
pub const MAX_PER_SHARD: usize = 16;

/// The whole run's hang limit: the baseline, the control and [`MAX_MUTANTS`] mutants.
pub const RUN_LIMIT: Duration = Duration::from_secs(3300);

/// The positive control's crate: `reached` has a test, `unreached` has none. Written to the run's scratch folder; path-only, no
/// dependency, so it needs no registry and no network.
pub const CONTROL_MANIFEST: &str =
    "[package]\nname = \"mutants-control\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n";

/// The control's source. The function the control expects to be missed is [`CONTROL_MISSED`].
/// (Written in two pieces so that the check that reads test regions by their attribute does not take this constant for a test module.)
pub const CONTROL_LIB: &str = concat!(
    "pub fn reached(n: u32) -> u32 {\n    n + 1\n}\n\npub fn unreached(n: u32) -> u32 {\n    n * 2\n}\n\n#[cfg(",
    "test)]\nmod tests {\n    #[test]\n    fn reached_adds_one() {\n        assert_eq!(super::reached(1), 2);\n    }\n}\n"
);

/// The function of [`CONTROL_LIB`] no test reaches.
pub const CONTROL_MISSED: &str = "unreached";

/// What cargo-mutants said about one mutant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// A test failed with the mutation applied: the tests reach the code and check it.
    Caught,
    /// Every test passed with the mutation applied: a finding.
    Missed,
    /// The tests did not end within [`MUTANT_TIMEOUT_SECS`]: nothing is proven.
    Timeout,
    /// The mutated code did not compile: nothing is proven.
    Unviable,
}

/// One mutant and its verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tested {
    /// The file, relative to the workspace folder.
    pub file: String,
    /// The line of the mutation.
    pub line: u64,
    /// The enclosing function.
    pub function: String,
    /// The mutation in words (`replace unreached -> u32 with 0`).
    pub mutation: String,
    /// How the tests judged it.
    pub verdict: Verdict,
}

/// How the unmutated tree did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Baseline {
    /// Its tests passed: the mutants' verdicts mean something.
    Passed,
    /// Its tests failed: no verdict means anything.
    Failed,
    /// The run did not record one.
    Absent,
}

/// `outcomes.json` of a cargo-mutants run, parsed once (ER-42: through the JSON parser, never by text matching).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcomes {
    /// The unmutated tree's result.
    pub baseline: Baseline,
    /// Every mutant that has a verdict.
    pub mutants: Vec<Tested>,
}

impl Outcomes {
    /// How many mutants have `v`.
    pub fn count(&self, v: Verdict) -> usize {
        self.mutants.iter().filter(|m| m.verdict == v).count()
    }
}

/// Parses `outcomes.json`. A summary this parser does not know is an error, never skipped: a new cargo-mutants state must be read on
/// purpose (ER-33).
///
/// # Errors
/// The text is not the JSON shape cargo-mutants writes, or a mutant has a summary this version does not know.
pub fn parse_outcomes(text: &str) -> Result<Outcomes, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("outcomes.json is not JSON: {e}"))?;
    let list = v["outcomes"].as_array().ok_or("outcomes.json has no `outcomes` list")?;
    let mut baseline = Baseline::Absent;
    let mut mutants = Vec::new();
    for o in list {
        let summary = o["summary"].as_str().ok_or("an outcome has no `summary`")?;
        if o["scenario"].as_str() == Some("Baseline") {
            baseline = match summary {
                "Success" => Baseline::Passed,
                _ => Baseline::Failed,
            };
            continue;
        }
        let m = &o["scenario"]["Mutant"];
        let verdict = match summary {
            "CaughtMutant" => Verdict::Caught,
            "MissedMutant" => Verdict::Missed,
            "Timeout" => Verdict::Timeout,
            "Unviable" => Verdict::Unviable,
            other => return Err(format!("a mutant has the summary `{other}`, which this tool version does not know")),
        };
        let name = m["name"].as_str().ok_or("a mutant has no `name`")?;
        mutants.push(Tested {
            file: m["file"].as_str().ok_or("a mutant has no `file`")?.to_string(),
            line: m["span"]["start"]["line"].as_u64().ok_or("a mutant has no line")?,
            function: m["function"]["function_name"].as_str().unwrap_or("(no function)").to_string(),
            mutation: mutation_of(name),
            verdict,
        });
    }
    Ok(Outcomes { baseline, mutants })
}

/// The mutation of a mutant's name: `src/a.rs:6:5: replace f -> u32 with 0` is `replace f -> u32 with 0`.
fn mutation_of(name: &str) -> String {
    let mut parts = name.splitn(4, ':');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(_), Some(l), Some(c), Some(rest)) if l.trim().parse::<u64>().is_ok() && c.trim().parse::<u64>().is_ok() => {
            rest.trim().to_string()
        }
        _ => name.to_string(),
    }
}

/// The annotation of a finding in `mode`: the function, the mutation and what it means.
pub fn annotation(ws: &str, m: &Tested, mode: Mode) -> String {
    let level = if mode == Mode::Gate { "error" } else { "warning" };
    format!(
        "::{level} file={ws}/{},line={}::mutants: MISSED in `{}`: {}. Every test passed with that change, so no test reaches this code or checks its result. Next step: add a test that fails on it, or delete the code.",
        m.file, m.line, m.function, m.mutation
    )
}

/// One program run's result.
#[derive(Clone, Debug, Default)]
pub struct Ran {
    /// The exit code; `None` when it did not end by itself.
    pub code: Option<i32>,
    /// Standard output.
    pub stdout: String,
    /// The tail of standard error, for a message.
    pub stderr: String,
}

/// What the row starts: cargo-mutants and git. Fake in the tests.
pub trait Tool {
    /// `cargo mutants <args>` in `cwd`, scratch files under `tmp`, bounded by [`RUN_LIMIT`].
    fn mutants(&self, cwd: &Path, args: &[String], tmp: &Path) -> Result<Ran, String>;
    /// The unified diff of the Rust sources under `ws` between `base` and the committed `HEAD`, paths relative to `ws`.
    fn diff(&self, root: &Path, ws: &str, base: &str) -> Result<String, String>;
    /// A self-contained copy of the commit `head` of the repository at `root` in the new folder `tree`, with a git repository of its own
    /// (the tests read the repository they run in), where mutants may be applied in place.
    fn checkout(&self, root: &Path, head: &str, tree: &Path) -> Result<(), String>;
}

/// The real programs.
pub struct Real;

impl Tool for Real {
    fn mutants(&self, cwd: &Path, args: &[String], tmp: &Path) -> Result<Ran, String> {
        let mut cmd = command("cargo");
        cmd.arg("mutants")
            .args(args)
            .current_dir(cwd)
            .env("TMPDIR", scratch_tmp(tmp))
            .env("RUSTUP_AUTO_INSTALL", "0");
        let out = run_bounded(cmd, Limits::new(RUN_LIMIT).max_output(1 << 26)).map_err(|e| format!("cargo mutants could not run: {e}"))?;
        Ok(Ran {
            code: exit_code(&out),
            stdout: out.stdout_text(),
            stderr: tail(&out.stderr_text()),
        })
    }

    fn diff(&self, root: &Path, ws: &str, base: &str) -> Result<String, String> {
        // Committed `HEAD`, not the working tree: the tree the mutants run in is a checkout of `HEAD` ([`Tool::checkout`]).
        let mut cmd = crate::git_env::git();
        cmd.args(["diff", "--relative", "--no-color", base, "HEAD", "--", "*.rs"])
            .current_dir(root.join(ws));
        let out = run_bounded(cmd, Limits::new(Duration::from_secs(120)).max_output(1 << 26))
            .map_err(|e| format!("git diff could not run: {e}"))?;
        if !out.success() {
            return Err(format!("git diff {base} HEAD failed: {}", tail(&out.stderr_text())));
        }
        Ok(out.stdout_text())
    }

    fn checkout(&self, root: &Path, head: &str, tree: &Path) -> Result<(), String> {
        // A local clone (objects hardlinked, so it is quick), with the whole history: tests replay trains from `git log`. Its `origin` is
        // the repository at `root`, whose branches it holds as remote branches, which is what the safe-remove proof reads. Then `head`.
        let step = Limits::new(Duration::from_secs(900)).max_output(1 << 22);
        let mut clone = crate::git_env::git();
        clone.args(["clone", "--quiet", "--local", "--no-checkout"]).arg(root).arg(tree);
        let out = run_bounded(clone, step).map_err(|e| format!("git clone could not run: {e}"))?;
        if !out.success() {
            return Err(format!("git clone failed: {}", tail(&out.stderr_text())));
        }
        let mut checkout = crate::git_env::git();
        checkout.current_dir(tree).args(["checkout", "--quiet", "--detach", head]);
        let out = run_bounded(checkout, step).map_err(|e| format!("git checkout could not run: {e}"))?;
        if !out.success() {
            return Err(format!("git checkout {head} failed in the clone: {}", tail(&out.stderr_text())));
        }
        Ok(())
    }
}

/// The exit code of a bounded run; `None` when it was ended (timeout, output cap) or was killed by a signal.
fn exit_code(out: &paddock_proc::Output) -> Option<i32> {
    match out.outcome {
        paddock_proc::Outcome::Exited(s) => s.code(),
        _ => None,
    }
}

fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(8)..].join(" | ")
}

/// What a whole run found.
#[derive(Debug, Default)]
pub struct Summary {
    /// The `::warning::` or `::error::` lines of the MISSED mutants.
    pub findings: Vec<String>,
    /// Mutants tested, caught, missed, timed out, unviable, over all workspaces.
    pub tested: usize,
    /// Caught mutants.
    pub caught: usize,
    /// Missed mutants.
    pub missed: usize,
    /// Mutants that timed out: not a pass.
    pub timeouts: usize,
    /// Mutants that did not compile: not a pass.
    pub unviable: usize,
    /// Mutants the cap left out: not measured.
    pub over_cap: usize,
    /// Why the row could not look (at least in part), one line each: the run is not a clean pass.
    pub not_measured: Vec<String>,
    /// Why the check itself cannot be trusted.
    pub broken: Option<String>,
    /// One line per workspace for the table.
    pub lines: Vec<String>,
}

/// The mutants `--list --json` says an `--in-diff` run would test: how many.
fn listed(text: &str) -> Result<usize, String> {
    serde_json::from_str::<Value>(text)
        .map_err(|e| format!("`--list --json` is not JSON: {e}"))?
        .as_array()
        .map(Vec::len)
        .ok_or_else(|| "`--list --json` is not a list".to_string())
}

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// The installed cargo-mutants version from `--version` (`cargo-mutants 27.1.0`).
fn version_of(text: &str) -> Option<&str> {
    text.split_whitespace().nth(1)
}

/// What a person needs to see why the unmutated tree did not pass, from the baseline's log (the scratch folder is gone by then): the
/// first ten failing lines (`FAIL`, `TIMEOUT`, `SIG...` of nextest, and `error` lines), the `Summary` line, and the last five lines
/// of the log, each at most 200 characters, with terminal colours removed.
pub fn baseline_failures(log: &str) -> String {
    let plain: Vec<String> = log
        .lines()
        .map(strip_colours)
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let failing = |l: &&String| {
        ["FAIL ", "FAIL[", "TIMEOUT", "SIG", "error", "Summary"]
            .iter()
            .any(|p| l.starts_with(p))
            || l.contains("FAIL [")
    };
    let mut picked: Vec<&String> = plain.iter().filter(failing).take(10).collect();
    for l in plain.iter().rev().take(5).rev() {
        if !picked.contains(&l) {
            picked.push(l);
        }
    }
    if picked.is_empty() {
        return "(the baseline log is empty)".to_string();
    }
    picked
        .iter()
        .map(|l| l.chars().take(200).collect::<String>())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// `text` without ANSI escape sequences (`ESC [ ... letter`).
fn strip_colours(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The positive control: [`CONTROL_LIB`] must show [`CONTROL_MISSED`] as MISSED and a caught mutant, else the check cannot see
/// what it is for. `Err` is the cause.
fn control(tool: &dyn Tool, scratch: &Path) -> Result<(), String> {
    let dir = scratch.join("control");
    std::fs::create_dir_all(dir.join("src")).map_err(|e| format!("the control folder cannot be made: {e}"))?;
    std::fs::write(dir.join("Cargo.toml"), CONTROL_MANIFEST).map_err(|e| format!("the control cannot be written: {e}"))?;
    std::fs::write(dir.join("src/lib.rs"), CONTROL_LIB).map_err(|e| format!("the control cannot be written: {e}"))?;
    let out = dir.join("out");
    let a = args(&[
        "--no-shuffle",
        "--jobs",
        "1",
        "--timeout",
        "120",
        "--build-timeout",
        "120",
        "--output",
        &out.to_string_lossy(),
    ]);
    let ran = tool.mutants(&dir, &a, scratch)?;
    if ran.code.is_none() {
        return Err(format!("the control run did not end by itself: {}", ran.stderr));
    }
    let text = std::fs::read_to_string(out.join("mutants.out/outcomes.json")).map_err(|e| {
        format!(
            "the control wrote no outcomes.json ({e}; cargo-mutants exit {:?}: {})",
            ran.code, ran.stderr
        )
    })?;
    let o = parse_outcomes(&text)?;
    let missed = o
        .mutants
        .iter()
        .any(|m| m.verdict == Verdict::Missed && m.function == CONTROL_MISSED);
    if !missed || o.count(Verdict::Caught) == 0 {
        return Err(format!(
            "the control crate (`{CONTROL_MISSED}` has no test) showed {} missed and {} caught mutants: the probe cannot tell a tested function from an untested one",
            o.count(Verdict::Missed),
            o.count(Verdict::Caught)
        ));
    }
    Ok(())
}

/// Runs the control, then mutates the changed functions of every workspace in [`WORKSPACES`] between `base` and the commit `head` of
/// the repository at `root`, in a checkout of `head` under the scratch folder `scratch` (mutants are applied in place there, never in
/// `root`). The run's findings and what it could not measure are in the [`Summary`].
pub fn measure(tool: &dyn Tool, root: &Path, head: &str, base: &str, shard: Option<(u32, u32)>, scratch: &Path, mode: Mode) -> Summary {
    let mut s = Summary::default();
    match tool.mutants(root, &args(&["--version"]), scratch) {
        Ok(r) if version_of(&r.stdout) == Some(PIN) => {}
        Ok(r) => {
            s.not_measured.push(format!(
                "cargo-mutants {} is installed and the pin is {PIN}: install it with `cargo install cargo-mutants --version {PIN} --locked` (an upgrade is one change of mutants_gate::PIN)",
                version_of(&r.stdout).unwrap_or("is not")
            ));
            return s;
        }
        Err(e) => {
            s.not_measured.push(format!(
                "cargo-mutants {PIN} cannot be started ({e}): install it with `cargo install cargo-mutants --version {PIN} --locked`"
            ));
            return s;
        }
    }
    if let Err(why) = control(tool, scratch) {
        s.broken = Some(why);
        return s;
    }
    let tree = scratch.join("tree");
    let mut budget = MAX_MUTANTS;
    let mut checked_out = false;
    for ws in WORKSPACES {
        let diff = match tool.diff(root, ws, base) {
            Ok(d) => d,
            Err(e) => {
                s.not_measured.push(format!("{ws}: the diff against {base} cannot be read ({e})"));
                continue;
            }
        };
        if diff.trim().is_empty() {
            s.lines.push(format!("{ws}: no Rust source changed since {base}"));
            continue;
        }
        let slug = ws.replace('/', "-");
        let diff_file = scratch.join(format!("{slug}.diff"));
        if let Err(e) = std::fs::write(&diff_file, &diff) {
            s.not_measured.push(format!("{ws}: the diff cannot be written ({e})"));
            continue;
        }
        if !checked_out {
            if let Err(e) = tool.checkout(root, head, &tree) {
                s.not_measured
                    .push(format!("the checkout of {head} for the mutants cannot be made ({e})"));
                return s;
            }
            checked_out = true;
        }
        let dir = tree.join(ws);
        let diff_arg = diff_file.to_string_lossy().into_owned();
        let shard_arg = shard.map(|(k, n)| format!("{k}/{n}"));
        let mut list_args = args(&["--in-diff", &diff_arg, "--list", "--json"]);
        list_args.extend(shard_arg.iter().flat_map(|a| args(&["--shard", a])));
        let listing = tool.mutants(&dir, &list_args, scratch);
        let n = match listing.as_ref().map_err(Clone::clone).and_then(|r| listed(&r.stdout)) {
            Ok(n) => n,
            Err(e) => {
                s.not_measured.push(format!("{ws}: the mutants of the diff cannot be listed ({e})"));
                continue;
            }
        };
        if n == 0 {
            s.lines.push(format!("{ws}: the changed lines hold no function to mutate"));
            continue;
        }
        // A planned shard is whole or not measured (the plan's shard count is what bounds it); an unsharded run takes the cap's slice.
        let take = if shard.is_some() {
            if n > MAX_PER_SHARD { 0 } else { n }
        } else {
            n.min(budget)
        };
        if take == 0 {
            s.over_cap += n;
            s.lines.push(format!(
                "{ws}: {n} mutants not measured (the cap of {MAX_MUTANTS} {})",
                if shard.is_some() {
                    "per shard: plan more shards"
                } else {
                    "is used"
                }
            ));
            continue;
        }
        let out = scratch.join(format!("{slug}-out"));
        let mut a = args(&[
            "--in-diff",
            &diff_arg,
            "--in-place",
            "--no-shuffle",
            "--timeout",
            &MUTANT_TIMEOUT_SECS.to_string(),
            "--build-timeout",
            &MUTANT_TIMEOUT_SECS.to_string(),
            "--test-tool",
            "nextest",
            "--output",
            &out.to_string_lossy(),
        ]);
        if let Some(sh) = &shard_arg {
            a.extend(args(&["--shard", sh]));
        } else if take < n {
            // The first `take` in source order: shard 0 of `ceil(n / take)` slices of consecutive mutants (the default method).
            a.extend(args(&["--shard", &format!("0/{}", n.div_ceil(take))]));
        }
        a.extend(args(&["--", "--profile", NEXTEST_PROFILE]));
        let ran = match tool.mutants(&dir, &a, scratch) {
            Ok(r) => r,
            Err(e) => {
                s.not_measured.push(format!("{ws}: {e}"));
                continue;
            }
        };
        // Cut short (the run limit or a signal): the mutants that have a verdict are read, the rest is said not to be measured.
        let cut_short = ran.code.is_none();
        let parsed = std::fs::read_to_string(out.join("mutants.out/outcomes.json"))
            .map_err(|e| format!("no outcomes.json ({e}; cargo-mutants exit {:?}: {})", ran.code, ran.stderr))
            .and_then(|t| parse_outcomes(&t));
        let o = match parsed {
            Ok(o) => o,
            Err(e) => {
                s.not_measured.push(format!("{ws}: {e}"));
                continue;
            }
        };
        if o.baseline != Baseline::Passed {
            s.not_measured.push(format!(
                "{ws}: the tests of the unmutated tree did not pass (baseline {:?}), so no mutant's verdict means anything; fix the tests, then re-run `./dev mutants gate`. First failures: {}",
                o.baseline,
                baseline_failures(&std::fs::read_to_string(out.join("mutants.out/log/baseline.log")).unwrap_or_default())
            ));
            continue;
        }
        if cut_short {
            s.not_measured.push(format!(
                "{ws}: cargo-mutants was cut short by the run limit of {} s or a signal after {} of {n} mutants; the rest is not measured: {}",
                RUN_LIMIT.as_secs(),
                o.mutants.len(),
                ran.stderr
            ));
        }
        let ran_n = o.mutants.len();
        budget = budget.saturating_sub(ran_n);
        let (c, m, t, u) = (
            o.count(Verdict::Caught),
            o.count(Verdict::Missed),
            o.count(Verdict::Timeout),
            o.count(Verdict::Unviable),
        );
        s.tested += ran_n;
        s.caught += c;
        s.missed += m;
        s.timeouts += t;
        s.unviable += u;
        s.over_cap += n.saturating_sub(ran_n);
        s.findings.extend(
            o.mutants
                .iter()
                .filter(|x| x.verdict == Verdict::Missed)
                .map(|x| annotation(ws, x, mode)),
        );
        s.lines.push(format!(
            "{ws}: {} mutants of {n}: {c} caught, {m} MISSED, {t} timed out (not a pass), {u} unviable (not a pass){}",
            ran_n,
            if ran_n < n {
                format!(", {} not measured (cap)", n - ran_n)
            } else {
                String::new()
            }
        ));
    }
    s
}

/// The exit and the text of a [`Summary`] in `mode`.
pub fn report(s: &Summary, mode: Mode, started: std::time::Instant) -> Report {
    let mut text = String::new();
    for f in &s.findings {
        text.push_str(f);
        text.push('\n');
    }
    for l in &s.lines {
        text.push_str(&format!("mutants gate: {l}\n"));
    }
    for why in &s.not_measured {
        text.push_str(&format!("::warning::mutants gate: NOT MEASURED: {why}\n"));
    }
    if let Some(why) = &s.broken {
        text.push_str(&format!("::warning::mutants gate: CHECK BROKEN: {why}. Next step: run `./dev mutants gate --base <rev>` and read the control's output.\n"));
    }
    text.push_str(&format!(
        "mutants gate: {} tested, {} caught, {} MISSED, {} timed out, {} unviable, {} not measured by the cap, {} part(s) not measured, {} ({} s)\n",
        s.tested,
        s.caught,
        s.missed,
        s.timeouts,
        s.unviable,
        s.over_cap,
        s.not_measured.len(),
        if mode == Mode::Gate { "blocking" } else { "report-only: not a failure yet (ER-41)" },
        started.elapsed().as_secs()
    ));
    let data = json!({
        "mode": if mode == Mode::Gate { "gate" } else { "report-only" },
        "tested": s.tested, "caught": s.caught, "missed": s.missed, "timeouts": s.timeouts, "unviable": s.unviable,
        "overCap": s.over_cap, "notMeasured": s.not_measured, "broken": s.broken, "findings": s.findings,
    });
    let report = Report::new(data, text);
    if mode == Mode::ReportOnly {
        return report;
    }
    if s.broken.is_some() {
        report.with_exit(Exit::CheckBroken)
    } else if !s.not_measured.is_empty() {
        report.with_exit(Exit::NotMeasured)
    } else if s.missed > 0 {
        report.with_exit(Exit::Findings)
    } else {
        report
    }
}

/// `k/n` of `--shard` (`k` below `n`, as cargo-mutants reads it).
pub fn parse_shard(text: &str) -> Result<(u32, u32), String> {
    let bad = || format!("--shard `{text}` is not k/n with k below n (for example 0/4)");
    let (k, n) = text.split_once('/').ok_or_else(bad)?;
    let (k, n): (u32, u32) = (k.trim().parse().map_err(|_| bad())?, n.trim().parse().map_err(|_| bad())?);
    if n == 0 || k >= n { Err(bad()) } else { Ok((k, n)) }
}

/// `mutants gate --base <rev> [--shard k/n]`.
pub fn command_gate(root: Option<PathBuf>, base: &str, shard: Option<&str>) -> Result<Report, CliError> {
    let shard = shard.map(parse_shard).transpose().map_err(CliError::usage)?;
    let root = crate::gates::command::repo_root(root.as_deref())?;
    let started = std::time::Instant::now();
    let head = crate::plan_repo::git_text(&root, &["rev-parse", "HEAD"])
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| CliError::failure(format!("{} has no HEAD commit", root.display())))?;
    // The scratch parent: the dev-targets folder where there is one (a developer's machine), else the job's own temp folder (CI has no
    // worktree root: `RUNNER_TEMP`, which the runner makes for this job and removes after it).
    let parent = match crate::places::dev_targets(&root, &|k| std::env::var(k).ok()) {
        Ok(targets) => targets.join("mutants"),
        Err(e) => match std::env::var("RUNNER_TEMP").ok().filter(|t| !t.is_empty()) {
            Some(t) => PathBuf::from(t).join("mutants"),
            None => return Err(e),
        },
    };
    let scratch = parent.join(format!("run-{}-{}", std::process::id(), started_stamp()));
    std::fs::create_dir_all(&scratch)
        .map_err(|e| CliError::failure(format!("the scratch folder {} cannot be made: {e}", scratch.display())))?;
    crate::disk::record::write(
        &scratch,
        &crate::disk::record::Record::new(crate::disk::record::Kind::Scratch, "dev:mutants-gate"),
    )
    .map_err(|e| CliError::failure(format!("the scratch folder {} has no owner record: {e}", scratch.display())))?;
    let summary = measure(&Real, &root, &head, base, shard, &scratch, MODE);
    remove_scratch(&parent, &scratch);
    Ok(report(&summary, MODE, started))
}

/// The temporary folder the tests of the mutants get: `tmp` below the run's scratch folder. Apart from the checkout, because the tests leave
/// repositories of their own there (no remote: [`crate::safe_remove`] refuses a folder that holds one), and it is removed first by
/// [`crate::safe_remove::remove_under`], which asks for no proof beyond "strictly under the run's folder".
fn scratch_tmp(scratch: &Path) -> PathBuf {
    let dir = scratch.join("tmp");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Removes the run's own scratch folder through [`crate::safe_remove`] (it has an owner record, its checkout is clean and has a
/// remote branch at its one commit). A refusal is a warning that names the folder, never silent.
fn remove_scratch(parent: &Path, scratch: &Path) {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Err(e) = crate::safe_remove::remove_under(scratch, &scratch.join("tmp"), home.as_deref()) {
        eprintln!(
            "::warning::mutants gate: the tests' temporary folder in {} was not removed ({e:?})",
            scratch.display()
        );
    }
    if let Err(e) = crate::safe_remove::remove(scratch, std::slice::from_ref(&parent.to_path_buf())) {
        eprintln!(
            "::warning::mutants gate: the scratch folder {} was not removed ({e}); remove it with `./dev` or by hand",
            scratch.display()
        );
    }
}

fn started_stamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// An `outcomes.json` in the shape cargo-mutants 27 writes (checked against a real run of the control crate), with invented names.
    fn outcomes(baseline: &str, mutants: &[(&str, &str, u64, &str)]) -> String {
        let mut list = vec![json!({"scenario": "Baseline", "summary": baseline})];
        for (summary, function, line, what) in mutants {
            list.push(json!({
                "scenario": {"Mutant": {
                    "name": format!("src/a.rs:{line}:5: {what}"),
                    "package": "p",
                    "file": "src/a.rs",
                    "function": {"function_name": function, "return_type": "-> u32"},
                    "span": {"start": {"line": line, "column": 5}, "end": {"line": line, "column": 9}},
                    "replacement": "0",
                    "genre": "FnValue"
                }},
                "summary": summary,
            }));
        }
        json!({"outcomes": list}).to_string()
    }

    /// A scripted cargo-mutants and git: answers by the arguments, writes `outcomes.json` where `--output` says.
    struct Fake {
        version: String,
        control: String,
        listed: usize,
        run: String,
        run_code: Option<i32>,
        diff: String,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl Fake {
        fn new() -> Fake {
            Fake {
                version: format!("cargo-mutants {PIN}\n"),
                control: outcomes(
                    "Success",
                    &[
                        ("CaughtMutant", "reached", 2, "replace reached -> u32 with 0"),
                        ("MissedMutant", CONTROL_MISSED, 6, "replace unreached -> u32 with 0"),
                    ],
                ),
                listed: 3,
                run: outcomes("Success", &[("CaughtMutant", "f", 10, "replace f -> u32 with 0")]),
                run_code: Some(0),
                diff: "diff --git a/src/a.rs b/src/a.rs\n".into(),
                calls: Mutex::new(Vec::new()),
            }
        }
        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Tool for Fake {
        fn mutants(&self, cwd: &Path, a: &[String], _tmp: &Path) -> Result<Ran, String> {
            self.calls.lock().unwrap().push(a.to_vec());
            let ok = |stdout: String, code| {
                Ok(Ran {
                    code,
                    stdout,
                    stderr: String::new(),
                })
            };
            if a == ["--version"] {
                return ok(self.version.clone(), Some(0));
            }
            if a.iter().any(|x| x == "--list") {
                return ok(json!(vec![0; self.listed]).to_string(), Some(0));
            }
            let text = if cwd.ends_with("control") { &self.control } else { &self.run };
            let out = a.iter().position(|x| x == "--output").map(|i| PathBuf::from(&a[i + 1])).unwrap();
            std::fs::create_dir_all(out.join("mutants.out")).unwrap();
            std::fs::write(out.join("mutants.out/outcomes.json"), text).unwrap();
            ok(String::new(), if cwd.ends_with("control") { Some(2) } else { self.run_code })
        }
        fn diff(&self, _root: &Path, ws: &str, _base: &str) -> Result<String, String> {
            Ok(if ws == "crates" { self.diff.clone() } else { String::new() })
        }
        fn checkout(&self, _root: &Path, head: &str, tree: &Path) -> Result<(), String> {
            self.calls.lock().unwrap().push(vec!["checkout".into(), head.into()]);
            std::fs::create_dir_all(tree).map_err(|e| e.to_string())
        }
    }

    fn measure_with(f: &Fake) -> Summary {
        let scratch = tempfile::tempdir().unwrap();
        measure(f, scratch.path(), "headsha", "base", None, scratch.path(), Mode::ReportOnly)
    }

    #[test]
    fn the_outcomes_of_a_run_are_read_by_verdict() {
        let text = outcomes(
            "Success",
            &[
                ("CaughtMutant", "a", 1, "replace a -> u32 with 0"),
                ("MissedMutant", "b", 7, "replace * with + in b"),
                ("Timeout", "c", 9, "replace c -> u32 with 1"),
                ("Unviable", "d", 11, "replace d -> String with String::new()"),
            ],
        );
        let o = parse_outcomes(&text).unwrap();
        assert_eq!(o.baseline, Baseline::Passed);
        let counts = [Verdict::Caught, Verdict::Missed, Verdict::Timeout, Verdict::Unviable].map(|v| o.count(v));
        assert_eq!(counts, [1, 1, 1, 1]);
        let missed = o.mutants.iter().find(|m| m.verdict == Verdict::Missed).unwrap();
        assert_eq!(
            (missed.function.as_str(), missed.line, missed.mutation.as_str()),
            ("b", 7, "replace * with + in b")
        );
        assert_eq!(parse_outcomes(&outcomes("Failure", &[])).unwrap().baseline, Baseline::Failed);
    }

    #[test]
    fn a_summary_this_version_does_not_know_and_a_text_that_is_no_json_are_errors_not_skips() {
        assert!(
            parse_outcomes(&outcomes("Success", &[("Skipped", "a", 1, "x")]))
                .unwrap_err()
                .contains("Skipped")
        );
        assert!(parse_outcomes("not json").is_err());
        assert!(parse_outcomes("{}").is_err());
    }

    #[test]
    fn a_finding_names_the_function_and_the_mutation_and_is_a_warning_while_report_only() {
        let m = Tested {
            file: "src/a.rs".into(),
            line: 6,
            function: "unreached".into(),
            mutation: "replace unreached -> u32 with 0".into(),
            verdict: Verdict::Missed,
        };
        let w = annotation("crates", &m, Mode::ReportOnly);
        assert!(w.starts_with("::warning file=crates/src/a.rs,line=6::"), "{w}");
        assert!(w.contains("`unreached`") && w.contains("replace unreached -> u32 with 0"), "{w}");
        assert!(annotation("crates", &m, Mode::Gate).starts_with("::error "));
    }

    /// Positive control (ER-33): the run that shows a MISSED mutant, with the timeouts and unviable ones counted apart.
    #[test]
    fn a_missed_mutant_is_a_finding_and_timeouts_and_unviable_ones_are_counted_apart_never_as_caught() {
        let mut f = Fake::new();
        f.listed = 4;
        f.run = outcomes(
            "Success",
            &[
                ("CaughtMutant", "f", 10, "replace f -> u32 with 0"),
                ("MissedMutant", "g", 20, "replace g -> bool with true"),
                ("Timeout", "h", 30, "replace h -> u32 with 1"),
                ("Unviable", "i", 40, "replace i -> String with String::new()"),
            ],
        );
        let s = measure_with(&f);
        assert_eq!((s.tested, s.caught, s.missed, s.timeouts, s.unviable), (4, 1, 1, 1, 1));
        assert_eq!(s.findings.len(), 1);
        assert!(
            s.findings[0].contains("`g`") && s.findings[0].contains("replace g -> bool with true"),
            "{:?}",
            s.findings
        );
        assert!(s.not_measured.is_empty() && s.broken.is_none());
        let r = report(&s, Mode::ReportOnly, std::time::Instant::now());
        assert_eq!(r.exit, Exit::Ok, "report-only never fails (ER-41)");
        assert!(
            r.text.contains("1 timed out (not a pass)") && r.text.contains("1 unviable (not a pass)"),
            "{}",
            r.text
        );
        assert_eq!(report(&s, Mode::Gate, std::time::Instant::now()).exit, Exit::Findings);
    }

    /// A control that cannot see a missed mutant makes the whole row `check broken`, and nothing after it runs.
    #[test]
    fn a_control_that_shows_no_missed_mutant_is_check_broken_and_looks_at_nothing() {
        let mut f = Fake::new();
        f.control = outcomes("Success", &[("CaughtMutant", "reached", 2, "replace reached -> u32 with 0")]);
        let s = measure_with(&f);
        assert!(s.broken.as_deref().is_some_and(|b| b.contains("cannot tell")), "{:?}", s.broken);
        assert_eq!(s.tested, 0);
        assert!(
            f.calls().iter().all(|a| !a.iter().any(|x| x == "--in-diff")),
            "no diff mutated after a broken control"
        );
        let r = report(&s, Mode::Gate, std::time::Instant::now());
        assert_eq!(r.exit, Exit::CheckBroken);
        assert!(r.text.contains("CHECK BROKEN"), "{}", r.text);
        assert_eq!(report(&s, Mode::ReportOnly, std::time::Instant::now()).exit, Exit::Ok);
    }

    #[test]
    fn another_cargo_mutants_version_is_not_measured_with_the_install_command() {
        let mut f = Fake::new();
        f.version = "cargo-mutants 26.0.0\n".into();
        let s = measure_with(&f);
        assert_eq!(s.not_measured.len(), 1);
        assert!(
            s.not_measured[0].contains(&format!("cargo install cargo-mutants --version {PIN} --locked")),
            "{:?}",
            s.not_measured
        );
        assert_eq!(f.calls().len(), 1, "only the version was asked");
        assert_eq!(report(&s, Mode::Gate, std::time::Instant::now()).exit, Exit::NotMeasured);
    }

    #[test]
    fn a_failing_baseline_or_a_cut_short_run_is_not_measured_never_clean() {
        let mut f = Fake::new();
        f.run = outcomes("Failure", &[]);
        let s = measure_with(&f);
        assert!(s.not_measured.iter().any(|w| w.contains("unmutated tree")), "{:?}", s.not_measured);
        let mut f = Fake::new();
        f.run_code = None;
        let s = measure_with(&f);
        assert!(s.not_measured.iter().any(|w| w.contains("cut short")), "{:?}", s.not_measured);
        assert_eq!(
            s.tested, 1,
            "the mutant that has a verdict is read, never lost, and never taken for the whole run"
        );
    }

    #[test]
    fn a_diff_with_more_mutants_than_the_cap_runs_a_slice_and_counts_the_rest_as_not_measured() {
        let mut f = Fake::new();
        f.listed = MAX_MUTANTS * 2 + 10;
        let s = measure_with(&f);
        let run = f
            .calls()
            .into_iter()
            .find(|a| a.iter().any(|x| x == "--shard"))
            .expect("a shard argument");
        let i = run.iter().position(|x| x == "--shard").unwrap();
        assert_eq!(run[i + 1], "0/3", "ceil({} / {MAX_MUTANTS}) slices", MAX_MUTANTS * 2 + 10);
        assert_eq!(
            s.over_cap,
            MAX_MUTANTS * 2 + 10 - 1,
            "the fake ran one mutant: the others are not measured"
        );
        let tail: Vec<&str> = run.iter().rev().take(3).rev().map(String::as_str).collect();
        assert_eq!(
            tail,
            ["--", "--profile", NEXTEST_PROFILE],
            "the mutants' tests run under CI's nextest profile"
        );
    }

    #[test]
    fn nothing_to_mutate_is_said_and_never_counted_as_tested() {
        let mut f = Fake::new();
        f.listed = 0;
        let s = measure_with(&f);
        assert_eq!(s.tested, 0);
        assert!(s.lines.iter().any(|l| l.contains("no function to mutate")), "{:?}", s.lines);
        assert!(
            s.lines.iter().any(|l| l.starts_with("tools/paddock-dev: no Rust source changed")),
            "{:?}",
            s.lines
        );
    }

    /// The control must stay a crate whose second function no test reaches: a "fix" that adds a test for it blinds the probe.
    #[test]
    fn the_control_crate_has_a_function_no_test_reaches() {
        let (code, tests) = CONTROL_LIB.split_once(concat!("#[cfg(", "test)]")).unwrap();
        assert!(code.contains(&format!("fn {CONTROL_MISSED}")));
        assert!(!tests.contains(CONTROL_MISSED), "a test reaches the control's untested function");
        assert!(tests.contains("super::reached"));
    }

    /// The mutants run in place in a checkout of the committed head, never in the repository: the run asks for one, and passes `--in-place`.
    #[test]
    fn the_mutants_run_in_place_in_a_checkout_of_the_head_never_in_the_repository() {
        let f = Fake::new();
        let _ = measure_with(&f);
        let calls = f.calls();
        assert!(calls.iter().any(|a| a == &["checkout", "headsha"]), "{calls:?}");
        let run = calls
            .iter()
            .find(|a| a.iter().any(|x| x == "--test-tool"))
            .expect("the mutants run");
        assert!(run.iter().any(|x| x == "--in-place"), "{run:?}");
    }

    #[test]
    fn a_shard_is_k_of_n_with_k_below_n() {
        assert_eq!(parse_shard("0/4"), Ok((0, 4)));
        assert_eq!(parse_shard("3/4"), Ok((3, 4)));
        for bad in ["4/4", "0/0", "a/4", "1", "-1/4", ""] {
            assert!(parse_shard(bad).is_err(), "{bad}");
        }
    }

    /// A planned shard goes to cargo-mutants whole (the listing and the run), is never sliced again, and a shard over the budget is
    /// not measured, never a partial pass.
    #[test]
    fn a_planned_shard_is_passed_on_whole_and_one_over_the_budget_is_not_measured() {
        let f = Fake::new();
        let scratch = tempfile::tempdir().unwrap();
        let s = measure(
            &f,
            scratch.path(),
            "headsha",
            "base",
            Some((1, 4)),
            scratch.path(),
            Mode::ReportOnly,
        );
        let shards: Vec<Vec<String>> = f.calls().into_iter().filter(|a| a.iter().any(|x| x == "--shard")).collect();
        assert_eq!(shards.len(), 2, "the listing and the run: {shards:?}");
        assert!(
            shards.iter().all(|a| a.windows(2).any(|w| w[0] == "--shard" && w[1] == "1/4")),
            "{shards:?}"
        );
        assert_eq!(s.tested, 1);
        let mut over = Fake::new();
        over.listed = MAX_PER_SHARD + 1;
        let scratch = tempfile::tempdir().unwrap();
        let s = measure(
            &over,
            scratch.path(),
            "headsha",
            "base",
            Some((0, 2)),
            scratch.path(),
            Mode::ReportOnly,
        );
        assert_eq!((s.tested, s.over_cap), (0, MAX_PER_SHARD + 1), "{:?}", s.lines);
        assert!(s.lines.iter().any(|l| l.contains("plan more shards")), "{:?}", s.lines);
    }

    #[test]
    fn the_baseline_failures_are_the_failing_lines_and_the_tail_of_its_log_without_colours() {
        let log =
            "   Compiling x\n        PASS [ 1s] a::b\n\u{1b}[31;1m        FAIL\u{1b}[0m [ 2s] a::c\nerror: test run failed\nlast line\n";
        let got = baseline_failures(log);
        assert!(
            got.contains("FAIL [ 2s] a::c") && got.contains("error: test run failed") && got.contains("last line"),
            "{got}"
        );
        assert!(!got.contains('\u{1b}'), "{got}");
        assert_eq!(baseline_failures(""), "(the baseline log is empty)");
        let many: String = (0..30).map(|i| format!("FAIL [ 1s] t{i}\n")).collect();
        assert!(baseline_failures(&many).matches("FAIL [ 1s] t").count() <= 15, "bounded");
    }

    #[test]
    fn the_pin_is_a_plain_version() {
        assert!(
            PIN.split('.').count() == 3 && PIN.split('.').all(|p| p.parse::<u32>().is_ok()),
            "{PIN}"
        );
    }
}
