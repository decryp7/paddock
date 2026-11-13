//! The conformance engine of the Rust host (WP-10 batch 1) against the JavaScript host, the oracle: the same command line gives the
//! same stdout, the same `--out` and `--summary` files and the same exit code, over this working tree and over fixture trees,
//! including the inline waivers of every ported check. Node is required: a missing oracle fails the test (ER-33).
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

fn run_host(js: bool, args: &[&str], scratch: &Path) -> Ran {
    let (out, summary) = (scratch.join("out.json"), scratch.join("summary.md"));
    let mut c = if js {
        let mut c = paddock_proc::command("node");
        c.arg("conformance/bin/paddock-conformance.mjs");
        c
    } else {
        let mut c = paddock_proc::command(env!("CARGO_BIN_EXE_paddock-dev"));
        c.arg("conformance");
        c
    };
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

/// Both hosts over the same arguments; asserts they agree byte for byte and returns the answer.
fn same(args: &[&str]) -> Ran {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (js, rust) = (run_host(true, args, a.path()), run_host(false, args, b.path()));
    if let (Ok(a), Ok(b)) = (
        serde_json::from_str::<Value>(&js.stdout),
        serde_json::from_str::<Value>(&rust.stdout),
    ) {
        for k in ["findings", "waived", "checks"] {
            assert_eq!(a[k], b[k], "{k} of {args:?}");
        }
    }
    assert_eq!(js.stdout, rust.stdout, "stdout of {args:?}");
    assert_eq!(js.out, rust.out, "--out of {args:?}");
    assert_eq!(js.summary, rust.summary, "--summary of {args:?}");
    assert_eq!(js.code, rust.code, "exit status of {args:?}");
    let scrub = |r: &Ran, d: &tempfile::TempDir| r.stderr.replace(d.path().to_str().unwrap(), "<scratch>");
    assert_eq!(scrub(&js, &a), scrub(&rust, &b), "stderr of {args:?}");
    rust
}

const PORTED: &str = "C39,C53,C119,C120";

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
fn this_working_tree_gives_the_same_report_files_and_exit_code_in_both_hosts() {
    let root = repo();
    let r = same(&["--app", "paddock", "--root", root.to_str().unwrap(), "--only", PORTED]);
    assert!(r.stdout.contains("paddock: 4 checks"), "{}", r.stdout);
    let out: Value = serde_json::from_str(r.out.as_deref().unwrap()).unwrap();
    assert_eq!(out["checks"], json!(["C39", "C53", "C119", "C120"]));
    assert!(
        out["waived"].as_array().unwrap().iter().any(|w| w["waiver"]["expires"] == "inline"),
        "the inline waivers of the tree are listed under waived"
    );
}

#[test]
fn json_mode_strict_and_the_exit_statuses_agree() {
    let root = repo();
    let root = root.to_str().unwrap();
    let json_run = same(&["--app", "paddock", "--root", root, "--only", "C119,C120", "--json"]);
    assert!(json_run.stdout.trim_start().starts_with('{'));
    // C120 and C119 are report-only: a warning never fails the run, `--strict` makes the named check's warnings fail.
    let bad = tree(&[("scripts/x.sh", "#!/bin/sh\nd=$(dirname \"$0\")/../deps\ncd \"$d\"\n")]);
    let loose = same(&["--app", "t", "--root", bad.path().to_str().unwrap(), "--only", "C120"]);
    let strict = same(&[
        "--app",
        "t",
        "--root",
        bad.path().to_str().unwrap(),
        "--only",
        "C120",
        "--strict",
        "C120",
    ]);
    assert_eq!(loose.code, Some(check::outcome::CLEAN), "{}", loose.stdout);
    assert_eq!(strict.code, Some(check::outcome::FINDINGS), "{}", strict.stdout);
}

#[test]
fn usage_errors_and_a_missing_app_agree() {
    let empty = tempfile::tempdir().unwrap();
    let root = empty.path().to_str().unwrap();
    for args in [
        vec!["--root", root],
        vec!["--app", "t", "--bogus"],
        vec!["--app", "t", "stray"],
        vec!["--app"],
    ] {
        let r = same(&args);
        assert_eq!(r.code, Some(check::outcome::USAGE), "{args:?}: {}", r.stderr);
    }
}

/// Every kind of inline waiver of the four ported checks, the bare ones, and a dated waiver beside them.
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
            ".gitea/workflows/w.yml",
            "jobs:\n  a:\n    steps:\n      # conformance: dev-before-toolchain — the image has the only compiler\n      - run: ./dev x\n  b:\n    steps:\n      # conformance: dev-before-toolchain\n      - run: ./dev y\n  c:\n    steps:\n      - run: ./dev z\n",
        ),
        (
            "crates/a/src/l.rs",
            "// conformance: file-layout-guess — filler data only\nfn a() { let q = base.join(\"deps\"); }\nfn b() {\n    // conformance: layout-guess — pinned layout\n    let q = base.join(\"deps\");\n}\n",
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
    // C119 judges the Paddock repository only (the JavaScript check returns nothing for another app): the same app name in both.
    let r = same(&["--app", "paddock", "--root", d.path().to_str().unwrap(), "--only", PORTED, "--json"]);
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
        6,
        "freed-port, pid-kill, pid-liveness, dev-before-toolchain and both layout-guess forms: {listed:?}"
    );
    for (check, reason) in [
        ("C39", "handed to a child that retries"),
        ("C53", "a handle this process owns"),
        ("C53", "the pid came from our own child handle"),
        ("C119", "the image has the only compiler"),
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

#[test]
fn the_registry_is_one_list_and_list_json_agrees_with_the_javascript_host() {
    let reg = check::registry();
    let ids: Vec<&str> = reg.iter().map(|c| c.id()).collect();
    assert_eq!(ids, ["C39", "C53", "C119", "C120"]);
    assert_eq!(check::sources().iter().map(|(id, _)| *id).collect::<Vec<_>>(), ids);
    let rust = check::conformance::list_checks(&reg);
    let js = paddock_proc::command("node")
        .args(["conformance/bin/paddock-conformance.mjs", "list", "--json"])
        .current_dir(repo())
        .output()
        .expect("node must be installed: the oracle cannot be skipped");
    assert!(js.status.success(), "{}", String::from_utf8_lossy(&js.stderr));
    let js: Vec<Value> = serde_json::from_slice(&js.stdout).unwrap();
    for entry in rust.as_array().unwrap() {
        let theirs = js
            .iter()
            .find(|j| j["id"] == entry["id"])
            .expect("the JavaScript host has the check");
        for k in ["title", "rule", "scope", "mode", "needs"] {
            assert_eq!(entry[k], theirs[k], "{} {k}", entry["id"]);
        }
        assert!(entry["file"].as_str().unwrap().ends_with(".rs"));
    }
}
