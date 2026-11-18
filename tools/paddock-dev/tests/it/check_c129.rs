//! C129 (no new shell): the Rust host against the answers the JavaScript check gave (recorded in `tests/fixtures/check/recorded`; the JS module is retired, pair P3), on a fixture tree and on this working tree.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{LIVE_STALL, repo_root};
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{RunOptions, registry, run_checks, run_checks_in};
use serde_json::{Value, json};
use std::path::Path;

const BASELINE: &str = "conformance/baselines/C129-shell.json";
const EXCEPTIONS: &str = "conformance/shell-exceptions.json";

/// A new file of each kind, a baselined file that grew, one that shrank, an excepted path and pattern, a stale baseline entry and
/// exception, shebangs of an extensionless file (`env`, python, node, none), a binary, a caller naming a missing script (and one it
/// resolves from an app folder), a variable and a comment, and inline `run:` blocks.
fn cases() -> Vec<(&'static str, String)> {
    vec![
        (
            BASELINE,
            "{\"files\":[{\"path\":\"grew.sh\",\"lines\":2},{\"path\":\"shrank.sh\",\"lines\":9},{\"path\":\"gone.sh\",\"lines\":3},{\"path\":\"apps/d/scripts/in-app.sh\",\"lines\":1}]}".into(),
        ),
        (
            EXCEPTIONS,
            "{\"exceptions\":[{\"path\":\"dev\",\"id\":\"E1\",\"reason\":\"it must start first\",\"owner\":\"plan\"},{\"path\":\"vm/a.ps1\",\"id\":\"E5\",\"reason\":\"runs in the guest\",\"owner\":\"plan\"},{\"path\":\"gone/x.sh\",\"id\":\"E6\",\"reason\":\"names no file\",\"owner\":\"plan\"}]}".into(),
        ),
        ("dev", "#!/bin/sh\necho\n".into()),
        ("vm/a.ps1", "x\n".into()),
        ("vm/sub/b.ps1", "x\n".into()),
        ("new.sh", "a\nb\nc\n".into()),
        ("new.PY", "print(1)".into()),
        ("grew.sh", "1\n2\n3\n".into()),
        ("shrank.sh", "1\n".into()),
        ("bin/env-bash", "#!/usr/bin/env bash\necho\n".into()),
        ("bin/py", "#!/usr/bin/env -S python3 -I\n".into()),
        ("bin/node", "#!/usr/bin/env node\n".into()),
        ("bin/plain", "echo hi\n".into()),
        ("bin/blob", "#!/bin/sh\0\n".into()),
        ("é/ü.bat", "echo é\r\nexit\r\n".into()),
        ("apps/d/scripts/in-app.sh", "x\n".into()),
        ("scripts/keep.txt", "a top-level folder the callers' paths start in\n".into()),
        (
            ".gitea/workflows/x.yml",
            "jobs:\n  a:\n    steps:\n      - run: scripts/gone.sh\n      - run: bash ./new.sh\n      - run: scripts/in-app.sh\n      - run: scripts/$N.sh\n      # scripts/c.sh\n      - run: |\n          echo a\n\n          echo b\n      - name: n\n        run: >-\n          one\n".into(),
        ),
        ("apps/d/paddock.ci.json", "{\"prepare\": [\"scripts/in-app.sh\", \"scripts/nope.ps1\"]}".into()),
        ("lefthook.yml", "pre-push:\n  run: ./new.sh\n".into()),
        ("icons/a.png", "#!/bin/sh bytes".into()),
    ]
}

fn options(root: &Path, app: &str) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare(app),
        scope: None,
        only: Some(vec!["C129".into()]),
        files: None,
        today: "2026-11-15".into(),
        base_ref: Some("origin/main".into()),
        ci: Some(false),
    }
}

fn shared_part(v: &Value) -> Value {
    json!({"app": v["app"], "checks": v["checks"], "findings": v["findings"], "waived": v["waived"], "errors": v["errors"], "warnings": v["warnings"]})
}

fn tree_of<T: AsRef<str>>(files: &[(&str, T)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let to = d.path().join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, text.as_ref()).unwrap();
    }
    d
}

/// The Rust host over `root` and the answer recorded for `case`: the shared part of each report, the Rust one first.
fn both(case: &str, root: &Path) -> (Value, Value) {
    let r = run_checks(&options(root, "fixture"), &registry()).unwrap();
    let got = shared_part(&r.json());
    // The JS host named its own regeneration tool in the fixes; the tool is now `./dev check shell-baseline` (pair P3).
    let want = crate::recorded::expect(case, &got);
    let want = serde_json::from_str(
        &want
            .to_string()
            .replace("node conformance/bin/paddock-shell-baseline.mjs", "./dev check shell-baseline"),
    )
    .unwrap();
    (got, want)
}

