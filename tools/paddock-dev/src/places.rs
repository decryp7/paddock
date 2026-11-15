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
//! `./dev places` prints them for the scripts (vm-lock, vm-win, the vm-smoke harness, wp0-run); `--pinned` reads only the
//! repository's own git config (`git config --local`) and ignores every variable `./dev places` itself reads. Its trust, said
//! plainly: on Unix git runs with an otherwise empty environment and a fixed PATH of `/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin`,
//! so no variable picks the git or the config it reads; `/usr/local/bin` and `/opt/homebrew/bin` are writable by the user, so a
//! program the user put there is trusted. It guards against mistakes, not against the user (the Windows test VM's guards: the
//! user's own VMs must never be targeted by accident). On Windows `--pinned` is refused: nothing runs it there (its callers are
//! the Mac host's VM scripts) and Windows has no fixed system folder to take git from.
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

/// The dev-targets folder from the repository's own git config alone (`git config --local`; on Unix git runs with an empty
/// environment and the fixed PATH of the module doc, whose `/usr/local/bin` and `/opt/homebrew/bin` the user can write to). No
/// variable of `./dev places` moves it. For the guards of the Windows test VM, which run on the Mac host; refused on Windows.
pub fn dev_targets_pinned(from: &Path) -> Result<PathBuf, CliError> {
    if let Some(why) = pinned_unsupported() {
        return Err(CliError::usage(why));
    }
    dev_targets_of(None, pinned_worktree_root(from))
}

/// Why `--pinned` is refused on this platform: on Windows, with the cause and the next step; elsewhere `None`.
pub fn pinned_unsupported() -> Option<&'static str> {
    cfg!(windows).then_some(
        "`./dev places --pinned` is not supported on Windows: it runs git from fixed system folders, which Windows does not have, \
         and its only callers (the Windows test VM's guards) run on the Mac host. Next step: run it on the Mac host, or use \
         `./dev places` without --pinned.",
    )
}

#[cfg(not(windows))]
fn pinned_worktree_root(from: &Path) -> Option<PathBuf> {
    let mut cmd = paddock_proc::command("git");
    cmd.env_clear().env("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin");
    cmd.arg("-C").arg(from).args(["config", "--local", "--get", "paddock.worktreeRoot"]);
    let out = paddock_proc::run_bounded(
        cmd,
        paddock_proc::Limits::new(std::time::Duration::from_secs(30)).max_output(1 << 16),
    )
    .ok()?;
    let v = out.stdout_text().trim().to_string();
    (out.success() && !v.is_empty()).then(|| PathBuf::from(v))
}

