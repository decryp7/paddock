//! `ci landing-held`: the command that joins the CI verdict with the local slow rows ([`super::train_split`]). It lives with the push gate
//! (gates layer) because it needs both: the verdict reader of `ci_runs` and the slow record this gate writes.

use super::train_split::{Ci, Landing, SlowState, landing, read};
use crate::ci_runs::{SERVER, find_root, landing_verdict, verdict};
use clap::Args;
use paddock_cli::{CliError, Exit, Report};
use std::path::PathBuf;

/// The one answer to "may `sha` land": the CI half ([`landing_verdict`]) joined with the slow-row record of `root`'s state folder
/// ([`landing`]). `ci landing-held` and the pre-push guard of pushes to main ([`super::main_guard`]) both read it here (ER-22).
pub fn landing_of(server: &str, repo: &str, workflow: &str, sha: &str, root: &std::path::Path) -> Landing {
    let ci = match landing_verdict(server, repo, workflow, sha, root) {
        verdict::Verdict::Green { .. } | verdict::Verdict::GreenWithFixItems { .. } => Ci::Green,
        verdict::Verdict::Red { reason, .. } => Ci::Red(reason),
        verdict::Verdict::NotMeasured(why) => Ci::NotMeasured(why),
    };
    let slow: SlowState = read(&crate::git::state_dir(root), sha);
    landing(&ci, &slow)
}

/// Arguments of `ci landing-held`.
#[derive(Args)]
pub struct LandingHeldArgs {
    /// The full 40-hex commit that is to land.
    #[arg(long)]
    sha: String,
    /// The workflow whose complete green run is needed.
    #[arg(long, default_value = "ci.yml")]
    workflow: String,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// `ci landing-held`: ER-49's landing needs BOTH the complete green CI run ([`verdict::verdict_landing_at`]) and the green local slow rows
/// (`./dev check --pre-push --member-gate --set slow`, [`super::train_split`]) on the same commit. Exit 0 landable, 5 refused
/// (CI red or a slow row failed), 6 not held (anything unfinished, missing or unread: never a pass, ER-33).
pub fn landing_held(a: &LandingHeldArgs) -> Result<Report, CliError> {
    if a.sha.len() != 40 || !a.sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(CliError::usage(format!("--sha {:?} is not a full 40-hex commit id.", a.sha))
            .with_hint("Pass the whole sha (`git rev-parse HEAD`)."));
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = find_root(&cwd).unwrap_or(cwd);
    let landing = landing_of(&a.server, &a.repo, &a.workflow, &a.sha, &root);
    let (exit, text) = match landing {
        Landing::Landable => (
            Exit::Ok,
            format!("{} may land: the complete green CI run and the green slow rows hold on it", a.sha),
        ),
        Landing::Refused(why) => (Exit::Findings, format!("{} may not land: {why}", a.sha)),
        Landing::NotHeld(why) => (Exit::NotMeasured, format!("{} is not held for landing: {why}", a.sha)),
    };
    Ok(Report::new(serde_json::json!({ "sha": a.sha, "landable": exit == Exit::Ok }), text).with_exit(exit))
}
