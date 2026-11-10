//! `ci timings`: real CI timings of the Paddock family from the Gitea Actions API (read-only), the port of
//! `scripts/ci-timings.mjs` with the same Markdown output (its oracle: the script's output on the same recorded replies).
//!
//! Per repo and workflow it takes the last N completed runs and reports, per job: wall time, dispatch delay (time from the
//! previous job finishing, or the run being created, to the job starting), runner, per-step durations (API step timestamps)
//! and, from the job logs, compile versus test-run phases. The critical path of a run is rebuilt from timestamps (the API does
//! not expose `needs`): the last job to finish, then repeatedly the job that finished last before it started.

use super::client::{Client, RunInfo, TimedJob};
use super::time::ts;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

/// The repositories measured by default: the apps live in the monorepo now (the script still defaulted to the old per-app ones).
pub const DEFAULT_REPOS: &str = "paddock";
const BEGIN: &str = "<!-- ci-timings:begin -->";
const END: &str = "<!-- ci-timings:end -->";

fn re(pattern: &str) -> Regex {
    // The patterns are literals of this file, checked by the tests below.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

// ---- insertion-ordered map (a JS object's order) ----

/// A map that iterates in insertion order, like the objects the script keys by name.
#[derive(Debug, Clone)]
pub struct Ordered<T> {
    keys: Vec<String>,
    map: HashMap<String, T>,
}

impl<T> Default for Ordered<T> {
    fn default() -> Self {
        Ordered {
            keys: Vec::new(),
            map: HashMap::new(),
        }
    }
}

impl<T> Ordered<T> {
    fn entry(&mut self, key: &str) -> &mut T
    where
        T: Default,
    {
        if !self.map.contains_key(key) {
            self.keys.push(key.to_string());
        }
        self.map.entry(key.to_string()).or_default()
    }

    fn get(&self, key: &str) -> Option<&T> {
        self.map.get(key)
    }

    fn iter(&self) -> impl Iterator<Item = (&String, &T)> {
        self.keys.iter().map(|k| (k, &self.map[k]))
    }

    fn len(&self) -> usize {
        self.keys.len()
    }
}

// ---- statistics ----

/// `Math.round`: halves go up.
fn round(x: f64) -> f64 {
    (x + 0.5).floor()
}

fn pct(a: &[f64], p: f64) -> Option<f64> {
    if a.is_empty() {
        return None;
    }
    let mut s = a.to_vec();
    s.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let k = (s.len() - 1) as f64 * p;
    let (lo, hi) = (k.floor(), k.ceil());
    Some(s[lo as usize] + (s[hi as usize] - s[lo as usize]) * (k - lo))
}

fn median(a: &[f64]) -> Option<f64> {
    pct(a, 0.5)
}

fn p90(a: &[f64]) -> Option<f64> {
    pct(a, 0.9)
}

/// Seconds as `42s` or `3m05s`, `-` when there is no value.
fn fmt(s: Option<f64>) -> String {
    let Some(s) = s else {
        return "-".into();
    };
    if s < 90.0 {
        return format!("{}s", round(s));
    }
    let t = round(s) as i64;
    format!("{}m{:02}s", t.div_euclid(60), t.rem_euclid(60))
}

// ---- step categories (first match wins) ----

static CATEGORIES: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    [
        ("checkout", "(?i)checkout"),
        ("apt / system libs", "(?i)apt|linux dependencies|system dep"),
        ("cache save", "(?i)cache/save|cache save|save.*cache"),
        ("cache restore", "(?i)cache"),
        (
            "toolchain / tools",
            "(?i)toolchain|install-action|rustup|tauri-driver|webdriver|setup-node|libduckdb|nextest|cargo-deny|sccache",
        ),
        ("playwright install", "(?i)playwright install"),
        ("npm ci", "(?i)^install$|npm ci|npm install|node_modules"),
        ("playwright run (e2e)", "(?i)end-to-end|e2e|playwright"),
        ("vitest / unit tests", "(?i)unit test|vitest|ng test"),
        ("ng build", "(?i)frontend build|ng build|translations"),
        ("lint / format / audit", "(?i)lint|clippy|format|audit|licen|deny"),
        (
            "rust tests (compile + run)",
            "(?i)^tests$|doc tests|memory budget|cargo test|nextest|rust test",
        ),
        (
            "bundle / app build",
            "(?i)bundle|tauri build|build app|app binary|dist|sidecar|installer|dmg|msi|smoke",
        ),
    ]
    .into_iter()
    .map(|(n, p)| (n, re(p)))
    .collect()
});

