//! [`TestCommand`]: the one way a test starts `./dev`, `cargo` or `node` (ADR 0100, check C123).
//!
//! A child started with a plain `Command` inherits the machine's whole environment, so a new environment lookup in the program
//! under test (`ENGINEERING_RULES`, `HEAVY_LOCK_DIR`, `HOME`, a token) quietly reaches real machine state: the pre-push tests ran
//! the real heavy steps, and `dev()` found the real `heavy.sh` and took machine-wide slots in `~/.cache/heavy-slots` (ER-14).
//! `TestCommand` starts every child from an **empty** environment (`env_clear`) plus a listed set:
//!
//! - `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, the `XDG_*` folders, `TMPDIR`, `TEMP` and `TMP` point into a private
//!   folder made for this command ([`TestCommand::private_dir`]), removed when the command drops;
//! - `PATH` is curated: a folder of links to the toolchain's programs (`cargo`, `rustc`, `rustup`, ...; never the folder they are
//!   in, where `cargo install` puts other tools), shared under the build's target folder or the command's own, then the folders
//!   of `node`, `npm`, `npx` and `git` on the captured `PATH` ([`crate::ambient`]), on Windows Git for Windows' `usr/bin` after git's own folder, then the system folders (`/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`: `lsof` and `ps` are there), nothing else; [`TestCommand::path_first`] puts a test's own stand-in folder first, and `env("PATH", ..)`
//!   replaces it whole;
//! - the toolchain variables cargo needs (`RUSTUP_HOME`, `CARGO_HOME`, `RUSTUP_TOOLCHAIN`) and, on Windows, the few the OS
//!   needs (`SystemRoot`, ...): [`PASSED_THROUGH`], from the machine values captured once per process ([`crate::ambient`]),
//!   never read live;
//! - a private global git config, `.gitconfig` in the private home, which git finds through `HOME` (no `GIT_CONFIG_GLOBAL`, so a test
//!   that names another home gets that home's `.gitconfig`, as on a machine): `maintenance.auto=false`, `gc.auto=0`, a test identity
//!   and `safe.directory=*` (the system config, where CI's image names it, is not read), so no test's git starts background maintenance;
//!   written only when the home has none (a `new_in` home the test prepared keeps its own); `GIT_CEILING_DIRECTORIES` names the temp
//!   root, so the trust stops there (CVE-2022-24765);
//! - fixed values that make git deterministic: an author and committer identity, no system git config.
//!
//! Everything else a test needs is a named addition (`.env("NAME", value)`), which is also what a reader of the test sees.
//! [`allowed_names`] lists every name a fresh command carries; the tests of this module compare a real child's environment with
//! it, so a variable added here without being listed fails.
//!
//! The command wraps a [`std::process::Command`] and has its own `arg`, `args`, `current_dir`, `env`, `output`, `spawn` and the rest;
//! it derefs to the `Command` read-only (`get_program`, `get_envs`), never mutably, so nothing can reach past the listed set.
//!
//! The program is resolved once, when the command is made: a bare name (`node`, `git`) becomes the full path found on the
//! **captured** `PATH` (with the captured `PATHEXT` on Windows), except `cargo`, `rustc` and `rustup`, which start through the
//! toolchain links (`cargo` is the cargo that runs the tests: `CARGO`, else the toolchain's own from `rustc --print sysroot`),
//! never rustup's proxy, which puts all of `$CARGO_HOME/bin` in front of its child's PATH. [`TestCommand::path_first`] and `env("PATH", ..)` change only the child's own lookups, never which program
//! starts: to start a stand-in, name it by its path.
//!
//! A child [`TestCommand::spawn`] starts is a [`TestChild`], which ends that child through its handle when it drops. What the child
//! starts below it is not the testkit's to end (it has no `paddock_proc`): start the command through
//! `paddock_testkit_start::Reaper::spawn_test` when it does (ADR 0100 D8, ADR 0103).

use std::ffi::{OsStr, OsString};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::Arc;

use crate::ambient::{Ambient, ambient};

/// Variables copied from the test process when it has them: the toolchain cargo and rustup need to find themselves, and what
/// Windows needs to start any program. Nothing ambient (no tokens, proxies, `CI`, `GITHUB_*`, `PADDOCK_*`, user variables).
pub const PASSED_THROUGH: &[&str] = &[
    "RUSTUP_HOME",
    "CARGO_HOME",
    "RUSTUP_TOOLCHAIN",
    // A bound on the compiles a child cargo starts (ER-24), not state: the machine's value keeps a test's build as small as the run's.
    "CARGO_BUILD_JOBS",
    "SystemRoot",
    "SYSTEMROOT",
    "windir",
    "SystemDrive",
    "ComSpec",
    "PATHEXT",
    "PROCESSOR_ARCHITECTURE",
];

/// The variables that decide how a child `cargo build` compiles: a test that builds a program must compile it as the run that
/// started the test does (coverage instrumentation, flags, a compiler wrapper), or the program under test differs from the run's.
/// Named additions made by [`TestCommand::cargo_build`], not part of a fresh command.
pub const BUILD_VARS: &[&str] = &[
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "LLVM_PROFILE_FILE",
    "CARGO_LLVM_COV",
    "CARGO_LLVM_COV_TARGET_DIR",
    "CARGO_INCREMENTAL",
];

/// Fixed values every command carries, so git runs the same on every machine and never reads the user's or the system's config.
const FIXED: &[(&str, &str)] = &[
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_AUTHOR_NAME", "Paddock Test"),
    ("GIT_AUTHOR_EMAIL", "test@example.invalid"),
    ("GIT_COMMITTER_NAME", "Paddock Test"),
    ("GIT_COMMITTER_EMAIL", "test@example.invalid"),
    ("NO_COLOR", "1"),
];

/// The private global git config: no background maintenance or gc, a test identity, every folder trusted (see the module).
const GITCONFIG: &str = "[maintenance]\n\tauto = false\n[gc]\n\tauto = 0\n[user]\n\tname = Paddock Test\n\temail = test@example.invalid\n[safe]\n\tdirectory = *\n";

/// The folders the private home and temp variables name (see [`TestCommand::private_dir`]).
const REDIRECTED: &[&str] = &[
    "GIT_CEILING_DIRECTORIES",
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
    "TMPDIR",
    "TEMP",
    "TMP",
];

