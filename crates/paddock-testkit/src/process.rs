//! [`TestCommand`]: the one way a test starts `./dev`, `cargo` or `node` (ADR 0100, check C123).
//!
//! A child started with a plain `Command` inherits the machine's whole environment, so a new environment lookup in the program
//! under test (`ENGINEERING_RULES`, `HEAVY_LOCK_DIR`, `HOME`, a token) quietly reaches real machine state: the pre-push tests ran
//! the real heavy steps, and `dev()` found the real `heavy.sh` and took machine-wide slots in `~/.cache/heavy-slots` (ER-14).
//! `TestCommand` starts every child from an **empty** environment (`env_clear`) plus a listed set:
//!
//! - `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, the `XDG_*` folders, `TMPDIR`, `TEMP` and `TMP` point into a private
//!   folder made for this command ([`TestCommand::private_dir`]), removed when the command drops;
//! - `PATH` is curated: the folders of the toolchain the test run itself uses (`cargo`, `rustc`, `node`, `git`), then the
//!   system folders (`/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`: `lsof` and `ps` are there), nothing else; [`TestCommand::path_first`] puts a test's own stand-in folder first, and `env("PATH", ..)`
//!   replaces it whole;
//! - the toolchain variables cargo needs (`RUSTUP_HOME`, `CARGO_HOME`, `RUSTUP_TOOLCHAIN`) and, on Windows, the few the OS
//!   needs (`SystemRoot`, ...): [`PASSED_THROUGH`], read from this process;
//! - a private global git config, `.gitconfig` in the private home, which git finds through `HOME` (no `GIT_CONFIG_GLOBAL`, so a test
//!   that names another home gets that home's `.gitconfig`, as on a machine): `maintenance.auto=false`, `gc.auto=0`, a test identity
//!   and `safe.directory=*` (the system config, where CI's image names it, is not read), so no test's git starts background maintenance;
//! - fixed values that make git deterministic: an author and committer identity, no system git config.
//!
//! Everything else a test needs is a named addition (`.env("NAME", value)`), which is also what a reader of the test sees.
//! [`ALLOWED_NAMES`] lists every name a fresh command carries; the tests of this module compare a real child's environment with
//! it, so a variable added here without being listed fails.
//!
//! The command derefs to [`std::process::Command`], so `args`, `current_dir`, `output`, `spawn` and the rest are the usual ones.

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

/// The folders the private home and temp variables name (see [`TestCommand::private_dir`]).
const REDIRECTED: &[&str] = &[
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
        // The private global git config, read through HOME: no background maintenance or gc (a detached `git maintenance` holds
        // `objects/maintenance.lock` and a bare clone made meanwhile copies it: the kept-clone fixture hit that 104 times in 1200 under
        // load), a test identity, and every folder trusted, since GIT_CONFIG_NOSYSTEM hides the system config where the CI image and the
        // local Linux container name `safe.directory` (a worktree mounted there belongs to another user).
        let _ = std::fs::write(
            home.join(".gitconfig"),
            "[maintenance]\n\tauto = false\n[gc]\n\tauto = 0\n[user]\n\tname = Paddock Test\n\temail = test@example.invalid\n[safe]\n\tdirectory = *\n",
        );
        let vars: [(&str, PathBuf); 11] = [
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
        cmd.env("PATH", curated_path(&[]));
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
pub struct PrivateGuard(#[allow(dead_code)] Arc<PrivateDir>);

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

/// The curated `PATH`: `first`, then the folder of the cargo that runs this test (`CARGO`, set by cargo for tests, so also the
/// folder of `rustc`), of `node` and of `git` found on this process's `PATH`, then the system folders.
fn curated_path(first: &[PathBuf]) -> OsString {
    let mut dirs: Vec<PathBuf> = first.to_vec();
    if let Some(cargo) = std::env::var_os("CARGO").map(PathBuf::from)
        && let Some(d) = cargo.parent()
    {
        dirs.push(d.to_path_buf());
    }
    let ambient = std::env::var_os("PATH");
    for tool in ["node", "git"] {
        let found = resolve(OsStr::new(tool), ambient.as_deref());
        if let Some(d) = Path::new(&found)
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
        {
            dirs.push(d.to_path_buf());
        }
    }
    #[cfg(windows)]
    if let Some(git_dir) = Path::new(&resolve(OsStr::new("git"), ambient.as_deref()))
        .parent()
        .and_then(Path::parent)
    {
        // Git for Windows keeps `sh`, `bash`, `ps`, `sed` and the other tools its scripts need in `usr/bin`, not beside `git.exe`.
        for sub in ["usr/bin", "bin"] {
            let d = git_dir.join(sub);
            if d.is_dir() {
                dirs.push(d);
            }
        }
    }
    dirs.extend(system_dirs());
    let mut seen = Vec::new();
    dirs.retain(|d| {
        let new = !seen.contains(d);
        seen.push(d.clone());
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
