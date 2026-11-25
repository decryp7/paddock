//! The conformance engine of the Rust host (WP-10 batch 1) against the answers the JavaScript host gave (recorded in
//! `tests/fixtures/check/recorded`, `engine-*`, before the checks it ran were retired, pair P3): the same command line gives the same
//! stdout, the same `--out` and `--summary` files and the same exit code, over fixture trees, including the inline waivers of every
//! ported check; this working tree's findings are held to the recorded ones as a ceiling.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_dev::check::{self, Check, CheckOutput, Control, Ctx, RunOptions, record::Precondition, run_checks};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    crate::full_tree(&paddock_env::winpath::real(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap())
}

/// What a host printed and left behind.
#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    out: Option<String>,
    summary: Option<String>,
}

fn run_host(args: &[&str], scratch: &Path) -> Ran {
    let (out, summary) = (scratch.join("out.json"), scratch.join("summary.md"));
    let mut c = crate::dev_bin::command();
    c.arg("conformance");
    c.args(args)
        .arg("--out")
        .arg(&out)
        .arg("--summary")
        .arg(&summary)
        .current_dir(repo())
        .env_remove("PADDOCK_BASE_REF")
        .env_remove("GITHUB_STEP_SUMMARY");
    let o = c.output().expect("the host must run: the oracle cannot be skipped");
    let read = |p: &Path| std::fs::read_to_string(p).ok();
    let ran = Ran {
        code: o.status.code(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        out: read(&out),
        summary: read(&summary),
    };
    let _ = std::fs::remove_file(out);
    let _ = std::fs::remove_file(summary);
    ran
}

/// A run as recorded data, with the scratch folder and the tree's root named `<scratch>` and `<root>` (no machine in a recording, ER-46).
/// The files the run was handed are `<scratch>/<name>` on every OS: the host prints them as the OS writes them (`\` on Windows), and the
/// recordings were made on macOS (CI run 8346, windows-latest).
fn ran_json(r: &Ran, scratch: &Path, root: &str) -> Value {
    let scrub = |t: &str| {
        let mut t = t.to_string();
        for name in ["out.json", "summary.md"] {
            t = t.replace(scratch.join(name).to_str().unwrap(), &format!("<scratch>/{name}"));
        }
        let t = t.replace(scratch.to_str().unwrap(), "<scratch>");
        if root.is_empty() { t } else { t.replace(root, "<root>") }
    };
    json!({
        "code": r.code,
        "stdout": scrub(&r.stdout),
        "stderr": scrub(&r.stderr),
        "out": r.out.as_deref().map(scrub),
        "summary": r.summary.as_deref().map(scrub),
    })
}

/// The Rust host over `args`, held to the answer the JS host recorded for `case` (stdout, `--out`, `--summary`, exit status and
/// standard error, byte for byte); returns the Rust answer.
fn same(case: &str, args: &[&str]) -> Ran {
    let b = tempfile::tempdir().unwrap();
    let rust = run_host(args, b.path());
    let root = args
        .iter()
        .position(|a| *a == "--root")
        .and_then(|i| args.get(i + 1))
        .copied()
        .unwrap_or("");
    let got = ran_json(&rust, b.path(), root);
    let want = crate::recorded::expect(case, &got);
    for k in ["code", "stdout", "out", "summary", "stderr"] {
        assert_eq!(got[k], want[k], "{k} of {args:?} ({case})");
    }
    rust
}

const PORTED: &str = "C39,C53,C120";

fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (p, t) in files {
        let f = d.path().join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, t).unwrap();
    }
    d
}