fn categorize(name: &str) -> &'static str {
    CATEGORIES.iter().find(|(_, r)| r.is_match(name)).map_or("other", |(n, _)| n)
}

// ---- log metrics (compile versus test-run phases) ----

/// Metric name to the values seen, in the order first seen.
pub type Metrics = Ordered<Vec<f64>>;

static ANSI: LazyLock<Regex> = LazyLock::new(|| re(r"\x1b\[[0-9;]*m"));
static FINISHED: LazyLock<Regex> = LazyLock::new(|| re(r"Finished `?([\w-]+)`? profile .* in (?:(\d+)m )?(?:(\d+)h )?([\d.]+)s"));
static NEXTEST: LazyLock<Regex> = LazyLock::new(|| re(r"Summary \[\s*([\d.]+)s\]\s+(\d+) tests? run"));
static VITEST: LazyLock<Regex> = LazyLock::new(|| re(r"\bDuration\s+([\d.]+)(ms|s)\b"));
static PLAYWRIGHT: LazyLock<Regex> = LazyLock::new(|| re(r"\b(\d+) passed \(([\d.]+)(ms|s|m|h)\)"));
static NG_BUNDLE: LazyLock<Regex> = LazyLock::new(|| re(r"Application bundle generation complete\. \[([\d.]+) seconds\]"));
static COMPILING: LazyLock<Regex> = LazyLock::new(|| re(r"^\S+\s+\s*Compiling "));

fn dur(n: &str, unit: &str) -> Option<f64> {
    let k = match unit {
        "ms" => 0.001,
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        _ => return None,
    };
    n.parse::<f64>().ok().map(|n| n * k)
}

/// The compile and test-run phases a job log reports.
pub fn parse_log(text: &str) -> Metrics {
    let mut m = Metrics::default();
    let num = |s: &str| s.parse::<f64>().ok();
    for raw in text.split('\n') {
        let line = ANSI.replace_all(raw, "");
        if let Some(r) = FINISHED.captures(&line) {
            let mins = r.get(2).and_then(|x| num(x.as_str())).unwrap_or(0.0);
            let hours = r.get(3).and_then(|x| num(x.as_str())).unwrap_or(0.0);
            if let Some(s) = num(&r[4]) {
                m.entry(&format!("cargo build ({})", &r[1])).push(mins * 60.0 + hours * 3600.0 + s);
            }
        } else if let Some(r) = NEXTEST.captures(&line) {
            if let Some(s) = num(&r[1]) {
                m.entry("nextest run").push(s);
            }
        } else if let Some(r) = VITEST.captures(&line) {
            if let Some(s) = dur(&r[1], &r[2]) {
                m.entry("vitest").push(s);
            }
        } else if let Some(r) = PLAYWRIGHT.captures(&line) {
            if let Some(s) = dur(&r[2], &r[3]) {
                m.entry("playwright run").push(s);
            }
        } else if let Some(r) = NG_BUNDLE.captures(&line) {
            if let Some(s) = num(&r[1]) {
                m.entry("ng build bundle").push(s);
            }
        } else if COMPILING.is_match(&line) {
            let v = m.entry("crates compiled");
            let n = v.first().copied().unwrap_or(0.0) + 1.0;
            *v = vec![n];
        }
    }
    m
}

fn metrics_json(m: &Metrics) -> Value {
    let mut o = Map::new();
    for (k, v) in m.iter() {
        o.insert(k.clone(), json!(v));
    }
    Value::Object(o)
}

fn metrics_from_json(v: &Value) -> Option<Metrics> {
    let mut m = Metrics::default();
    for (k, arr) in v.as_object()? {
        *m.entry(k) = arr.as_array()?.iter().filter_map(Value::as_f64).collect();
    }
    Some(m)
}

// ---- collection ----

/// A job with the figures derived across its run.
#[derive(Debug, Clone)]
pub struct Job {
    id: u64,
    name: String,
    runner: Option<String>,
    conclusion: String,
    start: f64,
    end: f64,
    steps: Vec<(String, Option<f64>)>,
    wall: f64,
    pred: Option<u64>,
    delay: f64,
    first_wave: bool,
    metrics: Option<Metrics>,
}

/// A run with its jobs.
#[derive(Debug, Clone)]
pub struct Loaded {
    run: RunInfo,
    jobs: Vec<Job>,
    created: f64,
    wall: f64,
}

