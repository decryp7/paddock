//! The `paddock-bench` command end to end, on files in a temporary folder.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_paddock-bench"))
}

fn write(dir: &std::path::Path, name: &str, text: &str) -> String {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p.to_string_lossy().into_owned()
}

fn results(value: f64) -> String {
    format!(
        r#"{{"version":1,"app":"demo","app_version":"1","commit":null,"host":{{"os":"x","arch":"y","cpus":4}},
        "measurements":[{{"name":"q","kind":"query-latency","value":{value},"runs":3}}]}}"#
    )
}

const BUDGETS: &str = "[[budget]]\nname = \"q\"\nlimit = 100\n";

#[test]
fn check_exits_0_within_budget_and_5_over_it() {
    let d = tempfile::tempdir().unwrap();
    let b = write(d.path(), "budgets.toml", BUDGETS);
    let ok = write(d.path(), "ok.json", &results(90.0));
    let bad = write(d.path(), "bad.json", &results(500.0));
    let out = bin()
        .args(["check", "--budgets", &b, "--results", &ok])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = bin()
        .args(["check", "--budgets", &b, "--results", &bad, "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(5));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        (v["ok"].as_bool(), v["withinBudget"].as_bool()),
        (Some(true), Some(false))
    );
    let out = bin()
        .args(["check", "--budgets", &b, "--results", &bad, "--markdown"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("| q |"));
}

#[test]
fn a_regression_against_the_previous_run_exits_5_and_a_damaged_previous_only_warns() {
    let d = tempfile::tempdir().unwrap();
    let b = write(
        d.path(),
        "budgets.toml",
        "[[budget]]\nname = \"q\"\nlimit = 1000\n",
    );
    let now = write(d.path(), "now.json", &results(300.0));
    let prev = write(d.path(), "prev.json", &results(100.0));
    let out = bin()
        .args([
            "check",
            "--budgets",
            &b,
            "--results",
            &now,
            "--previous",
            &prev,
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(5));
    let broken = write(d.path(), "broken.json", "{");
    let out = bin()
        .args([
            "check",
            "--budgets",
            &b,
            "--results",
            &now,
            "--previous",
            &broken,
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no regression check"));
}

#[test]
fn usage_errors_exit_2_and_unreadable_files_exit_1() {
    let d = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| bin().args(args).output().unwrap();
    assert_eq!(run(&[]).status.code(), Some(2));
    assert_eq!(run(&["frobnicate"]).status.code(), Some(2));
    assert_eq!(run(&["check"]).status.code(), Some(2));
    assert_eq!(run(&["check", "--nope"]).status.code(), Some(2));
    assert_eq!(run(&["check", "--budgets"]).status.code(), Some(2));
    assert_eq!(run(&["check", "extra", "more"]).status.code(), Some(2));
    assert_eq!(run(&["propose", "--margin", "500"]).status.code(), Some(2));
    let missing = d.path().join("none.json").to_string_lossy().into_owned();
    let b = write(d.path(), "budgets.toml", BUDGETS);
    assert_eq!(
        run(&["check", "--budgets", &b, "--results", &missing])
            .status
            .code(),
        Some(1)
    );
    let help = run(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));
}

#[test]
fn propose_writes_a_budgets_file_that_the_check_accepts() {
    let d = tempfile::tempdir().unwrap();
    let r = write(d.path(), "r.json", &results(80.0));
    let out = bin()
        .args(["propose", "--results", &r, "--margin", "50"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let b = write(
        d.path(),
        "budgets.toml",
        &String::from_utf8(out.stdout).unwrap(),
    );
    let out = bin()
        .args(["check", "--budgets", &b, "--results", &r])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
}

/// ADR 0089 amendment 4.1 and 4.2, end to end: a budget whose measurement the run did not record is exit 6, an empty
/// results file against an empty budget list (the check deleted) is exit 6, and a pass says what it is a pass of.
#[test]
fn a_missing_measurement_or_nothing_compared_is_exit_6_never_0() {
    let d = tempfile::tempdir().unwrap();
    let b = write(d.path(), "budgets.toml", BUDGETS);
    let empty_run = write(
        d.path(),
        "empty.json",
        r#"{"version":1,"app":"demo","app_version":"1","commit":null,"host":{"os":"x","arch":"y","cpus":4},"measurements":[]}"#,
    );
    let out = bin()
        .args(["check", "--budgets", &b, "--results", &empty_run])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(6),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let no_budgets = write(d.path(), "none.toml", "");
    let out = bin()
        .args(["check", "--budgets", &no_budgets, "--results", &empty_run])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(6));
    let ok = write(d.path(), "ok.json", &results(90.0));
    let out = bin()
        .args(["check", "--budgets", &b, "--results", &ok, "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["outcomes"][0]["status"], "clean");
    assert_eq!(v["outcomes"][0]["evidence"]["check"], "bench:demo");
    assert_eq!(v["outcomes"][0]["evidence"]["subject"]["kind"], "files");
}
