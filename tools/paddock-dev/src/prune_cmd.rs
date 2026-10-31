//! `dev host prune cap|worktrees`: the command line and the executor around the pure plan in [`crate::prune`]. Rules and the
//! differences from the four scripts are written down there.
//!
//! `--dry-run` runs every step except the removals, on the real folders, and prints what the real run would do.

use crate::prune::{self, CapPlan, DirNode, FileId, PruneError, RelPath, Unit};
use crate::prune_fs::{self, Kind};
use crate::prune_root::{check_not_mount_point, check_root};
use crate::prune_worktrees::{self as wt, Action, Candidate};
use crate::slots::owns;
use clap::{Args, Subcommand};
use paddock_cli::{CliError, Exit, Report};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// `host prune`'s two forms.
#[derive(Subcommand)]
pub enum PruneCommand {
    /// Keep one build folder under a size cap: incremental caches first, then the folder (prune-build-folders.sh and, with
    /// --ci-home, prune-ci-build-folder.sh).
    Cap(CapArgs),
    /// List, or with --remove delete, the build folders of finished worktrees (prune-build-folders.ps1).
    Worktrees(WorktreeArgs),
}

/// `cap`'s arguments.
#[derive(Args)]
pub struct CapArgs {
    /// The build folder (the CI form accepts an empty value: a job that set no build folder).
    #[arg(long, value_parser = path_allow_empty())]
    pub root: PathBuf,
    /// The cap in whole gigabytes.
    #[arg(long)]
    pub max_gb: String,
    /// The unit of the cap: gb, or kb for tests.
    #[arg(long, default_value = "gb", value_parser = ["gb", "kb"], hide = true)]
    pub unit: String,
    /// The CI form: only a `target-<name>-<slot>` folder directly in this CI home is touched, else a notice and exit 0.
    #[arg(long)]
    pub ci_home: Option<PathBuf>,
    /// This job's slot (s1, s2, ...), for the CI form.
    #[arg(long)]
    pub slot: Option<String>,
    /// First remove <folder>/dist/bundle (installers already uploaded). CI form only.
    #[arg(long, requires = "ci_home")]
    pub drop_bundles: bool,
    /// Say what would be removed; remove nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// The user's home folder for the safety check (default: the HOME / USERPROFILE of this process).
    #[arg(long)]
    pub home: Option<PathBuf>,
}

/// `worktrees`' arguments.
#[derive(Args)]
pub struct WorktreeArgs {
    /// The app: finds `~/.<app>-target*` and `<repo>/target/*`.
    #[arg(long)]
    pub app: String,
    /// The repository whose `target/` is searched (default: the current folder).
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// The user's home folder (default: the HOME / USERPROFILE of this process).
    #[arg(long)]
    pub home: Option<PathBuf>,
    /// Delete the folders not named by --keep (without it the folders are only listed).
    #[arg(long)]
    pub remove: bool,
    /// Suffixes of folders still in use: `6c` keeps `.<app>-target-6c*` and `agent-6c*`.
    #[arg(long, value_delimiter = ',')]
    pub keep: Vec<String>,
    /// Say what would be removed; remove nothing.
    #[arg(long)]
    pub dry_run: bool,
}

fn path_allow_empty() -> impl clap::builder::TypedValueParser<Value = PathBuf> {
    use clap::builder::TypedValueParser;
    clap::builder::OsStringValueParser::new().map(PathBuf::from)
}

fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// The home folder, resolved: `--home`, else HOME / USERPROFILE. No usable home is an error (the guard fails closed).
fn resolve_home(arg: Option<&Path>) -> Result<PathBuf, PruneError> {
    let home = arg
        .map(Path::to_path_buf)
        .or_else(user_home)
        .ok_or_else(|| PruneError::NoHome("HOME and USERPROFILE are not set".into()))?;
    std::fs::canonicalize(&home).map_err(|e| PruneError::NoHome(format!("{}: {e}", home.display())))
}

