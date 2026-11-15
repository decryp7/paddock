//! Where this machine keeps its development folders, derived and never named (C125, rule 40). On 2026-11-14 the dev data moved to
//! another disk and every hard-coded disk path went stale: the brief check refused the new scratch folders, `./dev disk` and
//! `./dev load` looked at disks that were gone.
//!
//! - The dev-targets folder (worktrees, build folders, scratch, the disk index): `PADDOCK_DEV_TARGETS`, else the parent of the
//!   worktree root (git config `paddock.worktreeRoot`, then `PADDOCK_WORKTREE_ROOT`, as [`crate::worktree::ops::resolve_root`]).
//! - The dev disk: the parent of the dev-targets folder; the test VMs (`PADDOCK_VM_DATA`, else `<dev disk>/vm-data`) and the test
//!   machines' lock (`PADDOCK_VM_LOCK_ROOT`, else `<dev disk>/ci/locks/test-machines`) are below it.
//! - The main checkout: the folder that holds the git common directory ([`Repo::discover`]).
//!
//! `./dev places` prints them for the scripts (vm-lock, vm-win, the vm-smoke harness); `--pinned` reads only the
//! repository's own git config, with an empty environment for git, so no variable can move a guarded folder (the Windows test VM).
//! Each fails closed with the next step when it cannot be derived. The one other copy of the rule is the hooks'
//! `.claude/hooks/lib/paddockpaths.py` (hooks run before any binary is built; docs/known-copies.toml, `dev-places`;
//! .claude/hooks/paddockpaths-parity.test.sh runs both on one table). The layout itself is described once, in docs/dev-setup.md.

use crate::worktree::git::Repo;
use clap::Args;
use paddock_cli::{CliError, Report};
use serde_json::json;
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

/// The dev-targets folder from the repository's own git config alone (`git config --local`, git run with an empty environment):
/// no environment variable can move it. For the guards of the Windows test VM.
pub fn dev_targets_pinned(from: &Path) -> Result<PathBuf, CliError> {
    dev_targets_of(None, pinned_worktree_root(from))
}

fn pinned_worktree_root(from: &Path) -> Option<PathBuf> {
    let mut cmd = paddock_proc::command("git");
    cmd.arg("-C").arg(from).args(["config", "--local", "--get", "paddock.worktreeRoot"]);
    cmd.env_clear().env("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin");
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        cmd.env("SystemRoot", root);
    }
    let out = paddock_proc::run_bounded(
        cmd,
        paddock_proc::Limits::new(std::time::Duration::from_secs(30)).max_output(1 << 16),
    )
    .ok()?;
    let v = out.stdout_text().trim().to_string();
    (out.success() && !v.is_empty()).then(|| PathBuf::from(v))
}

/// The pure rule (the hooks' copy is `paddockpaths.dev_targets_of`): the named folder (absolute, not `/`, no `..`), else the
/// parent of the worktree root (absolute, no `..`, and the parent is not `/`); a trailing `/` is dropped.
pub fn dev_targets_of(set: Option<String>, worktree_root: Option<PathBuf>) -> Result<PathBuf, CliError> {
    let (path, from) = match (set, worktree_root) {
        (Some(v), _) => (PathBuf::from(trim_slash(v.trim())), DEV_TARGETS_ENV.to_string()),
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

fn trim_slash(v: &str) -> &str {
    let t = v.trim_end_matches('/');
    if t.is_empty() && !v.is_empty() { "/" } else { t }
}

/// The dev disk: the parent of the dev-targets folder, refused when that is the filesystem root.
pub fn dev_disk_of(dev_targets: &Path) -> Result<PathBuf, CliError> {
    dev_targets
        .parent()
        .filter(|p| p.parent().is_some())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            CliError::usage(format!(
                "The dev-targets folder {} has no dev disk above it (it is directly under the filesystem root).",
                dev_targets.display()
            ))
            .with_hint(HINT)
        })
}

