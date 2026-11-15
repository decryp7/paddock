//! `./dev`, stage 0 (shell-to-rust WP-1): the script that builds the launcher with cargo when its sources changed and runs it. A
//! stand-in `cargo`, `rustc` and `rustup` "build" a script and keep a list of installed toolchains, so these tests need no compiler;
//! the launcher itself is tested in `tools/dev-launch/tests` and `wrapper.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// windows-twin: none: owed to the shared compiled stand-in builder of train 14 (these stand-ins are sh scripts); until then the Windows run of dev.cmd on the test VM is recorded in the ADR 0022 amendment of 2026-11-15, and dev-launch's own tests run on Windows in ci.yml ("Launcher")
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
    std::fs::write(
        root.join("rust-toolchain.toml"),
        "# the pin\n[toolchain]\nchannel = \"1.100.0\"\nprofile = \"minimal\"\ncomponents = [\"rustfmt\", \"clippy\"]\n",
    )
    .unwrap();
    std::fs::write(root.join("tools/dev-launch/Cargo.toml"), "# placeholder\n").unwrap();
    std::fs::write(root.join("tools/dev-launch/Cargo.lock"), "# placeholder\n").unwrap();
    std::fs::write(root.join("tools/dev-launch/src/main.rs"), "fn main() {}\n").unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "fixture"]);
    let shims = dir.path().join("shims");
    let (log, marker, list) = (dir.path().join("cargo.log"), shims.join("installed"), shims.join("list"));
    // rustup 1.28.1 and later install a missing pin from any proxy that runs below rust-toolchain.toml, unless RUSTUP_AUTO_INSTALL=0:
    // while `shims/pin-missing` exists, every stand-in that runs without it "contacts the dist server" (dist.log).
    let dist = format!(
        "[ -e '{}' ] && [ \"${{RUSTUP_AUTO_INSTALL:-}}\" != 0 ] && echo \"dist: $0 $*\" >> '{}'\n",
        shims.join("pin-missing").display(),
        dir.path().join("dist.log").display()
    );
    std::fs::create_dir_all(&shims).unwrap();
    std::fs::write(&list, toolchains.iter().map(|t| format!("{t}\n")).collect::<String>()).unwrap();
    write_script(
        &shims.join("cargo"),
        &format!(
            r#"{dist}if [ "$1" = --version ]; then {run}; fi
echo "build rustup_toolchain=[${{RUSTUP_TOOLCHAIN:-}}] build_target=[${{CARGO_BUILD_TARGET:-}}] $*" >> '{log}'
while [ $# -gt 0 ]; do [ "$1" = --target-dir ] && t=$2; shift; done
/bin/mkdir -p "$t/release"
printf '#!/bin/sh\necho "launcher ran: $*; toolchain=[${{RUSTUP_TOOLCHAIN:-}}] git_dir=[${{GIT_DIR:-}}]"\n[ "$1" = fail3 ] && exit 3\nexit 0\n' > "$t/release/dev-launch"
/bin/chmod +x "$t/release/dev-launch""#,
            run = if cargo_runs {
                "echo cargo 1.100.0; exit 0".to_string()
            } else {
                format!(
                    "[ -e '{}' ] && {{ echo cargo 1.100.0; exit 0; }}; echo 'error: no default toolchain' >&2; exit 1",
                    marker.display()
                )
            },
            log = log.display(),
            dist = dist
        ),
    );
    write_script(
        &shims.join("rustc"),
        &format!(
            "{dist}{}",
            r#"case "${RUSTUP_TOOLCHAIN:-}" in no|none*|'') exit 1 ;; 1.80*) echo "rustc 1.80.0 (abc 2024-01-01)" ;; *) echo "rustc 1.100.0 (abc 2026-01-01)" ;; esac"#
        ),
    );
    write_script(
        &shims.join("rustup"),
        &format!(
            r#"{dist}case "$1 $2" in
  'toolchain list') if [ -s '{list}' ]; then cat '{list}'; else echo 'no installed toolchains'; fi ;;
  'toolchain install') echo "install in $PWD: $*" >> '{inst}'; sleep 2; echo '1.100.0-test (default)' >> '{list}'; touch '{marker}' ;;
esac"#,
            list = list.display(),
            inst = dir.path().join("installs.log").display(),
            marker = marker.display(),
            dist = dist
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
    assert_eq!(
        s.bins().iter().filter(|p| !p.to_string_lossy().ends_with(".at")).count(),
        1,
        "one published launcher: {:?}",
        s.bins()
    );
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
    let bin = s.bins().into_iter().find(|p| !p.to_string_lossy().ends_with(".at")).unwrap();
    let before = std::fs::metadata(&bin).unwrap().ino();
    std::fs::remove_file(s.root().join("tools/dev-launch/target/release/dev-launch")).unwrap();
    assert_eq!(s.dev(&s.root(), &[], &[]).0, 0);
    assert_eq!(s.builds(), 1);
    assert_eq!(std::fs::metadata(&bin).unwrap().ino(), before);
}