/// Every variable name a fresh [`TestCommand`] can carry: the redirected folders, `PATH`, the fixed values and
/// [`PASSED_THROUGH`]. A test's own additions come on top of these and are named in the test.
#[must_use]
pub fn allowed_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = REDIRECTED.to_vec();
    names.push("PATH");
    names.extend(FIXED.iter().map(|(k, _)| *k));
    names.extend(PASSED_THROUGH.iter().copied());
    names.sort_unstable();
    names.dedup();
    names
}

/// A [`Command`] that starts from an empty environment plus the listed one (see the module documentation).
#[derive(Debug)]
pub struct TestCommand {
    cmd: Command,
    /// The environment a fresh command carries: what [`TestCommand::env_clear`] goes back to.
    base: Vec<(OsString, OsString)>,
    private: Arc<PrivateDir>,
    /// The toolchain link folder this command's `PATH` starts with, when the machine has a toolchain.
    links: Option<PathBuf>,
    /// Git for Windows' install folder, when one was found (always `None` elsewhere).
    git_root: Option<PathBuf>,
    /// The machine values this command was made from.
    ambient: Ambient,
}

#[derive(Debug)]
enum PrivateDir {
    Own(tempfile::TempDir),
    Given(PathBuf),
}

impl PrivateDir {
    fn path(&self) -> &Path {
        match self {
            PrivateDir::Own(d) => d.path(),
            PrivateDir::Given(p) => p,
        }
    }
}

impl TestCommand {
    /// A command for `program` with its own private home and temp folders. A bare name (`node`, `cargo`, `git`) is looked up on
    /// this process's `PATH` once, here, and started by its full path; the child's own `PATH` is the curated one.
    ///
    /// # Panics
    ///
    /// When the private folder cannot be made (the test cannot run safely without it).
    #[must_use]
    pub fn new(program: impl AsRef<OsStr>) -> TestCommand {
        TestCommand::on(program, ambient())
    }

    /// Like [`new`](TestCommand::new), with the private home and temp folders under `dir` (a folder the test owns and removes), for a
    /// test that reads what the child wrote there or starts several children that share a home.
    ///
    /// # Panics
    ///
    /// When `dir` cannot be made.
    #[must_use]
    pub fn new_in(program: impl AsRef<OsStr>, dir: &Path) -> TestCommand {
        TestCommand::on_in(program, dir, ambient())
    }

    /// Like [`new`](TestCommand::new), from the machine values `ambient` instead of this process's ([`crate::ambient::ambient`]):
    /// for a test that needs a stand-in toolchain on `PATH` or its own link folder, without changing the process environment.
    ///
    /// # Panics
    ///
    /// When the private folder cannot be made.
    #[must_use]
    pub fn on(program: impl AsRef<OsStr>, ambient: &Ambient) -> TestCommand {
        let dir = tempfile::Builder::new()
            .prefix("paddock-test-cmd-")
            .tempdir_in(&ambient.temp_dir)
            .unwrap_or_else(|e| panic!("TestCommand: cannot make its private folder: {e}"));
        TestCommand::build(program.as_ref(), PrivateDir::Own(dir), ambient)
    }

    /// [`on`](TestCommand::on) with the private folders under `dir`, as [`new_in`](TestCommand::new_in).
    ///
    /// # Panics
    ///
    /// When `dir` cannot be made.
    #[must_use]
    pub fn on_in(program: impl AsRef<OsStr>, dir: &Path, ambient: &Ambient) -> TestCommand {
        std::fs::create_dir_all(dir)
            .unwrap_or_else(|e| panic!("TestCommand: cannot make {}: {e}", dir.display()));
        TestCommand::build(
            program.as_ref(),
            PrivateDir::Given(dir.to_path_buf()),
            ambient,
        )
    }

    fn build(program: &OsStr, private: PrivateDir, machine: &Ambient) -> TestCommand {
        // `cargo`, `rustc` and `rustup` start through the toolchain links (`cargo`: the cargo that runs this test), never rustup's
        // proxy in `$CARGO_HOME/bin`, which puts all of that folder in front of its child's PATH (review of 0916c8060).
        let curated = curated(machine);
        // The shared folder of the build's target folder; a test program run outside one (copied elsewhere), or one whose
        // target folder refused it, links into its own private folder instead, removed with it.
        let links = if curated.shared {
            curated.links.clone()
        } else {
            link_folder(machine, &private.path().join("toolchain"))
                .ok()
                .flatten()
        };
        let linked = (Path::new(program).components().count() == 1
            && TOOLCHAIN_STARTED.contains(&program.to_string_lossy().as_ref()))
        .then(|| {
            links
                .iter()
                .chain(&curated.toolchain_dirs)
                .find_map(|d| linked_program(d, program))
        })
        .flatten();
        let resolved = linked.map_or_else(|| resolve_on(program, machine), PathBuf::into_os_string);
        // `paddock_env::command::command` is the one place a program is started from (no console window on Windows, as the
        // product's children), and `paddock_proc::command` is the same function; C123 forbids a test's own `Command::new`.
        let mut cmd = paddock_env::command::command(resolved);
        cmd.env_clear();
        let root = private.path().to_path_buf();
        let (home, tmp) = (root.join("home"), root.join("tmp"));
        for d in [&home, &tmp] {
            // A failure surfaces as the child's own error about its home.
            let _ = std::fs::create_dir_all(d);
        }
        // The private global git config, read through HOME (GITCONFIG): no background maintenance or gc (a detached `git maintenance` holds
        // `objects/maintenance.lock` and a bare clone made meanwhile copies it: the kept-clone fixture hit that 104 times in 1200 under
        // load), a test identity, and every folder trusted, since GIT_CONFIG_NOSYSTEM hides the system config where the CI image and the
        // local Linux container name `safe.directory` (a worktree mounted there belongs to another user).
        // Only when the home has none, and whole: written to a temporary file, then linked into place, which never replaces a
        // file (a test that prepared a `new_in` home keeps its own; children sharing one home never see half a file).
        let config = home.join(".gitconfig");
        if !config.exists()
            && let Ok(mut staged) = tempfile::NamedTempFile::new_in(&home)
            && std::io::Write::write_all(&mut staged, GITCONFIG.as_bytes()).is_ok()
        {
            // `persist_noclobber` links the file into place and fails, leaving the other one, when one appeared meanwhile.
            let _ = staged.persist_noclobber(&config);
        }
        // With every folder trusted, git must not look for a repository above the temp root (CVE-2022-24765: a `.git` another user
        // planted in a shared `/tmp`); a repository a test made below it is still found.
        let vars: [(&str, PathBuf); 12] = [
            ("GIT_CEILING_DIRECTORIES", machine.temp_dir.clone()),
            ("HOME", home.clone()),
            ("USERPROFILE", home.clone()),
            ("APPDATA", home.join("AppData/Roaming")),
            ("LOCALAPPDATA", home.join("AppData/Local")),
            ("XDG_CONFIG_HOME", home.join(".config")),
            ("XDG_DATA_HOME", home.join(".local/share")),
            ("XDG_CACHE_HOME", home.join(".cache")),
            ("XDG_STATE_HOME", home.join(".local/state")),
            ("TMPDIR", tmp.clone()),
            ("TEMP", tmp.clone()),
            ("TMP", tmp),
        ];
        for (k, v) in vars {
            cmd.env(k, v);
        }
        for (k, v) in FIXED {
            cmd.env(k, v);
        }
        for k in PASSED_THROUGH {
            if let Some(v) = machine.var(k) {
                cmd.env(k, v);
            }
        }
        // With HOME redirected, cargo and rustup would look for their folders under the private home and find nothing (offline
        // registry, toolchains): name the real ones, the only place a child reads the user's folders, and only the toolchain's.
        for (k, folder) in [("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")] {
            if machine.var(k).is_none()
                && let Some(real) = machine
                    .home
                    .as_ref()
                    .map(|h| h.join(folder))
                    .filter(|p| p.is_dir())
            {
                cmd.env(k, real);
            }
        }
        let path: Vec<PathBuf> = links
            .iter()
            .cloned()
            .chain(curated.toolchain_dirs.iter().cloned())
            .chain(curated.tail.iter().cloned())
            .collect();
        cmd.env("PATH", join_paths(&path));
        let base = cmd
            .get_envs()
            .filter_map(|(k, v)| v.map(|v| (k.to_os_string(), v.to_os_string())))
            .collect();
        TestCommand {
            cmd,
            base,
            private: Arc::new(private),
            links,
            git_root: curated.git_root,
            ambient: machine.clone(),
        }
    }