fn error(e: PruneError) -> CliError {
    match e {
        PruneError::BadCap(_) | PruneError::BadApp(_) | PruneError::BadKeep(_) | PruneError::NotAbsolute(_) | PruneError::DotDot(_) => {
            CliError::usage(format!("{e}."))
        }
        PruneError::TooShallow(_)
        | PruneError::ContainsHome(_)
        | PruneError::UserFolder(_)
        | PruneError::MountPoint(_)
        | PruneError::NoHome(_) => {
            CliError::refused(format!("{e}.")).with_hint("Give a build folder of your own, never a home or system folder.")
        }
        PruneError::TooLarge(_) | PruneError::Io(_) => CliError::failure(format!("{e}.")),
    }
}

/// The one place a folder is removed: the real file system, or a fake in tests that fails on purpose.
trait Remover {
    /// Removes the folder at `rel` below `root` (empty `rel`: `root` itself); `id` is its scanned identity.
    fn remove(&self, root: &Path, rel: &RelPath, id: Option<FileId>) -> Result<(), String>;
}

/// Removes for real, after the identity checks of [`prune_fs::remove_rel`].
struct FsRemover;

impl Remover for FsRemover {
    fn remove(&self, root: &Path, rel: &RelPath, id: Option<FileId>) -> Result<(), String> {
        prune_fs::remove_rel(root, rel, id)
    }
}

fn joined(rel: &[String]) -> String {
    rel.join("/")
}

/// The result of one `cap` run, before it becomes a report.
struct CapRun {
    lines: Vec<String>,
    action: &'static str,
    size_before: Option<u64>,
    size_after: Option<u64>,
    removed: Vec<String>,
    folder_removed: bool,
    folder_failed: bool,
    not_cargo: bool,
    errors: Vec<String>,
    unreadable: u64,
}

impl CapRun {
    fn none(action: &'static str, line: Option<String>) -> CapRun {
        CapRun {
            lines: line.into_iter().collect(),
            action,
            size_before: None,
            size_after: None,
            removed: Vec::new(),
            folder_removed: false,
            folder_failed: false,
            not_cargo: false,
            errors: Vec::new(),
            unreadable: 0,
        }
    }
}

fn cap_report(a: &CapArgs, max: Option<u64>, unit: Unit, run: CapRun) -> Report {
    let mut text = run.lines.clone();
    text.extend(run.errors.iter().map(|e| format!("prune: could not remove {e}")));
    Report::new(
        json!({
            "command": "host prune cap",
            "dryRun": a.dry_run,
            "mode": if a.ci_home.is_some() { "ci" } else { "plain" },
            "folder": a.root.display().to_string(),
            "action": run.action,
            "unit": unit.name(),
            "max": max,
            "sizeBefore": run.size_before,
            "sizeAfter": run.size_after,
            "removed": run.removed,
            "folderRemoved": run.folder_removed,
            "folderRemovalFailed": run.folder_failed,
            "notCargoFolder": run.not_cargo,
            "errors": run.errors,
            "unreadable": run.unreadable,
        }),
        text.join("\n"),
    )
}

/// `host prune cap`.
pub fn cap(a: &CapArgs) -> Result<Report, CliError> {
    cap_with(a, &FsRemover)
}

fn cap_with(a: &CapArgs, rm: &dyn Remover) -> Result<Report, CliError> {
    let unit = if a.unit == "kb" { Unit::Kb } else { Unit::Gb };
    if a.ci_home.is_some() {
        return Ok(ci_cap(a, unit, rm));
    }
    let max = prune::parse_cap(&a.max_gb).map_err(error)?;
    let run = resolve_home(a.home.as_deref())
        .and_then(|h| cap_run(a, max, unit, &h, rm))
        .map_err(error)?;
    // Only a failed removal of the whole folder fails the plain form, as the script's final `rm -rf` did; the report
    // (removed, errors, sizes) is still printed first.
    // A folder that is not a Cargo build folder is refused (exit 4), also with the report.
    let exit = if run.folder_failed {
        Exit::Failure
    } else if run.not_cargo {
        Exit::Refused
    } else {
        Exit::Ok
    };
    let report = cap_report(a, Some(max), unit, run);
    Ok(report.with_exit(exit))
}

