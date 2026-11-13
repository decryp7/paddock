//! `./dev check run` (RT-5): the Rust host and its first two checks against the JavaScript ones, the oracle.
//!
//! - `fixture_*`: the recorded answers of the JS runner on `tests/fixtures/check/cases` (`expected.json`, `masked.json`, made by
//!   `generate.mjs`); these need no node, so they outlive the JS host.
//! - `live_*`: the JS host itself as the oracle, on the fixtures and on this working tree (every tracked `.rs` file masked, and the
//!   findings of C39 and C53). When node is missing: a note on a workstation, a failure in CI on Linux (so a missing oracle cannot
//!   let the comparison pass unseen), a `::notice::` on the other CI legs (the Linux leg runs it).
//! - the host: `--only`, `--files`, run records, `not measured`, `check broken`, the report shape and the CLI.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check::mask::mask_rust;
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{RunOptions, registry, run_checks, run_checks_in};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The checks the recorded JS findings cover (C39 and C53); later checks have their own parity tests (check_c119.rs).
fn legacy_registry() -> Vec<Box<dyn paddock_dev::check::Check>> {
    registry().into_iter().filter(|c| matches!(c.id(), "C39" | "C53")).collect()
}

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    crate::full_tree(manifest().ancestors().nth(2).unwrap())
}

fn fixtures() -> PathBuf {
    manifest().join("tests/fixtures/check")
}

fn skip(reason: &str) {
    let ci = ["CI", "GITHUB_ACTIONS"].iter().any(|k| std::env::var(k).is_ok_and(|v| v == "true"));
    if ci && cfg!(target_os = "linux") {
        panic!("the live comparison cannot run in CI: {reason}");
    }
    if ci {
        println!("::notice::the live comparison against the conformance host was skipped on this host: {reason}. The Linux leg runs it.");
    } else {
        eprintln!("skipped: {reason}");
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "a test spawns node, git or the built binary; it ends them itself"
)]
fn has_node() -> bool {
    Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// The fixture cases materialized into a scratch tree (`a__b__c.rs.txt` is `a/b/c.rs`).
fn materialize() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for e in std::fs::read_dir(fixtures().join("cases")).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().into_owned();
        let rel = name.trim_end_matches(".txt").replace("__", "/");
        let to = d.path().join(&rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixtures().join("cases").join(&name), to).unwrap();
    }
    d
}

fn options(root: &Path) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        app: "fixture".into(),
        only: Some(vec!["C39".into(), "C53".into()]),
        files: None,
        today: "2026-11-10".into(),
        base_ref: Some("origin/main".into()),
    }
}

fn read_json(p: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// The part of a report the JS host also writes.
fn shared_part(v: &Value) -> Value {
    json!({"app": v["app"], "checks": v["checks"], "findings": v["findings"], "waived": v["waived"], "errors": v["errors"], "warnings": v["warnings"]})
}

#[test]
fn fixture_findings_equal_the_recorded_js_findings_exactly() {
    let tree = materialize();
    let r = run_checks(&options(tree.path()), &legacy_registry()).unwrap();
    let got = shared_part(&r.json());
    let want = shared_part(&read_json(&fixtures().join("expected.json")));
    // Same values and, as text, the same key order and finding order.
    assert_eq!(got, want);
    assert_eq!(
        serde_json::to_string_pretty(&got).unwrap(),
        serde_json::to_string_pretty(&want).unwrap()
    );
    assert!(
        r.errors() > 40 && r.waived.len() == 3,
        "the fixture must exercise many findings and the waivers"
    );
}

/// The tree reports its work: once when the listing is done and once per file read from disk (a cached read is no new work), so a
/// run over a slow disk can be judged on progress (ADR 0071).
#[test]
fn a_tree_reports_the_listing_and_every_file_it_reads_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}").unwrap();
    std::fs::write(dir.path().join("b.rs"), "fn b() {}").unwrap();
    let ticks = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let seen = ticks.clone();
    let tree = Tree::open(dir.path()).with_progress(move || seen.set(seen.get() + 1));
    assert_eq!(ticks.get(), 0, "nothing is read until asked");
    assert_eq!(tree.files().len(), 2);
    assert_eq!(ticks.get(), 1, "the listing");
    tree.files();
    assert_eq!(ticks.get(), 1, "a listing is made once");
    tree.read("a.rs").unwrap();
    tree.read("b.rs").unwrap();
    tree.read("a.rs").unwrap();
    assert_eq!(ticks.get(), 3, "two files read from disk, the repeat served from the cache");
}

#[test]
fn fixture_masks_equal_the_recorded_js_masks() {
    let want = read_json(&fixtures().join("masked.json"));
    let tree = materialize();
    let mut n = 0;
    for (rel, masked) in want.as_object().unwrap() {
        let text = String::from_utf8_lossy(&std::fs::read(tree.path().join(rel)).unwrap()).into_owned();
        assert_eq!(mask_rust(&text), masked.as_str().unwrap(), "{rel}");
        n += 1;
    }
    assert!(n >= 14, "{n} cases masked");
}

