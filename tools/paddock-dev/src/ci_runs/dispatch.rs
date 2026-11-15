// ci-runs-reader: ci dispatch: lists unfinished runs on a shared runner class before dispatching; no verdict
//! `ci dispatch <workflow> --ref <ref>`: start a workflow only when no run on the same single-runner class is going.
//!
//! The Mac (and Windows) runner is one machine: two runs on it at once make each other's timing-sensitive tests time out (CI runs
//! 7960 with 7961, and 7969-7971 with 7967, ER-36). So before it dispatches, the command lists the Gitea runs that are not
//! finished, reads each one's workflow file to learn the runner classes its jobs use, and waits (like `dev wait --ci-run`,
//! bounded by the required `--max`) while one of them shares a class with the workflow to start. Linux jobs run on container
//! runners that may overlap, so a Linux-only workflow dispatches at once.
//!
//! A class is read from the jobs' `runs-on` labels (a matrix expression stands for the job's matrix values), following job-level
//! `uses: ./.gitea/workflows/<file>` calls; other text of the file (steps, `runner.os` tests) does not count. The token is read
//! from `GITEA_TOKEN`, travels in a header only and is printed nowhere.

use super::client::{Client, RunInfo};
use crate::wait::{Clock, Http, MAX_LIMIT, MIN_INTERVAL, Outcome, Poll, Probe, SystemClock, fmt_duration, parse_duration, wait_for};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::time::Duration;
use yaml_rust2::{Yaml, YamlLoader};

/// A runner class with a single machine behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RunnerClass {
    /// The Mac runner.
    Macos,
    /// The Windows runner.
    Windows,
}

impl RunnerClass {
    fn name(self) -> &'static str {
        match self {
            RunnerClass::Macos => "macOS",
            RunnerClass::Windows => "Windows",
        }
    }
}

/// Reads a workflow file of the repository by its file name (`dyno.yml`); `None` when it is not there.
pub type ReadWorkflow<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The single-machine classes `workflow` can run on, including the workflows it calls. They come from each job's `runs-on`
/// labels (a `${{ matrix.x }}` label stands for every value of the job's matrix) and never from other text of the file: a step
/// that mentions macOS does not make a Linux job a Mac job. `Err` when the file cannot be read or parsed.
pub fn classes_of(workflow: &str, read: ReadWorkflow<'_>) -> Result<BTreeSet<RunnerClass>, String> {
    let mut classes = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut todo = vec![workflow.to_string()];
    while let Some(name) = todo.pop() {
        if !seen.insert(name.clone()) || seen.len() > 20 {
            continue;
        }
        let Some(text) = read(&name) else {
            if name == workflow {
                return Err(format!("workflow file .gitea/workflows/{workflow} is not in this checkout"));
            }
            continue;
        };
        let docs = match YamlLoader::load_from_str(&text) {
            Ok(d) => d,
            Err(e) if name == workflow => return Err(format!("workflow file {workflow} is not valid YAML: {e}")),
            Err(_) => continue,
        };
        for doc in &docs {
            let Some(jobs) = doc["jobs"].as_hash() else { continue };
            for (_, job) in jobs {
                if let Some(called) = job["uses"].as_str().and_then(called_workflow) {
                    todo.push(called);
                }
                let mut labels = Vec::new();
                collect_strings(&job["runs-on"], &mut labels);
                let mut from_matrix = Vec::new();
                collect_strings(&job["strategy"]["matrix"], &mut from_matrix);
                for label in labels {
                    if label.contains("${{") {
                        classify_all(&from_matrix, &mut classes);
                    } else {
                        classify_all(&[label], &mut classes);
                    }
                }
            }
        }
    }
    Ok(classes)
}

/// Every string scalar under `y`, whatever the nesting.
fn collect_strings(y: &Yaml, out: &mut Vec<String>) {
    match y {
        Yaml::String(s) => out.push(s.clone()),
        Yaml::Array(a) => a.iter().for_each(|v| collect_strings(v, out)),
        Yaml::Hash(h) => h.values().for_each(|v| collect_strings(v, out)),
        _ => {}
    }
}

fn classify_all(labels: &[String], classes: &mut BTreeSet<RunnerClass>) {
    for l in labels {
        let l = l.to_ascii_lowercase();
        if l.contains("macos") {
            classes.insert(RunnerClass::Macos);
        }
        if l.contains("windows") {
            classes.insert(RunnerClass::Windows);
        }
    }
}

/// The local workflow a job's `uses: ./.gitea/workflows/<file>` calls.
fn called_workflow(uses: &str) -> Option<String> {
    let rest = uses.trim().strip_prefix("./.gitea/workflows/")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        .collect();
    (!name.is_empty()).then_some(name)
}