#[test]
fn c129_flags_new_and_grown_shell_and_names_stale_entries_as_warnings() {
    let tree = tree_of(&cases());
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize, &str)> = r
        .findings
        .iter()
        .map(|f| (f.file.as_str(), f.line, f.message.split([':', '(']).next().unwrap().trim_end()))
        .collect();
    assert_eq!(
        got,
        [
            ("bin/env-bash", 1, "new shell file"),
            ("bin/py", 1, "new shell file"),
            ("grew.sh", 1, "shell file grew from 2 to 3 lines"),
            ("new.PY", 1, "new shell file"),
            ("new.sh", 1, "new shell file"),
            (BASELINE, 1, "shrank.sh is under its baseline count"),
            ("vm/sub/b.ps1", 1, "new shell file"),
            ("é/ü.bat", 1, "new shell file"),
            (BASELINE, 1, "stale baseline entry"),
            (EXCEPTIONS, 1, "stale exception"),
            (BASELINE, 1, "the baseline's shrink-only rule is not enforced here"),
            (EXCEPTIONS, 1, "the exception list's shrink-only rule is not enforced here"),
            (
                ".gitea/workflows/x.yml",
                4,
                "names the script `scripts/gone.sh`, which the tree does not have"
            ),
            (
                "apps/d/paddock.ci.json",
                1,
                "names the script `scripts/nope.ps1`, which the tree does not have"
            ),
            (".gitea", 1, "inline shell in .gitea"),
        ]
    );
    assert_eq!(r.errors(), 0, "report-only until its first green run (ER-41)");
}

/// "Delete the check": a baseline or exception list that is missing, not JSON or incomplete makes the check broken, one finding and
/// nothing else, on both hosts, never a pass.
#[test]
fn c129_with_a_broken_list_is_one_check_broken_finding_on_both_hosts() {
    let good_b = "{\"files\":[]}";
    let good_e = "{\"exceptions\":[]}";
    for (i, (which, text)) in [
        (BASELINE, None),
        (BASELINE, Some("nope")),
        (BASELINE, Some("{}")),
        (BASELINE, Some("{\"files\":[{\"path\":\"a.sh\"}]}")),
        (EXCEPTIONS, None),
        (EXCEPTIONS, Some("nope")),
        (
            EXCEPTIONS,
            Some("{\"exceptions\":[{\"path\":\"scripts/\",\"id\":\"E5\",\"reason\":\"long enough\",\"owner\":\"x\"}]}"),
        ),
        (
            EXCEPTIONS,
            Some("{\"exceptions\":[{\"path\":\"a\",\"id\":\"E9\",\"reason\":\"long enough\",\"owner\":\"x\"}]}"),
        ),
        (
            EXCEPTIONS,
            Some("{\"exceptions\":[{\"path\":\"a\",\"id\":\"E1\",\"reason\":\"short\",\"owner\":\"x\"}]}"),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut files = vec![("a.sh", "x\n".to_string())];
        for (path, good) in [(BASELINE, good_b), (EXCEPTIONS, good_e)] {
            if path != which {
                files.push((path, good.to_string()));
            } else if let Some(t) = text {
                files.push((path, t.to_string()));
            }
        }
        let tree = tree_of(&files);
        let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
        let got: Vec<(&str, Option<&str>)> = r.findings.iter().map(|f| (f.file.as_str(), f.outcome.map(|o| o.name()))).collect();
        assert_eq!(got, [(which, Some("check-broken"))], "{which} {text:?}");
        // The JSON parsers word their own error differently (serde_json and V8): the message is compared up to that text.
        let cut = |mut v: Value| {
            for f in v["findings"].as_array_mut().unwrap() {
                let m = f["message"].as_str().unwrap().to_string();
                f["message"] = Value::from(m.split(" is not JSON: ").next().unwrap());
            }
            v
        };
        let (rust, js) = both(&format!("c129-broken-list-{i}"), tree.path());
        assert_eq!(cut(rust), cut(js), "{which} {text:?}");
    }
}

/// A file the run cannot read is not measured, at that file, on both hosts (root reads a 000 file: then nothing is checked).
#[test]
#[cfg(unix)]
fn c129_an_unreadable_file_is_not_measured_on_both_hosts() {
    use std::os::unix::fs::PermissionsExt;
    let tree = tree_of(&[
        (
            BASELINE,
            "{\"files\":[{\"path\":\"scripts/a.sh\",\"lines\":1},{\"path\":\"scripts/b.sh\",\"lines\":1}]}".to_string(),
        ),
        (EXCEPTIONS, "{\"exceptions\":[]}".to_string()),
        ("scripts/a.sh", "x\n".to_string()),
        ("scripts/b.sh", "y\n".to_string()),
    ]);
    let locked = tree.path().join("scripts/a.sh");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&locked).is_ok() {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
        eprintln!("not measured: running as root, which reads a 000 file");
        return;
    }
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let parity = Some(both("c129-unreadable", tree.path()));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    let got: Vec<(&str, Option<&str>)> = r
        .findings
        .iter()
        .filter(|f| !f.message.contains("shrink-only rule is not enforced"))
        .map(|f| (f.file.as_str(), f.outcome.map(|o| o.name())))
        .collect();
    assert_eq!(got, [("scripts/a.sh", Some("not-measured"))]);
    if let Some((rust, js)) = parity {
        assert_eq!(rust, js);
    }
}

