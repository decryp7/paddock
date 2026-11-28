//! The `./dev` wrapper and `dev self hash` compute one value, in shell and in Rust, from the same inputs.
//!
//! Every child these tests start has CARGO_TARGET_DIR and CARGO_BUILD_TARGET_DIR removed (ER-47, a named clearing): the wrapper warns about
//! them and the toolchain checks fail on that warning, so a test's result would depend on the build folder the agent running it exports.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::input_hash::{Real, compute, is_hashed_var};
use paddock_testkit::TestCommand;
#[cfg(unix)]
use paddock_testkit::{Actions, Case, StandIn};
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    crate::full_tree(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap())
}

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

/// A repository with the real wrapper and input list, and a placeholder for every listed path.
pub(crate) struct Fixture {
    dir: tempfile::TempDir,
    repo: PathBuf,
    pub(crate) cargo_home: tempfile::TempDir,
}

impl Fixture {
    pub(crate) fn new() -> Fixture {
        // The repository sits one folder below the scratch folder, so a test can put a config "above the repository".
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let root = repo.as_path();
        std::fs::create_dir_all(root.join("tools/paddock-dev/src")).unwrap();
        std::fs::copy(repo_root().join("dev"), root.join("dev")).unwrap();
        let inputs = std::fs::read_to_string(repo_root().join("tools/paddock-dev/inputs.txt")).unwrap();
        std::fs::write(root.join("tools/paddock-dev/inputs.txt"), &inputs).unwrap();
        std::fs::write(root.join("tools/paddock-dev/src/main.rs"), "fn main() {}\n").unwrap();
        for p in paddock_dev::input_hash::parse_inputs(&inputs).unwrap() {
            if p.ends_with(".toml") {
                let f = root.join(&p);
                std::fs::create_dir_all(f.parent().unwrap()).unwrap();
                std::fs::write(f, "# placeholder\n").unwrap();
            } else {
                std::fs::create_dir_all(root.join(&p)).unwrap();
                std::fs::write(root.join(&p).join("keep"), "x\n").unwrap();
            }
        }
        std::fs::write(root.join(".gitignore"), "/tools/paddock-dev/target/\n").unwrap();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "chore: fixture"]);
        Fixture {
            dir,
            repo: repo.clone(),
            cargo_home: tempfile::tempdir().unwrap(),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.repo
    }

    /// The environment both implementations see (no user config: an empty CARGO_HOME).
    fn env(&self, extra: &[(&str, String)]) -> Vec<(String, String)> {
        let mut v = vec![("CARGO_HOME".to_string(), self.cargo_home.path().display().to_string())];
        v.extend(extra.iter().map(|(k, val)| (k.to_string(), val.clone())));
        v
    }

    fn shell(&self, env: &[(String, String)]) -> (String, bool) {
        let mut cmd = TestCommand::new("sh");
        cmd.arg(self.root().join("dev"))
            .arg("--wrapper-hash")
            .current_dir(self.root().join("tools/paddock-dev/src"))
            .env_remove("RUSTC")
            .env_remove("CARGO_BUILD_RUSTC")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR");
        // The hash reads the environment: start from none of the variables it uses, as `rust` does.
        paddock_dev::process_env::vars()
            .filter(|(k, _)| is_hashed_var(k))
            .for_each(|(k, _)| {
                cmd.env_remove(k);
            });
        cmd.envs(env.iter().cloned());
        // Tests never touch the user's cache: the wrapper's home is a scratch folder too.
        cmd.env("PADDOCK_DEV_HOME", self.cargo_home.path().join("dev-home"));
        let out = cmd.output().unwrap();
        assert!(
            !self.cargo_home.path().join("dev-home").exists(),
            "--wrapper-hash must not create the cache"
        );
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let mut lines = text.lines();
        (lines.next().unwrap().to_string(), lines.next() == Some("dirty"))
    }

    fn rust(&self, env: &[(String, String)]) -> (String, bool) {
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let h = compute(&Real, &get, env, self.root()).unwrap();
        (h.hash, h.dirty)
    }

    pub(crate) fn both(&self, extra: &[(&str, String)]) -> (String, bool) {
        let env = self.env(extra);
        let (s, r) = (self.shell(&env), self.rust(&env));
        assert_eq!(s, r, "the wrapper and the tool must agree");
        s
    }
}

/// A "compiler" that prints a chosen `-vV`: one self-contained file, so a test may copy it.
#[cfg(unix)]
pub(crate) fn fake_rustc(dir: &Path, name: &str, release: &str, commit: &str, host: &str) -> String {
    let p = dir.join(name);
    StandIn::new(dir, name)
        .stdout(&format!("rustc {release}"))
        .stdout("binary: rustc")
        .stdout(&format!("commit-hash: {commit}"))
        .stdout("commit-date: d")
        .stdout(&format!("host: {host}"))
        .stdout(&format!("release: {release}"))
        .stdout("LLVM version: 1")
        .write_single_file(&p)
        .unwrap();
    p.display().to_string()
}

#[test]
fn shell_and_rust_agree_and_the_hash_is_stable_between_runs() {
    let f = Fixture::new();
    let (a, dirty) = f.both(&[]);
    assert!(!dirty && a.len() >= 40);
    assert_eq!(f.both(&[]).0, a);
}

#[test]
fn a_changed_input_in_a_commit_changes_the_hash() {
    let f = Fixture::new();
    let before = f.both(&[]).0;
    std::fs::write(f.root().join("tools/paddock-dev/src/main.rs"), "fn main() { }\n").unwrap();
    git(f.root(), &["commit", "-qam", "fix: change"]);
    let after = f.both(&[]);
    assert!(!after.1 && after.0 != before);
}

#[cfg(unix)]
#[test]
fn the_compiler_release_and_commit_hash_count_and_the_host_does_not() {
    let f = Fixture::new();
    let d = tempfile::tempdir().unwrap();
    let a = fake_rustc(d.path(), "a", "1.90.0", "aaaa", "aarch64-apple-darwin");
    let same_other_host = fake_rustc(d.path(), "b", "1.90.0", "aaaa", "x86_64-pc-windows-msvc");
    let newer = fake_rustc(d.path(), "c", "1.91.0", "aaaa", "aarch64-apple-darwin");
    let other_commit = fake_rustc(d.path(), "d", "1.90.0", "bbbb", "aarch64-apple-darwin");
    let h = |p: &str| f.both(&[("RUSTC", p.to_string())]).0;
    assert_eq!(h(&a), h(&same_other_host), "the host line is not part of the hash");
    assert_ne!(h(&a), h(&newer));
    assert_ne!(h(&a), h(&other_commit));
}

#[cfg(unix)]
#[test]
fn the_compiler_is_resolved_like_cargo_does() {
    let f = Fixture::new();
    let d = tempfile::tempdir().unwrap();
    let via_env = fake_rustc(d.path(), "env", "1.80.0", "e", "h");
    let via_build_env = fake_rustc(d.path(), "benv", "1.81.0", "f", "h");
    let via_config = fake_rustc(d.path(), "cfg", "1.82.0", "g", "h");
    let default = f.both(&[]).0;
    // CARGO_BUILD_RUSTC alone.
    let by_build_env = f.both(&[("CARGO_BUILD_RUSTC", via_build_env.clone())]).0;
    assert_ne!(by_build_env, default);
    // RUSTC wins over CARGO_BUILD_RUSTC.
    let by_env = f
        .both(&[("RUSTC", via_env.clone()), ("CARGO_BUILD_RUSTC", via_build_env.clone())])
        .0;
    assert_eq!(by_env, f.both(&[("RUSTC", via_env)]).0);
    assert_ne!(by_env, by_build_env);
    // `build.rustc` from the repository's .cargo/config.toml.
    std::fs::write(f.root().join(".cargo/config.toml"), format!("[build]\nrustc = \"{via_config}\"\n")).unwrap();
    git(f.root(), &["commit", "-qam", "chore: pin a compiler in config"]);
    let by_config = f.both(&[]).0;
    assert_ne!(by_config, default);
    // The environment still wins over the config.
    assert_eq!(f.both(&[("CARGO_BUILD_RUSTC", via_build_env)]).0.len(), by_config.len());
    assert_ne!(
        f.both(&[("CARGO_BUILD_RUSTC", fake_rustc(d.path(), "again", "1.99.9", "z", "h"))])
            .0,
        by_config
    );
}

#[test]
fn a_new_untracked_file_under_an_input_path_makes_the_wrapper_build() {
    let f = Fixture::new();
    assert!(!f.both(&[]).1);
    std::fs::write(f.root().join("tools/paddock-dev/src/new_module.rs"), "// new\n").unwrap();
    assert!(f.both(&[]).1, "an untracked, non-ignored file is not in HEAD's tree");
    std::fs::remove_file(f.root().join("tools/paddock-dev/src/new_module.rs")).unwrap();
    // The local build folder is ignored and never counts as a change.
    std::fs::create_dir_all(f.root().join("tools/paddock-dev/target/release")).unwrap();
    std::fs::write(f.root().join("tools/paddock-dev/target/release/paddock-dev"), "bin").unwrap();
    assert!(!f.both(&[]).1);
    // A modified tracked file is dirty too.
    std::fs::write(f.root().join("tools/paddock-dev/src/main.rs"), "fn main() { 1; }\n").unwrap();
    assert!(f.both(&[]).1);
}

#[test]
fn the_wrapper_finds_the_root_from_a_subfolder_and_refuses_to_run_outside_a_repository() {
    let f = Fixture::new();
    let (hash, _) = f.shell(&f.env(&[]));
    assert!(!hash.is_empty(), "run from tools/paddock-dev/src");
    let outside = tempfile::tempdir().unwrap();
    let out = TestCommand::new("sh")
        .arg(f.root().join("dev"))
        .arg("--wrapper-hash")
        .current_dir(outside.path())
        .env("GIT_CEILING_DIRECTORIES", outside.path().parent().unwrap())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("inside the Paddock repository"));
}

// The flag checks are three tests, not one: each `both` runs the shell wrapper (about 35 programs, slow to start under Git bash)
// and the Rust side, about 4 s on the Windows runner, so twelve in one test took 50 s of its 60 s and timed out in runs 7725,
// 7812 and 7819; split, nextest runs them side by side.

#[test]
fn compiler_flags_are_part_of_the_hash_in_both_implementations() {
    let f = Fixture::new();
    let plain = f.both(&[]).0;
    let rustflags = f.both(&[("RUSTFLAGS", "-C target-cpu=native".to_string())]).0;
    let encoded = f.both(&[("CARGO_ENCODED_RUSTFLAGS", "-Ca\u{1f}-Cb".to_string())]).0;
    assert!(plain != rustflags && plain != encoded && rustflags != encoded);
}

#[test]
fn build_profile_and_target_variables_count_and_the_job_count_does_not() {
    let f = Fixture::new();
    let plain = f.both(&[]).0;
    for (k, v) in [
        ("CARGO_BUILD_RUSTFLAGS", "-Ca"),
        ("CARGO_PROFILE_RELEASE_OPT_LEVEL", "1"),
        ("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS", "-Cb"),
        ("CARGO_BUILD_TARGET", "wasm32-unknown-unknown"),
    ] {
        assert_ne!(f.both(&[(k, v.to_string())]).0, plain, "{k}");
    }
    assert_eq!(
        f.both(&[("CARGO_BUILD_JOBS", "3".to_string())]).0,
        plain,
        "the job count does not change the binary"
    );
}