    /// The private folder of this command: `home/` and `tmp/` are in it.
    #[must_use]
    pub fn private_dir(&self) -> &Path {
        self.private.path()
    }

    /// The private home folder (`HOME`).
    #[must_use]
    pub fn home(&self) -> PathBuf {
        self.private.path().join("home")
    }

    /// Puts `dir` first on the child's `PATH` (a folder of stand-in programs); the curated folders follow. A second call puts its folder in front of the first.
    /// It changes the child's own lookups only: the program this command starts was resolved when it was made (see the module).
    pub fn path_first(&mut self, dir: &Path) -> &mut TestCommand {
        let mut first: Vec<PathBuf> = vec![dir.to_path_buf()];
        if let Some((_, Some(v))) = self.cmd.get_envs().find(|(k, _)| *k == "PATH") {
            first.extend(std::env::split_paths(v));
        }
        self.cmd.env("PATH", join_paths(&first));
        self
    }

    /// Sets `CARGO_TARGET_DIR` to the build folder this test program was built into, for a child `cargo build` that must put its
    /// output where the test looks for it (`cargo_layout::example_bin`, `sibling_bin`).
    ///
    /// # Panics
    ///
    /// When the test program is not in a cargo build folder (see [`crate::cargo_layout::profile_dir`]).
    pub fn cargo_target_dir(&mut self) -> &mut TestCommand {
        let profile = crate::cargo_layout::profile_dir();
        let target = profile
            .parent()
            .unwrap_or_else(|| panic!("{} has no target folder", profile.display()))
            .to_path_buf();
        self.cmd.env("CARGO_TARGET_DIR", target);
        self
    }

    /// Adds an argument.
    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut TestCommand {
        self.cmd.arg(arg);
        self
    }

    /// Adds arguments.
    pub fn args<I, S>(&mut self, args: I) -> &mut TestCommand
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.cmd.args(args);
        self
    }

    /// Sets the child's working folder.
    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut TestCommand {
        self.cmd.current_dir(dir);
        self
    }

    /// A named addition to the child's environment (or a replacement of a listed value, `PATH` included).
    pub fn env(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut TestCommand {
        self.cmd.env(key, value);
        self
    }

    /// Named additions.
    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut TestCommand
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.cmd.envs(vars);
        self
    }

    /// Removes a variable from the child's environment (a listed one, or an addition made earlier).
    pub fn env_remove(&mut self, key: impl AsRef<OsStr>) -> &mut TestCommand {
        self.cmd.env_remove(key);
        self
    }

    /// Goes back to the environment a fresh command carries: the additions made so far are dropped, the listed set is never
    /// erased (a plain `Command::env_clear` would empty it, and the child would then read the machine's `PATH` for its lookups).
    pub fn env_clear(&mut self) -> &mut TestCommand {
        self.cmd.env_clear();
        for (k, v) in &self.base {
            self.cmd.env(k, v);
        }
        self
    }

    /// The child's standard input.
    pub fn stdin(&mut self, cfg: impl Into<Stdio>) -> &mut TestCommand {
        self.cmd.stdin(cfg);
        self
    }

    /// The child's standard output.
    pub fn stdout(&mut self, cfg: impl Into<Stdio>) -> &mut TestCommand {
        self.cmd.stdout(cfg);
        self
    }

    /// The child's standard error.
    pub fn stderr(&mut self, cfg: impl Into<Stdio>) -> &mut TestCommand {
        self.cmd.stderr(cfg);
        self
    }

    /// Runs the child to its end and collects its output.
    ///
    /// # Errors
    ///
    /// When the program cannot be started.
    pub fn output(&mut self) -> std::io::Result<Output> {
        crate::exe::output(&mut self.cmd)
    }

    /// Runs the child to its end with inherited standard streams.
    ///
    /// # Errors
    ///
    /// When the program cannot be started.
    pub fn status(&mut self) -> std::io::Result<ExitStatus> {
        crate::exe::status(&mut self.cmd)
    }

    /// Starts the child. The returned [`TestChild`] keeps the private folders until it is waited on or dropped, whatever happens to
    /// this command, and kills and reaps a child that is still running when it drops.
    ///
    /// # Errors
    ///
    /// When the program cannot be started.
    pub fn spawn(&mut self) -> std::io::Result<TestChild> {
        let child = crate::exe::spawn(&mut self.cmd)?;
        Ok(TestChild {
            child: Some(child),
            private: Arc::clone(&self.private),
        })
    }

    /// For a child `cargo build` of the program a test looks for: [`cargo_target_dir`](TestCommand::cargo_target_dir) and the
    /// [`BUILD_VARS`] the test run has (`RUSTFLAGS`, `RUSTC_WRAPPER`, the coverage variables).
    pub fn cargo_build(&mut self) -> &mut TestCommand {
        self.cargo_target_dir();
        for k in BUILD_VARS {
            if let Some(v) = self.ambient.var(k) {
                self.cmd.env(k, v);
            }
        }
        self
    }

    /// The plain command, for a runner that takes a [`Command`] by value (`paddock_proc::run_bounded`), and the guard that keeps the
    /// private folder until it is dropped: hold the guard for as long as the child runs.
    #[must_use]
    pub fn into_parts(self) -> (Command, PrivateGuard) {
        (self.cmd, PrivateGuard(self.private))
    }

    /// The environment the child will start with, as the command holds it now.
    #[must_use]
    pub fn environment(&self) -> Vec<(OsString, OsString)> {
        self.cmd
            .get_envs()
            .filter_map(|(k, v)| v.map(|v| (k.to_os_string(), v.to_os_string())))
            .collect()
    }
}

