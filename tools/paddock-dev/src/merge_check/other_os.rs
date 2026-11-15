//! The `other-os` member gate of `./dev merge check --gates` (train 14, early-catch item 1; ADR 0087 amendment of 2026-11-16):
//! a member's affected tests run on Linux (the CI image, as root, TMPDIR on an ext4 loop mount, in a fresh clone with no
//! repo-local git config) and on Windows (the test VM under its lock, `vm-win.sh run --ci-like`) BEFORE the member joins a
//! train. Train 12 lost about 17 hours to three red CI rounds whose every failure was Linux- or Windows-only.
//!
//! This module is the pure part and the verdict: which changed paths reach which OS ([`select`], from the paths of the member's
//! own change), what a run's answer means ([`interpret`]), and the row. The two runs themselves are scripts
//! (`scripts/member-gate-linux.sh`, `scripts/member-gate-windows.sh`) behind the [`Runner`] port, so every verdict is tested
//! without Docker or the VM.
//!
//! **Never a silent skip (ER-33).** Every outcome is a row with its reason: `passed`, `failed`, `not measured` (no Docker, the VM
//! is busy, a control line is missing) or `not run` (no changed path reaches the OS). A pass needs the run's own control line for
//! this merge's tree and the nextest summary; an exit code alone is never a pass. **Report-only until its first green run
//! (ER-41)**: [`ENFORCED`] is false, so the exit code is unchanged and a `::warning::` says what would happen.

use super::generator_step::Step;
use super::{Merge, git::Git, short};
use paddock_cli::Exit;
use paddock_proc::{Limits, command, run_bounded};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Whether a failing row changes the exit code. False until the first green report-only run is recorded in ADR 0087's amendment
/// of 2026-11-16 (ER-41); the change that records it flips this.
pub const ENFORCED: bool = false;

/// The exit code of the two scripts when tests failed (nextest's own code for it).
pub const EXIT_TESTS_FAILED: i32 = 100;
/// The exit code of the two scripts when nothing was measured (EX_TEMPFAIL: the VM is busy, Docker is down, a control failed).
pub const EXIT_NOT_MEASURED: i32 = 75;

/// An outer bound on one OS run (ER-32). A cold Linux build of `paddock-dev` in the container is the long case.
const TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// An operating system the gate runs on besides the host's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Os {
    /// The CI image in a container, as root, on an ext4 loop mount.
    Linux,
    /// The test VM, the way the Gitea Windows runner starts a step.
    Windows,
}

impl Os {
    /// The name in rows and in the JSON.
    pub fn name(self) -> &'static str {
        match self {
            Os::Linux => "linux",
            Os::Windows => "windows",
        }
    }
}

/// One thing to run on an OS.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Target {
    /// `cargo nextest run -p <package>` in the Cargo workspace at `workspace` (relative to the repository root).
    Nextest {
        /// The workspace folder, for example `crates` or `tools/paddock-dev`.
        workspace: String,
        /// The package name.
        package: String,
    },
    /// A Node test command run from a folder (Linux only: the Windows gate runs the Rust tests the runner runs).
    Node {
        /// The folder, relative to the repository root.
        dir: String,
        /// The command, run with `sh -c`.
        cmd: String,
    },
}

impl Target {
    /// The line the scripts take: `nextest <workspace> <package>` or `node <dir> <command...>`.
    pub fn spec(&self) -> String {
        match self {
            Target::Nextest { workspace, package } => format!("nextest {workspace} {package}"),
            Target::Node { dir, cmd } => format!("node {dir} {cmd}"),
        }
    }
}

/// What the gate runs on each OS, and why.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// Targets for the Linux run.
    pub linux: BTreeSet<Target>,
    /// Targets for the Windows run.
    pub windows: BTreeSet<Target>,
    /// One line per trigger, for the row's text.
    pub why: Vec<String>,
}

