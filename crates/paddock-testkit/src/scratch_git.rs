//! [`scratch_git`]: the one way a test runs git in a scratch repository (ER-21, ER-45, ER-47; the Rust twin of
//! conformance/test/lib/scratch-git.mjs).
//!
//! A test that starts git with a plain `Command` inherits the caller's environment, and under a git hook (the pre-push gate runs
//! the tests) or a shell with `GIT_DIR` and `GIT_WORK_TREE` set, `git init`, `git config` or `git checkout -b` in a temporary folder
//! wrote into the real repository (2026-11-19: `core.worktree` and `user.*` landed in the shared `.git/config`).
//!
//! [`ScratchGit`] starts git as a [`TestCommand`] does, from an empty environment plus the listed set (no `GIT_*` variable, a private
//! global git configuration, no system configuration), adds `-c init.defaultBranch=main`, and refuses before git starts:
//!
//! - an environment variable that selects another repository or configuration (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_COMMON_DIR`, `GIT_NAMESPACE`, `GIT_CONFIG_COUNT`, ...); the child inherits no `GIT_*` variable from the test process, and a caller may set the others (`GIT_AUTHOR_NAME`, `GIT_CONFIG_GLOBAL`, ...);
//! - git's global options `-C`, `--git-dir`, `--work-tree` and `--namespace` (both spellings): name the folder with
//!   [`ScratchGit::current_dir`];
//! - `config --file`, `-f`, `--global`, `--system`; the options `--separate-git-dir`, `--template`, `--reference` and
//!   `--upload-pack`; a configuration key that moves the work tree or the hooks (`core.worktree`, `core.hooksPath`, `include.path`);
//! - any path argument (absolute, `~` or with a `..` segment, also after `--opt=` or in a `file://` URL) that resolves outside the
//!   temp folder: `clone` into, `init` in, `worktree add` at or `bundle create` of such a path;
//! - for every verb that is not a read (`ls-files`, `rev-parse`, `log`, `show`, `diff`, `cat-file`, `check-ignore`, `check-attr`,
//!   `rev-list`, `ls-tree`, `for-each-ref`, `merge-base`, `describe`, `grep`, `count-objects`, `status --no-optional-locks`): a
//!   working folder that is not set or is not inside the temp folder. A read may run in the real tree (`current_dir(repo_root)`),
//!   but `--output` is refused for it.
//!
//! A refusal is an `io::Error` from `output`, `status` or `spawn` (a test's `unwrap` shows it) naming the argument and the next step.

use crate::process::{PrivateGuard, TestChild, TestCommand};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};

/// Verbs that only read (`status` only with `--no-optional-locks`).
const READ_ONLY: &[&str] = &[
    "ls-files",
    "rev-parse",
    "log",
    "show",
    "diff",
    "cat-file",
    "check-ignore",
    "check-attr",
    "rev-list",
    "ls-tree",
    "for-each-ref",
    "merge-base",
    "describe",
    "grep",
    "count-objects",
    "status",
];

/// Variables that select another repository, object store or configuration: a caller may not set them (the child gets none).
const REPOSITORY_VARS: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
];

/// A `git` for a scratch repository; see the module documentation.
pub struct ScratchGit {
    cmd: TestCommand,
    args: Vec<OsString>,
    cwd: Option<PathBuf>,
    /// The first `GIT_*` variable the caller tried to set.
    refused_env: Option<String>,
}

/// A `git` that runs in a scratch repository: set its folder with [`ScratchGit::current_dir`].
pub fn scratch_git() -> ScratchGit {
    let mut cmd = TestCommand::new("git");
    cmd.args([
        "-c",
        "init.defaultBranch=main",
        "-c",
        "commit.gpgsign=false",
    ]);
    ScratchGit {
        cmd,
        args: Vec::new(),
        cwd: None,
        refused_env: None,
    }
}

impl ScratchGit {
    /// Adds an argument (checked when the command starts).
    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut ScratchGit {
        self.args.push(arg.as_ref().to_os_string());
        self.cmd.arg(arg);
        self
    }

