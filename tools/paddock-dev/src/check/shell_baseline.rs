//! `./dev check shell-baseline`: C129's baseline file (`conformance/baselines/C129-shell.json`), the shell and Python files of the tree
//! that no exception covers with their line counts. It was `node conformance/bin/paddock-shell-baseline.mjs`, retired with the
//! JavaScript C129 (pair P3, ADR 0022 amendment 2026-11-19); the files are found by the check's own reader ([`super::c129::shell_files`]).
//!
//! - no flag: lists the files and counts, writes nothing;
//! - `--shrink`: rewrites the baseline with only the entries whose file is still shell, each lowered to its current line count
//!   (stale entries removed; nothing added, nothing raised);
//! - `--init`: writes it from every file: the first run, or a reset with the user's approval (the baseline may only shrink).

use super::c129::{BASELINE, EXCEPTIONS, parse_baseline, parse_exceptions, shell_files};
use super::config::Config;
use super::tree::Tree;
use super::{Ctx, today_utc};
use paddock_cli::{CliError, Exit, Report};
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

/// What the tool does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// List, write nothing.
    List,
    /// Lower and remove, never add or raise.
    Shrink,
    /// Write from every file.
    Init,
}

/// The `_comment` the file carries (the regenerate command it names is this tool's).
pub const COMMENT: &str = "Shell and Python files of the tree that no exception covers (C129), with their line counts. May only shrink: port the script to Rust, delete its entry. Regenerate with: ./dev check shell-baseline --shrink";

/// The file's text for `entries` (sorted by the caller): one entry per line, as the baseline has always been written.
pub fn file_text(entries: &[(String, usize)]) -> String {
    let rows: Vec<String> = entries
        .iter()
        .map(|(path, lines)| format!("  {}", serde_json::json!({ "path": path, "lines": lines })))
        .collect();
    format!(
        "{{\n \"_comment\": {},\n \"files\": [\n{}\n ]\n}}\n",
        serde_json::Value::String(COMMENT.into()),
        rows.join(",\n")
    )
}

/// What `--shrink` keeps of `old` given the files that are shell now (`current`), and the paths it drops.
pub fn shrink(old: &[(String, usize)], current: &BTreeMap<String, usize>) -> (Vec<(String, usize)>, Vec<String>) {
    let mut kept: Vec<(String, usize)> = old
        .iter()
        .filter_map(|(p, l)| current.get(p).map(|c| (p.clone(), (*l).min(*c))))
        .collect();
    kept.sort();
    let removed = old
        .iter()
        .filter(|(p, _)| !current.contains_key(p))
        .map(|(p, _)| p.clone())
        .collect();
    (kept, removed)
}

/// Runs the tool over the checkout at `root`.
pub fn command(root: &Path, mode: Mode) -> Result<Report, CliError> {
    let broken = |what: String| {
        CliError::new(
            Exit::Failure,
            format!("::error::shell-baseline: {what}. Next step: fix what it names, then run it again."),
        )
    };
    let tree = Tree::open(root);
    let config = Config::bare("paddock");
    let files = tree.files();
    let ctx = Ctx::new(&tree, Rc::clone(&files), None, &config, None, today_utc());
    let exceptions_text = ctx.read(EXCEPTIONS).ok_or_else(|| broken(format!("{EXCEPTIONS} cannot be read")))?;
    let exceptions = parse_exceptions(&exceptions_text).map_err(broken)?;
    let found = shell_files(&ctx, &files, &exceptions);
    if let Some(first) = found.unread.first() {
        return Err(broken(format!(
            "cannot read {} file(s), for example {first}: fix that first",
            found.unread.len()
        )));
    }
    let current: BTreeMap<String, usize> = found.shell.iter().cloned().collect();
    let path = root.join(BASELINE);
    let mut text = String::new();
    let write = |entries: &[(String, usize)]| {
        std::fs::write(&path, file_text(entries)).map_err(|e| broken(format!("cannot write {BASELINE}: {e}")))
    };
    match mode {
        Mode::Init => {
            write(&found.shell)?;
            text.push_str(&format!("wrote {} entries to {BASELINE}", found.shell.len()));
        }
        Mode::Shrink => {
            let old = ctx
                .read(BASELINE)
                .ok_or_else(|| broken(format!("{BASELINE} does not exist: --init writes it the first time")))?;
            let old: Vec<(String, usize)> = parse_baseline(&old)
                .map_err(broken)?
                .into_iter()
                .map(|e| (e.path, e.lines))
                .collect();
            let (kept, removed) = shrink(&old, &current);
            let fresh = found.shell.iter().filter(|(f, _)| !old.iter().any(|(p, _)| p == f)).count();
            for p in &removed {
                text.push_str(&format!("removed: {p}\n"));
            }
            write(&kept)?;
            text.push_str(&format!("wrote {} entries to {BASELINE}\n", kept.len()));
            text.push_str(&format!(
                "{} removed; {fresh} shell files are not in the baseline and were not added",
                removed.len()
            ));
        }
        Mode::List => {
            for (f, l) in &found.shell {
                text.push_str(&format!("{l}\t{f}\n"));
            }
            text.push_str(&format!(
                "{} shell files, {} lines",
                found.shell.len(),
                found.shell.iter().map(|(_, l)| l).sum::<usize>()
            ));
        }
    }
    Ok(Report::new(serde_json::json!({ "shell": current.len() }), text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(v: &[(&str, usize)]) -> Vec<(String, usize)> {
        v.iter().map(|(p, l)| (p.to_string(), *l)).collect()
    }

    #[test]
    fn shrink_lowers_and_removes_and_never_adds_or_raises() {
        let old = pairs(&[("a.sh", 4), ("b.sh", 5), ("gone.sh", 2)]);
        let current = BTreeMap::from([("a.sh".to_string(), 9), ("b.sh".to_string(), 3), ("new.sh".to_string(), 1)]);
        let (kept, removed) = shrink(&old, &current);
        assert_eq!(
            kept,
            pairs(&[("a.sh", 4), ("b.sh", 3)]),
            "a.sh is not raised to 9, new.sh is not added"
        );
        assert_eq!(removed, ["gone.sh"]);
    }

    #[test]
    fn the_file_text_is_the_one_the_check_reads_back() {
        let entries = pairs(&[("a.sh", 1), ("b/c.ps1", 22)]);
        let text = file_text(&entries);
        let back: Vec<(String, usize)> = parse_baseline(&text).unwrap().into_iter().map(|e| (e.path, e.lines)).collect();
        assert_eq!(back, entries);
        assert!(text.contains("./dev check shell-baseline --shrink"));
        assert!(text.ends_with(" ]\n}\n"));
    }
}