#[test]
fn tree_this_working_tree_gives_a_sound_report() {
    let root = repo();
    // The tree changes under the tests: only what holds whatever it holds is asserted (`recorded::assert_live_tree_sound`); the report
    // files and exit code are compared with the JS host's on fixture trees above.
    let scratch = tempfile::tempdir().unwrap();
    let args = ["--app", "paddock", "--root", root.to_str().unwrap(), "--only", PORTED];
    let r = run_host(&args, scratch.path());
    // The CLI's report has the JS host's shape (no run records): soundness is asked of the same checks run in process.
    let opts = RunOptions {
        root: root.clone(),
        config: check::config::Config::bare("paddock"),
        scope: None,
        only: Some(PORTED.split(',').map(String::from).collect()),
        files: None,
        today: check::today_utc(),
        base_ref: None,
        ci: Some(false),
    };
    crate::recorded::assert_live_tree_sound("engine", &run_checks(&opts, &check::registry()).unwrap().json());
    assert!(r.stdout.contains("paddock: 3 checks"), "{}", r.stdout);
    let out: Value = serde_json::from_str(r.out.as_deref().unwrap()).unwrap();
    assert_eq!(out["checks"], json!(["C39", "C53", "C120"]));
    assert!(
        out["waived"].as_array().unwrap().iter().any(|w| w["waiver"]["expires"] == "inline"),
        "the inline waivers of the tree are listed under waived"
    );
}

#[test]
fn json_mode_strict_and_the_exit_statuses_agree() {
    let root = repo();
    let root = root.to_str().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let json_run = run_host(&["--app", "paddock", "--root", root, "--only", "C120", "--json"], scratch.path());
    assert!(json_run.stdout.trim_start().starts_with('{'));
    // C123 blocks since its first green run (ci.yml 9065, ER-41): its finding is an error, so the run fails with or without
    // `--strict`. No Rust-host check is report-only any more; how `--strict` turns a report-only check's warnings into a failure is
    // shown by `check::outcome`'s own tests (`strict_makes_the_named_checks_warnings_fail`).
    // The binary's name is joined here, as in C123's own samples, so the dev_bin audit does not take this sample for a spawn.
    let sample = format!(
        "#[test]\nfn t() {{\n    let _ = std::process::Command::new(env!(\"{}\")).output();\n}}\n",
        ["CARGO_BIN_EXE", "paddock-dev"].join("_")
    );
    let bad = tree(&[("crates/x/tests/bad.rs", &sample)]);
    let loose = same(
        "engine-loose",
        &["--app", "t", "--root", bad.path().to_str().unwrap(), "--only", "C123"],
    );
    let strict = same(
        "engine-strict",
        &[
            "--app",
            "t",
            "--root",
            bad.path().to_str().unwrap(),
            "--only",
            "C123",
            "--strict",
            "C123",
        ],
    );
    assert_eq!(loose.code, Some(check::outcome::FINDINGS), "{}", loose.stdout);
    assert_eq!(strict.code, Some(check::outcome::FINDINGS), "{}", strict.stdout);
    // C120 blocks since its first green run (ci.yml 8869, ER-41): its site fails the run without `--strict`.
    // conformance: stand-in — the shell file is the data of the C120 layout check, never run; conformance: layout-guess — a shell fixture for the blocking test, not a layout this code computes
    let guess = tree(&[("scripts/x.sh", "#!/bin/sh\nd=$(dirname \"$0\")/../deps\ncd \"$d\"\n")]);
    let blocking = same(
        "engine-c120-blocks",
        &["--app", "t", "--root", guess.path().to_str().unwrap(), "--only", "C120"],
    );
    assert_eq!(blocking.code, Some(check::outcome::FINDINGS), "{}", blocking.stdout);
}

#[test]
fn usage_errors_and_a_missing_app_agree() {
    let empty = tempfile::tempdir().unwrap();
    let root = empty.path().to_str().unwrap();
    for (n, args) in [
        vec!["--root", root],
        vec!["--app", "t", "--bogus"],
        vec!["--app", "t", "stray"],
        vec!["--app"],
    ]
    .into_iter()
    .enumerate()
    {
        let r = same(&format!("engine-usage-{n}"), &args);
        assert_eq!(r.code, Some(check::outcome::USAGE), "{args:?}: {}", r.stderr);
    }
}

