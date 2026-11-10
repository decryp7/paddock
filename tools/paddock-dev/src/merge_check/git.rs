//! The git reader of the merge check: every call is bounded, and a failure names the command and git's own words (ER-27), so a
//! caller can turn it into "not measured" (ADR 0089) instead of an empty answer.

use paddock_proc::{Limits, command, run_bounded};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(120);
const MAX_OUTPUT: usize = 1 << 28;

/// A repository folder to run git in.
pub struct Git<'a> {
    root: &'a Path,
    max_output: usize,
    /// Texts already read, by revision and path: the detectors read the same file at the same revision more than once.
    texts: RefCell<HashMap<(String, String), Option<String>>>,
}

impl<'a> Git<'a> {
    /// Reads the repository at `root`.
    pub fn new(root: &'a Path) -> Git<'a> {
        Git {
            root,
            max_output: MAX_OUTPUT,
            texts: RefCell::new(HashMap::new()),
        }
    }

    /// The same reader with another output cap (a read longer than it is an error, never a cut answer).
    pub fn with_max_output(mut self, bytes: usize) -> Git<'a> {
        self.max_output = bytes;
        self
    }

    /// Standard output of `git -C root <args>` as bytes; an error names the command and git's last words.
    pub fn bytes(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        self.run(args, false)
    }

    /// Like [`Git::bytes`], but exit status 1 is an answer too (`git grep` with no match): the output is empty then.
    pub fn bytes_or_none(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        self.run(args, true)
    }

    fn run(&self, args: &[&str], status_1_is_empty: bool) -> Result<Vec<u8>, String> {
        let mut cmd = command("git");
        // Stable, parseable output whatever the user's git config says.
        cmd.arg("-C")
            .arg(self.root)
            .args([
                "-c",
                "core.quotePath=false",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "log.showSignature=false",
            ])
            .args(args);
        let shown = args.join(" ");
        let out =
            run_bounded(cmd, Limits::new(TIMEOUT).max_output(self.max_output)).map_err(|e| format!("git {shown} could not run: {e}"))?;
        if out.truncated {
            return Err(format!("git {shown} wrote more than {} bytes", self.max_output));
        }
        if status_1_is_empty && matches!(out.outcome, paddock_proc::Outcome::Exited(st) if st.code() == Some(1)) && out.stdout.is_empty() {
            return Ok(Vec::new());
        }
        if !out.success() {
            let err = out.stderr_text();
            let tail: String = err.trim().chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
            return Err(format!("git {shown} failed ({:?}): {tail}", out.outcome));
        }
        Ok(out.stdout)
    }

    /// Standard output as text (lossy UTF-8).
    pub fn text(&self, args: &[&str]) -> Result<String, String> {
        self.bytes(args).map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// The NUL-separated paths `args` print.
    pub fn paths(&self, args: &[&str]) -> Result<Vec<String>, String> {
        Ok(self.text(args)?.split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect())
    }

    /// The paths that differ between two tree-ish revisions (no rename detection: both names of a rename are listed).
    pub fn changed_paths(&self, from: &str, to: &str) -> Result<Vec<String>, String> {
        self.paths(&["diff", "--name-only", "-z", "--no-renames", from, to])
    }

    /// Whether this clone is shallow (its history is cut, so parents may be missing).
    pub fn is_shallow(&self) -> Result<bool, String> {
        Ok(self.text(&["rev-parse", "--is-shallow-repository"])?.trim() == "true")
    }

    /// Every file of `rev` with its blob id.
    pub fn blob_ids(&self, rev: &str) -> Result<HashMap<String, String>, String> {
        let listing = self.text(&["ls-tree", "-r", "-z", rev])?;
        let mut m = HashMap::new();
        for entry in listing.split('\0').filter(|e| !e.is_empty()) {
            // "<mode> <type> <oid>\t<path>"
            if let Some((meta, path)) = entry.split_once('\t')
                && let Some(oid) = meta.split(' ').nth(2)
            {
                m.insert(path.to_string(), oid.to_string());
            }
        }
        Ok(m)
    }

    /// The bytes of `path` at `rev` (a commit or tree), `None` when the revision has no such file.
    pub fn file_at(&self, rev: &str, path: &str) -> Result<Option<Vec<u8>>, String> {
        let spec = format!("{rev}:{path}");
        match self.bytes(&["cat-file", "blob", &spec]) {
            Ok(b) => Ok(Some(b)),
            Err(first) => {
                // One process in the common case; only a failure asks whether the file is simply not there.
                let listing = self
                    .text(&["ls-tree", "-z", rev, "--", path])
                    .map_err(|e| format!("{first}; {e}"))?;
                if listing.split('\0').any(|e| e.split_once('\t').is_some_and(|(_, p)| p == path)) {
                    Err(first)
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// The file as UTF-8 text (read once per revision and path); a file that is not UTF-8 is an error (it cannot be measured).
    pub fn text_at(&self, rev: &str, path: &str) -> Result<Option<String>, String> {
        let key = (rev.to_string(), path.to_string());
        if let Some(hit) = self.texts.borrow().get(&key) {
            return Ok(hit.clone());
        }
        let text = match self.file_at(rev, path)? {
            None => None,
            Some(b) => Some(String::from_utf8(b).map_err(|_| format!("{path} at {} is not UTF-8", short(rev)))?),
        };
        self.texts.borrow_mut().insert(key, text.clone());
        Ok(text)
    }
}

/// The first nine characters of a revision name or hash, for messages.
pub fn short(rev: &str) -> &str {
    rev.get(..9).unwrap_or(rev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_longer_than_the_cap_is_an_error_never_a_cut_answer() {
        let here = Path::new(".");
        let err = Git::new(here).with_max_output(4).text(&["--version"]).unwrap_err();
        assert!(err.contains("wrote more than 4 bytes"), "{err}");
        assert!(Git::new(here).text(&["--version"]).unwrap().starts_with("git version"));
    }
}