#[cfg(windows)]
fn pinned_worktree_root(_from: &Path) -> Option<PathBuf> {
    None
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

/// The refusal of `--pinned`: one `::error::` line with the cause and the next step on `err`, nothing on standard output, exit 2.
fn pinned_refused(why: &str, err: &mut dyn std::io::Write) -> Report {
    let _ = writeln!(err, "::error::{why}");
    Report::new(json!({ "error": why }), String::new()).with_exit(paddock_cli::Exit::Usage)
}

/// `places`: the folders, or the one `--get` names.
pub fn command(a: &PlacesArgs) -> Result<Report, CliError> {
    let from = match &a.repo {
        Some(r) => r.clone(),
        None => std::env::current_dir().map_err(|e| CliError::failure(format!("cannot read the current folder: {e}.")))?,
    };
    if a.pinned
        && let Some(why) = pinned_unsupported()
    {
        return Ok(pinned_refused(why, &mut std::io::stderr()));
    }
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

/// Test helper: an invented folder that is absolute on this platform, the path itself on Unix and on drive `C:` on Windows (`/srv`
/// is not absolute there). Tests name invented folders only (C125). Shared by the places and brief_check tests; train 14 moves it
/// to `paddock_testkit::path`.
#[cfg(test)]
pub(crate) fn test_abs(unix: &str) -> String {
    if cfg!(windows) { format!("C:{unix}") } else { unix.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::git::Git;

    use super::test_abs as abs;

    #[test]
    fn the_named_folder_beats_the_worktree_root() {
        let got = dev_targets_of(Some(abs("/srv/dt")), Some(abs("/elsewhere/dev-targets/wt").into())).unwrap();
        assert_eq!(got, PathBuf::from(abs("/srv/dt")));
    }

    #[test]
    fn without_a_name_it_is_the_parent_of_the_worktree_root() {
        let got = dev_targets_of(None, Some(abs("/srv/dev-targets/wt").into())).unwrap();
        assert_eq!(got, PathBuf::from(abs("/srv/dev-targets")));
    }

    #[test]
    fn it_fails_closed_with_the_next_step() {
        for (set, root) in [
            (None, None),
            (Some("relative/dt".to_string()), None),
            (None, Some(PathBuf::from("/wt"))),
            (Some("/".to_string()), None),
            (Some(abs("/srv/../x")), None),
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
        git.text(&["config", "paddock.worktreeRoot", &abs("/srv/disk-a/dev-targets/wt")])
            .unwrap();
        assert_eq!(dev_targets(d.path(), &none).unwrap(), PathBuf::from(abs("/srv/disk-a/dev-targets")));
        let named = |k: &str| (k == DEV_TARGETS_ENV).then(|| abs("/srv/disk-b/dt"));
        assert_eq!(dev_targets(d.path(), &named).unwrap(), PathBuf::from(abs("/srv/disk-b/dt")));
        let main = main_checkout(d.path()).unwrap();
        let real = |p: &Path| paddock_env::winpath::real(p).unwrap();
        assert_eq!(real(&main), real(d.path()));
    }

    #[test]
    fn outside_a_repository_the_worktree_root_variable_still_works() {
        let d = tempfile::tempdir().unwrap();
        let env = |k: &str| (k == "PADDOCK_WORKTREE_ROOT").then(|| abs("/srv/disk-c/dev-targets/wt"));
        assert_eq!(dev_targets(d.path(), &env).unwrap(), PathBuf::from(abs("/srv/disk-c/dev-targets")));
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
        git.text(&["config", "paddock.worktreeRoot", &abs("/srv/disk-d/dev-targets/wt")])
            .unwrap();
        let linked = d.path().join("linked");
        git.text(&["worktree", "add", "-q", "--detach", &linked.to_string_lossy()]).unwrap();
        let real = |p: &Path| paddock_env::winpath::real(p).unwrap();
        assert_eq!(real(&main_checkout(&linked).unwrap()), real(&main));
        let none = |_: &str| None;
        assert_eq!(dev_targets(&linked, &none).unwrap(), PathBuf::from(abs("/srv/disk-d/dev-targets")));
        // `--pinned` is for the Mac host's VM guards; on Windows it is refused (its own test below).
        #[cfg(not(windows))]
        assert_eq!(dev_targets_pinned(&linked).unwrap(), PathBuf::from(abs("/srv/disk-d/dev-targets")));
    }

    #[test]
    fn the_layout_puts_the_vm_and_lock_folders_on_the_dev_disk_and_refuses_a_root_level_dev_targets_folder() {
        let d = tempfile::tempdir().unwrap();
        Git::at(d.path()).text(&["init", "-q"]).unwrap();
        let env = |k: &str| (k == DEV_TARGETS_ENV).then(|| abs("/srv/disk-e/dev-targets"));
        let l = Layout::derive(d.path(), &env, false);
        assert_eq!(l.get("dev-disk").unwrap(), Path::new(&abs("/srv/disk-e")));
        assert_eq!(l.get("vm-data").unwrap(), Path::new(&abs("/srv/disk-e/vm-data")));
        assert_eq!(l.get("lock-root").unwrap(), Path::new(&abs("/srv/disk-e/ci/locks/test-machines")));
        let named = |k: &str| match k {
            DEV_TARGETS_ENV => Some(abs("/srv/disk-e/dev-targets")),
            "PADDOCK_VM_DATA" => Some(abs("/srv/vms")),
            "PADDOCK_VM_LOCK_ROOT" => Some("relative".to_string()),
            _ => None,
        };
        let l = Layout::derive(d.path(), &named, false);
        assert_eq!(l.get("vm-data").unwrap(), Path::new(&abs("/srv/vms")));
        assert!(l.get("lock-root").is_err(), "a relative lock root is refused");
        // Pinned: every variable is ignored, and this repository has no worktree root.
        assert!(Layout::derive(d.path(), &named, true).get("vm-data").is_err());
        let top = |k: &str| (k == DEV_TARGETS_ENV).then(|| abs("/dt"));
        let l = Layout::derive(d.path(), &top, false);
        assert_eq!(l.get("dev-targets").unwrap(), Path::new(&abs("/dt")));
        assert!(
            l.get("dev-disk").is_err() && l.get("vm-data").is_err(),
            "no dev disk above a root-level folder"
        );
        assert!(l.get("nope").is_err());
    }

    #[test]
    fn the_pinned_refusal_is_one_error_line_with_the_cause_and_the_next_step() {
        let why = "`./dev places --pinned` is not supported here. Next step: run it on the Mac host.";
        let mut err = Vec::new();
        let r = pinned_refused(why, &mut err);
        assert_eq!(r.exit, paddock_cli::Exit::Usage);
        assert!(r.text.is_empty(), "nothing on standard output");
        let err = String::from_utf8(err).unwrap();
        assert_eq!(err.lines().count(), 1, "{err}");
        assert!(err.starts_with("::error::") && err.contains("Next step"), "{err}");
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_pinned_is_refused() {
        let d = tempfile::tempdir().unwrap();
        Git::at(d.path()).text(&["init", "-q"]).unwrap();
        Git::at(d.path())
            .text(&["config", "paddock.worktreeRoot", &abs("/srv/disk-f/dev-targets/wt")])
            .unwrap();
        let why = pinned_unsupported().unwrap();
        assert!(why.contains("not supported on Windows") && why.contains("Next step"), "{why}");
        let e = dev_targets_pinned(d.path()).unwrap_err();
        assert!(e.text().contains("not supported on Windows"), "{}", e.text());
        assert!(Layout::derive(d.path(), &|_| None, true).get("vm-data").is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn elsewhere_pinned_is_supported() {
        assert!(pinned_unsupported().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn windows_roots_and_relative_forms_are_refused_and_drive_paths_are_absolute() {
        // A named folder: the filesystem roots, drive-relative, rooted without a drive, and a bare Unix path are refused.
        for bad in [
            abs("/"),
            "C:/".to_string(),
            "C:\\".to_string(),
            "C:srv".to_string(),
            "\\srv".to_string(),
            "/srv/dt".to_string(),
        ] {
            assert!(dev_targets_of(Some(bad.clone()), None).is_err(), "{bad:?} must be refused");
        }
        // A worktree root whose parent is a root (`C:\wt`) or is not absolute is refused too.
        for bad in ["C:/wt", "C:\\wt", "C:srv\\wt", "\\srv\\wt", "/srv/dt/wt"] {
            assert!(dev_targets_of(None, Some(PathBuf::from(bad))).is_err(), "{bad:?} must be refused");
        }
        // A UNC share root is a root: a folder directly under it has no dev disk above it.
        let unc = dev_targets_of(Some("\\\\h\\s\\dt".to_string()), None).unwrap();
        assert!(dev_disk_of(&unc).is_err(), "{unc:?}: no dev disk above a UNC share's root");
        // Drive paths work with either separator.
        assert_eq!(
            dev_targets_of(None, Some(PathBuf::from("C:\\srv\\dt\\wt"))).unwrap(),
            PathBuf::from("C:\\srv\\dt")
        );
        assert_eq!(dev_targets_of(Some(abs("/srv/dt")), None).unwrap(), PathBuf::from("C:/srv/dt"));
    }
}