    /// Adds arguments (checked when the command starts).
    pub fn args<I, S>(&mut self, args: I) -> &mut ScratchGit
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for a in args {
            self.arg(a);
        }
        self
    }

    /// The folder git runs in: inside the temp folder for every verb that writes; any folder for a read.
    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut ScratchGit {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self.cmd.current_dir(dir);
        self
    }

    /// Sets an environment variable of the child; one that selects another repository or configuration is refused when the command
    /// starts (see the module documentation).
    pub fn env(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut ScratchGit {
        let name = key.as_ref().to_string_lossy().to_ascii_uppercase();
        if REPOSITORY_VARS.contains(&name.as_str())
            || name.starts_with("GIT_CONFIG_KEY_")
            || name.starts_with("GIT_CONFIG_VALUE_")
        {
            self.refused_env.get_or_insert(name);
        }
        self.cmd.env(key, value);
        self
    }

    /// Removes an environment variable from the child.
    pub fn env_remove(&mut self, key: impl AsRef<OsStr>) -> &mut ScratchGit {
        self.cmd.env_remove(key);
        self
    }

    /// The child's standard input.
    pub fn stdin(&mut self, cfg: impl Into<Stdio>) -> &mut ScratchGit {
        self.cmd.stdin(cfg);
        self
    }

    /// The child's standard output.
    pub fn stdout(&mut self, cfg: impl Into<Stdio>) -> &mut ScratchGit {
        self.cmd.stdout(cfg);
        self
    }

    /// The child's standard error.
    pub fn stderr(&mut self, cfg: impl Into<Stdio>) -> &mut ScratchGit {
        self.cmd.stderr(cfg);
        self
    }
}

impl ScratchGit {
    /// Refuses what would let git act outside the temp folder (see the module documentation).
    fn check(&self) -> io::Result<()> {
        if let Some(k) = &self.refused_env {
            return Err(refusal(format!(
                "{k} selects a repository or configuration; a scratch git never gets one"
            )));
        }
        let temp = temp_root();
        refuse_escaping(self.cwd.as_deref(), &self.args, &temp).map_err(refusal)
    }

    /// Runs git to its end and collects its output.
    ///
    /// # Errors
    ///
    /// When the arguments or the folder would leave the temp folder (see the module documentation), or git cannot be started.
    pub fn output(&mut self) -> io::Result<Output> {
        self.check()?;
        self.cmd.output()
    }

    /// Runs git to its end with inherited standard streams.
    ///
    /// # Errors
    ///
    /// As [`ScratchGit::output`].
    pub fn status(&mut self) -> io::Result<ExitStatus> {
        self.check()?;
        self.cmd.status()
    }

    /// The checked `Command` and the guard that keeps its private folders, for a caller that runs it with its own bound
    /// (`paddock_proc::run_bounded`).
    ///
    /// # Errors
    ///
    /// As [`ScratchGit::output`].
    pub fn into_parts(self) -> io::Result<(Command, PrivateGuard)> {
        self.check()?;
        Ok(self.cmd.into_parts())
    }

    /// Starts git.
    ///
    /// # Errors
    ///
    /// As [`ScratchGit::output`].
    pub fn spawn(&mut self) -> io::Result<TestChild> {
        self.check()?;
        self.cmd.spawn()
    }
}

fn refusal(why: String) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("scratch_git refused: {why}"),
    )
}

/// The temp folder, resolved (macOS: `/var` is a link to `/private/var`).
fn temp_root() -> PathBuf {
    let t = std::env::temp_dir();
    paddock_env::winpath::real(&t).unwrap_or(t)
}

/// Whether `path` (absolute, or relative to `base`) lies inside `temp`, through its nearest existing ancestor; `temp` itself is
/// not inside.
fn inside(temp: &Path, base: &Path, path: &Path) -> bool {
    let mut cur = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    let mut rest: Vec<OsString> = Vec::new();
    loop {
        if let Ok(real) = paddock_env::winpath::real(&cur) {
            let full = rest.iter().rev().fold(real, |a, c| a.join(c));
            return full.starts_with(temp) && full != temp;
        }
        match (
            cur.file_name().map(OsStr::to_os_string),
            cur.parent().map(Path::to_path_buf),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                cur = parent;
            }
            _ => return false,
        }
    }
}