/// The CI form never fails: whatever is wrong becomes a notice and nothing is removed (exit 0, as the script).
fn ci_cap(a: &CapArgs, unit: Unit, rm: &dyn Remover) -> Report {
    let shown = a.root.display().to_string();
    let slot = a.slot.as_deref();
    if a.root.as_os_str().is_empty() {
        return cap_report(a, None, unit, CapRun::none("none", Some(prune::NO_FOLDER_MESSAGE.into())));
    }
    if a.max_gb.is_empty() {
        return cap_report(a, None, unit, CapRun::none("none", Some(prune::NO_CAP_MESSAGE.into())));
    }
    let Ok(max) = prune::parse_cap(&a.max_gb) else {
        let line = format!("prune: the cap must be a whole number, got '{}'", a.max_gb);
        return cap_report(a, None, unit, CapRun::none("none", Some(line)));
    };
    if a.slot.is_none() {
        return cap_report(a, Some(max), unit, CapRun::none("none", Some(prune::NO_SLOT_MESSAGE.into())));
    }
    let run = match resolve_home(a.home.as_deref()).and_then(|h| cap_run(a, max, unit, &h, rm)) {
        Ok(run) => run,
        Err(e @ (PruneError::Io(_) | PruneError::TooLarge(_))) => CapRun::none("failed", Some(format!("prune: {e}; leaving it alone"))),
        Err(_) => CapRun::none("left-alone", Some(prune::not_owned_message(&shown, slot))),
    };
    cap_report(a, Some(max), unit, run)
}

fn cap_run(a: &CapArgs, max: u64, unit: Unit, home: &Path, rm: &dyn Remover) -> Result<CapRun, PruneError> {
    let shown = a.root.display().to_string();
    let ci = a.ci_home.is_some();
    let slot = a.slot.as_deref();
    let left_alone = || Ok(CapRun::none("left-alone", Some(prune::not_owned_message(&shown, slot))));
    check_root(&a.root, home, &prune_fs::same_file)?;
    // `kind` drops trailing separators, so `--root <link>/` is seen as the link and never followed.
    if prune_fs::kind(&a.root) != Kind::Dir {
        return if ci { left_alone() } else { Ok(CapRun::none("absent", None)) };
    }
    let root = std::fs::canonicalize(&a.root).map_err(|e| PruneError::Io(format!("{shown}: {e}")))?;
    check_root(&root, home, &prune_fs::same_file)?;
    check_not_mount_point(&root, &prune_fs::is_mount_point)?;
    let root_id = prune_fs::dir_id(&root);
    if let Some(ci_home) = &a.ci_home {
        match std::fs::canonicalize(ci_home) {
            Ok(h) if owns(&h, &root, slot) => {}
            _ => return left_alone(),
        }
    }
    // Both forms work on the resolved path; the plain form prints the path it was given, as the script did.
    let work: &Path = &root;
    let display = if ci { work.display().to_string() } else { shown.clone() };
    let mut run = CapRun::none("none", None);
    let mut gone: BTreeSet<RelPath> = BTreeSet::new();
    let bundle = prune::bundle_path();
    prune::check_unchanged(root_id, prune_fs::dir_id(work), &shown, "checked")?;
    if a.drop_bundles {
        let target = work.join("dist").join("bundle");
        let real_dir = prune_fs::kind(&work.join("dist")) == Kind::Dir && prune_fs::kind(&target) == Kind::Dir;
        if real_dir {
            if a.dry_run {
                gone.insert(bundle.clone());
                run.removed.push(joined(&bundle));
            } else if let Err(e) = rm.remove(work, &bundle, None) {
                run.errors.push(e);
            } else {
                run.removed.push(joined(&bundle));
            }
        }
    }
    let scan = prune_fs::scan(work)?;
    prune::check_unchanged(root_id, scan.tree.id, &shown, "checked")?;
    run.unreadable = scan.unreadable;
    let plan = prune::plan_cap(&scan.tree, &gone, max, unit);
    run.size_before = Some(plan.size_before);
    run.size_after = Some(plan.size_before);
    if plan.over {
        run.action = "incremental";
        run.lines.push(prune::over_message(&display, plan.size_before, max));
        let left = remove_incremental(work, &scan.tree, &plan, a.dry_run, &mut run, (max, unit, root_id), rm)?;
        let (size_after, id) = (left.size, left.id);
        run.size_after = Some(size_after);
        if left.remove_folder {
            if !prune_fs::is_cargo_folder(work) {
                run.action = "not-cargo";
                run.not_cargo = true;
                run.lines.push(prune::not_cargo_message(&display));
                run.lines.push(prune::NOT_CARGO_HINT.to_string());
                return Ok(run);
            }
            run.action = "folder";
            run.lines.push(prune::still_message(&display, size_after));
            if a.dry_run {
                run.folder_removed = true;
            } else {
                run.folder_removed = remove_folder_itself(work, id, &mut run.errors, rm);
                run.folder_failed = !run.folder_removed;
            }
        }
    }
    Ok(run)
}

