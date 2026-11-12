// ci-runs-reader: the commands over the client: flakes, timings, schedule-skip, verdict
//! `ci flakes` and `ci timings`: what CI runs say over time, read from the Gitea Actions API (read-only).
//!
//! - `ci flakes --app <app> [--days 30]`: the tests nextest and Playwright marked flaky, vitest's failed specs, and tests that
//!   failed and then passed on a rerun of the same commit ([`flakes`], ADR 0082 D6; the log readers are [`logfind`]). A run whose
//!   logs cannot be read is "not measured", never "no flakes" (ER-33).
//! - `ci timings`: the port of `scripts/ci-timings.mjs` ([`timings`]).
//! - `ci verdict --workflow <file> --sha <sha>`: is there a full green run of the workflow on the commit ([`verdict`]; exit 0, 5
//!   red or partial only, 6 not measured). The one reader of the runs for a pass/fail answer; the tag workflow asks it.
//! - `ci schedule-skip`: whether a scheduled run ends early because a train is in CI ([`schedule_skip`]; the job token in
//!   `PLAN_TOKEN` or `GITEA_TOKEN`, and it never fails the run).
//!
//! The API client ([`client`]) reuses the planner's [`Http`](crate::wait::Http) and [`GiteaRuns`](crate::plan_since::GiteaRuns).
//! The token is read from `GITEA_TOKEN`, travels in a header only and is printed nowhere.

pub mod client;
pub mod dispatch;
pub mod flakes;
pub mod logfind;
pub mod preconditions;
pub mod record;
pub mod schedule_skip;
#[cfg(test)]
mod testutil;
pub mod time;
pub mod timings;
pub mod triage;
pub mod verdict;

use crate::wait::CurlHttp;
use clap::Args;
use client::Client;
use paddock_cli::{CliError, Exit, Report};
use std::path::PathBuf;

/// Reads a file of the repository for the settings preconditions.
type Reader = Box<dyn Fn(&str) -> std::io::Result<String>>;

const SERVER: &str = "https://dev.decryptology.net";

/// Arguments of `ci flakes`. Cancelled runs are counted and listed in the report ("N cancelled run(s) not read") but never change
/// the exit code.
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
    /// End the window at this date (`YYYY-MM-DD`, end of day UTC) or at the newest run of this commit (7+ hex digits), for
    /// the tag gate of ADR 0082 D6 (default: now).
    #[arg(long, value_name = "DATE|SHA")]
    until: Option<String>,
    /// Exit 5 when the measured window lists any flaky or failed test (default: exit 0, so a listing is not a failure).
    #[arg(long)]
    fail_on_flakes: bool,
    /// The repository checkout whose `apps/<app>/app/playwright.config.ts` is read for the Playwright precondition (default:
    /// the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
}

