//! `./dev`, stage 0 (shell-to-rust WP-1): the script that builds the launcher with cargo when a source is newer than the binary and
//! runs it. A stand-in `cargo` "builds" a script, so these tests need no compiler; the launcher itself is tested in
//! `tools/dev-launch/tests` and `wrapper.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// windows-twin: none: the cargo and rustup stand-ins are sh scripts; Windows runs dev-launch's own tests (tools/dev-launch/tests/launcher.rs) on every run, and dev.cmd in the test VM
#![cfg(unix)]
use super::wrapper::write_script;
use paddock_testkit::TestCommand;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    crate::full_tree(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap())
}

struct Stage0 {
    dir: tempfile::TempDir,
    log: PathBuf,
}

/// A repository holding the real `dev` and a placeholder launcher crate, a stand-in `cargo` that logs each call and writes a
/// launcher script where `--target-dir` says, and a `rustup` that lists one installed toolchain.
fn setup(cargo_runs: bool) -> Stage0 {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir_all(root.join("tools/dev-launch/src")).unwrap();
    std::fs::create_dir_all(root.join("tools/paddock-dev")).unwrap();
    std::fs::copy(repo_root().join("dev"), root.join("dev")).unwrap();
    std::fs::write(root.join("tools/dev-launch/Cargo.toml"), "# placeholder\n").unwrap();
    std::fs::write(root.join("tools/dev-launch/Cargo.lock"), "# placeholder\n").unwrap();
    std::fs::write(root.join("tools/dev-launch/src/main.rs"), "fn main() {}\n").unwrap();
    let st = TestCommand::new("git").args(["init", "-q"]).current_dir(&root).status().unwrap();
    assert!(st.success());
    let shims = dir.path().join("shims");
    let log = dir.path().join("cargo.log");
    let version = if cargo_runs {
        "echo cargo 1.100.0; exit 0"
    } else {
        "echo 'error: no default toolchain' >&2; exit 1"
    };
    write_script(
        &shims.join("cargo"),
        &format!(
            r#"[ "$1" = --version ] && {{ {version}; }}
echo "build rustup_toolchain=[${{RUSTUP_TOOLCHAIN:-}}] build_target=[${{CARGO_BUILD_TARGET:-}}] $*" >> '{log}'
while [ $# -gt 0 ]; do [ "$1" = --target-dir ] && t=$2; shift; done
/bin/mkdir -p "$t/release"
printf '#!/bin/sh\necho "launcher ran: $*; toolchain=[${{RUSTUP_TOOLCHAIN:-}}] git_dir=[${{GIT_DIR:-}}]"\n' > "$t/release/dev-launch"
/bin/chmod +x "$t/release/dev-launch""#,
            log = log.display()
        ),
    );
    write_script(
        &shims.join("rustup"),
        "[ \"$1 $2\" = 'toolchain list' ] && printf '1.80.0-aarch64-apple-darwin (default)\\nstable-aarch64-apple-darwin\\n'",
    );
    // Sources older than the binary a build leaves.
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
    for f in ["Cargo.toml", "Cargo.lock", "src/main.rs"] {
        std::fs::File::options()
            .write(true)
            .open(root.join("tools/dev-launch").join(f))
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
    Stage0 { dir, log }
}

impl Stage0 {
    fn root(&self) -> PathBuf {
        self.dir.path().join("repo")
    }

    fn builds(&self) -> usize {
        std::fs::read_to_string(&self.log).map_or(0, |t| t.lines().filter(|l| l.starts_with("build ")).count())
    }

    fn dev(&self, cwd: &Path, args: &[&str], env: &[(&str, String)]) -> (i32, String, String) {
        let mut cmd = TestCommand::new("sh");
        cmd.arg(self.root().join("dev"))
            .args(args)
            .current_dir(cwd)
            .path_first(&self.dir.path().join("shims"))
            // The test run's own toolchain choice is not part of what is tested.
            .env_remove("RUSTUP_TOOLCHAIN")
            .envs(env.iter().map(|(k, v)| (*k, v.as_str())));
        let out = cmd.output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

#[test]
fn the_first_run_builds_the_launcher_and_passes_every_argument_on_later_runs_do_not_build() {
    let s = setup(true);
    let (code, out, err) = s.dev(&s.root(), &["--wrapper-hash", "two words", "x"], &[]);
    assert_eq!(
        (code, out.trim()),
        (0, "launcher ran: --wrapper-hash two words x; toolchain=[] git_dir=[]"),
        "{err}"
    );
    assert_eq!(s.builds(), 1);
    let log = std::fs::read_to_string(&s.log).unwrap();
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
    let (code, _, err) = s.dev(&s.root(), &["again"], &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        s.builds(),
        1,
        "an up-to-date launcher is not built again: cargo is not even started"
    );
}

#[test]
fn a_source_newer_than_the_binary_builds_it_again() {
    let s = setup(true);
    assert_eq!(s.dev(&s.root(), &[], &[]).0, 0);
    let newer = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(s.root().join("tools/dev-launch/src/main.rs"))
        .unwrap()
        .set_modified(newer)
        .unwrap();
    assert_eq!(s.dev(&s.root(), &[], &[]).0, 0);
    assert_eq!(s.builds(), 2);
}

#[test]
fn a_cargo_that_cannot_run_yet_builds_with_the_first_installed_toolchain_and_the_launcher_does_not_inherit_it() {
    let s = setup(false);
    let (code, out, err) = s.dev(&s.root(), &["hello"], &[]);
    assert_eq!(code, 0, "{err}");
    assert!(
        std::fs::read_to_string(&s.log)
            .unwrap()
            .contains("rustup_toolchain=[1.80.0-aarch64-apple-darwin]")
    );
    assert!(
        out.contains("toolchain=[]"),
        "the launcher must see the repository's pin, not the stand-in toolchain: {out}"
    );
}

#[test]
fn a_cargo_build_target_never_moves_the_launcher_and_a_git_hooks_variables_never_move_the_repository() {
    let s = setup(true);
    let other = s.dir.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    assert!(
        TestCommand::new("git")
            .args(["init", "-q"])
            .current_dir(&other)
            .status()
            .unwrap()
            .success()
    );
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
    assert!(std::fs::read_to_string(&s.log).unwrap().contains("build_target=[]"));
    assert!(out.contains("git_dir=[]"), "the launcher inherits neither: {out}");
}

#[test]
fn outside_a_repository_and_in_a_checkout_without_the_launcher_it_says_what_to_do_and_exits_2() {
    let s = setup(true);
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
    let s = setup(true);
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
fn a_machine_with_rustup_and_no_toolchain_installs_the_pin_before_building_the_launcher() {
    let s = setup(false);
    let shims = s.dir.path().join("shims");
    let marker = s.dir.path().join("installed");
    // cargo runs once the install has run; rustup lists no toolchain and records where it installs from.
    let cargo = std::fs::read_to_string(shims.join("cargo")).unwrap().replace(
        "echo 'error: no default toolchain' >&2; exit 1",
        &format!("[ -e '{}' ] && {{ echo cargo 1.100.0; exit 0; }}; exit 1", marker.display()),
    );
    std::fs::write(shims.join("cargo"), cargo).unwrap();
    write_script(
        &shims.join("rustup"),
        &format!(
            "[ \"$1 $2\" = 'toolchain install' ] && {{ echo \"install in $PWD\" >> '{}.log'; touch '{}'; }}",
            marker.display(),
            marker.display()
        ),
    );
    let (code, out, err) = s.dev(&s.root(), &["hello"], &[]);
    assert_eq!(code, 0, "{err}");
    let log = std::fs::read_to_string(format!("{}.log", marker.display())).unwrap();
    assert!(
        log.trim_end().ends_with("tools/paddock-dev"),
        "rustup reads the pin from the tool's folder: {log}"
    );
    assert!(
        std::fs::read_to_string(&s.log).unwrap().contains("rustup_toolchain=[]"),
        "built with the pin, not a listed toolchain"
    );
    assert!(out.contains("launcher ran: hello"), "{out}");
}

#[test]
fn without_rustup_and_without_cargo_it_is_one_error_line_with_the_next_step() {
    let s = setup(false);
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