#[allow(
    clippy::disallowed_methods,
    reason = "a test spawns node, git or the built binary; it ends them itself"
)]
/// Runs a node program (an ES module given on the command line) in the repository and returns its standard output.
fn node(driver: &str, env: &[(&str, String)]) -> String {
    let mut c = Command::new("node");
    c.args(["--input-type=module", "-e", driver]).current_dir(repo_root());
    // The parity runs measure growth against `origin/main` of their own repository, whatever the caller's environment says.
    c.env_remove("PADDOCK_BASE_REF");
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.output().expect("node");
    assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// The longest a whole-repository live run may finish no unit of work (a file read, parsed or masked) before it is called hung
/// (ADR 0071). A slow disk or a loaded host stretches the run, never this: 42-49 s quiet against over 240 s under I/O load, and
/// every one of those seconds finished files. nextest's slow-timeout (`.config/nextest.toml`) is only the backstop.
// conformance: hang-guard
const LIVE_STALL: std::time::Duration = std::time::Duration::from_secs(120);

#[allow(
    clippy::disallowed_methods,
    reason = "a test spawns node, git or the built binary; it ends them itself"
)]
/// [`node`] for a driver that prints a `.` line per unit of work: every such line is a tick of `progress`, and everything else it
/// prints is the answer, returned. The child's end (its exit status) ends the wait; a hang ends it through the progress guard
/// around the call.
fn node_ticking(driver: &str, env: &[(&str, String)], progress: &paddock_testkit::Progress) -> String {
    use std::io::{BufRead, Read};
    let mut c = Command::new("node");
    c.args(["--input-type=module", "-e", driver])
        .current_dir(repo_root())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (k, v) in env {
        c.env(k, v);
    }
    let mut child = c.spawn().expect("node");
    let mut stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        drop(stderr.read_to_string(&mut text));
        text
    });
    let mut answer = String::new();
    for line in std::io::BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        if line == "." {
            progress.tick();
        } else {
            answer.push_str(&line);
            answer.push('\n');
        }
    }
    let status = child.wait().unwrap();
    let errors = errors.join().unwrap();
    assert!(status.success(), "node failed: {errors}");
    answer
}

/// [`JS_RUN`] printing a `.` line for every file the conformance runner reads (it reads the tree through `readFileSync`, which
/// the prefix wraps before the runner is imported), so [`node_ticking`] sees the JS host at work, as it does the Rust one.
const JS_RUN_TICKING: &str = r#"
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
const read = fs.readFileSync;
fs.readFileSync = (...args) => { process.stdout.write('.\n'); return read(...args); };
syncBuiltinESMExports();
"#;

const JS_RUN: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { runChecks } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/runner.mjs')).href);
const now = process.env.RT5_NOW ? new Date(process.env.RT5_NOW) : undefined;
const r = await runChecks({ root: process.env.RT5_ROOT, app: process.env.RT5_APP, only: ['C39', 'C53'], options: { now } });
process.stdout.write(JSON.stringify(r));
"#;

#[test]
fn live_fixture_findings_equal_the_js_host() {
    if !has_node() {
        return skip("node is not installed");
    }
    let tree = materialize();
    let js: Value = serde_json::from_str(&node(
        JS_RUN,
        &[
            ("RT5_ROOT", tree.path().display().to_string()),
            ("RT5_APP", "fixture".into()),
            ("RT5_NOW", "2026-11-10T12:00:00Z".into()),
        ],
    ))
    .unwrap();
    let r = run_checks(&options(tree.path()), &legacy_registry()).unwrap();
    assert_eq!(shared_part(&r.json()), shared_part(&js));
}

