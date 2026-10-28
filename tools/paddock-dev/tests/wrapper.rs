//! The `./dev` wrapper and `dev self hash` compute one value, in shell and in Rust, from the same inputs.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::input_hash::{Real, compute};
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

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
struct Fixture {
    dir: tempfile::TempDir,
    cargo_home: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
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
            cargo_home: tempfile::tempdir().unwrap(),
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    /// The environment both implementations see (no user config: an empty CARGO_HOME).
    fn env(&self, extra: &[(&str, String)]) -> Vec<(String, String)> {
        let mut v = vec![("CARGO_HOME".to_string(), self.cargo_home.path().display().to_string())];
        v.extend(extra.iter().map(|(k, val)| (k.to_string(), val.clone())));
        v
    }

    fn shell(&self, env: &[(String, String)]) -> (String, bool) {
        let mut cmd = Command::new("sh");
        cmd.arg(self.root().join("dev"))
            .arg("--wrapper-hash")
            .current_dir(self.root().join("tools/paddock-dev/src"))
            .env_remove("RUSTC")
            .env_remove("CARGO_BUILD_RUSTC");
        cmd.envs(env.iter().cloned());
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let mut lines = text.lines();
        (lines.next().unwrap().to_string(), lines.next() == Some("dirty"))
    }

    fn rust(&self, env: &[(String, String)]) -> (String, bool) {
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let h = compute(&Real, &get, self.root()).unwrap();
        (h.hash, h.dirty)
    }

    fn both(&self, extra: &[(&str, String)]) -> (String, bool) {
        let env = self.env(extra);
        let (s, r) = (self.shell(&env), self.rust(&env));
        assert_eq!(s, r, "the wrapper and the tool must agree");
        s
    }
}

/// A "compiler" that prints a chosen `-vV`.
#[cfg(unix)]
fn fake_rustc(dir: &Path, name: &str, release: &str, commit: &str, host: &str) -> String {
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