/// What the selection needs to know about the tree. The real one reads the checkout; tests give a table.
pub trait Tree {
    /// The Cargo package that owns `file` (the nearest `Cargo.toml` above it): its folder, its name and its workspace folder.
    fn package_of(&self, file: &str) -> Option<Package>;
    /// Whether the tests of the package at `dir` start programs (`Command::new`, `TestCommand`, `paddock_proc`, `spawn`).
    fn starts_programs(&self, dir: &str) -> bool;
}

/// A Cargo package found above a changed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The package folder, relative to the repository root.
    pub dir: String,
    /// The package name.
    pub name: String,
    /// The workspace folder it is built in.
    pub workspace: String,
}

/// Paths that always trigger a Rust or Node run, whatever the plan says: the code that starts programs for every test (the
/// testkit), the tool that runs the gates, the conformance checks and the CI kit under it, and the VM scripts.
const ALWAYS_RUST: [(&str, &str, &str); 3] = [
    ("tools/paddock-dev/", "tools/paddock-dev", "paddock-dev"),
    ("crates/paddock-testkit/", "crates", "paddock-testkit"),
    ("crates/paddock-testkit-start/", "crates", "paddock-testkit-start"),
];

/// Documentation and notes never start a program (the same rule as the plan's `is_docs`, which a Markdown file is).
fn docs_only(file: &str) -> bool {
    file.ends_with(".md") || file.starts_with("docs/")
}

/// Which OS runs what for the files a member changed: the member's own change (`parent..merge`), never the whole train.
pub fn select(changed: &[String], tree: &dyn Tree) -> Selection {
    let mut s = Selection::default();
    let note = |why: String, s: &mut Selection| {
        if !s.why.contains(&why) {
            s.why.push(why);
        }
    };
    for file in changed.iter().filter(|f| !docs_only(f)) {
        for (prefix, workspace, package) in ALWAYS_RUST {
            if file.starts_with(prefix) {
                let t = Target::Nextest {
                    workspace: workspace.into(),
                    package: package.into(),
                };
                s.linux.insert(t.clone());
                s.windows.insert(t);
                note(format!("{prefix} changed: {package} runs on both"), &mut s);
            }
        }
        if file.starts_with("conformance/") {
            s.linux.insert(Target::Node {
                dir: "conformance".into(),
                cmd: "npm test".into(),
            });
            note("conformance/ changed: its tests run on Linux".into(), &mut s);
        }
        if file.starts_with("scripts/vm-") {
            s.linux.insert(Target::Node {
                dir: ".".into(),
                cmd: "node --test scripts/vm-*.test.mjs".into(),
            });
            note("scripts/vm-* changed: its tests run on Linux".into(), &mut s);
        }
        if !file.ends_with(".rs") && !file.ends_with("Cargo.toml") {
            continue;
        }
        if let Some(p) = tree.package_of(file)
            && tree.starts_programs(&p.dir)
        {
            let t = Target::Nextest {
                workspace: p.workspace,
                package: p.name.clone(),
            };
            s.linux.insert(t.clone());
            s.windows.insert(t);
            note(
                format!("{} changed and its tests start programs: {} runs on both", p.dir, p.name),
                &mut s,
            );
        }
    }
    s
}

/// The real tree: reads `Cargo.toml` and the sources of the checkout.
pub struct DiskTree(pub PathBuf);

impl Tree for DiskTree {
    fn package_of(&self, file: &str) -> Option<Package> {
        let mut dir = Path::new(file).parent()?.to_path_buf();
        loop {
            let manifest = self.0.join(&dir).join("Cargo.toml");
            if let Ok(text) = std::fs::read_to_string(&manifest)
                && let Some(name) = package_name(&text)
            {
                let rel = dir.to_string_lossy().replace('\\', "/");
                return Some(Package {
                    workspace: self.workspace_of(&dir),
                    name,
                    dir: rel,
                });
            }
            if !dir.pop() || dir.as_os_str().is_empty() {
                return None;
            }
        }
    }

    fn starts_programs(&self, dir: &str) -> bool {
        starts_programs_in(&self.0.join(dir))
    }
}

