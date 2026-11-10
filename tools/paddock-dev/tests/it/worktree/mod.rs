//! `dev worktree` and `dev check primary` end to end with the real binary against throw-away repositories (ER-14): a bare
//! `origin`, a primary clone on `main`, and worktrees under one temporary folder. No test touches a real repository.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// Unix only: the tests start `sleep` and bash, and the in-use probe of `clean` reads other processes' folders, which is
// not implemented on Windows (there `clean` keeps everything). The Windows build is type-checked with `cargo xwin check`.
#![cfg(unix)]

mod concurrent;
mod list_clean;
mod ops;
mod oracle;
mod primary;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A bare origin, the primary checkout (cloned, on `main`, one commit pushed) and a folder for worktrees.
pub struct Fixture {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
    pub primary: PathBuf,
    pub wt: PathBuf,
    gitconfig: PathBuf,
}

fn run_of(out: std::process::Output) -> Run {
    let text = String::from_utf8_lossy(&out.stdout);
    Run {
        code: out.status.code().unwrap(),
        json: serde_json::from_str(text.trim()).unwrap_or(Value::Null),
        err: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// One result of the tool: exit code, the JSON object on stdout, standard error.
pub struct Run {
    pub code: i32,
    pub json: Value,
    pub err: String,
}

impl Fixture {
    pub fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let gitconfig = root.join("gitconfig");
        std::fs::write(
            &gitconfig,
            "[user]\n\tname = Test\n\temail = test@example.invalid\n[init]\n\tdefaultBranch = main\n[commit]\n\tgpgsign = false\n",
        )
        .unwrap();
        let fx = Fixture {
            primary: root.join("primary"),
            wt: root.join("wt"),
            root,
            gitconfig,
            _dir: dir,
        };
        std::fs::create_dir_all(&fx.wt).unwrap();
        fx.git(&fx.root, &["init", "-q", "--bare", "origin.git"]);
        fx.git(&fx.root, &["clone", "-q", "origin.git", "primary"]);
        std::fs::write(fx.primary.join("README.md"), "one\n").unwrap();
        fx.git(&fx.primary, &["add", "."]);
        fx.git(&fx.primary, &["commit", "-q", "-m", "first"]);
        fx.git(&fx.primary, &["push", "-q", "origin", "main"]);
        fx.git(&fx.primary, &["fetch", "-q", "origin"]);
        fx
    }

    // Starts the tool under test, git or a helper by program name (a throw-away fixture), never a process by pid (C53).
    #[allow(clippy::disallowed_methods)]
    fn git_cmd(&self, dir: &Path) -> Command {
        let mut c = Command::new("git");
        c.arg("-C")
            .arg(dir)
            .env("GIT_CONFIG_GLOBAL", &self.gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1");
        c
    }

    /// Runs git, which must succeed; returns its trimmed output.
    pub fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git_cmd(dir).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?} in {dir:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Runs git and reports whether it succeeded.
    pub fn git_ok(&self, dir: &Path, args: &[&str]) -> bool {
        self.git_cmd(dir).args(args).output().unwrap().status.success()
    }

    /// The tool as a command: run in `cwd`, with the test's git config (ER-14: never the user's), the worktree root set to
    /// this fixture's folder and no agent identity in the environment.
    pub fn cmd(&self, cwd: &Path, args: &[&str]) -> Command {
        let mut c = self.cmd_text(cwd, args);
        c.arg("--json");
        c
    }

    /// Like [`Fixture::cmd`] without `--json` (the text a person reads).
    // Starts the tool under test, git or a helper by program name (a throw-away fixture), never a process by pid (C53).
    #[allow(clippy::disallowed_methods)]
    pub fn cmd_text(&self, cwd: &Path, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_paddock-dev"));
        c.args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_GLOBAL", &self.gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("PADDOCK_WORKTREE_ROOT", &self.wt)
            .env_remove("PADDOCK_AGENT_ID")
            .env_remove("PADDOCK_AGENT_TYPE");
        c
    }

    /// The tool, run in `cwd`.
    pub fn dev_in(&self, cwd: &Path, args: &[&str]) -> Run {
        run_of(self.cmd(cwd, args).output().unwrap())
    }

    /// The tool, run in the primary.
    pub fn dev(&self, args: &[&str]) -> Run {
        self.dev_in(&self.primary, args)
    }

    /// A `git` script first on the PATH (the seam that slows or fails one git call): `body` is shell run before the real git
    /// with `$REAL` the real binary and the arguments in `$*`; falling off the end runs the real git. Returns the PATH.
    #[cfg(unix)]
    // Starts the tool under test, git or a helper by program name (a throw-away fixture), never a process by pid (C53).
    #[allow(clippy::disallowed_methods)]
    pub fn fake_git(&self, name: &str, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let real = String::from_utf8(Command::new("which").arg("git").output().unwrap().stdout).unwrap();
        let bin = self.root.join(format!("fakebin-{name}"));
        std::fs::create_dir_all(&bin).unwrap();
        let script = format!("#!/bin/sh\nREAL={}\n{body}\nexec \"$REAL\" \"$@\"\n", real.trim());
        std::fs::write(bin.join("git"), script).unwrap();
        std::fs::set_permissions(bin.join("git"), std::fs::Permissions::from_mode(0o755)).unwrap();
        format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
    }

    /// A fake `git` whose `worktree add` tells the test it has been reached, then holds for three seconds. The tool reserves
    /// the path before it starts git, so the signal means the reservation is in place; the test waits on it instead of
    /// polling the owner map (ER-12). Returns the `PATH` to give the tool and the receiver of the signal.
    // Starts the tool under test, git or a helper by program name (a throw-away fixture), never a process by pid (C53).
    #[allow(clippy::disallowed_methods)]
    pub fn fake_slow_add(&self, name: &str) -> (String, std::sync::mpsc::Receiver<()>) {
        let fifo = self.root.join(format!("reached-{name}.fifo"));
        assert!(Command::new("mkfifo").arg(&fifo).status().unwrap().success());
        let body = format!(
            r#"case "$*" in *"worktree add"*) echo reached > '{}'; sleep 3;; esac"#,
            fifo.display()
        );
        let path_env = self.fake_git(name, &body);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // Opening a fifo for reading blocks until the fake git opens it for writing; the caller bounds the wait.
            let _ = std::fs::read_to_string(&fifo);
            let _ = tx.send(());
        });
        (path_env, rx)
    }

    /// Waits until the `worktree add` of [`Fixture::fake_slow_add`] is inside its git call, at most ten seconds.
    pub fn wait_inside_add(&self, reached: &std::sync::mpsc::Receiver<()>) {
        reached
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the add never reached its git call");
        assert!(
            self.owners()["pending"].as_object().is_some_and(|m| !m.is_empty()),
            "the add reached git without a reservation"
        );
    }

    /// The path of a worktree named `name`.
    pub fn path(&self, name: &str) -> PathBuf {
        self.wt.join(name)
    }

    /// `worktree add` as the coordinator (no agent), a new branch `name` from `origin/main`; must succeed.
    pub fn add(&self, name: &str) -> PathBuf {
        let p = self.path(name);
        let r = self.dev(&[
            "worktree",
            "add",
            name,
            "--base",
            "origin/main",
            "--owner",
            &format!("implementer:{name}"),
        ]);
        assert_eq!(r.code, 0, "{} {}", r.json, r.err);
        p
    }

    /// `worktree claim` as `agent`.
    pub fn claim(&self, p: &Path, agent: &str) -> Run {
        self.dev(&["worktree", "claim", p.to_str().unwrap(), "--agent", agent])
    }

    /// The owner map as JSON (null when there is none).
    pub fn owners(&self) -> Value {
        let f = self.primary.join(".git/paddock-worktree-owners.json");
        std::fs::read_to_string(f)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null)
    }

    /// The owner of `p` in the map, if any.
    pub fn owner_of(&self, p: &Path) -> Option<String> {
        self.owners()["owners"][p.to_str().unwrap()]["agent_id"]
            .as_str()
            .map(str::to_string)
    }

    /// `git worktree list --porcelain` of the primary.
    pub fn porcelain(&self) -> String {
        self.git(&self.primary, &["worktree", "list", "--porcelain"])
    }

    /// The lock reason of the worktree at `p`, if it is locked.
    pub fn lock_reason(&self, p: &Path) -> Option<String> {
        let text = self.porcelain();
        let block = text
            .split("\n\n")
            .find(|b| b.lines().next() == Some(&format!("worktree {}", p.display())))?;
        block.lines().find_map(|l| {
            l.strip_prefix("locked ")
                .map(str::to_string)
                .or_else(|| (l == "locked").then(String::new))
        })
    }

    /// Commits a new file in the worktree `p`.
    pub fn commit_in(&self, p: &Path, file: &str) {
        std::fs::write(p.join(file), "x\n").unwrap();
        self.git(p, &["add", file]);
        self.git(p, &["commit", "-q", "-m", &format!("add {file}")]);
    }

    /// Makes `branch` part of `origin/main` (the way a landing would) and fetches it.
    pub fn land(&self, branch: &str) {
        let tip = self.git(&self.primary, &["rev-parse", branch]);
        self.git(&self.primary, &["push", "-q", "origin", &format!("{tip}:refs/heads/main")]);
        self.git(&self.primary, &["fetch", "-q", "origin"]);
    }
}

#[test]
fn the_fixture_is_a_real_repository_with_origin_main() {
    let fx = Fixture::new();
    assert_eq!(fx.git(&fx.primary, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(
        fx.git(&fx.primary, &["rev-parse", "origin/main"]),
        fx.git(&fx.primary, &["rev-parse", "main"])
    );
}