/// The path an argument names when it looks like one (absolute, `~`, a `file://` URL or a `..` segment; `--opt=path` is read too).
fn path_of(arg: &str) -> Option<String> {
    let mut a = arg;
    if let Some(rest) = a.strip_prefix("--").and_then(|r| r.split_once('=')) {
        a = rest.1;
    }
    let a = a.strip_prefix("file://").unwrap_or(a);
    if a == "/dev/null" || a == "NUL" {
        return None;
    }
    let climbs = Path::new(a).components().any(|c| c == Component::ParentDir);
    (a.starts_with('~') || Path::new(a).is_absolute() || climbs).then(|| a.to_string())
}

const GLOBAL_OPTIONS: &[&str] = &[
    "-C",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--exec-path",
    "--super-prefix",
    "--config-env",
];
const CONFIG_FILE_OPTIONS: &[&str] = &[
    "--global",
    "--system",
    "--file",
    "-f",
    "--blob",
    "--worktree",
];
const PLACEMENT_OPTIONS: &[&str] = &[
    "--separate-git-dir",
    "--template",
    "--reference",
    "--reference-if-able",
    "--upload-pack",
    "-u",
];
const DANGEROUS_KEYS: &[&str] = &["core.worktree", "core.hookspath", "include.path"];

fn is_option(arg: &str, names: &[&str]) -> bool {
    names
        .iter()
        .any(|n| arg == *n || arg.strip_prefix(n).is_some_and(|r| r.starts_with('=')))
}