impl DiskTree {
    /// The folder of the Cargo workspace a package folder is built in: the nearest `Cargo.toml` above it (or the folder itself)
    /// with a `[workspace]` table.
    fn workspace_of(&self, package_dir: &Path) -> String {
        let mut dir = package_dir.to_path_buf();
        loop {
            if let Ok(text) = std::fs::read_to_string(self.0.join(&dir).join("Cargo.toml"))
                && text.parse::<toml::Table>().is_ok_and(|t| t.contains_key("workspace"))
            {
                return dir.to_string_lossy().replace('\\', "/");
            }
            if !dir.pop() || dir.as_os_str().is_empty() {
                return package_dir.to_string_lossy().replace('\\', "/");
            }
        }
    }
}

/// The `[package] name` of a manifest.
fn package_name(manifest: &str) -> Option<String> {
    let table: toml::Table = manifest.parse().ok()?;
    table.get("package")?.get("name")?.as_str().map(str::to_string)
}

/// The text markers of a Rust test that starts a program. A heuristic on purpose: it only decides whether the other OSes are
/// worth their minutes, and a false positive costs a run while a false negative is what the gate exists to prevent.
const START_MARKERS: [&str; 5] = ["Command::new", "TestCommand", "paddock_proc", ".spawn(", "assert_cmd"];

/// Bounds of the source walk: a crate with more files or larger files than this is read only in part and counts as starting programs.
const MAX_FILES: usize = 4000;
const MAX_FILE_BYTES: u64 = 1 << 20;

fn starts_programs_in(dir: &Path) -> bool {
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0usize;
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() {
                if name != "target" && name != "node_modules" && !name.starts_with('.') {
                    stack.push(path);
                }
            } else if name.ends_with(".rs") {
                seen += 1;
                if seen > MAX_FILES {
                    return true;
                }
                if e.metadata().is_ok_and(|m| m.len() > MAX_FILE_BYTES) {
                    return true;
                }
                if std::fs::read_to_string(&path).is_ok_and(|t| START_MARKERS.iter().any(|m| t.contains(m))) {
                    return true;
                }
            }
        }
    }
    false
}

/// What one OS run answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    /// The script's exit code (`None`: ended by the bound or a signal).
    pub code: Option<i32>,
    /// What it printed, standard output then standard error.
    pub text: String,
}

/// Runs the targets on one OS in a fresh clone of the merge.
pub trait Runner {
    /// An error is a failure to start (Docker or the VM could not be used at all).
    fn run(&self, os: Os, root: &Path, merge_tree: &str, targets: &BTreeSet<Target>) -> Result<RunResult, String>;
}

/// The row of one OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The tests ran in the controlled environment and all passed.
    Passed,
    /// The tests ran in the controlled environment and some failed.
    Failed,
    /// Nothing can be said (the reason is the row's text): never a pass and never a failure of the member.
    NotMeasured(String),
}

impl Verdict {
    fn name(&self) -> &'static str {
        match self {
            Verdict::Passed => "passed",
            Verdict::Failed => "failed",
            Verdict::NotMeasured(_) => "not measured",
        }
    }
}

/// The control line a script prints once its environment is set up, for exactly this tree. A run without it did not show that the
/// environment was the one asked for (a Linux run as root on ext4 in a fresh clone; a Windows run in the runner-like guest).
pub fn control_line(os: Os, tree: &str) -> String {
    format!("other-os-control: os={} tree={tree}", os.name())
}