/// A child started by [`TestCommand::spawn`]: it derefs to the [`Child`], holds the private folders of its command for as long as it
/// lives, and kills and reaps itself when dropped while still running (through the handle it owns, never a bare pid, ADR 0033).
#[derive(Debug)]
pub struct TestChild {
    child: Option<Child>,
    private: Arc<PrivateDir>,
}

impl TestChild {
    /// A child started from a [`TestCommand`] taken apart with [`TestCommand::into_parts`] (a runner that starts the plain command
    /// itself, such as `paddock_testkit_start::Reaper::spawn_test`), with the guard that keeps its private folder.
    #[must_use]
    pub fn from_parts(child: Child, guard: PrivateGuard) -> TestChild {
        TestChild {
            child: Some(child),
            private: guard.0,
        }
    }

    /// The private folder of the command that started this child.
    #[must_use]
    pub fn private_dir(&self) -> &Path {
        self.private.path()
    }

    /// Waits for the child to end and collects its output; the private folders go when this guard drops at the end of the call.
    ///
    /// # Errors
    ///
    /// When waiting fails.
    ///
    /// # Panics
    ///
    /// Never: the child is present until this call takes it.
    pub fn wait_with_output(mut self) -> std::io::Result<Output> {
        #[allow(clippy::expect_used)]
        let child = self
            .child
            .take()
            .expect("the child is present until waited on");
        child.wait_with_output()
    }
}

impl Deref for TestChild {
    type Target = Child;
    #[allow(clippy::expect_used)]
    fn deref(&self) -> &Child {
        self.child
            .as_ref()
            .expect("the child is present until waited on")
    }
}

impl DerefMut for TestChild {
    #[allow(clippy::expect_used)]
    fn deref_mut(&mut self) -> &mut Child {
        self.child
            .as_mut()
            .expect("the child is present until waited on")
    }
}

impl Drop for TestChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take()
            && matches!(child.try_wait(), Ok(None))
        {
            // A handle this guard owns, not a pid read earlier: the child is ended and reaped before its folders are removed.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Keeps the private folder of a [`TestCommand`] that was taken apart with [`TestCommand::into_parts`].
#[derive(Debug)]
pub struct PrivateGuard(Arc<PrivateDir>);

impl Deref for TestCommand {
    type Target = Command;
    fn deref(&self) -> &Command {
        &self.cmd
    }
}

/// The toolchain's programs a child may start by name. They are linked into a private folder of the command (symbolic links on
/// Unix, where rustup's proxies dispatch on the name they were started as; hard links, else copies, on Windows), and that folder
/// joins the curated `PATH`, never the folder they came from: `cargo install` puts other tools beside them in `$CARGO_HOME/bin`
/// (`heavy`; review of 7edb8cad6, P1-1, which reopened ADR 0100's first incident).
const TOOLCHAIN: &[&str] = &[
    "cargo",
    "rustc",
    "rustup",
    "cargo-clippy",
    "cargo-fmt",
    "rustfmt",
    "clippy-driver",
    "rustdoc",
];

/// The other programs whose folder, as the test process finds it, joins the curated `PATH`. On Windows Git for Windows' `usr/bin`
/// joins too, after git's own folder, found from `git --exec-path` ([`git_root`]); the POSIX tools are there or in the system
/// folders.
pub(crate) const NAMED_TOOLS: &[&str] = &["node", "npm", "npx", "git"];

/// What decided a [`TestCommand`]'s `PATH` (ER-27): put it in a failure message, so a test that finds a tool missing says which
/// folders the child had, what the machine's `PATH` was and where the tools were found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathReport {
    /// The child's `PATH`, folder by folder, as the command holds it now.
    pub child: Vec<PathBuf>,
    /// The test process's own `PATH`.
    pub ambient: Vec<PathBuf>,
    /// Where `git`, `sh`, `bash`, `node`, `cargo`, `rustc` and `rustup` are on the test process's `PATH` (`None`: not found).
    pub found: Vec<(&'static str, Option<PathBuf>)>,
    /// Git for Windows' install folder, when one was found (always `None` elsewhere).
    pub git_root: Option<PathBuf>,
    /// The toolchain links of the folder the command was built with.
    pub toolchain: Vec<PathBuf>,
    /// Each link and the program it stands for, as recorded when the link was made (a hard link or a copy on Windows does not
    /// name its program; the record does).
    pub targets: Vec<(PathBuf, PathBuf)>,
}

impl std::fmt::Display for PathReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let list = |v: &[PathBuf]| {
            v.iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" ; ")
        };
        writeln!(f, "child PATH:   {}", list(&self.child))?;
        writeln!(f, "machine PATH: {}", list(&self.ambient))?;
        for (name, at) in &self.found {
            writeln!(
                f,
                "{name}: {}",
                at.as_ref()
                    .map_or_else(|| "not found".to_string(), |p| p.display().to_string())
            )?;
        }
        writeln!(
            f,
            "Git for Windows: {}",
            self.git_root
                .as_ref()
                .map_or_else(|| "none".to_string(), |p| p.display().to_string())
        )?;
        write!(f, "toolchain links: {}", list(&self.toolchain))?;
        for (link, target) in &self.targets {
            write!(f, "\n  {} -> {}", link.display(), target.display())?;
        }
        Ok(())
    }
}