/// What is left after the incremental caches: the figure, whether the folder goes too, and the folder's scanned identity.
struct Removal {
    size: u64,
    remove_folder: bool,
    id: Option<FileId>,
}

/// Rule 3, then the second measurement of rule 4: predicted in a dry run, measured again after a real removal.
fn remove_incremental(
    work: &Path,
    tree: &DirNode,
    plan: &CapPlan,
    dry: bool,
    run: &mut CapRun,
    (max, unit, root_id): (u64, Unit, Option<FileId>),
    rm: &dyn Remover,
) -> Result<Removal, PruneError> {
    for rel in &plan.incremental {
        if dry {
            run.removed.push(joined(rel));
        } else if let Err(e) = rm.remove(work, rel, prune_fs::id_of(tree, rel)) {
            run.errors.push(e);
        } else {
            run.removed.push(joined(rel));
        }
    }
    if dry {
        return Ok(Removal {
            size: plan.size_after,
            remove_folder: plan.remove_folder,
            id: tree.id,
        });
    }
    let again = prune_fs::scan(work)?;
    prune::check_unchanged(root_id, again.tree.id, &work.display().to_string(), "pruned")?;
    let size = prune::plan_cap(&again.tree, &BTreeSet::new(), max, unit).size_before;
    Ok(Removal {
        size,
        remove_folder: size >= max,
        id: again.tree.id,
    })
}

fn remove_folder_itself(work: &Path, id: Option<FileId>, errors: &mut Vec<String>, rm: &dyn Remover) -> bool {
    match rm.remove(work, &Vec::new(), id) {
        Ok(()) => true,
        Err(e) => {
            errors.push(e);
            false
        }
    }
}