#[test]
fn live_tree_findings_equal_the_js_host() {
    if !has_node() {
        return skip("node is not installed");
    }
    let root = repo_root();
    // The JS host prints a line per file it reads and the Rust host ticks per file: both are judged on progress.
    let js_root = root.clone();
    let js = paddock_testkit::guarded("the JS check host over the live tree", LIVE_STALL, move |progress| {
        let driver = format!("{JS_RUN_TICKING}{JS_RUN}");
        let env = [("RT5_ROOT", js_root.display().to_string()), ("RT5_APP", "paddock".into())];
        let answer = node_ticking(&driver, &env, &progress);
        serde_json::from_str::<Value>(answer.trim()).unwrap()
    });
    // The Rust host reports every file it reads: a slow disk passes, a run that reads nothing for LIVE_STALL does not.
    let r = paddock_testkit::guarded("the Rust check host over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let opts = RunOptions {
                app: "paddock".into(),
                today: paddock_dev::check::today_utc(),
                base_ref: None,
                ..options(&root)
            };
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &opts, &legacy_registry()).unwrap()
        }
    });
    let (got, want) = (shared_part(&r.json()), shared_part(&js));
    assert_eq!(
        got["findings"], want["findings"],
        "findings of C39, C53 and the waiver engine on this tree"
    );
    assert_eq!(got["waived"], want["waived"]);
    assert_eq!(got, want);
    assert!(r.waived.len() > 5, "the tree's own waivers must be exercised");
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "a test spawns node, git or the built binary; it ends them itself"
)]
fn live_mask_equals_the_js_mask_on_every_tracked_rust_file() {
    if !has_node() {
        return skip("node is not installed");
    }
    let root = repo_root();
    let listed = Command::new("git")
        .args(["ls-files", "-z", "--", "*.rs"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(listed.status.success());
    let files: Vec<String> = String::from_utf8_lossy(&listed.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    assert!(files.len() > 1000, "the golden needs every tracked .rs file, found {}", files.len());
    // The JS side masks every file and prints a line per file; the Rust side then masks and compares each file. Both count as
    // progress, so a disk that is slow under load passes and one that finishes no file for LIVE_STALL does not.
    let differ = paddock_testkit::guarded("masking every tracked .rs file in JS and in Rust", LIVE_STALL, move |progress| {
        let out = tempfile::tempdir().unwrap();
        const DRIVER: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
import { readFileSync, writeFileSync } from 'node:fs';
const { maskRust } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/checks/C39-freed-port.mjs')).href);
const files = JSON.parse(readFileSync(process.env.RT5_LIST, 'utf8'));
files.forEach((f, i) => { writeFileSync(join(process.env.RT5_OUT, `${i}.txt`), maskRust(readFileSync(f, 'utf8')), 'utf8'); process.stdout.write('.\n'); });
"#;
        let list = out.path().join("list.json");
        std::fs::write(&list, serde_json::to_string(&files).unwrap()).unwrap();
        node_ticking(
            DRIVER,
            &[
                ("RT5_LIST", list.display().to_string()),
                ("RT5_OUT", out.path().display().to_string()),
            ],
            &progress,
        );
        let mut differ = Vec::new();
        for (i, f) in files.iter().enumerate() {
            let text = String::from_utf8_lossy(&std::fs::read(root.join(f)).unwrap()).into_owned();
            let want = std::fs::read(out.path().join(format!("{i}.txt"))).unwrap();
            if mask_rust(&text).as_bytes() != want.as_slice() {
                differ.push(f.clone());
            }
            progress.tick();
        }
        (differ, files.len())
    });
    let (differ, total) = differ;
    assert!(
        differ.is_empty(),
        "{} of {} files are masked differently from the JS maskRust: {:?}",
        differ.len(),
        total,
        &differ[..differ.len().min(5)]
    );
}

#[test]
fn only_selects_checks_and_an_unknown_id_is_refused() {
    let tree = materialize();
    let mut o = options(tree.path());
    o.only = Some(vec!["C53".into()]);
    let r = run_checks(&o, &legacy_registry()).unwrap();
    assert_eq!(r.checks, ["C53"]);
    assert!(r.findings.iter().all(|f| f.check != "C39"));
    o.only = Some(vec!["C39".into(), "C999".into()]);
    assert_eq!(run_checks(&o, &legacy_registry()).err().unwrap().0, ["C999"]);
}

#[test]
fn files_scopes_the_run_and_the_record_says_so() {
    let tree = materialize();
    let mut o = options(tree.path());
    o.files = Some(vec![
        "crates/a/src/c53_bad.rs".into(),
        "./crates/a/tests/c39_good.rs".into(),
        "crates/a/src/gone.rs".into(),
    ]);
    let r = run_checks(&o, &legacy_registry()).unwrap();
    let files: std::collections::BTreeSet<&str> = r
        .findings
        .iter()
        .filter(|f| f.check.starts_with('C'))
        .map(|f| f.file.as_str())
        .collect();
    assert_eq!(files, ["crates/a/src/c53_bad.rs"].into_iter().collect());
    let (c39, kept39) = (&r.runs[0].record, r.runs[0].kept);
    assert_eq!((c39.scope.name(), c39.probes, kept39), ("files", 2, 0));
    assert!(c39.measured(), "the two .rs files in scope were read");
    // No waiver warning for waivers of files outside the scope.
    assert!(
        r.findings.iter().all(|f| f.check != "W003"),
        "{:?}",
        r.findings.iter().filter(|f| f.check == "W003").collect::<Vec<_>>()
    );
}

/// A tree with no waiver problems: `a.rs` (a file C39 and C53 read, with no findings) and `notes.md` (one they do not).
fn small_tree() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(d.path().join("notes.md"), "x").unwrap();
    d
}

#[test]
fn a_scoped_run_that_reads_no_rust_file_is_not_applicable_and_not_an_error() {
    let tree = small_tree();
    let mut o = options(tree.path());
    o.only = Some(vec!["C39".into()]);
    o.files = Some(vec!["notes.md".into()]);
    let r = run_checks(&o, &legacy_registry()).unwrap();
    for e in &r.runs {
        assert_eq!(e.record.status(e.kept > 0).name(), "not applicable", "{}", e.record.check);
    }
    assert_eq!(r.errors(), 0, "a push gate on a docs change must not fail: {:?}", r.findings);
    assert!(r.findings.iter().all(|f| !f.check.starts_with('C')));
}

#[test]
fn files_that_are_all_unknown_are_an_unwaivable_error_and_each_is_a_warning() {
    let tree = small_tree();
    let mut o = options(tree.path());
    o.only = Some(vec!["C39".into()]);
    o.files = Some(vec!["typo/one.rs".into(), "typo/two.rs".into()]);
    let r = run_checks(&o, &legacy_registry()).unwrap();
    assert_eq!(r.warnings(), 2, "one counted warning per unknown entry: {:?}", r.findings);
    let broken: Vec<_> = r.findings.iter().filter(|f| f.unwaivable).collect();
    assert_eq!(broken.len(), 1, "{:?}", r.findings);
    assert!(broken[0].message.contains("--files"), "{}", broken[0].message);
    assert_eq!(r.runs[0].record.status(true).name(), "not measured");
    assert!(r.errors() >= 1);
}

#[test]
fn a_whole_tree_run_that_looks_at_nothing_is_an_error_no_waiver_covers() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("readme.md"), "x").unwrap();
    std::fs::write(
        d.path().join("paddock.waivers.toml"),
        "[[waiver]]\ncheck = \"C39\"\npath = \"*\"\nreason = \"try\"\nexpires = \"2099-01-01\"\n",
    )
    .unwrap();
    let r = run_checks(&options(d.path()), &legacy_registry()).unwrap();
    let broken: Vec<_> = r.findings.iter().filter(|f| f.unwaivable).collect();
    assert_eq!(broken.len(), 2, "{:?}", r.findings);
    assert!(broken[0].message.starts_with("not measured"));
    assert!(r.waived.is_empty());
    assert_eq!(r.runs[0].record.status(true).name(), "not measured");
}

#[test]
fn an_empty_tree_fails_the_precondition() {
    let d = tempfile::tempdir().unwrap();
    let r = run_checks(&options(d.path()), &legacy_registry()).unwrap();
    assert!(r.runs.iter().all(|e| !e.record.precondition.met));
    assert!(r.errors() >= 2);
}

#[test]
fn the_report_has_the_js_shape_and_the_run_records() {
    let tree = materialize();
    let v = run_checks(&options(tree.path()), &legacy_registry()).unwrap().json();
    for k in ["app", "checks", "findings", "waived", "errors", "warnings", "runs"] {
        assert!(v.get(k).is_some(), "{k}");
    }
    let run = &v["runs"][0];
    for k in [
        "check",
        "ran",
        "completed",
        "precondition",
        "probes",
        "scope",
        "os",
        "control",
        "status",
    ] {
        assert!(run.get(k).is_some(), "{k}");
    }
    assert_eq!(run["scope"], "root");
    assert_eq!(run["os"], std::env::consts::OS);
    assert!(run["probes"].as_u64().unwrap() >= 10);
}

