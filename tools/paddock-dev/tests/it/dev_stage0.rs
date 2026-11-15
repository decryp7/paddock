//! `./dev`, stage 0 (shell-to-rust WP-1): the script that builds the launcher with cargo when its sources changed and runs it. A
//! stand-in `cargo`, `rustc` and `rustup` "build" a script and keep a list of installed toolchains, so these tests need no compiler;
//! the launcher itself is tested in `tools/dev-launch/tests` and `wrapper.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// windows-twin: none: the cargo and rustup stand-ins are sh scripts; Windows runs dev-launch's own tests (tools/dev-launch/tests/it) on every run, and dev.cmd in the test VM
#![cfg(unix)]
use super::wrapper::write_script;
use paddock_testkit::TestCommand;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    crate::full_tree(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap())
}

struct Stage0 {
    dir: tempfile::TempDir,
}

fn git(root: &Path, args: &[&str]) {
    assert!(
        TestCommand::new("git").args(args).current_dir(root).status().unwrap().success(),
        "git {args:?}"
    );
}

/// A committed repository holding the real `dev` and a placeholder launcher crate, and stand-ins first on `PATH`: a `cargo` that
/// logs each build and writes a launcher script where `--target-dir` says (it runs only when `cargo_runs` or after a toolchain
/// install), a `rustc` that answers by `RUSTUP_TOOLCHAIN` (`1.80*` is old, `none*` does not run, the rest are 1.100.0) and a
/// `rustup` that lists the toolchains in `shims/list` ("no installed toolchains" when empty) and installs one slowly.
fn setup(cargo_runs: bool, toolchains: &[&str]) -> Stage0 {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir_all(root.join("tools/dev-launch/src")).unwrap();
    std::fs::create_dir_all(root.join("tools/paddock-dev")).unwrap();
    std::fs::copy(repo_root().join("dev"), root.join("dev")).unwrap();
    std::fs::write(root.join(".gitignore"), "/tools/dev-launch/target/\n").unwrap();
    std::fs::write(root.join("tools/dev-launch/Cargo.toml"), "# placeholder\n").unwrap();
    std::fs::write(root.join("tools/dev-launch/Cargo.lock"), "# placeholder\n").unwrap();
    std::fs::write(root.join("tools/dev-launch/src/main.rs"), "fn main() {}\n").unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "fixture"]);
    let shims = dir.path().join("shims");
    let (log, marker, list) = (dir.path().join("cargo.log"), shims.join("installed"), shims.join("list"));
    std::fs::create_dir_all(&shims).unwrap();
    std::fs::write(&list, toolchains.iter().map(|t| format!("{t}\n")).collect::<String>()).unwrap();
    write_script(
        &shims.join("cargo"),
        &format!(
            r#"if [ "$1" = --version ]; then {run}; fi
echo "build rustup_toolchain=[${{RUSTUP_TOOLCHAIN:-}}] build_target=[${{CARGO_BUILD_TARGET:-}}] $*" >> '{log}'
while [ $# -gt 0 ]; do [ "$1" = --target-dir ] && t=$2; shift; done
/bin/mkdir -p "$t/release"
printf '#!/bin/sh\necho "launcher ran: $*; toolchain=[${{RUSTUP_TOOLCHAIN:-}}] git_dir=[${{GIT_DIR:-}}]"\n' > "$t/release/dev-launch"
/bin/chmod +x "$t/release/dev-launch""#,
            run = if cargo_runs {
                "echo cargo 1.100.0; exit 0".to_string()
            } else {
                format!(
                    "[ -e '{}' ] && {{ echo cargo 1.100.0; exit 0; }}; echo 'error: no default toolchain' >&2; exit 1",
                    marker.display()
                )
            },
            log = log.display()
        ),
    );
    write_script(
        &shims.join("rustc"),
        r#"case "${RUSTUP_TOOLCHAIN:-}" in no|none*|'') exit 1 ;; 1.80*) echo "rustc 1.80.0 (abc 2024-01-01)" ;; *) echo "rustc 1.100.0 (abc 2026-01-01)" ;; esac"#,
    );
    write_script(
        &shims.join("rustup"),
        &format!(
            r#"case "$1 $2" in
  'toolchain list') if [ -s '{list}' ]; then cat '{list}'; else echo 'no installed toolchains'; fi ;;
  'toolchain install') echo "install in $PWD: $*" >> '{inst}'; sleep 2; echo '1.100.0-test (default)' >> '{list}'; touch '{marker}' ;;
esac"#,
            list = list.display(),
            inst = dir.path().join("installs.log").display(),
            marker = marker.display()
        ),
    );
    Stage0 { dir }
}

