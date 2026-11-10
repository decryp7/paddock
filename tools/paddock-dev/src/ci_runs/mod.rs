//! `ci flakes` and `ci timings`: what CI runs say over time, read from the Gitea Actions API (read-only).
//!
//! - `ci flakes --app <app> [--days 30]`: the tests nextest and Playwright marked flaky, vitest's failed specs, and tests that
//!   failed and then passed on a rerun of the same commit ([`flakes`], ADR 0082 D6; the log readers are [`logfind`]). A run whose
//!   logs cannot be read is "not measured", never "no flakes" (ER-33).
//! - `ci timings`: the port of `scripts/ci-timings.mjs` ([`timings`]).
//!
//! The API client ([`client`]) reuses the planner's [`Http`](crate::wait::Http) and [`GiteaRuns`](crate::plan_since::GiteaRuns).
//! The token is read from `GITEA_TOKEN`, travels in a header only and is printed nowhere.

pub mod client;
pub mod flakes;
pub mod logfind;
#[cfg(test)]
mod testutil;
pub mod time;
pub mod timings;

use crate::wait::CurlHttp;
use clap::Args;
use client::Client;
use paddock_cli::{CliError, Exit, Report};
use std::path::PathBuf;

const SERVER: &str = "https://dev.decryptology.net";

/// Arguments of `ci flakes`.
#[derive(Args)]
pub struct FlakesArgs {
    /// The app (`dyno`, `debrief`, ...): its own workflows' jobs and the `app <app>` jobs of the shared ones.
    #[arg(long)]
    app: String,
    /// The window in days.
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u32).range(1..=366))]
    days: u32,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// Arguments of `ci timings`.
#[derive(Args)]
pub struct TimingsArgs {
    /// Repositories of the owner, comma separated.
    #[arg(long, default_value = timings::DEFAULT_REPOS)]
    repos: String,
    /// Finished runs per workflow.
    #[arg(long, default_value_t = 30)]
    limit: usize,
    /// Skip the job logs (no compile versus test phases).
    #[arg(long)]
    no_logs: bool,
    /// Where the log metrics are kept between runs (default: nowhere; every log is read again).
    #[arg(long)]
    cache: Option<PathBuf>,
    /// Replace the region between the `ci-timings` markers of this file instead of printing.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Also write the per-workflow summaries as JSON to this file (the script's `--json FILE`).
    #[arg(long = "json-file")]
    json_file: Option<PathBuf>,
    /// The API root of the owner's repositories.
    #[arg(long, default_value = "https://dev.decryptology.net/api/v1/repos/decryp7")]
    base: String,
}

fn token() -> Result<String, CliError> {
    token_from(std::env::var("GITEA_TOKEN").ok())
}

fn token_from(value: Option<String>) -> Result<String, CliError> {
    value.filter(|t| !t.is_empty()).ok_or_else(|| {
        CliError::usage("GITEA_TOKEN is not set.").with_hint("Run `source ~/.zshenv`, or export a read token for the Gitea API.")
    })
}

/// `ci flakes`.
pub fn flakes(a: &FlakesArgs) -> Result<Report, CliError> {
    let token = token()?;
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
    let report = flakes::collect(&mut client, &a.app, a.days, time::now_secs(), &a.repo, &mut |l| eprintln!("{l}")).map_err(|e| {
        CliError::failure(format!("the runs of {} could not be listed: {e}", a.repo))
            .with_hint("Check the network and that GITEA_TOKEN can read the repository.")
    })?;
    // An incomplete measurement ends with the findings code: a script must not read it as "no flakes".
    let exit = if report.complete() { Exit::Ok } else { Exit::Findings };
    Ok(Report::new(report.json(), report.text()).with_exit(exit))
}

/// `ci timings`.
pub fn timings(a: &TimingsArgs) -> Result<Report, CliError> {
    let token = token()?;
    let (server, owner) = a
        .base
        .split_once("/api/v1/repos/")
        .ok_or_else(|| CliError::usage("--base must look like https://host/api/v1/repos/<owner>."))?;
    let mut http = CurlHttp;
    let mut results = Vec::new();
    for repo in a.repos.split(',').filter(|r| !r.is_empty()) {
        eprintln!("{repo} ...");
        let mut client = Client::new(&mut http, server, &format!("{owner}/{repo}"), &token);
        let mut log = |l: &str| eprintln!("{l}");
        let mut src = timings::Source {
            client: &mut client,
            cache: a.cache.as_deref().filter(|_| !a.no_logs),
            log: &mut log,
        };
        let r = timings::analyse(&mut src, repo, a.limit, !a.no_logs)
            .map_err(|e| CliError::failure(format!("{repo}: {e}")).with_hint("Check the network and GITEA_TOKEN."))?;
        results.push(r);
    }
    let md = timings::render(&results, a.limit, &time::date_of(time::now_secs()));
    if let Some(file) = &a.json_file {
        std::fs::write(file, timings::summaries_json(&results).to_string())
            .map_err(|e| CliError::failure(format!("cannot write {}: {e}", file.display())))?;
    }
    if let Some(out) = &a.out {
        let existing = std::fs::read_to_string(out).unwrap_or_default();
        std::fs::write(out, timings::splice(&existing, &md))
            .map_err(|e| CliError::failure(format!("cannot write {}: {e}", out.display())))?;
        return Ok(Report::new(serde_json::json!({"out": out.display().to_string()}), String::new()));
    }
    Ok(Report::new(serde_json::json!({"markdown": md}), md))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = crate::run(&args, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn a_missing_token_is_a_usage_error_that_names_the_next_step() {
        let e = token_from(None).unwrap_err();
        assert!(
            e.text().contains("GITEA_TOKEN is not set") && e.text().contains("source ~/.zshenv"),
            "{}",
            e.text()
        );
        assert!(token_from(Some(String::new())).is_err());
        assert_eq!(token_from(Some("t".into())).unwrap(), "t");
    }

    #[test]
    fn flakes_needs_an_app_and_a_sane_window() {
        let (code, _, err) = run(&["ci", "flakes"]);
        assert_eq!(code, Exit::Usage.code(), "{err}");
        assert!(err.contains("required arguments"), "{err}");
        let (code, _, err) = run(&["ci", "flakes", "--app", "dyno", "--days", "0"]);
        assert_eq!(code, Exit::Usage.code(), "{err}");
        let (code, out, _) = run(&["ci", "flakes", "--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("--days") && out.contains("--repo"), "{out}");
    }

    #[test]
    fn timings_accepts_the_scripts_options() {
        let (code, out, _) = run(&["ci", "timings", "--help"]);
        assert_eq!(code, 0);
        for flag in ["--repos", "--limit", "--no-logs", "--cache", "--out", "--json-file", "--base"] {
            assert!(out.contains(flag), "{flag} missing in {out}");
        }
    }
}
