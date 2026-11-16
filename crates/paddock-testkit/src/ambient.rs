//! [`Ambient`]: the machine values the test kit reads (`PATH`, `CARGO`, the temp folder, the user's home, the toolchain
//! variables), captured **once** per test process and never read live again (ADR 0100).
//!
//! The plain test harness runs a binary's tests on threads of one process, and a test that changes the environment through
//! [`crate::EnvLock`] (`hermetic` points `TMPDIR` into its scratch) changes it for every thread. A kit lookup that read the live
//! value then saw another test's folder: a `Scratch` made inside another test's `tmp` was removed with it ("scratch folder: No
//! such file or directory"), a temporary folder made in a hermetic test's `tmp` was reported as its leftover, and a command
//! linked another test's stand-in toolchain. The capture is taken while holding the environment lock, when no test holds a
//! changed environment (every [`crate::EnvLock`] puts its changes back before it releases the lock).
//!
//! A test that needs other machine values (a stand-in toolchain on `PATH`, its own link folder) passes an [`Ambient`] it made
//! from [`ambient`] to [`crate::TestCommand::on`]; it never changes the process environment for that.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The machine values the kit reads (see the module).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ambient {
    /// The test process's `PATH`.
    pub path: Option<OsString>,
    /// The cargo that runs the tests: `CARGO`, else the one of the toolchain `rustc --print sysroot` names (a test program
    /// started directly, or copied, has no `CARGO`; rustup's proxy would put all of `$CARGO_HOME/bin` in front of its child's
    /// `PATH`).
    pub cargo: Option<PathBuf>,
    /// The temp folder (`TMPDIR`, `TEMP`).
    pub temp_dir: PathBuf,
    /// The user's home (`HOME`, `USERPROFILE` on Windows): only for the toolchain folders.
    pub home: Option<PathBuf>,
    /// The toolchain and build variables a child may get ([`crate::process::PASSED_THROUGH`], [`crate::process::BUILD_VARS`]).
    pub vars: Vec<(OsString, OsString)>,
    /// Where the shared toolchain link folders go: `<target>/paddock-test-toolchain` of the cargo target folder this program
    /// runs from (a folder with cargo's `CACHEDIR.TAG`), else `None` (each command links into its own folder).
    pub link_root: Option<PathBuf>,
}

impl Ambient {
    /// The value of a captured variable.
    #[must_use]
    pub fn var(&self, key: &str) -> Option<&OsStr> {
        self.vars
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_os_str())
    }
}

static CAPTURED: OnceLock<Ambient> = OnceLock::new();

/// The values of this test process, captured on first use (see the module).
pub fn ambient() -> &'static Ambient {
    if let Some(a) = CAPTURED.get() {
        return a;
    }
    // Taking the lock captures them (env_lock calls `capture`); a thread that holds the lock already captured them then.
    drop(crate::env::env_lock());
    CAPTURED.get_or_init(capture_now)
}

/// Captures the values if nobody has: called by [`crate::env_lock`] right after it takes the lock, before any change.
pub(crate) fn capture() {
    let _ = CAPTURED.get_or_init(capture_now);
}

fn capture_now() -> Ambient {
    let path = std::env::var_os("PATH");
    let vars: Vec<(OsString, OsString)> = crate::process::PASSED_THROUGH
        .iter()
        .chain(crate::process::BUILD_VARS)
        .filter_map(|k| std::env::var_os(k).map(|v| (OsString::from(k), v)))
        .collect();
    let cargo = std::env::var_os("CARGO")
        .map(PathBuf::from)
        .or_else(|| cargo_of_toolchain(path.as_deref()));
    Ambient {
        cargo,
        temp_dir: std::env::temp_dir(),
        home: std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from),
        vars,
        link_root: link_root(),
        path,
    }
}

