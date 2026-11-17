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

#[cfg(test)]
mod tests {
    use super::*;

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