/// Arguments of `ci dispatch`.
#[derive(Args)]
pub struct DispatchArgs {
    /// The workflow file in `.gitea/workflows` (`dyno-release.yml`).
    workflow: String,
    /// The branch or tag to run it on.
    #[arg(long = "ref", value_name = "REF")]
    git_ref: String,
    /// A workflow input, `key=value` (repeat for more).
    #[arg(long = "input", value_name = "KEY=VALUE")]
    inputs: Vec<String>,
    /// The longest to wait for a busy runner, for example 30m. Required: the wait always ends.
    #[arg(long, value_name = "DURATION")]
    max: String,
    /// How often to look (default 15s).
    #[arg(long, value_name = "DURATION")]
    interval: Option<String>,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
    /// The checkout whose `.gitea/workflows` is read (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
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

/// Arguments of `ci verdict`.
#[derive(Args)]
pub struct VerdictArgs {
    /// The workflow file whose runs are read (`ci.yml`, `debrief.yml`).
    #[arg(long)]
    workflow: String,
    /// The full 40-hex commit id.
    #[arg(long)]
    sha: String,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// Arguments of `ci triage`.
#[derive(Args)]
pub struct TriageArgs {
    /// The finished CI run to triage (an id from the run's URL).
    run: u64,
    /// Compare with this run instead of the last finished main run of the same workflow.
    #[arg(long)]
    against: Option<u64>,
    /// The app whose flake history is read (default: the workflow's own app and the `app <name>` jobs of the run).
    #[arg(long)]
    app: Option<String>,
    /// The window of the flake history in days.
    #[arg(long, default_value_t = 14, value_parser = clap::value_parser!(u32).range(1..=366))]
    days: u32,
    /// Skip the flake history (faster: it reads the failed job logs of the window).
    #[arg(long)]
    no_history: bool,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// Arguments of `ci schedule-skip`. The event, server and repository default to the job's (`GITHUB_EVENT_NAME`,
/// `GITHUB_SERVER_URL`, `GITHUB_REPOSITORY`).
#[derive(Args)]
pub struct ScheduleSkipArgs {
    /// The event of the run (default: GITHUB_EVENT_NAME); only `schedule` is ever skipped.
    #[arg(long)]
    event: Option<String>,
    /// The repository (default: GITHUB_REPOSITORY, then decryp7/paddock).
    #[arg(long)]
    repo: Option<String>,
    /// The Gitea server (default: GITHUB_SERVER_URL, then the family's).
    #[arg(long)]
    server: Option<String>,
    /// The workflow file being decided (`nightly.yml`): its own scheduled runs bound the skip.
    #[arg(long)]
    workflow: String,
    /// Never skip when the workflow's last measured scheduled run is older than this (36 h: one nightly at most; a weekly
    /// workflow passes a week and a day). Whatever the limit, a workflow is never skipped twice in a row.
    #[arg(long, default_value_t = 36.0)]
    max_gap_hours: f64,
}

/// `ci schedule-skip`: `skip=true|false` to `GITHUB_OUTPUT` (standard output without it), the annotation on standard output,
/// the reason in the job summary. Exit 0 whatever the API says (a scheduled test run fails open).
pub fn schedule_skip(a: &ScheduleSkipArgs) -> Result<Report, CliError> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let event = a.event.clone().or_else(|| env("GITHUB_EVENT_NAME")).unwrap_or_default();
    let server = a
        .server
        .clone()
        .or_else(|| env("GITHUB_SERVER_URL"))
        .unwrap_or_else(|| SERVER.into());
    let repo = a
        .repo
        .clone()
        .or_else(|| env("GITHUB_REPOSITORY"))
        .unwrap_or_else(|| "decryp7/paddock".into());
    let token = env("PLAN_TOKEN").or_else(|| env("GITEA_TOKEN")).unwrap_or_default();
    let now_ms = time::now_secs() as f64 * 1000.0;
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, &server, &repo, &token);
    let d = schedule_skip::decide_scheduled(&mut client, &event, &a.workflow, now_ms, a.max_gap_hours);
    let answer = format!("skip={}", d.skip());
    let mut text: Vec<String> = d.annotation().into_iter().collect();
    match env("GITHUB_OUTPUT") {
        Some(path) => append(&path, &format!("{answer}\n"))?,
        None => text.push(answer),
    }
    if let Some(path) = env("GITHUB_STEP_SUMMARY") {
        append(&path, &d.summary())?;
    }
    Ok(Report::new(
        serde_json::json!({ "skip": d.skip(), "summary": d.summary() }),
        text.join("\n"),
    ))
}

fn append(path: &str, text: &str) -> Result<(), CliError> {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .map_err(|e| CliError::failure(format!("{path} cannot be written: {e}.")))
}

/// Arguments of `ci record-fixture`.
#[derive(Args)]
pub struct RecordArgs {
    /// The finished runs to record (ids from the runs' URLs).
    #[arg(required = true)]
    runs: Vec<u64>,
    /// The folder the files are written to (create it under the test folder of the tool that reads them).
    #[arg(long)]
    out: PathBuf,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

fn token() -> Result<String, CliError> {
    token_from(std::env::var("GITEA_TOKEN").ok())
}

fn token_from(value: Option<String>) -> Result<String, CliError> {
    value.filter(|t| !t.is_empty()).ok_or_else(|| {
        CliError::usage("GITEA_TOKEN is not set.").with_hint("Run `source ~/.zshenv`, or export a read token for the Gitea API.")
    })
}

/// The folder containing `.git`, from `start` upwards.
fn find_root(start: &std::path::Path) -> Option<PathBuf> {
    start.ancestors().find(|d| d.join(".git").exists()).map(PathBuf::from)
}

/// A reader of the repository's files at a commit, through `git` (`args` to its output when it succeeded). `Err(gap)` when the
/// commit is not in this clone: its settings are then unknown, which is a gap and never "no config". Only a path that the
/// existing commit does not contain is `NotFound` (no such config); any other failure of `git show` is unknown.
fn commit_reader<G>(git: G, commit: &str) -> Result<Reader, String>
where
    G: Fn(&[&str]) -> Option<String> + 'static,
{
    if git(&["cat-file", "-e", &format!("{commit}^{{commit}}")]).is_none() {
        return Err(format!(
            "commit {commit} is not in this clone (fetch it with `git fetch origin`), so the Playwright and nextest settings \
             at it are unknown"
        ));
    }
    let commit = commit.to_string();
    Ok(Box::new(move |path: &str| {
        let spec = format!("{commit}:{path}");
        if git(&["cat-file", "-e", &spec]).is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{path} is not in commit {commit}"),
            ));
        }
        git(&["show", &spec]).ok_or_else(|| std::io::Error::other(format!("git show {spec} failed")))
    }))
}

