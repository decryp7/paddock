//! Windows paths without the verbatim `\\?\` prefix, in one place (ADR 0021 amendment 2026-11-11, D-P1).
//!
//! `canonicalize` returns the verbatim form on Windows, and that form later meets things that do not speak it: git, cargo,
//! text, comparisons with plain paths. Every path leaves `canonicalize` through one of three doors:
//!
//! - **for programs**: [`for_tools`] and [`real_for_tools`]: the plain spelling with `/` separators, or a refusal that says
//!   why and what to do ([`NotPlain`]). A verbatim path never reaches a program.
//! - **plain where possible**: [`plain`] and [`real`] (display, and Rust's own file APIs, which open either form).
//! - **verbatim**: [`real_verbatim`], for Win32 calls past the length limit, compared only with other verbatim paths.
//!
//! Comparison is its own operation: [`same`] (never opens anything; strips the prefix lexically).
//!
//! The text functions are pure and take the platform ([`Os`]) as a parameter, so every OS tests the Windows rules. No other
//! code removes or tests the prefix; this module holds the one `#[expect]` for `canonicalize`.

use crate::Os;
use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

/// What a stripped path is for: a new folder leaves room for an 8.3 file name (Microsoft: a directory name cannot exceed
/// `MAX_PATH` minus 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Display, or a file or existing folder: at most 259 UTF-16 units.
    Plain,
    /// A folder about to be created: at most 247 units.
    CreateDir,
}

/// The spelling of a path (see the table in the amendment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `C:\x` or `\\?\C:\x`.
    Drive {
        /// The drive letter as written.
        letter: char,
        /// Whether the path carries the `\\?\` prefix.
        verbatim: bool,
    },
    /// `\\srv\share\x` or `\\?\UNC\srv\share\x`.
    Unc {
        /// Whether the path carries the `\\?\` prefix.
        verbatim: bool,
    },
    /// `\\?\Volume{..}\x`: a volume without a drive letter.
    Volume,
    /// `\\.\x`, `\\?\GLOBALROOT\..` and other `\\?\` forms.
    Device,
    /// Anything else on Windows (relative, rooted without a drive, drive-relative).
    Relative,
    /// Any path on macOS or Linux.
    Unix,
}

/// A path that cannot be handed to a program as it is, with the cause and the next step (ER-27).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotPlain {
    /// The path as given.
    pub path: PathBuf,
    /// Why it cannot be made plain.
    pub reason: String,
    /// What the user can do.
    pub next_step: String,
}

impl fmt::Display for NotPlain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot hand `{}` to a program: {}; {}",
            self.path.display(),
            self.reason,
            self.next_step
        )
    }
}

impl std::error::Error for NotPlain {}

/// What [`real_for_tools`] can fail on.
#[derive(Debug)]
pub enum RealError {
    /// The path could not be resolved (it does not exist, or it cannot be read).
    Io(std::io::Error),
    /// It resolved to a path that has no plain spelling.
    NotPlain(NotPlain),
}

impl fmt::Display for RealError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::NotPlain(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RealError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::NotPlain(e) => Some(e),
        }
    }
}

/// Whether a verbatim drive or UNC path may lose its prefix without naming another file: every component a valid,
/// non-reserved Windows name, no `.` or `..`, and at most 259 UTF-16 units (247 for [`Purpose::CreateDir`]). `false` for
/// anything that is not such a verbatim path, and always off Windows.
pub fn safe_to_strip(path: &Path, os: Os, purpose: Purpose) -> bool {
    let _ = (path, os, purpose);
    unimplemented!()
}

/// The path without its verbatim prefix when [`safe_to_strip`], else **unchanged**. For display and for Rust's file APIs only,
/// which open either form: the result may still be a verbatim path, so never hand it to another program ([`for_tools`]).
pub fn plain(path: &Path, os: Os) -> Cow<'_, Path> {
    let _ = os;
    Cow::Borrowed(path)
}

/// The path for another program (git, cargo, npm, Git bash, Explorer): the plain spelling with `/` separators, or why that is
/// impossible: a volume path, a device path, an unsafe name, or a path over the limit. Callers return the error to their
/// user; none falls back to the verbatim text.
pub fn for_tools(path: &Path, os: Os) -> Result<String, NotPlain> {
    let _ = (path, os);
    unimplemented!()
}

/// `canonicalize`, then [`plain`] for the running OS.
pub fn real(path: &Path) -> std::io::Result<PathBuf> {
    let _ = path;
    unimplemented!()
}

/// `canonicalize`, then [`for_tools`] for the running OS.
pub fn real_for_tools(path: &Path) -> Result<String, RealError> {
    let _ = path;
    unimplemented!()
}

/// [`real`], or the given path when it cannot be resolved (it does not exist).
pub fn real_or_given(path: &Path) -> PathBuf {
    let _ = path;
    unimplemented!()
}

/// `canonicalize` unchanged: the one named door to the verbatim form (Win32 calls past the limit, compared only with other
/// verbatim paths).
pub fn real_verbatim(path: &Path) -> std::io::Result<PathBuf> {
    let _ = path;
    unimplemented!()
}

/// Whether two paths name the same place by spelling: the prefix is stripped lexically from drive and UNC paths, `/` and `\`
/// are one separator, a trailing separator is ignored and names compare ignoring case (Windows ordinal rule). Volume and
/// device paths compare only with the same form. Never opens anything. Off Windows the comparison is exact.
pub fn same(a: &Path, b: &Path, os: Os) -> bool {
    let _ = (a, b, os);
    unimplemented!()
}

/// The components of a path as [`same`] sees them: drive paths start with `C:`, UNC paths with the server (never `UNC`),
/// the prefix is dropped, empty parts are skipped. Off Windows: `/` for an absolute path, then the names.
pub fn components(path: &Path, os: Os) -> Vec<String> {
    let _ = (path, os);
    unimplemented!()
}

/// The spelling of `path`.
pub fn kind(path: &Path, os: Os) -> Kind {
    let _ = (path, os);
    unimplemented!()
}