fn load_jobs(run: &RunInfo, raw: Vec<TimedJob>) -> Loaded {
    let mut norm: Vec<Job> = raw
        .into_iter()
        .filter_map(|j| {
            let (start, end) = (j.start?, j.end?);
            Some(Job {
                id: j.id,
                name: j.name,
                runner: j.runner,
                conclusion: j.conclusion,
                start,
                end,
                steps: j.steps,
                wall: (end - start) / 1000.0,
                pred: None,
                delay: 0.0,
                first_wave: false,
                metrics: None,
            })
        })
        .collect();
    let created = ts(&run.created_at).unwrap_or_else(|| norm.iter().map(|j| j.start).fold(f64::INFINITY, f64::min));
    let ends: Vec<(u64, f64)> = norm.iter().map(|k| (k.id, k.end)).collect();
    for (i, j) in norm.iter_mut().enumerate() {
        // The job that ended last before this one started (ties: the first listed).
        let mut prev: Option<(u64, f64)> = None;
        for (k, (id, end)) in ends.iter().enumerate() {
            if k != i && *end <= j.start + 2000.0 && prev.is_none_or(|(_, e)| *end > e) {
                prev = Some((*id, *end));
            }
        }
        j.pred = prev.map(|p| p.0);
        j.delay = (j.start - prev.map_or(created, |p| p.1)) / 1000.0;
        j.first_wave = prev.is_none();
    }
    let run_end = norm.iter().map(|j| j.end).fold(created, f64::max);
    Loaded {
        run: run.clone(),
        jobs: norm,
        created,
        wall: (run_end - created) / 1000.0,
    }
}

fn critical_path(jobs: &[Job]) -> Vec<&Job> {
    let Some(mut cur) = jobs.iter().reduce(|a, b| if b.end > a.end { b } else { a }) else {
        return Vec::new();
    };
    let mut path = vec![cur];
    while let Some(next) = cur
        .pred
        .and_then(|p| jobs.iter().find(|j| j.id == p))
        .filter(|_| path.len() <= jobs.len())
    {
        path.insert(0, next);
        cur = next;
    }
    path
}

/// One workflow's runs of one kind.
#[derive(Debug, Clone)]
pub struct Workflow {
    path: String,
    kind: &'static str,
    counts: Ordered<usize>,
    range: String,
    runs: Vec<Loaded>,
}

/// One repository's workflows.
#[derive(Debug, Clone)]
pub struct RepoResult {
    repo: String,
    workflows: Vec<Workflow>,
}

fn wf(r: &RunInfo) -> &str {
    r.path.split('@').next().unwrap_or("")
}

/// Runs that ran installer or release jobs are timed apart from plain check runs.
fn kind_of(x: &Loaded) -> &'static str {
    static BUNDLES: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(bundle|release|build installers)"));
    if x.jobs.iter().any(|j| BUNDLES.is_match(&j.name)) {
        "with-bundles"
    } else if x.run.event == "pull_request" {
        "pull_request"
    } else {
        "push"
    }
}

/// What the collection needs from outside: the API, the metric cache and a progress line.
pub struct Source<'a, 'c> {
    /// The API of the repository being read.
    pub client: &'a mut Client<'c>,
    /// Where metrics of read logs are kept between runs (`None`: no cache).
    pub cache: Option<&'a Path>,
    /// Progress lines.
    pub log: &'a mut dyn FnMut(&str),
}

fn list_runs(client: &mut Client, limit: usize) -> Result<Vec<RunInfo>, String> {
    let mut all: Vec<RunInfo> = Vec::new();
    let mut per_wf: Ordered<usize> = Ordered::default();
    for page in 1..=12 {
        let p = client.runs_page(page)?;
        if p.runs.is_empty() {
            break;
        }
        for r in &p.runs {
            if r.status == "completed" && matches!(r.conclusion.as_str(), "success" | "failure") {
                *per_wf.entry(wf(r)) += 1;
            }
        }
        all.extend(p.runs);
        if per_wf.len() > 0 && per_wf.iter().all(|(_, n)| *n >= limit) {
            break;
        }
        if all.len() as u64 >= p.total {
            break;
        }
    }
    Ok(all)
}

fn job_metrics(src: &mut Source, repo: &str, job: u64) -> Result<Metrics, String> {
    let file = src
        .cache
        .map(|c| c.join(format!("{}-{job}.json", repo.rsplit('/').next().unwrap_or(repo))));
    if let Some(m) = file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| metrics_from_json(&v))
    {
        return Ok(m);
    }
    let m = src.client.log(job)?.map(|t| parse_log(&t)).unwrap_or_default();
    if let Some(f) = &file {
        let written = f
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(f, metrics_json(&m).to_string()));
        if let Err(e) = written {
            (src.log)(&format!("::warning::the log metrics of job {job} were not cached ({e})"));
        }
    }
    Ok(m)
}

