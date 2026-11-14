//! Where this machine keeps its development folders, derived and never named (C125, rule 40). On 2026-11-14 the dev data moved to
//! another disk and every hard-coded disk path went stale: the brief check refused the new scratch folders, `./dev disk` and
//! `./dev load` looked at disks that were gone.
//!
//! - The dev-targets folder (worktrees, build folders, scratch, the disk index): `PADDOCK_DEV_TARGETS`, else the parent of the
//!   worktree root (git config `paddock.worktreeRoot`, then `PADDOCK_WORKTREE_ROOT`, as [`crate::worktree::ops::resolve_root`]).
//! - The main checkout: the folder that holds the git common directory ([`Repo::discover`]).
//!
//! Each fails closed with the next step when it cannot be derived. Copies of the dev-targets rule: `.claude/hooks/lib/paddockpaths.py`,
//! `scripts/dev-places.sh` and `scripts/dev-places.mjs` (docs/known-copies.toml, `dev-places`). The layout itself is described once,
//! in docs/dev-setup.md.

use crate::worktree::git::Repo;
use paddock_cli::CliError;
use std::path::{Path, PathBuf};

/// The environment variable that names the dev-targets folder outright.
pub const DEV_TARGETS_ENV: &str = "PADDOCK_DEV_TARGETS";

const HINT: &str = "Run `git config paddock.worktreeRoot <dev-targets>/wt` once in the main checkout (docs/dev-setup.md), or set \
                    PADDOCK_DEV_TARGETS to the folder that holds the worktrees, build folders and scratch.";

/// The dev-targets folder of the repository at `from` (see the module doc), or why it cannot be derived.
pub fn dev_targets(from: &Path, env: &dyn Fn(&str) -> Option<String>) -> Result<PathBuf, CliError> {
    let set = env(DEV_TARGETS_ENV).filter(|v| !v.trim().is_empty());
    let root = if set.is_some() {
        None
    } else {
        match Repo::discover(from) {
            Ok(repo) => crate::worktree::ops::resolve_root(&repo, None, env).ok(),
            Err(_) => env("PADDOCK_WORKTREE_ROOT").filter(|v| !v.is_empty()).map(PathBuf::from),
        }
    };
    dev_targets_of(set, root)
}

/// The pure rule: the named folder, else the parent of the worktree root; both absolute, never the filesystem root.
pub fn dev_targets_of(set: Option<String>, worktree_root: Option<PathBuf>) -> Result<PathBuf, CliError> {
    let (path, from) = match (set, worktree_root) {
        (Some(v), _) => (PathBuf::from(v.trim()), DEV_TARGETS_ENV.to_string()),
        (None, Some(root)) => {
            let parent = root
                .parent()
                .filter(|p| p.parent().is_some())
                .map(Path::to_path_buf)
                .ok_or_else(|| {
                    CliError::usage(format!(
                        "The worktree root {} has no parent folder to serve as the dev-targets folder.",
                        root.display()
                    ))
                    .with_hint(HINT)
                })?;
            (parent, format!("the parent of the worktree root {}", root.display()))
        }
        (None, None) => {
            return Err(CliError::usage("The dev-targets folder cannot be derived: no worktree root is set.").with_hint(HINT));
        }
    };
    if !path.is_absolute() || path.parent().is_none() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(CliError::usage(format!(
            "The dev-targets folder {} ({from}) is not an absolute folder below the filesystem root.",
            path.display()
        ))
        .with_hint(HINT));
    }
    Ok(path)
}

/// The main checkout of the repository at `from`: the folder that holds the git common directory.
pub fn main_checkout(from: &Path) -> Result<PathBuf, CliError> {
    Repo::discover(from).map(|r| r.primary).map_err(|e| {
        CliError::usage(format!("The main checkout cannot be derived: {e}."))
            .with_hint("Run the command inside a checkout of the repository.")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::git::Git;

    #[test]
    fn the_named_folder_beats_the_worktree_root() {
        let got = dev_targets_of(Some("/srv/dt".into()), Some("/elsewhere/dev-targets/wt".into())).unwrap();
        assert_eq!(got, PathBuf::from("/srv/dt"));
    }

    #[test]
    fn without_a_name_it_is_the_parent_of_the_worktree_root() {
        let got = dev_targets_of(None, Some("/srv/dev-targets/wt".into())).unwrap();
        assert_eq!(got, PathBuf::from("/srv/dev-targets"));
    }

    #[test]
    fn it_fails_closed_with_the_next_step() {
        for (set, root) in [
            (None, None),
            (Some("relative/dt".to_string()), None),
            (None, Some(PathBuf::from("/wt"))),
            (Some("/".to_string()), None),
            (Some("/srv/../x".to_string()), None),
        ] {
            let e = dev_targets_of(set.clone(), root.clone()).unwrap_err().to_string();
            assert!(e.contains("dev-targets"), "{set:?} {root:?}: {e}");
        }
        let e = dev_targets_of(None, None).unwrap_err();
        assert!(format!("{e:?}").contains("paddock.worktreeRoot"), "{e:?}");
    }

    #[test]
    fn a_repository_derives_it_from_git_config_and_its_main_checkout_from_the_common_dir() {
        let d = tempfile::tempdir().unwrap();
        let git = Git::at(d.path());
        git.text(&["init", "-q"]).unwrap();
        let none = |_: &str| None;
        assert!(dev_targets(d.path(), &none).is_err(), "no worktree root set: refused");
        git.text(&["config", "paddock.worktreeRoot", "/srv/disk-a/dev-targets/wt"]).unwrap();
        assert_eq!(dev_targets(d.path(), &none).unwrap(), PathBuf::from("/srv/disk-a/dev-targets"));
        let named = |k: &str| (k == DEV_TARGETS_ENV).then(|| "/srv/disk-b/dt".to_string());
        assert_eq!(dev_targets(d.path(), &named).unwrap(), PathBuf::from("/srv/disk-b/dt"));
        let main = main_checkout(d.path()).unwrap();
        let real = |p: &Path| paddock_env::winpath::real(p).unwrap();
        assert_eq!(real(&main), real(d.path()));
    }

    #[test]
    fn outside_a_repository_the_worktree_root_variable_still_works() {
        let d = tempfile::tempdir().unwrap();
        let env = |k: &str| (k == "PADDOCK_WORKTREE_ROOT").then(|| "/srv/disk-c/dev-targets/wt".to_string());
        assert_eq!(dev_targets(d.path(), &env).unwrap(), PathBuf::from("/srv/disk-c/dev-targets"));
        assert!(main_checkout(d.path()).is_err());
    }
}