/// The exit code of a report: an incomplete measurement is a failure (1, until ADR 0089 gives it its own code), flakes found
/// are findings (5) only when asked for with `--fail-on-flakes`, anything else is 0.
pub fn exit_of(report: &flakes::Report, fail_on_flakes: bool) -> Exit {
    if !report.complete() {
        Exit::Failure
    } else if fail_on_flakes && !report.rows.is_empty() {
        Exit::Findings
    } else {
        Exit::Ok
    }
}

/// `ci flakes`.
pub fn flakes(a: &FlakesArgs) -> Result<Report, CliError> {
    let token = token()?;
    let until = match a.until.as_deref() {
        None => flakes::Until::Now,
        Some(u) => flakes::Until::parse(u).map_err(CliError::usage)?,
    };
    let root = a.root.clone().or_else(|| std::env::current_dir().ok().and_then(|d| find_root(&d)));
    // The settings that decide whether flakes are visible are read at the end of the window: from the checkout, or, with
    // `--until <sha>`, from that commit.
    let mut unknown_commit: Option<String> = None;
    let (read, config_source): (Reader, String) = match (&root, &until) {
        (Some(r), flakes::Until::Commit(c)) => {
            let r2 = r.clone();
            match commit_reader(move |args: &[&str]| crate::plan_repo::git_text(&r2, args), c) {
                Ok(reader) => (reader, format!("commit {c}")),
                // The commit is not here: nothing about its settings is known, and the report says so once.
                Err(gap) => {
                    unknown_commit = Some(gap);
                    (
                        Box::new(|_: &str| Err(std::io::Error::other("unused"))),
                        format!("commit {c} (not in this clone)"),
                    )
                }
            }
        }
        (Some(r), _) => {
            let r2 = r.clone();
            (
                Box::new(move |p: &str| std::fs::read_to_string(r2.join(p))),
                format!("the working tree at {}", r.display()),
            )
        }
        (None, _) => (
            Box::new(|_: &str| Err(std::io::Error::other("no repository checkout was found (use --root)"))),
            "no checkout".into(),
        ),
    };
    let preconditions: Vec<String> = match unknown_commit {
        Some(gap) => vec![gap],
        None => preconditions::playwright(&a.app, read.as_ref())
            .into_iter()
            .chain(preconditions::nextest(&a.app, read.as_ref()))
            .collect(),
    };
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
    let req = flakes::Request {
        app: a.app.clone(),
        days: a.days,
        until,
        repo: a.repo.clone(),
        now_secs: time::now_secs(),
        preconditions,
        config_source,
    };
    let report = flakes::collect(&mut client, &req, &mut |l| eprintln!("{l}")).map_err(|e| {
        CliError::failure(format!("the runs of {} could not be listed: {e}", a.repo))
            .with_hint("Check the network, that GITEA_TOKEN can read the repository, and --until.")
    })?;
    let exit = exit_of(&report, a.fail_on_flakes);
    if !report.complete() {
        // A script must not read an incomplete measurement as "no flakes" (ER-33); the annotation goes to the job log.
        eprintln!("::error::{}", report.error_line());
    }
    Ok(Report::new(report.json(), report.text()).with_exit(exit))
}