#[test]
fn every_cargo_config_the_tool_reads_is_part_of_the_hash_in_both_implementations() {
    let f = Fixture::new();
    let plain = f.both(&[]).0;
    // A `rustflags` line in a committed config (a target table) and in $CARGO_HOME's config.
    std::fs::write(
        f.root().join(".cargo/config.toml"),
        "[target.x86_64-pc-windows-msvc]\nrustflags = [\"-C\", \"target-feature=+crt-static\"]\n",
    )
    .unwrap();
    git(f.root(), &["commit", "-qam", "chore: static crt"]);
    let committed = f.both(&[]).0;
    assert_ne!(committed, plain);
    std::fs::write(
        f.cargo_home.path().join("config.toml"),
        "[build]\nrustflags = [\"-D\", \"warnings\"]\n",
    )
    .unwrap();
    let with_home = f.both(&[]).0;
    assert_ne!(with_home, committed);
    // Any change to a config counts, whatever the table: a profile or a linker in $CARGO_HOME's config.
    std::fs::write(
        f.cargo_home.path().join("config.toml"),
        "[profile.release]\nlto = true\n[target.x86_64-unknown-linux-gnu]\nlinker = \"clang\"\n",
    )
    .unwrap();
    let profile = f.both(&[]).0;
    assert_ne!(profile, committed);
    assert_ne!(profile, with_home);
    // The git-ignored include next to the tool counts too.
    std::fs::remove_file(f.cargo_home.path().join("config.toml")).unwrap();
    std::fs::create_dir_all(f.root().join("tools/paddock-dev/.cargo")).unwrap();
    std::fs::write(
        f.root().join("tools/paddock-dev/.cargo/ci.toml"),
        "[build]\nrustflags = [\"-Dx\"]\n",
    )
    .unwrap();
    let included = f.both(&[]).0;
    assert_ne!(included, committed);
    // A config above the repository counts as well (cargo reads every ancestor's `.cargo`).
    let above = f.dir.path().join(".cargo");
    std::fs::create_dir_all(&above).unwrap();
    std::fs::write(above.join("config.toml"), "[build]\nrustflags = [\"-Dy\"]\n").unwrap();
    let with_above = f.both(&[]).0;
    std::fs::remove_dir_all(&above).unwrap();
    assert_ne!(with_above, included);
}

/// CI's build-folder action writes `tools/paddock-dev/.cargo/ci.toml` (only `build.target-dir`, the job slot's folder) after
/// job-isolation took the slot with `./dev`; the release step's `./dev` then saw another hash and rebuilt the tool after the
/// job's temp folder was gone (runs 7774, 7810 and 7819, macOS). The build folder never changes the binary, as
/// `CARGO_TARGET_DIR` does not: a config that names nothing else is not hashed, whatever folder it names.
#[test]
fn a_config_that_only_names_the_build_folder_does_not_change_the_hash() {
    let f = Fixture::new();
    let ci = f.root().join("tools/paddock-dev/.cargo/ci.toml");
    std::fs::create_dir_all(ci.parent().unwrap()).unwrap();
    std::fs::write(
        f.root().join(".gitignore"),
        "/tools/paddock-dev/target/\n/tools/paddock-dev/.cargo/ci.toml\n",
    )
    .unwrap();
    git(f.root(), &["commit", "-qam", "chore: ignore ci.toml"]);
    let plain = f.both(&[]);
    assert!(!plain.1);
    for slot in ["s1", "s2"] {
        std::fs::write(
            &ci,
            format!("# Written by build-folder.sh for this CI job (git-ignored).\n[build]\r\ntarget-dir = 'C:/ci/target-paddock-{slot}'\n"),
        )
        .unwrap();
        assert_eq!(f.both(&[]), plain, "a build folder ({slot}) is not part of the hash");
        // The same file with the remap of that folder (user decision 142), and a container's remap alone: the folder is still the
        // only thing it names, so a slot's own folder in the flag never changes the hash (both implementations, as the wrapper's awk).
        std::fs::write(
            &ci,
            format!(
                "# Written by `./dev ci build-folder` for this CI job (git-ignored).\n[build]\ntarget-dir = 'C:/ci/target-paddock-{slot}'\n\
                 [target.'cfg(all())']\nrustflags = ['--remap-path-prefix=C:/ci/target-paddock-{slot}=/paddock-target']\n"
            ),
        )
        .unwrap();
        assert_eq!(f.both(&[]), plain, "the remap of the build folder ({slot}) is not part of the hash");
        std::fs::write(
            &ci,
            format!("[target.'cfg(all())']\r\nrustflags = ['--remap-path-prefix=/work/ws-{slot}/target=/paddock-target']\r\n"),
        )
        .unwrap();
        assert_eq!(f.both(&[]), plain, "nor a container's remap alone ({slot})");
    }
    std::fs::write(f.cargo_home.path().join("config.toml"), "build.target-dir = \"/elsewhere\"\n").unwrap();
    assert_eq!(f.both(&[]), plain, "nor as a dotted key in $CARGO_HOME's config");
    std::fs::remove_file(f.cargo_home.path().join("config.toml")).unwrap();
    // Anything else next to the build folder counts, and so does a `target-dir` outside `[build]` (not cargo's key).
    for other in [
        "[build]\ntarget-dir = 'x'\nrustflags = [\"-Dx\"]\n",
        "[build]\ntarget-dir = 'x'\n[profile.release]\nlto = true\n",
        "[profile.release]\ntarget-dir = 'x'\n",
        "build = { target-dir = 'x' }\n",
        // Only the remap of the build folder to the one fixed name is skipped: another flag, another target or another table counts.
        "[target.'cfg(all())']\nrustflags = ['-Dwarnings']\n",
        "[target.'cfg(all())']\nrustflags = ['--remap-path-prefix=x=/elsewhere']\n",
        "[target.'cfg(all())']\nrustflags = ['--remap-path-prefix=x=/paddock-target', '-Dwarnings']\n",
        "[target.'cfg(unix)']\nrustflags = ['--remap-path-prefix=x=/paddock-target']\n",
        "[target.'cfg(all())']\nlinker = 'x'\n",
    ] {
        std::fs::write(&ci, other).unwrap();
        assert_ne!(f.both(&[]).0, plain.0, "{other:?} is hashed");
    }
}

/// A stand-in program at `path` (its folder is made); install it when its answers are set.
#[cfg(unix)]
pub(crate) fn stand_in_at(path: &Path) -> StandIn {
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir).unwrap();
    StandIn::new(dir, path.file_name().unwrap().to_str().unwrap())
}

/// Writes `tool` as one self-contained file at `path` (its folder is made), the way a built or downloaded tool is a single file.
#[cfg(unix)]
pub(crate) fn write_tool(path: &Path, tool: &StandIn) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    tool.write_single_file(path).unwrap();
}

/// A tool that does nothing and succeeds (a cached or old entry the wrapper only lists, prunes or probes).
#[cfg(unix)]
pub(crate) fn idle_tool() -> StandIn {
    StandIn::new(Path::new(""), "idle")
}

/// A stand-in `cargo` in `shims`, answering as `build` says. With `folder_rule` it takes its build folder from
/// `--config build.target-dir='...'` and fails when `CARGO_TARGET_DIR` reaches it: sccache hashes every `CARGO_*` variable, so the
/// variable defeats the compile cache across worktrees (lessons.md). `conformance/ci/dev-tool-publish.sh` builds one folder per
/// tool, so its test turns the rule off.
#[cfg(unix)]
pub(crate) fn install_cargo(shims: &Path, folder_rule: bool, build: impl FnOnce(StandIn) -> StandIn) {
    std::fs::create_dir_all(shims).unwrap();
    let mut cargo = build(StandIn::new(shims, "cargo"));
    if folder_rule {
        cargo = cargo.when(
            &Case::new().env_is_set("CARGO_TARGET_DIR"),
            Actions::new()
                .stderr("CARGO_TARGET_DIR reached cargo: pass --config build.target-dir")
                .exit(97),
        );
    }
    cargo.install().unwrap();
}

/// The build folder a stand-in `cargo` was given: `--config build.target-dir='...'`, without the quotes.
#[cfg(unix)]
const BUILD_FOLDER: &str = "{flag:build.target-dir='|'}";

/// The tool a stand-in `cargo` "builds": answers `--version`, `self` (the hash it was built for, as the real tool shows) and
/// otherwise prints `says` (a template the tool fills when it runs: write `{{env:NAME}}` for a variable).
#[cfg(unix)]
pub(crate) fn built_tool(says: &str, hash: &str) -> StandIn {
    StandIn::new(Path::new(""), "paddock-dev")
        .answers(Actions::new().stdout_with(says))
        .when_args(&["--version"], Actions::new().stdout("0"))
        .when_args(&["self"], Actions::new().stdout(&format!("embedded {hash}")))
}

/// A call of `cargo build` that is given its build folder with `--config build.target-dir='...'` (a probe such as `cargo --version`
/// builds nothing).
#[cfg(unix)]
pub(crate) fn build_call() -> Case {
    Case::new().args_text_contains("build.target-dir=")
}

/// `cargo` answers a call that is given a build folder with `builds`, and any other call as it was set to.
#[cfg(unix)]
pub(crate) fn building(cargo: StandIn, builds: Actions) -> StandIn {
    cargo.when(&build_call(), builds)
}

/// The build folder of a stand-in `cargo` that has no folder rule (`conformance/ci/dev-tool-publish.sh` passes `CARGO_TARGET_DIR`).
#[cfg(unix)]
pub(crate) const BUILD_FOLDER_ENV: &str = "{env:CARGO_TARGET_DIR}";

/// The steps of a stand-in `cargo` that "builds" `tool` into the build folder `folder` (a template) after running `pre`.
#[cfg(unix)]
pub(crate) fn cargo_builds(folder: &str, pre: Actions, tool: &StandIn) -> Actions {
    pre.makes_dir(&format!("{folder}/release"))
        .installs(&format!("{folder}/release/paddock-dev"), tool)
}

/// A stand-in `cargo` that "builds" a tool answering `--version`, `self hash` (the hash it was built for, as the real tool shows)
/// and a marker.
#[cfg(unix)]
pub(crate) fn shim_cargo() -> Actions {
    cargo_builds(BUILD_FOLDER, Actions::new(), &built_tool("built-by-shim", "{env:PADDOCK_DEV_HASH}"))
}

/// Runs the whole wrapper (not just `--wrapper-hash`) with a scratch cache and a stand-in `cargo` that "builds" a script.
#[cfg(unix)]
fn run_wrapper(f: &Fixture, home: &Path, extra_env: &[(&str, String)]) -> (i32, String, String) {
    run_wrapper_with_cargo(f, home, extra_env, |c| building(c, shim_cargo()))
}

/// [`run_wrapper`] with a `cargo` of the test's own.
#[cfg(unix)]
fn run_wrapper_with_cargo(
    f: &Fixture,
    home: &Path,
    extra_env: &[(&str, String)],
    cargo: impl FnOnce(StandIn) -> StandIn,
) -> (i32, String, String) {
    let shims = f.cargo_home.path().join("shims");
    install_cargo(&shims, true, cargo);
    let shims_dir = shims.to_path_buf();
    let mut cmd = TestCommand::new("sh");
    cmd.arg(f.root().join("dev"))
        .arg("hello")
        .current_dir(f.root())
        .path_first(&shims_dir)
        .env("PADDOCK_DEV_HOME", home)
        .env("PADDOCK_DEV_TARGET_DIR", f.cargo_home.path().join("target"))
        .env("CARGO_HOME", f.cargo_home.path())
        // The toolchain install lock lives in RUSTUP_HOME: never the user's.
        .env("RUSTUP_HOME", f.cargo_home.path().join("rustup"))
        .env_remove("RUSTC")
        .env_remove("CARGO_BUILD_RUSTC")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        // `dev` changes its messages and defaults on a CI runner; a test opts in through `extra_env`.
        .env_remove("GITHUB_ACTIONS")
        // What decides whether a cold build goes through the compile cache (a developer's shim, a tag run's ref).
        .env_remove("RUSTC_WRAPPER")
        .env_remove("GITHUB_REF")
        .env_remove("PADDOCK_TAG_REHEARSAL")
        .env_remove("PADDOCK_DEV_SELFBUILD_CACHE");
    cmd.envs(extra_env.iter().map(|(k, v)| (*k, v.as_str())));
    let out = cmd.output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[cfg(unix)]
#[test]
fn a_cached_binary_that_does_not_run_is_replaced_and_a_good_one_is_used_as_it_is() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let env = [("RUSTC", rustc.clone())];
    let (hash, _) = f.both(&env);
    let home = scratch.path().join("home");
    let cached = home.join("bin").join(&hash).join("paddock-dev-test-host");
    // A damaged cache entry: an empty file that is executable.
    std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
    paddock_testkit::exe::write_program(&cached, "").unwrap();
    let (code, out, err) = run_wrapper(&f, &home, &env);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    assert!(err.contains("does not run or is not for hash"), "{err}");
    // The rebuilt copy is cached and answers --version, so the next run uses it without building.
    std::fs::remove_dir_all(f.cargo_home.path().join("target")).unwrap();
    let (code, out, err) = run_wrapper(&f, &home, &env);
    assert_eq!((code, out.trim(), err.as_str()), (0, "built-by-shim", ""));
    assert!(!f.cargo_home.path().join("target").exists(), "no second build");
}

