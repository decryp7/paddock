//! Rules sources for the copy check: a file, or `<git ref>:<path>` read with `git show` in the repository folder.

use std::path::Path;
use std::time::Duration;

use paddock_proc::{Limits, command, run_bounded};
use serde_json::json;

use crate::lint::{MAX_FILE_BYTES, RuleSource};
use crate::trace::Trace;

/// `git show` is ended after this long.
const GIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Is `spec` shaped `<ref>:<path>`: not starting with `/` or `-`, exactly one `:` (not counting a first character), and
/// something after it?
pub fn looks_like_git_ref(spec: &str) -> bool {
    let mut chars = spec.chars();
    let Some(first) = chars.next() else { return false };
    if first == '/' || first == '-' {
        return false;
    }
    let rest = chars.as_str();
    let mut parts = rest.split(':');
    matches!((parts.next(), parts.next(), parts.next()), (Some(_), Some(path), None) if !path.is_empty())
}

/// Why a rules source could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadError {
    /// The source is a `<ref>:<path>` that cannot be read (ref not fetched, or path absent on that ref): callers warn.
    pub is_ref: bool,
    /// The cause, naming the source.
    pub cause: String,
    /// What to do next.
    pub next_step: String,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.cause, self.next_step)
    }
}

fn fail(cause: String, next_step: &str) -> ReadError {
    ReadError {
        is_ref: false,
        cause,
        next_step: next_step.to_string(),
    }
}

/// Runs `git <args>` in `repo`, recording the exact command and how it ended.
fn git(repo: &Path, args: &[&str], trace: &Trace<'_>) -> Result<paddock_proc::Output, String> {
    let mut cmd = command("git");
    cmd.args(args).current_dir(repo);
    let shown = format!("git {}", args.join(" "));
    let started = std::time::Instant::now();
    let result = run_bounded(
        cmd,
        Limits::new(GIT_TIMEOUT).max_output(usize::try_from(MAX_FILE_BYTES).unwrap_or(usize::MAX)),
    );
    let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let cwd = repo.display().to_string();
    match &result {
        Ok(out) => {
            let exit = match out.outcome {
                paddock_proc::Outcome::Exited(s) => s.code().map_or_else(|| "signal".to_string(), |c| c.to_string()),
                paddock_proc::Outcome::TimedOut => "timed out".to_string(),
                paddock_proc::Outcome::OutputCap => "output cap".to_string(),
            };
            let mut props = vec![
                ("command", json!(shown)),
                ("cwd", json!(cwd)),
                ("exit", json!(exit)),
                ("ms", json!(ms)),
            ];
            let failed = !out.success();
            if failed {
                // The tail of git's complaint (its last lines, bounded), which is what says why.
                let text = out.stderr_text();
                let tail: Vec<&str> = text.trim().lines().rev().take(4).collect();
                let tail: String = tail.into_iter().rev().collect::<Vec<_>>().join(" / ");
                props.push(("stderr_tail", json!(tail.chars().take(500).collect::<String>())));
            }
            trace.event(
                if failed { "Warning" } else { "Information" },
                &format!("{shown} exit {exit}"),
                &props,
            );
        }
        Err(e) => trace.event(
            "Error",
            &format!("{shown} did not run: {e}"),
            &[("command", json!(shown)), ("cwd", json!(cwd))],
        ),
    }
    result.map_err(|e| e.to_string())
}

/// The first line of git's complaint, or a stand-in.
fn first_line(out: &paddock_proc::Output) -> String {
    out.stderr_text().trim().lines().next().unwrap_or("no message").to_string()
}