/// The main checkout of the repository at `from`: the folder that holds the git common directory.
pub fn main_checkout(from: &Path) -> Result<PathBuf, CliError> {
    Repo::discover(from).map(|r| r.primary).map_err(|e| {
        CliError::usage(format!("The main checkout cannot be derived: {e}."))
            .with_hint("Run the command inside a checkout of the repository.")
    })
}

/// The folders `./dev places` prints, each derived or refused.
pub struct Layout {
    /// `dev-targets`.
    pub dev_targets: Result<PathBuf, CliError>,
    /// `dev-disk`.
    pub dev_disk: Result<PathBuf, CliError>,
    /// `vm-data`.
    pub vm_data: Result<PathBuf, CliError>,
    /// `lock-root`.
    pub lock_root: Result<PathBuf, CliError>,
    /// `main-checkout`.
    pub main_checkout: Result<PathBuf, CliError>,
}

/// The names `--get` takes, in print order.
pub const NAMES: [&str; 5] = ["dev-targets", "dev-disk", "vm-data", "lock-root", "main-checkout"];

impl Layout {
    /// The layout of the repository at `from`; with `pinned`, every environment variable is ignored and git config is read alone.
    pub fn derive(from: &Path, env: &dyn Fn(&str) -> Option<String>, pinned: bool) -> Layout {
        let none = |_: &str| None;
        let env: &dyn Fn(&str) -> Option<String> = if pinned { &none } else { env };
        let dev_targets = if pinned { dev_targets_pinned(from) } else { dev_targets(from, env) };
        let dev_disk = dev_targets.as_ref().map_err(clone_err).and_then(|d| dev_disk_of(d));
        let below = |var: &str, rel: &str| match env(var).filter(|v| !v.trim().is_empty()) {
            Some(v) => dev_targets_of(Some(v), None).map_err(|e| CliError::usage(format!("{var}: {}", e.text()))),
            None => dev_disk.as_ref().map_err(clone_err).map(|d| d.join(rel)),
        };
        Layout {
            vm_data: below("PADDOCK_VM_DATA", "vm-data"),
            lock_root: below("PADDOCK_VM_LOCK_ROOT", "ci/locks/test-machines"),
            main_checkout: main_checkout(from),
            dev_targets,
            dev_disk,
        }
    }

    /// The folder of one name, or why it cannot be derived (an unknown name is a usage error).
    pub fn get(&self, name: &str) -> Result<&Path, CliError> {
        let r = match name {
            "dev-targets" => &self.dev_targets,
            "dev-disk" => &self.dev_disk,
            "vm-data" => &self.vm_data,
            "lock-root" => &self.lock_root,
            "main-checkout" => &self.main_checkout,
            other => {
                return Err(CliError::usage(format!("`{other}` is not a place.")).with_hint(format!("Use one of: {}.", NAMES.join(", "))));
            }
        };
        r.as_deref().map_err(clone_err)
    }
}

fn clone_err(e: &CliError) -> CliError {
    CliError::usage(e.text())
}

/// Arguments of `places`.
#[derive(Args)]
pub struct PlacesArgs {
    /// Print only this folder (dev-targets, dev-disk, vm-data, lock-root, main-checkout); fails when it cannot be derived.
    #[arg(long)]
    get: Option<String>,
    /// Read the repository's own git config alone, ignoring every environment variable (the Windows test VM's guards).
    #[arg(long)]
    pinned: bool,
    /// The repository (default: the current folder's).
    #[arg(long)]
    repo: Option<PathBuf>,
}

