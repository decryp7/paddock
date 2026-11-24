//! The base of a train's landing dispatch (ADR 0117 WP-8, D11, audit gaps G1 and G14).
//!
//! A full dispatch of ci.yml on a `train/*` ref used to carry no base, so the plan job read it as "a manual run without a base:
//! everything" (runs 8610, 8635 and 8647). The landing run now tests the train's diff from main's last commit that the one
//! definition of green accepts ([`green::find_base`]: complete, and green or green with fix items): the dispatch asks for that
//! commit and sends it as the `landing_base` input. It is not the `base` input, which makes a run a member run (narrowed nextest,
//! never complete); the plan job plans the range and a push to main's `need` is the run's own plan, so the run is complete for the
//! tree it changed and the push to main reuses it (`ci reuse`, ER-49).
//!
//! No complete base, or runs that cannot be read, sends nothing: the run plans everything, the safe side. Nightly runs, tags, member
//! runs, reruns, `os` parts and an app's own gates never get one.

use super::dispatch::is_train;

/// The workflow input that carries the base of a landing run.
pub const LANDING_BASE: &str = "landing_base";

/// Whether this dispatch is a landing run: a full ci.yml dispatch on a train ref. Any input that makes it another kind of run (a
/// `base` makes a member run, `since_run` a rerun, `os` one part, `app` one app's gates) or a base already given leaves it alone.
pub fn is_landing(workflow: &str, git_ref: &str, inputs: &[(String, String)]) -> bool {
    let set = |k: &str| inputs.iter().any(|(key, v)| key == k && !v.trim().is_empty());
    workflow == "ci.yml" && is_train(git_ref) && !["base", "since_run", "os", "app", LANDING_BASE].iter().any(|k| set(k))
}

/// The inputs of a dispatch with the landing base added when it is a landing run and `lookup` finds one (the commit and the run
/// that decided it). Without one the inputs are unchanged and `warn` says why: the run tests everything.
pub fn with_landing_base(
    workflow: &str,
    git_ref: &str,
    mut inputs: Vec<(String, String)>,
    lookup: &mut dyn FnMut() -> Result<Option<(String, u64)>, String>,
    warn: &mut dyn FnMut(&str),
) -> Vec<(String, String)> {
    if !is_landing(workflow, git_ref, &inputs) {
        return inputs;
    }
    match lookup() {
        Ok(Some((sha, run))) => {
            warn(&format!(
                "landing run: testing the diff from {} (the last complete green ci run on main, run {run})",
                sha.chars().take(10).collect::<String>()
            ));
            inputs.push((LANDING_BASE.into(), sha));
        }
        Ok(None) => warn("::warning::landing run: no complete green ci run among main's newest commits; this run tests everything"),
        Err(why) => warn(&format!(
            "::warning::landing run: main's ci runs could not be read ({why}); this run tests everything"
        )),
    }
    inputs
}

/// Main's own first-parent commits, newest first, the tip included (`origin/main` of the checkout; empty when git cannot say, then no
/// base is found). A stale `origin/main` only gives an older base: a wider diff, never a missed one.
pub fn main_chain(root: &std::path::Path) -> Vec<String> {
    let n = super::green::MAX_COMMITS.to_string();
    crate::plan_repo::git_text(root, &["rev-list", "--first-parent", "-n", &n, "origin/main"])
        .map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

/// [`with_landing_base`] over the real runs: reads main's runs through the API and the checkout's history.
pub fn with_real_landing_base(
    http: &mut dyn crate::wait::Http,
    target: &super::dispatch::Target<'_>,
    root: &std::path::Path,
    workflow: &str,
    git_ref: &str,
    inputs: Vec<(String, String)>,
) -> Vec<(String, String)> {
    let mut warn = |l: &str| crate::log_text::eline(l);
    with_landing_base(
        workflow,
        git_ref,
        inputs,
        &mut || {
            let mut client = super::client::Client::new(http, target.server, target.repo, target.token);
            let chain = main_chain(root);
            crate::plan_cmd::green_base_over(&mut client, root, &chain, &mut |_| {})
        },
        &mut warn,
    )
}