#[allow(
    clippy::disallowed_methods,
    reason = "a test spawns node, git or the built binary; it ends them itself"
)]
fn dev(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_paddock-dev")).args(args).output().unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn the_command_exits_5_on_findings_and_prints_one_json_object() {
    let tree = materialize();
    let root = tree.path().display().to_string();
    let (code, out, _) = dev(&["--json", "check", "run", "--root", &root, "--only", "c53", "--app", "fixture"]);
    assert_eq!(code, 5, "findings exit with 5");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["checks"], json!(["C53"]));
    let clean = tempfile::tempdir().unwrap();
    std::fs::write(clean.path().join("ok.rs"), "fn a() {}\n").unwrap();
    let (code, out, _) = dev(&[
        "check",
        "run",
        "--root",
        &clean.path().display().to_string(),
        "--files",
        "ok.rs",
        "--only",
        "C39",
    ]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("run C39: clean"), "{out}");
    let (code, _, err) = dev(&["check", "run", "--root", &root, "--only", "C1"]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown check: C1"), "{err}");
}

#[test]
fn the_command_writes_the_report_to_out() {
    let tree = materialize();
    let out = tree.path().join("report.json");
    let (_, _, _) = dev(&[
        "check",
        "run",
        "--root",
        &tree.path().display().to_string(),
        "--out",
        &out.display().to_string(),
    ]);
    let v = read_json(&out);
    assert!(v["findings"].as_array().unwrap().len() > 40);
}

// ---- ER-33: every way a run can fail to look adds an error no waiver covers -------------------------------------------------

use paddock_dev::check::finding::{Finding, Severity};
use paddock_dev::check::record::Precondition;
use paddock_dev::check::{Check, CheckOutput, Control, Ctx};

#[derive(Clone, Copy, Debug)]
enum Way {
    Fine,
    Panics,
    Incomplete,
    ControlFails,
    PreconditionUnmet,
    ZeroProbes,
}

/// A check that always has one waivable finding at `a.rs:1`, and misbehaves in one way on the real tree (never on its control tree).
struct Stub {
    id: &'static str,
    way: Way,
}

impl Check for Stub {
    fn id(&self) -> &'static str {
        self.id
    }
    fn rule(&self) -> u32 {
        1
    }
    fn title(&self) -> &'static str {
        "stub"
    }
    fn control(&self) -> Control {
        Control {
            files: vec![("control/x.rs", "x")],
            expect: match self.way {
                Way::ControlFails => vec![],
                _ => vec![("a.rs", 1)],
            },
        }
    }
    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let in_control = ctx.files().iter().any(|f| f == "control/x.rs");
        let way = if in_control { Way::Fine } else { self.way };
        if matches!(way, Way::Panics) {
            panic!("boom");
        }
        CheckOutput {
            findings: vec![Finding::error(self.id, 1, "a.rs", 1, "a finding", "fix it")],
            probes: if matches!(way, Way::ZeroProbes) {
                0
            } else {
                ctx.files().iter().filter(|f| f.ends_with(".rs")).count()
            },
            completed: !matches!(way, Way::Incomplete),
            precondition: Precondition {
                met: !matches!(way, Way::PreconditionUnmet),
                detail: "the stub's input".into(),
            },
        }
    }
}

fn stub_tree(ids: &[&str]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.rs"), "x").unwrap();
    let waivers: String = ids
        .iter()
        .map(|id| format!("[[waiver]]\ncheck = \"{id}\"\npath = \"*\"\nreason = \"waive all\"\nexpires = \"2099-01-01\"\n"))
        .collect();
    std::fs::write(d.path().join("paddock.waivers.toml"), waivers).unwrap();
    d
}

fn stub_run(way: Way, only_fine_is_clean: bool) -> paddock_dev::check::RunResult {
    let tree = stub_tree(&["S1"]);
    let reg: Vec<Box<dyn Check>> = vec![Box::new(Stub { id: "S1", way })];
    let mut o = options(tree.path());
    o.only = None;
    let r = run_checks(&o, &reg).unwrap();
    assert_eq!(
        r.waived.len(),
        usize::from(!matches!(way, Way::Panics)),
        "{way:?}: the stub's own finding is waived by the `*` waiver; a crash has none and its error is not waived"
    );
    if only_fine_is_clean {
        assert_eq!(r.errors(), 0, "{:?}", r.findings);
    }
    r
}

/// Every scope times every way of not looking, through the host: the record's status and the unwaivable error agree.
#[test]
fn status_and_error_agree_for_every_scope_and_way_through_the_host() {
    let scopes: [(&str, Option<Vec<&str>>); 5] = [
        ("root", None),
        ("files a.rs", Some(vec!["a.rs"])),
        ("files notes.md", Some(vec!["notes.md"])),
        ("files unknown", Some(vec!["typo.rs"])),
        ("files known and unknown", Some(vec!["a.rs", "typo.rs"])),
    ];
    for way in [
        Way::Fine,
        Way::Panics,
        Way::Incomplete,
        Way::ControlFails,
        Way::PreconditionUnmet,
        Way::ZeroProbes,
    ] {
        for (sname, files) in &scopes {
            let tree = stub_tree(&["S1"]);
            std::fs::write(tree.path().join("notes.md"), "x").unwrap();
            let reg: Vec<Box<dyn Check>> = vec![Box::new(Stub { id: "S1", way })];
            let mut o = options(tree.path());
            o.only = None;
            o.files = files.as_ref().map(|f| f.iter().map(|s| s.to_string()).collect());
            let r = run_checks(&o, &reg).unwrap();
            let e = &r.runs[0];
            let status = e.record.status(e.kept > 0).name();
            let unwaivable = r.findings.iter().filter(|f| f.unwaivable).count();
            let all_unknown = files.as_ref().is_some_and(|f| f.iter().all(|x| *x == "typo.rs"));
            let expect = match (way, all_unknown, *sname) {
                (Way::ControlFails, _, _) => "check broken",
                (Way::Panics | Way::Incomplete | Way::PreconditionUnmet, _, _) => "not measured",
                (_, true, _) => "not measured",
                (Way::ZeroProbes, _, s) if s != "root" => "not applicable",
                (Way::ZeroProbes, _, _) => "not measured",
                (Way::Fine, _, "files notes.md") => "not applicable",
                (Way::Fine, _, _) => "clean",
            };
            assert_eq!(status, expect, "{way:?} / {sname}");
            let fails = matches!(status, "not measured" | "check broken");
            assert_eq!(unwaivable, usize::from(fails), "{way:?} / {sname}: {status}: {:?}", r.findings);
            assert_eq!(r.errors() > 0, fails, "{way:?} / {sname}: the exit follows the status");
        }
    }
}

