//! The generator step of `./dev merge check` (ER-40): which generated or pinned files does this merge make stale? The paths the
//! merge changed against its first parent (the train before it) mark the generators of `docs/generators.toml` that are due; their checks run in the
//! checkout, which must be the merge result (a check reads files, not trees). A checkout that is not the result, or a registry
//! that cannot be read, is NOT MEASURED, never a pass (ER-33).

use super::{Merge, git::Git};
use crate::generators::{
    self,
    registry::{Cost, PATH},
    run::Runner,
};
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
pub fn run(root: &Path, git: &Git, merge: &Merge, heavy: bool, runner: &dyn Runner, log: &(dyn Fn(&str) + Sync)) -> Option<Step> {
    if !root.join(PATH).exists() {
        return None;
    }
    let reg = match generators::load(root) {
        Ok(r) => r,
        Err(e) => return Some(unmeasured(&e.message, &format!("fix {PATH}"))),
    };
    if let Some((why, next)) = checkout_problem(git, merge) {
        return Some(unmeasured(&why, &next));
    }
    let [first, _second] = merge.parents.as_slice() else {
        return Some(unmeasured("the merge does not have two parents", "check a two-parent merge"));
    };
    // Due-ness is what the merge changed against the train before it (the first parent), as ER-8 says: a member cut from an older
    // train base must not make everything the train changed since then look due (train 15, 2026-11-19: 21 generators for a docs-only member).
    let changed = git.changed_paths(first, &merge.tree);
    let changed = match changed {
        Ok(c) => c,
        Err(e) => {
            return Some(unmeasured(
                &format!("the paths the merge changed cannot be listed: {e}"),
                "run `git fetch --unshallow` if the clone is shallow",
            ));
        }
    };
    // A heavy generator (it builds a workspace: minutes, and again for every merge of a train) is deferred to the one
    // `./dev gen check --base origin/main` that runs beside the member gate: its due set is the union of every merge's, so nothing a merge
    // made due is left unchecked, and a stale file there still names its generator and inputs (ER-36, ER-20; docs/benchmarks.md).
    let due = reg.due(&changed);
    let deferred: Vec<String> = if heavy {
        Vec::new()
    } else {
        due.iter()
            .filter(|g| g.cost == Cost::Heavy && g.manual.is_none())
            .map(|g| g.id.clone())
            .collect()
    };
    let now: Vec<String> = due.iter().map(|g| g.id.clone()).filter(|id| !deferred.contains(id)).collect();
    let mut step = if !deferred.is_empty() && now.is_empty() {
        // Nothing but heavy entries is due: nothing starts.
        Step {
            text: Vec::new(),
            data: json!({"outcome": "clean", "registered": reg.generators.len(), "due": [], "manual": [], "results": []}),
            exit: Exit::Ok,
        }
    } else {
        // `only` names the entries to run whatever the change is, so with nothing deferred the plain due selection is kept.
        let only: &[String] = if deferred.is_empty() { &[] } else { &now };
        match generators::check_with(&reg, root, &changed, false, only, runner, log) {
            Ok(r) => Step {
                text: r.text.lines().map(str::to_string).collect(),
                data: r.data,
                exit: r.exit,
            },
            Err(e) => return Some(unmeasured(&e.message, "fix the registry")),
        }
    };
    if !deferred.is_empty() {
        step.text.push(format!(
            "gen check: deferred, heavy ({}): not run in this merge check. The final `./dev gen check --base origin/main` runs them once on the train (or pass --heavy-generators to run them here).",
            deferred.join(", ")
        ));
        step.data["deferred"] = json!(deferred);
    }
    Some(step)
}

/// Why the checkout is not the merge result, and the next step: `None` when HEAD's tree is the merge's and nothing tracked is
/// modified. A check that reads files (the generator checks, the member gate) is meaningless on any other checkout (ER-33).
pub(super) fn checkout_problem(git: &Git, merge: &Merge) -> Option<(String, String)> {
    let head_tree = match git.text(&["rev-parse", "HEAD^{tree}"]) {
        Ok(t) => t.trim().to_string(),
        Err(e) => {
            return Some((
                format!("HEAD cannot be read: {e}"),
                "run from the worktree that holds the merge".into(),
            ));
        }
    };
    if head_tree != merge.tree {
        return Some((
            "the checkout is not the merge result (its HEAD tree differs from the merge's)".into(),
            "check out the merge commit in this worktree (the integrator's), then run it again; or pass --no-generators / leave out --gates to skip the step on purpose".into(),
        ));
    }
    match git.text(&["status", "--porcelain", "--untracked-files=no"]) {
        Ok(s) if s.trim().is_empty() => None,
        Ok(_) => Some((
            "the checkout has uncommitted changes, so the files are not the merge result".into(),
            "commit or discard them, then run it again".into(),
        )),
        Err(e) => Some((
            format!("git status failed: {e}"),
            "run from the worktree that holds the merge".into(),
        )),
    }
}