/// `host prune worktrees`.
pub fn worktrees(a: &WorktreeArgs) -> Result<Report, CliError> {
    if !wt::valid_app(&a.app) {
        return Err(error(PruneError::BadApp(a.app.clone())));
    }
    wt::check_keep(&a.keep).map_err(error)?;
    let home = resolve_home(a.home.as_deref()).map_err(error)?;
    let repo = match &a.repo {
        Some(r) => r.clone(),
        None => std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder is unknown: {e}.")))?,
    };
    let app = a.app.clone();
    let (listed, mut errors) = prune_fs::list_dirs(&home, &|n| wt::home_candidate(&app, n));
    let (more, more_errors) = prune_fs::list_dirs(&repo.join("target"), &wt::target_candidate);
    let candidates: Vec<Candidate> = listed
        .into_iter()
        .chain(more)
        .map(|l| Candidate {
            path: l.path,
            name: l.name,
            bytes: l.bytes,
            symlink: l.symlink,
            cargo: l.cargo,
        })
        .collect();
    errors.extend(more_errors);
    let rows = wt::plan_rows(&a.app, &a.keep, a.remove, a.dry_run, &candidates);
    for row in rows.iter().filter(|r| r.action == Action::Removing) {
        if let Some(why) = refuse_candidate(&row.path, &home) {
            errors.push(why);
        } else if let Err(e) = prune_fs::remove_rel(&row.path, &Vec::new(), None) {
            errors.push(e);
        }
    }
    let mut lines: Vec<String> = rows.iter().map(wt::format_row).collect();
    lines.extend(errors.iter().map(|e| format!("could not remove or list {e}")));
    let data: Vec<Value> = rows
        .iter()
        .map(|r| json!({"path": r.path.display().to_string(), "bytes": r.bytes, "action": r.action.word()}))
        .collect();
    Ok(Report::new(
        json!({"command": "host prune worktrees", "dryRun": a.dry_run, "remove": a.remove, "app": a.app, "folders": data, "errors": errors}),
        lines.join("\n"),
    ))
}