#[test]
fn a_run_that_looks_fine_and_whose_findings_are_all_waived_passes() {
    let r = stub_run(Way::Fine, true);
    assert!(r.findings.is_empty());
    assert_eq!((r.runs[0].raw, r.runs[0].kept), (1, 0));
    assert_eq!(r.runs[0].record.status(r.runs[0].kept > 0).name(), "clean");
}

#[test]
fn every_way_of_not_looking_is_an_unwaivable_error_next_to_waived_findings() {
    for (way, word) in [
        (Way::Incomplete, "did not complete"),
        (Way::Panics, "crashed: boom"),
        (Way::ControlFails, "positive control"),
        (Way::PreconditionUnmet, "precondition failed"),
        (Way::ZeroProbes, "no file"),
    ] {
        let r = stub_run(way, false);
        let broken: Vec<&Finding> = r.findings.iter().filter(|f| f.unwaivable).collect();
        assert_eq!(broken.len(), 1, "{way:?}: {:?}", r.findings);
        assert!(broken[0].message.contains(word), "{way:?}: {}", broken[0].message);
        assert!(r.errors() >= 1, "{way:?}: the run fails");
        assert_ne!(r.runs[0].record.status(r.runs[0].kept > 0).name(), "clean", "{way:?}");
    }
}

#[test]
fn a_panicking_check_is_not_waivable_even_by_a_star_waiver() {
    let r = stub_run(Way::Panics, false);
    let broken: Vec<&Finding> = r.findings.iter().filter(|f| f.unwaivable).collect();
    assert_eq!(broken.len(), 1, "exactly one unwaivable finding: {:?}", r.findings);
    assert_eq!(broken[0].check, "S1");
    assert_eq!(broken[0].severity, Severity::Error);
    assert_eq!(
        broken[0].message, "not measured: S1 it crashed: boom",
        "the crash is named in the one unwaivable error"
    );
    assert!(
        r.waived.iter().all(|w| w.finding.line == 1 && !w.finding.unwaivable),
        "the crash finding is never in `waived`"
    );
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_next_to_a_waived_finding_fails_the_run() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.rs"), "fn a() { Command::new(\"taskkill\"); }\n").unwrap();
    std::fs::write(d.path().join("b.rs"), "fn b() {}\n").unwrap();
    std::fs::write(
        d.path().join("paddock.waivers.toml"),
        "[[waiver]]\ncheck = \"C53\"\npath = \"a.rs\"\nreason = \"x\"\nexpires = \"2099-01-01\"\n",
    )
    .unwrap();
    std::fs::set_permissions(d.path().join("b.rs"), std::fs::Permissions::from_mode(0o000)).unwrap();
    let readable_anyway = std::fs::read(d.path().join("b.rs")).is_ok(); // root ignores the mode (CI containers)
    let mut o = options(d.path());
    o.only = Some(vec!["C53".into()]);
    let r = run_checks(&o, &legacy_registry()).unwrap();
    std::fs::set_permissions(d.path().join("b.rs"), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(r.waived.len(), 1);
    if readable_anyway {
        eprintln!("skipped the failing half: this user can read a mode 000 file");
        assert_eq!(r.errors(), 0);
    } else {
        assert!(
            r.findings.iter().any(|f| f.unwaivable && f.message.contains("did not complete")),
            "{:?}",
            r.findings
        );
        assert!(r.errors() >= 1);
    }
}

#[test]
fn zero_probes_with_a_rust_file_present_and_no_rust_file_at_all_are_both_not_measured() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("empty.rs"), "").unwrap();
    let r = run_checks(&options(d.path()), &legacy_registry()).unwrap();
    assert!(r.runs.iter().all(|e| e.record.precondition.met && e.record.probes == 0));
    assert!(
        r.findings.iter().filter(|f| f.unwaivable).all(|f| f.message.contains("no file")),
        "{:?}",
        r.findings
    );
    assert_eq!(r.errors(), 2);
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("readme.md"), "x").unwrap();
    let r = run_checks(&options(d.path()), &legacy_registry()).unwrap();
    assert!(r.runs.iter().all(|e| !e.record.precondition.met), "no .rs file in the tree");
}

#[test]
fn the_real_checks_pass_their_controls_through_the_host() {
    let tree = materialize();
    let r = run_checks(&options(tree.path()), &legacy_registry()).unwrap();
    assert!(
        r.runs.iter().all(|e| e.record.control),
        "C39 and C53 must see their control sites through the prefilter and the file loop"
    );
}

// ---- --files and the record ------------------------------------------------------------------------------------------------

