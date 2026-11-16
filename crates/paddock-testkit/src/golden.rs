//! Golden files: the expected output of a function kept in a file, compared on every run.
//!
//! A golden is captured on one machine and checked on every other: the Mac, the Linux CI image (which runs as root) and Windows. A
//! value that belongs to the capturing machine (a uid, a home folder, the RAM, a temp path, the host name) makes it fail elsewhere
//! (train 14 WP-2 had six such goldens). Normalise such a value in the test, before the comparison (for example `norm()` in
//! `tools/paddock-dev/tests/it/host_steps.rs`), so the file says `<uid>` and every machine writes the same; conformance C131 reads
//! the tracked golden folders for these literals.
//!
//! **Rewriting.** With [`UPDATE_VAR`] set, a differing golden is rewritten, and the run prints [`confirm_command`]: the exact command
//! that runs that test on Linux as root in the CI image (`tools/linux-container.sh --ci-image`). Run it before committing the file.
//! `./dev golden confirm <test filter>` does both runs, on this machine and in that image, and passes only if both pass
//! (docs/dev-setup.md, "Golden files").

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The variable that turns checking into rewriting (any value except empty or `0`).
pub const UPDATE_VAR: &str = "PADDOCK_UPDATE_GOLDEN";

/// Whether a golden file is checked or rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoldenMode {
    /// A difference is an error.
    Check,
    /// A difference rewrites the file.
    Update,
}

impl GoldenMode {
    /// [`GoldenMode::Update`] when [`UPDATE_VAR`] is set, else [`GoldenMode::Check`].
    pub fn from_env() -> GoldenMode {
        Self::from_var(
            std::env::var_os(UPDATE_VAR)
                .as_deref()
                .and_then(|v| v.to_str()),
        )
    }

    /// The mode for a value of [`UPDATE_VAR`].
    pub fn from_var(value: Option<&str>) -> GoldenMode {
        match value {
            Some(v) if !v.is_empty() && v != "0" => GoldenMode::Update,
            _ => GoldenMode::Check,
        }
    }
}

/// What [`check_golden`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Golden {
    /// The file matched.
    Matched,
    /// The file was missing or different and was rewritten ([`GoldenMode::Update`]).
    Updated,
}

/// Why a golden check failed.
#[derive(Debug)]
pub enum GoldenError {
    /// The file does not exist (in [`GoldenMode::Check`]).
    Missing {
        /// The golden file.
        path: PathBuf,
    },
    /// The file differs; the first differing line (1-based) with both texts.
    Differs {
        /// The golden file.
        path: PathBuf,
        /// The first line that differs.
        line: usize,
        /// That line in the file (empty past its end).
        expected: String,
        /// That line in the actual output (empty past its end).
        actual: String,
    },
    /// The file could not be read or written.
    Io {
        /// The golden file.
        path: PathBuf,
        /// What failed.
        source: io::Error,
    },
}

impl fmt::Display for GoldenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GoldenError::Missing { path } => write!(
                f,
                "golden file {} is missing; run the test with {UPDATE_VAR}=1 and commit it",
                path.display()
            ),
            GoldenError::Differs {
                path,
                line,
                expected,
                actual,
            } => write!(
                f,
                "golden file {} differs at line {line}\n  file:   {expected:?}\n  actual: {actual:?}\nrun the test with {UPDATE_VAR}=1 to accept the new output",
                path.display()
            ),
            GoldenError::Io { path, source } => {
                write!(f, "golden file {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for GoldenError {}

/// The name of the running test: nextest's `NEXTEST_TEST_NAME` (the bare test path), else a libtest thread's name (`main` under
/// `--test-threads=1` is no test path). A name with a character other than letters, digits, `_` and `:` is no name.
pub fn test_name_of<'a>(nextest: Option<&'a str>, thread: Option<&'a str>) -> Option<&'a str> {
    let ok = |n: &&str| {
        !n.is_empty()
            && *n != "main"
            && n.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':'))
    };
    nextest.filter(ok).or(thread.filter(ok))
}