/// A run over other files (`--files`, the push gate) says nothing about baseline entries and exceptions whose files it did not look at.
#[test]
fn c129_a_files_run_judges_only_the_files_it_looked_at() {
    let tree = tree_of(&[
        (BASELINE, "{\"files\":[{\"path\":\"old.sh\",\"lines\":1}]}".to_string()),
        (
            EXCEPTIONS,
            "{\"exceptions\":[{\"path\":\"fx/a.sh\",\"id\":\"E6\",\"reason\":\"fixtures\",\"owner\":\"plan\"}]}".to_string(),
        ),
        ("old.sh", "x\n".to_string()),
        ("fx/a.sh", "x\n".to_string()),
        ("new.sh", "x\n".to_string()),
        ("a.txt", "x\n".to_string()),
    ]);
    let o = RunOptions {
        files: Some(vec!["new.sh".into(), "a.txt".into()]),
        ..options(tree.path(), "fixture")
    };
    let r = run_checks(&o, &registry()).unwrap();
    let got: Vec<(&str, &str)> = r
        .findings
        .iter()
        .filter(|f| !f.message.contains("shrink-only rule is not enforced"))
        .map(|f| (f.file.as_str(), f.message.split(" (").next().unwrap()))
        .collect();
    assert_eq!(got, [("new.sh", "new shell file")]);
}

#[test]
fn live_c129_fixture_findings_equal_the_recorded_js_answer() {
    let tree = tree_of(&cases());
    let (got, want) = both("c129-fixture", tree.path());
    assert_eq!(got["findings"], want["findings"]);
    assert_eq!(got, want);
}

#[test]
fn live_c129_tree_findings_stay_within_the_recorded_js_answer() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C129 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    crate::recorded::assert_tree_within("c129-real-tree", &r.json());
}

// Plain `git` in a scratch repository, started and awaited here; nothing is killed by pid.
#[allow(clippy::disallowed_methods)]
fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// The exceptions only shrink: one the merge base's list lacks (a widening, or a changed id) is a finding that names it; a removed
/// one is not.
#[test]
fn c129_an_added_or_widened_exception_is_a_finding_against_the_merge_base() {
    let list = |rows: &[(&str, &str)]| {
        let rows: Vec<String> = rows
            .iter()
            .map(|(p, i)| format!("{{\"path\":\"{p}\",\"id\":\"{i}\",\"reason\":\"it must start first\",\"owner\":\"plan\"}}"))
            .collect();
        format!("{{\"exceptions\":[{}]}}", rows.join(","))
    };
    let tree = tree_of(&[
        (BASELINE, "{\"files\":[]}".to_string()),
        (EXCEPTIONS, list(&[("dev", "E1"), ("scripts/b.sh", "E5")])),
        ("dev", "#!/bin/sh\n".to_string()),
        ("scripts/a.sh", "x\n".to_string()),
        ("scripts/b.sh", "x\n".to_string()),
    ]);
    let d = tree.path();
    git(d, &["init", "-q", "-b", "main"]);
    git(d, &["add", "-A"]);
    git(d, &["commit", "-q", "-m", "base"]);
    git(d, &["checkout", "-q", "-b", "work"]);
    std::fs::write(d.join(EXCEPTIONS), list(&[("scripts/a.sh", "E5"), ("dev", "E3")])).unwrap();
    git(d, &["commit", "-q", "-am", "widen"]);
    let o = RunOptions {
        base_ref: Some("main".into()),
        ..options(d, "fixture")
    };
    let r = run_checks(&o, &registry()).unwrap();
    let grew: Vec<&str> = r
        .findings
        .iter()
        .map(|f| f.message.as_str())
        .filter(|m| m.contains("exceptions grew"))
        .collect();
    assert_eq!(grew.len(), 1, "{:?}", r.findings);
    assert!(
        grew[0].contains("2 entries are (scripts/a.sh E5; dev E3)") && grew[0].contains("needs the user's approval"),
        "{grew:?}"
    );
}

