//! Checks an app (or a shell crate) runs in its own tests, over every instance at once (rule 40):
//!
//! - [`assert_async`]: every `#[tauri::command]` under the given source folders is `async`, and every table entry is
//!   such a command. A sync command runs on the main thread, where window creation deadlocks and a wait for a lock or a
//!   file freezes every window; the check makes a sync command a failing test.
//! - [`sync_generated`]: a generated file (TypeScript from [`crate::ts`] or `ts-rs`) equals what the code generates now,
//!   or is rewritten when `PADDOCK_WRITE_TS` is set; a stale file fails the test, so it fails CI.
//!
//! These read files; they are meant for tests and build checks, not for the running app.

use crate::table::Table;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The environment variable that turns [`sync_generated`] from checking into writing.
pub const WRITE_VAR: &str = "PADDOCK_WRITE_TS";

/// Whether [`sync_generated`] checks or writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Compare; a difference is an error.
    Check,
    /// Rewrite the file when it differs.
    Write,
}

impl WriteMode {
    /// [`WriteMode::Write`] when [`WRITE_VAR`] is set, else [`WriteMode::Check`].
    pub fn from_env() -> WriteMode {
        Self::from_var(std::env::var_os(WRITE_VAR))
    }

    /// The mode for the value of [`WRITE_VAR`] (set to anything but empty or `0`: write).
    pub fn from_var(value: Option<std::ffi::OsString>) -> WriteMode {
        match value {
            Some(v) if !v.is_empty() && v != "0" => WriteMode::Write,
            _ => WriteMode::Check,
        }
    }
}

/// What [`sync_generated`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Synced {
    /// The file was current.
    Current,
    /// The file was missing or stale and was rewritten.
    Written,
}

/// Why [`sync_generated`] failed.
#[derive(Debug)]
pub enum GeneratedError {
    /// The file is missing or differs from the generated text (in [`WriteMode::Check`]).
    Stale { path: PathBuf },
    /// The file could not be written.
    Io { path: PathBuf, source: io::Error },
}

impl std::fmt::Display for GeneratedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeneratedError::Stale { path } => write!(
                f,
                "{} is stale or missing; run the test again with {WRITE_VAR}=1 and commit the file",
                path.display()
            ),
            GeneratedError::Io { path, source } => {
                write!(f, "cannot write {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for GeneratedError {}

/// Checks (or, in [`WriteMode::Write`], rewrites) the generated file `path` against `text`. Line endings on disk are
/// normalised first (a Windows checkout with `core.autocrlf` is not stale).
pub fn sync_generated(path: &Path, text: &str, mode: WriteMode) -> Result<Synced, GeneratedError> {
    let on_disk = fs::read_to_string(path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if on_disk == text {
        return Ok(Synced::Current);
    }
    if mode == WriteMode::Check {
        return Err(GeneratedError::Stale {
            path: path.to_path_buf(),
        });
    }
    let io = |source| GeneratedError::Io {
        path: path.to_path_buf(),
        source,
    };
    fs::create_dir_all(path.parent().unwrap_or(Path::new(""))).map_err(io)?;
    fs::write(path, text).map_err(io)?;
    Ok(Synced::Written)
}

/// A function marked as a Tauri command in a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundCommand {
    pub name: String,
    pub is_async: bool,
    pub file: PathBuf,
    /// 1-based line of the `fn`.
    pub line: usize,
}

/// Whether a trimmed line is a command attribute: `#[tauri::command]`, `#[::tauri::command]`, `#[command]`, with or
/// without arguments.
fn is_command_attribute(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("#[") else {
        return false;
    };
    let rest = rest.strip_prefix("::").unwrap_or(rest);
    let rest = rest.strip_prefix("tauri::").unwrap_or(rest);
    rest.strip_prefix("command")
        .is_some_and(|r| r.starts_with(']') || r.starts_with('('))
}

/// The name of the function declared on `line` (`pub(crate) async fn open_paths<R>(` -> `open_paths`).
fn fn_name(line: &str) -> Option<&str> {
    let code = line.split("//").next().unwrap_or_default().trim_start();
    let after = code
        .strip_prefix("fn ")
        .or_else(|| code.split_once(" fn ").map(|(_, a)| a))?;
    let end = after
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(after.len());
    (end > 0).then(|| &after[..end])
}

/// The commands declared in `text` (the content of `file`). An attribute is matched to the first `fn` within the next
/// eight lines (doc comments and other attributes may sit between them).
pub fn commands_in(file: &Path, text: &str) -> Vec<FoundCommand> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if !is_command_attribute(l.trim()) {
            continue;
        }
        let sig = lines
            .iter()
            .enumerate()
            .skip(i + 1)
            .take(8)
            .find_map(|(j, s)| fn_name(s).map(|n| (j, s, n)));
        if let Some((j, sig, name)) = sig {
            found.push(FoundCommand {
                name: name.to_string(),
                is_async: sig.contains("async fn "),
                file: file.to_path_buf(),
                line: j + 1,
            });
        }
    }
    found
}

/// Every command declared in the `.rs` files under `roots` (recursively).
pub fn scan(roots: &[&Path]) -> io::Result<Vec<FoundCommand>> {
    let mut found = Vec::new();
    let mut stack: Vec<PathBuf> = roots.iter().map(|r| r.to_path_buf()).collect();
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<PathBuf> = fs::read_dir(&dir)?
            .map(|e| e.map(|e| e.path()))
            .collect::<io::Result<_>>()?;
        entries.sort();
        for path in entries {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.extend(commands_in(&path, &fs::read_to_string(&path)?));
            }
        }
    }
    Ok(found)
}

/// What is wrong with `found` for `table`: every sync command, and every table entry that is not among the found
/// commands (its handler lives outside the scanned folders, so its asyncness is unchecked). Empty when all is well.
pub fn async_problems(table: &Table, found: &[FoundCommand]) -> Vec<String> {
    let mut problems: Vec<String> = found
        .iter()
        .filter(|c| !c.is_async)
        .map(|c| {
            format!(
                "{}:{}: {} is a sync command (make it async)",
                c.file.display(),
                c.line,
                c.name
            )
        })
        .collect();
    for s in table.specs() {
        if !found.iter().any(|c| c.name == s.name()) {
            problems.push(format!(
                "{} ({}) is in the table but no command of that name was found",
                s.name(),
                s.path()
            ));
        }
    }
    problems
}

/// Panics, listing every problem, unless every command under `roots` is async and every entry of `table` is one of them.
/// Call it from a test of the crate that owns the table, with its `src` folder (and the folders of any other crate whose
/// commands the table lists).
pub fn assert_async(table: &Table, roots: &[&Path]) {
    let found = scan(roots).unwrap_or_else(|e| panic!("cannot scan {roots:?} for commands: {e}"));
    let problems = async_problems(table, &found);
    assert!(
        problems.is_empty(),
        "every command must be async:\n{}",
        problems.join("\n")
    );
}