#[test]
fn a_changed_source_builds_again_committed_or_not() {
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
        err.contains("no installed Rust toolchain is 1.89 or newer") && err.contains("rustup toolchain install 1.100.0"),
        "{err}"
    );
    assert_eq!((s.installs(), s.builds()), (0, 0));
}

/// Stage 0 installs nothing, as xtask and cargo never do: a machine with rustup and no toolchain at all gets one `::error::` line with
/// the exact command (from rust-toolchain.toml) and exit 2, and a CI job is marked so no later step tries again.
#[test]
fn a_machine_with_rustup_and_no_toolchain_at_all_gets_the_install_command_and_nothing_is_installed() {
    let s = setup(false, &[]);
    let env_file = s.dir.path().join("github-env");
    let job = [
        ("GITHUB_ACTIONS", "true".to_string()),
        ("GITHUB_ENV", env_file.display().to_string()),
    ];
    let (code, _, err) = s.dev(&s.root(), &["hello"], &job);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("::error::./dev: rustup has no toolchain installed")
            && err.contains("rustup toolchain install 1.100.0 --profile minimal --component rustfmt --component clippy"),
        "{err}"
    );
    assert_eq!((s.installs(), s.builds()), (0, 0), "nothing installed, nothing built");
    assert_eq!(std::fs::read_to_string(&env_file).unwrap(), "PADDOCK_TOOLCHAIN_FAILED=1\n");
    assert!(
        !s.dir.path().join("rustup").exists() || std::fs::read_dir(s.dir.path().join("rustup")).unwrap().count() == 0,
        "no lock folder"
    );
    // A person's run writes nothing, whatever GITHUB_ENV holds.
    let other = s.dir.path().join("other-env");
    let (code, _, _) = s.dev(&s.root(), &[], &[("GITHUB_ENV", other.display().to_string())]);
    assert_eq!(code, 2);
    assert!(!other.exists());
}

/// rustup 1.28.1 and later install a missing pin from any proxy run below rust-toolchain.toml: every probe of stage 0 says it may not.
#[test]
fn a_probe_with_a_missing_pin_never_contacts_the_dist_server() {
    for toolchains in [&["1.95.0-test"][..], &[][..], &["1.80.0-test (default)"][..]] {
        let s = setup(false, toolchains);
        std::fs::write(s.dir.path().join("shims/pin-missing"), "").unwrap();
        let _ = s.dev(&s.root(), &["hello"], &[]);
        let dist = std::fs::read_to_string(s.dir.path().join("dist.log")).unwrap_or_default();
        assert_eq!(
            dist, "",
            "toolchains {toolchains:?}: a probe or the launcher build started an install"
        );
    }
}

/// The job-setup mode is the one place a failed setup step marks the job (PADDOCK_JOB_ABORTED=1, which the later `!cancelled()` steps
/// test, conformance C117) and says so once; it covers a launcher that cannot be built, which no action's own script could.
#[test]
fn job_setup_marks_the_job_and_says_so_once_when_the_step_or_the_tool_build_fails() {
    let s = setup(true, GOOD);
    let env_file = s.dir.path().join("github-env");
    let job = [("GITHUB_ENV", env_file.display().to_string())];
    // A good step: exit 0 and nothing written.
    let (code, out, err) = s.dev(&s.root(), &["--job-setup", "hello"], &job);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("launcher ran: hello"), "{out}");
    assert!(!env_file.exists(), "a good step marks nothing");
    // A step that fails keeps its own exit code.
    let (code, _, err) = s.dev(&s.root(), &["--job-setup", "fail3"], &job);
    assert_eq!(code, 3, "{err}");
    assert_eq!(std::fs::read_to_string(&env_file).unwrap(), "PADDOCK_JOB_ABORTED=1\n");
    assert_eq!(err.matches("A job setup step failed").count(), 1, "one error line: {err}");
    assert!(
        err.contains("exit 3") && err.contains("./dev fail3"),
        "it names the step and the exit: {err}"
    );
    // A launcher that cannot be built (another stamp, so it builds).
    std::fs::remove_file(&env_file).unwrap();
    write_script(
        &s.dir.path().join("shims/cargo"),
        "[ \"$1\" = --version ] && exit 0\necho 'error: no build' >&2\nexit 101",
    );
    std::fs::write(s.root().join("tools/dev-launch/src/main.rs"), "fn main() { 2; }\n").unwrap();
    let (code, _, err) = s.dev(&s.root(), &["--job-setup", "ci", "build-jobs"], &job);
    assert_ne!(code, 0, "{err}");
    assert_eq!(std::fs::read_to_string(&env_file).unwrap(), "PADDOCK_JOB_ABORTED=1\n", "{err}");
    assert_eq!(err.matches("A job setup step failed").count(), 1, "{err}");
    // Without GITHUB_ENV it still fails and says so.
    let (code, _, err) = s.dev(&s.root(), &["--job-setup", "ci", "build-jobs"], &[]);
    assert_ne!(code, 0);
    assert_eq!(err.matches("A job setup step failed").count(), 1, "{err}");
}