/// A candidate is removed only when its resolved path passes the same root checks as `cap`'s.
fn refuse_candidate(path: &Path, home: &Path) -> Option<String> {
    match std::fs::canonicalize(path) {
        Err(e) => Some(format!("{}: {e}", path.display())),
        Ok(real) => check_root(&real, home, &prune_fs::same_file)
            .and_then(|()| check_not_mount_point(&real, &prune_fs::is_mount_point))
            .err()
            .map(|e| format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::fs;

    /// Fails the removals it is told to (an empty `rel` is the folder itself), does the others for real and records every call.
    struct Fake {
        fail: Vec<RelPath>,
        calls: RefCell<Vec<RelPath>>,
    }

    impl Fake {
        fn failing(fail: &[&[&str]]) -> Fake {
            Fake {
                fail: fail.iter().map(|p| p.iter().map(|s| s.to_string()).collect()).collect(),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl Remover for Fake {
        fn remove(&self, root: &Path, rel: &RelPath, id: Option<FileId>) -> Result<(), String> {
            self.calls.borrow_mut().push(rel.clone());
            if self.fail.contains(rel) {
                Err(format!("{}: injected failure", joined(rel)))
            } else {
                FsRemover.remove(root, rel, id)
            }
        }
    }

    /// `<root>/ci/target-app-s1` with two incremental folders, 700 KiB in all, and a Cargo marker unless `marker` is false.
    fn build(root: &Path, marker: bool) -> PathBuf {
        let folder = root.join("ci").join("target-app-s1");
        for (rel, kib) in [("debug/incremental/a", 400), ("release/incremental/b", 100), ("debug/deps/c", 200)] {
            let p = folder.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, vec![1u8; kib * 1024]).unwrap();
        }
        if marker {
            fs::write(folder.join("CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55").unwrap();
        }
        folder
    }

    fn args(root: &Path, folder: &Path, max: &str, ci: bool) -> CapArgs {
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        CapArgs {
            root: folder.to_path_buf(),
            max_gb: max.into(),
            unit: "kb".into(),
            ci_home: ci.then(|| root.join("ci")),
            slot: ci.then(|| "s1".to_string()),
            drop_bundles: false,
            dry_run: false,
            home: Some(home),
        }
    }

    fn scratch() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let p = fs::canonicalize(t.path()).unwrap();
        (t, p)
    }

    #[test]
    fn a_failed_incremental_removal_is_reported_and_the_run_still_succeeds() {
        let (_t, root) = scratch();
        let folder = build(&root, true);
        // 700 KiB; only release/incremental can go, which leaves 600: under the 650 cap, so the folder stays.
        let fake = Fake::failing(&[&["debug", "incremental"]]);
        let report = cap_with(&args(&root, &folder, "650", false), &fake).unwrap();
        assert_eq!(report.exit, Exit::Ok);
        let v = report.json();
        assert_eq!(v["ok"], true);
        assert_eq!(v["errors"].as_array().unwrap().len(), 1);
        assert_eq!(v["folderRemovalFailed"], false);
        assert!(report.text.contains("could not remove"), "{}", report.text);
        assert!(
            !fake.calls.borrow().contains(&Vec::<String>::new()),
            "the folder itself was not removed"
        );
    }

    #[test]
    fn a_failed_whole_folder_removal_fails_the_plain_form_and_still_carries_the_report() {
        let (_t, root) = scratch();
        let folder = build(&root, true);
        let fake = Fake::failing(&[&[]]);
        let report = cap_with(&args(&root, &folder, "0", false), &fake).unwrap();
        assert_eq!(report.exit, Exit::Failure);
        let v = report.json();
        assert_eq!(
            (v["ok"].clone(), v["exit"].clone(), v["kind"].clone()),
            (false.into(), 1.into(), "failure".into())
        );
        assert_eq!(v["folderRemovalFailed"], true);
        assert_eq!(v["removed"].as_array().unwrap().len(), 2, "the report names what did go");
        assert!(
            report.text.contains("could not remove") && report.text.contains("removing"),
            "{}",
            report.text
        );
    }

    #[test]
    fn the_ci_form_exits_zero_when_the_whole_folder_cannot_be_removed() {
        let (_t, root) = scratch();
        let folder = build(&root, true);
        let report = cap_with(&args(&root, &folder, "0", true), &Fake::failing(&[&[]])).unwrap();
        assert_eq!(report.exit, Exit::Ok);
        assert!(report.text.contains("could not remove"), "{}", report.text);
        assert_eq!(report.json()["folderRemovalFailed"], true);
    }

    #[test]
    fn a_folder_without_a_cargo_marker_keeps_the_folder_and_removes_the_caches() {
        let (_t, root) = scratch();
        let folder = build(&root, false);
        let fake = Fake::failing(&[]);
        let plain = cap_with(&args(&root, &folder, "0", false), &fake).unwrap();
        assert_eq!(plain.exit, Exit::Refused);
        let v = plain.json();
        assert_eq!(
            (v["ok"].clone(), v["action"].clone(), v["notCargoFolder"].clone()),
            (false.into(), "not-cargo".into(), true.into())
        );
        assert!(plain.text.contains("not a Cargo build folder") && plain.text.contains("Run it on a Cargo target folder"));
        assert!(
            !fake.calls.borrow().contains(&Vec::<String>::new()),
            "the folder itself is never removed"
        );
        let ci = cap_with(&args(&root, &folder, "0", true), &Fake::failing(&[])).unwrap();
        assert_eq!(ci.exit, Exit::Ok, "the CI form never fails");
        assert!(ci.text.contains("not a Cargo build folder"));
    }

    #[test]
    fn a_dry_run_without_a_marker_predicts_the_refusal_and_removes_nothing() {
        let (_t, root) = scratch();
        let folder = build(&root, false);
        let mut a = args(&root, &folder, "0", false);
        a.dry_run = true;
        let fake = Fake::failing(&[]);
        let report = cap_with(&a, &fake).unwrap();
        assert_eq!(report.exit, Exit::Refused);
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn the_marker_may_be_the_rustc_info_file() {
        let (_t, root) = scratch();
        let folder = build(&root, false);
        fs::write(folder.join(".rustc_info.json"), "{}").unwrap();
        let fake = Fake::failing(&[]);
        let report = cap_with(&args(&root, &folder, "0", false), &fake).unwrap();
        assert_eq!(report.exit, Exit::Ok);
        assert!(fake.calls.borrow().contains(&Vec::<String>::new()));
    }
}
