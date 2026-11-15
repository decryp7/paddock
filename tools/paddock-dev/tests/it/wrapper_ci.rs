//! What the `./dev` wrapper does for a CI job, which `conformance/ci/kit.sh` did before the kit's shell was retired (WP-2 of
//! docs/plans/shell-to-rust.md): the tool and its build folder live in the CI home's `tools` folder on a host runner, a job without a
//! read token builds from source, the tool's own first build never goes through the job's compiler wrapper and has a default core limit,
//! and a tool the job pinned runs as it is. Each case runs the real wrapper against a scratch CI home and a stand-in `cargo`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// windows-twin: none: it runs the ./dev shell wrapper, which Windows runs through Git bash on every job
#![cfg(unix)]
use super::wrapper::{Fixture, fake_rustc, install_toolchain, run_wrapper, rustup_without_toolchain, write_script};
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
            .env("RUSTC", &self.rustc)
            // Not the machine's own kept mount (the CI image has /sccache): a host runner has none.
            .env("PADDOCK_CI_MOUNT", self.path("no-mount"));
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
    let ci_real = paddock_env::winpath::real(&ci).unwrap();
    let (code, out, err) = r.run(&["hello"], &host_job(&ci));
    assert_eq!(code, 0, "{err}");
    // No read token: the tool was built from source and is kept in its own folder, in the CI home, not in the runner's home.
    assert!(
        dir_has_tool(&ci_real.join("tools/paddock-dev/bin-source")),
        "{err}\n{:?}",
        std::fs::read_dir(ci_real.join("tools")).map(|d| d.flatten().map(|e| e.path()).collect::<Vec<_>>())
    );
    assert!(
        ci_real.join("tools/paddock-dev-target").is_dir(),
        "the build folder is the CI home's too"
    );
    assert!(!r.path("home/.paddock-dev").exists(), "nothing in the runner's own home");
    assert!(
        out.contains(&format!("home={}", ci_real.join("tools/paddock-dev").display())),
        "the tool sees its home: {out}"
    );
    assert!(
        !err.contains("could not download"),
        "a job with no token never tries a download: {err}"
    );
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
    let (code, _, err) = r.run(
        &["hello"],
        &[("RUNNER_OS", "macOS".into()), ("PADDOCK_CI_HOME", ci.display().to_string())],
    );
    assert_eq!(code, 0, "{err}");
    assert!(!ci.join("tools").exists(), "neither used the CI home");
}

#[test]
fn a_ci_home_that_is_relative_or_not_mounted_stops_the_job_and_creates_nothing() {
    let r = Runner::new();
    let mut env = host_job(&r.path("unmounted"));
    let (code, out, err) = r.run(&["hello"], &env);
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    assert!(
        err.contains("::error::PADDOCK_CI_HOME") && err.contains("does not exist: is its disk mounted"),
        "{err}"
    );
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
        dir_has_tool(&paddock_env::winpath::real(&disk).unwrap().join("tools/paddock-dev/bin-source")),
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
    env.push(("CARGO_BUILD_JOBS", String::new()));
    let (code, out, err) = r.run(&["hello"], &env);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        r.cargo_saw().trim(),
        "wrapper= jobs=4",
        "the build: no job wrapper, the default limit"
    );
    assert!(
        out.contains("tool wrapper=sccache jobs= "),
        "the tool and what it starts keep the job's own: {out}"
    );
    // The job's own CARGO_BUILD_JOBS is the limit once it has one, and a local run is never limited.
    let r2 = Runner::new();
    let ci2 = r2.path("ci");
    std::fs::create_dir_all(&ci2).unwrap();
    let mut env = host_job(&ci2);
    env.push(("CARGO_BUILD_JOBS", "7".into()));
    r2.run(&["hello"], &env);
    assert_eq!(r2.cargo_saw().trim(), "wrapper= jobs=7");
    let r3 = Runner::new();
    r3.run(&["hello"], &[("CARGO_BUILD_JOBS", String::new())]);
    assert_eq!(r3.cargo_saw().trim(), "wrapper= jobs=");
}

