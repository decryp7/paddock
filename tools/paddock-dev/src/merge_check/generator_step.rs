//! The generator step of `./dev merge check` (ER-40): which generated or pinned files does this merge make stale? The paths the
//! two parents changed since their merge base mark the generators of `docs/generators.toml` that are due; their checks run in the
//! checkout, which must be the merge result (a check reads files, not trees). A checkout that is not the result, or a registry
//! that cannot be read, is NOT MEASURED, never a pass (ER-33).

use super::{Merge, git::Git};
use crate::generators::{self, registry::PATH, run::Runner};
use paddock_cli::Exit;
use serde_json::{Value, json};
use std::path::Path;

/// What the step found, to be joined to the detectors' findings.
pub struct Step {
    /// Lines for the text output (`::error::` lines for what needs a person).
    pub text: Vec<String>,
    /// The structured result, under `"generators"`.
    pub data: Value,
    /// `Exit::Ok`, `Exit::Findings` (a stale file) or `Exit::NotMeasured`.
    pub exit: Exit,
}

/// The step was not asked for or cannot apply: said out loud, never silent, and not a failure.
pub fn not_run(why: &str) -> Step {
    Step {
        text: vec![format!("generator checks not run ({why})")],
        data: json!({"outcome": "not run", "why": why}),
        exit: Exit::Ok,
    }
}

fn unmeasured(why: &str, next: &str) -> Step {
    Step {
        text: vec![format!(
            "::error::generator checks NOT MEASURED: {why}. Next step: {next}; a merge whose generated files were not checked is not a merge that passed."
        )],
        data: json!({"outcome": "not measured", "why": why}),
        exit: Exit::NotMeasured,
    }
}

/// Runs the generator checks a merge makes due. `None` when the repository declares no registry (nothing to check).
pub fn run(root: &Path, git: &Git, merge: &Merge, runner: &dyn Runner, log: &(dyn Fn(&str) + Sync)) -> Option<Step> {
    if !root.join(PATH).exists() {
        return None;
    }
    let reg = match generators::load(root) {
        Ok(r) => r,
        Err(e) => return Some(unmeasured(&e.message, &format!("fix {PATH}"))),
    };
    // A check reads files: the checkout must hold exactly the merge result.
    let head_tree = match git.text(&["rev-parse", "HEAD^{tree}"]) {
        Ok(t) => t.trim().to_string(),
        Err(e) => {
            return Some(unmeasured(
                &format!("HEAD cannot be read: {e}"),
                "run from the worktree that holds the merge",
            ));
        }
    };
    if head_tree != merge.tree {
        return Some(unmeasured(
            "the checkout is not the merge result (its HEAD tree differs from the merge's)",
            "check out the merge commit in this worktree (the integrator's), then run it again; or pass --no-generators to skip this step on purpose",
        ));
    }
    match git.text(&["status", "--porcelain", "--untracked-files=no"]) {
        Ok(s) if s.trim().is_empty() => {}
        Ok(_) => {
            return Some(unmeasured(
                "the checkout has uncommitted changes, so the files are not the merge result",
                "commit or discard them, then run it again",
            ));
        }
        Err(e) => {
            return Some(unmeasured(
                &format!("git status failed: {e}"),
                "run from the worktree that holds the merge",
            ));
        }
    }
    let [first, second] = merge.parents.as_slice() else {
        return Some(unmeasured("the merge does not have two parents", "check a two-parent merge"));
    };
    let changed = git
        .text(&["merge-base", first, second])
        .and_then(|b| git.changed_paths(b.trim(), &merge.tree));
    let changed = match changed {
        Ok(c) => c,
        Err(e) => {
            return Some(unmeasured(
                &format!("the paths the merge changed cannot be listed: {e}"),
                "run `git fetch --unshallow` if the clone is shallow",
            ));
        }
    };
    match generators::check_with(&reg, root, &changed, false, &[], runner, log) {
        Ok(r) => Some(Step {
            text: r.text.lines().map(str::to_string).collect(),
            data: r.data,
            exit: r.exit,
        }),
        Err(e) => Some(unmeasured(&e.message, "fix the registry")),
    }
}
