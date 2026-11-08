//! The `.gitea` and `conformance` files the planner narrows workflow changes with, read from git and the working tree
//! (the I/O half of `planFor` in `scripts/affected.mjs`).

use crate::plan_gitea::GiteaSource;
use paddock_proc::{Limits, command, run_bounded};
use paddock_repo_graph::GraphError;
use regex::Regex;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(120);

/// Runs `git -C root args`, returning standard output when git succeeded.
pub fn git_text(root: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = command("git");
    cmd.arg("-C").arg(root).args(args);
    let out = run_bounded(cmd, Limits::new(TIMEOUT).max_output(1 << 28)).ok()?;
    out.success().then(|| out.stdout_text())
}

fn lines(text: Option<String>) -> Vec<String> {
    text.unwrap_or_default()
        .split('\n')
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Like [`git_text`], but a failure (no repository, a bad revision, a timeout) is an error that names the command: the planner
/// must fail, never plan from an empty list.
fn git_lines(root: &Path, args: &[&str]) -> Result<Vec<String>, GraphError> {
    git_text(root, args)
        .map(|t| lines(Some(t)))
        .ok_or_else(|| GraphError::History(format!("git {} failed in {}", args.join(" "), root.display())))
}

/// The crate folders' Rust files that name `generated/<file name>`: `git grep -l -F` over `crates/**/*.rs`.
pub fn files_naming_generated(root: &Path, f: &str) -> Vec<String> {
    let base = f.rsplit('/').next().unwrap_or(f);
    lines(git_text(
        root,
        &["grep", "-l", "-F", &format!("generated/{base}"), "--", "crates/**/*.rs"],
    ))
}

/// The files of a revision pair (or, in files mode, of HEAD and the working tree).
pub struct GitGitea {
    root: PathBuf,
    apps: Vec<String>,
    /// Files mode: `Some(set of paths that differ from HEAD)`.
    dirty: Option<BTreeSet<String>>,
    /// The revision `before` reads (files mode: HEAD).
    before_rev: String,
    /// The revision `after` reads (`None` in files mode: the working tree).
    after_rev: Option<String>,
    gitea: Vec<String>,
    scripts: Vec<String>,
    cache: RefCell<HashMap<(bool, String), Option<String>>>,
}

impl GitGitea {
    /// Files mode (`files = true`: HEAD against the working tree) or base mode (`base` against `head`).
    pub fn new(root: &Path, apps: Vec<String>, files: bool, base: &str, head: &str) -> Result<GitGitea, GraphError> {
        let list = |paths: &[&str]| -> Result<Vec<String>, GraphError> {
            let mut args: Vec<&str> = if files {
                vec!["ls-files"]
            } else {
                vec!["ls-tree", "-r", "--name-only", head]
            };
            if !files {
                args.push("--");
            }
            args.extend(paths);
            git_lines(root, &args)
        };
        let gitea = list(&[".gitea"])?;
        let script_re = Regex::new(r"\.(mjs|cjs|js|sh)$");
        let scripts = list(&["conformance"])?
            .into_iter()
            .filter(|x| {
                script_re.as_ref().is_ok_and(|r| r.is_match(x))
                    && !x.starts_with("conformance/test/")
                    && !x.starts_with("conformance/fixtures/")
            })
            .collect();
        let dirty = if files {
            // `git status --porcelain` lines are `XY path`.
            Some(
                git_lines(root, &["status", "--porcelain", "--no-renames", "--", ".gitea", "conformance"])?
                    .into_iter()
                    .map(|l| l.chars().skip(3).collect::<String>())
                    .collect(),
            )
        } else {
            None
        };
        Ok(GitGitea {
            root: root.to_path_buf(),
            apps,
            dirty,
            before_rev: if files { "HEAD".into() } else { base.into() },
            after_rev: (!files).then(|| head.to_string()),
            gitea,
            scripts,
            cache: RefCell::new(HashMap::new()),
        })
    }

    fn show(&self, rev: &str, path: &str) -> Option<String> {
        git_text(&self.root, &["show", &format!("{rev}:{path}")])
    }
}

impl GiteaSource for GitGitea {
    fn app_names(&self) -> Vec<String> {
        self.apps.clone()
    }
    fn files(&self) -> Vec<String> {
        self.gitea.clone()
    }
    fn scripts(&self) -> Vec<String> {
        self.scripts.clone()
    }
    fn dirty(&self, f: &str) -> Option<bool> {
        self.dirty.as_ref().map(|d| d.contains(f))
    }
    fn before(&self, f: &str) -> Option<String> {
        self.show(&self.before_rev, f)
    }
    fn after(&self, f: &str) -> Option<String> {
        let key = (true, f.to_string());
        if let Some(v) = self.cache.borrow().get(&key) {
            return v.clone();
        }
        let v = match &self.after_rev {
            Some(rev) => self.show(rev, f),
            None => std::fs::read_to_string(self.root.join(f)).ok(),
        };
        self.cache.borrow_mut().insert(key, v.clone());
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failing_git_read_is_an_error_never_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        for (files, head) in [(true, "HEAD"), (false, "HEAD")] {
            let err = GitGitea::new(dir.path(), vec![], files, "HEAD", head)
                .err()
                .expect("not a repository");
            assert!(err.to_string().contains("git "), "{err}");
        }
    }
}