impl TestCommand {
    /// What decided this command's `PATH` (see [`PathReport`]): the folders and the Git root the command was built with, and
    /// where the tools are on the machine's `PATH` now. A query: it starts nothing and changes nothing.
    #[must_use]
    pub fn path_report(&self) -> PathReport {
        let ambient = self.ambient.path.clone();
        let child = self
            .cmd
            .get_envs()
            .find(|(k, _)| *k == "PATH")
            .and_then(|(_, v)| v)
            .map(|v| std::env::split_paths(v).collect())
            .unwrap_or_default();
        let found_at = |name: &str| {
            Some(PathBuf::from(resolve_on(OsStr::new(name), &self.ambient)))
                .filter(|p| p.is_absolute())
        };
        let toolchain = self
            .links
            .as_deref()
            .and_then(|d| std::fs::read_dir(d).ok())
            .map(|d| {
                d.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.file_name() != Some(OsStr::new(LINKS)))
                    .collect()
            })
            .unwrap_or_default();
        // The record the folder was made with: `<link name>\t<program>` per line.
        let targets = self
            .links
            .as_deref()
            .and_then(|d| {
                std::fs::read_to_string(d.join(LINKS))
                    .ok()
                    .map(|t| (d.to_path_buf(), t))
            })
            .map(|(d, t)| {
                t.lines()
                    .skip(1)
                    .filter_map(|l| l.split_once('\t'))
                    .map(|(name, target)| (d.join(name), PathBuf::from(target)))
                    .collect()
            })
            .unwrap_or_default();
        PathReport {
            child,
            ambient: ambient
                .as_deref()
                .map(|v| std::env::split_paths(v).collect())
                .unwrap_or_default(),
            found: ["git", "sh", "bash", "node", "cargo", "rustc", "rustup"]
                .into_iter()
                .map(|n| (n, found_at(n)))
                .collect(),
            git_root: self.git_root.clone(),
            toolchain,
            targets,
        }
    }
}

/// The file in a link folder that records each link and its program, `<link name>\t<program>` per line, written when the links
/// are made; [`TestCommand::path_report`] reads it.
const LINKS: &str = "links.txt";

/// The bare program names a [`TestCommand`] starts through its toolchain link, never through the machine's `PATH`.
const TOOLCHAIN_STARTED: &[&str] = &["cargo", "rustc", "rustup"];

/// What the curated `PATH` is made of for one set of machine values: worked out once per test process and reused by every
/// command (ci.yml run 8152: worked out per command, Windows tests that start many children ran about 6 times slower).
#[derive(Clone, Debug)]
struct Curated {
    /// The shared toolchain link folder under [`Ambient::link_root`], when the machine has a toolchain.
    links: Option<PathBuf>,
    /// Whether `links` is the shared folder's answer; `false` when there is no link root or it refused the folder, and each
    /// command then links into its own folder.
    shared: bool,
    /// On Windows, toolchain folders that go on the `PATH` as they are, not linked ([`toolchain_dirs`]).
    toolchain_dirs: Vec<PathBuf>,
    /// The folders after them: the [`NAMED_TOOLS`]' (on Windows Git's `usr/bin` after git's own folder), then the system folders.
    tail: Vec<PathBuf>,
    /// Git for Windows' install folder, when one was found.
    git_root: Option<PathBuf>,
}

/// The [`Curated`] parts for `machine`, worked out on first use; a cached link folder that is gone or no longer valid (another
/// process removed it, an older format) is made again, never left off the PATH.
fn curated(machine: &Ambient) -> Curated {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Key = (Option<OsString>, Option<PathBuf>, Option<PathBuf>);
    static SEEN: OnceLock<Mutex<HashMap<Key, Curated>>> = OnceLock::new();
    let key: Key = (
        machine.path.clone(),
        machine.cargo.clone(),
        machine.link_root.clone(),
    );
    let mut seen = SEEN
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(c) = seen.get(&key)
        && c.links.as_deref().is_none_or(folder_is_valid)
    {
        return c.clone();
    }
    let (links, shared) = match machine.link_root.as_deref() {
        None => (None, false),
        Some(root) => match link_folder(machine, root) {
            Ok(links) => (links, true),
            Err(e) => {
                log_once(&format!(
                    "TestCommand: cannot make the toolchain link folder under {} ({e}); each command links into its own folder",
                    root.display()
                ));
                (None, false)
            }
        },
    };
    let git_root = git_root_of(machine);
    let made = Curated {
        tail: tail_dirs(machine, links.as_deref(), git_root.as_deref()),
        toolchain_dirs: toolchain_dirs(machine),
        links,
        shared,
        git_root,
    };
    seen.insert(key, made.clone());
    made
}

/// Prints `line` on stderr the first time it is given in this process.
fn log_once(line: &str) {
    use std::sync::{Mutex, OnceLock};
    static SAID: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut said = SAID
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !said.iter().any(|l| l == line) {
        eprintln!("{line}");
        said.push(line.to_string());
    }
}

/// The link to `program` in the link folder (with `.exe` on Windows), when there is one.
fn linked_program(dir: &Path, program: &OsStr) -> Option<PathBuf> {
    let mut name = program.to_os_string();
    name.push(std::env::consts::EXE_SUFFIX);
    Some(dir.join(name)).filter(|p| p.exists())
}

/// The first line of a link folder's [`LINKS`] record: a folder without it, or with another, is made again.
const LINKS_HEADER: &str = "# paddock-test-toolchain links v2";

/// Whether `dir` is a complete link folder of the current format: its record starts with [`LINKS_HEADER`] and every link it
/// names is there.
fn folder_is_valid(dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join(LINKS)) else {
        return false;
    };
    let mut lines = text.lines();
    lines.next() == Some(LINKS_HEADER)
        && lines.all(|l| {
            l.split_once('\t')
                .is_some_and(|(name, _)| dir.join(name).exists())
        })
}