/// `places`: the folders, or the one `--get` names.
pub fn command(a: &PlacesArgs) -> Result<Report, CliError> {
    let from = match &a.repo {
        Some(r) => r.clone(),
        None => std::env::current_dir().map_err(|e| CliError::failure(format!("cannot read the current folder: {e}.")))?,
    };
    let layout = Layout::derive(&from, &|k| std::env::var(k).ok(), a.pinned);
    if let Some(name) = &a.get {
        let p = layout.get(name)?;
        return Ok(Report::new(
            json!({ name.as_str(): p.display().to_string() }),
            p.display().to_string(),
        ));
    }
    let mut data = serde_json::Map::new();
    let mut text = String::new();
    for name in NAMES {
        match layout.get(name) {
            Ok(p) => {
                data.insert(name.into(), json!(p.display().to_string()));
                text.push_str(&format!("{name}\t{}\n", p.display()));
            }
            Err(e) => {
                data.insert(name.into(), json!({ "error": e.text() }));
                text.push_str(&format!("{name}\tnot derived: {}\n", e.text()));
            }
        }
    }
    Ok(Report::new(serde_json::Value::Object(data), text.trim_end().to_string()))
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

    #[test]
    fn from_a_linked_worktree_the_main_checkout_is_the_primary_and_the_root_is_shared() {
        let d = tempfile::tempdir().unwrap();
        let main = d.path().join("main");
        std::fs::create_dir(&main).unwrap();
        let git = Git::at(&main);
        git.text(&["init", "-q"]).unwrap();
        git.text(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ])
        .unwrap();
        git.text(&["config", "paddock.worktreeRoot", "/srv/disk-d/dev-targets/wt"]).unwrap();
        let linked = d.path().join("linked");
        git.text(&["worktree", "add", "-q", "--detach", &linked.to_string_lossy()]).unwrap();
        let real = |p: &Path| paddock_env::winpath::real(p).unwrap();
        assert_eq!(real(&main_checkout(&linked).unwrap()), real(&main));
        let none = |_: &str| None;
        assert_eq!(dev_targets(&linked, &none).unwrap(), PathBuf::from("/srv/disk-d/dev-targets"));
        assert_eq!(dev_targets_pinned(&linked).unwrap(), PathBuf::from("/srv/disk-d/dev-targets"));
    }

    #[test]
    fn the_layout_puts_the_vm_and_lock_folders_on_the_dev_disk_and_refuses_a_root_level_dev_targets_folder() {
        let d = tempfile::tempdir().unwrap();
        Git::at(d.path()).text(&["init", "-q"]).unwrap();
        let env = |k: &str| (k == DEV_TARGETS_ENV).then(|| "/srv/disk-e/dev-targets".to_string());
        let l = Layout::derive(d.path(), &env, false);
        assert_eq!(l.get("dev-disk").unwrap(), Path::new("/srv/disk-e"));
        assert_eq!(l.get("vm-data").unwrap(), Path::new("/srv/disk-e/vm-data"));
        assert_eq!(l.get("lock-root").unwrap(), Path::new("/srv/disk-e/ci/locks/test-machines"));
        let named = |k: &str| match k {
            DEV_TARGETS_ENV => Some("/srv/disk-e/dev-targets".to_string()),
            "PADDOCK_VM_DATA" => Some("/srv/vms".to_string()),
            "PADDOCK_VM_LOCK_ROOT" => Some("relative".to_string()),
            _ => None,
        };
        let l = Layout::derive(d.path(), &named, false);
        assert_eq!(l.get("vm-data").unwrap(), Path::new("/srv/vms"));
        assert!(l.get("lock-root").is_err(), "a relative lock root is refused");
        // Pinned: every variable is ignored, and this repository has no worktree root.
        assert!(Layout::derive(d.path(), &named, true).get("vm-data").is_err());
        let top = |k: &str| (k == DEV_TARGETS_ENV).then(|| "/dt".to_string());
        let l = Layout::derive(d.path(), &top, false);
        assert_eq!(l.get("dev-targets").unwrap(), Path::new("/dt"));
        assert!(
            l.get("dev-disk").is_err() && l.get("vm-data").is_err(),
            "no dev disk above a root-level folder"
        );
        assert!(l.get("nope").is_err());
    }
}