/// `ci verdict`: is there a full green run of the workflow on the commit (exit 0), a red or only partial one (5), or is it not
/// measured (6; also without a token)? The token is read from `GITEA_TOKEN`.
pub fn verdict(a: &VerdictArgs) -> Result<Report, CliError> {
    if a.sha.len() != 40 || !a.sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(CliError::usage(format!("--sha {:?} is not a full 40-hex commit id.", a.sha))
            .with_hint("Pass the whole sha (`git rev-parse HEAD`)."));
    }
    let v = match token() {
        Err(_) => verdict::Verdict::NotMeasured("GITEA_TOKEN is not set, so the runs cannot be read".into()),
        Ok(token) => {
            let mut http = CurlHttp;
            let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
            verdict::verdict(&mut client, &a.workflow, &a.sha)
        }
    };
    let r = verdict::report(&v, &a.workflow, &a.sha);
    if r.exit != Exit::Ok {
        eprintln!("::error::ci verdict: {}", r.text);
    }
    Ok(r)
}

/// `ci dispatch`: waits while a run shares the workflow's single-machine runner class, then dispatches.
pub fn dispatch(a: &DispatchArgs) -> Result<Report, CliError> {
    let hint = "Run `./dev ci dispatch --help`.";
    let token =
        token_from(std::env::var("GITEA_TOKEN").ok()).map_err(|e| e.with_hint("The dispatch needs a token that can write Actions."))?;
    let req = dispatch::parse_request(&a.workflow, &a.git_ref, &a.inputs, &a.max, a.interval.as_deref())
        .map_err(|e| CliError::usage(e).with_hint(hint))?;
    let root = a
        .root
        .clone()
        .or_else(|| std::env::current_dir().ok().and_then(|d| find_root(&d)))
        .ok_or_else(|| CliError::usage("not inside a repository checkout.").with_hint("Pass --root <checkout>."))?;
    let read = |name: &str| std::fs::read_to_string(root.join(".gitea/workflows").join(name)).ok();
    let mut http = CurlHttp;
    let mut clock = dispatch::system_clock();
    match dispatch::run(
        &mut http,
        &dispatch::Target {
            server: &a.server,
            repo: &a.repo,
            token: &token,
        },
        &req,
        &read,
        &mut clock,
        &mut std::io::stderr(),
    ) {
        Ok(dispatch::Dispatched::Yes { waited }) => Ok(Report::new(
            serde_json::json!({"dispatched": true, "waitedMs": waited.as_millis() as u64}),
            format!(
                "Dispatched {} on {} after waiting {}.",
                req.workflow,
                req.git_ref,
                crate::wait::fmt_duration(waited)
            ),
        )),
        Ok(dispatch::Dispatched::StillBusy { last }) => Err(CliError::failure(format!("not dispatched: the runner stayed busy ({last})."))
            .with_hint("Wait for that run with `./dev wait --ci-run <id> --max <duration>`, then dispatch again.")),
        Err(e) => {
            eprintln!("::error::dispatch: {e}");
            Err(CliError::failure(e).with_hint("Check the workflow name, GITEA_TOKEN's scope and the network."))
        }
    }
}