/// The [`TOOLCHAIN`] programs `machine` has, each with the name its link gets (`cargo`: [`Ambient::cargo`], the cargo that runs
/// the tests).
fn toolchain_sources(machine: &Ambient) -> Vec<(PathBuf, String)> {
    TOOLCHAIN
        .iter()
        .map(|name| {
            let source = match &machine.cargo {
                Some(c) if *name == "cargo" => c.clone(),
                _ => PathBuf::from(resolve_on(OsStr::new(name), machine)),
            };
            (source, format!("{name}{}", std::env::consts::EXE_SUFFIX))
        })
        .filter(|(p, _)| p.is_absolute() && p.is_file())
        .collect()
}

/// On Windows, the folders of [`toolchain_sources`] that are a toolchain's own `bin` (they hold `rustc_driver-*.dll`): its
/// programs load their DLLs from their own folder, so a hard link or a copy elsewhere does not start (`rustc -vV` exited
/// 0xC0000135, STATUS_DLL_NOT_FOUND, on the test VM, whose toolchain has no rustup). Such a folder holds only the toolchain's
/// programs, never a `cargo install`ed tool, so it goes on the `PATH` as it is. Empty elsewhere (a symbolic link starts the program
/// from its own folder) and for rustup's proxies, which need no DLL.
fn toolchain_dirs(machine: &Ambient) -> Vec<PathBuf> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    for (source, _) in toolchain_sources(machine) {
        let Some(dir) = source.parent() else { continue };
        let holds_driver = std::fs::read_dir(dir).is_ok_and(|entries| {
            entries.flatten().any(|e| {
                let n = e.file_name().to_string_lossy().to_lowercase();
                n.starts_with("rustc_driver-") && n.ends_with(".dll")
            })
        });
        if holds_driver && !dirs.iter().any(|d| d == dir) {
            dirs.push(dir.to_path_buf());
        }
    }
    dirs
}

/// The folder of links to the [`TOOLCHAIN`] programs `machine` has (`cargo`: [`Ambient::cargo`], the cargo that runs the
/// tests), under `root`: one folder per toolchain, named by the programs' paths, sizes and times, made once (in a staging folder
/// of this process, then renamed into place) and reused by every later command and test process that use the same `root`.
/// Symbolic links on Unix; on Windows hard links, else a copy, logged with its error. `Ok(None)` when the machine has none of
/// the programs; `Err` when `root`, a link or the record cannot be written (the caller links into the command's own folder).
fn link_folder(machine: &Ambient, root: &Path) -> std::io::Result<Option<PathBuf>> {
    use std::hash::{Hash, Hasher};
    // Each link is named for its program plus the platform's suffix (`rustc.exe`), never after the spelling that found it
    // (`rustc.EXE` through `PATHEXT`; ci.yml run 8154), so `linked_program` finds it by the same name on every file system.
    let in_place = toolchain_dirs(machine);
    let (sources, names): (Vec<PathBuf>, Vec<String>) = toolchain_sources(machine)
        .into_iter()
        .filter(|(p, _)| p.parent().is_none_or(|d| !in_place.iter().any(|t| t == d)))
        .unzip();
    if sources.is_empty() {
        return Ok(None);
    }
    let mut h = std::hash::DefaultHasher::new();
    LINKS_HEADER.hash(&mut h);
    for src in &sources {
        src.hash(&mut h);
        if let Ok(m) = std::fs::metadata(src) {
            m.len().hash(&mut h);
            m.modified().ok().hash(&mut h);
        }
    }
    let dir = root.join(format!("{:016x}", h.finish()));
    if folder_is_valid(&dir) {
        return Ok(Some(dir));
    }
    std::fs::create_dir_all(root)?;
    sweep(root, std::time::SystemTime::now());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let staging = root.join(format!(
        ".{:016x}-{}-{nanos}",
        h.finish(),
        std::process::id()
    ));
    std::fs::create_dir_all(&staging)?;
    let made = (|| -> std::io::Result<()> {
        for (src, name) in sources.iter().zip(&names) {
            let link = staging.join(name);
            #[cfg(unix)]
            std::os::unix::fs::symlink(src, &link)?;
            #[cfg(windows)]
            if let Err(e) = std::fs::hard_link(src, &link) {
                // Another volume, or a file at NTFS's limit of 1024 links: a copy, made once per toolchain and target folder.
                eprintln!(
                    "TestCommand: cannot hard-link {} into {} ({e}); copying it instead (once per toolchain and target folder)",
                    src.display(),
                    staging.display()
                );
                std::fs::copy(src, &link)?;
            }
        }
        let record: String = std::iter::once(format!("{LINKS_HEADER}\n"))
            .chain(
                names
                    .iter()
                    .zip(&sources)
                    .map(|(n, src)| format!("{n}\t{}\n", src.display())),
            )
            .collect();
        std::fs::write(staging.join(LINKS), record)
    })();
    if let Err(e) = made {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    // A folder of an older format, or one missing a link, is replaced.
    if dir.is_dir() && !folder_is_valid(&dir) {
        let _ = std::fs::remove_dir_all(&dir);
    }
    match std::fs::rename(&staging, &dir) {
        Ok(()) => Ok(Some(dir)),
        // Another process made it first: use that one.
        Err(_) if folder_is_valid(&dir) => {
            let _ = std::fs::remove_dir_all(&staging);
            Ok(Some(dir))
        }
        // Refused (on Windows a scanner may hold a new file for a moment): the staging folder is complete, and this process
        // uses it; the sweep removes it once it is an hour old. No retry: never seen on the test VM (docs/benchmarks.md,
        // "Rename retry"), and this answer is correct.
        Err(e) => {
            eprintln!(
                "TestCommand: cannot rename {} to {} ({e}); this process uses the staging folder",
                staging.display(),
                dir.display()
            );
            Ok(Some(staging))
        }
    }
}

/// The sweep of [`link_folder`] over `root` as of `now`, for the tests of its age rules.
#[doc(hidden)]
pub fn sweep_toolchain_folders(root: &Path, now: std::time::SystemTime) {
    sweep(root, now);
}

/// How old a folder must be before the sweep removes it: a live process fills a staging folder in well under a second, and a
/// test process lives minutes, so an hour-old staging folder is a crashed process's, and an hour-old link folder whose programs
/// are all gone belongs to a test's stand-in toolchain nobody uses any more.
const SWEEP_AGE: std::time::Duration = std::time::Duration::from_secs(3600);

/// Removes from `root`, once they are older than [`SWEEP_AGE`] as of `now`: the staging folders (`.`) a crashed process left,
/// and the link folders whose recorded programs are all gone (a test's stand-in toolchain). Other link folders stay: `cargo
/// clean` removes them.
fn sweep(root: &Path, now: std::time::SystemTime) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age > SWEEP_AGE));
        if !old {
            continue;
        }
        let path = e.path();
        let staging = e.file_name().to_string_lossy().starts_with('.');
        let all_gone = || {
            std::fs::read_to_string(path.join(LINKS)).is_ok_and(|t| {
                let targets: Vec<&str> = t
                    .lines()
                    .skip(1)
                    .filter_map(|l| l.split_once('\t'))
                    .map(|(_, src)| src)
                    .collect();
                !targets.is_empty() && targets.iter().all(|src| !Path::new(src).exists())
            })
        };
        if staging || all_gone() {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// Git for Windows' install folder from the `git` on `machine`'s `PATH` (always `None` elsewhere).
fn git_root_of(machine: &Ambient) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let git = PathBuf::from(resolve_on(OsStr::new("git"), machine));
        git.is_absolute().then(|| git_root(&git)).flatten()
    }
    #[cfg(not(windows))]
    {
        let _ = machine;
        None
    }
}

