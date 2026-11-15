//! The launcher as a program, on a throw-away repository: the hash it prints (goldens made by the shell `./dev` it replaces, run on
//! the same tree, environment and configs), where it looks for the repository and how little it needs on `PATH` (run 8138: a
//! curated `PATH` without `grep`, `tr` or `touch` broke the shell on Windows).
#![expect(clippy::unwrap_used, reason = "tests")]
#![expect(clippy::disallowed_methods, reason = "tests start the program under test and git")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The launcher program: `DEV_LAUNCH_BIN` when set (the Windows test VM has no toolchain: the program and this test are built on the
/// Mac and the compiled-in Mac path means nothing there), else the one cargo built for this test.
fn launcher_bin() -> PathBuf {
    std::env::var_os("DEV_LAUNCH_BIN").map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_dev-launch")), PathBuf::from)
}

/// A scratch folder in the machine's temp folder (found at run time, for the same reason), removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    #[expect(
        clippy::disallowed_methods,
        reason = "this crate has no scratch helper (it is std only); the folder is removed on drop"
    )]
    fn new() -> Scratch {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!("launcher-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&p).unwrap();
        Scratch(p)
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let f = self.0.join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, text).unwrap();
        f
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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

/// A compiler that prints a fixed `-vV`.
fn fake_rustc(s: &Scratch) -> PathBuf {
    let lines = [
        "rustc 1.90.0",
        "binary: rustc",
        "commit-hash: aaaa0000bbbb1111cccc2222dddd3333eeee4444",
        "commit-date: d",
        "host: test-host",
        "release: 1.90.0",
        "LLVM version: 1",
    ];
    if cfg!(windows) {
        let body: String = lines.iter().map(|l| format!("echo {l}\r\n")).collect();
        s.write("rustc.cmd", &format!("@echo off\r\n{body}"))
    } else {
        let body: String = lines.iter().map(|l| format!("{l}\\n")).collect();
        let p = s.write("rustc", &format!("#!/bin/sh\nprintf '{body}'\n"));
        #[cfg(unix)]
        std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        p
    }
}

/// The repository every golden was made on: committed, with the inputs listed in `inputs.txt`.
fn repo(s: &Scratch) -> PathBuf {
    s.write(
        "repo/tools/paddock-dev/inputs.txt",
        "# inputs\ntools/paddock-dev\ncrates/a\n.cargo/config.toml\n",
    );
    s.write("repo/tools/paddock-dev/src/main.rs", "fn main() {}\n");
    s.write("repo/crates/a/lib.rs", "pub fn a() {}\n");
    s.write("repo/.cargo/config.toml", "# placeholder\n");
    let root = s.0.join("repo");
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "fixture"]);
    root
}

/// The launcher in `cwd` with an environment of its own (nothing of the test run's), the compiler `rustc` and an empty `CARGO_HOME`.
fn launcher(s: &Scratch, cwd: &Path, rustc: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(launcher_bin());
    c.args(args)
        .current_dir(cwd)
        .env_clear()
        .env("RUSTC", rustc)
        .env("CARGO_HOME", s.0.join("ch"))
        .env("PADDOCK_DEV_HOME", s.0.join("home"));
    for k in [
        "PATH",
        "SystemRoot",
        "SYSTEMROOT",
        "PATHEXT",
        "ComSpec",
        "USERPROFILE",
        "TEMP",
        "TMP",
    ] {
        if let Some(v) = std::env::var_os(k) {
            c.env(k, v);
        }
    }
    c
}