/// A control is hermetic (ER-21 class: a check's own control read the real repository): an in-memory tree has no git history, so the
/// control must hold whatever base ref the process has and whatever repository it is in. Here the process stands in a repository
/// whose lists differ from the control's; reaching for git would report the control's entries as added and fail the control.
#[test]
fn a_control_holds_in_a_repository_whose_lists_differ_from_its_own() {
    let repo = tree_of(&[
        (BASELINE, "{\"files\":[{\"path\":\"other.sh\",\"lines\":1}]}".to_string()),
        (
            EXCEPTIONS,
            "{\"exceptions\":[{\"path\":\"other\",\"id\":\"E1\",\"reason\":\"it must start first\",\"owner\":\"plan\"}]}".to_string(),
        ),
        ("other.sh", "x\n".to_string()),
    ]);
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &["commit", "-q", "-m", "base"]);
    git(repo.path(), &["checkout", "-q", "-b", "work"]);
    std::fs::write(repo.path().join("other.sh"), "x\ny\n").unwrap();
    git(repo.path(), &["commit", "-q", "-am", "edit"]);
    // The process's working folder is changed and not restored: nextest runs every test in its own process, so no other test sees it.
    std::env::set_current_dir(repo.path()).unwrap();
    // the ref a control would compare with by default (origin/main) exists here, with lists that differ from the control's
    git(repo.path(), &["update-ref", "refs/remotes/origin/main", "main"]);
    let elsewhere = tree_of(&[
        (BASELINE, "{\"files\":[]}".to_string()),
        (EXCEPTIONS, "{\"exceptions\":[]}".to_string()),
        ("a.txt", "x\n".to_string()),
    ]);
    for id in ["C129", "C53"] {
        let o = RunOptions {
            only: Some(vec![id.into()]),
            base_ref: None,
            ..options(elsewhere.path(), "fixture")
        };
        let r = run_checks(&o, &registry()).unwrap();
        assert_eq!(
            r.json()["runs"][0]["control"],
            json!(true),
            "{id}'s control read the repository the process stands in"
        );
    }
}

/// The regeneration tool (`./dev check shell-baseline`, the port of the retired `paddock-shell-baseline.mjs`): no flag lists and writes
/// nothing, `--shrink` removes stale entries and lowers counts and never adds or raises, `--init` writes from every file.
#[test]
#[allow(clippy::disallowed_methods, reason = "a test starts the built binary; it ends it itself")]
fn the_shell_baseline_tool_lists_shrinks_and_inits() {
    let tree = tree_of(&[
        (
            BASELINE,
            "{\"files\":[{\"path\":\"a.sh\",\"lines\":4},{\"path\":\"gone.sh\",\"lines\":9},{\"path\":\"b.sh\",\"lines\":2}]}".to_string(),
        ),
        (EXCEPTIONS, "{\"exceptions\":[]}".to_string()),
        ("a.sh", "x\n".repeat(2)),
        ("b.sh", "x\n".repeat(5)),
        ("new.sh", "x\n".to_string()),
    ]);
    let run = |flag: Option<&str>| {
        let mut c = crate::dev_bin::command();
        c.args(["check", "shell-baseline", "--root"]).arg(tree.path());
        if let Some(f) = flag {
            c.arg(f);
        }
        let o = c.output().unwrap();
        assert!(
            o.status.success(),
            "{flag:?}: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let before = std::fs::read_to_string(tree.path().join(BASELINE)).unwrap();
    let listed = run(None);
    assert!(
        listed.contains("2\ta.sh") && listed.contains("1\tnew.sh") && listed.contains("3 shell files, 8 lines"),
        "{listed}"
    );
    assert_eq!(
        std::fs::read_to_string(tree.path().join(BASELINE)).unwrap(),
        before,
        "listing writes nothing"
    );
    let shrunk = run(Some("--shrink"));
    assert!(
        shrunk.contains("removed: gone.sh") && shrunk.contains("1 removed; 1 shell files are not in the baseline and were not added"),
        "{shrunk}"
    );
    let after: Value = serde_json::from_str(&std::fs::read_to_string(tree.path().join(BASELINE)).unwrap()).unwrap();
    assert_eq!(
        after["files"],
        json!([{"path": "a.sh", "lines": 2}, {"path": "b.sh", "lines": 2}]),
        "a.sh is lowered from 4 to 2, b.sh is not raised from 2 to 5, new.sh is not added, gone.sh is removed"
    );
    run(Some("--init"));
    let init: Value = serde_json::from_str(&std::fs::read_to_string(tree.path().join(BASELINE)).unwrap()).unwrap();
    assert_eq!(init["files"].as_array().unwrap().len(), 3, "--init writes every shell file");
}
