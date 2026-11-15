//! [`TestCommand`]: the one way a test starts `./dev`, `cargo` or `node` (ADR 0100, check C123).
//!
//! A child started with a plain `Command` inherits the machine's whole environment, so a new environment lookup in the program
//! under test (`ENGINEERING_RULES`, `HEAVY_LOCK_DIR`, `HOME`, a token) quietly reaches real machine state: the pre-push tests ran
//! the real heavy steps, and `dev()` found the real `heavy.sh` and took machine-wide slots in `~/.cache/heavy-slots` (ER-14).
//! `TestCommand` starts every child from an **empty** environment (`env_clear`) plus a listed set:
//!
//! - `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, the `XDG_*` folders, `TMPDIR`, `TEMP` and `TMP` point into a private
//!   folder made for this command ([`TestCommand::private_dir`]), removed when the command drops;
//! - `PATH` is curated: a private folder of links to the toolchain's programs (`cargo`, `rustc`, `rustup`, ...; never the folder
//!   they are in, where `cargo install` puts other tools), the folders of `node`, `npm`, `npx` and `git` as the test process
//!   finds them, on Windows Git for Windows' `usr/bin` and `bin`, then the system folders (`/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`: `lsof` and `ps` are there), nothing else; [`TestCommand::path_first`] puts a test's own stand-in folder first, and `env("PATH", ..)`
//!   replaces it whole;
//! - the toolchain variables cargo needs (`RUSTUP_HOME`, `CARGO_HOME`, `RUSTUP_TOOLCHAIN`) and, on Windows, the few the OS
//!   needs (`SystemRoot`, ...): [`PASSED_THROUGH`], read from this process;
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
//! The program is resolved once, when the command is made: a bare name (`node`, `git`) becomes the full path found on the **test
//! process's** `PATH`. [`TestCommand::path_first`] and `env("PATH", ..)` change only the child's own lookups, never which program
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

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A [`Command`] that starts from an empty environment plus the listed one (see the module documentation).
#[derive(Debug)]
pub struct TestCommand {
    cmd: Command,
    /// The environment a fresh command carries: what [`TestCommand::env_clear`] goes back to.
    base: Vec<(OsString, OsString)>,
    private: Arc<PrivateDir>,
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
        let dir = tempfile::Builder::new()
            .prefix("paddock-test-cmd-")
            .tempdir()
            .unwrap_or_else(|e| panic!("TestCommand: cannot make its private folder: {e}"));
        TestCommand::build(program.as_ref(), PrivateDir::Own(dir))
    }

    /// Like [`new`](TestCommand::new), with the private home and temp folders under `dir` (a folder the test owns and removes), for a
    /// test that reads what the child wrote there or starts several children that share a home.
    ///
    /// # Panics
    ///
    /// When `dir` cannot be made.
    #[must_use]
    pub fn new_in(program: impl AsRef<OsStr>, dir: &Path) -> TestCommand {
        std::fs::create_dir_all(dir)
            .unwrap_or_else(|e| panic!("TestCommand: cannot make {}: {e}", dir.display()));
        TestCommand::build(program.as_ref(), PrivateDir::Given(dir.to_path_buf()))
    }

    #[allow(clippy::disallowed_methods)] // the one place a test's child process is made (C123 forbids it elsewhere)
    fn build(program: &OsStr, private: PrivateDir) -> TestCommand {
        let resolved = resolve(program, std::env::var_os("PATH").as_deref());
        let mut cmd = Command::new(resolved);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
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
            ("GIT_CEILING_DIRECTORIES", std::env::temp_dir()),
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
            if let Some(v) = std::env::var_os(k) {
                cmd.env(k, v);
            }
        }
        // With HOME redirected, cargo and rustup would look for their folders under the private home and find nothing (offline
        // registry, toolchains): name the real ones, the only place a child reads the user's folders, and only the toolchain's.
        for (k, folder) in [("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")] {
            if std::env::var_os(k).is_none()
                && let Some(real) = real_home().map(|h| h.join(folder)).filter(|p| p.is_dir())
            {
                cmd.env(k, real);
            }
        }
        cmd.env("PATH", curated_path(&[], &bin_dir(&root)));
        let base = cmd
            .get_envs()
            .filter_map(|(k, v)| v.map(|v| (k.to_os_string(), v.to_os_string())))
            .collect();
        TestCommand {
            cmd,
            base,
            private: Arc::new(private),
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
        self.cmd.output()
    }

    /// Runs the child to its end with inherited standard streams.
    ///
    /// # Errors
    ///
    /// When the program cannot be started.
    pub fn status(&mut self) -> std::io::Result<ExitStatus> {
        self.cmd.status()
    }

    /// Starts the child. The returned [`TestChild`] keeps the private folders until it is waited on or dropped, whatever happens to
    /// this command, and kills and reaps a child that is still running when it drops.
    ///
    /// # Errors
    ///
    /// When the program cannot be started.
    pub fn spawn(&mut self) -> std::io::Result<TestChild> {
        let child = self.cmd.spawn()?;
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
            if let Some(v) = std::env::var_os(k) {
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

/// The user's home folder as this process sees it (read once for the toolchain folders; never given to a child as `HOME`).
fn real_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
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
/// and `bin` join too, found from `git --exec-path` ([`git_root`]); the POSIX tools are in those or in the system folders.
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
    /// The toolchain links in the command's private folder.
    pub toolchain: Vec<PathBuf>,
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
        write!(f, "toolchain links: {}", list(&self.toolchain))
    }
}