#[test]
fn files_accepts_backslashes_and_reports_entries_the_tree_does_not_have() {
    let tree = materialize();
    let mut o = options(tree.path());
    o.files = Some(vec![
        r"crates\a\src\c53_bad.rs".into(),
        "crates/a/src/typo.rs".into(),
        "./crates/a/src/c53_good.rs".into(),
    ]);
    let r = run_checks(&o, &legacy_registry()).unwrap();
    assert_eq!(r.unknown_files, ["crates/a/src/typo.rs"]);
    assert!(r.findings.iter().any(|f| f.file == "crates/a/src/c53_bad.rs"));
    assert!(r.text().contains("::warning::--files names 1 file(s)"));
    assert_eq!(r.json()["unknownFiles"], json!(["crates/a/src/typo.rs"]));
    match &r.runs[0].record.scope {
        paddock_dev::check::record::Scope::Files(f) => assert_eq!(f, &["crates/a/src/c53_bad.rs", "crates/a/src/c53_good.rs"]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_record_carries_raw_and_kept_counts_and_the_matched_keys() {
    let tree = materialize();
    let v = run_checks(&options(tree.path()), &legacy_registry()).unwrap().json();
    let c39 = &v["runs"][0];
    assert!(c39["found"]["raw"].as_u64().unwrap() > c39["found"]["kept"].as_u64().unwrap() - 1);
    assert!(
        c39["matched"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k == "crates/a/tests/c39_bad.rs:7"),
        "{}",
        c39["matched"]
    );
}

// ---- a seeded token-soup differential against the JS host ------------------------------------------------------------------

const TOKENS: &[&str] = &[
    "TcpListener::bind(",
    "TcpSocket::bind(",
    "Socket::new(",
    ")",
    "(",
    "{",
    "}",
    ";",
    ".unwrap()",
    "?",
    ".await",
    ".expect(\"e\")",
    ".local_addr()",
    ".port()",
    "let l = ",
    "let mut s = ",
    "let p = ",
    "l",
    "s",
    "p",
    "drop(l);",
    "drop(s);",
    "s.bind(&a);",
    "s.set_reuse(true);",
    "\n",
    "\n",
    " ",
    "\"",
    "'",
    "'a'",
    "'\\n'",
    "'\\u{1F600}'",
    "r#\"",
    "\"#",
    "r\"",
    "br\"",
    "xr\"",
    "cr\"",
    "//",
    "/*",
    "*/",
    "\\",
    "é",
    "\u{1F600}",
    "\r\n",
    " ",
    " ",
    "TerminateProcess(",
    "TerminateJobObject (",
    "\"taskkill\"",
    "\"taskkill.exe\"",
    "\"Stop-Process -Id\"",
    "\"/bin/kill\"",
    "Command::new(\"kill\")",
    "command(\"pkill\")",
    "#![allow(clippy::disallowed_methods)]",
    "// ADR 0033 W\u{50}-3",
    "// ADR 0033 WP-\u{661}",
    "// conformance: pid-kill — ok reason",
    "// conformance: pid-kill",
    "// conformance: freed-port — fine here",
    "// conformance: freed-port — \u{1F600}a",
    "// conformance: freed-port -- a",
    "// conformance: freed-port",
    "use std::net::TcpListener as L;",
    "L::bind(",
    "fn f() ",
    "x",
    "=",
    ":",
    "kill",
    "#",
];

fn soup(seed: u64, files: usize, root: &Path) {
    let mut s = seed;
    let mut rnd = |n: usize| {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((s >> 33) as usize) % n
    };
    for i in 0..files {
        let mut t = String::new();
        for _ in 0..(5 + rnd(80)) {
            t.push_str(TOKENS[rnd(TOKENS.len())]);
        }
        let p = root.join(format!("crates/f/src/f{i}.rs"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, t).unwrap();
    }
}

#[test]
fn live_token_soup_findings_equal_the_js_host() {
    if !has_node() {
        return skip("node is not installed");
    }
    for seed in [1u64, 2, 3] {
        let d = tempfile::tempdir().unwrap();
        soup(seed, 250, d.path());
        let js: Value = serde_json::from_str(&node(
            JS_RUN,
            &[("RT5_ROOT", d.path().display().to_string()), ("RT5_APP", "soup".into())],
        ))
        .unwrap();
        let mut o = options(d.path());
        o.app = "soup".into();
        let r = run_checks(&o, &legacy_registry()).unwrap();
        assert_eq!(shared_part(&r.json())["findings"], js["findings"], "seed {seed}");
        assert!(
            js["findings"].as_array().unwrap().len() > 20,
            "seed {seed}: the soup must hit the checks"
        );
    }
}

const BASELINE: &str = "conformance/baselines/C53.json";

#[allow(
    clippy::disallowed_methods,
    reason = "a test spawns node, git or the built binary; it ends them itself"
)]
fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// The fixture tree as a git repository whose merge base holds a baseline with one entry fewer than the working tree's (the baseline
/// GREW by one entry): `origin/main` is the first commit, `topic` the second.
fn grown_baseline_repo() -> tempfile::TempDir {
    baseline_repo(|sites| sites.truncate(1))
}

/// Like [`grown_baseline_repo`], with the merge base's baseline entries edited by `edit` first.
fn baseline_repo(edit: impl FnOnce(&mut Vec<Value>)) -> tempfile::TempDir {
    let tree = materialize();
    let root = tree.path();
    let full = std::fs::read_to_string(root.join(BASELINE)).unwrap();
    let mut doc: Value = serde_json::from_str(&full).unwrap();
    let mut kept = doc["sites"].as_array().unwrap().clone();
    edit(&mut kept);
    doc["sites"] = Value::Array(kept);
    std::fs::write(root.join(BASELINE), serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "base"]);
    git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(root, &["checkout", "-q", "-b", "topic"]);
    std::fs::write(root.join(BASELINE), full).unwrap();
    git(root, &["commit", "-q", "-a", "-m", "grow"]);
    tree
}

fn baseline_findings(r: &paddock_dev::check::RunResult) -> Vec<String> {
    r.findings
        .iter()
        .filter(|f| f.file == BASELINE)
        .map(|f| f.message.clone())
        .collect()
}

#[test]
fn live_baseline_growth_equals_the_js_host_in_a_git_repository() {
    if !has_node() {
        return skip("node is not installed");
    }
    let tree = grown_baseline_repo();
    let js: Value = serde_json::from_str(&node(
        JS_RUN,
        &[
            ("RT5_ROOT", tree.path().display().to_string()),
            ("RT5_APP", "fixture".into()),
            ("RT5_NOW", "2026-11-10T12:00:00Z".into()),
        ],
    ))
    .unwrap();
    let r = run_checks(&options(tree.path()), &legacy_registry()).unwrap();
    assert_eq!(shared_part(&r.json()), shared_part(&js));
    let grew = baseline_findings(&r);
    assert_eq!(grew.len(), 1, "{grew:?}");
    assert!(grew[0].contains("the baseline grew: 1 entry is"), "{grew:?}");
}

/// The merge base lists the first site under another file (the code moved since): same hash, so not growth, on both hosts.
#[test]
fn a_moved_site_is_not_growth_on_either_host_but_a_new_one_beside_it_is() {
    if !has_node() {
        return skip("node is not installed");
    }
    let move_first = |sites: &mut Vec<Value>| sites[0]["file"] = json!("crates/a/src/old_home.rs");
    for (name, tree, grew) in [
        ("moved only", baseline_repo(move_first), 0),
        (
            "moved and one new",
            baseline_repo(|sites| {
                move_first(sites);
                sites.truncate(1);
            }),
            1,
        ),
    ] {
        let js: Value = serde_json::from_str(&node(
            JS_RUN,
            &[
                ("RT5_ROOT", tree.path().display().to_string()),
                ("RT5_APP", "fixture".into()),
                ("RT5_NOW", "2026-11-10T12:00:00Z".into()),
            ],
        ))
        .unwrap();
        let r = run_checks(&options(tree.path()), &legacy_registry()).unwrap();
        assert_eq!(shared_part(&r.json()), shared_part(&js), "{name}: the hosts agree");
        let msgs = baseline_findings(&r);
        let grown: Vec<_> = msgs.iter().filter(|m| m.contains("the baseline grew")).collect();
        assert_eq!(grown.len(), grew, "{name}: {msgs:?}");
        if grew == 1 {
            assert!(grown[0].contains("the baseline grew: 1 entry is"), "{name}: {msgs:?}");
        }
        assert!(
            msgs.iter().any(|m| m.starts_with("moved: ")),
            "{name}: the move is reported as a warning: {msgs:?}"
        );
    }
}

/// The site check is exact: a baseline entry whose file still exists does not cover the same line in another file (a deleted line in
/// one file, added in a new one, is an error); an entry whose file is gone does, as a warning that names both files. Both hosts agree.
#[test]
fn a_site_moved_from_a_live_file_is_an_error_and_from_a_gone_file_a_warning_on_both_hosts() {
    if !has_node() {
        return skip("node is not installed");
    }
    for (name, from, errors) in [
        ("old file still exists", "crates/a/src/c53_good.rs", true),
        ("old file is gone", "crates/a/src/renamed_away.rs", false),
    ] {
        let tree = materialize();
        let mut doc: Value = read_json(&tree.path().join(BASELINE));
        doc["sites"][0]["file"] = json!(from);
        std::fs::write(tree.path().join(BASELINE), serde_json::to_string_pretty(&doc).unwrap()).unwrap();
        let js: Value = serde_json::from_str(&node(
            JS_RUN,
            &[
                ("RT5_ROOT", tree.path().display().to_string()),
                ("RT5_APP", "fixture".into()),
                ("RT5_NOW", "2026-11-10T12:00:00Z".into()),
            ],
        ))
        .unwrap();
        let r = run_checks(&options(tree.path()), &legacy_registry()).unwrap();
        assert_eq!(shared_part(&r.json()), shared_part(&js), "{name}: the hosts agree");
        let site = r
            .findings
            .iter()
            .filter(|f| f.file == "crates/a/src/c53_liveness_bad.rs" && f.line == 5);
        let (errs, warns): (Vec<_>, Vec<_>) = site.partition(|f| f.json()["severity"] == "error");
        assert_eq!(!errs.is_empty(), errors, "{name}: {errs:?}");
        assert_eq!(
            warns.iter().any(|f| f
                .message
                .contains(&format!("moved from {from} to crates/a/src/c53_liveness_bad.rs"))),
            !errors,
            "{name}: {warns:?}"
        );
    }
}

#[test]
fn a_scoped_run_over_the_baseline_catches_growth_and_reports_stale_as_not_measured() {
    let tree = grown_baseline_repo();
    let mut o = options(tree.path());
    o.files = Some(vec![BASELINE.into()]);
    let r = run_checks(&o, &legacy_registry()).unwrap();
    let c53 = r.runs.iter().find(|e| e.record.check == "C53").unwrap();
    assert!(
        baseline_findings(&r).iter().any(|m| m.contains("the baseline grew: 1 entry is")),
        "growth is caught: {:?}",
        r.findings
    );
    assert_eq!(
        c53.record.status(true).name(),
        "not measured",
        "the stale-entry half cannot be judged from a scoped run"
    );
    assert!(
        r.findings
            .iter()
            .any(|f| f.check == "C53" && f.message.contains("stale") && f.message.starts_with("not measured")),
        "{:?}",
        r.findings
    );
    // Not in scope: the baseline is left alone (as before): a scoped run on one Rust file says nothing about it.
    let mut other = options(tree.path());
    other.files = Some(vec!["crates/a/src/c53_good.rs".into()]);
    let r = run_checks(&other, &legacy_registry()).unwrap();
    assert!(baseline_findings(&r).is_empty());
}

#[test]
fn an_invalid_baseline_fails_closed_with_one_error_on_both_hosts() {
    let tree = materialize();
    for bad in ["{ not json", "{\"sites\": 3}", "[]"] {
        std::fs::write(tree.path().join(BASELINE), bad).unwrap();
        let r = run_checks(&options(tree.path()), &legacy_registry()).unwrap();
        let msgs = baseline_findings(&r);
        assert_eq!(msgs.len(), 1, "{bad}: {msgs:?}");
        assert!(msgs[0].contains("is not JSON with a `sites` list"), "{msgs:?}");
        if has_node() {
            let js: Value = serde_json::from_str(&node(
                JS_RUN,
                &[
                    ("RT5_ROOT", tree.path().display().to_string()),
                    ("RT5_APP", "fixture".into()),
                    ("RT5_NOW", "2026-11-10T12:00:00Z".into()),
                ],
            ))
            .unwrap();
            assert_eq!(shared_part(&r.json()), shared_part(&js), "{bad}");
        }
    }
}

// ---- C121 (ADR 0098 WP-2): the Rust host's check against its JavaScript port ----------------------------------------------------

/// A tree with every shape C121 reads: flagged and clean registries and tests, an allowed folder, a fixture folder, comments.
fn c121_tree() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let files: &[(&str, &str)] = &[
        (
            "conformance/c121-allow.json",
            "{\"allow\":[{\"path\":\"conformance/test/\",\"reason\":\"tests of checks of workflows\"},{\"path\":\"scripts/ok.test.mjs\",\"reason\":\"reads a workflow as its subject\"}]}",
        ),
        (
            "docs/a.toml",
            "[[g]]\ncheck = [\"sh\", \"-c\", \"sed -n p .gitea/workflows/ci.yml\"]\n",
        ),
        (
            "docs/b.toml",
            "# grep .gitea/workflows/ci.yml in a comment\ncheck = [\"./dev\", \"ci\", \"conformance-args\"]\n",
        ),
        (
            "docs/c.toml",
            "check = [\"sh\", \"-c\", \"rg -n x workflows/ci.yml | awk 1\"]\nwhat = \"the sedate .gitea/workflows/ci.yml\"\n",
        ),
        (
            "tools/t/tests/it/a.rs",
            "fn a() {\n    let _ = Regex::new(\"x\"); // .gitea/workflows/ci.yml\n}\n",
        ),
        (
            "tools/t/tests/it/b.rs",
            "fn b() {\n    let _ = read(\".gitea/workflows/ci.yml\");\n}\n",
        ),
        (
            "tools/t/src/tests.rs",
            "fn c() { Command::new(\"grep\").arg(\".gitea/workflows/x.yml\"); }\n",
        ),
        (
            "conformance/test/x.test.mjs",
            "const t = a.match(/x/); // .gitea/workflows/ci.yml\n",
        ),
        ("scripts/ok.test.mjs", "sed .gitea/workflows/ci.yml\n"),
        (
            "scripts/bad.test.mjs",
            "const r = new RegExp(\"x\").test(readFileSync('.gitea/workflows/ci.yml'));\nconst m = t.matchAll(/x/g); // workflows/ci.yml\n",
        ),
        (
            "tools/t/tests/fixtures/f.rs",
            "Command::new(\"grep\"); // .gitea/workflows/ci.yml\n",
        ),
        ("a/b_test.rs", "perl -ne print .gitea/workflows/ci.yml\r\nx\r\n"),
    ];
    for (p, t) in files {
        let to = d.path().join(p);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, t).unwrap();
    }
    d
}