/// Untracked launcher sources are part of the stamp (a new module, or an edit of one, must not run the old launcher), and a
/// launcher of another stamp that is less than a minute old is never removed by this one.
#[test]
fn untracked_files_count_in_the_stamp_and_a_young_launcher_of_another_stamp_is_kept() {
    let s = setup(true, GOOD);
    assert_eq!(s.dev(&s.root(), &[], &[]).0, 0);
    std::fs::write(s.root().join("tools/dev-launch/src/new.rs"), "// one\n").unwrap();
    assert_eq!(s.dev(&s.root(), &[], &[]).0, 0);
    assert_eq!(s.builds(), 2, "a new untracked file is a new stamp");
    std::fs::write(s.root().join("tools/dev-launch/src/new.rs"), "// two\n").unwrap();
    assert_eq!(s.dev(&s.root(), &[], &[]).0, 0);
    assert_eq!(s.builds(), 3, "an edit of an untracked file is a new stamp");
    // The first stamp's launcher was published seconds ago: it is still there, with the second's (the third is current).
    let bins: Vec<_> = s.bins().iter().filter(|p| !p.to_string_lossy().ends_with(".at")).cloned().collect();
    assert_eq!(bins.len(), 3, "young launchers of other stamps stay: {bins:?}");
    // Make them old: their publish time is in the .at file.
    for p in s.bins() {
        if p.to_string_lossy().ends_with(".at") {
            std::fs::write(&p, "1000\n").unwrap();
        }
    }
    std::fs::write(s.root().join("tools/dev-launch/src/new.rs"), "// three\n").unwrap();
    assert_eq!(s.dev(&s.root(), &[], &[]).0, 0);
    let bins: Vec<_> = s.bins().iter().filter(|p| !p.to_string_lossy().ends_with(".at")).cloned().collect();
    assert_eq!(bins.len(), 1, "old ones go, the new one stays: {bins:?}");
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

/// setup-rust's toolchain step starts with `./dev --job-setup --require-rustup`: no repository, compiler or launcher is needed, a runner
/// without rustup marks the job, and one with it passes.
#[test]
fn a_missing_rustup_marks_the_job_through_job_setup_and_one_with_rustup_passes() {
    let s = setup(true, GOOD);
    let outside = tempfile::tempdir().unwrap();
    let env_file = s.dir.path().join("github-env");
    let empty = s.dir.path().join("empty-bin");
    std::fs::create_dir_all(&empty).unwrap();
    // Only `sh` (what stage 0 starts itself with): no rustup, no cargo.
    std::os::unix::fs::symlink("/bin/sh", empty.join("sh")).unwrap();
    let mut cmd = TestCommand::new("sh");
    cmd.arg(s.root().join("dev"))
        .args(["--job-setup", "--require-rustup"])
        .current_dir(outside.path())
        .env("PATH", &empty)
        .env("GITHUB_ENV", &env_file);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("rustup is not installed"), "{err}");
    assert_eq!(err.matches("A job setup step failed").count(), 1, "{err}");
    assert_eq!(std::fs::read_to_string(&env_file).unwrap(), "PADDOCK_JOB_ABORTED=1\n");
    std::fs::remove_file(&env_file).unwrap();
    let (code, _, err) = s.dev(
        outside.path(),
        &["--job-setup", "--require-rustup"],
        &[("GITHUB_ENV", env_file.display().to_string())],
    );
    assert_eq!(code, 0, "{err}");
    assert!(!env_file.exists());
}