/// Reads one repository: the last `limit` finished runs per workflow, their jobs and, with `logs`, the metrics of their logs.
pub fn analyse(src: &mut Source, repo: &str, limit: usize, logs: bool) -> Result<RepoResult, String> {
    let runs = list_runs(src.client, limit)?;
    let mut by_wf: Ordered<Vec<RunInfo>> = Ordered::default();
    for r in runs {
        by_wf.entry(wf(&r)).push(r);
    }
    let mut result = RepoResult {
        repo: repo.to_string(),
        workflows: Vec::new(),
    };
    for (path, list) in by_wf.iter() {
        let finished: Vec<&RunInfo> = list.iter().filter(|r| r.status == "completed").collect(); // newest first
        let timed: Vec<&RunInfo> = finished
            .iter()
            .filter(|r| matches!(r.conclusion.as_str(), "success" | "failure"))
            .take(limit)
            .copied()
            .collect();
        let floor = timed.last().map_or("", |r| r.created_at.as_str());
        let done: Vec<&&RunInfo> = finished.iter().filter(|r| r.created_at.as_str() >= floor).collect();
        if timed.is_empty() {
            continue;
        }
        let mut useful: Vec<Loaded> = Vec::new();
        for r in &timed {
            let jobs = src
                .client
                .timed_jobs(r.id)
                .map_err(|e| format!("the jobs of run {} could not be read: {e}", r.id))?;
            let l = load_jobs(r, jobs);
            if !l.jobs.is_empty() {
                useful.push(l);
            }
        }
        if logs {
            for x in &mut useful {
                for j in x.jobs.iter_mut().filter(|j| j.conclusion == "success" && j.wall > 30.0) {
                    j.metrics = Some(job_metrics(src, repo, j.id)?);
                }
            }
        }
        let mut counts: Ordered<usize> = Ordered::default();
        for r in &done {
            *counts.entry(&r.conclusion) += 1;
        }
        let range = [done.last(), done.first()]
            .iter()
            .map(|r| r.map_or("", |r| r.created_at.get(..10).unwrap_or(&r.created_at)))
            .collect::<Vec<_>>()
            .join(" to ");
        for kind in ["with-bundles", "pull_request", "push"] {
            let part: Vec<Loaded> = useful.iter().filter(|x| kind_of(x) == kind).cloned().collect();
            if !part.is_empty() {
                result.workflows.push(Workflow {
                    path: path.clone(),
                    kind,
                    counts: counts.clone(),
                    range: range.clone(),
                    runs: part,
                });
            }
        }
        (src.log)(&format!("{repo} {path}: {} runs timed", timed.len()));
    }
    Ok(result)
}

// ---- aggregation ----

/// Count, median and p90 of a series.
#[derive(Debug, Clone)]
struct Stat {
    n: usize,
    med: Option<f64>,
    p90: Option<f64>,
}

fn stat(a: &[f64]) -> Stat {
    Stat {
        n: a.len(),
        med: median(a),
        p90: p90(a),
    }
}

struct JobSummary {
    name: String,
    wall: Stat,
    delay: Stat,
    queue: Stat,
    runners: Ordered<usize>,
    by_runner: Ordered<Option<f64>>,
    cats: Vec<(String, Option<f64>)>,
    steps: Vec<(String, Option<f64>)>,
    metrics: Vec<(String, Option<f64>, usize)>,
}

struct Critical {
    name: String,
    share: f64,
    wall: Option<f64>,
    delay: Option<f64>,
}

struct Summary {
    jobs: Vec<JobSummary>,
    critical: Vec<Critical>,
    walls: Vec<f64>,
    run_delay: Vec<f64>,
    median_chain: Option<f64>,
    median_wait: Option<f64>,
    ref_count: usize,
    ref_kind: &'static str,
}

