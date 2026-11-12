//! Which folders a new worktree checks out: everything, or a sparse cone chosen from `--for <area>` or the paths named.
//!
//! Work in `tools/`, `conformance/` or `.gitea/` needs the whole tree (the paddock-dev and conformance tests read the repository
//! around them), so a sparse cone that does not contain them is a trap: it fails later, in a test, with a missing file. The
//! default is therefore the full tree; a sparse checkout is asked for by `--sparse <folders>` or `--for <area>`, and a request
//! that names one of those folders becomes a full checkout.

use paddock_cli::CliError;

/// Folders whose work needs the full tree.
const NEEDS_FULL: [&str; 3] = ["tools", "conformance", ".gitea"];
/// The folders every sparse worktree has (AGENTS.md, "Working in the monorepo").
const BASE_CONE: [&str; 5] = ["crates", "ui", "scripts", "docs", "tools"];

/// The checkout of a new worktree.
#[derive(Debug, PartialEq, Eq)]
pub enum Checkout {
    /// The whole tree.
    Full,
    /// A cone of these folders.
    Sparse(Vec<String>),
}

/// Decides the checkout from `--full`, `--sparse` and `--for`. Nothing given means the full tree.
pub fn choose(full: bool, sparse: &[String], area: Option<&str>) -> Result<Checkout, CliError> {
    if full {
        return Ok(Checkout::Full);
    }
    let named = |p: &str| NEEDS_FULL.contains(&p.trim_matches('/'));
    if !sparse.is_empty() {
        return Ok(if sparse.iter().any(|p| named(p)) {
            Checkout::Full
        } else {
            Checkout::Sparse(sparse.to_vec())
        });
    }
    let Some(area) = area else { return Ok(Checkout::Full) };
    match area {
        "tools" | "conformance" | "ci" | "gitea" | "all" => Ok(Checkout::Full),
        "crates" | "ui" | "docs" | "scripts" => Ok(Checkout::Sparse(BASE_CONE.iter().map(|s| s.to_string()).collect())),
        a => match a.strip_prefix("app:").filter(|n| !n.is_empty() && !n.contains(['/', '\\', '.'])) {
            Some(app) => {
                let mut v: Vec<String> = BASE_CONE.iter().map(|s| s.to_string()).collect();
                v.push(format!("apps/{app}"));
                Ok(Checkout::Sparse(v))
            }
            None => Err(CliError::usage(format!("`--for {a}` is not an area."))
                .with_hint("Use tools, conformance, ci (full tree), crates, ui, docs, scripts (sparse) or app:<name> (sparse, one app).")),
        },
    }
}

/// The one line a test prints when the tree around it is sparse.
pub const SPARSE_MESSAGE: &str = "sparse worktree: run `git sparse-checkout disable` or use a full worktree";

/// `Some(SPARSE_MESSAGE)` when the checkout at `root` is sparse (the tests that read the whole repository check it first and
/// fail with this one line, not with a missing-file error deep in a test).
pub fn sparse_problem(root: &std::path::Path) -> Option<&'static str> {
    let out = paddock_proc::command("git")
        .arg("-C")
        .arg(root)
        .args(["config", "--get", "core.sparseCheckout"])
        .output()
        .ok()?;
    (String::from_utf8_lossy(&out.stdout).trim() == "true").then_some(SPARSE_MESSAGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_default_and_the_areas_that_read_the_whole_repository_are_full() {
        assert_eq!(choose(false, &[], None).unwrap(), Checkout::Full);
        for a in ["tools", "conformance", "ci"] {
            assert_eq!(choose(false, &[], Some(a)).unwrap(), Checkout::Full, "{a}");
        }
        assert_eq!(choose(true, &v(&["ui"]), Some("ui")).unwrap(), Checkout::Full);
    }

    #[test]
    fn a_sparse_request_that_names_tools_conformance_or_gitea_becomes_full() {
        for p in ["tools", "conformance/", ".gitea"] {
            assert_eq!(choose(false, &v(&["ui", p]), None).unwrap(), Checkout::Full, "{p}");
        }
        assert_eq!(
            choose(false, &v(&["ui", "crates"]), None).unwrap(),
            Checkout::Sparse(v(&["ui", "crates"]))
        );
    }

    #[test]
    fn a_sparse_checkout_is_reported_with_one_clear_line_and_a_full_one_is_not() {
        let d = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            assert!(
                paddock_proc::command("git")
                    .arg("-C")
                    .arg(d.path())
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success(),
                "{args:?}"
            );
        };
        git(&["init", "-q"]);
        assert_eq!(sparse_problem(d.path()), None);
        git(&["sparse-checkout", "init", "--cone"]);
        assert_eq!(sparse_problem(d.path()), Some(SPARSE_MESSAGE));
        assert_eq!(SPARSE_MESSAGE.lines().count(), 1);
        git(&["sparse-checkout", "disable"]);
        assert_eq!(sparse_problem(d.path()), None);
    }

    #[test]
    fn an_app_area_is_the_documented_cone_plus_the_app_and_a_bad_area_is_refused() {
        let Checkout::Sparse(s) = choose(false, &[], Some("app:dyno")).unwrap() else {
            panic!()
        };
        assert_eq!(s, v(&["crates", "ui", "scripts", "docs", "tools", "apps/dyno"]));
        for bad in ["nonsense", "app:", "app:../x"] {
            assert!(choose(false, &[], Some(bad)).is_err(), "{bad}");
        }
    }
}