/// Git Bash on Windows: the build folder reaches cargo inside `--config build.target-dir='...'`, where MSYS does not turn a
/// `/c/Users/...` path into `C:/Users/...`, so cargo built into `C:\\c\\Users\\...` and the wrapper found an old binary in the
/// folder it named (run 8208: "built a paddock-dev that is not for hash ... three times in a row", five Windows jobs). Where
/// `cygpath` exists the folder is passed in its mixed form (`cygpath -m`). A stand-in `cygpath` records the call; it answers the
/// same path, so the build still lands where the wrapper looks.
#[cfg(unix)]
#[test]
fn where_cygpath_exists_the_build_folder_reaches_cargo_in_its_windows_form() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let calls = scratch.path().join("cygpath-calls");
    let log = || Actions::new().appends_with(&calls.display().to_string(), "{args}\n");
    stand_in_at(&f.cargo_home.path().join("shims").join("cygpath"))
        .answers(log().stdout_with("{arg:1}"))
        .when_args(&["-m"], log().stdout_with("{arg:2}"))
        .install()
        .unwrap();
    let home = scratch.path().join("home");
    let (code, out, err) = run_wrapper(&f, &home, &[("RUSTC", rustc)]);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    let seen = std::fs::read_to_string(&calls).unwrap_or_default();
    let target = f.cargo_home.path().join("target");
    assert!(
        seen.lines().any(|l| l.starts_with("-m ") && l.contains(&*target.to_string_lossy())),
        "the build folder went to cargo without cygpath -m: {seen:?}"
    );
}

/// A cached tool that answers `--version` and otherwise says it ran (what an old cached binary would do).
#[cfg(unix)]
fn cached_tool(path: &Path) {
    cached_tool_for(path, "some-hash");
}

/// The same, built for `hash` (`self hash` says so, as the real tool does): the wrapper runs it only under that hash.
#[cfg(unix)]
fn cached_tool_for(path: &Path, hash: &str) {
    write_tool(
        path,
        &StandIn::new(Path::new(""), "cached")
            .answers(Actions::new().stdout_with("cached ran {args}"))
            .when_args(&["--version"], Actions::new().stdout("0"))
            .when_args(&["self"], Actions::new().stdout(&format!("embedded {hash}"))),
    );
}

/// A git hook runs its commands with `GIT_DIR` set (and a worktree's hooks may set `GIT_WORK_TREE`): the wrapper finds the repository
/// from where it is, never from what it inherited, or it hashes another repository's inputs (or none) and builds the wrong tool.
#[cfg(unix)]
#[test]
fn an_inherited_git_dir_or_work_tree_does_not_move_the_wrapper_to_another_repository() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let env = [("RUSTC", rustc.clone())];
    let (hash, _) = f.both(&env);
    let home = scratch.path().join("home");
    cached_tool_for(&home.join("bin").join(&hash).join("paddock-dev-test-host"), &hash);
    // Another repository, named by the variables a hook would have set.
    let other = scratch.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    assert!(
        TestCommand::new("git")
            .args(["init", "-q"])
            .current_dir(&other)
            .status()
            .unwrap()
            .success()
    );
    let mut hooked = env.to_vec();
    hooked.push(("GIT_DIR", other.join(".git").display().to_string()));
    hooked.push(("GIT_WORK_TREE", other.display().to_string()));
    let (code, out, err) = run_wrapper(&f, &home, &hooked);
    assert_eq!((code, out.trim()), (0, "cached ran hello"), "{err}");
}

#[cfg(unix)]
#[test]
fn without_a_compiler_no_cached_binary_is_run_and_the_error_names_the_next_step() {
    // Run 8034: an old cached tool without `ci build-jobs` ran in place of the one for this commit and exited 2. Fail closed (ER-33).
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let missing = [("RUSTC", scratch.path().join("no-such-rustc").display().to_string())];
    let (code, _, err) = run_wrapper(&f, &home, &missing);
    assert_eq!(code, 2);
    assert!(err.contains("rustup") && err.contains("rustc was not found"), "{err}");
    for root in ["bin", "bin-source"] {
        cached_tool(&home.join(root).join("some-hash/paddock-dev-test-host"));
    }
    for extra in [
        vec![],
        vec![("GITHUB_ACTIONS", "true".to_string())],
        vec![("PADDOCK_DEV_DOWNLOAD", "0".to_string())],
    ] {
        let env: Vec<_> = missing.iter().cloned().chain(extra).collect();
        let (code, out, err) = run_wrapper(&f, &home, &env);
        assert_eq!(code, 2, "{out}{err}");
        assert!(!out.contains("cached ran"), "{out}");
        assert!(
            err.contains("::error::./dev: no Rust compiler runs here") && err.contains("rustup toolchain install"),
            "{err}"
        );
    }
}

/// How the fake rustup answers a call after logging it (its folder and arguments).
#[cfg(unix)]
#[derive(Clone, Copy)]
enum Rustup {
    /// `toolchain install` makes the toolchain appear; any other call fails.
    Installs,
    /// `toolchain install` makes the toolchain appear; any other call succeeds.
    InstallsOk,
    /// As `InstallsOk`, and `--version` prints a version line.
    Reports,
    /// `toolchain install` never returns (until it is ended); any other call succeeds.
    Hangs,
    /// Every call fails.
    Fails,
}

/// What `rustc -vV` prints for the pinned toolchain.
#[cfg(unix)]
fn rustc_version_lines(a: Actions) -> Actions {
    a.stdout("rustc 1.90.0")
        .stdout("binary: rustc")
        .stdout("commit-hash: aaaa")
        .stdout("commit-date: d")
        .stdout("host: test-host")
        .stdout("release: 1.90.0")
        .stdout("LLVM version: 1")
}

/// A Windows-shaped runner: rustup is there, its `rustc` shim cannot run until the pinned toolchain is installed, no RUSTC is set.
/// Returns the folder of the shims and the log of the fake rustup.
#[cfg(unix)]
fn rustup_without_toolchain(f: &Fixture, how: Rustup) -> (PathBuf, PathBuf) {
    std::fs::write(
        f.root().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.90.0\"\nprofile = \"minimal\"\ncomponents = [\"clippy\"]\n",
    )
    .unwrap();
    let shims = f.cargo_home.path().join("shims");
    std::fs::create_dir_all(&shims).unwrap();
    let marker = f.cargo_home.path().join("installed");
    let log = f.cargo_home.path().join("rustup.log");
    StandIn::new(&shims, "rustc")
        .answers(rustc_version_lines(Actions::new()))
        .when(
            &Case::new().file_missing(&marker),
            Actions::new().stderr("error: toolchain '1.90.0' is not installed").exit(1),
        )
        .install()
        .unwrap();
    let logged = || Actions::new().appends_with(&log.display().to_string(), "{cwd}: {args}\n");
    let install = Case::new().args_start_with(&["toolchain", "install"]);
    let rustup = StandIn::new(&shims, "rustup");
    let rustup = match how {
        Rustup::Installs => rustup.answers(logged().exit(1)).when(&install, logged().writes(&marker, "")),
        Rustup::InstallsOk => rustup.answers(logged()).when(&install, logged().writes(&marker, "")),
        Rustup::Reports => rustup
            .answers(logged())
            .when(&install, logged().writes(&marker, ""))
            .when_args(&["--version"], logged().stdout("rustup 9.9.9 (test)")),
        Rustup::Hangs => rustup
            .answers(logged())
            .when(&install, logged().sleeps(std::time::Duration::from_secs(30))),
        Rustup::Fails => rustup.answers(logged().exit(1)),
    };
    rustup.install().unwrap();
    // The install is checked by running cargo and the components the file asks for (clippy here); a test may replace this one.
    install_cargo(&shims, true, |c| c.stdout("cargo 1.90.0"));
    (shims, log)
}

#[cfg(unix)]
#[test]
fn a_runner_with_rustup_but_without_the_pinned_toolchain_installs_it_before_building() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let (_, log) = rustup_without_toolchain(&f, Rustup::Installs);
    let (code, out, err) = run_wrapper(
        &f,
        &scratch.path().join("home"),
        &[("GITHUB_ACTIONS", "true".to_string()), ("PADDOCK_DEV_DOWNLOAD", "0".to_string())],
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.trim(), "built-by-shim");
    assert!(
        err.contains("installing the toolchain") && err.contains("installed the pinned toolchain in"),
        "{err}"
    );
    let log = std::fs::read_to_string(log).unwrap();
    assert!(
        log.contains("tools/paddock-dev: toolchain install"),
        "rustup ran in the tool's folder, so it reads the file: {log}"
    );
}

#[cfg(unix)]
#[test]
fn a_failed_or_stuck_toolchain_install_is_an_error_with_the_next_step_and_runs_no_cached_tool() {
    for (body, timeout, want) in [(Rustup::Fails, "30", "exit 1"), (Rustup::Hangs, "1", "exit 124")] {
        let f = Fixture::new();
        let scratch = tempfile::tempdir().unwrap();
        let home = scratch.path().join("home");
        cached_tool(&home.join("bin/some-hash/paddock-dev-test-host"));
        rustup_without_toolchain(&f, body);
        let (code, out, err) = run_wrapper(&f, &home, &[("PADDOCK_DEV_TOOLCHAIN_TIMEOUT", timeout.to_string())]);
        assert_eq!(code, 2, "{out}{err}");
        assert!(
            err.contains("::error::./dev: 'rustup toolchain install'") && err.contains(want) && err.contains("by hand"),
            "{err}"
        );
        assert!(!out.contains("cached ran"), "{out}");
    }
}