fn summarise(w: &Workflow) -> Summary {
    let success: Vec<&Loaded> = w.runs.iter().filter(|x| x.run.conclusion == "success").collect();
    let all: Vec<&Loaded> = w.runs.iter().collect();
    let (reference, ref_kind) = if success.len() >= 3 {
        (success, "successful")
    } else {
        (all, "all finished (fewer than 3 successful)")
    };

    let mut jobs_by: Ordered<Vec<&Job>> = Ordered::default();
    for x in &w.runs {
        for j in x.jobs.iter().filter(|j| j.conclusion == "success") {
            jobs_by.entry(&j.name).push(j);
        }
    }
    let mut jobs: Vec<JobSummary> = jobs_by
        .iter()
        .map(|(name, js)| {
            let mut steps: Ordered<Vec<f64>> = Ordered::default();
            let mut cats: Ordered<Vec<f64>> = Ordered::default();
            let mut metrics: Ordered<Vec<f64>> = Ordered::default();
            for j in js {
                let mut cat_sum: Ordered<f64> = Ordered::default();
                for (sname, sec) in &j.steps {
                    let Some(sec) = sec else { continue };
                    steps.entry(sname).push(*sec);
                    *cat_sum.entry(categorize(sname)) += sec;
                }
                for (c, v) in cat_sum.iter() {
                    cats.entry(c).push(*v);
                }
                if let Some(m) = &j.metrics {
                    for (k, v) in m.iter() {
                        let one = if k == "crates compiled" {
                            v.first().copied().unwrap_or(0.0)
                        } else {
                            v.iter().sum()
                        };
                        metrics.entry(k).push(one);
                    }
                }
            }
            let mut runners: Ordered<usize> = Ordered::default();
            for j in js {
                *runners.entry(j.runner.as_deref().unwrap_or("unknown")) += 1;
            }
            let mut by_runner: Ordered<Option<f64>> = Ordered::default();
            for (r, _) in runners.iter() {
                let walls: Vec<f64> = js
                    .iter()
                    .filter(|j| j.runner.as_deref().unwrap_or("unknown") == r)
                    .map(|j| j.wall)
                    .collect();
                *by_runner.entry(r) = median(&walls);
            }
            let mut cat_rows: Vec<(String, Option<f64>)> = cats.iter().map(|(c, a)| (c.clone(), median(a))).collect();
            cat_rows.sort_by(|a, b| desc(a.1, b.1));
            let mut step_rows: Vec<(String, Option<f64>)> = steps.iter().map(|(s, a)| (s.clone(), median(a))).collect();
            step_rows.sort_by(|a, b| desc(a.1, b.1));
            step_rows.truncate(5);
            JobSummary {
                name: name.clone(),
                wall: stat(&js.iter().map(|j| j.wall).collect::<Vec<_>>()),
                delay: stat(&js.iter().map(|j| j.delay).collect::<Vec<_>>()),
                queue: stat(&js.iter().filter(|j| j.first_wave).map(|j| j.delay).collect::<Vec<_>>()),
                runners,
                by_runner,
                cats: cat_rows,
                steps: step_rows,
                metrics: metrics.iter().map(|(k, a)| (k.clone(), median(a), a.len())).collect(),
            }
        })
        .collect();
    jobs.sort_by(|a, b| desc(a.wall.med, b.wall.med));

    let walls: Vec<f64> = reference.iter().map(|x| x.wall).collect();
    let paths: Vec<Vec<&Job>> = reference.iter().map(|x| critical_path(&x.jobs)).collect();
    let mut on_path: Ordered<(Vec<f64>, Vec<f64>)> = Ordered::default();
    for p in &paths {
        for j in p {
            let e = on_path.entry(&j.name);
            e.0.push(j.wall);
            e.1.push(j.delay);
        }
    }
    let total = paths.len() as f64;
    let mut critical: Vec<Critical> = on_path
        .iter()
        .map(|(name, (wall, delay))| Critical {
            name: name.clone(),
            share: wall.len() as f64 / total,
            wall: median(wall),
            delay: median(delay),
        })
        .collect();
    critical.sort_by(|a, b| {
        b.share
            .partial_cmp(&a.share)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| desc(a.wall, b.wall))
    });
    let run_delay: Vec<f64> = reference
        .iter()
        .map(|x| x.jobs.iter().map(|j| (j.start - x.created) / 1000.0).fold(f64::INFINITY, f64::min))
        .collect();
    let median_chain = median(&paths.iter().map(|p| p.iter().map(|j| j.wall).sum()).collect::<Vec<f64>>());
    let median_wait = median(&paths.iter().map(|p| p.iter().map(|j| j.delay).sum()).collect::<Vec<f64>>());
    Summary {
        jobs,
        critical,
        walls,
        run_delay,
        median_chain,
        median_wait,
        ref_count: reference.len(),
        ref_kind,
    }
}