/// What a run's answer means. A pass is exit 0 with the control line for this tree and nextest's summary for every nextest target
/// (so a run that executed no test is not a pass); failures are exit 100 with the control line; everything else did not look.
pub fn interpret(os: Os, tree: &str, targets: &BTreeSet<Target>, answer: &Result<RunResult, String>) -> Verdict {
    let run = match answer {
        Err(why) => return Verdict::NotMeasured(why.clone()),
        Ok(r) => r,
    };
    let control = control_line(os, tree);
    let has_control = run.text.lines().any(|l| l.trim_end().starts_with(&control));
    let tail = || last_lines(&run.text, 3);
    match run.code {
        None => Verdict::NotMeasured(format!("the run was ended by its time bound or a signal: {}", tail())),
        Some(EXIT_NOT_MEASURED) => Verdict::NotMeasured(format!("the script did not measure: {}", tail())),
        _ if !has_control => Verdict::NotMeasured(format!(
            "no `{control}` line: the run did not show that its environment was the one asked for (exit {:?}): {}",
            run.code,
            tail()
        )),
        Some(0) => {
            let nextest = targets.iter().filter(|t| matches!(t, Target::Nextest { .. })).count();
            let summaries = run
                .text
                .lines()
                .filter(|l| l.contains("Summary [") && !l.contains(" 0 tests run"))
                .count();
            if summaries < nextest {
                Verdict::NotMeasured(format!(
                    "exit 0 but {summaries} of {nextest} nextest summaries show tests run; an exit code alone is not a pass"
                ))
            } else {
                Verdict::Passed
            }
        }
        Some(EXIT_TESTS_FAILED) => Verdict::Failed,
        Some(c) => Verdict::NotMeasured(format!(
            "exit {c} is neither a pass (0) nor failed tests ({EXIT_TESTS_FAILED}): {}",
            tail()
        )),
    }
}

fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join(" | ")
}

fn row(os: Os, targets: &BTreeSet<Target>, verdict: &Verdict, seconds: f64, log: &str) -> Value {
    json!({
        "os": os.name(),
        "outcome": verdict.name(),
        "why": match verdict { Verdict::NotMeasured(w) => w.as_str(), _ => "" },
        "targets": targets.iter().map(Target::spec).collect::<Vec<_>>(),
        "seconds": (seconds * 10.0).round() / 10.0,
        "log": log,
    })
}

/// What a failed or unmeasured row costs: nothing while report-only, the exit code once enforced.
fn cost(enforced: bool, verdict: &Verdict) -> Exit {
    match (enforced, verdict) {
        (false, _) | (_, Verdict::Passed) => Exit::Ok,
        (true, Verdict::Failed) => Exit::Findings,
        (true, Verdict::NotMeasured(_)) => Exit::NotMeasured,
    }
}

/// Said out loud when no changed path reaches an OS; not a failure.
pub fn not_run(why: &str) -> Step {
    Step {
        text: vec![format!("member gate other-os: not run ({why})")],
        data: json!({"outcome": "not run", "why": why}),
        exit: Exit::Ok,
    }
}

/// Runs the gate for a merge checked out in `root`: the paths the member changed (`parents[0]` to the merge's tree) pick the work
/// per OS, each OS runs in its own fresh clone, and every OS gets a row. `enforced` is [`ENFORCED`] outside the tests.
pub fn run(root: &Path, git: &Git, merge: &Merge, tree: &dyn Tree, runner: &dyn Runner, enforced: bool) -> Step {
    let Some(base) = merge.parents.first() else {
        return not_run("the merge has no parents");
    };
    let changed = match git.changed_paths(base, &merge.tree) {
        Ok(c) => c,
        Err(e) => {
            return unmeasured(enforced, &format!("the paths the merge changed cannot be listed: {e}"));
        }
    };
    let selection = select(&changed, tree);
    if selection.linux.is_empty() && selection.windows.is_empty() {
        return not_run(&format!(
            "no changed path reaches the other OSes: not tools/paddock-dev, crates/paddock-testkit, conformance, scripts/vm-* or a Rust crate whose tests start programs ({} paths checked)",
            changed.len()
        ));
    }
    execute(root, &merge.tree, &selection, runner, enforced, short(base))
}

