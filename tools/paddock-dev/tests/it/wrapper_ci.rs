//! What the `./dev` wrapper does for a CI job, which `conformance/ci/kit.sh` did before the kit's shell was retired (WP-2 of
//! docs/plans/shell-to-rust.md): the tool and its build folder live in the CI home's `tools` folder on a host runner, a job without a
//! read token builds from source, the tool's own first build never goes through the job's compiler wrapper and has a default core limit,
//! and a tool the job pinned runs as it is. Each case runs the real wrapper against a scratch CI home and a stand-in `cargo`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// windows-twin: none: it runs the ./dev shell wrapper, which Windows runs through Git bash on every job
#![cfg(unix)]
use super::wrapper::{Fixture, fake_rustc, write_script};
use paddock_testkit::TestCommand;
use std::path::{Path, PathBuf};

/// A scratch runner: a repository with the real wrapper, a home, a stand-in compiler and a stand-in `cargo` that records what it
/// saw of the job's compiler wrapper and core limit, then "builds" a tool that prints what it sees of the same two.
struct Runner {
    f: Fixture,
    scratch: tempfile::TempDir,
    rustc: String,
}

impl Runner {
    fn new() -> Runner {
        let f = Fixture::new();
        let scratch = tempfile::tempdir().unwrap();
        let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
        std::fs::create_dir_all(scratch.path().join("home")).unwrap();
        let cargo = format!(
            r#"echo "wrapper=${{RUSTC_WRAPPER:-}} jobs=${{CARGO_BUILD_JOBS:-}}" >> '{rec}'
mkdir -p "$CARGO_TARGET_DIR/release"
printf '#!/bin/sh\nif [ "$1" = "--version" ]; then echo 0; exit 0; fi\nif [ "$1" = self ]; then echo "embedded %s"; exit 0; fi\necho "tool wrapper=$RUSTC_WRAPPER jobs=$CARGO_BUILD_JOBS home=$PADDOCK_DEV_HOME"\n' "$PADDOCK_DEV_HASH" > "$CARGO_TARGET_DIR/release/paddock-dev"
chmod +x "$CARGO_TARGET_DIR/release/paddock-dev""#,
            rec = scratch.path().join("cargo-saw").display()
        );
        write_script(&f.cargo_home.path().join("shims/cargo"), &cargo);
        Runner { f, scratch, rustc }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.scratch.path().join(rel)
    }

    /// The wrapper with `args` in a job whose environment is `env` (the home, the compiler and the stand-in cargo are the runner's).
    fn run(&self, args: &[&str], env: &[(&str, String)]) -> (i32, String, String) {
        let shims = self.f.cargo_home.path().join("shims");
        let mut cmd = TestCommand::new("sh");
        cmd.arg(self.f.root().join("dev"))
            .args(args)
            .current_dir(self.f.root())
            .path_first(&shims)
            .env("HOME", self.path("home"))
            .env("CARGO_HOME", self.f.cargo_home.path())
            .env("RUSTUP_HOME", self.f.cargo_home.path().join("rustup"))
            .env("RUSTC", &self.rustc);
        cmd.envs(env.iter().map(|(k, v)| (*k, v.as_str())));
        let out = cmd.output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn cargo_saw(&self) -> String {
        std::fs::read_to_string(self.path("cargo-saw")).unwrap_or_default()
    }
}

fn host_job(ci: &Path) -> Vec<(&'static str, String)> {
    vec![
        ("GITHUB_ACTIONS", "true".into()),
        ("RUNNER_OS", "macOS".into()),
        ("PADDOCK_CI_HOME", ci.display().to_string()),
    ]
}

fn dir_has_tool(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|hashes| hashes.flatten().any(|h| h.path().join("paddock-dev-test-host").is_file()))
        .unwrap_or(false)
}

#[test]
fn a_host_runner_job_keeps_the_tool_and_its_build_folder_in_the_ci_homes_tools_folder() {
    let r = Runner::new();
    let ci = r.path("ci");
    std::fs::create_dir_all(&ci).unwrap();
    let ci_real = std::fs::canonicalize(&ci).unwrap();
    let (code, out, err) = r.run(&["hello"], &host_job(&ci));
    assert_eq!(code, 0, "{err}");
    // No read token: the tool was built from source and is kept in its own folder, in the CI home, not in the runner's home.
    assert!(
        dir_has_tool(&ci_real.join("tools/paddock-dev/bin-source")),
        "{err}\n{:?}",
        std::fs::read_dir(ci_real.join("tools")).map(|d| d.flatten().map(|e| e.path()).collect::<Vec<_>>())
    );
    assert!(ci_real.join("tools/paddock-dev-target").is_dir(), "the build folder is the CI home's too");
    assert!(!r.path("home/.paddock-dev").exists(), "nothing in the runner's own home");
    assert!(out.contains(&format!("home={}", ci_real.join("tools/paddock-dev").display())), "the tool sees its home: {out}");
    assert!(!err.contains("could not download"), "a job with no token never tries a download: {err}");
}

#[test]
fn an_explicit_tool_home_wins_over_the_ci_home() {
    let r = Runner::new();
    let ci = r.path("ci");
    std::fs::create_dir_all(&ci).unwrap();
    let mine = r.path("mine");
    let mut env = host_job(&ci);
    env.push(("PADDOCK_DEV_HOME", mine.display().to_string()));
    let (code, _, err) = r.run(&["hello"], &env);
    assert_eq!(code, 0, "{err}");
    assert!(dir_has_tool(&mine.join("bin-source")), "{err}");
    assert!(!ci.join("tools").exists());
}