/// Git for Windows' install folder from the `git` found on the PATH: from `git --exec-path` (`<root>/mingw64/libexec/git-core`,
/// also through a shim such as scoop's), asked once per `git`, else from the layout of the folders around it. `None` when the
/// folder is not Git's (it has no `usr/bin`).
#[cfg(windows)]
fn git_root(git: &Path) -> Option<PathBuf> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashMap<PathBuf, Option<PathBuf>>>> = OnceLock::new();
    let seen = SEEN.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(known) = seen
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(git)
    {
        return known.clone();
    }
    let exec = exec_path(git, EXEC_PATH_LIMIT);
    let root = git_root_from(exec.as_deref(), git);
    seen.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(git.to_path_buf(), root.clone());
    root
}

/// `git --exec-path`, waited for at most `limit` ([`EXEC_PATH_LIMIT`]); a `git` that does not answer in time is ended through the
/// handle held here and counts as no answer (the layout is used then).
#[cfg_attr(not(windows), allow(dead_code))]
fn exec_path(git: &Path, limit: std::time::Duration) -> Option<PathBuf> {
    match answer(git, &["--exec-path"], &[], limit) {
        Answered::Said(text) => {
            Some(PathBuf::from(text.trim())).filter(|p| !p.as_os_str().is_empty())
        }
        Answered::Failed | Answered::TimedOut { .. } => None,
    }
}

/// How [`answer`] ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Answered {
    /// The program ended successfully within the limit; what it printed on stdout.
    Said(String),
    /// It could not start, or ended with a failure.
    Failed,
    /// It did not answer within the limit and was ended through the handle held (`pid`: the process that was ended).
    TimedOut { pid: u32 },
}

/// What `program args` prints on stdout when it ends successfully within `limit`, with `env` added to its environment; a
/// program that does not answer in time is ended through the handle held here and counts as no answer (logged).
pub(crate) fn answer(
    program: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    limit: std::time::Duration,
) -> Answered {
    use std::io::Read;
    #[allow(clippy::disallowed_methods)]
    // a read-only question; the child is ended only through the handle held here
    let child = Command::new(program)
        .args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return Answered::Failed;
    };
    let Some(mut out) = child.stdout.take() else {
        return Answered::Failed;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = out.read_to_string(&mut text);
        let _ = tx.send(text);
    });
    match rx.recv_timeout(limit) {
        Ok(text) => {
            if child.wait().is_ok_and(|s| s.success()) {
                Answered::Said(text)
            } else {
                Answered::Failed
            }
        }
        Err(_) => {
            eprintln!(
                "TestCommand: {} {} did not answer within {} ms; it is ended and counts as no answer",
                program.display(),
                args.join(" "),
                limit.as_millis()
            );
            let pid = child.id();
            let _ = child.kill();
            let _ = child.wait();
            Answered::TimedOut { pid }
        }
    }
}

/// How long `git --exec-path` may take: a hang guard, not a budget (measured 40 to 71 ms on the Windows test VM, docs/benchmarks.md,
/// "TestCommand construction").
#[cfg_attr(not(windows), allow(dead_code))]
const EXEC_PATH_LIMIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The pure part of [`git_root`]: the folder three levels above git's exec path (`<root>/mingw64/libexec/git-core` gives
/// `<root>`), else the first of the three folders above `git` (its own folder, that one's parent, and the next) that holds
/// `cmd/git.exe` or `bin/git.exe`; either way only a folder with a `usr/bin` is Git's.
#[cfg_attr(not(windows), allow(dead_code))]
fn git_root_from(exec_path: Option<&Path>, git: &Path) -> Option<PathBuf> {
    let is_git = |r: &Path| r.join("usr").join("bin").is_dir();
    if let Some(root) = exec_path
        .and_then(|e| e.ancestors().nth(3))
        .filter(|r| is_git(r))
    {
        return Some(root.to_path_buf());
    }
    git.ancestors()
        .skip(1)
        .take(3)
        .find(|r| {
            is_git(r)
                && (r.join("cmd").join("git.exe").is_file()
                    || r.join("bin").join("git.exe").is_file())
        })
        .map(Path::to_path_buf)
}

/// A folder as `PATH` compares it: Windows folders ignore case and separator spelling (`System32` and `system32` are one).
fn path_key(d: &Path) -> String {
    let s = d.to_string_lossy();
    if cfg!(windows) {
        s.replace('/', "\\").trim_end_matches('\\').to_lowercase()
    } else {
        s.into_owned()
    }
}