impl TestCommand {
    /// What decided this command's `PATH` (see [`PathReport`]).
    #[must_use]
    pub fn path_report(&self) -> PathReport {
        let ambient = std::env::var_os("PATH");
        let child = self
            .cmd
            .get_envs()
            .find(|(k, _)| *k == "PATH")
            .and_then(|(_, v)| v)
            .map(|v| std::env::split_paths(v).collect())
            .unwrap_or_default();
        let found_at = |name: &str| {
            Some(PathBuf::from(resolve(OsStr::new(name), ambient.as_deref())))
                .filter(|p| p.is_absolute())
        };
        let git = found_at("git");
        #[cfg(windows)]
        let git_root = git.as_deref().and_then(git_root);
        #[cfg(not(windows))]
        let git_root = None;
        let toolchain = std::fs::read_dir(bin_dir(self.private.path()))
            .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        PathReport {
            child,
            ambient: ambient
                .as_deref()
                .map(|v| std::env::split_paths(v).collect())
                .unwrap_or_default(),
            found: ["git", "sh", "bash", "node", "cargo", "rustc", "rustup"]
                .into_iter()
                .map(|n| (n, if n == "git" { git.clone() } else { found_at(n) }))
                .collect(),
            git_root,
            toolchain,
        }
    }
}

/// The command's private folder of toolchain links.
fn bin_dir(private: &Path) -> PathBuf {
    private.join("bin")
}

/// Links the [`TOOLCHAIN`] programs found on `ambient` (`cargo`: the one that runs this test, `CARGO`, when set) into `bin`;
/// whether `bin` holds any. A link that is already there (a `new_in` folder shared by several commands) is kept.
fn link_toolchain(bin: &Path, ambient: Option<&OsStr>) -> bool {
    for name in TOOLCHAIN {
        let source = match std::env::var_os("CARGO") {
            Some(c) if *name == "cargo" => PathBuf::from(c),
            _ => PathBuf::from(resolve(OsStr::new(name), ambient)),
        };
        let (true, Some(file)) = (source.is_absolute() && source.is_file(), source.file_name())
        else {
            continue;
        };
        let link = bin.join(file);
        if std::fs::create_dir_all(bin).is_err() || link.exists() {
            continue;
        }
        #[cfg(unix)]
        let _ = std::os::unix::fs::symlink(&source, &link);
        #[cfg(windows)]
        if std::fs::hard_link(&source, &link).is_err() {
            // Another volume: a copy (rustup's proxies are one program that reads the name it was started as).
            let _ = std::fs::copy(&source, &link);
        }
    }
    std::fs::read_dir(bin).is_ok_and(|mut d| d.next().is_some())
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
    #[allow(clippy::disallowed_methods)]
    // a read-only question to git, awaited here; nothing is ended by pid
    let exec = Command::new(git)
        .arg("--exec-path")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
    let root = git_root_from(exec.as_deref(), git);
    seen.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(git.to_path_buf(), root.clone());
    root
}

/// The pure part of [`git_root`]: three folders above git's exec path, else the nearest folder above `git` (at most three up)
/// that holds `cmd/git.exe` or `bin/git.exe`; only a folder with a `usr/bin` is Git's.
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

/// The curated `PATH`: `first`, then the private folder of toolchain links (`bin`), the folders of the [`NAMED_TOOLS`] found on
/// this process's `PATH` (and, on Windows, Git for Windows' `usr/bin` and `bin`), then the system folders.
fn curated_path(first: &[PathBuf], bin: &Path) -> OsString {
    let mut dirs: Vec<PathBuf> = first.to_vec();
    let ambient = std::env::var_os("PATH");
    if link_toolchain(bin, ambient.as_deref()) {
        dirs.push(bin.to_path_buf());
    }
    let system = system_dirs();
    let system_keys: Vec<String> = system.iter().map(|d| path_key(d)).collect();
    for tool in NAMED_TOOLS {
        let found = PathBuf::from(resolve(OsStr::new(tool), ambient.as_deref()));
        // `resolve` returns the bare name when the tool is not on the PATH: no folder then.
        if !found.is_absolute() {
            continue;
        }
        #[cfg(windows)]
        if *tool == "git"
            && let Some(root) = git_root(&found)
        {
            dirs.extend(
                [root.join("usr").join("bin"), root.join("bin")]
                    .into_iter()
                    .filter(|d| d.is_dir()),
            );
        }
        if let Some(d) = found
            .parent()
            .filter(|d| !d.as_os_str().is_empty() && !system_keys.contains(&path_key(d)))
        {
            dirs.push(d.to_path_buf());
        }
    }
    dirs.extend(system);
    let mut seen = Vec::new();
    dirs.retain(|d| {
        let key = path_key(d);
        let new = !seen.contains(&key);
        seen.push(key);
        new
    });
    join_paths(&dirs)
}

#[cfg(not(windows))]
fn system_dirs() -> Vec<PathBuf> {
    ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
        .map(PathBuf::from)
        .to_vec()
}

#[cfg(windows)]
fn system_dirs() -> Vec<PathBuf> {
    let root =
        std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    vec![root.join("System32"), root]
}

fn join_paths(dirs: &[PathBuf]) -> OsString {
    std::env::join_paths(dirs)
        .unwrap_or_else(|e| panic!("TestCommand: a PATH folder holds the separator: {e}"))
}

/// `program` as a full path when it is a bare name found in `path`; anything with a separator, or not found, as given.
fn resolve(program: &OsStr, path: Option<&OsStr>) -> OsString {
    let p = Path::new(program);
    if p.components().count() != 1 || p.is_absolute() {
        return program.to_os_string();
    }
    let exts: Vec<String> = if cfg!(windows) {
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
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
        let d = tempfile::tempdir().unwrap();
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
