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
//! - fixed values that make git deterministic: an author and committer identity, no system or global git config.
//!
//! Everything else a test needs is a named addition (`.env("NAME", value)`), which is also what a reader of the test sees.
//! [`ALLOWED_NAMES`] lists every name a fresh command carries; the tests of this module compare a real child's environment with
//! it, so a variable added here without being listed fails.
//!
//! The command derefs to [`std::process::Command`], so `args`, `current_dir`, `output`, `spawn` and the rest are the usual ones.

use std::ffi::{OsStr, OsString};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::process::Command;

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
    "GIT_CONFIG_GLOBAL",
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
    private: PrivateDir,
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
        let vars: [(&str, PathBuf); 12] = [
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
            ("GIT_CONFIG_GLOBAL", home.join(".gitconfig")),
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
        TestCommand { cmd, private }
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

    /// Puts `dir` first on the child's `PATH` (a folder of stand-in programs); the curated folders follow. Chain it before the
    /// `Command` methods (`env` and `args` return the plain `Command`); a second call puts its folder in front of the first.
    pub fn path_first(&mut self, dir: &Path) -> &mut TestCommand {
        let mut first: Vec<PathBuf> = vec![dir.to_path_buf()];
        if let Some((_, Some(v))) = self.cmd.get_envs().find(|(k, _)| *k == "PATH") {
            first.extend(std::env::split_paths(v));
        }
        self.cmd.env("PATH", join_paths(&first));
        self
    }

    /// Sets `CARGO_TARGET_DIR` to the build folder this test program was built into, for a child `cargo build` that must put its
    /// output where the test looks for it (`cargo_layout::example_bin`, `sibling_bin`). Chain it before the `Command` methods.
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

/// Keeps the private folder of a [`TestCommand`] that was taken apart with [`TestCommand::into_parts`].
#[derive(Debug)]
pub struct PrivateGuard(#[allow(dead_code)] PrivateDir);

impl Deref for TestCommand {
    type Target = Command;
    fn deref(&self) -> &Command {
        &self.cmd
    }
}

impl DerefMut for TestCommand {
    fn deref_mut(&mut self) -> &mut Command {
        &mut self.cmd
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
