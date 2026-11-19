//! The variables that point git at one repository, folder or index (`git -C <dir>` does not override them).
//!
//! Git runs a hook with `GIT_DIR` set (a linked worktree's own git folder, for `pre-push`). A program the hook starts that runs
//! `git -C <other worktree> ...` would then read the hook's repository, not `<other worktree>`: `./dev check --pre-push` plans the
//! pushed commit in a throw-away worktree, and its `git ls-files` listed the files of the pushing worktree's HEAD (Paddock train 14,
//! 2026-11-17: "ui/projects/paddock-ui/scroll/ng-package.json could not be read"). A git call that names its folder removes these
//! first; so does the environment handed to the gates. They do not restrict the child (ER-47): they only point it somewhere.

use std::collections::HashMap;
use std::process::Command;

/// Names of the variables that select a repository, work tree, index or object store (one list, with the planner's reader).
pub use paddock_repo_graph::GIT_REPO_VARS as REPO_VARS;

/// A `git` child with no repository-selecting variable of ours: the one way paddock-dev starts git (a bare `command("git")` is
/// refused by the source check below, ER-21: the class "a hook's `GIT_DIR` reaches a `git -C <other folder>`" had four instances).
pub fn git() -> Command {
    let mut cmd = paddock_proc::command("git");
    remove_from(&mut cmd);
    cmd
}

/// Removes them from one child's environment.
pub fn remove_from(cmd: &mut Command) {
    for v in REPO_VARS {
        cmd.env_remove(v);
    }
}

/// An environment map without them.
pub fn without(env: &HashMap<String, String>) -> HashMap<String, String> {
    env.iter()
        .filter(|(k, _)| !REPO_VARS.contains(&k.to_ascii_uppercase().as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// The lines of `text` that start git directly (`command("git")`, `Command::new("git")`), with their 1-based numbers: what
/// [`git`] is for. `ToolCall::new("git")` and `Exec::run("git", ..)` are not named here: both strip the variables themselves.
#[cfg(test)]
fn bare_git_lines(text: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            let l = l.trim_start();
            // `TestCommand::new("git")` is the testkit's builder with a listed environment (ADR 0100): it passes no GIT_* variable.
            !l.starts_with("//") && (l.contains("command(\"git\")") || l.replace("TestCommand::new(", "").contains("Command::new(\"git\")"))
        })
        .map(|(i, _)| i + 1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                rust_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    /// Positive control: the scan sees a bare start in a sample, ignores a comment and the builder's own call.
    #[test]
    fn the_scan_finds_a_bare_git_start_and_nothing_else() {
        let sample = "let a = command(\"git\");\n// command(\"git\")\nlet b = std::process::Command::new(\"git\");\nlet c = git_env::git();\nlet d = TestCommand::new(\"git\");\n";
        assert_eq!(bare_git_lines(sample), [1, 3]);
    }

    /// Every source file of this tool starts git through [`git`] (the class: a hook's `GIT_DIR` reaching `git -C <folder>`).
    #[test]
    fn no_source_file_starts_git_without_the_builder() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        assert!(files.len() > 50, "the scan read {} files: wrong folder?", files.len());
        let mut bad = Vec::new();
        for f in files.iter().filter(|f| !f.ends_with("git_env.rs")) {
            for n in bare_git_lines(&std::fs::read_to_string(f).unwrap()) {
                bad.push(format!("{}:{n}", f.strip_prefix(&src).unwrap().display()));
            }
        }
        assert!(
            bad.is_empty(),
            "start git with `crate::git_env::git()`, not a bare command: {bad:?}"
        );
    }

    /// The `.rs` files below every folder named `tests` under `crates/`, `tools/` and `apps/` (not `target`, `node_modules` or fixtures).
    fn test_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
        fn walk(dir: &std::path::Path, in_tests: bool, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                if p.is_dir() {
                    if matches!(name.as_str(), "target" | "node_modules" | "fixtures" | ".git") {
                        continue;
                    }
                    walk(&p, in_tests || name == "tests", out);
                } else if in_tests && p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let mut out = Vec::new();
        for top in ["crates", "tools", "apps"] {
            walk(&root.join(top), false, &mut out);
        }
        out
    }

    /// Every test of the repository starts git through `paddock_testkit::scratch_git()` (or `TestCommand`), never bare: a bare start
    /// inherits `GIT_DIR` and `GIT_WORK_TREE` from a git hook and wrote into the real repository (2026-11-19).
    #[test]
    fn no_test_file_starts_git_without_the_builder() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = test_files(&root);
        for must in [
            "tools/paddock-dev/tests/it/main.rs",
            "tools/wiki/tests/it/common.rs",
            "crates/paddock-repo-graph/tests/it/adapters.rs",
        ] {
            assert!(
                files.iter().any(|f| f.ends_with(must)),
                "the scan did not read {must}: wrong folder?"
            );
        }
        let mut bad = Vec::new();
        for f in &files {
            for n in bare_git_lines(&std::fs::read_to_string(f).unwrap()) {
                bad.push(format!("{}:{n}", f.strip_prefix(&root).unwrap().display()));
            }
        }
        assert!(
            bad.is_empty(),
            "start git in a test with `paddock_testkit::scratch_git()` (it drops GIT_*, checks its folder and arguments) or `TestCommand`, not a bare command: {bad:?}"
        );
    }

    #[test]
    fn the_map_loses_exactly_the_repository_variables() {
        let env: HashMap<String, String> = [("GIT_DIR", "/x"), ("git_index_file", "i"), ("PATH", "/bin"), ("GIT_EDITOR", "true")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let got = without(&env);
        let mut keys: Vec<_> = got.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["GIT_EDITOR", "PATH"]);
    }

    #[test]
    fn a_command_loses_them_and_keeps_the_rest() {
        let mut cmd = paddock_proc::command("git");
        cmd.env("GIT_DIR", "/x").env("KEEP", "1");
        remove_from(&mut cmd);
        let envs: HashMap<_, _> = cmd
            .get_envs()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.is_some()))
            .collect();
        assert!(!envs["GIT_DIR"], "removed");
        assert!(envs["KEEP"]);
    }
}