#[cfg(unix)]
#[test]
fn ensure_toolchain_succeeds_when_the_compiler_runs_after_the_install() {
    let f = Fixture::new();
    let (shims, _) = rustup_without_toolchain(&f, Rustup::Installs);
    let shims_dir = shims.to_path_buf();
    let run = || {
        TestCommand::new("sh")
            .arg(f.root().join("dev"))
            .arg("--ensure-toolchain")
            .current_dir(f.root())
            .path_first(&shims_dir)
            .env("CARGO_HOME", f.cargo_home.path())
            .env("RUSTUP_HOME", f.cargo_home.path().join("rustup"))
            .env_remove("RUSTC")
            .env_remove("CARGO_BUILD_RUSTC")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR")
            .output()
            .unwrap()
    };
    let first = run();
    assert_eq!(first.status.code(), Some(0), "{}", String::from_utf8_lossy(&first.stderr));
    // Second time the compiler runs: nothing is installed again.
    let second = run();
    assert_eq!(second.status.code(), Some(0));
    assert!(
        !String::from_utf8_lossy(&second.stderr).contains("installing"),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
}

/// A rustup whose install leaves the toolchain broken in the way `what` names, as the Windows runner's 1.100.0 after two jobs
/// installed it at once (CI run 8060: "cargo-clippy is not applicable to the toolchain") and after a repair uninstalled it under
/// running jobs (run 8063: "can't find crate for `std`"). Returns the shims and rustup's log.
#[cfg(unix)]
fn rustup_installing_broken(f: &Fixture, what: &str) -> (PathBuf, PathBuf) {
    let (shims, log) = rustup_without_toolchain(f, Rustup::InstallsOk);
    let marker = f.cargo_home.path().join("installed");
    match what {
        "clippy" => install_cargo(&shims, true, |c| {
            c.stdout("cargo 1.90.0").when_args(
                &["clippy", "--version"],
                Actions::new()
                    .stderr("error: the 'cargo-clippy' binary is not applicable to the toolchain")
                    .exit(1),
            )
        }),
        "std" => StandIn::new(&shims, "rustc")
            .answers(rustc_version_lines(Actions::new()))
            .when_arg_contains(".rs", Actions::new().stderr("error[E0463]: can't find crate for 'std'").exit(1))
            .when(
                &Case::new().file_missing(&marker),
                Actions::new().stderr("error: toolchain '1.90.0' is not installed").exit(1),
            )
            .install()
            .map(|_| ())
            .unwrap(),
        other => panic!("unknown breakage {other}"),
    }
    (shims, log)
}

#[cfg(unix)]
fn install_toolchain(f: &Fixture, shims: &Path, args: &[&str]) -> std::process::Output {
    // Run from outside any repository: the setup-rust action calls this mode before anything else is set up.
    let outside = tempfile::tempdir().unwrap();
    TestCommand::new("sh")
        .arg(f.root().join("dev"))
        .arg("--install-toolchain")
        .args(args)
        .current_dir(outside.path())
        .path_first(shims)
        .env("RUSTUP_HOME", f.cargo_home.path().join("rustup"))
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .output()
        .unwrap()
}

/// ER-26 (runs 8027, 8028, 8060, 8063): a toolchain whose install finished but whose std, cargo, clippy or rustfmt does not work is an
/// error that names the manual repair, done with no jobs running. CI never uninstalls a toolchain: other jobs on the runner may be
/// compiling with it (run 8063: a repair's uninstall took std away from four running jobs).
#[cfg(unix)]
#[test]
fn a_broken_toolchain_is_an_error_naming_the_manual_repair_and_is_never_uninstalled() {
    for (what, says) in [("clippy", "cargo clippy does not run"), ("std", "a std program does not compile")] {
        let f = Fixture::new();
        let (shims, log) = rustup_installing_broken(&f, what);
        let out = install_toolchain(&f, &shims, &["1.90.0", "clippy", ""]);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{what}: {err}");
        assert_eq!(err.matches("::error::").count(), 1, "{what}: one error line (ER-27): {err}");
        assert!(
            err.contains(says) && err.contains("with no jobs running") && err.contains("rustup toolchain uninstall 1.90.0"),
            "{what}: {err}"
        );
        let log = std::fs::read_to_string(log).unwrap();
        assert!(!log.contains("uninstall"), "{what}: CI never uninstalls: {log}");
        assert_eq!(log.matches("toolchain install").count(), 1, "{what}: {log}");
        assert!(
            !f.cargo_home.path().join("rustup/.paddock-install-1.90.0.lock").exists(),
            "{what}: the lock is released on failure too"
        );
    }
}

/// ./dev's own install (a machine whose rustc does not run) checks the toolchain the same way and never uninstalls it either.
#[cfg(unix)]
#[test]
fn ensure_toolchain_refuses_a_broken_install_without_uninstalling_it() {
    let f = Fixture::new();
    let (shims, log) = rustup_installing_broken(&f, "clippy");
    let out = TestCommand::new("sh")
        .arg(f.root().join("dev"))
        .arg("--ensure-toolchain")
        .current_dir(f.root())
        .path_first(&shims)
        .env("CARGO_HOME", f.cargo_home.path())
        .env("RUSTUP_HOME", f.cargo_home.path().join("rustup"))
        .env_remove("RUSTC")
        .env_remove("CARGO_BUILD_RUSTC")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_ne!(out.status.code(), Some(0), "{err}");
    assert!(
        err.contains("cargo clippy does not run") && err.contains("with no jobs running"),
        "{err}"
    );
    assert!(!std::fs::read_to_string(log).unwrap().contains("uninstall"));
}

/// A working toolchain passes the check after its install, with the components and targets asked for.
#[cfg(unix)]
#[test]
fn a_working_toolchain_passes_the_check_after_its_install() {
    let f = Fixture::new();
    let (shims, log) = rustup_without_toolchain(&f, Rustup::Reports);
    let out = install_toolchain(&f, &shims, &["1.90.0", "clippy", "x86_64-pc-windows-msvc"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{err}");
    // ER-26: the job log shows which rustup and which compiler it got, so drift (a self-update, a new stable) is visible.
    assert!(err.contains("rustup 9.9.9 (test)") && err.contains("rustc 1.90.0"), "{err}");
    let log = std::fs::read_to_string(log).unwrap();
    assert!(
        log.contains("toolchain install 1.90.0 --profile minimal --component clippy --target x86_64-pc-windows-msvc --no-self-update"),
        "{log}"
    );
}

#[cfg(unix)]
#[test]
fn a_sparse_checkout_without_the_tool_is_told_how_to_add_it() {
    let f = Fixture::new();
    std::fs::remove_file(f.root().join("tools/paddock-dev/inputs.txt")).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let (code, _, err) = run_wrapper(&f, &scratch.path().join("home"), &[]);
    assert_eq!(code, 2);
    assert!(
        err.contains("tools/paddock-dev is not in this checkout") && err.contains("git sparse-checkout add tools"),
        "{err}"
    );
}

#[cfg(unix)]
#[test]
fn a_compiler_without_a_release_line_and_a_build_target_are_refused_aloud() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let odd = scratch.path().join("odd-rustc");
    write_tool(&odd, &StandIn::new(Path::new(""), "odd").stdout("not rustc"));
    let (code, _, err) = run_wrapper(&f, &home, &[("RUSTC", odd.display().to_string())]);
    assert_eq!(code, 2);
    assert!(err.contains("printed no release line"), "{err}");
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (code, _, err) = run_wrapper(
        &f,
        &home,
        &[("RUSTC", rustc), ("CARGO_BUILD_TARGET", "wasm32-unknown-unknown".to_string())],
    );
    assert_eq!(code, 2);
    assert!(err.contains("CARGO_BUILD_TARGET is set"), "{err}");
}

#[cfg(unix)]
#[test]
fn only_the_five_newest_cached_tools_are_kept() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    for i in 0..7 {
        write_tool(&home.join(format!("bin/old-{i}/paddock-dev-test-host")), &idle_tool());
        // Older directories get older times so the order is certain.
        let t = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 86_400 - i * 100);
        std::fs::File::open(home.join(format!("bin/old-{i}")))
            .unwrap()
            .set_modified(t)
            .unwrap();
    }
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (code, _, err) = run_wrapper(&f, &home, &[("RUSTC", rustc)]);
    assert_eq!(code, 0, "{err}");
    let kept = std::fs::read_dir(home.join("bin")).unwrap().count();
    assert_eq!(kept, 5);
}

#[cfg(unix)]
#[test]
fn a_source_only_cache_keeps_the_five_newest_tools_and_leaves_bin_alone() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    for i in 0..7 {
        write_tool(&home.join(format!("bin-source/old-{i}/paddock-dev-test-host")), &idle_tool());
        // Older directories get older times so the order is certain; all are past the one-day window a tool is kept for.
        let t = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 86_400 + 1000 - i * 100);
        std::fs::File::open(home.join(format!("bin-source/old-{i}")))
            .unwrap()
            .set_modified(t)
            .unwrap();
    }
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (code, _, err) = run_wrapper(&f, &home, &[("RUSTC", rustc), ("PADDOCK_DEV_DOWNLOAD", "0".to_string())]);
    assert_eq!(code, 0, "{err}");
    let kept = std::fs::read_dir(home.join("bin-source")).unwrap().count();
    assert_eq!(kept, 5);
    assert!(!home.join("bin").exists(), "a run that never downloads writes only its own cache");
}