/// Runs each OS the selection names and builds the step. Linux and Windows are independent rows; neither hides the other.
pub fn execute(root: &Path, tree: &str, selection: &Selection, runner: &dyn Runner, enforced: bool, base: &str) -> Step {
    let (level, report_only) = if enforced {
        ("::error", "")
    } else {
        (
            "::warning",
            " REPORT-ONLY until its first green run is recorded in the ADR 0087 amendment of 2026-11-16 (ER-41): the exit code is unchanged.",
        )
    };
    let mut text = vec![format!("member gate other-os (base {base}): {}", selection.why.join("; "))];
    let mut rows = Vec::new();
    let mut exit = Exit::Ok;
    for (os, targets) in [(Os::Linux, &selection.linux), (Os::Windows, &selection.windows)] {
        if targets.is_empty() {
            rows.push(json!({"os": os.name(), "outcome": "not run", "why": "no changed path reaches this OS", "targets": []}));
            text.push(format!("  {}: not run (no changed path reaches it)", os.name()));
            continue;
        }
        let started = Instant::now();
        let answer = runner.run(os, root, tree, targets);
        let seconds = started.elapsed().as_secs_f64();
        let verdict = interpret(os, tree, targets, &answer);
        let log = answer.as_ref().map(|r| r.text.as_str()).unwrap_or("");
        text.push(format!(
            "  {}: {} in {:.0} s ({} targets)",
            os.name(),
            verdict.name(),
            seconds,
            targets.len()
        ));
        match &verdict {
            Verdict::Passed => {}
            Verdict::Failed => {
                text.push(log.to_string());
                text.push(format!(
                    "{level}::member gate other-os FAILED on {}: the member's affected tests fail there. Next step: take the member out and send it back to its author with the failures above; do not fix it on the train.{report_only}",
                    os.name()
                ));
            }
            Verdict::NotMeasured(why) => text.push(format!(
                "{level}::member gate other-os NOT MEASURED on {}: {why}. Next step: run `scripts/member-gate-{}.sh` yourself (docs/adr/amendments/0087-member-gate-other-os-2026-11-16.md); a member whose other-OS tests did not run is not a member that passed.{report_only}",
                os.name(),
                os.name()
            )),
        }
        let c = cost(enforced, &verdict);
        if c == Exit::Findings || (c == Exit::NotMeasured && exit == Exit::Ok) {
            exit = c;
        }
        rows.push(row(os, targets, &verdict, seconds, &tail_of(log)));
    }
    Step {
        text,
        data: json!({"outcome": outcome_of(&rows), "base": base, "enforced": enforced, "rows": rows}),
        exit,
    }
}

/// The last 4 KiB of a log: enough for the rows' evidence without the whole build output in the JSON.
fn tail_of(log: &str) -> String {
    let start = log.len().saturating_sub(4096);
    let start = (start..=log.len()).find(|i| log.is_char_boundary(*i)).unwrap_or(log.len());
    log[start..].to_string()
}

/// The one word for the step: `failed` beats `not measured` beats `passed`.
fn outcome_of(rows: &[Value]) -> &'static str {
    let has = |w: &str| rows.iter().any(|r| r["outcome"] == w);
    if has("failed") {
        "failed"
    } else if has("not measured") {
        "not measured"
    } else if has("passed") {
        "passed"
    } else {
        "not run"
    }
}

pub(super) fn unmeasured(enforced: bool, why: &str) -> Step {
    Step {
        text: vec![format!("::warning::member gate other-os NOT MEASURED: {why}")],
        data: json!({"outcome": "not measured", "why": why, "enforced": enforced}),
        exit: if enforced { Exit::NotMeasured } else { Exit::Ok },
    }
}

/// The real runner: the two scripts, started from the checkout. Linux needs Docker (OrbStack); Windows needs the test VM, which
/// the script takes under its lock (`scripts/vm-lock-run.sh`; busy is exit 75, not measured).
pub struct Scripts;

