//! `./dev ci release-isolation` through the real binary (ADR 0085 WP-2): the exit codes and `::error::` lines a workflow
//! step sees, on a synthetic job under a scratch folder with its own HOME (ER-14). The probes themselves are unit-tested.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_dev::release_isolation::fixture::{Fixture, Leak};
use paddock_testkit::TestCommand;
use std::process::Output;

// Starts the tool under test, git or a helper by program name (a throw-away fixture), never a process by pid (C53).
#[allow(clippy::disallowed_methods)]
fn dev(f: &Fixture, args: &[&str]) -> Output {
    let mut cmd = TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"));
    cmd.args(["ci", "release-isolation"]).args(args).env_clear();
    for (k, v) in &f.env.0 {
        cmd.env(k, v);
    }
    cmd.env("PADDOCK_RELEASE_LEDGER", &f.ledger);
    cmd.output().unwrap()
}

fn verify_args(f: &Fixture) -> Vec<String> {
    let mut a = vec!["verify".to_string()];
    for w in &f.opts.cargo_workspaces {
        a.extend(["--cargo-workspace".into(), w.clone()]);
    }
    for d in &f.opts.app_dirs {
        a.extend(["--app-dir".into(), d.clone()]);
    }
    if let Some(s) = &f.opts.sccache_stats {
        a.extend(["--sccache-stats".into(), s.display().to_string()]);
    }
    if let Some(p) = &f.opts.tauri_pins {
        a.extend(["--tauri-pins".into(), p.display().to_string()]);
    }
    a
}

fn run_verify(f: &Fixture) -> Output {
    let args = verify_args(f);
    dev(f, &args.iter().map(String::as_str).collect::<Vec<_>>())
}

#[test]
fn verify_passes_a_clean_job_with_the_summary_line_and_fails_a_leak_with_one_error_per_finding() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = Fixture::build(dir.path());
    let ok = run_verify(&f);
    assert!(
        ok.status.success(),
        "{}{}",
        String::from_utf8_lossy(&ok.stdout),
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(
        String::from_utf8_lossy(&ok.stdout)
            .contains("release isolation: 8 kinds made by this run, 0 shared; 2 crate archives match Cargo.lock")
    );
    f.plant(Leak::ConfigInParent);
    let bad = run_verify(&f);
    assert!(!bad.status.success());
    let text = format!("{}{}", String::from_utf8_lossy(&bad.stdout), String::from_utf8_lossy(&bad.stderr));
    assert_eq!(text.matches("::error::release-isolation cargo-config:").count(), 1, "{text}");
    assert!(text.contains("Next: "), "{text}");
}

#[test]
fn record_is_the_one_writer_one_line_per_kind() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::build(dir.path());
    let again = dev(&f, &["record", "libduckdb", "job", "--path", &f.npm.display().to_string()]);
    assert!(again.status.success(), "{}", String::from_utf8_lossy(&again.stderr));
    let twice = dev(&f, &["record", "libduckdb", "job", "--path", &f.npm.display().to_string()]);
    assert!(!twice.status.success());
    assert!(String::from_utf8_lossy(&twice.stderr).contains("one line per kind"));
    let shared = dev(&f, &["record", "angular", "shared"]);
    assert!(
        !shared.status.success(),
        "a mode other than job, run or off is refused at the writer too"
    );
}

#[test]
// Starts the tool under test, git or a helper by program name (a throw-away fixture), never a process by pid (C53).
#[allow(clippy::disallowed_methods)]
fn setup_prints_the_environment_and_refuses_to_run_twice() {
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("job");
    std::fs::create_dir_all(&temp).unwrap();
    let envfile = dir.path().join("github-env");
    let run = || {
        TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"))
            .args(["ci", "release-isolation", "setup", "--env-file"])
            .arg(&envfile)
            .env_clear()
            .env("RUNNER_TEMP", &temp)
            .output()
            .unwrap()
    };
    let first = run();
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    let env = std::fs::read_to_string(&envfile).unwrap();
    for key in [
        "PADDOCK_RELEASE_MODE=1",
        "CARGO_HOME=",
        "npm_config_cache=",
        "PADDOCK_RELEASE_TAURI_TOOLS=",
        "RUSTC_WRAPPER=\n",
    ] {
        assert!(env.contains(key), "{key} in {env}");
    }
    assert!(!run().status.success());
}
