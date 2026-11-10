//! `dev ci release-isolation setup|record|verify` (ADR 0085 WP-2 and the tool part of WP-1a; RT-2 of the Rust tooling plan).
//!
//! A tag build reads nothing kept: every folder it uses is made by the run, and a check proves it.
//!
//! - [`setup`]: makes the clone, build, `CARGO_HOME`, npm cache, `node_modules` and Tauri tool folders with unique names under
//!   the job's folder and records each one's identity in the ledger. It prints the `KEY=value` lines for `$GITHUB_ENV`.
//! - `record`: the only ledger writer (`ledger::append`): one line per kind, for the actions that choose a folder or a cache.
//! - [`verify`]: the ledger is complete, every folder is the one recorded, every `.crate` matches the tagged lock files,
//!   no cargo, git or npm configuration comes from outside the tag, and no compile cache ran. One `::error::` per finding.
//!
//! Freshness is by identity, never by modification time ([`identity`]): device and inode are the equality check, and the
//! change time is only a floor (a directory's ctime moves when entries are added; see `verify::identity_probe`, ADR 0085 §5 item 2). Windows, Linux and
//! macOS share one implementation; the only OS-specific code is [`identity::identity_of`]. [`fixture`] builds a synthetic
//! job with one plantable leak per probe; ADR 0025's build-folder check can reuse it.

pub mod config;
pub mod crates;
pub mod fixture;
pub mod identity;
pub mod ledger;
pub mod setup;
pub mod verify;

#[cfg(test)]
mod tests;

use clap::{Args, Subcommand};
use ledger::{Kind, Mode};
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;
use verify::{Env, VerifyOptions};

/// The commands.
#[derive(Subcommand)]
pub enum Command {
    /// Make this job's folders (clone, build, CARGO_HOME, npm cache, node_modules, Tauri tools) new and record them. Run it
    /// first in a release job, once.
    Setup(SetupArgs),
    /// Append the line of one action to the ledger (the only writer): its kind, and the folder it chose or `off`.
    Record(RecordArgs),
    /// Check the ledger, the identities, the crate archives and the configuration. One `::error::` per finding.
    Verify(VerifyArgs),
}

/// `setup`.
#[derive(Args)]
pub struct SetupArgs {
    /// The job's folder (default: $RUNNER_TEMP).
    #[arg(long)]
    temp: Option<PathBuf>,
    /// The ledger file (default: $PADDOCK_RELEASE_LEDGER, else <temp>/release-isolation/ledger.tsv).
    #[arg(long)]
    ledger: Option<PathBuf>,
    /// Append the `KEY=value` lines to this file too (the job's $GITHUB_ENV).
    #[arg(long)]
    env_file: Option<PathBuf>,
}

/// `record`.
#[derive(Args)]
pub struct RecordArgs {
    /// The kind: workspace, target, cargo-home, npm, node-modules, tauri-tools, dev-tool, sccache, angular or libduckdb.
    kind: String,
    /// job (made by this run inside the job folder), run (made by this run elsewhere) or off.
    mode: String,
    /// The folder (not for `off`).
    #[arg(long)]
    path: Option<PathBuf>,
    /// The ledger file (default as for `setup`).
    #[arg(long)]
    ledger: Option<PathBuf>,
}

/// `verify`.
#[derive(Args)]
pub struct VerifyArgs {
    /// The ledger file (default as for `setup`).
    #[arg(long)]
    ledger: Option<PathBuf>,
    /// The kinds the job must have recorded, comma separated (default: every kind but libduckdb).
    #[arg(long, value_delimiter = ',')]
    expect: Vec<String>,
    /// A Cargo workspace the job builds, relative to the clone (repeat; their Cargo.lock files form the union).
    #[arg(long = "cargo-workspace")]
    cargo_workspaces: Vec<String>,
    /// An Angular app folder relative to the clone (repeat).
    #[arg(long = "app-dir")]
    app_dirs: Vec<String>,
    /// The JSON of `sccache --show-stats --stats-format=json`, taken after the build.
    #[arg(long)]
    sccache_stats: Option<PathBuf>,
    /// The pinned SHA-256 values of the Tauri tool files (`<sha256>  <relative path>`).
    #[arg(long)]
    tauri_pins: Option<PathBuf>,
    /// Accept a missing --tauri-pins (a local trial only; refused when GITHUB_REF starts with refs/tags/).
    #[arg(long)]
    allow_unpinned_tauri: bool,
    /// Append the result to this file too (the job's $GITHUB_STEP_SUMMARY).
    #[arg(long)]
    summary_file: Option<PathBuf>,
}

fn ledger_path(given: Option<PathBuf>, env: &Env, temp: Option<&std::path::Path>) -> Result<PathBuf, CliError> {
    if let Some(p) = given {
        return Ok(p);
    }
    if let Some(p) = env.get("PADDOCK_RELEASE_LEDGER") {
        return Ok(PathBuf::from(p));
    }
    let base = temp.map(PathBuf::from).or_else(|| env.get("RUNNER_TEMP").map(PathBuf::from));
    base.map(|t| t.join("release-isolation").join("ledger.tsv")).ok_or_else(|| {
        CliError::usage("No ledger path: --ledger, PADDOCK_RELEASE_LEDGER and RUNNER_TEMP are all unset.")
            .with_hint("Pass --ledger <file> or run inside a CI job.")
    })
}

