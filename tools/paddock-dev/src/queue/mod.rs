//! `queue tick`: one step of the merge queue (docs/plans/merge-queue.md, slices 1 to 4 without rule 54, plan-status and the
//! tags). Reads `GITEA_TOKEN` (queue-bot's, with write on the repository; the queue never pushes tags).
//!
//! - [`ports`]: the four ports (ledger, train repository, local gates, CI);
//! - [`tick`]: the pure state machine over them;
//! - [`real`]: the adapters over Gitea, git, `ci dispatch`, `ci verdict` and `ci triage`.

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
    /// The queue's checkout (default: the folder containing .git, found from the current folder). It is reset to the train.
    #[arg(long)]
    root: Option<PathBuf>,
    /// The local hour for the backup window (default: the machine's).
    #[arg(long, value_parser = clap::value_parser!(u32).range(0..24))]
    hour: Option<u32>,
}

fn local_hour() -> u32 {
    paddock_proc::command("date")
        .arg("+%H")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(12)
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
            let mut train = real::GitTrain { root: root.clone() };
            let mut gates = real::LocalGates { root: root.clone() };
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
                gates: &mut gates,
                ci: &mut ci,
            };
            let env = tick::Env {
                hour: a.hour.unwrap_or_else(local_hour),
            };
            let t = tick::tick(&mut ports, &env).map_err(|e| {
                CliError::failure(format!("queue tick: {e}"))
                    .with_hint("Check the network, GITEA_TOKEN's scope and the checkout; the next tick starts from the labels.")
            })?;
            let exit = match &t {
                tick::Tick::Stuck { measured: false, .. } => Exit::NotMeasured,
                tick::Tick::Stuck { .. } => Exit::Findings,
                _ => Exit::Ok,
            };
            let text = t.text();
            if exit != Exit::Ok {
                eprintln!("{text}");
            }
            Ok(Report::new(serde_json::json!({"tick": format!("{t:?}"), "text": text}), text).with_exit(exit))
        }
    }
}