#[test]
fn a_tool_the_job_pinned_runs_as_it_is_for_ci_commands_only() {
    let r = Runner::new();
    let pinned = r.path("pinned");
    write_script(&pinned, r#"if [ "$1" = "--version" ]; then echo 0; else echo "pinned ran $*"; fi"#);
    let job = vec![
        ("GITHUB_ACTIONS", "true".to_string()),
        ("PADDOCK_DEV", pinned.display().to_string()),
    ];
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

/// P1-2 of the WP-2 review: the kit's shell wrote PADDOCK_TOOLCHAIN_FAILED=1 to the job's environment when the pinned toolchain could not
/// be installed, so that the job's other steps (the always-run clean-ups) did not each try the 900 s install again. The wrapper reads the
/// variable; it now also writes it.
#[test]
fn a_failed_toolchain_install_in_a_ci_job_marks_the_job_so_no_later_step_tries_again() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    rustup_without_toolchain(&f, "exit 1");
    let env_file = scratch.path().join("github-env");
    let job = [
        ("GITHUB_ACTIONS", "true".to_string()),
        ("GITHUB_ENV", env_file.display().to_string()),
    ];
    let (code, _, err) = run_wrapper(&f, &scratch.path().join("home"), &job);
    assert_eq!(code, 2, "{err}");
    assert_eq!(
        std::fs::read_to_string(&env_file).unwrap_or_default(),
        "PADDOCK_TOOLCHAIN_FAILED=1\n",
        "{err}"
    );
    // A person's run (no GITHUB_ACTIONS) writes nothing, whatever GITHUB_ENV holds.
    let other = scratch.path().join("other-env");
    let (code, _, _) = run_wrapper(&f, &scratch.path().join("home2"), &[("GITHUB_ENV", other.display().to_string())]);
    assert_eq!(code, 2);
    assert!(!other.exists());
}

/// P1-3: the job-setup mode is the one place a failed setup step marks the job (PADDOCK_JOB_ABORTED=1, which the later `!cancelled()`
/// steps test, conformance C117) and says so once; it covers a tool that cannot be built, which no action's own script could.
#[test]
fn job_setup_marks_the_job_and_says_so_once_when_the_step_or_the_tool_build_fails() {
    let r = Runner::new();
    let env_file = r.path("github-env");
    let job = |extra: &[(&'static str, String)]| {
        let mut v = vec![("GITHUB_ENV", env_file.display().to_string())];
        v.extend(extra.iter().cloned());
        v
    };
    // A good step: exit 0 and nothing written.
    let (code, out, err) = r.run(&["--job-setup", "hello"], &job(&[]));
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("tool wrapper="), "the step ran: {out}");
    assert!(!env_file.exists(), "a good step marks nothing");
    // A tool that cannot be built (a runner of its own: the first one already cached a built tool).
    let r = Runner::new();
    let env_file = r.path("github-env");
    let job = |extra: &[(&'static str, String)]| {
        let mut v = vec![("GITHUB_ENV", env_file.display().to_string())];
        v.extend(extra.iter().cloned());
        v
    };
    write_script(&r.f.cargo_home.path().join("shims/cargo"), "echo 'error: no build' >&2; exit 1");
    let (code, _, err) = r.run(&["--job-setup", "ci", "build-jobs"], &job(&[]));
    assert_ne!(code, 0, "{err}");
    assert_eq!(
        std::fs::read_to_string(&env_file).unwrap_or_default(),
        "PADDOCK_JOB_ABORTED=1\n",
        "{err}"
    );
    assert_eq!(err.matches("A job setup step failed").count(), 1, "one error line: {err}");
    assert!(
        err.contains("exit ") && err.contains("./dev ci build-jobs"),
        "it names the step and the exit: {err}"
    );
    // A step of the tool that fails: its own exit code is kept.
    std::fs::remove_file(&env_file).unwrap();
    let pinned = r.path("pinned");
    write_script(&pinned, r#"if [ "$1" = "--version" ]; then echo 0; else exit 3; fi"#);
    let (code, _, err) = r.run(
        &["--job-setup", "ci", "isolation", "acquire"],
        &job(&[("GITHUB_ACTIONS", "true".into()), ("PADDOCK_DEV", pinned.display().to_string())]),
    );
    assert_eq!(code, 3, "{err}");
    assert_eq!(std::fs::read_to_string(&env_file).unwrap_or_default(), "PADDOCK_JOB_ABORTED=1\n");
    // Without GITHUB_ENV it still fails and says so.
    let (code, _, err) = r.run(&["--job-setup", "ci", "build-jobs"], &[]);
    assert_ne!(code, 0);
    assert_eq!(err.matches("A job setup step failed").count(), 1, "{err}");
}

/// The wrapper reads the CI home itself, because the tool that knows the rule is not built yet (docs/known-copies.toml, `ci-home-rule`).
/// This holds the two equal: for each way a CI home can be given, the folder the wrapper keeps the tool in is `ci isolation path tools`'s
/// answer, and a home the tool refuses stops the wrapper the same way.
#[test]
fn the_wrappers_ci_home_is_the_one_ci_isolation_gives() {
    let r = Runner::new();
    let disk = r.path("disk");
    std::fs::create_dir_all(&disk).unwrap();
    std::os::unix::fs::symlink(&disk, r.path("link")).unwrap();
    let cases: Vec<(&str, Option<String>)> = vec![
        ("the default home", None),
        ("a relocated home", Some(disk.display().to_string())),
        // conformance: path-text — the trailing slash is the case under test, not an expected path
        ("a trailing slash", Some(format!("{}/", disk.display()))),
        ("a link", Some(r.path("link").display().to_string())),
        ("a relative path", Some("disk".to_string())),
        ("an unmounted disk", Some(r.path("unmounted").display().to_string())),
    ];
    for (what, home) in cases {
        let mut env = vec![("GITHUB_ACTIONS", "true".to_string()), ("RUNNER_OS", "macOS".to_string())];
        if let Some(h) = &home {
            env.push(("PADDOCK_CI_HOME", h.clone()));
        }
        let tool = {
            let mut cmd = TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"));
            cmd.args(["ci", "isolation", "path", "tools"]).env("HOME", r.path("home"));
            for (k, v) in &env {
                cmd.env(k, v);
            }
            cmd.output().unwrap()
        };
        let (code, out, err) = r.run(&["hello"], &env);
        if tool.status.success() {
            let tools = String::from_utf8_lossy(&tool.stdout).trim().to_string();
            assert_eq!(code, 0, "{what}: {err}");
            assert!(
                out.contains(&format!("home={tools}/paddock-dev")),
                "{what}: the wrapper keeps the tool in {tools}/paddock-dev: {out}"
            );
        } else {
            assert_eq!(code, tool.status.code().unwrap(), "{what}: both stop the job: {err}");
            let said = String::from_utf8_lossy(&tool.stdout).to_string() + &String::from_utf8_lossy(&tool.stderr);
            for phrase in ["is not an absolute path", "does not exist: is its disk mounted"] {
                assert_eq!(
                    said.contains(phrase),
                    err.contains(phrase),
                    "{what}: the same refusal ({phrase}): tool {said} wrapper {err}"
                );
            }
        }
        // Each case starts from its own home folder, so a default home made by one is not found by the next.
        let _ = std::fs::remove_dir_all(r.path("home/.paddock-ci"));
    }
}

/// P2-2 of the WP-2 review: the setup-rust action's own install (`--install-toolchain`) remembers a failure for the job too, in a CI job
/// only, and `--job-setup --require-rustup` marks the job when the runner has no rustup (the first thing that step checks).
#[test]
fn a_failed_install_toolchain_marks_the_job_and_a_missing_rustup_marks_it_through_job_setup() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let (shims, _) = rustup_without_toolchain(&f, "exit 1");
    let env_file = scratch.path().join("github-env");
    let outside = tempfile::tempdir().unwrap();
    let run = |ci: bool| {
        let mut cmd = paddock_testkit::TestCommand::new("sh");
        cmd.arg(f.root().join("dev"))
            .arg("--install-toolchain")
            .args(["1.90.0", "clippy", ""])
            .current_dir(outside.path())
            .path_first(&shims)
            .env("RUSTUP_HOME", f.cargo_home.path().join("rustup"))
            .env("GITHUB_ENV", &env_file)
            .env_remove("RUSTUP_TOOLCHAIN");
        if ci {
            cmd.env("GITHUB_ACTIONS", "true");
        }
        cmd.output().unwrap()
    };
    let out = run(false);
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!env_file.exists(), "a person's run writes nothing");
    let out = run(true);
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        std::fs::read_to_string(&env_file).unwrap_or_default(),
        "PADDOCK_TOOLCHAIN_FAILED=1\n"
    );
    // The plain helper the older tests use still works without the variable.
    let _ = install_toolchain;
    // No rustup on PATH: `--job-setup --require-rustup` fails, says so once and marks the job; with one it passes.
    std::fs::remove_file(&env_file).unwrap();
    let empty = scratch.path().join("empty-bin");
    std::fs::create_dir_all(&empty).unwrap();
    // Only `sh` (what the wrapper starts itself with): no rustup, no cargo.
    std::os::unix::fs::symlink("/bin/sh", empty.join("sh")).unwrap();
    let no_rustup = paddock_testkit::TestCommand::new("sh")
        .arg(f.root().join("dev"))
        .args(["--job-setup", "--require-rustup"])
        .current_dir(outside.path())
        .env("PATH", &empty)
        .env("GITHUB_ENV", &env_file)
        .output()
        .unwrap();
    assert_eq!(no_rustup.status.code(), Some(1));
    let err = String::from_utf8_lossy(&no_rustup.stderr);
    assert!(err.contains("rustup is not installed"), "{err}");
    assert_eq!(err.matches("A job setup step failed").count(), 1, "{err}");
    assert_eq!(std::fs::read_to_string(&env_file).unwrap_or_default(), "PADDOCK_JOB_ABORTED=1\n");
    std::fs::remove_file(&env_file).unwrap();
    let with = paddock_testkit::TestCommand::new("sh")
        .arg(f.root().join("dev"))
        .args(["--job-setup", "--require-rustup"])
        .current_dir(outside.path())
        .path_first(&shims)
        .env("GITHUB_ENV", &env_file)
        .output()
        .unwrap();
    assert!(with.status.success(), "{}", String::from_utf8_lossy(&with.stderr));
    assert!(!env_file.exists());
}
