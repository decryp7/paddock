use crate::GraphError;

/// A commit: its id and subject line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// The full id.
    pub id: String,
    /// The first line of the message.
    pub subject: String,
}

/// The port to the repository's history. CI plugs in `git` ([`GitHistory`], feature `git-history`); Pacenotes plugs in
/// its own reader.
pub trait History {
    /// The tracked files at the current state, sorted, `/`-separated.
    fn tracked_files(&self) -> Result<Vec<String>, GraphError>;
    /// The files that differ between two revisions (renames as delete plus add), in git's order.
    fn changed_files(&self, base: &str, head: &str) -> Result<Vec<String>, GraphError>;
    /// A file's text at a revision, or `None` when the revision has no such file.
    fn file_at(&self, rev: &str, path: &str) -> Result<Option<String>, GraphError>;
    /// Up to `limit` commits of `range` (a revision or `a..b`), newest first.
    fn log(&self, range: &str, limit: usize) -> Result<Vec<Commit>, GraphError>;
}

/// The unit separator between the id and the subject in the log format [`GitHistory`] asks for.
const FIELD: char = '\u{1f}';

/// Parses `git log --format=%H<US>%s` output; lines without the separator are skipped.
pub fn parse_log(text: &str) -> Vec<Commit> {
    text.lines()
        .filter_map(|l| l.split_once(FIELD))
        .map(|(id, subject)| Commit {
            id: id.to_string(),
            subject: subject.to_string(),
        })
        .collect()
}

/// `git` through `paddock-proc`, every call bounded by a timeout.
#[cfg(feature = "git-history")]
pub struct GitHistory {
    root: std::path::PathBuf,
    timeout: std::time::Duration,
    program: std::ffi::OsString,
}

#[cfg(feature = "git-history")]
impl GitHistory {
    /// For the repository whose working tree is `root`.
    pub fn new(root: impl Into<std::path::PathBuf>, timeout: std::time::Duration) -> GitHistory {
        GitHistory {
            root: root.into(),
            timeout,
            program: "git".into(),
        }
    }

    /// The same with another `git` program (a full path, or a name that is not there, for tests).
    pub fn with_program(mut self, program: impl Into<std::ffi::OsString>) -> GitHistory {
        self.program = program.into();
        self
    }

    fn git(&self, args: &[&str]) -> Result<paddock_proc::Output, GraphError> {
        let mut cmd = paddock_proc::command(&self.program);
        cmd.arg("-C").arg(&self.root).args(args);
        paddock_proc::run_bounded(
            cmd,
            paddock_proc::Limits::new(self.timeout).max_output(1 << 28),
        )
        .map_err(|e| GraphError::History(e.to_string()))
    }

    fn lines(&self, args: &[&str]) -> Result<Vec<String>, GraphError> {
        let out = self.git(args)?;
        if !out.success() {
            return Err(GraphError::History(format!(
                "git {} failed: {}",
                args.first().unwrap_or(&""),
                out.stderr_text().trim()
            )));
        }
        Ok(out
            .stdout_text()
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect())
    }
}

/// A revision that starts with `-` would be read by git as an option; it is refused instead.
#[cfg(feature = "git-history")]
fn check_rev(rev: &str) -> Result<(), GraphError> {
    if rev.starts_with('-') {
        return Err(GraphError::History(format!(
            "{rev:?} is not a revision (it starts with '-'). Pass a branch, tag or commit id."
        )));
    }
    Ok(())
}

#[cfg(feature = "git-history")]
impl History for GitHistory {
    fn tracked_files(&self) -> Result<Vec<String>, GraphError> {
        self.lines(&["ls-files"])
    }

    fn changed_files(&self, base: &str, head: &str) -> Result<Vec<String>, GraphError> {
        check_rev(base)?;
        check_rev(head)?;
        self.lines(&["diff", "--name-only", "--no-renames", base, head])
    }

    fn file_at(&self, rev: &str, path: &str) -> Result<Option<String>, GraphError> {
        check_rev(rev)?;
        let out = self.git(&["show", &format!("{rev}:{path}")])?;
        Ok(out.success().then(|| out.stdout_text()))
    }

    fn log(&self, range: &str, limit: usize) -> Result<Vec<Commit>, GraphError> {
        check_rev(range)?;
        let n = format!("-n{limit}");
        let out = self.git(&["log", &n, &format!("--format=%H{FIELD}%s"), range])?;
        if !out.success() {
            return Err(GraphError::History(format!(
                "git log failed: {}",
                out.stderr_text().trim()
            )));
        }
        Ok(parse_log(&out.stdout_text()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "git-history")]
    #[test]
    fn a_revision_that_looks_like_an_option_is_refused_before_git_runs() {
        let h = GitHistory::new(".", std::time::Duration::from_secs(5))
            .with_program("paddock-no-such-git");
        for result in [
            h.changed_files("--output=x", "HEAD").err(),
            h.changed_files("HEAD", "-x").err(),
            h.file_at("--help", "a").err(),
            h.log("--all", 1).err(),
        ] {
            let msg = result.unwrap().to_string();
            assert!(msg.contains("starts with '-'"), "{msg}");
        }
    }

    #[test]
    fn log_lines_split_on_the_separator() {
        let text = "abc\u{1f}feat: one\ndef\u{1f}fix: two \u{1f} x\nnoise\n";
        let c = parse_log(text);
        assert_eq!(c.len(), 2);
        assert_eq!(
            c[0],
            Commit {
                id: "abc".into(),
                subject: "feat: one".into()
            }
        );
        assert_eq!(c[1].subject, "fix: two \u{1f} x");
        assert!(parse_log("").is_empty());
    }
}
