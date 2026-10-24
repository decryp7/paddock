//! Golden files: the expected output of a function kept in a file, compared on every run.

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
        Self::from_var(std::env::var_os(UPDATE_VAR).as_deref().and_then(|v| v.to_str()))
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
        return Ok(Golden::Updated);
    }
    let Some(expected) = expected else {
        return Err(GoldenError::Missing { path: path.to_path_buf() });
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