/// Descending order of optional numbers (`b - a`; a missing value sorts as 0, as JavaScript's `null - n` does).
fn desc(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    b.unwrap_or(0.0).partial_cmp(&a.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
}

// ---- rendering ----

fn row<S: AsRef<str>>(cells: &[S]) -> String {
    format!("| {} |", cells.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(" | "))
}

fn dashes(n: usize) -> String {
    row(&vec!["---"; n])
}

fn short(path: &str) -> &str {
    path.strip_prefix(".gitea/workflows/").unwrap_or(path)
}

/// The Markdown report. `today` is `YYYY-MM-DD` (UTC), `limit` the run count the script was asked for.
pub fn render(results: &[RepoResult], limit: usize, today: &str) -> String {
    let mut l: Vec<String> = Vec::new();
    l.push(format!(
        "Generated by `scripts/ci-timings.mjs` on {today}; last {limit} successful or failed runs per workflow (cancelled runs in the same span are only counted). "
    ));
    l.push(
        "Times: median / p90. \"Delay\" is the time from the previous job finishing (or the run being created) to the job starting, so it includes waiting for `needs` as well as queueing. Only successful jobs are timed.\n".into(),
    );
    let mut overview = vec![
        row(&[
            "repo",
            "workflow (trigger)",
            "sample",
            "runs timed",
            "success/failure/cancelled in window",
            "run wall median",
            "p90",
            "first job starts after (median)",
            "critical path jobs-only",
            "critical path waits",
        ]),
        dashes(10),
    ];
    let mut body: Vec<String> = Vec::new();
    for res in results {
        for w in &res.workflows {
            let s = summarise(w);
            let c = |k: &str| w.counts.get(k).copied().unwrap_or(0);
            overview.push(row(&[
                res.repo.clone(),
                format!("{} ({})", short(&w.path), w.kind),
                w.range.clone(),
                w.runs.len().to_string(),
                format!("{}/{}/{}", c("success"), c("failure"), c("cancelled")),
                fmt(median(&s.walls)),
                fmt(p90(&s.walls)),
                fmt(median(&s.run_delay)),
                fmt(s.median_chain),
                fmt(s.median_wait),
            ]));
            if w.runs.len() < 3 {
                continue;
            }
            body.push(format!("### {} / {} ({})\n", res.repo, short(&w.path), w.kind));
            body.push(format!("Timed on {} {} runs.\n", s.ref_count, s.ref_kind));
            body.push("Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):\n".into());
            body.push(row(&["job", "share", "wall (median)", "delay before it (median)"]));
            body.push(dashes(4));
            for c2 in s.critical.iter().take(8) {
                body.push(row(&[
                    c2.name.clone(),
                    format!("{}%", round(c2.share * 100.0)),
                    fmt(c2.wall),
                    fmt(c2.delay),
                ]));
            }
            body.push("\nJobs:\n".into());
            body.push(row(&[
                "job",
                "n",
                "wall median",
                "p90",
                "delay median",
                "queue, first-wave runs (median; n)",
                "runners (count; median wall)",
                "log phases (median)",
                "biggest steps (median)",
            ]));
            body.push(dashes(9));
            for j in &s.jobs {
                let runners = j
                    .runners
                    .iter()
                    .map(|(r, n)| {
                        format!(
                            "{} {n}; {}",
                            r.replace("Gitea Action Runner ", ""),
                            fmt(j.by_runner.get(r).copied().flatten())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let phases = j
                    .metrics
                    .iter()
                    .map(|(k, v, n)| {
                        if k == "crates compiled" {
                            format!("{} crates compiled", v.map_or("NaN".to_string(), |v| round(v).to_string()))
                        } else {
                            format!(
                                "{k} {}{}",
                                fmt(*v),
                                if *n < j.wall.n { format!(" ({n} runs)") } else { String::new() }
                            )
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                let phases = if phases.is_empty() { "-".to_string() } else { phases };
                let steps = j
                    .steps
                    .iter()
                    .map(|(n, v)| format!("{} {}", n.strip_prefix("Run ").unwrap_or(n), fmt(*v)))
                    .collect::<Vec<_>>()
                    .join("; ");
                body.push(row(&[
                    j.name.clone(),
                    j.wall.n.to_string(),
                    fmt(j.wall.med),
                    fmt(j.wall.p90),
                    fmt(j.delay.med),
                    if j.queue.n > 0 {
                        format!("{}; {}", fmt(j.queue.med), j.queue.n)
                    } else {
                        "-".into()
                    },
                    runners,
                    phases,
                    steps,
                ]));
            }
            body.push("\nStep categories per job (median seconds summed per job run; categories by step name):\n".into());
            body.push(row(&["job", "categories"]));
            body.push(dashes(2));
            for j in &s.jobs {
                let cats = j
                    .cats
                    .iter()
                    .filter(|(_, v)| v.is_some_and(|v| v >= 3.0))
                    .map(|(c, v)| format!("{c} {}", fmt(*v)))
                    .collect::<Vec<_>>()
                    .join("; ");
                body.push(row(&[j.name.clone(), cats]));
            }
            body.push(String::new());
        }
    }
    struct RunnerBusy {
        busy: f64,
        n: usize,
        queue: Vec<f64>,
    }
    let mut by_runner: Ordered<RunnerBusy> = Ordered::default();
    impl Default for RunnerBusy {
        fn default() -> Self {
            RunnerBusy {
                busy: 0.0,
                n: 0,
                queue: Vec::new(),
            }
        }
    }
    for res in results {
        for w in &res.workflows {
            for x in &w.runs {
                for j in x.jobs.iter().filter(|j| j.conclusion == "success") {
                    let r = by_runner.entry(j.runner.as_deref().unwrap_or("unknown"));
                    r.busy += j.wall;
                    r.n += 1;
                    if j.first_wave {
                        r.queue.push(j.delay);
                    }
                }
            }
        }
    }
    let mut runner_rows: Vec<(&String, &RunnerBusy)> = by_runner.iter().collect();
    runner_rows.sort_by(|a, b| b.1.busy.partial_cmp(&a.1.busy).unwrap_or(std::cmp::Ordering::Equal));
    let mut runner_table = vec![
        row(&[
            "runner",
            "successful jobs",
            "busy time",
            "first-wave jobs",
            "queue median",
            "queue p90",
        ]),
        dashes(6),
    ];
    for (name, r) in runner_rows {
        runner_table.push(row(&[
            name.clone(),
            r.n.to_string(),
            fmt(Some(r.busy)),
            r.queue.len().to_string(),
            fmt(median(&r.queue)),
            fmt(p90(&r.queue)),
        ]));
    }
    l.push("### Overview\n".into());
    l.extend(overview);
    l.push(String::new());
    l.push("### Runners (all repos, all timed runs)\n".into());
    l.extend(runner_table);
    l.push(String::new());
    l.extend(body);
    l.join("\n")
}

/// The report with its markers replaced inside `existing`, or appended when it has none (`--out`).
pub fn splice(existing: &str, md: &str) -> String {
    let region = format!("{BEGIN}\n{md}\n{END}");
    match (existing.find(BEGIN), existing.find(END)) {
        (Some(b), Some(e)) if e > b => format!("{}{region}{}", &existing[..b], &existing[e + END.len()..]),
        _ => format!("{existing}\n{region}\n"),
    }
}

/// The summaries as JSON (`--json-file`).
pub fn summaries_json(results: &[RepoResult]) -> Value {
    let opt = |v: Option<f64>| v.map_or(Value::Null, |v| json!(v));
    let st = |s: &Stat| json!({"n": s.n, "med": opt(s.med), "p90": opt(s.p90)});
    Value::Array(
        results
            .iter()
            .map(|r| {
                json!({"repo": r.repo, "workflows": r.workflows.iter().map(|w| {
                    let s = summarise(w);
                    json!({
                        "path": w.path, "kind": w.kind, "range": w.range, "runs": w.runs.len(),
                        "summary": {
                            "refCount": s.ref_count, "refKind": s.ref_kind, "medianChain": opt(s.median_chain), "medianWait": opt(s.median_wait),
                            "walls": s.walls, "runDelay": s.run_delay,
                            "critical": s.critical.iter().map(|c| json!({"name": c.name, "share": c.share, "wall": opt(c.wall), "delay": opt(c.delay)})).collect::<Vec<_>>(),
                            "jobs": s.jobs.iter().map(|j| json!({"name": j.name, "wall": st(&j.wall), "delay": st(&j.delay), "queue": st(&j.queue)})).collect::<Vec<_>>(),
                        }
                    })
                }).collect::<Vec<_>>()})
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_runs::testutil::Fake;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ci-runs/timings");

    fn collect(logs: bool, cache: Option<&Path>) -> (Vec<RepoResult>, Fake) {
        let mut fake = Fake::from_dir(Path::new(FIXTURES), "decryp7/paddock");
        let mut c = Client::new(&mut fake, "https://gitea.invalid", "decryp7/paddock", "s3cr3t");
        c.backoff_ms = 0;
        let mut log = |_: &str| {};
        let mut src = Source {
            client: &mut c,
            cache,
            log: &mut log,
        };
        let r = analyse(&mut src, "paddock", 30, logs).unwrap();
        (vec![r], fake)
    }

    /// The oracle: `scripts/ci-timings.mjs --repos paddock --limit 30` on these recorded replies (served by a local stand-in of the
    /// API), run on 2026-11-10; regenerate with that command if the fixtures change.
    #[test]
    fn the_markdown_equals_what_ci_timings_mjs_prints_on_the_same_replies() {
        let (results, _) = collect(true, None);
        let md = render(&results, 30, "2026-11-10");
        let golden = std::fs::read_to_string(format!("{FIXTURES}/golden.md")).unwrap();
        // The script ends its output with a newline of its own.
        assert_eq!(format!("{md}\n"), golden);
    }

    #[test]
    fn the_golden_covers_every_part_of_the_report() {
        let golden = std::fs::read_to_string(format!("{FIXTURES}/golden.md")).unwrap();
        for part in [
            "(with-bundles)",
            "(pull_request)",
            "Critical path",
            "crates compiled",
            "cargo build (test)",
            "nextest run",
            "vitest",
            "playwright run",
            "ng build bundle",
            "Step categories per job",
            "| unknown |",
            "(4 runs)",
        ] {
            assert!(golden.contains(part), "the oracle's output lacks {part}");
        }
    }

    #[test]
    fn without_logs_no_log_is_asked_for() {
        let (results, fake) = collect(false, None);
        assert!(fake.requested().iter().all(|u| !u.contains("/logs")), "{:?}", fake.requested());
        assert!(!render(&results, 30, "2026-11-10").contains("crates compiled"));
    }

    #[test]
    fn log_metrics_are_cached_and_read_back_without_asking_the_server_again() {
        let cache = tempfile::tempdir().unwrap();
        let (first, f1) = collect(true, Some(cache.path()));
        assert!(f1.requested().iter().any(|u| u.contains("/logs")));
        let (second, f2) = collect(true, Some(cache.path()));
        assert!(f2.requested().iter().all(|u| !u.contains("/logs")), "the cache was not used");
        assert_eq!(render(&first, 30, "d"), render(&second, 30, "d"));
    }

    #[test]
    fn log_phases_are_read_from_coloured_logs() {
        let log = "2026-11-09T10:00:00.1Z \x1b[32m   Compiling\x1b[0m serde v1\n2026-11-09T10:00:01.1Z    Compiling syn v2\n\
                   x    Finished `test` profile [unoptimized] target(s) in 1m 05.5s\n\
                   x     Summary [  12.250s] 120 tests run: 120 passed\n\
                   x   Duration  2.50s (transform 1s)\nx   Duration  900ms\n\
                   x  34 passed (1.5m)\nx Application bundle generation complete. [42.100 seconds]\n";
        let m = parse_log(log);
        let get = |k: &str| m.get(k).cloned().unwrap_or_default();
        assert_eq!(get("crates compiled"), vec![2.0]);
        assert_eq!(get("cargo build (test)"), vec![65.5]);
        assert_eq!(get("nextest run"), vec![12.25]);
        assert_eq!(get("vitest"), vec![2.5, 0.9]);
        assert_eq!(get("playwright run"), vec![90.0]);
        assert_eq!(get("ng build bundle"), vec![42.1]);
    }

    #[test]
    fn durations_print_like_the_script() {
        assert_eq!(fmt(None), "-");
        assert_eq!(fmt(Some(89.4)), "89s");
        assert_eq!(fmt(Some(89.5)), "90s");
        assert_eq!(fmt(Some(90.0)), "1m30s");
        assert_eq!(fmt(Some(3725.0)), "62m05s");
        assert_eq!(median(&[3.0, 1.0, 2.0, 10.0]), Some(2.5));
        assert_eq!(p90(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]), Some(9.1));
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn the_out_region_is_replaced_and_the_text_around_it_kept() {
        let first = splice("# Notes\n", "one");
        assert_eq!(first, "# Notes\n\n<!-- ci-timings:begin -->\none\n<!-- ci-timings:end -->\n");
        let second = splice(&first, "two");
        assert_eq!(second, "# Notes\n\n<!-- ci-timings:begin -->\ntwo\n<!-- ci-timings:end -->\n");
        assert_eq!(
            splice("a<!-- ci-timings:begin -->x<!-- ci-timings:end -->b", "y"),
            "a<!-- ci-timings:begin -->\ny\n<!-- ci-timings:end -->b"
        );
    }
}
