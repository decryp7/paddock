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
    Io {
        /// The path as given.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// It resolved to a path that has no plain spelling.
    NotPlain(NotPlain),
}

impl fmt::Display for RealError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot resolve `{}`: {source}", path.display()),
            Self::NotPlain(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RealError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::NotPlain(e) => Some(e),
        }
    }
}

/// Whether a verbatim drive or UNC path may lose its prefix without naming another file: every component a valid,
/// non-reserved Windows name, no `.` or `..`, and at most 259 UTF-16 units (247 for [`Purpose::CreateDir`]). `false` for
/// anything that is not such a verbatim path, and always off Windows.
pub fn safe_to_strip(path: &Path, os: Os, purpose: Purpose) -> bool {
    matches!(plan(path, os, purpose), Plan::Strip(_))
}

/// The path without its verbatim prefix when [`safe_to_strip`], else **unchanged**. For display and for Rust's file APIs only,
/// which open either form: the result may still be a verbatim path, so never hand it to another program ([`for_tools`]).
pub fn plain(path: &Path, os: Os) -> Cow<'_, Path> {
    match plan(path, os, Purpose::Plain) {
        Plan::Strip(text) => Cow::Owned(PathBuf::from(text)),
        Plan::NotVerbatim | Plan::Refuse(_) => Cow::Borrowed(path),
    }
}

/// The path for another program (git, cargo, npm, Git bash, Explorer): the plain spelling with `/` separators, or why that is
/// impossible: a volume path, a device path, an unsafe name, or a path over the limit. Callers return the error to their
/// user; none falls back to the verbatim text.
pub fn for_tools(path: &Path, os: Os) -> Result<String, NotPlain> {
    let refuse = |why: Unsafe| {
        let (reason, next_step) = why.explain();
        NotPlain {
            path: path.to_path_buf(),
            reason,
            next_step,
        }
    };
    let Some(text) = path.to_str() else {
        return Err(NotPlain {
            path: path.to_path_buf(),
            reason: "the path is not valid Unicode".to_owned(),
            next_step: "use a folder whose name is plain text".to_owned(),
        });
    };
    if os != Os::Windows {
        return Ok(text.to_owned());
    }
    match plan(path, os, Purpose::Plain) {
        Plan::Strip(plain_text) => Ok(plain_text.replace('\\', "/")),
        Plan::Refuse(why) => Err(refuse(why)),
        Plan::NotVerbatim if parse(text).0 == Class::Device => Err(refuse(Unsafe::Device)),
        Plan::NotVerbatim => Ok(text.replace('\\', "/")),
    }
}

/// `canonicalize`, then [`plain`] for the running OS.
pub fn real(path: &Path) -> std::io::Result<PathBuf> {
    let resolved = real_verbatim(path)?;
    Ok(plain(&resolved, Os::current()).into_owned())
}

