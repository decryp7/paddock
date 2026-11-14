//! `queue tick|hold|unhold`: one step of the merge queue (docs/plans/merge-queue.md, slices 1 to 4 without rule 54, plan-status and the
//! tags). Reads `GITEA_TOKEN` (queue-bot's, with write on the repository; the queue never pushes tags).
//!
//! - [`ports`]: the three ports (ledger, train repository, CI); the tick runs no code of any pull request, so there are no local
//!   gates: they are the first jobs of the dispatched workflow;
//! - [`tick`]: the pure state machine over them;
//! - [`real`]: the adapters over Gitea, git, `ci dispatch`, `ci verdict` and `ci triage`.

pub mod blame;
pub mod ports;
pub mod real;
pub mod tick;

#[cfg(test)]
mod tests;

use crate::pr::{known, root_of};
use crate::wait::CurlHttp;
use clap::{Args, Subcommand};
use paddock_cli::{CliError, Exit, Report};
use std::path::PathBuf;

const SERVER: &str = "https://dev.decryptology.net";

/// `queue`'s commands.
#[derive(Subcommand)]
pub enum QueueCommand {
    /// One step of the merge queue: take approved pull requests, build the train, run the local gates, dispatch ci.yml, judge
    /// its result (drop the pull requests that broke it, retry the rest) and fast-forward main when a full run is green. State
    /// lives in the pull requests' labels and the train branch; a tick can end anywhere. Exit 5 when the queue is stuck, 6 when
    /// something it needs was not measured.
    Tick(TickArgs),
    /// Stop the queue: put `queue/hold` on a pull request. Every tick then does nothing after reading the ledger (no build, no
    /// dispatch, no landing) until `queue unhold`.
    Hold {
        /// The pull request that carries the label.
        number: u64,
        /// Server, repository and checkout.
        #[command(flatten)]
        common: Common,
    },
    /// Remove `queue/hold` from every pull request that has it.
    Unhold {
        /// Server, repository and checkout.
        #[command(flatten)]
        common: Common,
    },
}