impl Stage0 {
    fn root(&self) -> PathBuf {
        self.dir.path().join("repo")
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("cargo.log")).unwrap_or_default()
    }

    fn builds(&self) -> usize {
        self.log().lines().filter(|l| l.starts_with("build ")).count()
    }

    fn installs(&self) -> usize {
        std::fs::read_to_string(self.dir.path().join("installs.log")).map_or(0, |t| t.lines().count())
    }

    fn bins(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.root().join("tools/dev-launch/target/bin"))
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    }

    fn command(&self, cwd: &Path, args: &[&str], env: &[(&str, String)]) -> TestCommand {
        let mut cmd = TestCommand::new("sh");
        cmd.arg(self.root().join("dev"))
            .args(args)
            .current_dir(cwd)
            .path_first(&self.dir.path().join("shims"))
            .env("RUSTUP_HOME", self.dir.path().join("rustup"))
            // The test run's own toolchain choice is not part of what is tested.
            .env_remove("RUSTUP_TOOLCHAIN")
            .envs(env.iter().map(|(k, v)| (*k, v.as_str())));
        cmd
    }

    fn dev(&self, cwd: &Path, args: &[&str], env: &[(&str, String)]) -> (i32, String, String) {
        let out = self.command(cwd, args, env).output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

const GOOD: &[&str] = &["1.100.0-test (default)"];

#[test]
fn the_first_run_builds_the_launcher_and_passes_every_argument_on_later_runs_do_not_build() {
    let s = setup(true, GOOD);
    let (code, out, err) = s.dev(&s.root(), &["--wrapper-hash", "two words", "x"], &[]);
    assert_eq!(
        (code, out.trim()),
        (0, "launcher ran: --wrapper-hash two words x; toolchain=[] git_dir=[]"),
        "{err}"
    );
    assert_eq!(s.builds(), 1);
    let log = s.log();
    for part in [
        "-q",
        "--release",
        "--locked",
        "--manifest-path",
        "tools/dev-launch/Cargo.toml",
        "--target-dir",
        "tools/dev-launch/target",
    ] {
        assert!(log.contains(part), "cargo build lacks {part}: {log}");
    }
    assert_eq!(s.bins().len(), 1, "one published launcher: {:?}", s.bins());
    let (code, _, err) = s.dev(&s.root(), &["again"], &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        s.builds(),
        1,
        "an up-to-date launcher is not built again: cargo is not even started"
    );
}

#[test]
fn the_published_launcher_is_never_replaced_and_does_not_depend_on_cargos_own_output() {
    // Cargo replaces its uplifted exe even on a no-op build, and Windows cannot replace a running program: ./dev runs a copy named
    // by the stamp of the sources, and a later call neither builds nor touches it.
    use std::os::unix::fs::MetadataExt;
    let s = setup(true, GOOD);
    assert_eq!(s.dev(&s.root(), &[], &[]).0, 0);
    let bin = s.bins().remove(0);
    let before = std::fs::metadata(&bin).unwrap().ino();
    std::fs::remove_file(s.root().join("tools/dev-launch/target/release/dev-launch")).unwrap();
    assert_eq!(s.dev(&s.root(), &[], &[]).0, 0);
    assert_eq!(s.builds(), 1);
    assert_eq!(std::fs::metadata(&bin).unwrap().ino(), before);
}

#[test]
fn a_changed_source_builds_again_committed_or_not_and_the_old_launcher_goes() {
    let s = setup(true, GOOD);
    assert_eq!(s.dev(&s.root(), &[], &[]).0, 0);
    let first = s.bins();
    std::fs::write(s.root().join("tools/dev-launch/src/main.rs"), "fn main() { 1; }\n").unwrap();
    assert_eq!(s.dev(&s.root(), &[], &[]).0, 0, "an uncommitted change");
    assert_eq!(s.builds(), 2);
    assert_eq!(s.dev(&s.root(), &[], &[]).0, 0);
    assert_eq!(s.builds(), 2, "the same uncommitted change is not built twice");
    git(&s.root(), &["commit", "-qam", "change"]);
    assert_eq!(s.dev(&s.root(), &[], &[]).0, 0, "the same change, committed, is a new stamp");
    assert_eq!(s.builds(), 3);
    let now = s.bins();
    assert_eq!(now.len(), 1, "{now:?}");
    assert_ne!(now, first);
}

#[test]
fn a_cargo_that_cannot_run_yet_builds_with_an_installed_toolchain_of_1_89_or_newer_and_the_launcher_does_not_inherit_it() {
    let s = setup(false, &["1.80.0-test (default)", "none-test", "1.95.0-test"]);
    let (code, out, err) = s.dev(&s.root(), &["hello"], &[]);
    assert_eq!(code, 0, "{err}");
    assert!(
        s.log().contains("rustup_toolchain=[1.95.0-test]"),
        "the old and the broken ones are skipped: {}",
        s.log()
    );
    assert!(
        out.contains("toolchain=[]"),
        "the launcher must see the repository's pin, not the toolchain that built it: {out}"
    );
    assert_eq!(s.installs(), 0, "a machine with a usable toolchain installs nothing");
}

#[test]
fn only_old_toolchains_is_an_error_that_installs_nothing() {
    let s = setup(false, &["1.80.0-test (default)"]);
    let (code, _, err) = s.dev(&s.root(), &[], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("no installed Rust toolchain is 1.89 or newer") && err.contains("rustup toolchain install stable"),
        "{err}"
    );
    assert_eq!((s.installs(), s.builds()), (0, 0));
}

#[test]
fn a_machine_with_rustup_and_no_toolchain_at_all_installs_the_pin_once_in_the_tools_folder() {
    let s = setup(false, &[]);
    let (code, out, err) = s.dev(&s.root(), &["hello"], &[]);
    assert_eq!(code, 0, "{err}");
    let installs = std::fs::read_to_string(s.dir.path().join("installs.log")).unwrap();
    assert!(
        installs
            .trim_end()
            .ends_with("tools/paddock-dev: toolchain install --no-self-update"),
        "rustup reads the pin from the tool's folder and never updates itself: {installs}"
    );
    assert!(out.contains("launcher ran: hello"), "{out}");
}

/// Run 8060's shape: two jobs of one runner meet a machine with no toolchain at the same moment (setup-rust now starts with stage 0).
/// One install runs; the other waits for it and builds with what it left.
#[test]
fn two_concurrent_stage_0s_with_no_toolchain_make_exactly_one_install() {
    let s = setup(false, &[]);
    let runs: Vec<_> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..2).map(|_| sc.spawn(|| s.dev(&s.root(), &["x"], &[]))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (code, _, err) in &runs {
        assert_eq!(*code, 0, "{err}");
    }
    assert_eq!(s.installs(), 1, "one install, not two");
    assert!(
        !s.dir.path().join("rustup/.paddock-stage0-install.lock").exists(),
        "the lock is released"
    );
}

#[test]
fn after_a_failed_install_earlier_in_the_job_stage_0_installs_nothing() {
    let s = setup(false, &[]);
    let (code, out, err) = s.dev(&s.root(), &[], &[("PADDOCK_TOOLCHAIN_FAILED", "1".to_string())]);
    assert_eq!((code, s.installs()), (2, 0), "out={out} err={err} log={}", s.log());
    assert!(err.contains("PADDOCK_TOOLCHAIN_FAILED=1"), "{err}");
}

#[test]
fn a_stuck_install_ends_at_the_limit_with_the_next_step_and_removes_its_lock() {
    let s = setup(false, &[]);
    write_script(
        &s.dir.path().join("shims/rustup"),
        "case \"$1 $2\" in 'toolchain list') echo 'no installed toolchains' ;; 'toolchain install') exec sleep 30 ;; esac",
    );
    let (code, _, err) = s.dev(&s.root(), &[], &[("PADDOCK_DEV_TOOLCHAIN_TIMEOUT", "1".to_string())]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("did not leave a working toolchain"), "{err}");
    assert!(!s.dir.path().join("rustup/.paddock-stage0-install.lock").exists());
}

#[test]
fn a_cargo_build_target_never_moves_the_launcher_and_a_git_hooks_variables_never_move_the_repository() {
    let s = setup(true, GOOD);
    let other = s.dir.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q"]);
    let (code, out, err) = s.dev(
        &s.root(),
        &["x"],
        &[
            ("CARGO_BUILD_TARGET", "wasm32-unknown-unknown".to_string()),
            ("GIT_DIR", other.join(".git").display().to_string()),
            ("GIT_WORK_TREE", other.display().to_string()),
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert!(s.log().contains("build_target=[]"));
    assert!(out.contains("git_dir=[]"), "the launcher inherits neither: {out}");
}

#[test]
fn outside_a_repository_and_in_a_checkout_without_the_launcher_it_says_what_to_do_and_exits_2() {
    let s = setup(true, GOOD);
    let outside = tempfile::tempdir().unwrap();
    let (code, _, err) = s.dev(
        outside.path(),
        &[],
        &[("GIT_CEILING_DIRECTORIES", outside.path().parent().unwrap().display().to_string())],
    );
    assert_eq!(code, 2);
    assert!(err.contains("dev: run this inside the Paddock repository."), "{err}");
    std::fs::remove_file(s.root().join("tools/dev-launch/Cargo.toml")).unwrap();
    let (code, _, err) = s.dev(&s.root(), &[], &[]);
    assert_eq!(code, 2);
    assert!(
        err.contains("tools/dev-launch is not in this checkout") && err.contains("git sparse-checkout add tools"),
        "{err}"
    );
}

#[test]
fn a_failed_launcher_build_is_one_error_line_with_the_next_step() {
    let s = setup(true, GOOD);
    write_script(
        &s.dir.path().join("shims/cargo"),
        "[ \"$1\" = --version ] && exit 0\necho 'error: boom' >&2\nexit 101",
    );
    let (code, _, err) = s.dev(&s.root(), &[], &[]);
    assert_eq!(code, 1);
    assert!(
        err.contains("::error::./dev could not build its launcher") && err.contains("Rust 1.89 or newer") && err.contains("error: boom"),
        "{err}"
    );
}

#[test]
fn without_rustup_and_without_cargo_it_is_one_error_line_with_the_next_step() {
    let s = setup(false, &[]);
    let shims = s.dir.path().join("shims");
    std::fs::remove_file(shims.join("rustup")).unwrap();
    let mut cmd = TestCommand::new("sh");
    // Only the stand-in cargo, git's and the system folders: no rustup.
    cmd.arg(s.root().join("dev"))
        .current_dir(s.root())
        .env("PATH", format!("{}:/usr/bin:/bin", shims.display()))
        .env_remove("RUSTUP_TOOLCHAIN");
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("::error::./dev: no Rust toolchain runs here and rustup is not installed"),
        "{err}"
    );
}