/// `canonicalize`, then [`for_tools`] for the running OS.
pub fn real_for_tools(path: &Path) -> Result<String, RealError> {
    let resolved = real_verbatim(path).map_err(|source| RealError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    for_tools(&resolved, Os::current()).map_err(RealError::NotPlain)
}

/// [`real`], or the given path when it cannot be resolved (it does not exist).
pub fn real_or_given(path: &Path) -> PathBuf {
    real(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `canonicalize` unchanged: the one named door to the verbatim form (Win32 calls past the limit, compared only with other
/// verbatim paths).
pub fn real_verbatim(path: &Path) -> std::io::Result<PathBuf> {
    #[expect(
        clippy::disallowed_methods,
        reason = "winpath is the one door to canonicalize (ADR 0021 amendment 2026-11-11, D-P2)"
    )]
    std::fs::canonicalize(path)
}

/// Whether two paths name the same place by spelling: the prefix is stripped lexically from drive and UNC paths, `/` and `\`
/// are one separator, a trailing separator is ignored and names compare ignoring case (Windows ordinal rule). Volume and
/// device paths compare only with the same form. Never opens anything. Off Windows the comparison is exact.
pub fn same(a: &Path, b: &Path, os: Os) -> bool {
    if os != Os::Windows {
        return a == b;
    }
    let (Some(a_text), Some(b_text)) = (a.to_str(), b.to_str()) else {
        return a == b;
    };
    let (class_a, parts_a) = parse(a_text);
    let (class_b, parts_b) = parse(b_text);
    class_a.lexical() == class_b.lexical()
        && parts_a.len() == parts_b.len()
        && parts_a
            .iter()
            .zip(&parts_b)
            .all(|(x, y)| fold(x) == fold(y))
}

/// The components of a path as [`same`] sees them: drive paths start with `C:`, UNC paths with the server (never `UNC`),
/// the prefix is dropped, empty parts are skipped. Off Windows: `/` for an absolute path, then the names.
pub fn components(path: &Path, os: Os) -> Vec<String> {
    use std::path::Component;
    if os == Os::Windows
        && let Some(text) = path.to_str()
    {
        return parse(text).1;
    }
    path.components()
        .map(|c| match c {
            Component::RootDir => "/".to_owned(),
            other => other.as_os_str().to_string_lossy().into_owned(),
        })
        .collect()
}

/// The spelling of `path`.
pub fn kind(path: &Path, os: Os) -> Kind {
    if os != Os::Windows {
        return Kind::Unix;
    }
    let Some(text) = path.to_str() else {
        return Kind::Relative;
    };
    let (class, parts) = parse(text);
    let letter = parts.first().and_then(|p| p.chars().next()).unwrap_or('?');
    match class {
        Class::Drive { verbatim } => Kind::Drive { letter, verbatim },
        Class::Unc { verbatim } => Kind::Unc { verbatim },
        Class::Volume => Kind::Volume,
        Class::Device => Kind::Device,
        Class::Rooted | Class::Relative => Kind::Relative,
    }
}

/// The longest plain path in UTF-16 units (`MAX_PATH` is 260 including the terminating null).
const MAX_PLAIN_UNITS: usize = 259;
/// The longest plain folder to be created: `MAX_PATH` minus 12, so an 8.3 file name can be appended.
const MAX_CREATE_DIR_UNITS: usize = 247;

/// What `plan` found.
enum Plan {
    /// Not a verbatim path (or not on Windows): nothing to strip.
    NotVerbatim,
    /// Verbatim and safe: the plain spelling.
    Strip(String),
    /// Verbatim and not safe to strip.
    Refuse(Unsafe),
}

/// Why a verbatim path keeps its prefix.
enum Unsafe {
    Reserved(String),
    BadName(String),
    DotComponent,
    Incomplete,
    TooLong { units: usize, limit: usize },
    Volume,
    Device,
}

impl Unsafe {
    /// The cause and the next step, for [`NotPlain`].
    fn explain(&self) -> (String, String) {
        let (reason, next) = match self {
            Self::Reserved(n) => (format!("`{n}` is a reserved Windows name"), "rename the folder or file".to_owned()),
            Self::BadName(n) => (
                format!("`{n}` is not a valid Windows name (a forbidden character, an empty part, or a trailing dot or space)"),
                "rename the folder or file".to_owned(),
            ),
            Self::DotComponent => ("it contains a `.` or `..` part".to_owned(), "give the resolved path".to_owned()),
            Self::Incomplete => ("the share path has no server and share name".to_owned(), "give the full `\\\\server\\share` path".to_owned()),
            Self::TooLong { units, limit } => (
                format!("the path is {units} characters, longer than Windows' {limit}"),
                "move the work folder to a shorter path (or enable long paths: git `core.longpaths`)".to_owned(),
            ),
            Self::Volume => (
                "the folder is on a volume without a drive letter".to_owned(),
                "mount it on a drive letter or a folder path".to_owned(),
            ),
            Self::Device => ("it names a device, not a file or folder".to_owned(), "use a path on a drive or a share".to_owned()),
        };
        (reason, next)
    }
}

fn plan(path: &Path, os: Os, purpose: Purpose) -> Plan {
    if os != Os::Windows {
        return Plan::NotVerbatim;
    }
    let Some(text) = path.to_str() else {
        return Plan::NotVerbatim;
    };
    let (plain_text, names) = match verbatim(text) {
        None => return Plan::NotVerbatim,
        Some(Verbatim::Volume(_)) => return Plan::Refuse(Unsafe::Volume),
        Some(Verbatim::Device(_)) => return Plan::Refuse(Unsafe::Device),
        Some(Verbatim::Drive(rest)) => (rest.to_owned(), rest.get(3..).unwrap_or("")),
        Some(Verbatim::Unc(rest)) => {
            if rest.split('\\').filter(|n| !n.is_empty()).count() < 2 || rest.starts_with('\\') {
                return Plan::Refuse(Unsafe::Incomplete);
            }
            (format!(r"\\{rest}"), rest)
        }
    };
    if let Some(why) = bad_component(names) {
        return Plan::Refuse(why);
    }
    let limit = match purpose {
        Purpose::Plain => MAX_PLAIN_UNITS,
        Purpose::CreateDir => MAX_CREATE_DIR_UNITS,
    };
    let units = plain_text.encode_utf16().count();
    if units > limit {
        return Plan::Refuse(Unsafe::TooLong { units, limit });
    }
    Plan::Strip(plain_text)
}

/// The first component of `names` (backslash separated, one trailing separator allowed) that makes stripping unsafe.
fn bad_component(names: &str) -> Option<Unsafe> {
    let mut parts: Vec<&str> = names.split('\\').collect();
    if parts.last() == Some(&"") {
        parts.pop();
    }
    parts.into_iter().find_map(|name| {
        if name == "." || name == ".." {
            Some(Unsafe::DotComponent)
        } else if is_reserved(name) {
            Some(Unsafe::Reserved(name.to_owned()))
        } else if !is_valid_name(name) {
            Some(Unsafe::BadName(name.to_owned()))
        } else {
            None
        }
    })
}

/// A device name Windows resolves in every folder: `CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9` (also with the
/// superscript digits), `CONIN$` and `CONOUT$`, whatever follows the first dot, and with trailing spaces before it.
fn is_reserved(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        _ => {
            let digit = stem
                .strip_prefix("COM")
                .or_else(|| stem.strip_prefix("LPT"));
            digit.is_some_and(|d| {
                let mut chars = d.chars();
                matches!(
                    (chars.next(), chars.next()),
                    (Some('1'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
                )
            })
        }
    }
}

/// A name Windows keeps as written: not empty, no control character or `< > : " / \ | ? *`, no trailing dot or space.
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.ends_with(['.', ' '])
        && !name
            .chars()
            .any(|c| c < ' ' || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
}

/// The forms of a `\\?\` path.
enum Verbatim<'a> {
    /// `\\?\C:\...`: from the drive letter on.
    Drive(&'a str),
    /// `\\?\UNC\...`: after `UNC\`.
    Unc(&'a str),
    /// `\\?\Volume{..}\...`: after the prefix.
    Volume(&'a str),
    /// Any other `\\?\` form, and `\\?\C:` without its root.
    Device(&'a str),
}

fn verbatim(text: &str) -> Option<Verbatim<'_>> {
    let rest = text.strip_prefix(r"\\?\")?;
    if rest
        .get(..4)
        .is_some_and(|h| h.eq_ignore_ascii_case(r"UNC\"))
    {
        return Some(Verbatim::Unc(&rest[4..]));
    }
    if rest
        .get(..7)
        .is_some_and(|h| h.eq_ignore_ascii_case("Volume{"))
    {
        return Some(Verbatim::Volume(rest));
    }
    let b = rest.as_bytes();
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' {
        return Some(Verbatim::Drive(rest));
    }
    Some(Verbatim::Device(rest))
}

/// The class of a path for [`same`]: paths of different classes are never the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Drive { verbatim: bool },
    Unc { verbatim: bool },
    Volume,
    Device,
    Rooted,
    Relative,
}

impl Class {
    /// The class with the prefix flag cleared: `\\?\C:\x` and `C:\x` are the same place.
    fn lexical(self) -> Self {
        match self {
            Self::Drive { .. } => Self::Drive { verbatim: false },
            Self::Unc { .. } => Self::Unc { verbatim: false },
            other => other,
        }
    }
}

/// A path's class and its components, the prefix dropped, `/` and `\` one separator, empty parts skipped.
fn parse(text: &str) -> (Class, Vec<String>) {
    let split = |s: &str| {
        s.split('\\')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let (class, parts) = match verbatim(text) {
        Some(Verbatim::Drive(rest)) => (Class::Drive { verbatim: true }, split(rest)),
        Some(Verbatim::Unc(rest)) => (Class::Unc { verbatim: true }, split(rest)),
        Some(Verbatim::Volume(rest)) => (Class::Volume, split(rest)),
        Some(Verbatim::Device(rest)) => (Class::Device, split(rest)),
        None => {
            let s = text.replace('/', "\\");
            let b = s.as_bytes();
            if let Some(rest) = s.strip_prefix(r"\\.\").or_else(|| s.strip_prefix(r"\\?\")) {
                (Class::Device, split(rest))
            } else if let Some(rest) = s.strip_prefix(r"\\") {
                (Class::Unc { verbatim: false }, split(rest))
            } else if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' {
                (Class::Drive { verbatim: false }, split(&s))
            } else if s.starts_with('\\') {
                (Class::Rooted, split(&s))
            } else {
                (Class::Relative, split(&s))
            }
        }
    };
    (class, parts)
}

/// Ordinal ignore-case folding: each `char` through its single-character simple uppercase (`char::to_uppercase` when it
/// yields one character, else the character itself), the rule of `CompareStringOrdinal(.., TRUE)` for the letters paths use.
fn fold(name: &str) -> String {
    name.chars()
        .map(|c| {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(u), None) => u,
                _ => c,
            }
        })
        .collect()
}