impl Runner for Scripts {
    fn run(&self, os: Os, root: &Path, merge_tree: &str, targets: &BTreeSet<Target>) -> Result<RunResult, String> {
        let script = root.join(format!("scripts/member-gate-{}.sh", os.name()));
        if !script.is_file() {
            return Err(format!("{} is missing", script.display()));
        }
        let mut cmd = command("bash");
        cmd.arg(&script)
            .arg(merge_tree)
            .args(targets.iter().map(Target::spec))
            .current_dir(root);
        let out = run_bounded(cmd, Limits::new(TIMEOUT)).map_err(|e| format!("{} could not start: {e}", script.display()))?;
        let code = match out.outcome {
            paddock_proc::Outcome::Exited(st) => st.code(),
            _ => None,
        };
        Ok(RunResult {
            code,
            text: format!("{}\n{}", String::from_utf8_lossy(&out.stdout), out.stderr_text())
                .trim()
                .to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // Flip ENFORCED only in the change that records the first green report-only run (ER-41); this fails the build until it is decided.
    const _: () = assert!(!ENFORCED);
    use std::sync::Mutex;

    /// A tree table: file folder -> package, and which folders start programs.
    struct Table {
        packages: BTreeMap<&'static str, Package>,
        starts: Vec<&'static str>,
    }

    impl Tree for Table {
        fn package_of(&self, file: &str) -> Option<Package> {
            self.packages
                .iter()
                .find(|(d, _)| file.starts_with(&format!("{d}/")))
                .map(|(_, p)| p.clone())
        }
        fn starts_programs(&self, dir: &str) -> bool {
            self.starts.contains(&dir)
        }
    }

    fn pkg(dir: &str, name: &str, workspace: &str) -> Package {
        Package {
            dir: dir.into(),
            name: name.into(),
            workspace: workspace.into(),
        }
    }

    fn table() -> Table {
        let mut packages = BTreeMap::new();
        packages.insert("crates/paddock-proc", pkg("crates/paddock-proc", "paddock-proc", "crates"));
        packages.insert("crates/paddock-names", pkg("crates/paddock-names", "paddock-names", "crates"));
        Table {
            packages,
            starts: vec!["crates/paddock-proc"],
        }
    }

    fn files(f: &[&str]) -> Vec<String> {
        f.iter().map(|s| s.to_string()).collect()
    }

    fn nextest(ws: &str, p: &str) -> Target {
        Target::Nextest {
            workspace: ws.into(),
            package: p.into(),
        }
    }

    #[test]
    fn the_tool_and_the_testkit_reach_both_oses() {
        let s = select(&files(&["tools/paddock-dev/src/queue/mod.rs"]), &table());
        assert_eq!(s.linux, BTreeSet::from([nextest("tools/paddock-dev", "paddock-dev")]));
        assert_eq!(s.linux, s.windows);
        let s = select(&files(&["crates/paddock-testkit/src/lib.rs"]), &table());
        assert!(s.windows.contains(&nextest("crates", "paddock-testkit")));
    }

    #[test]
    fn conformance_and_the_vm_scripts_run_node_tests_on_linux_only() {
        let s = select(&files(&["conformance/ci/kit.sh", "scripts/vm-win.sh"]), &table());
        assert_eq!(s.linux.len(), 2);
        assert!(s.windows.is_empty());
        assert!(s.linux.iter().all(|t| matches!(t, Target::Node { .. })));
    }

    #[test]
    fn a_crate_whose_tests_start_programs_runs_on_both_and_one_that_does_not_is_skipped() {
        let s = select(&files(&["crates/paddock-proc/src/lib.rs"]), &table());
        assert_eq!(s.linux, BTreeSet::from([nextest("crates", "paddock-proc")]));
        assert_eq!(s.windows, s.linux);
        let s = select(
            &files(&["crates/paddock-names/src/lib.rs", "crates/paddock-names/Cargo.toml"]),
            &table(),
        );
        assert_eq!(s, Selection::default());
    }

    #[test]
    fn documentation_and_unrelated_files_select_nothing() {
        let s = select(
            &files(&[
                "docs/adr/0087-x.md",
                "tools/paddock-dev/README.md",
                "ui/projects/a.ts",
                "apps/dyno/app/x.html",
            ]),
            &table(),
        );
        assert_eq!(s, Selection::default());
    }

    #[test]
    fn the_package_walk_finds_the_workspace_and_the_markers_on_disk() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().to_path_buf();
        std::fs::create_dir_all(dir.join("crates/a/src")).unwrap();
        std::fs::create_dir_all(dir.join("crates/b/src")).unwrap();
        std::fs::write(dir.join("crates/Cargo.toml"), "[workspace]\nmembers=[\"a\",\"b\"]\n").unwrap();
        std::fs::write(dir.join("crates/a/Cargo.toml"), "[package]\nname=\"a\"\n").unwrap();
        std::fs::write(dir.join("crates/b/Cargo.toml"), "[package]\nname=\"b\"\n").unwrap();
        std::fs::write(dir.join("crates/a/src/lib.rs"), "fn t() { std::process::Command::new(\"x\"); }").unwrap();
        std::fs::write(dir.join("crates/b/src/lib.rs"), "fn t() {}").unwrap();
        let tree = DiskTree(dir.clone());
        let a = tree.package_of("crates/a/src/lib.rs").unwrap();
        assert_eq!((a.name.as_str(), a.workspace.as_str(), a.dir.as_str()), ("a", "crates", "crates/a"));
        assert!(tree.starts_programs("crates/a"));
        assert!(!tree.starts_programs("crates/b"));
        let s = select(&files(&["crates/a/src/lib.rs", "crates/b/src/lib.rs"]), &tree);
        assert_eq!(s.windows, BTreeSet::from([nextest("crates", "a")]));
    }

    fn one() -> BTreeSet<Target> {
        BTreeSet::from([nextest("crates", "paddock-proc")])
    }

    fn ran(code: Option<i32>, os: Os, tree: &str, body: &str) -> Result<RunResult, String> {
        Ok(RunResult {
            code,
            text: format!("{}\n{body}", control_line(os, tree)),
        })
    }

    const SUMMARY: &str = "     Summary [   1.2s] 41 tests run: 41 passed, 0 skipped";

    #[test]
    fn a_pass_needs_the_control_line_for_this_tree_and_a_nextest_summary() {
        let ok = ran(Some(0), Os::Linux, "t1", SUMMARY);
        assert_eq!(interpret(Os::Linux, "t1", &one(), &ok), Verdict::Passed);
        // Exit 0 alone, the control of another tree or of another OS, and a run of zero tests are never a pass.
        let alone = Ok(RunResult {
            code: Some(0),
            text: SUMMARY.into(),
        });
        let other_tree = ran(Some(0), Os::Linux, "t2", SUMMARY);
        let other_os = ran(Some(0), Os::Windows, "t1", SUMMARY);
        let zero = ran(Some(0), Os::Linux, "t1", "     Summary [   0.0s] 0 tests run: 0 passed");
        let no_summary = ran(Some(0), Os::Linux, "t1", "nothing");
        for a in [alone, other_tree, other_os, zero, no_summary] {
            assert!(matches!(interpret(Os::Linux, "t1", &one(), &a), Verdict::NotMeasured(_)), "{a:?}");
        }
    }

    #[test]
    fn failed_tests_are_a_failure_only_with_the_control_and_every_other_answer_did_not_look() {
        let failed = ran(Some(EXIT_TESTS_FAILED), Os::Windows, "t1", "FAIL [ 0.1s] x");
        assert_eq!(interpret(Os::Windows, "t1", &one(), &failed), Verdict::Failed);
        let no_control = Ok(RunResult {
            code: Some(EXIT_TESTS_FAILED),
            text: "FAIL".into(),
        });
        assert!(matches!(interpret(Os::Windows, "t1", &one(), &no_control), Verdict::NotMeasured(_)));
        for code in [Some(75), Some(1), Some(137), None] {
            let a = ran(code, Os::Windows, "t1", "x");
            assert!(
                matches!(interpret(Os::Windows, "t1", &one(), &a), Verdict::NotMeasured(_)),
                "{code:?}"
            );
        }
        assert!(matches!(
            interpret(Os::Linux, "t1", &one(), &Err("no docker".into())),
            Verdict::NotMeasured(_)
        ));
    }

    /// A runner that answers per OS and records what it was asked.
    struct Fake {
        linux: Result<RunResult, String>,
        windows: Result<RunResult, String>,
        asked: Mutex<Vec<Os>>,
    }

    impl Runner for Fake {
        fn run(&self, os: Os, _root: &Path, _tree: &str, _t: &BTreeSet<Target>) -> Result<RunResult, String> {
            self.asked.lock().unwrap().push(os);
            match os {
                Os::Linux => self.linux.clone(),
                Os::Windows => self.windows.clone(),
            }
        }
    }

    fn selected() -> Selection {
        Selection {
            linux: one(),
            windows: one(),
            why: vec!["x".into()],
        }
    }

    fn fake(linux: Result<RunResult, String>, windows: Result<RunResult, String>) -> Fake {
        Fake {
            linux,
            windows,
            asked: Mutex::new(Vec::new()),
        }
    }

    /// A planted Linux-only failure: Windows passes, Linux fails, and the exit changes only when enforced.
    #[test]
    fn a_planted_linux_only_failure_is_red_on_linux_and_green_on_windows() {
        for (enforced, want) in [(false, Exit::Ok), (true, Exit::Findings)] {
            let f = fake(
                ran(Some(EXIT_TESTS_FAILED), Os::Linux, "t1", "FAIL macos_only_path"),
                ran(Some(0), Os::Windows, "t1", SUMMARY),
            );
            let step = execute(Path::new("."), "t1", &selected(), &f, enforced, "abc");
            assert_eq!(step.exit, want, "enforced={enforced}");
            assert_eq!(step.data["outcome"], "failed");
            assert_eq!(step.data["rows"][0]["outcome"], "failed");
            assert_eq!(step.data["rows"][1]["outcome"], "passed");
            assert!(step.text.join("\n").contains(if enforced {
                "::error::member gate other-os FAILED on linux"
            } else {
                "::warning::member gate other-os FAILED on linux"
            }));
        }
    }

    /// A planted Windows-only failure (a `#!/bin/sh` fixture): Linux passes, Windows fails.
    #[test]
    fn a_planted_windows_only_failure_is_red_on_windows_and_green_on_linux() {
        let f = fake(
            ran(Some(0), Os::Linux, "t1", SUMMARY),
            ran(Some(EXIT_TESTS_FAILED), Os::Windows, "t1", "FAIL sh_fixture"),
        );
        let step = execute(Path::new("."), "t1", &selected(), &f, true, "abc");
        assert_eq!(step.exit, Exit::Findings);
        assert_eq!(step.data["rows"][0]["outcome"], "passed");
        assert_eq!(step.data["rows"][1]["outcome"], "failed");
    }

    #[test]
    fn an_unmeasured_os_never_hides_the_other_and_costs_the_not_measured_exit_once_enforced() {
        let f = fake(Err("docker is not running".into()), ran(Some(0), Os::Windows, "t1", SUMMARY));
        let step = execute(Path::new("."), "t1", &selected(), &f, true, "abc");
        assert_eq!(step.exit, Exit::NotMeasured);
        assert_eq!(step.data["outcome"], "not measured");
        assert_eq!(step.data["rows"][0]["why"], "docker is not running");
        assert_eq!(step.data["rows"][1]["outcome"], "passed");
        assert_eq!(f.asked.lock().unwrap().len(), 2);
        assert_eq!(execute(Path::new("."), "t1", &selected(), &f, false, "abc").exit, Exit::Ok);
    }

    #[test]
    fn an_os_with_nothing_selected_is_a_row_that_says_so_and_is_not_asked() {
        let f = fake(ran(Some(0), Os::Linux, "t1", SUMMARY), Err("must not be asked".into()));
        let only_linux = Selection {
            linux: one(),
            windows: BTreeSet::new(),
            why: vec![],
        };
        let step = execute(Path::new("."), "t1", &only_linux, &f, true, "abc");
        assert_eq!(step.exit, Exit::Ok);
        assert_eq!(step.data["rows"][1]["outcome"], "not run");
        assert_eq!(*f.asked.lock().unwrap(), vec![Os::Linux]);
    }
}
