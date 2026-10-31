//! `dev host prune cap|worktrees`: the command line and the executor around the pure plan in [`crate::prune`]. Rules and the
//! differences from the four scripts are written down there.
//!
//! `--dry-run` runs every step except the removals, on the real folders, and prints what the real run would do.

use crate::prune::{self, Action, Candidate, CapPlan, DirNode, FileId, PruneError, RelPath, Unit};
use crate::prune_fs::{self, Kind};
use clap::{Args, Subcommand};
use paddock_cli::{CliError, Report};
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

fn error(e: PruneError) -> CliError {
    match e {
        PruneError::BadCap(_) | PruneError::BadApp(_) | PruneError::BadKeep(_) | PruneError::NotAbsolute(_) | PruneError::DotDot(_) => {
            CliError::usage(format!("{e}."))
        }
        PruneError::TooShallow(_) | PruneError::ContainsHome(_) | PruneError::UserFolder(_) => {
            CliError::refused(format!("{e}.")).with_hint("Give a build folder of your own, never a home or system folder.")
        }
        PruneError::TooLarge(_) | PruneError::Io(_) => CliError::failure(format!("{e}.")),
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
            "errors": run.errors,
            "unreadable": run.unreadable,
        }),
        text.join("\n"),
    )
}

/// `host prune cap`.
pub fn cap(a: &CapArgs) -> Result<Report, CliError> {
    let max = prune::parse_cap(&a.max_gb).map_err(error)?;
    let unit = if a.unit == "kb" { Unit::Kb } else { Unit::Gb };
    let home = a.home.clone().or_else(user_home);
    let run = cap_run(a, max, unit, home.as_deref()).map_err(error)?;
    Ok(cap_report(a, Some(max), unit, run))
}

fn cap_run(a: &CapArgs, max: u64, unit: Unit, home: Option<&Path>) -> Result<CapRun, PruneError> {
    let shown = a.root.display().to_string();
    let ci = a.ci_home.is_some();
    if ci && a.root.as_os_str().is_empty() {
        return Ok(CapRun::none("none", Some(prune::NO_FOLDER_MESSAGE.to_string())));
    }
    let slot = a.slot.as_deref();
    let left_alone = || Ok(CapRun::none("left-alone", Some(prune::not_owned_message(&shown, slot))));
    prune::check_root(&a.root, home)?;
    if prune_fs::kind(&a.root) != Kind::Dir {
        return if ci { left_alone() } else { Ok(CapRun::none("absent", None)) };
    }
    let root = std::fs::canonicalize(&a.root).map_err(|e| PruneError::Io(format!("{shown}: {e}")))?;
    prune::check_root(&root, home)?;
    if let Some(ci_home) = &a.ci_home {
        match std::fs::canonicalize(ci_home) {
            Ok(h) if prune::owns(&h, &root, slot) => {}
            _ => return left_alone(),
        }
    }
    // The CI form works on the resolved path, as the script did; the plain form on the path it was given.
    let work: &Path = if ci { &root } else { &a.root };
    let display = work.display().to_string();
    let mut run = CapRun::none("none", None);
    let mut gone: BTreeSet<RelPath> = BTreeSet::new();
    let bundle = prune::bundle_path();
    if a.drop_bundles {
        let target = work.join("dist").join("bundle");
        let real_dir = prune_fs::kind(&work.join("dist")) == Kind::Dir && prune_fs::kind(&target) == Kind::Dir;
        if real_dir {
            if a.dry_run {
                gone.insert(bundle.clone());
                run.removed.push(joined(&bundle));
            } else if let Err(e) = prune_fs::remove_rel(work, &bundle, None) {
                run.errors.push(e);
            } else {
                run.removed.push(joined(&bundle));
            }
        }
    }
    let scan = prune_fs::scan(work)?;
    run.unreadable = scan.unreadable;
    let plan = prune::plan_cap(&scan.tree, &gone, max, unit);
    run.size_before = Some(plan.size_before);
    run.size_after = Some(plan.size_before);
    if plan.over {
        run.action = "incremental";
        run.lines.push(prune::over_message(&display, plan.size_before, max));
        let left = remove_incremental(work, &scan.tree, &plan, a.dry_run, &mut run, (max, unit))?;
        let (size_after, id) = (left.size, left.id);
        run.size_after = Some(size_after);
        if left.remove_folder {
            run.action = "folder";
            run.lines.push(prune::still_message(&display, size_after));
            if a.dry_run {
                run.folder_removed = true;
            } else {
                run.folder_removed = remove_folder_itself(work, id, &mut run.errors);
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
    (max, unit): (u64, Unit),
) -> Result<Removal, PruneError> {
    for rel in &plan.incremental {
        if dry {
            run.removed.push(joined(rel));
        } else if let Err(e) = prune_fs::remove_rel(work, rel, prune_fs::id_of(tree, rel)) {
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
    let size = prune::plan_cap(&again.tree, &BTreeSet::new(), max, unit).size_before;
    Ok(Removal {
        size,
        remove_folder: size >= max,
        id: again.tree.id,
    })
}

fn remove_folder_itself(work: &Path, id: Option<FileId>, errors: &mut Vec<String>) -> bool {
    match prune_fs::remove_rel(work, &Vec::new(), id) {
        Ok(()) => true,
        Err(e) => {
            errors.push(e);
            false
        }
    }
}

/// `host prune worktrees`.
pub fn worktrees(a: &WorktreeArgs) -> Result<Report, CliError> {
    if !prune::valid_app(&a.app) {
        return Err(error(PruneError::BadApp(a.app.clone())));
    }
    prune::check_keep(&a.keep).map_err(error)?;
    let home = a.home.clone().or_else(user_home);
    let repo = match &a.repo {
        Some(r) => r.clone(),
        None => std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder is unknown: {e}.")))?,
    };
    let mut candidates: Vec<Candidate> = Vec::new();
    if let Some(h) = &home {
        let app = a.app.clone();
        candidates.extend(prune_fs::list_dirs(h, &|n| prune::home_candidate(&app, n)).map_err(error)?);
    }
    candidates.extend(prune_fs::list_dirs(&repo.join("target"), &prune::target_candidate).map_err(error)?);
    let rows = prune::plan_rows(&a.app, &a.keep, a.remove, a.dry_run, &candidates);
    let mut errors = Vec::new();
    for row in rows.iter().filter(|r| r.action == Action::Removing) {
        if let Some(why) = refuse_candidate(&row.path, home.as_deref()) {
            errors.push(why);
        } else if let Err(e) = prune_fs::remove_rel(&row.path, &Vec::new(), None) {
            errors.push(e);
        }
    }
    let mut lines: Vec<String> = rows.iter().map(prune::format_row).collect();
    lines.extend(errors.iter().map(|e| format!("could not remove {e}")));
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
fn refuse_candidate(path: &Path, home: Option<&Path>) -> Option<String> {
    let real = match std::fs::canonicalize(path) {
        Ok(r) => r,
        Err(e) => return Some(format!("{}: {e}", path.display())),
    };
    match prune::check_root(&real, home) {
        Ok(()) => None,
        Err(e) => Some(format!("{}: {e}", path.display())),
    }
}
