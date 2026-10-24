//! `run` and `test` start a real plugin process against the fake host: the test kit's fixture, and copies of it that misbehave.

use crate::support::cli;
use paddock_cli::Exit;
use paddock_plugin_test::fixture;
use std::path::Path;

/// A project folder with the fixture's manifest (its program is named `paddock-plugin-fixture`).
fn project(dir: &Path) -> &Path {
    let manifest = fixture::manifest_toml("2.0", "paddock-plugin-fixture", "");
    std::fs::write(dir.join("manifest.toml"), manifest).unwrap();
    dir
}

#[test]
fn run_describes_the_plugin_and_test_finds_nothing_in_a_conforming_one() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    let program = fixture::locate().unwrap();
    let program = program.to_str().unwrap();

    let r = cli(project, &["run", "--program", program]);
    assert_eq!(r.exit, Exit::Ok, "{}", r.err);
    assert!(
        r.out.contains("tools: fixture.tool") && r.out.contains("views: fixture.view"),
        "{}",
        r.out
    );
    assert!(r.err.contains("Starting"), "{}", r.err);
    let j = cli(project, &["run", "--program", program, "--json", "-q"]);
    assert_eq!(j.json()["described"]["id"], fixture::ID);

    let t = cli(project, &["test", "--program", program, "--json"]);
    assert_eq!(t.exit, Exit::Ok, "{}{}", t.out, t.err);
    let v = t.json();
    assert_eq!(
        (v["count"].as_u64(), v["findings"].as_array().map(Vec::len)),
        (Some(0), Some(0))
    );
    let text = cli(project, &["test", "--program", program, "-q"]);
    assert!(text.out.contains("conforms: no findings"), "{}", text.out);
}

#[test]
fn a_packed_and_extracted_program_is_the_program_that_was_tested() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    let program = fixture::locate().unwrap();
    let target = paddock_plugin_package::manifest::host_target();
    let key = dir.path().join("k.key");
    cli(project, &["keygen", "--out", key.to_str().unwrap(), "-q"]);
    let package = dir.path().join("fixture.paddockplugin");
    let packed = cli(
        project,
        &[
            "pack",
            "--program",
            &format!("{target}={}", program.display()),
            "--key",
            key.to_str().unwrap(),
            "--out",
            package.to_str().unwrap(),
            "-q",
        ],
    );
    assert_eq!(packed.exit, Exit::Ok, "{}", packed.err);
    let opened = paddock_plugin_package::Package::open(&package).unwrap();
    let extracted = dir.path().join("extracted");
    opened.extract(&package, target, &extracted).unwrap();
    let t = cli(
        project,
        &["test", "--program", extracted.to_str().unwrap(), "-q"],
    );
    assert_eq!(t.exit, Exit::Ok, "{}{}", t.out, t.err);
}

#[test]
fn findings_end_with_exit_five_and_name_each_check() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    // A copy that ignores the MCP budget (the test kit's `greedy` plugin), and one with no contributions at all.
    let misbehaving = fixture::locate_misbehaving().unwrap();
    let greedy = dir
        .path()
        .join(format!("greedy{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(&misbehaving, &greedy).unwrap();
    let r = cli(
        project,
        &["test", "--program", greedy.to_str().unwrap(), "--json"],
    );
    assert_eq!(r.exit, Exit::Findings, "{}{}", r.out, r.err);
    let v = r.json();
    assert!(v["count"].as_u64().unwrap() >= 1);
    assert!(v["findings"][0]["check"].is_string() && v["findings"][0]["problem"].is_string());

    let bare = dir
        .path()
        .join(format!("bare{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(&misbehaving, &bare).unwrap();
    let r = cli(project, &["test", "--program", bare.to_str().unwrap()]);
    assert_eq!(r.exit, Exit::Findings);
    assert!(
        r.out.contains("[manifest] ") && r.out.contains("declares the tool fixture.tool"),
        "{}",
        r.out
    );
}

#[test]
fn a_program_that_exits_at_once_or_a_missing_project_is_a_failure_with_a_next_step() {
    let dir = tempfile::tempdir().unwrap();
    let r = cli(dir.path(), &["test", "--no-build"]);
    assert_eq!(r.exit, Exit::Failure);
    assert!(
        r.err.contains("no manifest.toml") && r.err.contains("--dir"),
        "{}",
        r.err
    );
    let project = project(dir.path());
    let r = cli(project, &["test", "--no-build"]);
    assert!(r.err.contains("has not been built"), "{}", r.err);
    // A plugin that dies on its first call is reported, not hung on.
    let misbehaving = fixture::locate_misbehaving().unwrap();
    let exiting = dir
        .path()
        .join(format!("exit{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(&misbehaving, &exiting).unwrap();
    let r = cli(project, &["run", "--program", exiting.to_str().unwrap()]);
    assert_eq!(r.exit, Exit::Failure, "{}{}", r.out, r.err);
    assert!(r.err.contains("did not describe itself"), "{}", r.err);
}

#[test]
fn a_plugin_built_for_an_older_protocol_is_refused_and_says_to_rebuild_it() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(dir.path());
    let v1 = fixture::locate_v1().unwrap();
    let r = cli(
        project,
        &["test", "--program", v1.to_str().unwrap(), "--json"],
    );
    assert_eq!(r.exit, Exit::Refused, "{}{}", r.out, r.err);
    let v = r.json();
    assert!(v["hint"].as_str().unwrap().contains("Rebuild"), "{v}");
    assert!(!v["error"].as_str().unwrap().is_empty());
}