#[cfg(unix)]
#[test]
fn a_trace_of_the_wrapper_never_prints_what_a_config_holds() {
    let f = Fixture::new();
    let token = "dummy-registry-token-0123456789";
    std::fs::write(
        f.cargo_home.path().join("config.toml"),
        format!("[registry]\ntoken = \"{token}\"\n"),
    )
    .unwrap();
    let env = f.env(&[]);
    let out = TestCommand::new("sh")
        .arg("-x")
        .arg(f.root().join("dev"))
        .arg("--wrapper-hash")
        .current_dir(f.root())
        .env_remove("RUSTC")
        .env_remove("CARGO_BUILD_RUSTC")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .envs(env.iter().cloned())
        .env("PADDOCK_DEV_HOME", f.cargo_home.path().join("dev-home"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let everything = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(
        everything.contains("config-file"),
        "the trace shows the steps, so the test looks at something: {everything}"
    );
    assert!(!everything.contains(token), "the token reached the trace");
    // ...and the token still counts: the Rust hash, which agrees with the wrapper, changes when it does.
    let before = f.both(&[]).0;
    std::fs::write(f.cargo_home.path().join("config.toml"), "[registry]\ntoken = \"another\"\n").unwrap();
    assert_ne!(f.both(&[]).0, before);
}

#[cfg(unix)]
#[test]
fn on_a_ci_runner_the_tool_never_lives_on_the_kept_mount_and_an_explicit_home_still_wins() {
    // ADR 0111: no folder is shared between concurrent CI lanes. The tool and its build folder were on the runner's /sccache mount
    // (sccache's LRU evicted cargo's rlibs there, run 8373; another lane's build could replace the binary in use). They are in the
    // job's own home now, and the mount is not touched.
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let mount = scratch.path().join("mount");
    std::fs::create_dir_all(&mount).unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (hash, _) = f.both(&[("RUSTC", rustc.clone())]);
    install_cargo(&f.cargo_home.path().join("shims"), true, |c| building(c, shim_cargo()));
    let shims_dir = f.cargo_home.path().join("shims").to_path_buf();
    let run = |extra: &[(&str, &Path)]| {
        let mut cmd = TestCommand::new("sh");
        cmd.arg(f.root().join("dev"))
            .arg("hello")
            .current_dir(f.root())
            .path_first(&shims_dir)
            .env("HOME", scratch.path().join("home"))
            .env("CARGO_HOME", f.cargo_home.path())
            .env("RUSTC", &rustc)
            .env("GITHUB_ACTIONS", "true")
            .env("PADDOCK_DEV_PACKAGES", "http://127.0.0.1:9/generic")
            // The variable of the wrapper before ADR 0111: it names a mount that exists and is writable, and is now ignored.
            .env("PADDOCK_CI_MOUNT", &mount);
        cmd.envs(extra.iter().map(|(k, v)| (*k, *v)));
        let out = cmd.output().unwrap();
        (out.status.code().unwrap(), String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    assert_eq!(run(&[]), (0, "built-by-shim".to_string()));
    let cached = scratch
        .path()
        .join("home/.paddock-dev/bin")
        .join(&hash)
        .join("paddock-dev-test-host");
    assert!(cached.exists(), "the tool is in the job's own home: {cached:?}");
    assert_eq!(
        std::fs::read_dir(&mount).unwrap().count(),
        0,
        "nothing is written to the kept mount"
    );
    // An explicit home is used as given.
    let own = scratch.path().join("own");
    assert_eq!(run(&[("PADDOCK_DEV_HOME", own.as_path())]).0, 0);
    assert!(own.join("bin").join(&hash).exists());
}

/// The programs `./dev --wrapper-hash` may start. The child's `PATH` holds only counting shims of these, so a program missing here
/// fails the run instead of going uncounted.
#[cfg(unix)]
const COUNTED: &[&str] = &[
    "git", "grep", "tr", "sed", "awk", "dirname", "ls", "uname", "sort", "cut", "env", "head", "cat",
];

/// Starts of one `./dev --wrapper-hash` (with `RUSTC` set and one config file), measured on macOS 2026-11-22 once the walk up the
/// folders and the parsing were done by the shell itself: 13. Before, 42 in the same test (eleven folder levels: a `dirname` per
/// level, and grep, sed, cut and tr per parsed line), at about 45 ms each on the Windows runner plus a fork per command substitution
/// (ER-20; run 8546, job 44099).
#[cfg(unix)]
const WRAPPER_HASH_STARTS: usize = 13;

#[cfg(unix)]
#[test]
fn a_wrapper_hash_costs_a_bounded_number_of_starts_whatever_the_folder_depth() {
    // ER-20: a command's count of process starts is a measured budget a test guards. The wrapper tests start `--wrapper-hash` many
    // times; on Windows each start costs about 45 ms and each `$(...)` a fork, so a start per folder level put one test over
    // nextest's 60 s (run 8546).
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc-real", "1.90.0", "aaaa", "test-host");
    let shims = scratch.path().join("shims");
    std::fs::create_dir_all(&shims).unwrap();
    let log = scratch.path().join("starts.log");
    let path: Vec<PathBuf> = paddock_dev::process_env::vars()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    let shim = |name: &str, real: &Path| {
        let body = format!("#!/bin/sh\necho {name} >> '{}'\nexec '{}' \"$@\"\n", log.display(), real.display());
        paddock_testkit::exe::write_program(shims.join(name), body).unwrap();
    };
    for name in COUNTED {
        let real = path
            .iter()
            .map(|d| d.join(name))
            .find(|p| p.is_file())
            .unwrap_or_else(|| panic!("{name} is not on PATH"));
        shim(name, &real);
    }
    shim("rustc", Path::new(&rustc));
    let mut cmd = TestCommand::new("sh");
    cmd.arg(f.root().join("dev"))
        .arg("--wrapper-hash")
        .current_dir(f.root().join("tools/paddock-dev/src"))
        .env("PATH", &shims)
        .env("RUSTC", shims.join("rustc"))
        .env("CARGO_HOME", f.cargo_home.path())
        .env("PADDOCK_DEV_HOME", f.cargo_home.path().join("dev-home"))
        .env_remove("CARGO_BUILD_RUSTC")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR");
    let out = cmd.output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    let starts: Vec<String> = std::fs::read_to_string(&log).unwrap().lines().map(str::to_string).collect();
    // Positive control: the probe sees the starts it must (the tree is read with git, the compiler is asked).
    assert!(
        starts.iter().any(|s| s == "git") && starts.iter().any(|s| s == "rustc"),
        "{starts:?}"
    );
    assert!(!starts.iter().any(|s| s == "dirname"), "no start per folder level: {starts:?}");
    assert!(
        starts.len() <= WRAPPER_HASH_STARTS,
        "{} starts, over the budget of {WRAPPER_HASH_STARTS}: {starts:?}",
        starts.len()
    );
}

#[cfg(unix)]
#[test]
fn a_cache_hit_costs_two_starts_the_probe_and_the_command() {
    // ER-20: a process count is a measured budget a test guards. The probe is `self hash` alone (no `--version` first), then the
    // command itself: two starts for a hit.
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let env = [("RUSTC", rustc.clone())];
    let (hash, _) = f.both(&env);
    let home = scratch.path().join("home");
    let log = scratch.path().join("starts.log");
    write_tool(
        &home.join("bin").join(&hash).join("paddock-dev-test-host"),
        &StandIn::new(Path::new(""), "cached")
            .answers(
                Actions::new()
                    .stdout_with("cached ran {args}")
                    .appends_with(&log.display().to_string(), "{args}\n"),
            )
            .when_args(
                &["self"],
                Actions::new()
                    .stdout(&format!("embedded {hash}"))
                    .appends_with(&log.display().to_string(), "{args}\n"),
            ),
    );
    let (code, out, err) = run_wrapper(&f, &home, &env);
    assert_eq!((code, out.trim()), (0, "cached ran hello"), "{err}");
    let starts: Vec<String> = std::fs::read_to_string(&log).unwrap().lines().map(str::to_string).collect();
    assert_eq!(starts, ["self hash", "hello"], "one probe start and one command start");
}

#[cfg(unix)]
#[test]
fn a_restored_binary_that_is_not_for_this_hash_is_rebuilt_and_one_for_it_is_used() {
    // The dev-tool cache restores a binary under the key of this hash; it may hold other bytes (a flipped byte, a tool of another
    // hash under a stale key). The wrapper runs it only when it runs and says it was built for this hash (ER-33).
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let env = [("RUSTC", rustc.clone())];
    let (hash, _) = f.both(&env);
    let home = scratch.path().join("home");
    let cached = home.join("bin").join(&hash).join("paddock-dev-test-host");
    // 1. runs, but was built for another hash.
    cached_tool_for(&cached, "0000000000000000000000000000000000000000");
    let (code, out, err) = run_wrapper(&f, &home, &env);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    assert!(err.contains("is not for hash"), "{err}");
    // 2. a byte flipped in a good copy: it no longer runs.
    std::fs::remove_dir_all(f.cargo_home.path().join("target")).unwrap();
    let mut bytes = std::fs::read(&cached).unwrap();
    // The first byte (the file's magic): a flip inside the program's data may leave a copy that still runs, which `self hash` covers.
    bytes[0] ^= 0xff;
    std::fs::write(&cached, &bytes).unwrap();
    let (code, out, err) = run_wrapper(&f, &home, &env);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    assert!(err.contains("rebuilding"), "{err}");
    // 3. positive control: a copy for this hash is run as it is, with no build.
    cached_tool_for(&cached, &hash);
    let (code, out, err) = run_wrapper(&f, &home, &env);
    assert_eq!((code, out.trim()), (0, "cached ran hello"), "{err}");
}

/// A stand-in `cargo` that writes its binary, announces itself with a file of its own in `$BARRIER`, and returns only once the test
/// has created `$BARRIER/go`: the test decides when the builds overlap, no sleep does.
#[cfg(unix)]
fn barrier_cargo() -> Actions {
    shim_cargo()
        .appends_with("{env:BARRIER}.dirs", &format!("{BUILD_FOLDER}\n"))
        .writes_with("{env:BARRIER}/built-{env:PADDOCK_DEV_HASH}-{pid}", "")
        .waits_for_file("{env:BARRIER}/go")
}

/// Starts one wrapper per compiler against one kept mount; returns once `expected` builds are inside `cargo`, then lets them all go.
#[cfg(unix)]
fn run_overlapping(rustcs: &[&str], expected: usize, shared_target: bool) -> (tempfile::TempDir, Vec<std::process::Output>, Fixture) {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let mount = scratch.path().join("mount");
    let barrier = scratch.path().join("barrier");
    std::fs::create_dir_all(&mount).unwrap();
    std::fs::create_dir_all(&barrier).unwrap();
    install_cargo(&f.cargo_home.path().join("shims"), true, |c| building(c, barrier_cargo()));
    let shims_dir = f.cargo_home.path().join("shims").to_path_buf();
    let mut children: Vec<_> = rustcs
        .iter()
        .enumerate()
        .map(|(i, rustc)| {
            // The child outlives this closure: its private folder is the test's, removed with `scratch`.
            let mut cmd = TestCommand::new_in("sh", &scratch.path().join(format!("child-{i}")));
            cmd.arg(f.root().join("dev"))
                .arg("hello")
                .current_dir(f.root())
                .path_first(&shims_dir)
                .env("HOME", scratch.path().join("home"))
                .env("CARGO_HOME", f.cargo_home.path())
                .env("RUSTC", rustc)
                .env("GITHUB_ACTIONS", "true")
                .env("PADDOCK_DEV_PACKAGES", "http://127.0.0.1:9/generic")
                .env("PADDOCK_DEV_HOME", mount.join("paddock-dev"))
                .env("BARRIER", &barrier);
            // A host runner's fixed build folder (conformance/ci/job-isolation.sh sets PADDOCK_DEV_TARGET_DIR for every job).
            if shared_target {
                cmd.env("PADDOCK_DEV_TARGET_DIR", scratch.path().join("shared-target"));
            }
            cmd.output_child()
        })
        .collect();
    // A wrapper that ends before its build is inside `cargo` can never reach the barrier: stop waiting at once and show what it
    // said (run 8327 waited out the test's timeout with nothing to show: a stand-in cargo had failed its install and exited 70).
    let mut ended_early = false;
    let overlapped = paddock_testkit::poll_until_within(std::time::Duration::from_secs(60), || {
        ended_early = children.iter_mut().any(|c| matches!(c.try_wait(), Ok(Some(_))));
        ended_early || std::fs::read_dir(&barrier).unwrap().count() >= expected
    });
    std::fs::write(barrier.join("go"), "").unwrap();
    let outs: Vec<std::process::Output> = children.into_iter().map(|c| c.wait_with_output().unwrap()).collect();
    let said = || {
        outs.iter()
            .map(|o| format!("exit {:?}: {}", o.status.code(), String::from_utf8_lossy(&o.stderr)))
            .collect::<Vec<_>>()
            .join("\n---\n")
    };
    assert!(!ended_early, "a wrapper ended before the builds overlapped:\n{}", said());
    assert!(overlapped, "the builds never overlapped in 60 s:\n{}", said());
    (scratch, outs, f)
}

#[cfg(unix)]
fn rustcs(dir: &Path) -> (String, String) {
    (
        fake_rustc(dir, "rustc-a", "1.90.0", "aaaa", "test-host"),
        fake_rustc(dir, "rustc-b", "1.91.0", "bbbb", "test-host"),
    )
}

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
#[cfg(unix)]
fn assert_cached(mount: &Path, hash: &str) {
    let bin = mount.join("paddock-dev/bin").join(hash);
    let out = Command::new(bin.join("paddock-dev-test-host"))
        .args(["self", "hash"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("embedded {hash}"),
        "the binary cached under {hash} was built for another hash"
    );
    let names: Vec<_> = std::fs::read_dir(&bin)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, ["paddock-dev-test-host"], "no temporary copy is left behind");
}

#[cfg(unix)]
#[test]
fn two_wrappers_with_different_hashes_on_one_kept_mount_each_cache_their_own_binary() {
    let probe = Fixture::new();
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = rustcs(tmp.path());
    let (hash_a, _) = probe.both(&[("RUSTC", a.clone())]);
    let (hash_b, _) = probe.both(&[("RUSTC", b.clone())]);
    assert_ne!(hash_a, hash_b);
    // Both builds are inside cargo at once, so a shared build folder would hold the later binary when the earlier one is copied.
    let (scratch, outs, _f) = run_overlapping(&[&a, &b], 2, false);
    for o in &outs {
        assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    }
    for hash in [&hash_a, &hash_b] {
        assert_cached(&scratch.path().join("mount"), hash);
    }
}

/// One build of a hash per host (user, 2026-11-26): two jobs on one host that need the same new hash built it at once (on 48 h of
/// runs, 38 such pairs, about 144 runner-min). The second wrapper waits for the first one's build lock, says so, and then runs the binary
/// the first one published: one build. Control: the first wrapper is held inside `cargo` until the second one is waiting, so a wrapper
/// without the lock would be inside `cargo` too (two builds) before either can publish.
#[cfg(unix)]
#[test]
fn two_wrappers_with_the_same_hash_on_one_kept_mount_build_it_once_and_both_run_it() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let (a, _) = rustcs(scratch.path());
    let (hash, _) = f.both(&[("RUSTC", a.clone())]);
    let mount = scratch.path().join("mount");
    let barrier = scratch.path().join("barrier");
    std::fs::create_dir_all(&mount).unwrap();
    std::fs::create_dir_all(&barrier).unwrap();
    install_cargo(&f.cargo_home.path().join("shims"), true, |c| building(c, barrier_cargo()));
    let shims_dir = f.cargo_home.path().join("shims").to_path_buf();
    let said_b = scratch.path().join("b.err");
    let start = |i: usize, err: Option<&Path>| {
        let mut cmd = TestCommand::new_in("sh", &scratch.path().join(format!("child-{i}")));
        match err {
            Some(e) => cmd.args(["-c", "exec sh \"$0\" hello 2>\"$1\""]).arg(f.root().join("dev")).arg(e),
            None => cmd.arg(f.root().join("dev")).arg("hello"),
        };
        cmd.current_dir(f.root())
            .path_first(&shims_dir)
            .env("HOME", scratch.path().join("home"))
            .env("CARGO_HOME", f.cargo_home.path())
            .env("RUSTC", &a)
            .env("GITHUB_ACTIONS", "true")
            .env("PADDOCK_DEV_PACKAGES", "http://127.0.0.1:9/generic")
            .env("PADDOCK_DEV_HOME", mount.join("paddock-dev"))
            .env("PADDOCK_DEV_TARGET_DIR", scratch.path().join(format!("target-{i}")))
            .env("BARRIER", &barrier);
        cmd.output_child()
    };
    let builds = || {
        std::fs::read_dir(&barrier)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("built-"))
            .count()
    };
    let first = start(0, None);
    assert!(
        paddock_testkit::poll_until_within(std::time::Duration::from_secs(60), || builds() >= 1),
        "the first build never started"
    );
    let second = start(1, Some(&said_b));
    let waiting = || {
        std::fs::read_to_string(&said_b)
            .unwrap_or_default()
            .contains("waiting for its build")
    };
    // The second wrapper either waits for the lock (it says so) or, without one, builds too.
    let settled = paddock_testkit::poll_until_within(std::time::Duration::from_secs(60), || waiting() || builds() >= 2);
    std::fs::write(barrier.join("go"), "").unwrap();
    let outs = [first.wait_with_output().unwrap(), second.wait_with_output().unwrap()];
    let said = std::fs::read_to_string(&said_b).unwrap_or_default();
    assert!(settled, "the second wrapper neither waited nor built in 60 s: {said}");
    for o in &outs {
        assert_eq!(o.status.code(), Some(0), "{}{said}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "built-by-shim");
    }
    assert_eq!(builds(), 1, "one build of the hash on this host; the second wrapper said: {said}");
    assert!(waiting(), "the second wrapper says it waits for the first one's build: {said}");
    assert_cached(&mount, &hash);
}

#[cfg(unix)]
#[test]
fn a_cache_hit_counts_as_use_and_an_orphaned_legacy_build_folder_goes() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let hours = |h: u64| std::time::SystemTime::now() - std::time::Duration::from_secs(h * 3600);
    let set = |p: &Path, t| std::fs::File::open(p).unwrap().set_modified(t).unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (hash, _) = f.both(&[("RUSTC", rustc.clone())]);
    // The tool of this hash, published long ago and used just now: a hit must refresh it.
    write_tool(
        &home.join("bin").join(&hash).join("paddock-dev-test-host"),
        &StandIn::new(Path::new(""), "tool").when_args(&["--version"], Actions::new().stdout("0")),
    );
    set(&home.join("bin").join(&hash), hours(72));
    // Five newer tools push it past the keep count; build folders: one orphaned and old, one orphaned and fresh, one with a tool.
    for i in 0..5 {
        write_tool(&home.join(format!("bin/newer-{i}/paddock-dev-test-host")), &idle_tool());
        set(&home.join(format!("bin/newer-{i}")), hours(48));
    }
    let kept_target = format!("target-{hash}");
    for (name, age) in [("target-orphan-old", 72), ("target-orphan-new", 0), (kept_target.as_str(), 72)] {
        std::fs::create_dir_all(home.join(name)).unwrap();
        set(&home.join(name), hours(age));
    }
    let (code, _, err) = run_wrapper(&f, &home, &[("RUSTC", rustc.clone())]);
    assert_eq!(code, 0, "{err}");
    assert!(home.join("bin").join(&hash).exists(), "a hit refreshed it, so it is not the oldest");
    // A second run that misses (another compiler) prunes with the hit's refreshed time in place.
    let other = fake_rustc(scratch.path(), "rustc2", "1.91.0", "bbbb", "test-host");
    let (code, _, err) = run_wrapper(&f, &home, &[("RUSTC", other)]);
    assert_eq!(code, 0, "{err}");
    assert!(home.join("bin").join(&hash).exists(), "the tool in use is kept over older ones");
    assert!(
        !home.join("target-orphan-old").exists(),
        "an old build folder with no tool is removed"
    );
    assert!(home.join("target-orphan-new").exists(), "a fresh one may be a build in progress");
    assert!(
        home.join(&kept_target).exists(),
        "a legacy build folder whose tool was published is the tool's"
    );
}

#[cfg(unix)]
trait SpawnPiped {
    fn output_child(&mut self) -> paddock_testkit::TestChild;
}

#[cfg(unix)]
impl SpawnPiped for TestCommand {
    fn output_child(&mut self) -> paddock_testkit::TestChild {
        use std::process::Stdio;
        self.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
    }
}

#[cfg(unix)]
#[test]
fn a_failed_build_names_the_hash_and_the_next_step_in_an_annotation() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    install_cargo(&f.cargo_home.path().join("shims"), true, |c| c.stderr("boom").exit(101));
    let shims_dir = f.cargo_home.path().join("shims").to_path_buf();
    let out = TestCommand::new("sh")
        .arg(f.root().join("dev"))
        .current_dir(f.root())
        .path_first(&shims_dir)
        .env("PADDOCK_DEV_HOME", scratch.path().join("home"))
        .env("CARGO_HOME", f.cargo_home.path())
        .env("RUSTC", rustc)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(
        err.contains("::error::./dev could not build paddock-dev") && err.contains("cargo build --release --locked"),
        "{err}"
    );
}

#[cfg(unix)]
#[test]
fn a_ci_run_without_a_kept_mount_says_so() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    install_cargo(&f.cargo_home.path().join("shims"), true, |c| building(c, shim_cargo()));
    let shims_dir = f.cargo_home.path().join("shims").to_path_buf();
    let out = TestCommand::new("sh")
        .arg(f.root().join("dev"))
        .current_dir(f.root())
        .path_first(&shims_dir)
        .env("HOME", scratch.path().join("home"))
        .env("CARGO_HOME", f.cargo_home.path())
        .env("RUSTC", rustc)
        .env("GITHUB_ACTIONS", "true")
        .env("PADDOCK_DEV_PACKAGES", "http://127.0.0.1:9/generic")
        .env("PADDOCK_CI_MOUNT", scratch.path().join("missing"))
        .env_remove("PADDOCK_DEV_HOME")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("::notice::./dev builds paddock-dev in this job"), "{err}");
}