/// Waits while a run that shares a class with the workflow to start is not finished.
struct BusyProbe<'a, 'b> {
    client: &'a mut Client<'b>,
    read: ReadWorkflow<'a>,
    wanted: BTreeSet<RunnerClass>,
}

impl BusyProbe<'_, '_> {
    /// The unfinished runs that share a class, as text, or `None` when there are none.
    fn busy(&mut self) -> Result<Option<String>, String> {
        let page = self.client.runs_page(1)?;
        Ok(page.runs.iter().filter(|r| !r.finished()).find_map(|run| self.clash(run)))
    }

    /// A workflow file that is not in this checkout cannot be classified and does not block.
    fn clash(&self, run: &RunInfo) -> Option<String> {
        match classes_of(run.workflow(), self.read) {
            Ok(theirs) => {
                let shared: Vec<&str> = theirs.intersection(&self.wanted).map(|c| c.name()).collect();
                (!shared.is_empty()).then(|| format!("run {} ({}, {}) is on {}", run.id, run.workflow(), run.status, shared.join(" and ")))
            }
            Err(_) => None,
        }
    }
}

impl Probe for BusyProbe<'_, '_> {
    fn describe(&self) -> String {
        let names: Vec<&str> = self.wanted.iter().map(|c| c.name()).collect();
        format!("the {} runner to be free of other runs", names.join(" and "))
    }
    fn poll(&mut self) -> Result<Poll, String> {
        Ok(match self.busy()? {
            None => Poll::Done("no other run is on the same runner".into()),
            Some(why) => Poll::Pending(why),
        })
    }
    fn next_step(&self) -> String {
        "wait for that run (`./dev wait --ci-run <id> --max <duration>`), or cancel it if it is stale, then dispatch again".into()
    }
}

/// What to dispatch.
#[derive(Debug)]
pub struct Request {
    /// The workflow file name, `dyno-release.yml`.
    pub workflow: String,
    /// The branch or tag to run it on.
    pub git_ref: String,
    /// `key=value` workflow inputs.
    pub inputs: Vec<(String, String)>,
    /// The longest to wait for a busy runner.
    pub max: Duration,
    /// How often to look.
    pub interval: Duration,
}

