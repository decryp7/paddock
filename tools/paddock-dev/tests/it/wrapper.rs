//! The `./dev` wrapper and `dev self hash` compute one value, in shell and in Rust, from the same inputs.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::input_hash::{Real, compute, is_hashed_var};
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

    #[expect(
        clippy::disallowed_methods,
        reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
    )]
    fn shell(&self, env: &[(String, String)]) -> (String, bool) {
        let mut cmd = Command::new("sh");
        cmd.arg(self.root().join("dev"))
            .arg("--wrapper-hash")
            .current_dir(self.root().join("tools/paddock-dev/src"))
            .env_remove("RUSTC")
            .env_remove("CARGO_BUILD_RUSTC");
        // The hash reads the environment: start from none of the variables it uses, as `rust` does.
        std::env::vars().filter(|(k, _)| is_hashed_var(k)).for_each(|(k, _)| {
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

/// A "compiler" that prints a chosen `-vV`.
#[cfg(unix)]
pub(crate) fn fake_rustc(dir: &Path, name: &str, release: &str, commit: &str, host: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    let body = format!(
        "#!/bin/sh\nprintf 'rustc {release}\\nbinary: rustc\\ncommit-hash: {commit}\\ncommit-date: d\\nhost: {host}\\nrelease: {release}\\nLLVM version: 1\\n'\n"
    );
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
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

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
#[test]
fn the_wrapper_finds_the_root_from_a_subfolder_and_refuses_to_run_outside_a_repository() {
    let f = Fixture::new();
    let (hash, _) = f.shell(&f.env(&[]));
    assert!(!hash.is_empty(), "run from tools/paddock-dev/src");
    let outside = tempfile::tempdir().unwrap();
    let out = Command::new("sh")
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
    ] {
        std::fs::write(&ci, other).unwrap();
        assert_ne!(f.both(&[]).0, plain.0, "{other:?} is hashed");
    }
}

/// A tiny stand-in for the tool binary: answers `--version`, otherwise prints a marker.
#[cfg(unix)]
pub(crate) fn write_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A stand-in `cargo` that "builds" a script answering `--version`, `self hash` (the hash it was built for, as the real tool shows)
/// and a marker, then takes `slow` seconds to return, as a busy build would.
#[cfg(unix)]
pub(crate) fn shim_cargo(slow: u32) -> String {
    format!(
        r#"mkdir -p "$CARGO_TARGET_DIR/release"; printf '#!/bin/sh\nif [ "$1" = "--version" ]; then echo 0; exit 0; fi\nif [ "$1" = self ]; then echo "embedded %s"; exit 0; fi\necho built-by-shim\n' "$PADDOCK_DEV_HASH" > "$CARGO_TARGET_DIR/release/paddock-dev"; chmod +x "$CARGO_TARGET_DIR/release/paddock-dev"; sleep {slow}"#
    )
}

/// Runs the whole wrapper (not just `--wrapper-hash`) with a scratch cache and a stand-in `cargo` that "builds" a script.
#[cfg(unix)]
fn run_wrapper(f: &Fixture, home: &Path, extra_env: &[(&str, String)]) -> (i32, String, String) {
    run_wrapper_with_cargo(f, home, extra_env, &shim_cargo(0))
}

/// [`run_wrapper`] with a `cargo` of the test's own.
#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
#[cfg(unix)]
fn run_wrapper_with_cargo(f: &Fixture, home: &Path, extra_env: &[(&str, String)], cargo: &str) -> (i32, String, String) {
    let shims = f.cargo_home.path().join("shims");
    write_script(&shims.join("cargo"), cargo);
    let path = format!("{}:{}", shims.display(), std::env::var("PATH").unwrap());
    let mut cmd = Command::new("sh");
    cmd.arg(f.root().join("dev"))
        .arg("hello")
        .current_dir(f.root())
        .env("PATH", path)
        .env("PADDOCK_DEV_HOME", home)
        .env("PADDOCK_DEV_TARGET_DIR", f.cargo_home.path().join("target"))
        .env("CARGO_HOME", f.cargo_home.path())
        // The toolchain install lock lives in RUSTUP_HOME: never the user's.
        .env("RUSTUP_HOME", f.cargo_home.path().join("rustup"))
        .env_remove("RUSTC")
        .env_remove("CARGO_BUILD_RUSTC")
        // `dev` changes its messages and defaults on a CI runner; a test opts in through `extra_env`.
        .env_remove("GITHUB_ACTIONS");
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
    write_script(&cached, "");
    std::fs::write(&cached, "").unwrap();
    let (code, out, err) = run_wrapper(&f, &home, &env);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    assert!(err.contains("does not run; rebuilding"), "{err}");
    // The rebuilt copy is cached and answers --version, so the next run uses it without building.
    std::fs::remove_dir_all(f.cargo_home.path().join("target")).unwrap();
    let (code, out, err) = run_wrapper(&f, &home, &env);
    assert_eq!((code, out.trim(), err.as_str()), (0, "built-by-shim", ""));
    assert!(!f.cargo_home.path().join("target").exists(), "no second build");
}

/// A cached tool that answers `--version` and otherwise says it ran (what an old cached binary would do).
#[cfg(unix)]
fn cached_tool(path: &Path) {
    write_script(path, r#"if [ "$1" = "--version" ]; then echo 0; else echo "cached ran $*"; fi"#);
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

/// A Windows-shaped runner: rustup is there, its `rustc` shim cannot run until the pinned toolchain is installed, no RUSTC is set.
/// Returns the folder of the shims and the log of the fake rustup.
#[cfg(unix)]
fn rustup_without_toolchain(f: &Fixture, rustup_body: &str) -> (PathBuf, PathBuf) {
    std::fs::write(
        f.root().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.90.0\"\nprofile = \"minimal\"\ncomponents = [\"clippy\"]\n",
    )
    .unwrap();
    let shims = f.cargo_home.path().join("shims");
    let marker = f.cargo_home.path().join("installed");
    let log = f.cargo_home.path().join("rustup.log");
    write_script(
        &shims.join("rustc"),
        &format!(
            "[ -e '{}' ] || {{ echo \"error: toolchain '1.90.0' is not installed\" >&2; exit 1; }}\nprintf 'rustc 1.90.0\\nbinary: rustc\\ncommit-hash: aaaa\\ncommit-date: d\\nhost: test-host\\nrelease: 1.90.0\\nLLVM version: 1\\n'",
            marker.display()
        ),
    );
    write_script(
        &shims.join("rustup"),
        &format!(
            "echo \"$PWD: $*\" >> '{}'\n{}",
            log.display(),
            rustup_body.replace("MARKER", &marker.display().to_string())
        ),
    );
    // The install is checked by running cargo and the components the file asks for (clippy here); a test may replace this one.
    write_script(&shims.join("cargo"), "echo cargo 1.90.0");
    (shims, log)
}

#[cfg(unix)]
#[test]
fn a_runner_with_rustup_but_without_the_pinned_toolchain_installs_it_before_building() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let (_, log) = rustup_without_toolchain(&f, "[ \"$1 $2\" = 'toolchain install' ] && touch 'MARKER'");
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
    for (body, timeout, want) in [
        ("exit 1", "30", "exit 1"),
        ("[ \"$1 $2\" = 'toolchain install' ] && exec sleep 30; exit 0", "1", "exit 124"),
    ] {
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

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
#[cfg(unix)]
#[test]
fn ensure_toolchain_succeeds_when_the_compiler_runs_after_the_install() {
    let f = Fixture::new();
    let (shims, _) = rustup_without_toolchain(&f, "[ \"$1 $2\" = 'toolchain install' ] && touch 'MARKER'");
    let path = format!("{}:{}", shims.display(), std::env::var("PATH").unwrap());
    let run = || {
        Command::new("sh")
            .arg(f.root().join("dev"))
            .arg("--ensure-toolchain")
            .current_dir(f.root())
            .env("PATH", &path)
            .env("CARGO_HOME", f.cargo_home.path())
            .env("RUSTUP_HOME", f.cargo_home.path().join("rustup"))
            .env_remove("RUSTC")
            .env_remove("CARGO_BUILD_RUSTC")
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

/// A rustup whose first install leaves the toolchain half installed (rustc runs, `cargo clippy` does not), as the Windows runner's
/// 1.100.0 after two jobs installed it at once (CI run 8060: "could not remove component file cargo.exe", then "the cargo-clippy
/// binary is not applicable to the toolchain"). An uninstall repairs it when `repairable`. Returns the shims and rustup's log.
#[cfg(unix)]
fn rustup_half_installing(f: &Fixture, repairable: bool) -> (PathBuf, PathBuf) {
    let repaired = f.cargo_home.path().join("repaired");
    let uninstall = if repairable {
        format!("touch '{}'", repaired.display())
    } else {
        String::new()
    };
    let (shims, log) = rustup_without_toolchain(
        f,
        &format!(
            "case \"$1 $2\" in 'toolchain install') touch 'MARKER' ;; 'toolchain uninstall') rm -f 'MARKER'; {uninstall} ;; esac; exit 0"
        ),
    );
    write_script(
        &shims.join("cargo"),
        &format!(
            "if [ \"$1 $2\" = 'clippy --version' ]; then [ -e '{}' ] || {{ echo \"error: the 'cargo-clippy' binary is not applicable to the toolchain\" >&2; exit 1; }}; fi\necho cargo 1.90.0",
            repaired.display()
        ),
    );
    (shims, log)
}

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
#[cfg(unix)]
fn install_toolchain(f: &Fixture, shims: &Path, args: &[&str]) -> std::process::Output {
    // Run from outside any repository: the setup-rust action calls this mode before anything else is set up.
    let outside = tempfile::tempdir().unwrap();
    Command::new("sh")
        .arg(f.root().join("dev"))
        .arg("--install-toolchain")
        .args(args)
        .current_dir(outside.path())
        .env("PATH", format!("{}:{}", shims.display(), std::env::var("PATH").unwrap()))
        .env("RUSTUP_HOME", f.cargo_home.path().join("rustup"))
        .env_remove("RUSTUP_TOOLCHAIN")
        .output()
        .unwrap()
}

/// ER-26 (runs 8027, 8028, 8060): a toolchain whose install succeeded but whose cargo, clippy or rustfmt does not run is uninstalled
/// and installed again, inside the install lock, and the job goes on.
#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
#[cfg(unix)]
#[test]
fn a_half_installed_toolchain_is_uninstalled_and_installed_again_under_the_lock() {
    let f = Fixture::new();
    let (shims, log) = rustup_half_installing(&f, true);
    let out = install_toolchain(&f, &shims, &["1.90.0", "clippy", ""]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{err}");
    assert!(err.contains("cargo clippy does not run") && err.contains("repaired"), "{err}");
    let log = std::fs::read_to_string(log).unwrap();
    let calls: Vec<&str> = log.lines().map(|l| l.split_once(": ").map_or(l, |(_, c)| c)).collect();
    assert_eq!(
        calls,
        [
            "toolchain install 1.90.0 --profile minimal --component clippy --no-self-update",
            "toolchain uninstall 1.90.0",
            "toolchain install 1.90.0 --profile minimal --component clippy --no-self-update",
        ],
        "{log}"
    );
    // The lock is released: a second run takes it at once.
    assert!(!f.cargo_home.path().join("rustup/.paddock-install-1.90.0.lock").exists());
    // ./dev's own install (a runner whose rustc does not run) checks and repairs the same way.
    let g = Fixture::new();
    let (shims, _) = rustup_half_installing(&g, true);
    let ensure = Command::new("sh")
        .arg(g.root().join("dev"))
        .arg("--ensure-toolchain")
        .current_dir(g.root())
        .env("PATH", format!("{}:{}", shims.display(), std::env::var("PATH").unwrap()))
        .env("CARGO_HOME", g.cargo_home.path())
        .env("RUSTUP_HOME", g.cargo_home.path().join("rustup"))
        .env_remove("RUSTC")
        .env_remove("CARGO_BUILD_RUSTC")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&ensure.stderr);
    assert_eq!(ensure.status.code(), Some(0), "{err}");
    assert!(err.contains("cargo clippy does not run") && err.contains("repaired"), "{err}");
}

/// A toolchain that a reinstall does not repair is an error with the next step (ER-27), never a job that runs on a broken compiler.
#[cfg(unix)]
#[test]
fn a_toolchain_a_reinstall_does_not_repair_is_an_error_with_the_next_step() {
    let f = Fixture::new();
    let (shims, _) = rustup_half_installing(&f, false);
    let out = install_toolchain(&f, &shims, &["1.90.0", "clippy", ""]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_ne!(out.status.code(), Some(0), "{err}");
    assert!(
        err.contains("::error::") && err.contains("cargo clippy does not run") && err.contains("rustup toolchain uninstall 1.90.0"),
        "{err}"
    );
    assert!(
        !f.cargo_home.path().join("rustup/.paddock-install-1.90.0.lock").exists(),
        "the lock is released on failure too"
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
    write_script(&odd, "echo 'not rustc'");
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
        write_script(&home.join(format!("bin/old-{i}/paddock-dev-test-host")), "exit 0");
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
        write_script(&home.join(format!("bin-source/old-{i}/paddock-dev-test-host")), "exit 0");
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

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
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
    let out = Command::new("sh")
        .arg("-x")
        .arg(f.root().join("dev"))
        .arg("--wrapper-hash")
        .current_dir(f.root())
        .env_remove("RUSTC")
        .env_remove("CARGO_BUILD_RUSTC")
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

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
#[cfg(unix)]
#[test]
fn on_a_ci_runner_with_a_kept_mount_the_tool_is_cached_there_and_an_explicit_home_still_wins() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let mount = scratch.path().join("mount");
    std::fs::create_dir_all(&mount).unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (hash, _) = f.both(&[("RUSTC", rustc.clone())]);
    write_script(&f.cargo_home.path().join("shims/cargo"), &shim_cargo(0));
    let path = format!("{}:{}", f.cargo_home.path().join("shims").display(), std::env::var("PATH").unwrap());
    let run = |extra: &[(&str, &Path)]| {
        let mut cmd = Command::new("sh");
        cmd.arg(f.root().join("dev"))
            .arg("hello")
            .current_dir(f.root())
            .env_clear()
            .env("PATH", &path)
            .env("HOME", scratch.path().join("home"))
            .env("CARGO_HOME", f.cargo_home.path())
            .env("RUSTC", &rustc)
            .env("GITHUB_ACTIONS", "true")
            .env("PADDOCK_DEV_PACKAGES", "http://127.0.0.1:9/generic")
            .env("PADDOCK_CI_MOUNT", &mount);
        cmd.envs(extra.iter().map(|(k, v)| (*k, *v)));
        let out = cmd.output().unwrap();
        (out.status.code().unwrap(), String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    assert_eq!(run(&[]), (0, "built-by-shim".to_string()));
    let cached = mount.join("paddock-dev/bin").join(&hash).join("paddock-dev-test-host");
    assert!(cached.exists(), "the tool is kept on the mount: {cached:?}");
    assert!(
        !mount.join(format!("paddock-dev/target-{hash}")).exists(),
        "the build folder (about 465 MB per hash) goes once the verified tool is published"
    );
    assert!(!scratch.path().join("home/.paddock-dev").exists(), "nothing in the runner's home");
    // The next run finds it without building.
    assert_eq!(run(&[]), (0, "built-by-shim".to_string()));
    assert!(!mount.join(format!("paddock-dev/target-{hash}")).exists(), "no second build");
    // An explicit home is used as given.
    let own = scratch.path().join("own");
    assert_eq!(run(&[("PADDOCK_DEV_HOME", own.as_path())]).0, 0);
    assert!(own.join("bin").join(&hash).exists());
}

/// A stand-in `cargo` that writes its binary, announces itself with a file of its own in `$BARRIER`, and returns only once the test
/// has created `$BARRIER/go`: the test decides when the builds overlap, no sleep does.
#[cfg(unix)]
fn barrier_cargo() -> String {
    format!(
        r#"{}; echo "$CARGO_TARGET_DIR" >> "$BARRIER.dirs"; touch "$BARRIER/built-$PADDOCK_DEV_HASH-$$"; n=0; while [ ! -e "$BARRIER/go" ] && [ $n -lt 300 ]; do sleep 0.1; n=$((n+1)); done"#,
        shim_cargo(0)
    )
}

/// Starts one wrapper per compiler against one kept mount; returns once `expected` builds are inside `cargo`, then lets them all go.
#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
#[cfg(unix)]
fn run_overlapping(rustcs: &[&str], expected: usize, shared_target: bool) -> (tempfile::TempDir, Vec<std::process::Output>, Fixture) {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let mount = scratch.path().join("mount");
    let barrier = scratch.path().join("barrier");
    std::fs::create_dir_all(&mount).unwrap();
    std::fs::create_dir_all(&barrier).unwrap();
    write_script(&f.cargo_home.path().join("shims/cargo"), &barrier_cargo());
    let path = format!("{}:{}", f.cargo_home.path().join("shims").display(), std::env::var("PATH").unwrap());
    let children: Vec<_> = rustcs
        .iter()
        .map(|rustc| {
            let mut cmd = Command::new("sh");
            cmd.arg(f.root().join("dev"))
                .arg("hello")
                .current_dir(f.root())
                .env_clear()
                .env("PATH", &path)
                .env("HOME", scratch.path().join("home"))
                .env("CARGO_HOME", f.cargo_home.path())
                .env("RUSTC", rustc)
                .env("GITHUB_ACTIONS", "true")
                .env("PADDOCK_DEV_PACKAGES", "http://127.0.0.1:9/generic")
                .env("PADDOCK_CI_MOUNT", &mount)
                .env("BARRIER", &barrier);
            // A host runner's fixed build folder (conformance/ci/job-isolation.sh sets PADDOCK_DEV_TARGET_DIR for every job).
            if shared_target {
                cmd.env("PADDOCK_DEV_TARGET_DIR", scratch.path().join("shared-target"));
            }
            cmd.output_child()
        })
        .collect();
    assert!(
        paddock_testkit::poll_until_within(std::time::Duration::from_secs(60), || std::fs::read_dir(&barrier).unwrap().count()
            >= expected),
        "the builds never overlapped"
    );
    std::fs::write(barrier.join("go"), "").unwrap();
    let outs = children.into_iter().map(|c| c.wait_with_output().unwrap()).collect();
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

#[cfg(unix)]
#[test]
fn two_wrappers_with_the_same_hash_on_one_kept_mount_both_succeed_and_leave_one_verified_binary() {
    let probe = Fixture::new();
    let tmp = tempfile::tempdir().unwrap();
    let (a, _) = rustcs(tmp.path());
    let (hash, _) = probe.both(&[("RUSTC", a.clone())]);
    let (scratch, outs, _f) = run_overlapping(&[&a, &a], 2, false);
    for o in &outs {
        assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "built-by-shim");
    }
    assert_cached(&scratch.path().join("mount"), &hash);
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
    write_script(
        &home.join("bin").join(&hash).join("paddock-dev-test-host"),
        "if [ \"$1\" = \"--version\" ]; then echo 0; fi",
    );
    set(&home.join("bin").join(&hash), hours(72));
    // Five newer tools push it past the keep count; build folders: one orphaned and old, one orphaned and fresh, one with a tool.
    for i in 0..5 {
        write_script(&home.join(format!("bin/newer-{i}/paddock-dev-test-host")), "exit 0");
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
    fn output_child(&mut self) -> std::process::Child;
}

#[cfg(unix)]
impl SpawnPiped for Command {
    fn output_child(&mut self) -> std::process::Child {
        use std::process::Stdio;
        self.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
#[cfg(unix)]
#[test]
fn a_failed_build_names_the_hash_and_the_next_step_in_an_annotation() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    write_script(&f.cargo_home.path().join("shims/cargo"), "echo boom >&2; exit 101");
    let path = format!("{}:{}", f.cargo_home.path().join("shims").display(), std::env::var("PATH").unwrap());
    let out = Command::new("sh")
        .arg(f.root().join("dev"))
        .current_dir(f.root())
        .env("PATH", path)
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

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
#[cfg(unix)]
#[test]
fn a_ci_run_without_a_kept_mount_says_so() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    write_script(&f.cargo_home.path().join("shims/cargo"), &shim_cargo(0));
    let path = format!("{}:{}", f.cargo_home.path().join("shims").display(), std::env::var("PATH").unwrap());
    let out = Command::new("sh")
        .arg(f.root().join("dev"))
        .current_dir(f.root())
        .env("PATH", path)
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
        write_script(&home.join(format!("bin/recent-{i}/paddock-dev-test-host")), "exit 0");
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

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (sh); nothing here ends a process by pid"
)]
#[cfg(unix)]
#[test]
fn print_binary_names_the_cached_tool_without_running_it() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (hash, _) = f.both(&[("RUSTC", rustc.clone())]);
    let (code, out, err) = {
        let mut cmd = Command::new("sh");
        write_script(&f.cargo_home.path().join("shims/cargo"), &shim_cargo(0));
        let path = format!("{}:{}", f.cargo_home.path().join("shims").display(), std::env::var("PATH").unwrap());
        cmd.arg(f.root().join("dev"))
            .arg("--print-binary")
            .current_dir(f.root())
            .env("PATH", path)
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
        write_script(&home.join(format!("bin/used-{i}/paddock-dev-test-host")), "exit 0");
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

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (sh); nothing here ends a process by pid"
)]
#[cfg(unix)]
#[test]
fn print_binary_refuses_a_build_with_uncommitted_inputs() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    std::fs::write(f.root().join("tools/paddock-dev/src/new_module.rs"), "// new\n").unwrap();
    let out = Command::new("sh")
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
    let body = format!(
        "n=$(wc -l < '{s}/cargo-runs' 2>/dev/null || echo 0); n=$((n+0)); [ $n -gt {cap} ] && n={cap}; printf 'rustc 1.9%s.0\\nbinary: rustc\\ncommit-hash: c%s\\ncommit-date: d\\nhost: test-host\\nrelease: 1.9%s.0\\nLLVM version: 1\\n' $n $n $n",
        s = state.display()
    );
    let p = dir.join("rustc-updating");
    write_script(&p, &body);
    p.display().to_string()
}

#[cfg(unix)]
fn run_with_updating_compiler(cap: u32) -> (i32, String, String, tempfile::TempDir) {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = updating_rustc(scratch.path(), cap);
    let state = scratch.path().join("state");
    let cargo = format!("echo run >> '{}/cargo-runs'; {}", state.display(), shim_cargo(0));
    let (code, out, err) = run_wrapper_with_cargo(&f, &scratch.path().join("home"), &[("RUSTC", rustc)], &cargo);
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

/// A `cargo` that builds a tool printing `what` (which may read the environment), after running `pre`.
#[cfg(unix)]
fn cargo_building(pre: &str, what: &str) -> String {
    format!(
        r#"{pre}
mkdir -p "$CARGO_TARGET_DIR/release"; printf '#!/bin/sh\nif [ "$1" = "--version" ]; then echo 0; exit 0; fi\nif [ "$1" = self ]; then echo "embedded %s"; exit 0; fi\n{what}\n' "$PADDOCK_DEV_HASH" > "$CARGO_TARGET_DIR/release/paddock-dev"; chmod +x "$CARGO_TARGET_DIR/release/paddock-dev""#
    )
}

#[cfg(unix)]
#[test]
fn the_restart_marker_is_not_passed_on_to_the_tool_so_a_nested_dev_can_restart() {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = updating_rustc(scratch.path(), 1);
    let state = scratch.path().join("state");
    let cargo = cargo_building(
        &format!("echo run >> '{}/cargo-runs'", state.display()),
        "echo restarted=${PADDOCK_DEV_RESTARTED:-unset}",
    );
    let (code, out, err) = run_wrapper_with_cargo(&f, &scratch.path().join("home"), &[("RUSTC", rustc)], &cargo);
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
    let first = cargo_building("touch \"$CARGO_TARGET_DIR/warm-marker\"", "echo first");
    let (code, _, err) = run_wrapper_with_cargo(&f, &home, &[("RUSTC", rustc.clone())], &first);
    assert_eq!(code, 0, "{err}");
    // A new commit under an input path: a new hash, the same compiler, so the same folder with what the first build left in it.
    std::fs::write(f.root().join("tools/paddock-dev/src/main.rs"), "fn main() { /* changed */ }\n").unwrap();
    git(f.root(), &["commit", "-q", "-am", "chore: change an input"]);
    let second = cargo_building(
        "[ -e \"$CARGO_TARGET_DIR/warm-marker\" ] || { echo cold >&2; exit 101; }",
        "echo second",
    );
    let (code, out, err) = run_wrapper_with_cargo(&f, &home, &[("RUSTC", rustc)], &second);
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
    let cargo = format!(
        r#"echo t >> '{t}'; if [ "$(wc -l < '{t}')" -eq 1 ]; then export PADDOCK_DEV_HASH=somebody-elses; fi
{b}"#,
        t = tries.display(),
        b = shim_cargo(0)
    );
    let (code, out, err) = run_wrapper_with_cargo(&f, &scratch.path().join("home"), &[("RUSTC", rustc)], &cargo);
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

/// The start time of a running process as the wrapper records it.
#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
#[cfg(unix)]
fn stamp_of(pid: u32) -> String {
    let out = Command::new("ps").args(["-o", "lstart=", "-p", &pid.to_string()]).output().unwrap();
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
    let (_, log) = rustup_without_toolchain(&f, "[ \"$1 $2\" = 'toolchain install' ] && exec sleep 30; exit 0");
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
    let (_, log) = rustup_without_toolchain(&f, "[ \"$1 $2\" = 'toolchain install' ] && touch 'MARKER'");
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

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts a short-lived sh to get a pid that has ended; nothing here ends a process by pid"
)]
#[cfg(unix)]
#[test]
fn a_lock_of_a_gone_owner_or_of_a_reused_pid_is_taken_over_whatever_its_age() {
    let me = std::process::id();
    let dead = {
        let mut c = Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
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
        let (_, log) = rustup_without_toolchain(&f, "[ \"$1 $2\" = 'toolchain install' ] && touch 'MARKER'");
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
    let (_, log) = rustup_without_toolchain(&f, "exit 1");
    let (code, _, err) = run_wrapper(&f, &scratch.path().join("home"), &[("PADDOCK_TOOLCHAIN_FAILED", "1".to_string())]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("an earlier step of this job failed to"), "{err}");
    assert!(!log.exists(), "rustup was not run at all");
}