fn text(o: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n"),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// The shell `./dev --wrapper-hash` of the commit before this change printed these on this tree (a repository with the files of
/// [`repo`], the compiler of [`fake_rustc`]): the launcher must print the same, byte for byte, or every cached tool changes name.
const PLAIN: &str = "6454b45ed55de8d51ec1ec2b89e08149aabbbccb";
const RUSTFLAGS: &str = "b2529faca08fc5c78238ee8718b056dc1ef6fe29";
const REGISTRY_CONFIG: &str = "b0c6440722d11bea5a7467adee2af47fb39e50d1";

#[test]
fn the_hash_is_the_one_the_shell_wrapper_printed() {
    let s = Scratch::new();
    let root = repo(&s);
    let rustc = fake_rustc(&s);
    let run = |extra: &[(&str, &str)]| {
        let mut c = launcher(&s, &root, &rustc, &["--wrapper-hash"]);
        c.envs(extra.iter().copied());
        let out = c.output().unwrap();
        assert!(out.status.success(), "{}", text(&out).1);
        text(&out).0
    };
    assert_eq!(run(&[]), format!("{PLAIN}\nclean\n"));
    assert_eq!(run(&[("RUSTFLAGS", "-C target-cpu=native")]), format!("{RUSTFLAGS}\nclean\n"));
    s.write("ch/config.toml", "[registry]\ntoken = \"dummy\"\n");
    assert_eq!(
        run(&[]),
        format!("{REGISTRY_CONFIG}\nclean\n"),
        "a cargo config is part of the hash"
    );
    s.write("ch/config.toml", "[build]\ntarget-dir = \"/x\"\n");
    assert_eq!(
        run(&[]),
        format!("{PLAIN}\nclean\n"),
        "a config that only names the build folder is not"
    );
    // An uncommitted change to an input: the same hash (HEAD's) and the word that makes the caller build.
    std::fs::write(root.join("crates/a/lib.rs"), "pub fn a() {}\npub fn b() {}\n").unwrap();
    assert_eq!(run(&[]), format!("{PLAIN}\ndirty\n"));
}

#[test]
fn the_log_line_names_the_compiler_the_variables_and_the_config_digests_but_never_a_value() {
    let s = Scratch::new();
    let root = repo(&s);
    let rustc = fake_rustc(&s);
    s.write("ch/config.toml", "[registry]\ntoken = \"never-in-a-log\"\n");
    let out = launcher(&s, &root, &rustc, &["--wrapper-hash"])
        .env("RUSTFLAGS", "-Cvalue-not-printed")
        .output()
        .unwrap();
    let err = text(&out).1;
    assert!(
        err.contains("inputs: commit-hash: aaaa0000bbbb1111cccc2222dddd3333eeee4444 release: 1.90.0| env: RUSTFLAGS | configs: "),
        "{err}"
    );
    assert!(err.contains("| digests: "), "{err}");
    assert!(
        !err.contains("never-in-a-log") && !err.contains("value-not-printed"),
        "a value reached the log: {err}"
    );
}

#[test]
fn outside_a_repository_it_says_so_and_exits_2() {
    let s = Scratch::new();
    let rustc = fake_rustc(&s);
    let outside = s.0.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let out = launcher(&s, &outside, &rustc, &["--wrapper-hash"])
        .env("GIT_CEILING_DIRECTORIES", &s.0)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out).1.contains("dev: run this inside the Paddock repository."),
        "{}",
        text(&out).1
    );
}

#[test]
fn a_planted_git_dir_and_work_tree_do_not_move_it_to_another_repository() {
    let s = Scratch::new();
    let root = repo(&s);
    let rustc = fake_rustc(&s);
    let other = s.0.join("other");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q"]);
    let out = launcher(&s, &root, &rustc, &["--wrapper-hash"])
        .env("GIT_DIR", other.join(".git"))
        .env("GIT_WORK_TREE", &other)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out).1);
    assert_eq!(text(&out).0, format!("{PLAIN}\nclean\n"));
}

#[test]
fn a_checkout_without_the_tool_is_told_how_to_add_it() {
    let s = Scratch::new();
    let root = repo(&s);
    let rustc = fake_rustc(&s);
    std::fs::remove_file(root.join("tools/paddock-dev/inputs.txt")).unwrap();
    let out = launcher(&s, &root, &rustc, &["--wrapper-hash"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out).1.contains("git sparse-checkout add tools"), "{}", text(&out).1);
}

/// Run 8138's shape: a `PATH` that holds only `git` (a folder with nothing else in it). The launcher needs no `grep`, `sed`, `tr`,
/// `awk`, `touch` or `ps` to print the hash, and its compiler is named by an absolute path.
#[cfg(unix)]
#[test]
fn it_needs_nothing_on_path_but_git() {
    let s = Scratch::new();
    let root = repo(&s);
    let rustc = fake_rustc(&s);
    let real_git = String::from_utf8(Command::new("sh").args(["-c", "command -v git"]).output().unwrap().stdout).unwrap();
    // A shim, not a link: git finds its helpers from where it runs, and only this one name is on the path.
    let only = s.write("only/git", &format!("#!/bin/sh\nexec {} \"$@\"\n", real_git.trim()));
    std::fs::set_permissions(&only, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let out = launcher(&s, &root, &rustc, &["--wrapper-hash"])
        .env("PATH", only.parent().unwrap())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out).1);
    assert_eq!(text(&out).0, format!("{PLAIN}\nclean\n"));
}

/// Run 8138's shape on Windows: a `PATH` of only the folder of `git.exe`, with none of Git for Windows' `usr/bin` tools.
#[cfg(windows)]
#[test]
fn it_needs_nothing_on_path_but_git() {
    let s = Scratch::new();
    let root = repo(&s);
    let rustc = fake_rustc(&s);
    let git_dir = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .find(|d| d.join("git.exe").is_file())
        .expect("git.exe on PATH");
    let out = launcher(&s, &root, &rustc, &["--wrapper-hash"])
        .env("PATH", git_dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out).1);
    assert_eq!(text(&out).0, format!("{PLAIN}\nclean\n"));
}