/// Why git started with `args` in `cwd` could act outside `temp`, or `Ok`. Pure: the tests give their own `temp`.
fn refuse_escaping(cwd: Option<&Path>, args: &[OsString], temp: &Path) -> Result<(), String> {
    let args: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let line = args.join(" ");
    let refuse = |why: &str, arg: &str| -> Result<(), String> {
        Err(format!(
            "{why} (`git {line}`): {arg} could leave the scratch folder; run it with `current_dir` set to a folder inside the temp folder"
        ))
    };
    // The global options come before the verb (`-c key=value` pairs are skipped).
    let mut verb_at = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "-c" {
            i += 2;
            continue;
        }
        if is_option(a, GLOBAL_OPTIONS) {
            return refuse("a global option", a);
        }
        if !a.starts_with('-') {
            verb_at = Some(i);
            break;
        }
        i += 1;
    }
    let verb = verb_at.map_or("", |v| args[v].as_str());
    let reads = READ_ONLY.contains(&verb)
        && (verb != "status" || args.iter().any(|a| a == "--no-optional-locks"));
    for (n, a) in args.iter().enumerate() {
        let lower = a.to_ascii_lowercase();
        if DANGEROUS_KEYS.iter().any(|k| lower.contains(k)) || lower.contains("includeif.") {
            return refuse(
                "a configuration key that moves the work tree or the hooks",
                a,
            );
        }
        if verb_at.is_some_and(|v| n > v) {
            if verb == "config" && is_option(a, CONFIG_FILE_OPTIONS) {
                return refuse("`git config` naming another file", a);
            }
            if (verb == "init" || verb == "clone") && is_option(a, PLACEMENT_OPTIONS) {
                return refuse("a git dir, template or object store placed elsewhere", a);
            }
            if reads && (a == "--output" || a == "-o" || a.starts_with("--output=")) {
                return refuse("a read that writes a file", a);
            }
        }
        if n > 0 && args[n - 1] == "-c" {
            continue; // the value of a `-c key=value` pair: the key was checked above
        }
        if !reads
            && let Some(p) = path_of(a)
            && !inside(temp, cwd.unwrap_or(Path::new("")), Path::new(&p))
        {
            return refuse("a path outside the temp folder", a);
        }
    }
    if !reads {
        match cwd {
            Some(c) if inside(temp, Path::new(""), c) => {}
            Some(c) => {
                return Err(format!(
                    "the folder {} is not inside the temp folder {} (`git {line}`); a git that writes runs in a scratch folder",
                    c.display(),
                    temp.display()
                ));
            }
            None => {
                return Err(format!(
                    "no `current_dir` set (`git {line}`); a git that writes needs a folder inside the temp folder"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    /// A temp folder with an `inside` folder, and another folder beside it that stands for "outside".
    fn world() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let temp = paddock_env::winpath::real(root.path())
            .unwrap()
            .join("temp");
        let outside = paddock_env::winpath::real(root.path())
            .unwrap()
            .join("outside");
        std::fs::create_dir_all(temp.join("work")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        (root, temp, outside)
    }

    #[test]
    fn the_global_options_that_name_another_repository_are_refused_in_both_spellings() {
        let (_root, temp, outside) = world();
        let work = temp.join("work");
        let o = outside.display().to_string();
        for args in [
            vec!["-C", o.as_str(), "status"],
            vec!["--git-dir", o.as_str(), "status"],
            vec!["--git-dir=/x/.git", "status"],
            vec!["--work-tree", o.as_str(), "status"],
            vec!["--work-tree=/x", "status"],
            vec!["--namespace", "x", "status"],
            vec!["--namespace=x", "status"],
            vec!["-c", "user.name=t", "-C", "..", "status"],
        ] {
            assert!(
                refuse_escaping(Some(&work), &argv(&args), &temp).is_err(),
                "git {args:?}"
            );
        }
    }

    #[test]
    fn config_files_placements_keys_and_outside_paths_are_refused() {
        let (_root, temp, outside) = world();
        let work = temp.join("work");
        let o = outside.display().to_string();
        let target = outside.join("cfg").display().to_string();
        let clone_to = outside.join("clone").display().to_string();
        let wt = outside.join("wt").display().to_string();
        for args in [
            vec!["config", "--file", target.as_str(), "a.b", "c"],
            vec!["config", "-f", target.as_str(), "a.b", "c"],
            vec!["config", "--global", "a.b", "c"],
            vec!["config", "--system", "a.b", "c"],
            vec!["clone", "-q", "src", clone_to.as_str()],
            vec!["clone", "-q", "src", "../../../escape"],
            vec!["clone", "-q", "--separate-git-dir", "sep", "src", "dest"],
            vec!["init", "-q", o.as_str()],
            vec!["init", "-q", "--separate-git-dir=sep"],
            vec!["worktree", "add", wt.as_str()],
            vec!["config", "core.worktree", "x"],
            vec!["-c", "core.worktree=x", "status"],
            vec!["-c", "core.hooksPath=x", "status"],
            vec!["config", "include.path", "x"],
            vec!["diff", "--output=/x"],
        ] {
            assert!(
                refuse_escaping(Some(&work), &argv(&args), &temp).is_err(),
                "git {args:?}"
            );
        }
    }

    #[test]
    fn a_folder_outside_the_temp_folder_is_refused_for_a_write_and_allowed_for_a_read() {
        let (_root, temp, outside) = world();
        assert!(refuse_escaping(Some(&outside), &argv(&["init", "-q"]), &temp).is_err());
        assert!(
            refuse_escaping(None, &argv(&["commit", "-m", "x"]), &temp).is_err(),
            "no folder set"
        );
        assert!(refuse_escaping(Some(&outside), &argv(&["ls-files", "-z"]), &temp).is_ok());
        assert!(refuse_escaping(Some(&outside), &argv(&["rev-parse", "HEAD"]), &temp).is_ok());
        assert!(
            refuse_escaping(Some(&outside), &argv(&["status", "--porcelain"]), &temp).is_err(),
            "status writes the index"
        );
        assert!(
            refuse_escaping(
                Some(&outside),
                &argv(&["--no-optional-locks", "status", "--porcelain"]),
                &temp
            )
            .is_ok()
        );
        assert!(
            refuse_escaping(Some(&temp), &argv(&["init", "-q"]), &temp).is_err(),
            "the temp folder itself is not a scratch folder"
        );
    }

    #[test]
    fn paths_inside_the_temp_folder_still_work() {
        let (_root, temp, _outside) = world();
        let work = temp.join("work");
        let inside = temp.join("work").join("new").display().to_string();
        let bundle = temp.join("x.bundle").display().to_string();
        let url = ["file://", temp.join("src").display().to_string().as_str()].concat();
        for args in [
            vec!["init", "-q"],
            vec!["clone", "-q", url.as_str(), inside.as_str()],
            vec!["bundle", "create", bundle.as_str(), "HEAD"],
            vec!["config", "--local", "core.autocrlf", "false"],
            vec![
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "x",
            ],
            vec!["show", "HEAD:crates/a/lib.rs"],
            vec!["checkout", "-q", "--", "a.txt"],
        ] {
            assert!(
                refuse_escaping(Some(&work), &argv(&args), &temp).is_ok(),
                "git {args:?}"
            );
        }
    }

    #[test]
    fn a_git_variable_is_refused_when_the_command_starts() {
        let mut g = scratch_git();
        g.env("GIT_DIR", "/x").arg("--version");
        let e = g.output().unwrap_err();
        assert!(
            e.to_string().contains("GIT_DIR selects a repository"),
            "{e}"
        );
    }

    /// The behavioural control: a child test process started with `GIT_DIR` and `GIT_WORK_TREE` naming a stand-in "real" repository
    /// runs the scratch git; that repository's config, HEAD and refs do not change. The same child with a raw `git config` does change
    /// them, so the probe can see a leak (ER-33).
    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "the control starts this test binary and a raw git under a hostile environment on purpose"
    )]
    fn under_a_hostile_git_dir_the_scratch_git_leaves_that_repository_alone() {
        if std::env::var_os("PADDOCK_SCRATCH_GIT_CHILD").is_some() {
            return; // the child run below is the probe, see the next test
        }
        let real = tempfile::tempdir().unwrap();
        let init = std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(real.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(init.status.success());
        let snapshot = || {
            (
                std::fs::read_to_string(real.path().join(".git/config")).unwrap(),
                std::fs::read_to_string(real.path().join(".git/HEAD")).unwrap(),
                std::fs::read_dir(real.path().join(".git/refs/heads"))
                    .unwrap()
                    .count(),
            )
        };
        let probe = |mode: &str| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "scratch_git::tests::hostile_child_probe",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("PADDOCK_SCRATCH_GIT_CHILD", mode)
                .env("GIT_DIR", real.path().join(".git"))
                .env("GIT_WORK_TREE", real.path())
                .env("GIT_INDEX_FILE", real.path().join(".git/index"))
                .output()
                .unwrap()
        };
        let before = snapshot();
        let out = probe("scratch");
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            snapshot(),
            before,
            "the repository GIT_DIR names is untouched by the scratch git"
        );
        let out = probe("raw");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_ne!(
            snapshot().0,
            before.0,
            "control: a raw git under the same environment does write into it"
        );
    }

    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "the control starts a raw git under a hostile environment on purpose"
    )]
    fn hostile_child_probe() {
        let Some(mode) = std::env::var_os("PADDOCK_SCRATCH_GIT_CHILD") else {
            return;
        };
        let work = tempfile::tempdir().unwrap();
        if mode == "raw" {
            let o = std::process::Command::new("git")
                .args(["config", "user.name", "leak"])
                .current_dir(work.path())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            assert!(o.status.success());
            return;
        }
        for args in [
            vec!["init", "-q"],
            vec!["config", "core.autocrlf", "false"],
            vec!["commit", "-q", "--allow-empty", "-m", "one"],
            vec!["checkout", "-q", "-b", "topic"],
        ] {
            let o = scratch_git()
                .current_dir(work.path())
                .args(args.clone())
                .output()
                .unwrap();
            assert!(
                o.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        let o = scratch_git()
            .current_dir(work.path())
            .args(["rev-parse", "--absolute-git-dir"])
            .output()
            .unwrap();
        let dir = String::from_utf8_lossy(&o.stdout).trim().to_string();
        assert!(
            Path::new(&dir).starts_with(temp_root())
                || dir.contains(&*work.path().file_name().unwrap().to_string_lossy()),
            "{dir}"
        );
    }
}