/// Reads one rules source called `name`: the file `target` if it exists, else `target` as `<ref>:<path>` from git run in
/// `repo`. The error names the source, the cause and the next step.
pub fn read(name: &str, target: &str, repo: &Path, trace: &Trace<'_>) -> Result<RuleSource, ReadError> {
    let path = Path::new(target);
    if path.exists() {
        let meta = std::fs::metadata(path).map_err(|e| {
            fail(
                format!("cannot read rules source {name}: {e}."),
                "Check that the file exists and is readable.",
            )
        })?;
        if meta.len() > MAX_FILE_BYTES {
            return Err(fail(
                format!("rules source {name} is larger than {MAX_FILE_BYTES} bytes."),
                "List a smaller rules file.",
            ));
        }
        let bytes = std::fs::read(path).map_err(|e| {
            fail(
                format!("cannot read rules source {name}: {e}."),
                "Check that it is a readable file, not a folder.",
            )
        })?;
        return Ok(RuleSource {
            name: name.to_string(),
            text: String::from_utf8_lossy(&bytes).replace("\r\n", "\n"),
        });
    }
    if !looks_like_git_ref(target) {
        return Err(fail(
            format!("cannot read rules source {name}: no such file, and not a `<git ref>:<path>`."),
            "Fix the path in --rules or the `rules` list of wiki-lint.json (paths there are relative to the config file).",
        ));
    }
    let ref_part = target.split(':').next().unwrap_or(target);
    let ref_spec = format!("{ref_part}^{{commit}}");
    match git(repo, &["rev-parse", "--verify", "--quiet", &ref_spec], trace) {
        Err(e) => {
            return Err(fail(
                format!("cannot read rules source {name}: {e}."),
                "Install git and run from inside the repository, or pass --repo.",
            ));
        }
        Ok(out) if !out.success() => {
            return Err(ReadError {
                is_ref: true,
                cause: format!("rules source {name}: the ref `{ref_part}` does not exist in {}.", repo.display()),
                next_step: format!(
                    "In CI fetch full history (fetch-depth: 0) or `git fetch origin <branch>`; locally run `git fetch`, or remove `{name}` from the list."
                ),
            });
        }
        Ok(_) => {}
    }
    let out = git(repo, &["show", target], trace).map_err(|e| {
        fail(
            format!("cannot read rules source {name}: {e}."),
            "Install git and run from inside the repository, or pass --repo.",
        )
    })?;
    if !out.success() {
        return Err(ReadError {
            is_ref: true,
            cause: format!(
                "cannot read rules source {name}: git show failed in {} ({}).",
                repo.display(),
                first_line(&out)
            ),
            next_step: "Check the path exists on that ref, or remove the source from the list.".to_string(),
        });
    }
    if out.truncated {
        return Err(fail(
            format!("rules source {name} is larger than {MAX_FILE_BYTES} bytes."),
            "List a smaller rules file.",
        ));
    }
    Ok(RuleSource {
        name: name.to_string(),
        text: out.stdout_text().replace("\r\n", "\n"),
    })
}

/// The branches matching `glob` (`origin/docs/*`): local and remote-tracking branches, as short names, sorted.
pub fn branches(glob: &str, repo: &Path, trace: &Trace<'_>) -> Result<Vec<String>, ReadError> {
    if glob.starts_with('-') || glob.is_empty() {
        return Err(fail(
            format!("ruleBranches entry `{glob}` is not a branch pattern."),
            "Use a pattern like `origin/docs/*`.",
        ));
    }
    let heads = format!("refs/heads/{glob}");
    let remotes = format!("refs/remotes/{glob}");
    let out = git(repo, &["for-each-ref", "--format=%(refname:short)", &heads, &remotes], trace).map_err(|e| {
        fail(
            format!("cannot list branches for `{glob}`: {e}."),
            "Install git and run from inside the repository, or pass --repo.",
        )
    })?;
    if !out.success() {
        return Err(fail(
            format!("cannot list branches for `{glob}`: {}.", first_line(&out)),
            "Run from inside the repository, or pass --repo.",
        ));
    }
    let mut names: Vec<String> = out
        .stdout_text()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_shapes() {
        assert!(looks_like_git_ref("origin/main:docs/rules.md"));
        assert!(looks_like_git_ref("HEAD:a"));
        assert!(!looks_like_git_ref("/abs:path"));
        assert!(!looks_like_git_ref("-x:y"));
        assert!(!looks_like_git_ref("a:b:c"));
        assert!(!looks_like_git_ref("a:"));
        assert!(!looks_like_git_ref("nocolon"));
        assert!(!looks_like_git_ref(""));
    }
}