/// `ci triage`: read-only; it quarantines nothing and marks nothing green (ER-12).
pub fn triage(a: &TriageArgs) -> Result<Report, CliError> {
    let token = token()?;
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
    let req = triage::Request {
        run: a.run,
        against: a.against,
        app: a.app.clone(),
        days: a.days,
        history: !a.no_history,
        repo: a.repo.clone(),
        now_secs: time::now_secs(),
    };
    let report = triage::collect(&mut client, &req, &mut |l| eprintln!("{l}")).map_err(|e| {
        CliError::failure(format!("run {} could not be triaged: {e}", a.run))
            .with_hint("Check the network, that GITEA_TOKEN can read the repository, and that the run has finished.")
    })?;
    // An answer with an UNKNOWN row or a gap is not a clean one: exit 5, so a script cannot read it as "nothing to worry about".
    Ok(Report::new(report.json(), report.text()).with_exit(report.exit()))
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
        // Only a missing file starts empty; any other read error would overwrite what the file holds.
        let existing = match std::fs::read_to_string(out) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(CliError::failure(format!("cannot read {}: {e}", out.display()))),
        };
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
        for flag in ["--days", "--repo", "--until", "--fail-on-flakes", "--root"] {
            assert!(out.contains(flag), "{flag} missing in {out}");
        }
        let (code, _, _) = run(&["ci", "flakes", "--app", "dyno", "--until", "tomorrow"]);
        assert_eq!(code, Exit::Usage.code(), "a bad --until is refused before any request");
    }

    #[test]
    fn the_repository_root_is_the_folder_with_git() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("a/b")).unwrap();
        assert_eq!(find_root(&d.path().join("a/b")), None);
        std::fs::create_dir(d.path().join(".git")).unwrap();
        assert_eq!(find_root(&d.path().join("a/b")).as_deref(), Some(d.path()));
    }

    /// A fake git: `commits` exist, `files` are `commit:path` to text, `broken` are `commit:path` whose `show` fails.
    fn fake_git(
        commits: &'static [&'static str],
        files: &'static [(&'static str, &'static str)],
        broken: &'static [&'static str],
    ) -> impl Fn(&[&str]) -> Option<String> {
        move |args: &[&str]| match args {
            ["cat-file", "-e", spec] => {
                let ok = match spec.strip_suffix("^{commit}") {
                    Some(c) => commits.contains(&c),
                    None => files.iter().any(|(k, _)| k == spec) || broken.contains(spec),
                };
                ok.then(String::new)
            }
            ["show", spec] if !broken.contains(spec) => files.iter().find(|(k, _)| k == spec).map(|(_, v)| v.to_string()),
            _ => None,
        }
    }

    #[test]
    fn a_commit_missing_from_the_clone_is_a_gap_not_no_config() {
        let e = commit_reader(fake_git(&[], &[], &[]), "abc1234").err().unwrap();
        assert!(e.contains("abc1234 is not in this clone") && e.contains("git fetch"), "{e}");
    }

    #[test]
    fn only_a_path_missing_from_an_existing_commit_is_no_config() {
        let files = &[("abc1234:apps/dyno/app/playwright.config.ts", "retries: 0,")];
        let broken = &["abc1234:apps/dyno/.config/nextest.toml"];
        let read = commit_reader(fake_git(&["abc1234"], files, broken), "abc1234").ok().unwrap();
        assert_eq!(read("apps/dyno/app/playwright.config.ts").unwrap(), "retries: 0,");
        // The commit exists and does not have the file: not found, so no precondition.
        let e = read("apps/dyno/.config/other.toml").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
        // The file is there but `git show` fails: unknown, never "no config".
        let e = read("apps/dyno/.config/nextest.toml").unwrap_err();
        assert_ne!(e.kind(), std::io::ErrorKind::NotFound);
        // Through the preconditions, unknown becomes a gap and not-found does not.
        assert!(preconditions::nextest("dyno", read.as_ref()).unwrap().contains("could not be read"));
        assert!(preconditions::playwright("dyno", read.as_ref()).is_none());
    }

    #[test]
    fn verdict_needs_a_workflow_and_a_whole_sha_and_never_passes_without_a_token() {
        let (code, _, err) = run(&["ci", "verdict"]);
        assert_eq!(code, Exit::Usage.code(), "{err}");
        let (code, _, err) = run(&["ci", "verdict", "--workflow", "ci.yml", "--sha", "abc1234"]);
        assert_eq!(code, Exit::Usage.code(), "{err}");
        assert!(err.contains("40-hex"), "{err}");
        let (code, out, _) = run(&["ci", "verdict", "--help"]);
        assert_eq!(code, 0);
        for flag in ["--workflow", "--sha", "--repo", "--server"] {
            assert!(out.contains(flag), "{flag} missing in {out}");
        }
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