#[test]
fn a_report_file_that_cannot_be_written_is_an_io_error_with_the_cause_and_the_next_step() {
    let d = tree(&[("crates/a/src/x.rs", "fn a() {}\n")]);
    let missing = d.path().join("no-such-folder").join("out.json");
    for flag in ["--out", "--summary"] {
        let o = crate::dev_bin::command()
            .args(["conformance", "--app", "t", "--root"])
            .arg(d.path())
            .args(["--only", "C39", flag])
            .arg(&missing)
            .output()
            .unwrap();
        let (err, out) = (String::from_utf8_lossy(&o.stderr), String::from_utf8_lossy(&o.stdout));
        assert_eq!(o.status.code(), Some(check::outcome::IO_ERROR), "{flag}: {err}");
        assert!(
            err.contains("::error::") && err.contains("out.json") && err.contains("Fix:"),
            "the cause and the next step: {err}"
        );
        assert!(out.contains("t: 1 checks"), "the report is still printed: {out}");
    }
}

/// A waiver marker inside a string literal is data, not a waiver: the same marker in a comment is one. Both hosts agree.
#[test]
fn a_marker_in_a_string_literal_is_no_waiver_in_either_host() {
    let d = tree(&[
        (
            "crates/a/tests/s.rs",
            "fn a() {\n    let m = \"// conformance: freed-port — handed to a child that retries\";\n    let p = TcpListener::bind(\"127.0.0.1:0\").unwrap().local_addr().unwrap().port();\n}\nfn b() {\n    let p = TcpListener::bind(\"127.0.0.1:0\").unwrap().local_addr().unwrap().port(); // conformance: freed-port — handed to a child\n}\n",
        ),
        (
            "crates/a/src/k.rs",
            "fn a(h: HANDLE) {\n    let m = r#\"// conformance: pid-kill — a handle this process owns\"#;\n    unsafe { TerminateProcess(h, 1) };\n}\n",
        ),
        (
            "crates/a/src/l.rs",
            "// conformance: file-layout-guess — \"filler data\"\n#[cfg(test)]\nmod tests {\n    const M: &str = \"// conformance: layout-guess — pinned layout\";\n    fn a() { let q = base.join(\"deps\"); }\n}\n",
        ),
    ]);
    let r = same(
        "engine-string-markers",
        &["--app", "paddock", "--root", d.path().to_str().unwrap(), "--only", PORTED, "--json"],
    );
    let v: Value = serde_json::from_str(&r.stdout).unwrap();
    let waived: Vec<String> = v["waived"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| format!("{}:{}", w["file"].as_str().unwrap(), w["line"]))
        .collect();
    assert_eq!(
        waived,
        ["crates/a/tests/s.rs:6", "crates/a/src/l.rs:5"],
        "only the real comments waive; the file marker is a real comment"
    );
    let sites: Vec<String> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| format!("{}:{}", f["file"].as_str().unwrap(), f["line"]))
        .collect();
    for want in ["crates/a/tests/s.rs:3", "crates/a/src/k.rs:3"] {
        assert!(sites.iter().any(|s| s == want), "{want} stays a finding: {sites:?}");
    }
}