/// Parses `--max` and `--input k=v`.
pub fn parse_request(workflow: &str, git_ref: &str, inputs: &[String], max: &str, interval: Option<&str>) -> Result<Request, String> {
    let max = parse_duration(max)?;
    if max.is_zero() || max > MAX_LIMIT {
        return Err(format!("--max {} is out of range: use 1s to 24h.", fmt_duration(max)));
    }
    let interval = match interval {
        Some(i) => parse_duration(i)?,
        None => Duration::from_secs(15),
    }
    .max(MIN_INTERVAL);
    let inputs = inputs
        .iter()
        .map(|i| {
            i.split_once('=')
                .filter(|(k, _)| !k.is_empty())
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| format!("--input {i:?} is not key=value"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if workflow.contains('/') || workflow.is_empty() {
        return Err("the workflow is a file name in .gitea/workflows, like dyno-release.yml".into());
    }
    Ok(Request {
        workflow: workflow.to_string(),
        git_ref: git_ref.to_string(),
        inputs,
        max,
        interval,
    })
}

/// How a dispatch ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Dispatched {
    /// Dispatched after waiting `waited` (zero when the runner was free at once).
    Yes {
        /// How long the runner was busy.
        waited: Duration,
    },
    /// The runner stayed busy for `--max`; nothing was dispatched.
    StillBusy {
        /// The run that held it at the end.
        last: String,
    },
}

/// The Gitea repository and the token to ask it with.
#[derive(Clone, Copy)]
pub struct Target<'a> {
    /// `https://dev.decryptology.net`.
    pub server: &'a str,
    /// `decryp7/paddock`.
    pub repo: &'a str,
    /// The API token; sent in a header only.
    pub token: &'a str,
}

/// Waits for a free runner class, then dispatches. Errors are texts for `::error::`.
pub fn run(
    http: &mut dyn Http,
    target: &Target<'_>,
    req: &Request,
    read: ReadWorkflow<'_>,
    clock: &mut dyn Clock,
    log: &mut dyn Write,
) -> Result<Dispatched, String> {
    let Target { server, repo, token } = *target;
    let wanted = classes_of(&req.workflow, read)?;
    let mut waited = Duration::ZERO;
    if wanted.is_empty() {
        let _ = writeln!(
            log,
            "dispatch: {} uses no single-machine runner (Linux only); not waiting",
            req.workflow
        );
    } else {
        let mut client = Client::new(http, server, repo, token);
        client.backoff_ms = 0;
        let mut probe = BusyProbe {
            client: &mut client,
            read,
            wanted,
        };
        match wait_for(&mut probe, clock, req.max, req.interval, log) {
            Outcome::Done { elapsed, .. } => waited = elapsed,
            Outcome::TimedOut { last, max } => {
                let _ = writeln!(
                    log,
                    "::error::dispatch: not dispatching {}: still busy after {} (--max); {last}. Next: {}.",
                    req.workflow,
                    fmt_duration(max),
                    probe.next_step()
                );
                return Ok(Dispatched::StillBusy { last });
            }
            Outcome::Broken { error, .. } => return Err(format!("the run list could not be read: {error}")),
            Outcome::Failed { detail, .. } => return Err(detail),
        }
    }
    let url = format!(
        "{}/api/v1/repos/{repo}/actions/workflows/{}/dispatches",
        server.trim_end_matches('/'),
        req.workflow
    );
    let inputs: serde_json::Map<String, Value> = req.inputs.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    http.post_json(&url, token, &json!({"ref": req.git_ref, "inputs": inputs}))?;
    let _ = writeln!(
        log,
        "dispatch: dispatched {} on {} after waiting {}",
        req.workflow,
        req.git_ref,
        fmt_duration(waited)
    );
    Ok(Dispatched::Yes { waited })
}

/// The real clock, for the command.
pub fn system_clock() -> SystemClock {
    SystemClock::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    const SERVER: &str = "https://gitea.invalid";
    const REPO: &str = "decryp7/paddock";

    /// Run lists in order (the last one repeats), and the posts it got.
    struct Script {
        lists: VecDeque<Value>,
        posts: Vec<(String, Value)>,
        gets: usize,
    }

    impl Script {
        fn new(lists: Vec<Value>) -> Script {
            Script {
                lists: lists.into(),
                posts: Vec::new(),
                gets: 0,
            }
        }
    }

    impl Http for Script {
        fn get_json(&mut self, url: &str, token: &str) -> Result<Value, String> {
            assert!(!token.is_empty());
            assert!(url.contains("/actions/runs?"), "{url}");
            self.gets += 1;
            Ok(if self.lists.len() > 1 {
                self.lists.pop_front().unwrap()
            } else {
                self.lists[0].clone()
            })
        }
        fn post_json(&mut self, url: &str, _token: &str, body: &Value) -> Result<Value, String> {
            self.posts.push((url.to_string(), body.clone()));
            Ok(Value::Null)
        }
    }

    struct FakeClock(Duration);
    impl Clock for FakeClock {
        fn elapsed(&self) -> Duration {
            self.0
        }
        fn sleep(&mut self, d: Duration) {
            self.0 += d;
        }
    }

    fn runs(list: &[(u64, &str, &str)]) -> Value {
        json!({"total_count": list.len(), "workflow_runs": list.iter().map(|(id, wf, st)| json!({
            "id": id, "path": format!("{wf}@refs/heads/main"), "status": st, "event": "push"
        })).collect::<Vec<_>>()})
    }

    fn files(name: &str) -> Option<String> {
        match name {
            "mac.yml" => Some("jobs:\n  t:\n    runs-on: macos-latest\n".into()),
            "mac2.yml" => Some("jobs:\n  t:\n    runs-on: [self-hosted, macos]\n".into()),
            "linux.yml" => Some("jobs:\n  t:\n    runs-on: ubuntu-latest\n".into()),
            "matrix.yml" => Some(
                "jobs:\n  t:\n    strategy:\n      matrix:\n        os: [ubuntu-latest, windows-latest]\n    runs-on: ${{ matrix.os }}\n"
                    .into(),
            ),
            "caller.yml" => Some("jobs:\n  c:\n    uses: ./.gitea/workflows/mac.yml\n".into()),
            "comment.yml" => Some("# runs-on: macos-latest\njobs:\n  t:\n    runs-on: ubuntu-latest\n".into()),
            "step-text.yml" => Some(
                "jobs:\n  t:\n    runs-on: ubuntu-latest\n    steps:\n      - if: runner.os != 'Windows'\n        run: echo macOS\n".into(),
            ),
            "bench.yml" => Some(include_str!("../../../../.gitea/workflows/debrief-bench.yml").into()),
            "debrief.yml" => Some(include_str!("../../../../.gitea/workflows/debrief.yml").into()),
            _ => None,
        }
    }

    fn request(wf: &str, max: &str) -> Request {
        parse_request(wf, "main", &["gates=on".to_string()], max, Some("15s")).unwrap()
    }

    fn go(http: &mut Script, wf: &str, max: &str) -> (Result<Dispatched, String>, String, Duration) {
        let (mut clock, mut log) = (FakeClock(Duration::ZERO), Vec::new());
        let r = run(
            http,
            &Target {
                server: SERVER,
                repo: REPO,
                token: "secret-token",
            },
            &request(wf, max),
            &files,
            &mut clock,
            &mut log,
        );
        (r, String::from_utf8(log).unwrap(), clock.0)
    }

    #[test]
    fn busy_then_free_dispatches_after_the_wait() {
        let mut http = Script::new(vec![
            runs(&[(7960, "mac.yml", "in_progress")]),
            runs(&[(7960, "mac2.yml", "in_progress")]),
            runs(&[(7960, "mac.yml", "completed")]),
        ]);
        let (r, log, t) = go(&mut http, "mac.yml", "10m");
        assert_eq!(
            r.unwrap(),
            Dispatched::Yes {
                waited: Duration::from_secs(30)
            }
        );
        assert_eq!(t, Duration::from_secs(30));
        assert!(
            log.contains("run 7960 (mac.yml, in_progress) is on macOS") || log.contains("waiting for the macOS runner"),
            "{log}"
        );
        assert!(!log.contains("secret-token"));
        assert_eq!(http.posts.len(), 1);
        let (url, body) = &http.posts[0];
        assert!(url.ends_with("/actions/workflows/mac.yml/dispatches"), "{url}");
        assert_eq!(body, &json!({"ref": "main", "inputs": {"gates": "on"}}));
    }

    #[test]
    fn busy_past_max_gives_the_error_and_dispatches_nothing() {
        let mut http = Script::new(vec![runs(&[(7961, "mac2.yml", "in_progress")])]);
        let (r, log, t) = go(&mut http, "mac.yml", "1m");
        match r.unwrap() {
            Dispatched::StillBusy { last } => assert!(last.contains("run 7961"), "{last}"),
            other => panic!("{other:?}"),
        }
        assert!(http.posts.is_empty());
        assert_eq!(t, Duration::from_secs(60));
        assert!(
            log.contains("::error::dispatch: not dispatching mac.yml: still busy after 1m"),
            "{log}"
        );
        assert!(log.contains("run 7961") && log.contains("Next:"), "{log}");
        assert!(!log.contains("secret-token"));
    }

    #[test]
    fn another_runner_class_busy_dispatches_at_once() {
        let mut http = Script::new(vec![runs(&[(7967, "mac.yml", "in_progress"), (7968, "matrix.yml", "queued")])]);
        let (r, _log, t) = go(&mut http, "linux.yml", "1m");
        assert_eq!(r.unwrap(), Dispatched::Yes { waited: Duration::ZERO });
        assert_eq!(t, Duration::ZERO);
        assert_eq!(http.gets, 0, "a Linux-only workflow needs no run list");
        assert_eq!(http.posts.len(), 1);
    }

    #[test]
    fn a_finished_run_or_a_run_of_another_class_does_not_block() {
        let mut http = Script::new(vec![runs(&[(1, "mac.yml", "completed"), (2, "linux.yml", "in_progress")])]);
        let (r, _l, t) = go(&mut http, "mac.yml", "1m");
        assert_eq!(r.unwrap(), Dispatched::Yes { waited: Duration::ZERO });
        assert_eq!(t, Duration::ZERO);
    }

    #[test]
    fn classes_follow_matrices_calls_and_ignore_comments() {
        let c = |w| classes_of(w, &files).unwrap();
        assert_eq!(c("matrix.yml"), BTreeSet::from([RunnerClass::Windows]));
        assert_eq!(c("caller.yml"), BTreeSet::from([RunnerClass::Macos]));
        assert!(c("comment.yml").is_empty());
        assert!(classes_of("missing.yml", &files).unwrap_err().contains("not in this checkout"));
    }

    #[test]
    fn classes_come_from_runs_on_not_from_other_text() {
        let c = |w| classes_of(w, &files).unwrap();
        assert!(
            c("step-text.yml").is_empty(),
            "a step that mentions macOS or Windows is not a Mac job"
        );
        // debrief-bench.yml is all ubuntu-latest and its steps mention macOS and Windows.
        assert!(c("bench.yml").is_empty());
        // debrief.yml has a matrix with all three OS and a macos-latest job.
        assert_eq!(c("debrief.yml"), BTreeSet::from([RunnerClass::Macos, RunnerClass::Windows]));
    }

    #[test]
    fn a_bad_request_is_refused_with_the_fix() {
        assert!(
            parse_request("a.yml", "main", &["novalue".into()], "1m", None)
                .unwrap_err()
                .contains("key=value")
        );
        assert!(parse_request("a.yml", "main", &[], "0s", None).is_err());
        assert!(parse_request("../a.yml", "main", &[], "1m", None).is_err());
    }
}