const JS_RUN_C121: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { runChecks } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/runner.mjs')).href);
const r = await runChecks({ root: process.env.RT5_ROOT, app: process.env.RT5_APP, only: ['C121'], options: {} });
process.stdout.write(JSON.stringify(r));
"#;

fn c121_options(root: &Path, app: &str) -> RunOptions {
    RunOptions {
        only: Some(vec!["C121".into()]),
        app: app.into(),
        ..options(root)
    }
}

#[test]
fn c121_flags_the_shapes_it_names_and_only_as_warnings() {
    let tree = c121_tree();
    let r = run_checks(&c121_options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(String, usize)> = r.findings.iter().map(|f| (f.file.clone(), f.line)).collect();
    assert_eq!(
        got,
        [
            ("a/b_test.rs".to_string(), 1),
            ("docs/a.toml".to_string(), 2),
            ("docs/c.toml".to_string(), 1),
            ("scripts/bad.test.mjs".to_string(), 1),
            ("scripts/bad.test.mjs".to_string(), 2),
            ("tools/t/src/tests.rs".to_string(), 1),
            ("tools/t/tests/it/a.rs".to_string(), 2),
        ]
    );
    assert_eq!(r.errors(), 0, "report-only until its first green run (ER-41)");
    assert_eq!(r.warnings(), 7);
}

#[test]
fn c121_without_its_allow_list_cannot_look() {
    let tree = c121_tree();
    std::fs::remove_file(tree.path().join("conformance/c121-allow.json")).unwrap();
    let r = run_checks(&c121_options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(
        r.errors() > 0,
        "a check that could not read its allow list is an error no waiver covers"
    );
}

#[test]
fn live_c121_findings_equal_the_js_port_on_a_fixture_tree_and_on_this_tree() {
    if !has_node() {
        return skip("node is not installed");
    }
    let tree = c121_tree();
    for (root, app) in [(tree.path().to_path_buf(), "fixture"), (repo_root(), "paddock")] {
        let js: Value = serde_json::from_str(&node(
            JS_RUN_C121,
            &[("RT5_ROOT", root.display().to_string()), ("RT5_APP", app.into())],
        ))
        .unwrap();
        let r = run_checks(&c121_options(&root, app), &registry()).unwrap();
        let (got, want) = (shared_part(&r.json()), shared_part(&js));
        assert_eq!(got["findings"], want["findings"], "C121 on {app}");
        assert_eq!(got, want, "C121 on {app}");
    }
}