/// Every kind of inline waiver of the three ported checks, the bare ones, and a dated waiver beside them.
fn waiver_tree() -> tempfile::TempDir {
    tree(&[
        (
            "crates/a/tests/p.rs",
            "fn a() {\n    // conformance: freed-port — handed to a child that retries\n    let p = TcpListener::bind(\"127.0.0.1:0\").unwrap().local_addr().unwrap().port();\n}\nfn b() {\n    let q = TcpListener::bind(\"127.0.0.1:0\").unwrap().local_addr().unwrap().port(); // conformance: freed-port\n}\nfn c() {\n    let r = TcpListener::bind(\"127.0.0.1:0\").unwrap().local_addr().unwrap().port();\n}\n",
        ),
        (
            "crates/a/src/k.rs",
            "fn a(h: HANDLE) {\n    // conformance: pid-kill — a handle this process owns\n    unsafe { TerminateProcess(h, 1) };\n}\nfn b(h: HANDLE) {\n    unsafe { TerminateProcess(h, 1) }; // conformance: pid-kill\n}\nfn c(p: Pid, s: &System) {\n    // conformance: pid-liveness — the pid came from our own child handle\n    let _ = s.process(Pid::from(p));\n}\n",
        ),
        (
            "crates/a/src/l.rs",
            "// conformance: file-layout-guess — filler data only\nfn p() { let z = base.join(\"deps\"); }\n#[cfg(test)]\nmod tests {\n    fn a() { let q = base.join(\"deps\"); }\n}\nfn b() {\n    // conformance: layout-guess — pinned layout\n    let q = base.join(\"deps\");\n}\n",
        ),
        (
            "crates/a/src/l2.rs",
            "fn b() {\n    // conformance: layout-guess\n    let q = base.join(\"deps\");\n}\n",
        ),
    ])
}

#[test]
fn inline_waivers_are_listed_under_waived_identically_by_both_hosts() {
    let d = waiver_tree();
    let r = same(
        "engine-inline-waivers",
        &["--app", "paddock", "--root", d.path().to_str().unwrap(), "--only", PORTED, "--json"],
    );
    let v: Value = serde_json::from_str(&r.stdout).unwrap();
    let waived = v["waived"].as_array().unwrap();
    let listed: Vec<(String, String, u64)> = waived
        .iter()
        .map(|w| {
            assert_eq!(w["waiver"]["expires"], "inline");
            (
                w["check"].as_str().unwrap().into(),
                w["file"].as_str().unwrap().into(),
                w["line"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        listed.len(),
        5,
        "freed-port, pid-kill, pid-liveness and both layout-guess forms: {listed:?}"
    );
    for (check, reason) in [
        ("C39", "handed to a child that retries"),
        ("C53", "a handle this process owns"),
        ("C53", "the pid came from our own child handle"),
        ("C120", "filler data only"),
        ("C120", "pinned layout"),
    ] {
        assert!(
            waived.iter().any(|w| w["check"] == check && w["waiver"]["reason"] == reason),
            "{check}: {reason}"
        );
    }
    let errors: Vec<&str> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["severity"] == "error")
        .map(|f| f["message"].as_str().unwrap())
        .collect();
    assert!(
        errors.iter().any(|m| m.contains("the freed-port waiver gives no reason")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|m| m.contains("the pid-kill waiver gives no reason")),
        "{errors:?}"
    );
    assert_eq!(
        r.code,
        Some(check::outcome::FINDINGS),
        "bare waivers and an unwaived site fail the run"
    );
}

// ---- the engine through the library, with a check that is none of the ported ones ----

/// A check that owns no waiver code: it reports every `TODO` line and lets the engine's helper judge the comment above it.
struct Todo;

impl Check for Todo {
    fn id(&self) -> &'static str {
        "CX1"
    }
    fn rule(&self) -> u32 {
        1
    }
    fn title(&self) -> &'static str {
        "no TODO"
    }
    fn control(&self) -> Control {
        Control {
            files: vec![("a.txt", "TODO a\n// conformance: todo — tracked in the backlog\nTODO b\nTODO c\n")],
            expect: vec![("a.txt", 1), ("a.txt", 4)],
        }
    }
    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut findings = Vec::new();
        let mut probes = 0;
        for f in ctx.files().iter().filter(|f| f.ends_with(".txt")) {
            let text = ctx.read(f).unwrap();
            probes += 1;
            let lines: Vec<&str> = text.split('\n').collect();
            for (i, l) in lines.iter().enumerate().filter(|(_, l)| l.starts_with("TODO")) {
                let near = format!("{}\n{}", l, if i > 0 { lines[i - 1] } else { "" });
                let site = check::finding::Finding::error("CX1", 1, f, i + 1, "a TODO", "Do it.");
                findings.push(check::inline_waiver::recognise(&near, "todo").apply(site, || {
                    check::finding::Finding::error("CX1", 1, f, i + 1, "the todo waiver gives no reason", "Say why.")
                }));
            }
        }
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: true,
                detail: "x".into(),
            },
        }
    }
}