/// The folders after the link folder: those of the [`NAMED_TOOLS`] found on `machine`'s `PATH` (and, on Windows, Git for Windows'
/// `usr/bin` after git's own folder, never its `bin` of launchers), then the system folders; each once, Windows folders compared
/// without case.
fn tail_dirs(machine: &Ambient, links: Option<&Path>, git_root: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let system = system_dirs(machine);
    let system_keys: Vec<String> = system.iter().map(|d| path_key(d)).collect();
    for tool in NAMED_TOOLS {
        let found = PathBuf::from(resolve_on(OsStr::new(tool), machine));
        // `resolve` returns the bare name when the tool is not on the PATH: no folder then.
        if !found.is_absolute() {
            continue;
        }
        if let Some(d) = found
            .parent()
            .filter(|d| !d.as_os_str().is_empty() && !system_keys.contains(&path_key(d)))
        {
            dirs.push(d.to_path_buf());
        }
        // Git for Windows' shell tools come after git's own folder, so a child's `git` is the real program; `bin` (and `cmd`)
        // hold only launchers that start it again, one more process per call (ci.yml run 8161: merge_check about 5.7 times
        // slower on Windows; measured on the test VM: 62 against 44 ms per call alone, 167 against 101 ms with 8 at once).
        if *tool == "git"
            && let Some(usr_bin) = git_root
                .map(|root| root.join("usr").join("bin"))
                .filter(|d| d.is_dir())
        {
            dirs.push(usr_bin);
        }
    }
    dirs.extend(system);
    let mut seen: Vec<String> = links.map(path_key).into_iter().collect();
    dirs.retain(|d| {
        let key = path_key(d);
        let new = !seen.contains(&key);
        seen.push(key);
        new
    });
    dirs
}

#[cfg(not(windows))]
fn system_dirs(_machine: &Ambient) -> Vec<PathBuf> {
    ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
        .map(PathBuf::from)
        .to_vec()
}

#[cfg(windows)]
fn system_dirs(machine: &Ambient) -> Vec<PathBuf> {
    let root = machine
        .var("SystemRoot")
        .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    vec![root.join("System32"), root]
}

fn join_paths(dirs: &[PathBuf]) -> OsString {
    std::env::join_paths(dirs)
        .unwrap_or_else(|e| panic!("TestCommand: a PATH folder holds the separator: {e}"))
}

/// [`resolve`] on `machine`'s `PATH` and `PATHEXT`.
pub(crate) fn resolve_on(program: &OsStr, machine: &Ambient) -> OsString {
    resolve(program, machine.path.as_deref(), machine.var("PATHEXT"))
}

/// `program` as a full path when it is a bare name found in `path` (on Windows also with each `pathext` suffix); anything with
/// a separator, or not found, as given.
pub(crate) fn resolve(program: &OsStr, path: Option<&OsStr>, pathext: Option<&OsStr>) -> OsString {
    let p = Path::new(program);
    if p.components().count() != 1 || p.is_absolute() {
        return program.to_os_string();
    }
    let exts: Vec<String> = if cfg!(windows) {
        let pathext = pathext.map_or_else(
            || ".COM;.EXE;.BAT;.CMD".to_string(),
            |v| v.to_string_lossy().into_owned(),
        );
        std::iter::once(String::new())
            .chain(
                pathext
                    .split(';')
                    .filter(|e| !e.is_empty())
                    .map(str::to_string),
            )
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in path.map(std::env::split_paths).into_iter().flatten() {
        for ext in &exts {
            let mut name = program.to_os_string();
            name.push(ext);
            let candidate = dir.join(&name);
            if candidate.is_file() {
                return candidate.into_os_string();
            }
        }
    }
    program.to_os_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_s_root_comes_from_its_exec_path_or_its_layout_and_only_a_folder_with_usr_bin_is_git_s() {
        let d = crate::ambient::tempdir();
        let root = d.path().join("Git");
        for sub in ["mingw64/libexec/git-core", "mingw64/bin", "cmd", "usr/bin"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::write(root.join("cmd/git.exe"), "").unwrap();
        let git = root.join("mingw64/bin/git.exe");
        let exec = root.join("mingw64/libexec/git-core");
        assert_eq!(git_root_from(Some(&exec), &git), Some(root.clone()));
        assert_eq!(
            git_root_from(None, &git),
            Some(root.clone()),
            "the layout, when git cannot answer"
        );
        let other = d.path().join("tools/bin/git.exe");
        std::fs::create_dir_all(d.path().join("tools/bin")).unwrap();
        assert_eq!(
            git_root_from(None, &other),
            None,
            "a folder that is not Git's never counts"
        );
    }

    /// A `git` that does not answer `--exec-path` within the limit counts as no answer, is ended through its handle, and the
    /// call returns at the limit, not when the program would have ended (review of df9af3cda).
    #[cfg(unix)]
    #[test]
    fn a_git_that_does_not_answer_its_exec_path_in_time_is_no_answer() {
        use std::os::unix::fs::PermissionsExt;
        let d = crate::ambient::tempdir();
        let git = d.path().join("git");
        // It becomes the sleep that never answers: the process the handle ends is the one that waits.
        std::fs::write(&git, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
        let limit = std::time::Duration::from_millis(300);
        let t = std::time::Instant::now();
        let answered = answer(&git, &["--exec-path"], &[], limit);
        let waited = t.elapsed();
        let Answered::TimedOut { pid } = answered else {
            panic!("a git that does not answer: {answered:?}");
        };
        assert!(
            waited >= limit,
            "returned after {waited:?}, before the limit {limit:?}"
        );
        assert!(
            waited < std::time::Duration::from_secs(10),
            "waited {waited:?} for a git that did not answer"
        );
        // Asked of `ps`, which only reads the process table (it was waited for, so it is not a zombie).
        let listed = TestCommand::new("ps")
            .args(["-o", "pid=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let alive = !String::from_utf8_lossy(&listed.stdout).trim().is_empty();
        assert!(!alive, "the git that did not answer ({pid}) still runs");
        assert_eq!(
            exec_path(&git, limit),
            None,
            "exec_path counts it as no answer"
        );
        let answering = d.path().join("git-answers");
        std::fs::write(&answering, "#!/bin/sh\necho /opt/git/libexec/git-core\n").unwrap();
        std::fs::set_permissions(&answering, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            exec_path(&answering, std::time::Duration::from_secs(10)),
            Some(PathBuf::from("/opt/git/libexec/git-core"))
        );
    }

    #[test]
    fn windows_folders_compare_without_case_or_separator_spelling() {
        if cfg!(windows) {
            assert_eq!(
                path_key(Path::new(r"C:\WINDOWS\system32")),
                path_key(Path::new("C:/Windows/System32/"))
            );
        } else {
            assert_ne!(
                path_key(Path::new("/usr/Bin")),
                path_key(Path::new("/usr/bin"))
            );
        }
    }
}