#[cfg(unix)]
#[test]
fn a_cached_tool_touched_in_the_last_minutes_is_never_pruned() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    for i in 0..7 {
        write_tool(&home.join(format!("bin/recent-{i}/paddock-dev-test-host")), &idle_tool());
    }
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (code, _, err) = run_wrapper(&f, &home, &[("RUSTC", rustc)]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        std::fs::read_dir(home.join("bin")).unwrap().count(),
        8,
        "a job on the same mount may be about to run them"
    );
}

#[cfg(unix)]
#[test]
fn print_binary_names_the_cached_tool_without_running_it() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (hash, _) = f.both(&[("RUSTC", rustc.clone())]);
    let (code, out, err) = {
        let mut cmd = TestCommand::new("sh");
        install_cargo(&f.cargo_home.path().join("shims"), true, |c| building(c, shim_cargo()));
        let shims_dir = f.cargo_home.path().join("shims").to_path_buf();
        cmd.arg(f.root().join("dev"))
            .arg("--print-binary")
            .current_dir(f.root())
            .path_first(&shims_dir)
            .env("PADDOCK_DEV_HOME", &home)
            .env("PADDOCK_DEV_TARGET_DIR", f.cargo_home.path().join("target"))
            .env("CARGO_HOME", f.cargo_home.path())
            .env("RUSTC", rustc);
        let o = cmd.output().unwrap();
        (
            o.status.code().unwrap(),
            String::from_utf8_lossy(&o.stdout).trim().to_string(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        )
    };
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        home.join("bin").join(&hash).join("paddock-dev-test-host").display().to_string()
    );
    assert!(Path::new(&out).exists(), "built and cached, not run");
}

#[cfg(unix)]
#[test]
fn a_tool_used_two_hours_ago_is_kept_beyond_the_five_newest() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 3600);
    for i in 0..7 {
        write_tool(&home.join(format!("bin/used-{i}/paddock-dev-test-host")), &idle_tool());
        std::fs::File::open(home.join(format!("bin/used-{i}")))
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (code, _, err) = run_wrapper(&f, &home, &[("RUSTC", rustc)]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        std::fs::read_dir(home.join("bin")).unwrap().count(),
        8,
        "a job that started two hours ago still needs its tool"
    );
}

#[cfg(unix)]
#[test]
fn print_binary_refuses_a_build_with_uncommitted_inputs() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    std::fs::write(f.root().join("tools/paddock-dev/src/new_module.rs"), "// new\n").unwrap();
    let out = TestCommand::new("sh")
        .arg(f.root().join("dev"))
        .arg("--print-binary")
        .current_dir(f.root())
        .env("PADDOCK_DEV_HOME", scratch.path().join("home"))
        .env("CARGO_HOME", f.cargo_home.path())
        .env("RUSTC", rustc)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stdout.is_empty(), "no path to pin");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("uncommitted"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(unix)]
#[test]
fn builds_of_two_compilers_on_one_explicit_build_folder_get_a_folder_each() {
    let probe = Fixture::new();
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = rustcs(tmp.path());
    let (hash_a, _) = probe.both(&[("RUSTC", a.clone())]);
    let (hash_b, _) = probe.both(&[("RUSTC", b.clone())]);
    // Run 8027: two jobs of one host built in the fixed folder at once and broke each other's compiler outputs. Both builds are
    // inside cargo at once; a shared folder would hold the later binary when the earlier one is copied, and cargo would be told
    // the same CARGO_TARGET_DIR twice.
    let (scratch, outs, _f) = run_overlapping(&[&a, &b], 2, true);
    for o in &outs {
        assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    }
    for hash in [&hash_a, &hash_b] {
        assert_cached(&scratch.path().join("mount"), hash);
    }
    let dirs = std::fs::read_to_string(scratch.path().join("barrier.dirs")).unwrap();
    let set: std::collections::HashSet<&str> = dirs.lines().collect();
    assert_eq!(set.len(), 2, "each build is given a folder of its own: {dirs}");
    // One folder per compiler, kept for the next build (warm), named by the compiler.
    let mut left: Vec<String> = std::fs::read_dir(scratch.path().join("shared-target"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    left.sort();
    assert_eq!(left, ["rustc-aaaa", "rustc-bbbb"], "{left:?}");
}

/// A compiler whose commit follows the number of cargo runs so far (capped), as a toolchain updated by rustup mid-build does.
#[cfg(unix)]
fn updating_rustc(dir: &Path, cap: u32) -> String {
    let state = dir.join("state");
    std::fs::create_dir_all(&state).unwrap();
    let n = format!("{{lines:{}|{cap}}}", state.join("cargo-runs").display());
    let p = dir.join("rustc-updating");
    StandIn::new(dir, "rustc-updating")
        .answers(
            Actions::new()
                .stdout_with(&format!("rustc 1.9{n}.0"))
                .stdout("binary: rustc")
                .stdout_with(&format!("commit-hash: c{n}"))
                .stdout("commit-date: d")
                .stdout("host: test-host")
                .stdout_with(&format!("release: 1.9{n}.0"))
                .stdout("LLVM version: 1"),
        )
        .write_single_file(&p)
        .unwrap();
    p.display().to_string()
}

#[cfg(unix)]
fn run_with_updating_compiler(cap: u32) -> (i32, String, String, tempfile::TempDir) {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = updating_rustc(scratch.path(), cap);
    let state = scratch.path().join("state");
    let pre = Actions::new().appends(&state.join("cargo-runs"), "run\n");
    let cargo = cargo_builds(BUILD_FOLDER, pre, &built_tool("built-by-shim", "{env:PADDOCK_DEV_HASH}"));
    let (code, out, err) = run_wrapper_with_cargo(&f, &scratch.path().join("home"), &[("RUSTC", rustc)], |c| building(c, cargo));
    (code, out, err, scratch)
}

#[cfg(unix)]
#[test]
fn a_compiler_that_changes_during_the_build_starts_the_wrapper_over_once() {
    let (code, out, err, scratch) = run_with_updating_compiler(1);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.trim(), "built-by-shim");
    assert!(
        err.contains("::warning::./dev: the Rust compiler changed while paddock-dev was building"),
        "{err}"
    );
    assert!(err.contains("starting over once"), "{err}");
    // Only the tool built by the compiler that is installed now is published; the build folder of the first try is gone.
    let bins: Vec<_> = std::fs::read_dir(scratch.path().join("home/bin")).unwrap().flatten().collect();
    assert_eq!(bins.len(), 1, "{bins:?}");
}

#[cfg(unix)]
#[test]
fn a_compiler_that_changes_again_during_the_second_build_is_a_clear_error() {
    let (code, _, err, scratch) = run_with_updating_compiler(9);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("::error::./dev: the Rust compiler changed again"), "{err}");
    assert!(err.contains("rustup show"), "{err}");
    assert!(
        !scratch.path().join("home/bin").exists(),
        "nothing is published from a build the compiler changed under"
    );
}