/// The cargo of the toolchain the `rustc` on `path` belongs to (`rustc --print sysroot` plus `bin/cargo`), asked once, with
/// rustup told not to install anything. `None` when there is no `rustc` or it does not answer within the limit.
fn cargo_of_toolchain(path: Option<&OsStr>) -> Option<PathBuf> {
    let rustc = PathBuf::from(crate::process::resolve(OsStr::new("rustc"), path));
    if !rustc.is_absolute() {
        return None;
    }
    let crate::process::Answered::Said(sysroot) = crate::process::answer(
        &rustc,
        &["--print", "sysroot"],
        &[("RUSTUP_AUTO_INSTALL", "0")],
        SYSROOT_LIMIT,
    ) else {
        return None;
    };
    let cargo = Path::new(sysroot.trim())
        .join("bin")
        .join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
    cargo.is_file().then_some(cargo)
}

/// How long `rustc --print sysroot` may take: a hang guard, not a budget (the toolchain is installed; rustup is told not to
/// install one).
const SYSROOT_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

/// `<target>/paddock-test-toolchain` of the cargo target folder this program runs from: its profile folder's parent, and only
/// when that holds cargo's `CACHEDIR.TAG` (so a test program copied to `.../deps/x` elsewhere, a home folder say, never writes
/// there; ER-14).
fn link_root() -> Option<PathBuf> {
    link_root_of(&std::env::current_exe().ok()?)
}

/// [`link_root`] for the program `exe`.
fn link_root_of(exe: &Path) -> Option<PathBuf> {
    let target = paddock_env::cargo_layout::profile_dir_of_exe(exe)?
        .parent()?
        .to_path_buf();
    target
        .join("CACHEDIR.TAG")
        .is_file()
        .then(|| target.join("paddock-test-toolchain"))
}

/// A new temporary folder in the captured temp folder ([`Ambient::temp_dir`]): `tempfile::tempdir()` reads the live `TMPDIR`,
/// which another thread's hermetic test may have pointed into its own scratch.
///
/// # Panics
///
/// When the folder cannot be made (the test cannot run).
#[must_use]
pub fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir_in(&ambient().temp_dir)
        .unwrap_or_else(|e| panic!("cannot make a temporary folder: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program in a `<target>/<profile>/deps` layout gets a link root only when the target folder is cargo's (its
    /// `CACHEDIR.TAG`): a test program copied to `.../deps/x` in a home folder never writes there (review of a9a7e6484).
    #[test]
    fn the_link_root_is_only_in_a_folder_cargo_tagged_as_its_target() {
        let d = tempdir();
        // conformance: file-layout-guess — a stand-in program placed in one of the two layouts profile_dir_of_exe reads
        let exe = d.path().join("target/debug/deps/it-0123456789abcdef");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        assert_eq!(link_root_of(&exe), None, "a folder without CACHEDIR.TAG");
        std::fs::write(
            d.path().join("target/CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n",
        )
        .unwrap();
        assert_eq!(
            link_root_of(&exe),
            Some(d.path().join("target/paddock-test-toolchain"))
        );
    }

    /// Without `CARGO` (a test program started directly or copied), the cargo is the toolchain's own, found from `rustc --print
    /// sysroot`, never rustup's proxy beside `rustc` (review of a9a7e6484: the proxy, 15 of 15).
    #[test]
    fn without_cargo_the_cargo_is_the_toolchains_not_the_proxy() {
        let path = std::env::var_os("PATH");
        let Some(cargo) = cargo_of_toolchain(path.as_deref()) else {
            // No rustc on this machine's PATH: nothing to find.
            assert!(
                !PathBuf::from(crate::process::resolve(
                    OsStr::new("rustc"),
                    path.as_deref()
                ))
                .is_absolute()
            );
            return;
        };
        assert!(cargo.is_file(), "{}", cargo.display());
        let rustc = PathBuf::from(crate::process::resolve(
            OsStr::new("rustc"),
            path.as_deref(),
        ));
        let proxy_dir = rustc.parent().unwrap();
        // Under rustup `rustc` on PATH is a proxy in `$CARGO_HOME/bin`; the toolchain's cargo is elsewhere.
        if proxy_dir
            .join(format!("rustup{}", std::env::consts::EXE_SUFFIX))
            .is_file()
        {
            assert_ne!(
                cargo.parent(),
                Some(proxy_dir),
                "the proxy's folder: {}",
                cargo.display()
            );
        }
    }
}