/// Where the ledger lives.
#[derive(Args, Clone)]
pub struct Common {
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The checkout holding .gitea/labels.toml (default: found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
}

/// Puts `queue/hold` on pull request `number`.
pub fn hold(ledger: &mut dyn ports::Ledger, number: u64) -> Result<(), String> {
    ledger.relabel(number, &["+queue/hold"])
}

/// Removes `queue/hold` and `queue/blocked` from every open pull request that has them; returns their numbers. A blocked train
/// is judged again on the next tick (its failed run is triaged afresh), never rerun blindly.
pub fn unhold(ledger: &mut dyn ports::Ledger) -> Result<Vec<u64>, String> {
    let held: Vec<u64> = ledger
        .open_pulls()?
        .iter()
        .filter(|p| p.labels.iter().any(|l| l == ports::label::HOLD || l == ports::label::BLOCKED))
        .map(|p| p.number)
        .collect();
    for n in &held {
        ledger.relabel(*n, &["-queue/hold", "-queue/blocked"])?;
    }
    Ok(held)
}

/// Arguments of `queue tick`.
#[derive(Args)]
pub struct TickArgs {
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// A checkout of main holding `.gitea/labels.toml` and the workflow files (default: found from the current folder). It is only
    /// read: the queue merges in its own clone under --scratch.
    #[arg(long)]
    root: Option<PathBuf>,
    /// The folder of the queue's own clone, its planner checkout and its askpass program (kept between ticks, never the job's
    /// checkout).
    #[arg(long)]
    scratch: PathBuf,
    /// The repository's clone URL (default: <server>/<repo>.git).
    #[arg(long)]
    clone_url: Option<String>,
    /// Also give the token to clone and fetch (a private repository); by default only the push gets it.
    #[arg(long)]
    authenticated_fetch: bool,
    /// The Asia/Singapore hour for the backup window (default: the system clock plus 8 hours; no `date`, no time zone database).
    #[arg(long, value_parser = clap::value_parser!(u32).range(0..24))]
    hour: Option<u32>,
}

/// The hour (0-23) in Asia/Singapore at `now`: UTC plus 8 hours, the zone has no daylight saving. `None` for a clock before 1970.
pub fn singapore_hour(now: std::time::SystemTime) -> Option<u32> {
    let secs = now.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    u32::try_from(((secs + 8 * 3600) / 3600) % 24).ok()
}

/// The exit code of a tick: a held queue did nothing on purpose (not a success for a script that expects a step), a stuck one
/// needs a person, and one that could not measure something never reads as a pass.
pub fn exit_of(t: &tick::Tick) -> Exit {
    match t {
        tick::Tick::Stuck { measured: false, .. } => Exit::NotMeasured,
        tick::Tick::Stuck { .. } => Exit::Findings,
        tick::Tick::Held(_) => Exit::Refused,
        _ => Exit::Ok,
    }
}

/// Runs one `queue` command.
pub fn command(c: QueueCommand) -> Result<Report, CliError> {
    match c {
        QueueCommand::Tick(a) => {
            let root = root_of(&a.root)?;
            let known = known(&root)?;
            let token = std::env::var("GITEA_TOKEN").ok().filter(|t| !t.is_empty()).ok_or_else(|| {
                CliError::usage("GITEA_TOKEN is not set.").with_hint("Set queue-bot's token (secret QUEUE_BOT_TOKEN in the workflow).")
            })?;
            let mut http_pr = CurlHttp;
            let mut http_ci = CurlHttp;
            let mut ledger = real::GiteaLedger {
                api: crate::pr::gitea::Api::new(&mut http_pr, &a.server, &a.repo, &token),
                known,
            };
            let url = a
                .clone_url
                .clone()
                .unwrap_or_else(|| format!("{}/{}.git", a.server.trim_end_matches('/'), a.repo));
            let mut train = real::GitTrain::open(&url, &a.scratch, Some(token.clone()), a.authenticated_fetch, false).map_err(|e| {
                CliError::failure(format!("queue tick: {e}"))
                    .with_hint("The queue clones the repository itself; check --scratch and the URL.")
            })?;
            let mut ci = real::GiteaCi {
                server: a.server.clone(),
                repo: a.repo.clone(),
                token: token.clone(),
                root: root.clone(),
                max_wait: std::time::Duration::from_secs(240),
                http: &mut http_ci,
            };
            let mut ports = tick::Ports {
                ledger: &mut ledger,
                train: &mut train,
                ci: &mut ci,
            };
            let env = tick::Env {
                hour: a.hour.or_else(|| singapore_hour(std::time::SystemTime::now())),
            };
            let t = tick::tick(&mut ports, &env).map_err(|e| {
                CliError::failure(format!("queue tick: {e}"))
                    .with_hint("Check the network, GITEA_TOKEN's scope and the checkout; the next tick starts from the labels.")
            })?;
            let exit = exit_of(&t);
            let text = t.text();
            if exit != Exit::Ok {
                eprintln!("{text}");
            }
            Ok(Report::new(serde_json::json!({"tick": format!("{t:?}"), "text": text}), text).with_exit(exit))
        }
        QueueCommand::Hold { number, common } => with_ledger(&common, |l| {
            hold(l, number).map(|()| format!("#{number}: queue/hold set; the queue is stopped"))
        }),
        QueueCommand::Unhold { common } => with_ledger(&common, |l| {
            let held = unhold(l)?;
            Ok(if held.is_empty() {
                "no pull request had queue/hold".to_string()
            } else {
                format!(
                    "queue/hold removed from {}",
                    held.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ")
                )
            })
        }),
    }
}

fn with_ledger(c: &Common, f: impl FnOnce(&mut dyn ports::Ledger) -> Result<String, String>) -> Result<Report, CliError> {
    let root = root_of(&c.root)?;
    let known = known(&root)?;
    let token = std::env::var("GITEA_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| CliError::usage("GITEA_TOKEN is not set."))?;
    let mut http = CurlHttp;
    let mut ledger = real::GiteaLedger {
        api: crate::pr::gitea::Api::new(&mut http, &c.server, &c.repo, &token),
        known,
    };
    let text = f(&mut ledger).map_err(|e| CliError::failure(format!("queue: {e}")))?;
    Ok(Report::new(serde_json::json!({ "text": text }), text))
}