#[cfg(unix)]
#[test]
fn an_uncommitted_input_builds_in_a_folder_of_its_own_compiler() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let a = fake_rustc(scratch.path(), "rustc-a", "1.90.0", "aaaa1111bbbb2222", "test-host");
    let b = fake_rustc(scratch.path(), "rustc-b", "1.91.0", "cccc3333dddd4444", "test-host");
    std::fs::write(f.root().join("tools/paddock-dev/src/new.rs"), "// uncommitted\n").unwrap();
    for rustc in [&a, &b] {
        let (code, _, err) = run_wrapper(&f, &scratch.path().join("home"), &[("RUSTC", rustc.clone())]);
        assert_eq!(code, 0, "{err}");
    }
    let target = f.cargo_home.path().join("target");
    assert!(
        target.join("rustc-aaaa1111bbbb/release/paddock-dev").exists(),
        "{:?}",
        std::fs::read_dir(&target).map(|r| r.flatten().count())
    );
    assert!(target.join("rustc-cccc3333dddd/release/paddock-dev").exists());
}

/// The steps of a `cargo` that builds a tool printing `what` (a template the tool fills when it runs), after running `pre`.
#[cfg(unix)]
fn cargo_building(pre: Actions, what: &str) -> Actions {
    cargo_builds(BUILD_FOLDER, pre, &built_tool(what, "{env:PADDOCK_DEV_HASH}"))
}

#[cfg(unix)]
#[test]
fn the_restart_marker_is_not_passed_on_to_the_tool_so_a_nested_dev_can_restart() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = updating_rustc(scratch.path(), 1);
    let state = scratch.path().join("state");
    let cargo = cargo_building(
        Actions::new().appends(&state.join("cargo-runs"), "run\n"),
        "restarted={{env:PADDOCK_DEV_RESTARTED|unset}}",
    );
    let (code, out, err) = run_wrapper_with_cargo(&f, &scratch.path().join("home"), &[("RUSTC", rustc)], |c| building(c, cargo));
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("starting over once"), "the run did restart: {err}");
    assert_eq!(out.trim(), "restarted=unset");
}

#[cfg(unix)]
#[test]
fn a_changed_input_builds_warm_in_the_folder_of_the_same_compiler() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa1111bbbb2222", "test-host");
    let home = scratch.path().join("home");
    let first = cargo_building(Actions::new().writes_with(&format!("{BUILD_FOLDER}/warm-marker"), ""), "first");
    let (code, _, err) = run_wrapper_with_cargo(&f, &home, &[("RUSTC", rustc.clone())], |c| building(c, first));
    assert_eq!(code, 0, "{err}");
    // A new commit under an input path: a new hash, the same compiler, so the same folder with what the first build left in it.
    std::fs::write(f.root().join("tools/paddock-dev/src/main.rs"), "fn main() { /* changed */ }\n").unwrap();
    git(f.root(), &["commit", "-q", "-am", "chore: change an input"]);
    let second = cargo_building(Actions::new(), "second");
    let (code, out, err) = run_wrapper_with_cargo(&f, &home, &[("RUSTC", rustc)], |c| {
        building(c, second).when(
            &Case::new().file_missing_with(&format!("{BUILD_FOLDER}/warm-marker")),
            Actions::new().stderr("cold").exit(101),
        )
    });
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.trim(), "second");
}

#[cfg(unix)]
#[test]
fn a_binary_replaced_by_another_build_before_the_copy_is_built_again() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let tries = scratch.path().join("tries");
    // The first run of cargo leaves a tool for another hash (a build of another input in the same folder got there first).
    // Another input's build in the same folder got there first: the first run's tool names a hash that is not this one's.
    let build = |hash: &str| {
        cargo_builds(
            BUILD_FOLDER,
            Actions::new().appends(&tries, "t\n"),
            &built_tool("built-by-shim", hash),
        )
    };
    let (code, out, err) = run_wrapper_with_cargo(&f, &scratch.path().join("home"), &[("RUSTC", rustc)], |c| {
        building(c, build("{env:PADDOCK_DEV_HASH}")).when(&build_call().file_missing(&tries), build("somebody-elses"))
    });
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.trim(), "built-by-shim");
    assert!(err.contains("try 2"), "{err}");
}

#[cfg(unix)]
#[test]
fn build_folders_of_old_compilers_go_after_a_week_and_the_one_in_use_never() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let target = f.cargo_home.path().join("target");
    let days = |d: u64| std::time::SystemTime::now() - std::time::Duration::from_secs(d * 86400);
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    for (name, age) in [("rustc-old", 30), ("rustc-recent", 2), ("rustc-aaaa", 30)] {
        std::fs::create_dir_all(target.join(name)).unwrap();
        std::fs::File::open(target.join(name)).unwrap().set_modified(days(age)).unwrap();
    }
    let (code, _, err) = run_wrapper(&f, &home, &[("RUSTC", rustc)]);
    assert_eq!(code, 0, "{err}");
    assert!(!target.join("rustc-old").exists(), "another compiler's folder, a month unused");
    assert!(target.join("rustc-recent").exists());
    assert!(
        target.join("rustc-aaaa").exists(),
        "the compiler in use keeps its folder whatever its age"
    );
}

/// The start time of a running process as the wrapper records it: through the same builder, so `ps` prints it in the same locale
/// (the machine's `LANG` writes `Fri 13 Nov`, a clean environment `Fri Nov 13`).
#[cfg(unix)]
fn stamp_of(pid: u32) -> String {
    let out = TestCommand::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(unix)]
fn install_lock(f: &Fixture, owner_pid: u32, stamp: &str) -> PathBuf {
    let lock = f.cargo_home.path().join("rustup/.paddock-install-1.90.0.lock");
    std::fs::create_dir_all(&lock).unwrap();
    std::fs::write(lock.join("owner"), format!("{owner_pid}\n{stamp}\n")).unwrap();
    lock
}

#[cfg(unix)]
#[test]
fn a_timed_out_install_is_uninstalled_so_no_half_toolchain_is_left() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let (_, log) = rustup_without_toolchain(&f, Rustup::Hangs);
    let (code, _, err) = run_wrapper(
        &f,
        &scratch.path().join("home"),
        &[("PADDOCK_DEV_TOOLCHAIN_TIMEOUT", "1".to_string())],
    );
    assert_eq!(code, 2, "{err}");
    let log = std::fs::read_to_string(log).unwrap();
    assert!(log.contains("toolchain uninstall 1.90.0"), "{log}");
    assert!(
        !f.cargo_home.path().join("rustup/.paddock-install-1.90.0.lock").exists(),
        "the lock is released"
    );
}

#[cfg(unix)]
#[test]
fn an_install_waits_for_a_live_holder_of_the_lock_and_never_runs_beside_it() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let (_, log) = rustup_without_toolchain(&f, Rustup::Installs);
    // This test process is a live owner (pid and start time match), however old the lock is.
    let me = std::process::id();
    let lock = install_lock(&f, me, &stamp_of(me));
    let (code, _, err) = run_wrapper(
        &f,
        &scratch.path().join("home"),
        &[("PADDOCK_DEV_TOOLCHAIN_TIMEOUT", "2".to_string())],
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("holds") && err.contains(&me.to_string()), "{err}");
    assert!(lock.exists(), "a live holder's lock is not taken");
    assert!(
        !log.exists() || !std::fs::read_to_string(log).unwrap().contains("toolchain install"),
        "no install beside the holder"
    );
}

#[cfg(unix)]
#[test]
fn a_lock_of_a_gone_owner_or_of_a_reused_pid_is_taken_over_whatever_its_age() {
    let me = std::process::id();
    let dead = {
        let mut c = TestCommand::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
        let pid = c.id();
        c.wait().unwrap();
        pid
    };
    for (pid, stamp) in [
        (dead, "Mon Jan 1 00:00:00 2001".to_string()),
        (me, "Mon Jan 1 00:00:00 2001".to_string()),
    ] {
        let f = Fixture::new();
        let scratch = tempfile::tempdir().unwrap();
        let (_, log) = rustup_without_toolchain(&f, Rustup::Installs);
        install_lock(&f, pid, &stamp);
        let (code, _, err) = run_wrapper(&f, &scratch.path().join("home"), &[("PADDOCK_DEV_DOWNLOAD", "0".to_string())]);
        assert_eq!(code, 0, "pid {pid}: {err}");
        assert!(err.contains("taking it over"), "{err}");
        assert!(std::fs::read_to_string(log).unwrap().contains("toolchain install"));
    }
}

#[cfg(unix)]
#[test]
fn after_a_failed_install_earlier_in_the_job_no_second_install_is_tried() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let (_, log) = rustup_without_toolchain(&f, Rustup::Fails);
    let (code, _, err) = run_wrapper(&f, &scratch.path().join("home"), &[("PADDOCK_TOOLCHAIN_FAILED", "1".to_string())]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("an earlier step of this job failed to"), "{err}");
    assert!(!log.exists(), "rustup was not run at all");
}

/// Seven cached tools of three days ago: past the keep count and past the day a tool is kept for.
#[cfg(unix)]
fn old_entries(home: &Path, names: &[String], file: &str) {
    for (i, n) in names.iter().enumerate() {
        write_tool(&home.join("bin").join(n).join(file), &idle_tool());
        let t = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 86_400 - i as u64 * 100);
        std::fs::File::open(home.join("bin").join(n)).unwrap().set_modified(t).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn a_cached_tool_that_a_run_holds_is_never_pruned() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let names: Vec<String> = (0..7).map(|i| format!("old-{i}")).collect();
    old_entries(&home, &names, "paddock-dev-test-host");
    // A run holds the oldest (a hard link under held/, as tool_cache::hold makes): it is still running it.
    std::fs::create_dir_all(home.join("held")).unwrap();
    std::fs::hard_link(home.join("bin/old-0/paddock-dev-test-host"), home.join("held/paddock-dev-1-1")).unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (code, _, err) = run_wrapper(&f, &home, &[("RUSTC", rustc)]);
    assert_eq!(code, 0, "{err}");
    assert!(home.join("bin/old-0").exists(), "a held tool is kept, whatever its age: {err}");
    assert!(err.contains("a run holds it"), "the kept entry is said (ER-27): {err}");
    assert_eq!(
        std::fs::read_dir(home.join("bin")).unwrap().count(),
        6,
        "the five newest and the held one"
    );
}

#[cfg(unix)]
#[test]
fn other_tools_in_the_cache_are_not_counted_as_the_wrappers_own_and_keep_five_of_their_own() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let mine: Vec<String> = (0..3).map(|i| format!("old-{i}")).collect();
    old_entries(&home, &mine, "paddock-dev-test-host");
    let heavy: Vec<String> = (0..7).map(|i| format!("throttle-{i:040x}")).collect();
    old_entries(&home, &heavy, "throttle");
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (hash, _) = f.both(&[("RUSTC", rustc.clone())]);
    // A legacy build folder named like a heavy entry is not the wrapper's to remove for it.
    std::fs::create_dir_all(home.join(format!("target-{}", heavy[0]))).unwrap();
    let (code, _, err) = run_wrapper(&f, &home, &[("RUSTC", rustc)]);
    assert_eq!(code, 0, "{err}");
    for n in &mine {
        assert!(
            home.join("bin").join(n).exists(),
            "{n}: four of paddock-dev's own, all within its five"
        );
    }
    assert!(home.join("bin").join(&hash).exists());
    let heavy_left = heavy.iter().filter(|n| home.join("bin").join(n).exists()).count();
    assert_eq!(heavy_left, 5, "heavy keeps its own five newest");
    assert!(
        !home.join("bin").join(&heavy[0]).exists() && !home.join("bin").join(&heavy[1]).exists(),
        "its two oldest go"
    );
    assert!(home.join(format!("target-{}", heavy[0])).exists());
}