fn bad(e: String) -> CliError {
    CliError::failure(format!("::error::release-isolation {e}")).with_hint("Fix what the line says and run the step again.")
}

/// Runs a command.
pub fn command(cmd: Command) -> Result<Report, CliError> {
    let env = Env::from_process();
    match cmd {
        Command::Setup(a) => {
            let temp = a
                .temp
                .clone()
                .or_else(|| env.get("RUNNER_TEMP").map(PathBuf::from))
                .ok_or_else(|| {
                    CliError::usage("No job folder: pass --temp or set RUNNER_TEMP.").with_hint("Run it as a step of a CI job.")
                })?;
            let ledger = ledger_path(a.ledger, &env, Some(&temp))?;
            let made = setup::setup(&temp, &ledger).map_err(bad)?;
            let lines: Vec<String> = made.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
            if let Some(f) = a.env_file {
                let mut file = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&f)
                    .map_err(|e| CliError::failure(format!("::error::release-isolation {} cannot be opened: {e}", f.display())))?;
                file.write_all(format!("{}\n", lines.join("\n")).as_bytes())
                    .map_err(|e| CliError::failure(format!("::error::release-isolation {} cannot be written: {e}", f.display())))?;
            }
            let data = json!({
                "ledger": made.ledger,
                "folders": made.folders.iter().map(|(k, p)| (k.name().to_string(), json!(p))).collect::<serde_json::Map<_, _>>(),
                "env": made.env.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<serde_json::Map<_, _>>(),
            });
            Ok(Report::new(data, lines.join("\n")))
        }
        Command::Record(a) => {
            let kind = Kind::parse(&a.kind).ok_or_else(|| {
                CliError::usage(format!("`{}` is not a kind.", a.kind)).with_hint(
                    "Kinds: workspace, target, cargo-home, npm, node-modules, tauri-tools, dev-tool, sccache, angular, libduckdb.",
                )
            })?;
            let mode = Mode::parse(&a.mode).ok_or_else(|| {
                CliError::usage(format!("`{}` is not a mode.", a.mode))
                    .with_hint("Modes: job, run, off. Anything else is a shared or kept folder.")
            })?;
            let ledger = ledger_path(a.ledger, &env, None)?;
            let e = ledger::append(&ledger, kind, mode, a.path.as_deref()).map_err(bad)?;
            Ok(Report::new(
                json!({"kind": e.kind, "path": e.path, "mode": e.mode}),
                format!("recorded {} {} {}", e.kind, e.mode, e.path),
            ))
        }
        Command::Verify(a) => {
            let ledger = ledger_path(a.ledger, &env, None)?;
            let mut opts = VerifyOptions::new(ledger);
            if !a.expect.is_empty() {
                opts.expect = a
                    .expect
                    .iter()
                    .map(|k| Kind::parse(k).ok_or_else(|| CliError::usage(format!("`{k}` is not a kind.")).with_hint("Kinds: workspace, target, cargo-home, npm, node-modules, tauri-tools, dev-tool, sccache, angular, libduckdb.")))
                    .collect::<Result<_, _>>()?;
            }
            opts.cargo_workspaces = a.cargo_workspaces;
            opts.app_dirs = a.app_dirs;
            opts.sccache_stats = a.sccache_stats;
            opts.tauri_pins = a.tauri_pins;
            opts.allow_unpinned_tauri = a.allow_unpinned_tauri;
            let verdict = verify::verify(&opts, &env);
            let summary = if verdict.passed() {
                let mut s = verdict.summary();
                for n in &verdict.notices {
                    s.push_str(&format!("\nnotice: {n}"));
                }
                s
            } else {
                verdict.findings.iter().map(verify::Finding::line).collect::<Vec<_>>().join("\n")
            };
            if let Some(f) = a.summary_file
                && let Ok(mut file) = std::fs::OpenOptions::new().append(true).create(true).open(&f)
            {
                let _ = writeln!(file, "### Release isolation\n\n{}\n", summary.replace("::error::", ""));
            }
            if verdict.passed() {
                Ok(Report::new(
                    json!({"passed": true, "foldersConfirmed": verdict.folders_confirmed, "cratesMatched": verdict.crates_matched, "tauriFilesMatched": verdict.tauri_files_matched, "notices": verdict.notices}),
                    summary,
                ))
            } else {
                let broken = verdict.findings.iter().filter(|f| f.is_broken()).count();
                Err(CliError::refused(format!(
                    "{summary}\n::error::release-isolation {} finding(s), {broken} of them `check broken`.",
                    verdict.findings.len()
                ))
                .with_hint("The release does not pass: each line names its next step (ADR 0085 §5)."))
            }
        }
    }
}