#[test]
fn a_new_check_gets_inline_waivers_from_the_engine_without_any_waiver_code() {
    let d = tree(&[("a.txt", "TODO a\n// conformance: todo — tracked in the backlog\nTODO b\nTODO c\n")]);
    let opts = RunOptions {
        root: d.path().to_path_buf(),
        config: check::config::Config::bare("t"),
        scope: None,
        only: None,
        files: None,
        today: "2026-11-13".into(),
        base_ref: None,
        ci: Some(false),
    };
    let r = run_checks(&opts, &[Box::new(Todo)]).unwrap();
    let lines: Vec<usize> = r.findings.iter().map(|f| f.line).collect();
    assert_eq!(lines, [1, 4], "the waived TODO is not a finding");
    assert_eq!(r.waived.len(), 1);
    let w = &r.waived[0];
    assert_eq!(
        (w.expires.as_str(), w.reason.as_str(), w.finding.line),
        ("inline", "tracked in the backlog", 3)
    );
    assert!(check::report::format_text(&r).contains("waived CX1 a.txt:3  a TODO (until inline: tracked in the backlog)"));
}

/// The Rust check modules on disk (`src/check/c<n>.rs`; helpers such as `c53_liveness.rs` are not checks), as `c<n>`.
fn rust_check_modules() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/check");
    let mut out: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|f| f.strip_suffix(".rs").map(str::to_string))
        .filter(|m| {
            m.strip_prefix('c')
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    out.sort();
    out
}

/// The `register_checks!` lines for these ids, as a mismatch prints them.
fn macro_lines(ids: &[String]) -> String {
    ids.iter()
        .map(|id| format!("    {}::{id},", id.to_lowercase()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// What `register_checks!` must hold: every `src/check/c<n>.rs` module, in id order (the order of `conformance/src/checks/index.mjs`
/// while the JavaScript host had the checks).
fn expected_registry(modules: &[String]) -> Vec<String> {
    let mut mine: Vec<(u32, String)> = modules.iter().map(|m| (m[1..].parse().unwrap(), m.to_uppercase())).collect();
    mine.sort();
    mine.into_iter().map(|(_, id)| id).collect()
}

/// The ids `conformance.rust` of `conformance/paddock.ci.json` names, through the one accessor of that contract.
fn contract_rust_ids() -> Vec<String> {
    let args = paddock_dev::ci_run::contract::conformance_rust_args_at(&repo()).unwrap();
    let only = args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).unwrap();
    only.split(',').map(str::to_string).collect()
}

/// The checks the JavaScript host still lists (`list --json`).
fn js_list() -> Vec<Value> {
    let js = paddock_testkit::TestCommand::new("node")
        .args(["conformance/bin/paddock-conformance.mjs", "list", "--json"])
        .current_dir(repo())
        .output()
        .expect("node must be installed: the JavaScript host's list cannot be skipped");
    assert!(js.status.success(), "{}", String::from_utf8_lossy(&js.stderr));
    serde_json::from_slice(&js.stdout).unwrap()
}

#[test]
fn the_registry_is_one_list_in_id_order_and_names_every_check_module() {
    let reg = check::registry();
    let ids: Vec<String> = reg.iter().map(|c| c.id().to_string()).collect();
    assert_eq!(check::sources().iter().map(|(id, _)| id.to_string()).collect::<Vec<_>>(), ids);
    // The macro stays hand-written (Rust has no glob of modules): it is checked against the module files, and a mismatch prints the
    // whole block to paste (ER-42).
    let want = expected_registry(&rust_check_modules());
    assert!(
        ids == want,
        "register_checks! in tools/paddock-dev/src/check/mod.rs must name every src/check/c<n>.rs in id order; replace its lines with\n{}",
        macro_lines(&want)
    );
    for entry in check::conformance::list_checks(&reg).as_array().unwrap() {
        assert!(entry["file"].as_str().unwrap().ends_with(".rs"));
    }
}

/// Pair P3: a check that the JavaScript host no longer lists runs on the Rust host through `conformance.rust` of the contract, and that
/// list holds nothing else; a check both hosts have (C139 until it is retired) stays in `conformance.only` and agrees on its metadata.
#[test]
fn the_contracts_rust_list_is_the_registry_less_what_the_javascript_host_still_lists() {
    let js = js_list();
    let js_ids: Vec<&str> = js.iter().filter_map(|j| j["id"].as_str()).collect();
    let rust_only: Vec<String> = check::registry()
        .iter()
        .map(|c| c.id().to_string())
        .filter(|id| !js_ids.contains(&id.as_str()))
        .collect();
    let listed = contract_rust_ids();
    assert!(
        rust_only == listed,
        "`conformance.rust` of conformance/paddock.ci.json must be the checks only the Rust host has, in id order; set it to\n{}",
        serde_json::to_string(&rust_only).unwrap()
    );
    for entry in check::conformance::list_checks(&check::registry()).as_array().unwrap() {
        let Some(theirs) = js.iter().find(|j| j["id"] == entry["id"]) else {
            continue;
        };
        for k in ["title", "rule", "scope", "mode", "needs"] {
            assert_eq!(entry[k], theirs[k], "{} {k}", entry["id"]);
        }
    }
}

#[test]
fn a_new_rust_check_file_is_named_in_the_registry_message() {
    let modules: Vec<String> = ["c120", "c39", "c53", "c999", "c1000"].map(String::from).to_vec();
    let want = expected_registry(&modules);
    assert_eq!(want, ["C39", "C53", "C120", "C999", "C1000"]);
    assert_eq!(
        macro_lines(&want[3..]),
        "    c999::C999,\n    c1000::C1000,",
        "the message gives the exact lines"
    );
}

/// Every check the Rust host has and the JavaScript host does not has recorded JS answers to be held to: at least one file in
/// `tests/fixtures/check/recorded`, `units` or the `expected.json` of C39 and C53, named after it (a check with none was ported without
/// a parity proof). A check written in Rust from the start has no JavaScript answers to be held to: it is named in [`BORN_IN_RUST`]
/// (with its own fixtures in `tests/it/check_c<n>.rs`), and a check that is listed there and also has a JS module is an error.
const BORN_IN_RUST: [&str; 1] = ["C145"];

#[test]
fn every_rust_only_check_has_recorded_js_answers() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/check");
    let recorded: Vec<String> = std::fs::read_dir(dir.join("recorded"))
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    let js = js_list();
    for c in check::registry() {
        let id = c.id();
        if js.iter().any(|j| j["id"] == id) {
            assert!(
                !BORN_IN_RUST.contains(&id),
                "{id} has a JS module, so it was not born in Rust: take it off BORN_IN_RUST"
            );
            continue;
        }
        if BORN_IN_RUST.contains(&id) {
            assert!(
                dir.join("../../it").join(format!("check_{}.rs", id.to_lowercase())).is_file(),
                "{id} is born in Rust and has no tests/it/check_{}.rs",
                id.to_lowercase()
            );
            continue;
        }
        let low = id.to_lowercase();
        let ok = recorded
            .iter()
            .any(|f| f.starts_with(&format!("{low}-")) || f == &format!("corpus-{low}.json"))
            || dir.join("units").join(format!("{low}.json")).is_file()
            || matches!(id, "C39" | "C53") && dir.join("expected.json").is_file();
        assert!(
            ok,
            "{id} has no recorded JS answer: record its cases with `PADDOCK_GOLDEN_BLESS=1` (tests/it/recorded.rs)"
        );
    }
}