#[cfg(unix)]
#[test]
fn a_cached_tool_that_fails_its_probe_is_replaced_never_removed_first() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let env = [("RUSTC", rustc)];
    let (hash, _) = f.both(&env);
    let home = scratch.path().join("home");
    let cached = home.join("bin").join(&hash).join("paddock-dev-test-host");
    // A probe that fails (on a loaded machine a slow `--version` is enough) and a build that fails: another run pinned to this
    // path (`--print-binary`) must still find it.
    write_tool(
        &cached,
        &StandIn::new(Path::new(""), "tool")
            .stdout("pinned")
            .when_args(&["--version"], Actions::new().exit(1)),
    );
    let (code, _, err) = run_wrapper_with_cargo(&f, &home, &env, |c| c.exit(1));
    assert_ne!(code, 0, "{err}");
    assert!(
        cached.is_file(),
        "the cached file is replaced by the rebuild's rename, never removed before it: {err}"
    );
}

#[cfg(unix)]
#[test]
fn the_cached_tool_holds_itself_for_its_run_and_completes_when_its_cache_entry_is_pruned_meanwhile() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let entry = home.join("bin").join("0123456789abcdef0123456789abcdef01234567");
    std::fs::create_dir_all(&entry).unwrap();
    let cached = entry.join("paddock-dev-test-host");
    std::fs::copy(crate::dev_bin::PATH, &cached).unwrap();
    let signal = scratch.path().join("signal.log");
    std::fs::write(&signal, "").unwrap();
    let repo = scratch.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let init = TestCommand::new("git").args(["init", "-q"]).current_dir(&repo).output().unwrap();
    assert!(init.status.success(), "{}", String::from_utf8_lossy(&init.stderr));
    let mut run = TestCommand::new(&cached)
        .args(["wait", "--file"])
        .arg(&signal)
        .args(["--line", "^go$", "--include-past", "--max", "60s", "--interval", "100ms"])
        // `wait` keeps its one-waiter lock in the git folder of the repository it runs in: run it in a repository of the test's own,
        // never the checkout (ER-45; the checkout is read-only in the Linux container, where the lock write failed the run).
        .current_dir(&repo)
        // The harness points the lock folder at the test's private folder (STATE_FOLDERS); this test is about the repository's own, so the
        // setting is removed (the repository is the test's own, never the checkout).
        .env_remove("PADDOCK_WAIT_STATE")
        .env("PADDOCK_DEV_HOME", &home)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // The hold is made before the command runs, and `wait` says it is waiting once it runs: one line read is the event.
    let held = |home: &Path| std::fs::read_dir(home.join("held")).map(|d| d.count()).unwrap_or(0);
    let mut err = std::io::BufReader::new(run.stderr.take().unwrap());
    let mut first = String::new();
    std::io::BufRead::read_line(&mut err, &mut first).unwrap();
    assert!(first.contains("waiting for a line"), "{first}");
    assert_eq!(held(&home), 1, "the run holds its own binary");
    let waits = repo.join(".git/paddock/waits");
    let locks = std::fs::read_dir(&waits).map(|d| d.count()).unwrap_or(0);
    assert_eq!(
        locks,
        1,
        "the waiter's lock is in the test's own repository ({}), not the checkout's",
        waits.display()
    );
    {
        use std::os::unix::fs::MetadataExt;
        assert!(
            std::fs::metadata(&cached).unwrap().nlink() > 1,
            "so `./dev`'s prune would keep the entry"
        );
    }
    // Another run's prune (or a failed probe) removes the cache entry in the middle of the run; the run still completes.
    std::fs::remove_dir_all(&entry).unwrap();
    std::fs::write(&signal, "go\n").unwrap();
    let out = run.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(held(&home), 0, "the hold ends with the run");
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("this run holds"),
        "the self-hold is quiet"
    );
}

#[cfg(unix)]
#[test]
fn empty_cache_folders_are_a_group_of_their_own_and_never_take_the_cache_with_them() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    for i in 0..7 {
        let d = home.join(format!("bin/empty-{i}"));
        std::fs::create_dir_all(&d).unwrap();
        let t = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 86_400 - i * 100);
        std::fs::File::open(&d).unwrap().set_modified(t).unwrap();
    }
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (hash, _) = f.both(&[("RUSTC", rustc.clone())]);
    let (code, _, err) = run_wrapper(&f, &home, &[("RUSTC", rustc)]);
    assert_eq!(code, 0, "{err}");
    assert!(home.join("bin").join(&hash).is_dir(), "the tool just built is still cached: {err}");
    let empty_left = (0..7).filter(|i| home.join(format!("bin/empty-{i}")).exists()).count();
    assert_eq!(empty_left, 5, "the empty folders keep five, like any tool");
}

/// What a stand-in `sccache` answers to `--show-stats` in the self-build tests: the counters the wrapper reports, without cache paths.
#[cfg(unix)]
const SELF_BUILD_STATS: &str = "Compile requests                    385
Compile requests executed           316
Cache hits                          312
Cache misses                          1
Cache hits rate                  99.68 %
";

/// One run of the wrapper in a cold CI job (`GITHUB_ACTIONS`, a kept volume, a stand-in `sccache`) with a stand-in `cargo` that logs the
/// compile-cache variables it was started with and its arguments. Returns (exit code, wrapper output, cargo's log, sccache's calls).
#[cfg(unix)]
fn self_build(extra_env: &[(&str, String)], sccache_start_fails: bool) -> (i32, String, String, String) {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let mount = scratch.path().join("mount");
    std::fs::create_dir_all(&mount).unwrap();
    let (cargo_log, sccache_log) = (scratch.path().join("cargo-log"), scratch.path().join("sccache-log"));
    let call = |a: Actions| a.appends_with(&sccache_log.display().to_string(), "{args}\n");
    let mut sccache = stand_in_at(&f.cargo_home.path().join("shims").join("sccache"))
        .answers(call(Actions::new()))
        .when_args(&["--show-stats"], call(Actions::new()).stdout(SELF_BUILD_STATS));
    if sccache_start_fails {
        sccache = sccache.when_args(&["--start-server"], call(Actions::new()).stderr("cannot bind").exit(1));
    }
    sccache.install().unwrap();
    // The lock itself is the kernel's (util-linux `flock`, on the Linux CI image): macOS has none, and a stand-in that succeeds lets
    // the test see what the wrapper runs under it.
    stand_in_at(&f.cargo_home.path().join("shims").join("flock")).install().unwrap();
    let mut env = vec![
        ("RUSTC", rustc),
        ("GITHUB_ACTIONS", "true".to_string()),
        ("PADDOCK_SCCACHE_MOUNT", mount.display().to_string()),
        // A download that cannot connect (nothing listens on port 1): the wrapper then builds, and no test reaches the network.
        ("PADDOCK_DEV_DOWNLOAD", "1".to_string()),
        ("PADDOCK_DEV_PACKAGES", "http://127.0.0.1:1".to_string()),
        ("CARGO_BUILD_JOBS", "3".to_string()),
    ];
    env.extend(extra_env.iter().map(|(k, v)| (*k, v.clone())));
    let log = cargo_log.display().to_string();
    let (code, out, err) = run_wrapper_with_cargo(&f, &scratch.path().join("home"), &env, |c| {
        building(
            c,
            cargo_builds(
                BUILD_FOLDER,
                Actions::new().appends_with(
                    &log,
                    "wrapper={env:RUSTC_WRAPPER|unset} dir={env:SCCACHE_DIR|unset} jobs={env:CARGO_BUILD_JOBS|unset} incr={env:CARGO_INCREMENTAL|unset} args={args}\n",
                ),
                &built_tool("built-by-shim", "{env:PADDOCK_DEV_HASH}"),
            ),
        )
    });
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
    assert_eq!(code, 0, "{out}{err}");
    (code, format!("{out}{err}"), read(&cargo_log), read(&sccache_log))
}

/// The cold self-build of a CI job on a runner with the kept volume compiles through a local-disk sccache in a folder of that volume:
/// the dependencies come from it, so only the tool's own crate compiles (docs/benchmarks.md, 2026-11-27, "the tool's own build").
/// The server runs for this build alone (started, then stopped), and the counters are printed for the job log (ER-27).
#[cfg(unix)]
#[test]
fn a_cold_self_build_in_a_ci_job_compiles_through_the_kept_local_sccache() {
    let (_, said, cargo, sccache) = self_build(&[], false);
    assert!(
        cargo.contains("wrapper=sccache") && cargo.contains("/mount/paddock-dev-build-cache ") && cargo.contains("incr=0"),
        "{cargo}"
    );
    // The job count goes by flag: sccache hashes CARGO_BUILD_JOBS into its keys, which then differ from runner to runner.
    assert!(cargo.contains("jobs=unset") && cargo.contains("-j 3"), "{cargo}");
    let calls: Vec<&str> = sccache.lines().collect();
    assert_eq!(
        calls,
        ["--stop-server", "--start-server", "--show-stats", "--stop-server"],
        "a stale server is stopped, ours runs for this build, and ends with it"
    );
    assert!(
        said.contains("compile cache") && said.contains("312 hits") && said.contains("1 misses"),
        "{said}"
    );
}

/// The wrapper's hash does not know about the compile cache: the same inputs name the same tool with or without it (the dev-tool
/// store and the Actions cache are keyed by it).
#[cfg(unix)]
#[test]
fn the_compile_cache_does_not_change_the_hash() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let plain = f.both(&[("RUSTC", rustc.clone())]);
    let cached = f.both(&[
        ("RUSTC", rustc),
        ("GITHUB_ACTIONS", "true".to_string()),
        ("RUSTC_WRAPPER", "sccache".to_string()),
        ("SCCACHE_DIR", "/sccache/x".to_string()),
        ("CARGO_INCREMENTAL", "0".to_string()),
        ("PADDOCK_SCCACHE_MOUNT", scratch.path().display().to_string()),
    ]);
    assert_eq!(plain, cached);
}

/// An explicit `RUSTC_WRAPPER` wins (the job's own setup-rust, a developer's shim), and so do the cases where the cache must not be
/// read: not a CI job, no kept volume, a release, rule 54 or tag run (a tag build reads nothing kept, ADR 0085), the switch off.
#[cfg(unix)]
#[test]
fn an_explicit_wrapper_and_every_release_or_missing_volume_case_leave_the_build_as_it_was() {
    let none = |s: &str| ("PADDOCK_SCCACHE_MOUNT", s.to_string());
    let cases: Vec<(&str, Vec<(&str, String)>)> = vec![
        ("explicit wrapper", vec![("RUSTC_WRAPPER", "/opt/my-wrapper".to_string())]),
        ("not a CI job", vec![("GITHUB_ACTIONS", "false".to_string())]),
        ("no kept volume", vec![none("/nonexistent/volume")]),
        ("a release or rule 54 run", vec![("PADDOCK_DEV_DOWNLOAD", "0".to_string())]),
        ("a tag run", vec![("GITHUB_REF", "refs/tags/v1.2.3".to_string())]),
        ("a tag rehearsal", vec![("PADDOCK_TAG_REHEARSAL", "1".to_string())]),
        ("switched off", vec![("PADDOCK_DEV_SELFBUILD_CACHE", "off".to_string())]),
    ];
    for (what, env) in cases {
        let (_, said, cargo, sccache) = self_build(&env, false);
        let want = env
            .iter()
            .find(|(k, _)| *k == "RUSTC_WRAPPER")
            .map_or("unset".to_string(), |(_, v)| v.clone());
        assert!(cargo.contains(&format!("wrapper={want} ")), "{what}: {cargo}");
        assert!(
            cargo.contains("jobs=3") && !cargo.contains("-j 3"),
            "{what}: the job count stays in the environment: {cargo}"
        );
        assert_eq!(sccache, "", "{what}: sccache was not started");
        assert!(!said.contains("compile cache"), "{what}: {said}");
    }
}

/// The cache slows a build down, never fails it: a server that does not start leaves the build as it was, with one notice.
#[cfg(unix)]
#[test]
fn a_server_that_does_not_start_leaves_the_build_without_the_compile_cache() {
    let (_, said, cargo, sccache) = self_build(&[], true);
    assert!(cargo.contains("wrapper=unset ") && cargo.contains("jobs=3"), "{cargo}");
    assert!(said.contains("::notice::") && said.contains("did not start"), "{said}");
    assert!(sccache.contains("--start-server") && !sccache.contains("--show-stats"), "{sccache}");
}
