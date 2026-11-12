//! `./dev disk`: watch free space, track what uses it, and clean up what is safe to remove.
//!
//! - [`check`]: free space at a job's start or before a heavy build (warn under 200 GiB, refuse under 50 GiB by default);
//! - [`record`]: the `.paddock-owner.toml` in every folder the tooling makes (the source of truth);
//! - [`index`]: the SQLite cache of folders and size history (rebuilt from the records when missing or corrupt);
//! - [`scan`], [`measure`], [`state`]: find the folders, measure them in parallel, work out what state each is in;
//! - [`report`], [`clean`], [`adopt`]: the commands built on them.
//!
//! Nothing here deletes without `clean --apply`, and `clean` never touches a folder without a record.

pub mod adopt;
pub mod check;
pub mod clean;
pub mod config;
pub mod index;
pub mod measure;
pub mod record;
pub mod report;
pub mod scan;
pub mod state;

use clap::{Args, Subcommand};
use paddock_cli::{CliError, Report};
use std::path::PathBuf;

/// `dev disk ...`
#[derive(Subcommand)]
pub enum DiskCommand {
    /// Free space at job start or before a heavy build: one warning under 200 GiB (with the ten largest folders), one error
    /// and exit 1 under 50 GiB. Thresholds: --warn-gib / --refuse-gib or PADDOCK_DISK_WARN_GIB / PADDOCK_DISK_REFUSE_GIB.
    Check(check::CheckArgs),
    /// Every tracked folder per disk with size, owner, branch, last use and state, the reclaimable totals and what grew most
    /// in the last 7 days. Reads the usage index (rebuilt from the owner records when it is missing or corrupt).
    Report(report::ReportArgs),
    /// Refresh the usage index: the folders, their states and their sizes (parallel, bounded, a time limit per folder).
    Scan(ScanArgs),
    /// Remove folders that are merged or stale-pushed and not live. A dry run unless --apply; never touches unsaved or orphan
    /// folders, CI slots, runners, sccache, VMs or Docker.
    Clean(clean::CleanArgs),
    /// Write owner records for the existing worktrees and the build folders named after them.
    Adopt(adopt::AdoptArgs),
    /// Write (or, when there is one, touch) the owner record of a folder a tool made, for example a CARGO_TARGET_DIR.
    Record(RecordArgs),
}

/// `disk scan`'s arguments.
#[derive(Args)]
pub struct ScanArgs {
    #[command(flatten)]
    common: config::CommonArgs,
    /// Measure every folder again, not only those whose size is older than 6 hours.
    #[arg(long)]
    force: bool,
    /// Only the folders and their states; no sizes.
    #[arg(long, conflicts_with = "force")]
    no_sizes: bool,
}

/// `disk record`'s arguments.
#[derive(Args)]
pub struct RecordArgs {
    /// The folder (made if it does not exist).
    dir: PathBuf,
    /// What it is: worktree, build, slot or scratch.
    #[arg(long, default_value = "build")]
    kind: String,
    /// Who made it: an agent id (default PADDOCK_AGENT_ID) or a job.
    #[arg(long)]
    owner: Option<String>,
    /// The branch it is for.
    #[arg(long, default_value = "")]
    branch: String,
    /// The head commit.
    #[arg(long, default_value = "")]
    head: String,
    /// The worktree it serves (a build folder).
    #[arg(long, default_value = "")]
    link: String,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn say(line: &str) {
    eprintln!("{line}");
}

/// Runs a `disk` command other than `check` (which prints its own annotations, see [`check::run`]).
pub fn command(c: DiskCommand) -> Result<Report, CliError> {
    match c {
        DiskCommand::Check(_) => Err(CliError::failure("`disk check` was dispatched through the report path.")),
        DiskCommand::Report(a) => report::run(&a, &env, &mut say),
        DiskCommand::Clean(a) => clean::run(&a, &env, &mut say),
        DiskCommand::Adopt(a) => adopt::run(&a, &env),
        DiskCommand::Scan(a) => scan_command(&a),
        DiskCommand::Record(a) => record_command(&a),
    }
}

fn scan_command(a: &ScanArgs) -> Result<Report, CliError> {
    let s = config::Settings::resolve(&a.common, &env);
    let now = crate::worktree::note::now_secs();
    let (index, mut notes) = index::Index::open_or_recover(&s.index, now);
    let mut index = index;
    let mode = if a.no_sizes {
        scan::Sizes::Skip
    } else if a.force {
        scan::Sizes::All
    } else {
        scan::Sizes::Stale
    };
    let snap = scan::scan(&s, mode, index.as_mut(), &mut say);
    notes.extend(snap.notes.iter().cloned());
    let t = report::totals(&snap);
    let text = format!(
        "Scanned {} folder(s) into {}.\n{}",
        snap.folders.len(),
        if index.is_some() {
            s.index.display().to_string()
        } else {
            "no index (it could not be used)".to_string()
        },
        notes.iter().map(|n| format!("note: {n}\n")).collect::<String>()
    );
    Ok(Report::new(
        serde_json::json!({"folders": snap.folders.len(), "index": s.index.display().to_string(), "indexed": index.is_some(),
            "states": t.iter().map(|(k, v)| (k.word().to_string(), serde_json::json!(v.count))).collect::<serde_json::Map<_, _>>(),
            "notes": notes}),
        text,
    ))
}

fn record_command(a: &RecordArgs) -> Result<Report, CliError> {
    let kind = record::Kind::parse(&a.kind)
        .ok_or_else(|| CliError::usage(format!("`{}` is not a kind.", a.kind)).with_hint("Use worktree, build, slot or scratch."))?;
    std::fs::create_dir_all(&a.dir).map_err(|e| CliError::failure(format!("Cannot create {}: {e}.", a.dir.display())))?;
    match record::read(&a.dir) {
        Ok(Some(_)) => {
            let touched = record::touch(&a.dir);
            Ok(Report::new(
                serde_json::json!({"path": a.dir.display().to_string(), "created": false, "touched": touched}),
                format!(
                    "{} already has an owner record{}.",
                    a.dir.display(),
                    if touched { "; last use moved to now" } else { "" }
                ),
            ))
        }
        Ok(None) | Err(_) => {
            let owner = a
                .owner
                .clone()
                .or_else(|| env("PADDOCK_AGENT_ID"))
                .unwrap_or_else(|| "unknown".to_string());
            let mut r = record::Record::new(kind, &owner);
            r.branch.clone_from(&a.branch);
            r.head.clone_from(&a.head);
            r.link.clone_from(&a.link);
            record::write(&a.dir, &r).map_err(CliError::failure)?;
            Ok(Report::new(
                serde_json::json!({"path": a.dir.display().to_string(), "created": true, "owner": owner}),
                format!("Recorded {} (owner {owner}).", a.dir.display()),
            ))
        }
    }
}