#[test]
fn a_linux_container_job_and_a_local_run_do_not_use_the_ci_home() {
    let r = Runner::new();
    let ci = r.path("ci");
    std::fs::create_dir_all(&ci).unwrap();
    let mut linux = host_job(&ci);
    linux[1] = ("RUNNER_OS", "Linux".into());
    let (code, _, err) = r.run(&["hello"], &linux);
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = r.run(&["hello"], &[("RUNNER_OS", "macOS".into()), ("PADDOCK_CI_HOME", ci.display().to_string())]);
    assert_eq!(code, 0, "{err}");
    assert!(!ci.join("tools").exists(), "neither used the CI home");
}

#[test]
fn a_ci_home_that_is_relative_or_not_mounted_stops_the_job_and_creates_nothing() {
    let r = Runner::new();
    let mut env = host_job(&r.path("unmounted"));
    let (code, out, err) = r.run(&["hello"], &env);
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    assert!(err.contains("::error::PADDOCK_CI_HOME") && err.contains("does not exist: is its disk mounted"), "{err}");
    assert!(!r.path("unmounted").exists(), "the home is not made again on the internal disk");
    env[2] = ("PADDOCK_CI_HOME", "ci".into());
    let (code, _, err) = r.run(&["hello"], &env);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("is not an absolute path"), "{err}");
    assert!(r.cargo_saw().is_empty(), "nothing was built");
}

#[test]
fn the_default_ci_home_is_made_on_first_use_and_a_link_to_a_relocated_one_is_resolved() {
    let r = Runner::new();
    let env = vec![("GITHUB_ACTIONS", "true".to_string()), ("RUNNER_OS", "macOS".into())];
    let (code, _, err) = r.run(&["hello"], &env);
    assert_eq!(code, 0, "{err}");
    assert!(dir_has_tool(&r.path("home/.paddock-ci/tools/paddock-dev/bin-source")), "{err}");
    let disk = r.path("disk");
    std::fs::create_dir_all(&disk).unwrap();
    std::os::unix::fs::symlink(&disk, r.path("link")).unwrap();
    let (code, _, err) = r.run(&["hello"], &host_job(&r.path("link")));
    assert_eq!(code, 0, "{err}");
    assert!(
        dir_has_tool(&std::fs::canonicalize(&disk).unwrap().join("tools/paddock-dev/bin-source")),
        "the link is resolved: {err}"
    );
}

#[test]
fn the_tools_own_build_has_no_compiler_wrapper_and_a_core_limit_of_four_and_the_tool_keeps_both_variables() {
    let r = Runner::new();
    let ci = r.path("ci");
    std::fs::create_dir_all(&ci).unwrap();
    let mut env = host_job(&ci);
    env.push(("RUSTC_WRAPPER", "sccache".into()));
    let (code, out, err) = r.run(&["hello"], &env);
    assert_eq!(code, 0, "{err}");
    assert_eq!(r.cargo_saw().trim(), "wrapper= jobs=4", "the build: no job wrapper, the default limit");
    assert!(out.contains("tool wrapper=sccache jobs= "), "the tool and what it starts keep the job's own: {out}");
    // The job's own CARGO_BUILD_JOBS is the limit once it has one, and a local run is never limited.
    let r2 = Runner::new();
    let ci2 = r2.path("ci");
    std::fs::create_dir_all(&ci2).unwrap();
    let mut env = host_job(&ci2);
    env.push(("CARGO_BUILD_JOBS", "7".into()));
    r2.run(&["hello"], &env);
    assert_eq!(r2.cargo_saw().trim(), "wrapper= jobs=7");
    let r3 = Runner::new();
    r3.run(&["hello"], &[]);
    assert_eq!(r3.cargo_saw().trim(), "wrapper= jobs=");
}

#[test]
fn a_tool_the_job_pinned_runs_as_it_is_for_ci_commands_only() {
    let r = Runner::new();
    let pinned = r.path("pinned");
    write_script(&pinned, r#"if [ "$1" = "--version" ]; then echo 0; else echo "pinned ran $*"; fi"#);
    let job = vec![("GITHUB_ACTIONS", "true".to_string()), ("PADDOCK_DEV", pinned.display().to_string())];
    let (code, out, err) = r.run(&["ci", "isolation", "release"], &job);
    assert_eq!((code, out.trim()), (0, "pinned ran ci isolation release"), "{err}");
    assert!(r.cargo_saw().is_empty(), "no hash, no build");
    // Another command still goes through the wrapper (it is not a CI step), and so does a pin that does not run.
    let (_, out, _) = r.run(&["hello"], &job);
    assert!(out.contains("tool wrapper="), "{out}");
    write_script(&pinned, "exit 3");
    let (_, out, _) = r.run(&["ci", "x"], &job);
    assert!(out.contains("tool wrapper="), "a pin that does not run is not used: {out}");
    // The wrapper named as the pin is not run again (no loop).
    let mut me = job.clone();
    me[1] = ("PADDOCK_DEV", r.f.root().join("dev").display().to_string());
    let (code, out, _) = r.run(&["ci", "x"], &me);
    assert_eq!(code, 0);
    assert!(out.contains("tool wrapper="), "{out}");
}
