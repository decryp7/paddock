//! The git reader of the merge check: every call is bounded, and a failure names the command and git's own words (ER-27), so a
//! caller can turn it into "not measured" (ADR 0089) instead of an empty answer.

use paddock_proc::{Limits, command, run_bounded};
use std::path::Path;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(120);
const MAX_OUTPUT: usize = 1 << 28;

/// A repository folder to run git in.
pub struct Git<'a> {
    root: &'a Path,
}

impl<'a> Git<'a> {
    /// Reads the repository at `root`.
    pub fn new(root: &'a Path) -> Git<'a> {
        Git { root }
    }

    /// Standard output of `git -C root <args>` as bytes; an error names the command and git's last words.
    pub fn bytes(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let mut cmd = command("git");
        // Stable, parseable output whatever the user's git config says.
        cmd.arg("-C")
            .arg(self.root)
            .args(["-c", "core.quotePath=false", "-c", "core.fsmonitor=false"])
            .args(args);
        let shown = args.join(" ");
        let out = run_bounded(cmd, Limits::new(TIMEOUT).max_output(MAX_OUTPUT)).map_err(|e| format!("git {shown} could not run: {e}"))?;
        if out.truncated {
            return Err(format!("git {shown} wrote more than {MAX_OUTPUT} bytes"));
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

    /// The file as UTF-8 text; a file that is not UTF-8 is an error (it cannot be measured).
    pub fn text_at(&self, rev: &str, path: &str) -> Result<Option<String>, String> {
        match self.file_at(rev, path)? {
            None => Ok(None),
            Some(b) => String::from_utf8(b)
                .map(Some)
                .map_err(|_| format!("{path} at {} is not UTF-8", short(rev))),
        }
    }
}

/// The first nine characters of a revision name or hash, for messages.
pub fn short(rev: &str) -> &str {
    rev.get(..9).unwrap_or(rev)
}