/// `text` as one `sh` word: in single quotes, a `'` inside written as `'\''`.
pub fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The command that confirms the test `test` of the package `package` at `manifest_dir` on Linux as root in the CI image: run from the
/// repository root (the folder with `ci-image/VERSION` above `manifest_dir`), every word safe to paste into `sh`. A name that is not a
/// test path (`main`, as `--test-threads=1` names the thread, or nothing) leaves `<test name>` to fill in; so does a missing package or
/// folder. On an arm64 Mac the CI image is a local arm64 build (`tools/linux-container.sh --ci-image`), not the registry's amd64 one.
pub fn confirm_command(
    manifest_dir: Option<&Path>,
    package: Option<&str>,
    test: Option<&str>,
) -> String {
    let test = test
        .filter(|t| {
            *t != "main"
                && !t.is_empty()
                && t.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':'))
        })
        .unwrap_or("<test name>");
    let package = package
        .filter(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        })
        .map_or(String::new(), |p| format!("-p {p} "));
    let rel = manifest_dir
        .and_then(|dir| {
            let root = dir
                .ancestors()
                .find(|d| d.join("ci-image/VERSION").is_file())?;
            let rel = dir
                .strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            Some(if rel.is_empty() { ".".to_string() } else { rel })
        })
        .unwrap_or_else(|| "<the package folder>".to_string());
    let inner = format!("cd {} && cargo nextest run {package}{test}", sh_quote(&rel));
    format!(
        "tools/linux-container.sh --id paddock --ci-image -- sh -c {}",
        sh_quote(&inner)
    )
}

/// What a rewrite prints: the file, why it matters and [`confirm_command`] (a Mac-only rewrite that was never run as root on Linux is
/// how a golden with the Mac's uid reaches CI). `test` is `NEXTEST_TEST_NAME` when set, else the name of the thread.
pub fn rewritten_notice(
    path: &Path,
    manifest_dir: Option<&Path>,
    package: Option<&str>,
    test: Option<&str>,
) -> String {
    format!(
        "golden: rewrote {}\n  Confirm it on Linux as root in the CI image before you commit it (a value of this machine fails there):\n    {}\n  or both runs at once: ./dev golden confirm [-p <package>] <test filter>",
        path.display(),
        confirm_command(manifest_dir, package, test)
    )
}

/// Line endings as `\n`, so a Windows checkout with `core.autocrlf` still matches.
fn normalise(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Compares `actual` with the file at `path` (or rewrites it in [`GoldenMode::Update`], making its folders).
pub fn check_golden(path: &Path, actual: &str, mode: GoldenMode) -> Result<Golden, GoldenError> {
    let io_err = |source| GoldenError::Io {
        path: path.to_path_buf(),
        source,
    };
    let actual = normalise(actual);
    let expected = match fs::read_to_string(path) {
        Ok(text) => Some(normalise(&text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(io_err(e)),
    };
    if expected.as_deref() == Some(actual.as_str()) {
        return Ok(Golden::Matched);
    }
    if mode == GoldenMode::Update {
        // The folder of a bare file name is empty, and making an empty folder is a no-op.
        fs::create_dir_all(path.parent().unwrap_or(Path::new(""))).map_err(io_err)?;
        fs::write(path, actual.as_bytes()).map_err(io_err)?;
        let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from);
        let package = std::env::var("CARGO_PKG_NAME").ok();
        // nextest names the test in NEXTEST_TEST_NAME (its last word is the test path); a libtest thread is named after the test.
        let nextest = std::env::var("NEXTEST_TEST_NAME").ok();
        let thread = std::thread::current();
        let test = nextest
            .as_deref()
            .and_then(|n| n.split_whitespace().last())
            .or_else(|| thread.name());
        eprintln!(
            "{}",
            rewritten_notice(path, manifest_dir.as_deref(), package.as_deref(), test)
        );
        return Ok(Golden::Updated);
    }
    let Some(expected) = expected else {
        return Err(GoldenError::Missing {
            path: path.to_path_buf(),
        });
    };
    let (mut e, mut a) = (expected.lines(), actual.lines());
    for line in 1.. {
        let (x, y) = (e.next(), a.next());
        if x != y {
            return Err(GoldenError::Differs {
                path: path.to_path_buf(),
                line,
                expected: x.unwrap_or("").to_string(),
                actual: y.unwrap_or("").to_string(),
            });
        }
        if x.is_none() {
            break;
        }
    }
    // Same lines but a different trailing newline.
    let ending = |text: &str| {
        if text.ends_with('\n') {
            "(ends with a newline)"
        } else {
            "(no final newline)"
        }
    };
    Err(GoldenError::Differs {
        path: path.to_path_buf(),
        line: expected.lines().count() + 1,
        expected: ending(&expected).to_string(),
        actual: ending(&actual).to_string(),
    })
}

/// [`check_golden`] with the mode from [`UPDATE_VAR`], failing the test with the message on a difference.
///
/// # Panics
/// When the golden file is missing, differs or cannot be read (that is the assertion).
pub fn assert_golden(path: &Path, actual: &str) {
    if let Err(e) = check_golden(path, actual, GoldenMode::from_env()) {
        panic!("{e}");
    }
}
