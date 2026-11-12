//! `dev wait`: one bounded wait for agents and scripts (ER-21, ER-27).
//!
//! Agents kept writing `while pgrep -f "<cmd>"; do sleep 5; done`: the loop's own command line contains `<cmd>`, so it matches
//! itself and never ends (14 stuck loops in one day); or a `grep` for a line that never comes. This command replaces both:
//!
//! - `--plock <name>`: until the shared build queue (`plock`) logs `<name> done` (a line appended after the wait began; with
//!   `--include-past` also one already in the log);
//! - `--pid <pid>`: until that process exits, identified by pid and start time (rule 53, [`ProcessId`]), so a recycled pid does not
//!   keep the wait alive;
//! - `--file <path> --line <regex>`: until a line (appended after the wait began, or any with `--include-past`) matches;
//! - `--ci-run <id>`: until the Gitea Actions run completes (`GITEA_TOKEN` from the environment, never printed).
//!
//! `--max <duration>` is required: the wait always ends, with an `::error::` line that names the cause and the next step.
//! Phase lines go to standard error as they happen; the result goes to standard output (or JSON with `--json`).
//!
//! # Structure
//!
//! A [`Probe`] answers one question per poll; [`wait_for`] owns the clock, the interval, the limit and the log. Both the probe's
//! inputs (files, processes, HTTP) and the [`Clock`] are traits, so the tests run with fakes and a fake clock and never sleep.

use clap::{ArgGroup, Args};
use paddock_cli::{CliError, Report};
use paddock_proc::{Presence, ProcessId};
use regex::Regex;
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often the phase line "still waiting" is written.
pub const REPORT_EVERY: Duration = Duration::from_secs(30);
/// Polls in a row that may fail (an unreachable server, an unreadable file) before the wait gives up.
pub const MAX_CONSECUTIVE_ERRORS: u32 = 5;
/// The longest `--max` accepted: a day. Longer means the caller should not be waiting in a loop.
pub const MAX_LIMIT: Duration = Duration::from_secs(24 * 60 * 60);
/// The shortest poll interval, so a typo cannot spin a core.
pub const MIN_INTERVAL: Duration = Duration::from_millis(100);
/// At most this many bytes of a watched file are read per poll.
const READ_CHUNK: u64 = 4 * 1024 * 1024;

/// The time source and the sleeper; the tests use a fake that never sleeps.
pub trait Clock {
    /// Time since the wait began.
    fn elapsed(&self) -> Duration;
    /// Wait `d`.
    fn sleep(&mut self, d: Duration);
}

/// The real clock.
pub struct SystemClock(Instant);

impl SystemClock {
    /// A clock that starts now.
    pub fn new() -> SystemClock {
        SystemClock(Instant::now())
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        SystemClock::new()
    }
}

impl Clock for SystemClock {
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// What one poll found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    /// The thing waited for happened; the text says what was seen.
    Done(String),
    /// It ended, but not well (a CI run that failed); the wait stops with this as the error.
    Failed(String),
    /// Not yet; the text says what is seen now (it is shown in the "still waiting" lines).
    Pending(String),
}

/// One condition. `poll` must be cheap and never block for long.
pub trait Probe {
    /// What is waited for, in words (the first phase line).
    fn describe(&self) -> String;
    /// Look once. `Err` is a failed look, tolerated up to [`MAX_CONSECUTIVE_ERRORS`] times in a row.
    fn poll(&mut self) -> Result<Poll, String>;
    /// What to try when the limit passes, for the `::error::` line.
    fn next_step(&self) -> String;
}

/// How a wait ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The condition held after `elapsed`.
    Done {
        /// What the probe saw.
        detail: String,
        /// How long it took.
        elapsed: Duration,
    },
    /// The condition ended badly.
    Failed {
        /// What the probe saw.
        detail: String,
        /// How long it took.
        elapsed: Duration,
    },
    /// `--max` passed.
    TimedOut {
        /// The last thing the probe saw.
        last: String,
        /// The limit.
        max: Duration,
    },
    /// The probe failed [`MAX_CONSECUTIVE_ERRORS`] times in a row.
    Broken {
        /// The last error.
        error: String,
        /// How long the wait had gone on.
        elapsed: Duration,
    },
}

/// Polls `probe` every `interval` until it is done, failed or `max` passes, writing phase lines to `log`.
pub fn wait_for(probe: &mut dyn Probe, clock: &mut dyn Clock, max: Duration, interval: Duration, log: &mut dyn Write) -> Outcome {
    let _ = writeln!(
        log,
        "wait: waiting for {} (max {}, every {})",
        probe.describe(),
        fmt_duration(max),
        fmt_duration(interval)
    );
    let mut last: String;
    let mut errors = 0u32;
    let mut next_report = REPORT_EVERY;
    loop {
        match probe.poll() {
            Ok(Poll::Done(detail)) => {
                let elapsed = clock.elapsed();
                let _ = writeln!(log, "wait: done after {}: {detail}", fmt_duration(elapsed));
                return Outcome::Done { detail, elapsed };
            }
            Ok(Poll::Failed(detail)) => {
                let elapsed = clock.elapsed();
                let _ = writeln!(log, "wait: ended after {}, not well: {detail}", fmt_duration(elapsed));
                return Outcome::Failed { detail, elapsed };
            }
            Ok(Poll::Pending(detail)) => {
                errors = 0;
                last = detail;
            }
            Err(error) => {
                errors += 1;
                let _ = writeln!(log, "wait: look failed ({errors} of {MAX_CONSECUTIVE_ERRORS}): {error}");
                last = format!("look failed: {error}");
                if errors >= MAX_CONSECUTIVE_ERRORS {
                    return Outcome::Broken {
                        error,
                        elapsed: clock.elapsed(),
                    };
                }
            }
        }
        let now = clock.elapsed();
        if now >= max {
            let _ = writeln!(log, "wait: limit {} reached; last seen: {last}", fmt_duration(max));
            return Outcome::TimedOut { last, max };
        }
        if now >= next_report {
            let _ = writeln!(log, "wait: still waiting after {}: {last}", fmt_duration(now));
            next_report = now + REPORT_EVERY;
        }
        // Never sleep past the limit, so the timeout is reported at `max`, not up to one interval later.
        clock.sleep(interval.min(max - now));
    }
}

/// `90s`, `5m`, `1h30m` as text.
pub fn fmt_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        return format!("{ms}ms");
    }
    let s = d.as_secs();
    match (s / 3600, (s % 3600) / 60, s % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, 0) => format!("{m}m"),
        (0, m, s) => format!("{m}m{s}s"),
        (h, 0, 0) => format!("{h}h"),
        (h, m, 0) => format!("{h}h{m}m"),
        (h, m, s) => format!("{h}h{m}m{s}s"),
    }
}

/// Reads `500ms`, `90s`, `5m`, `2h`, `1h30m`; a bare number is seconds.
pub fn parse_duration(text: &str) -> Result<Duration, String> {
    let t = text.trim();
    let bad = || format!("\"{text}\" is not a duration: write 90s, 5m, 2h or 1h30m");
    if t.is_empty() {
        return Err(bad());
    }
    if let Ok(n) = t.parse::<u64>() {
        return Ok(Duration::from_secs(n));
    }
    let mut total = Duration::ZERO;
    let mut rest = t;
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !c.is_ascii_digit()).ok_or_else(bad)?;
        if digits == 0 {
            return Err(bad());
        }
        let n: u64 = rest[..digits].parse().map_err(|_| bad())?;
        rest = &rest[digits..];
        let (unit, len) = if rest.starts_with("ms") {
            (Duration::from_millis(n), 2)
        } else if rest.starts_with('s') {
            (Duration::from_secs(n), 1)
        } else if rest.starts_with('m') {
            (Duration::from_secs(n.saturating_mul(60)), 1)
        } else if rest.starts_with('h') {
            (Duration::from_secs(n.saturating_mul(3600)), 1)
        } else {
            return Err(bad());
        };
        total = total.saturating_add(unit);
        rest = &rest[len..];
    }
    Ok(total)
}

// ---- the files a probe reads ----

/// A file as the probes see it; the tests use an in-memory one.
pub trait Files {
    /// The size of `path`, or `None` when it does not exist.
    fn len(&self, path: &Path) -> std::io::Result<Option<u64>>;
    /// Up to `max` bytes of `path` from `offset`.
    fn read_from(&self, path: &Path, offset: u64, max: u64) -> std::io::Result<Vec<u8>>;
}

/// The real file system.
pub struct DiskFiles;

impl Files for DiskFiles {
    fn len(&self, path: &Path) -> std::io::Result<Option<u64>> {
        match std::fs::metadata(path) {
            Ok(m) => Ok(Some(m.len())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn read_from(&self, path: &Path, offset: u64, max: u64) -> std::io::Result<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(path)?;
        f.seek(SeekFrom::Start(offset))?;
        let mut buf = Vec::new();
        f.take(max).read_to_end(&mut buf)?;
        Ok(buf)
    }
}

/// Waits for a line that matches, in what is appended to a file. This is also what `--plock` is.
pub struct LineProbe<'a> {
    files: &'a dyn Files,
    path: PathBuf,
    pattern: Regex,
    what: String,
    offset: u64,
    /// A partial last line, kept until its newline arrives.
    carry: String,
    lines_seen: u64,
}

impl<'a> LineProbe<'a> {
    /// Starts at the end of the file now (or at its start with `include_past`).
    pub fn new(files: &'a dyn Files, path: PathBuf, pattern: Regex, what: String, include_past: bool) -> Result<LineProbe<'a>, String> {
        let offset = if include_past {
            0
        } else {
            files
                .len(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?
                .unwrap_or(0)
        };
        Ok(LineProbe {
            files,
            path,
            pattern,
            what,
            offset,
            carry: String::new(),
            lines_seen: 0,
        })
    }
}

impl Probe for LineProbe<'_> {
    fn describe(&self) -> String {
        format!("{} in {}", self.what, self.path.display())
    }

    fn poll(&mut self) -> Result<Poll, String> {
        let len = self
            .files
            .len(&self.path)
            .map_err(|e| format!("cannot read {}: {e}", self.path.display()))?;
        let Some(len) = len else {
            return Ok(Poll::Pending(format!("{} does not exist yet", self.path.display())));
        };
        if len < self.offset {
            // Truncated or rotated: start again from its beginning.
            self.offset = 0;
            self.carry.clear();
        }
        while self.offset < len {
            let bytes = self
                .files
                .read_from(&self.path, self.offset, READ_CHUNK.min(len - self.offset))
                .map_err(|e| format!("cannot read {}: {e}", self.path.display()))?;
            if bytes.is_empty() {
                break;
            }
            self.offset += bytes.len() as u64;
            self.carry.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(nl) = self.carry.find('\n') {
                let line: String = self.carry.drain(..=nl).collect();
                let line = line.trim_end_matches(['\n', '\r']);
                self.lines_seen += 1;
                if self.pattern.is_match(line) {
                    return Ok(Poll::Done(format!("line matched: {}", truncate(line, 200))));
                }
            }
        }
        Ok(Poll::Pending(format!("{} new lines read, none matches yet", self.lines_seen)))
    }

    fn next_step(&self) -> String {
        format!(
            "check that the writer of {} still runs; if the line was written before this wait began, add --include-past",
            self.path.display()
        )
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max).collect::<String>())
    }
}

/// `plock`'s activity log: `<root>/pitcrew-lock-activity.log`, the root as plock reads it from its environment.
pub fn plock_log_path(env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    let root = env("PLOCK_DIR")
        .or_else(|| env("PLOCK_ROOT"))
        .unwrap_or_else(|| "/private/tmp/claude-501".to_string());
    PathBuf::from(root).join("pitcrew-lock-activity.log")
}

/// The pattern for the line plock writes when `name` finishes: `<date> <time> <name> done rc=N, ...`.
pub fn plock_done_pattern(name: &str) -> Result<Regex, String> {
    Regex::new(&format!(r"^\S+ \S+ {} done\b", regex::escape(name))).map_err(|e| e.to_string())
}

// ---- --pid ----

/// Waits for a process to exit; the process is its pid AND its start time, so a recycled pid is "gone" (rule 53).
pub struct PidProbe {
    id: ProcessId,
    alive: Box<dyn Fn(ProcessId) -> Presence>,
}

impl PidProbe {
    /// Waits for `id`; `alive` is `ProcessId::presence` outside the tests. `Unknown` (the process table cannot be read) is
    /// never "done": the wait goes on until `--max`, which ends with an `::error::`.
    pub fn new(id: ProcessId, alive: Box<dyn Fn(ProcessId) -> Presence>) -> PidProbe {
        PidProbe { id, alive }
    }
}

impl Probe for PidProbe {
    fn describe(&self) -> String {
        format!("{} to exit", self.id)
    }
    fn poll(&mut self) -> Result<Poll, String> {
        match (self.alive)(self.id) {
            Presence::Running => Ok(Poll::Pending(format!("{} is still running", self.id))),
            Presence::Unknown => Ok(Poll::Pending(format!(
                "{}: the OS process table cannot be read, so it is not known whether it has exited",
                self.id
            ))),
            Presence::Gone => Ok(Poll::Done(format!(
                "{} has exited (or its pid belongs to another process now)",
                self.id
            ))),
        }
    }
    fn next_step(&self) -> String {
        format!(
            "look at what pid {} is doing (`ps -p {}`) or stop it, then run the wait again",
            self.id.pid(),
            self.id.pid()
        )
    }
}

// ---- --ci-run ----

/// A GET that returns JSON. The token travels in a header and appears in no message.
pub trait Http {
    /// Fetches `url`; errors are texts without the token.
    fn get_json(&mut self, url: &str, token: &str) -> Result<Value, String>;
    /// Fetches `url` as text (a job log); errors are texts without the token.
    fn get_text(&mut self, url: &str, token: &str) -> Result<String, String> {
        let _ = (url, token);
        Err("this reader fetches JSON only".into())
    }
}

/// Waits for a Gitea Actions run to complete.
pub struct CiRunProbe<'a> {
    http: &'a mut dyn Http,
    url: String,
    token: String,
    run: u64,
}

impl<'a> CiRunProbe<'a> {
    /// `server` like `https://dev.decryptology.net`, `repo` like `decryp7/paddock`.
    pub fn new(http: &'a mut dyn Http, server: &str, repo: &str, run: u64, token: String) -> CiRunProbe<'a> {
        let url = format!("{}/api/v1/repos/{repo}/actions/runs/{run}", server.trim_end_matches('/'));
        CiRunProbe { http, url, token, run }
    }
}

impl Probe for CiRunProbe<'_> {
    fn describe(&self) -> String {
        format!("CI run {} to complete", self.run)
    }
    fn poll(&mut self) -> Result<Poll, String> {
        let v = self.http.get_json(&self.url, &self.token)?;
        let status = v.get("status").and_then(Value::as_str).unwrap_or("unknown");
        let conclusion = v.get("conclusion").and_then(Value::as_str).unwrap_or("");
        let finished = matches!(status, "completed" | "success" | "failure" | "cancelled" | "skipped") || !conclusion.is_empty();
        if !finished {
            return Ok(Poll::Pending(format!("run {} is {status}", self.run)));
        }
        let result = if conclusion.is_empty() { status } else { conclusion };
        let text = format!("run {} finished: {result}", self.run);
        Ok(if result == "success" {
            Poll::Done(text)
        } else {
            Poll::Failed(text)
        })
    }
    fn next_step(&self) -> String {
        format!(
            "open run {} in the browser to see which job is stuck; a queued run means no runner is free",
            self.run
        )
    }
}

/// The real HTTP: `curl`, with the URL and the Authorization header on standard input (`-K -`) so the token is in no argument list.
pub struct CurlHttp;

impl CurlHttp {
    /// One GET through curl: the body, at most `max_bytes`, within `seconds`.
    fn get(url: &str, token: &str, accept: &str, max_bytes: u64, seconds: u32) -> Result<Vec<u8>, String> {
        use std::process::Stdio;
        if url.contains(['"', '\n', '\\']) || token.contains(['"', '\n', '\\']) {
            return Err("the URL or the token has a character that cannot be sent".into());
        }
        let (max_bytes, seconds) = (max_bytes.to_string(), seconds.to_string());
        let mut child = paddock_proc::command("curl")
            .args([
                "-sS",
                "--fail-with-body",
                "--max-time",
                &seconds,
                "--max-filesize",
                &max_bytes,
                "-K",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start curl: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            // An empty token is an anonymous request (the planner reads public run lists without one).
            let auth = if token.is_empty() {
                String::new()
            } else {
                format!("header = \"Authorization: token {token}\"\n")
            };
            let _ = write!(stdin, "url = \"{url}\"\n{auth}header = \"Accept: {accept}\"\n");
        }
        let out = child.wait_with_output().map_err(|e| format!("curl failed: {e}"))?;
        if !out.status.success() {
            // An empty token would match between every character.
            let mask = |b: &[u8]| {
                let t = String::from_utf8_lossy(b).into_owned();
                if token.is_empty() { t } else { t.replace(token, "***") }
            };
            let (err, body) = (mask(&out.stderr), mask(&out.stdout));
            return Err(format!(
                "curl exited with {}: {} {}",
                out.status,
                truncate(err.trim(), 200),
                truncate(body.trim(), 200)
            ));
        }
        Ok(out.stdout)
    }
}

impl Http for CurlHttp {
    fn get_json(&mut self, url: &str, token: &str) -> Result<Value, String> {
        let body = CurlHttp::get(url, token, "application/json", 16 << 20, 20)?;
        serde_json::from_slice(&body).map_err(|e| format!("the server's answer is not JSON: {e}"))
    }

    fn get_text(&mut self, url: &str, token: &str) -> Result<String, String> {
        // A job log is a few MB; 64 MiB bounds a runaway one.
        let body = CurlHttp::get(url, token, "text/plain", 64 << 20, 120)?;
        Ok(String::from_utf8_lossy(&body).into_owned())
    }
}

// ---- the command line ----

/// `wait`'s arguments.
#[derive(Args)]
#[command(group(ArgGroup::new("what").required(true).args(["plock", "pid", "file", "ci_run"])))]
pub struct WaitArgs {
    /// Until the shared build queue reports `<name> done`.
    #[arg(long, value_name = "NAME")]
    plock: Option<String>,
    /// Until this process exits (checked by pid and start time).
    #[arg(long, value_name = "PID")]
    pid: Option<u32>,
    /// Watch this file (with --line).
    #[arg(long, value_name = "PATH", requires = "line")]
    file: Option<PathBuf>,
    /// The regular expression a line of --file must match.
    #[arg(long, value_name = "REGEX", requires = "file")]
    line: Option<String>,
    /// Until this Gitea Actions run completes (the token is read from GITEA_TOKEN).
    #[arg(long, value_name = "ID")]
    ci_run: Option<u64>,
    /// The longest to wait, for example 90s, 10m, 2h. Required: a wait always ends.
    #[arg(long, value_name = "DURATION")]
    max: String,
    /// How often to look (default 2s; 15s for --ci-run).
    #[arg(long, value_name = "DURATION")]
    interval: Option<String>,
    /// Also accept a matching line that is already in the file (default: only lines written after the wait began).
    #[arg(long)]
    include_past: bool,
    /// plock's activity log (default: PLOCK_DIR or /private/tmp/claude-501, `pitcrew-lock-activity.log`).
    #[arg(long, value_name = "PATH", requires = "plock")]
    plock_log: Option<PathBuf>,
    /// The Gitea server for --ci-run.
    #[arg(long, default_value = "https://dev.decryptology.net", requires = "ci_run")]
    server: String,
    /// The repository for --ci-run.
    #[arg(long, default_value = "decryp7/paddock", requires = "ci_run")]
    repo: String,
}

/// What `run_with` needs from the outside, so the tests can fake all of it.
pub struct Env<'a> {
    /// Environment variables.
    pub var: &'a dyn Fn(&str) -> Option<String>,
    /// Files.
    pub files: &'a dyn Files,
    /// HTTP.
    pub http: &'a mut dyn Http,
    /// Whether the process `ProcessId` names is alive.
    pub alive: Box<dyn Fn(ProcessId) -> Presence>,
    /// The process with this pid now, if any.
    pub process: &'a dyn Fn(u32) -> Option<ProcessId>,
    /// A look at a pid, asked only when `process` finds no identity: only a proven `Gone` is "not running".
    pub look: &'a dyn Fn(u32) -> Presence,
    /// Where the one-waiter-per-target locks are (`None`: outside a repository, no lock is taken).
    pub locks: Option<crate::wait_lock::Locks<'a>>,
}

/// Runs the command against the real machine.
pub fn command(args: &WaitArgs) -> Result<Report, CliError> {
    let var = |k: &str| std::env::var(k).ok();
    let presence = |id: ProcessId| id.presence();
    let mut http = CurlHttp;
    let env = Env {
        var: &var,
        files: &DiskFiles,
        http: &mut http,
        alive: Box::new(ProcessId::presence),
        process: &ProcessId::of,
        look: &paddock_proc::presence,
        locks: real_locks(&presence),
    };
    run_with(args, env, &mut SystemClock::new(), &mut std::io::stderr())
}

/// The lock folder under the current repository's git common directory, or `None` outside a repository.
fn real_locks<'a>(presence: &'a dyn Fn(ProcessId) -> Presence) -> Option<crate::wait_lock::Locks<'a>> {
    let cwd = std::env::current_dir().ok()?;
    let repo = crate::worktree::git::Repo::discover(&cwd).ok()?;
    Some(crate::wait_lock::Locks {
        dir: crate::wait_lock::dir_for(&repo.common_dir),
        me: ProcessId::of(std::process::id())?,
        presence,
    })
}

fn usage(m: String) -> CliError {
    CliError::usage(m).with_hint("Run `./dev wait --help`.")
}

/// The command with its outside world passed in.
pub fn run_with(args: &WaitArgs, env: Env<'_>, clock: &mut dyn Clock, log: &mut dyn Write) -> Result<Report, CliError> {
    let max = parse_duration(&args.max).map_err(usage)?;
    if max.is_zero() || max > MAX_LIMIT {
        return Err(usage(format!("--max {} is out of range: use 1s to 24h.", args.max)));
    }
    let default_interval = if args.ci_run.is_some() {
        Duration::from_secs(15)
    } else {
        Duration::from_secs(2)
    };
    let interval = match &args.interval {
        Some(i) => parse_duration(i).map_err(usage)?,
        None => default_interval,
    }
    .max(MIN_INTERVAL);

    // One waiter per target (ER-32): a second one exits at once with the first one's pid.
    let target = if let Some(n) = &args.plock {
        format!("plock:{n}")
    } else if let Some(p) = args.pid {
        format!("pid:{p}")
    } else if let Some(f) = &args.file {
        format!("file:{}", f.display())
    } else if let Some(r) = args.ci_run {
        format!("ci-run:{}/{}/{r}", args.server, args.repo)
    } else {
        String::new()
    };
    let _lock = match env
        .locks
        .as_ref()
        .filter(|_| !target.is_empty())
        .map(|l| crate::wait_lock::acquire(l, &target))
    {
        Some(Err(crate::wait_lock::LockError::Held { pid })) => {
            let who = if pid == 0 {
                "another waiter".to_string()
            } else {
                format!("pid {pid}")
            };
            let msg = format!("::error::wait: {who} already waits for {target}; a second waiter on one event is refused (ER-32).");
            let _ = writeln!(log, "{msg}");
            return Err(CliError::failure(msg).with_hint(format!(
                "Next: let {who} finish (it ends at its own --max), or stop it, then run `./dev wait` again; never start a second waiter."
            )));
        }
        Some(Err(crate::wait_lock::LockError::Io(e))) => {
            let _ = writeln!(log, "wait: no waiter lock ({e}); waiting without one");
            None
        }
        Some(Ok(l)) => Some(l),
        None => None,
    };
    let kind;
    let outcome = if let Some(name) = &args.plock {
        kind = "plock";
        let path = args.plock_log.clone().unwrap_or_else(|| plock_log_path(env.var));
        let pattern = plock_done_pattern(name).map_err(usage)?;
        let mut probe = LineProbe::new(
            env.files,
            path,
            pattern,
            format!("plock job `{name}` to be done"),
            args.include_past,
        )
        .map_err(CliError::failure)?;
        run_probe(&mut probe, clock, max, interval, log)
    } else if let Some(pid) = args.pid {
        kind = "pid";
        let Some(id) = (env.process)(pid) else {
            if (env.look)(pid) != Presence::Gone {
                // Running (or not known) but without a readable start time: it cannot be told from a recycled pid later.
                let why = format!(
                    "pid {pid} cannot be identified because its start time cannot be read; wait on something else (a log line with --file, a plock job, a CI run)"
                );
                let _ = writeln!(log, "::error::wait: {why}");
                return Err(CliError::failure(format!("{why}.")));
            }
            let _ = writeln!(log, "wait: pid {pid} is not running; nothing to wait for");
            return Ok(Report::new(
                json!({"kind": kind, "result": "done", "detail": "not running at the start"}),
                format!("pid {pid} is not running."),
            ));
        };
        let mut probe = PidProbe::new(id, env.alive);
        run_probe(&mut probe, clock, max, interval, log)
    } else if let Some(path) = &args.file {
        kind = "file";
        let text = args.line.as_deref().unwrap_or_default();
        let pattern = Regex::new(text).map_err(|e| usage(format!("--line is not a valid regular expression: {e}")))?;
        let mut probe = LineProbe::new(
            env.files,
            path.clone(),
            pattern,
            format!("a line matching /{text}/"),
            args.include_past,
        )
        .map_err(CliError::failure)?;
        run_probe(&mut probe, clock, max, interval, log)
    } else if let Some(run) = args.ci_run {
        kind = "ci-run";
        let token = (env.var)("GITEA_TOKEN")
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| usage("GITEA_TOKEN is not set; --ci-run needs it to ask the Gitea API (run `source ~/.zshenv`).".into()))?;
        let mut probe = CiRunProbe::new(env.http, &args.server, &args.repo, run, token.trim().to_string());
        run_probe(&mut probe, clock, max, interval, log)
    } else {
        return Err(usage(
            "say what to wait for: --plock, --pid, --file with --line, or --ci-run.".into(),
        ));
    };
    finish(kind, outcome, log)
}

fn run_probe(probe: &mut dyn Probe, clock: &mut dyn Clock, max: Duration, interval: Duration, log: &mut dyn Write) -> (Outcome, String) {
    let next = probe.next_step();
    (wait_for(probe, clock, max, interval, log), next)
}

fn finish(kind: &str, (outcome, next): (Outcome, String), log: &mut dyn Write) -> Result<Report, CliError> {
    match outcome {
        Outcome::Done { detail, elapsed } => Ok(Report::new(
            json!({"kind": kind, "result": "done", "detail": detail, "elapsedMs": elapsed.as_millis() as u64}),
            format!("Done after {}: {detail}", fmt_duration(elapsed)),
        )),
        Outcome::Failed { detail, .. } => {
            let _ = writeln!(log, "::error::wait: {detail}");
            Err(CliError::failure(format!("{detail}.")).with_hint("Read the run's log for the failing step."))
        }
        Outcome::TimedOut { last, max } => {
            let msg = format!("::error::wait: gave up after {} (--max); last seen: {last}.", fmt_duration(max));
            let _ = writeln!(log, "{msg} Next: {next}");
            Err(CliError::failure(msg).with_hint(format!("Next: {next}.")))
        }
        Outcome::Broken { error, elapsed } => {
            let msg = format!(
                "::error::wait: the look failed {MAX_CONSECUTIVE_ERRORS} times in a row after {}: {error}.",
                fmt_duration(elapsed)
            );
            let _ = writeln!(log, "{msg}");
            Err(CliError::failure(msg).with_hint("Fix the cause above, then run the wait again."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A clock that only moves when the wait sleeps.
    #[derive(Default)]
    struct FakeClock {
        now: Duration,
        sleeps: Vec<Duration>,
    }
    impl Clock for FakeClock {
        fn elapsed(&self) -> Duration {
            self.now
        }
        fn sleep(&mut self, d: Duration) {
            self.sleeps.push(d);
            self.now += d;
        }
    }

    /// Files in memory; `on_read` lets a test append to a file as time passes.
    #[derive(Default)]
    struct FakeFiles {
        files: RefCell<HashMap<PathBuf, Vec<u8>>>,
    }
    impl FakeFiles {
        fn put(&self, p: &str, text: &str) {
            self.files.borrow_mut().insert(PathBuf::from(p), text.as_bytes().to_vec());
        }
        fn append(&self, p: &str, text: &str) {
            self.files
                .borrow_mut()
                .entry(PathBuf::from(p))
                .or_default()
                .extend_from_slice(text.as_bytes());
        }
    }
    impl Files for FakeFiles {
        fn len(&self, p: &Path) -> std::io::Result<Option<u64>> {
            Ok(self.files.borrow().get(p).map(|b| b.len() as u64))
        }
        fn read_from(&self, p: &Path, offset: u64, max: u64) -> std::io::Result<Vec<u8>> {
            let f = self.files.borrow();
            let b = f.get(p).ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
            Ok(b.iter().skip(offset as usize).take(max as usize).copied().collect())
        }
    }

    /// A probe that answers from a script, one answer per poll (the last repeats), and can act on the fake clock's polls.
    struct Scripted {
        answers: Vec<Result<Poll, String>>,
        polls: usize,
    }
    impl Probe for Scripted {
        fn describe(&self) -> String {
            "a scripted thing".into()
        }
        fn poll(&mut self) -> Result<Poll, String> {
            let i = self.polls.min(self.answers.len() - 1);
            self.polls += 1;
            self.answers[i].clone()
        }
        fn next_step(&self) -> String {
            "do the next thing".into()
        }
    }

    fn pending(t: &str) -> Result<Poll, String> {
        Ok(Poll::Pending(t.into()))
    }

    fn go(p: &mut dyn Probe, max: u64, interval: u64) -> (Outcome, String, FakeClock) {
        let mut clock = FakeClock::default();
        let mut log = Vec::new();
        let o = wait_for(p, &mut clock, Duration::from_secs(max), Duration::from_secs(interval), &mut log);
        (o, String::from_utf8(log).unwrap(), clock)
    }

    #[test]
    fn durations_parse_and_print() {
        assert_eq!(parse_duration("90s").unwrap(), Duration::from_secs(90));
        assert_eq!(parse_duration("1h30m").unwrap(), Duration::from_secs(5400));
        assert_eq!(parse_duration("250ms").unwrap(), Duration::from_millis(250));
        assert_eq!(parse_duration("45").unwrap(), Duration::from_secs(45));
        for bad in ["", "m", "5x", "1h-", "s5"] {
            assert!(parse_duration(bad).unwrap_err().contains("not a duration"), "{bad}");
        }
        assert_eq!(fmt_duration(Duration::from_secs(5430)), "1h30m30s");
        assert_eq!(fmt_duration(Duration::from_secs(120)), "2m");
        assert_eq!(fmt_duration(Duration::from_millis(40)), "40ms");
    }

    #[test]
    fn done_ends_the_wait_with_phase_lines_and_no_extra_sleep() {
        let mut p = Scripted {
            answers: vec![pending("a"), pending("b"), Ok(Poll::Done("it happened".into()))],
            polls: 0,
        };
        let (o, log, clock) = go(&mut p, 600, 2);
        assert_eq!(
            o,
            Outcome::Done {
                detail: "it happened".into(),
                elapsed: Duration::from_secs(4)
            }
        );
        assert_eq!(clock.sleeps, [Duration::from_secs(2), Duration::from_secs(2)]);
        assert!(log.starts_with("wait: waiting for a scripted thing (max 10m, every 2s)\n"), "{log}");
        assert!(log.contains("wait: done after 4s: it happened"), "{log}");
    }

    #[test]
    fn a_wait_that_never_ends_is_cut_at_max_not_one_interval_later() {
        let mut p = Scripted {
            answers: vec![pending("still nothing")],
            polls: 0,
        };
        let (o, log, clock) = go(&mut p, 70, 20);
        assert_eq!(
            o,
            Outcome::TimedOut {
                last: "still nothing".into(),
                max: Duration::from_secs(70)
            }
        );
        assert_eq!(clock.now, Duration::from_secs(70));
        assert_eq!(
            clock.sleeps.last(),
            Some(&Duration::from_secs(10)),
            "the last sleep is clipped to the limit"
        );
        assert_eq!(log.matches("still waiting after").count(), 1, "{log}");
        assert!(log.contains("limit 1m10s reached; last seen: still nothing"), "{log}");
    }

    #[test]
    fn failed_looks_are_tolerated_a_few_times_then_end_the_wait() {
        let mut p = Scripted {
            answers: vec![Err("HTTP 502".into()), Err("HTTP 502".into()), pending("back"), Err("x".into())],
            polls: 0,
        };
        let (o, _, _) = go(&mut p, 600, 2);
        // 2 errors, then a good poll resets the count, then errors forever.
        assert!(matches!(o, Outcome::Broken { ref error, .. } if error == "x"), "{o:?}");
        assert_eq!(p.polls, 3 + MAX_CONSECUTIVE_ERRORS as usize);
    }

    #[test]
    fn a_failed_ending_is_reported_as_failed() {
        let mut p = Scripted {
            answers: vec![Ok(Poll::Failed("run 9 finished: failure".into()))],
            polls: 0,
        };
        let (o, log, _) = go(&mut p, 60, 2);
        assert!(matches!(o, Outcome::Failed { .. }));
        assert!(log.contains("not well"), "{log}");
    }

    fn line_probe<'a>(files: &'a FakeFiles, re: &str, past: bool) -> LineProbe<'a> {
        LineProbe::new(files, PathBuf::from("/log"), Regex::new(re).unwrap(), "x".into(), past).unwrap()
    }

    #[test]
    fn a_line_probe_sees_only_new_lines_unless_past_is_asked_for() {
        let files = FakeFiles::default();
        files.put("/log", "old: ready\n");
        let mut p = line_probe(&files, "ready", false);
        assert!(matches!(p.poll().unwrap(), Poll::Pending(_)), "the old line is ignored");
        files.append("/log", "noise\nnew: re");
        assert!(matches!(p.poll().unwrap(), Poll::Pending(_)), "a partial line is not a line yet");
        files.append("/log", "ady\n");
        assert_eq!(p.poll().unwrap(), Poll::Done("line matched: new: ready".into()));
        let mut past = line_probe(&files, "old: ready", true);
        assert!(matches!(past.poll().unwrap(), Poll::Done(_)));
    }

    #[test]
    fn a_line_probe_waits_for_a_file_that_does_not_exist_yet_and_survives_rotation() {
        let files = FakeFiles::default();
        let mut p = line_probe(&files, "go", false);
        assert!(matches!(p.poll().unwrap(), Poll::Pending(t) if t.contains("does not exist")));
        files.put("/log", "a long first line of text\n");
        assert!(matches!(p.poll().unwrap(), Poll::Pending(_)));
        files.put("/log", "go\n"); // shorter than the offset: rotated
        assert!(matches!(p.poll().unwrap(), Poll::Done(_)));
    }

    #[test]
    fn plock_done_means_this_name_only_and_only_new_lines() {
        let files = FakeFiles::default();
        files.put("/log", "2026-11-03 22:00:00 redesign-small done rc=0, ran 3s\n");
        let re = plock_done_pattern("redesign-small").unwrap();
        let mut p = LineProbe::new(&files, PathBuf::from("/log"), re, "plock".into(), false).unwrap();
        files.append("/log", "2026-11-03 22:01:00 redesign-small2 done rc=0, ran 3s\n");
        files.append("/log", "2026-11-03 22:01:01 other waiting 5s: redesign-small done\n");
        files.append(
            "/log",
            "2026-11-03 22:01:02 redesign-small took the lock after 1s: cargo test redesign-small done\n",
        );
        assert!(matches!(p.poll().unwrap(), Poll::Pending(_)));
        files.append("/log", "2026-11-03 22:02:00 redesign-small done rc=101, ran 3s\n");
        assert!(matches!(p.poll().unwrap(), Poll::Done(_)));
        assert!(plock_done_pattern("a.b").unwrap().is_match("d t a.b done rc=0"));
        assert!(
            !plock_done_pattern("a.b").unwrap().is_match("d t axb done rc=0"),
            "the name is escaped"
        );
    }

    #[test]
    fn the_plock_log_follows_plocks_environment() {
        let env = |k: &str| (k == "PLOCK_ROOT").then(|| "/r".to_string());
        assert_eq!(plock_log_path(&env), Path::new("/r").join("pitcrew-lock-activity.log"));
        let none = |_: &str| None;
        assert_eq!(
            plock_log_path(&none),
            Path::new("/private/tmp/claude-501").join("pitcrew-lock-activity.log")
        );
    }

    #[test]
    fn a_pid_wait_follows_the_process_identity_not_the_number() {
        let id = ProcessId::of(std::process::id()).unwrap();
        let alive = std::rc::Rc::new(std::cell::Cell::new(3));
        let a = alive.clone();
        let mut p = PidProbe::new(
            id,
            Box::new(move |_| {
                a.set(a.get() - 1);
                if a.get() > 0 { Presence::Running } else { Presence::Gone }
            }),
        );
        let (o, log, _) = go(&mut p, 60, 1);
        assert!(
            matches!(o, Outcome::Done { elapsed, .. } if elapsed == Duration::from_secs(2)),
            "{o:?}"
        );
        assert!(log.contains("has exited"), "{log}");
    }

    #[test]
    fn a_pid_wait_that_cannot_read_the_process_table_never_reports_done() {
        let id = ProcessId::of(std::process::id()).unwrap();
        let mut p = PidProbe::new(id, Box::new(|_| Presence::Unknown));
        let (o, log, _) = go(&mut p, 10, 1);
        assert!(!matches!(o, Outcome::Done { .. }), "{o:?}");
        assert!(log.contains("cannot be read"), "{log}");
    }

    struct FakeHttp {
        answers: Vec<Result<Value, String>>,
        urls: Vec<String>,
    }
    impl Http for FakeHttp {
        fn get_json(&mut self, url: &str, _token: &str) -> Result<Value, String> {
            self.urls.push(url.to_string());
            let i = self.urls.len().min(self.answers.len()) - 1;
            self.answers[i].clone()
        }
    }

    #[test]
    fn a_ci_run_wait_polls_the_run_until_it_completes() {
        let mut http = FakeHttp {
            answers: vec![
                Ok(json!({"status": "queued"})),
                Ok(json!({"status": "in_progress"})),
                Ok(json!({"status": "completed", "conclusion": "success"})),
            ],
            urls: vec![],
        };
        let mut probe = CiRunProbe::new(&mut http, "https://ci.example/", "o/r", 77, "tok".into());
        let (o, _, _) = go(&mut probe, 600, 15);
        assert!(
            matches!(o, Outcome::Done { elapsed, .. } if elapsed == Duration::from_secs(30)),
            "{o:?}"
        );
        assert_eq!(http.urls[0], "https://ci.example/api/v1/repos/o/r/actions/runs/77");
        let mut http = FakeHttp {
            answers: vec![Ok(json!({"status": "completed", "conclusion": "failure"}))],
            urls: vec![],
        };
        let mut probe = CiRunProbe::new(&mut http, "https://ci.example", "o/r", 77, "tok".into());
        let (o, _, _) = go(&mut probe, 600, 15);
        assert!(
            matches!(o, Outcome::Failed { ref detail, .. } if detail.contains("failure")),
            "{o:?}"
        );
    }

    fn args(rest: &[&str]) -> WaitArgs {
        use clap::Parser;
        #[derive(Parser)]
        struct W {
            #[command(flatten)]
            a: WaitArgs,
        }
        let argv = std::iter::once("wait").chain(rest.iter().copied());
        W::try_parse_from(argv).unwrap_or_else(|e| panic!("{e}")).a
    }

    fn run(
        a: &WaitArgs,
        files: &FakeFiles,
        http: &mut FakeHttp,
        vars: &'static [(&'static str, &'static str)],
    ) -> (Result<Report, CliError>, String) {
        run_looking(a, files, http, vars, &paddock_proc::presence)
    }

    fn run_looking(
        a: &WaitArgs,
        files: &FakeFiles,
        http: &mut FakeHttp,
        vars: &'static [(&'static str, &'static str)],
        look: &dyn Fn(u32) -> Presence,
    ) -> (Result<Report, CliError>, String) {
        let var = move |k: &str| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        let process = |pid: u32| (pid == 1).then(|| ProcessId::of(std::process::id()).unwrap());
        let env = Env {
            var: &var,
            files,
            http,
            alive: Box::new(|_| Presence::Running),
            process: &process,
            look,
            locks: None,
        };
        let mut clock = FakeClock::default();
        let mut log = Vec::new();
        let r = run_with(a, env, &mut clock, &mut log);
        (r, String::from_utf8(log).unwrap())
    }

    #[test]
    fn the_command_requires_a_limit_and_exactly_one_condition() {
        use clap::Parser;
        #[derive(Parser)]
        struct W {
            #[command(flatten)]
            _a: WaitArgs,
        }
        for bad in [
            &["wait", "--pid", "5"][..],
            &["wait", "--max", "5s"],
            &["wait", "--pid", "5", "--plock", "x", "--max", "5s"],
            &["wait", "--file", "/f", "--max", "5s"],
        ] {
            assert!(W::try_parse_from(bad.iter().copied()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_timeout_prints_one_error_annotation_with_the_next_step_and_exits_nonzero() {
        let files = FakeFiles::default();
        files.put("/l", "");
        let mut http = FakeHttp {
            answers: vec![],
            urls: vec![],
        };
        let a = args(&["--file", "/l", "--line", "never", "--max", "10s", "--interval", "1s"]);
        let (r, log) = run(&a, &files, &mut http, &[]);
        let e = r.unwrap_err();
        assert!(e.message.starts_with("::error::wait: gave up after 10s (--max)"), "{}", e.message);
        assert!(e.hint.contains("--include-past"), "{}", e.hint);
        assert!(log.contains("::error::wait: gave up"), "{log}");
        assert_eq!(log.matches("::error::").count(), 1, "{log}");
    }

    #[test]
    fn a_pid_that_is_not_running_is_done_at_once() {
        let files = FakeFiles::default();
        let mut http = FakeHttp {
            answers: vec![],
            urls: vec![],
        };
        let (r, _) = run(&args(&["--pid", "999999", "--max", "5s"]), &files, &mut http, &[]);
        assert_eq!(r.unwrap().data["detail"], "not running at the start");
    }

    #[test]
    fn a_pid_that_cannot_be_identified_is_an_error_unless_it_is_proven_gone() {
        let files = FakeFiles::default();
        let mut http = FakeHttp {
            answers: vec![],
            urls: vec![],
        };
        // The identity lookup finds nothing (pid 5 is not the fake's pid 1): only a proven Gone is "done".
        let a = args(&["--pid", "5", "--max", "5s"]);
        let (r, _) = run_looking(&a, &files, &mut http, &[], &|_| Presence::Gone);
        assert_eq!(r.unwrap().data["detail"], "not running at the start");
        for look in [Presence::Running, Presence::Unknown] {
            let (r, log) = run_looking(&a, &files, &mut http, &[], &move |_| look);
            let e = r.unwrap_err();
            assert!(e.to_string().contains("start time"), "{look:?}: {e}");
            assert!(log.contains("::error::wait: pid 5 cannot be identified"), "{log}");
        }
    }

    #[test]
    fn ci_run_needs_the_token_and_never_prints_it() {
        let files = FakeFiles::default();
        let mut http = FakeHttp {
            answers: vec![Err("HTTP 401".into())],
            urls: vec![],
        };
        let a = args(&["--ci-run", "5", "--max", "30s"]);
        let (r, _) = run(&a, &files, &mut http, &[]);
        assert!(r.unwrap_err().message.contains("GITEA_TOKEN is not set"));
        let (r, log) = run(&a, &files, &mut http, &[("GITEA_TOKEN", "s3cr3t-token")]);
        let e = r.unwrap_err();
        assert!(e.message.contains("::error::") && e.message.contains("HTTP 401"), "{}", e.message);
        assert!(!log.contains("s3cr3t") && !e.text().contains("s3cr3t"));
    }

    #[test]
    fn plock_through_the_command_uses_the_log_of_the_environment() {
        let files = FakeFiles::default();
        files.put(&Path::new("/p").join("pitcrew-lock-activity.log").to_string_lossy(), "");
        let mut http = FakeHttp {
            answers: vec![],
            urls: vec![],
        };
        let a = args(&["--plock", "job1", "--max", "5s", "--interval", "1s"]);
        let (r, log) = run(&a, &files, &mut http, &[("PLOCK_DIR", "/p")]);
        assert!(r.is_err());
        // The log names the path as the OS spells it: `/p\pitcrew-lock-activity.log` on Windows.
        let expected = Path::new("/p").join("pitcrew-lock-activity.log");
        assert!(log.contains(&expected.display().to_string()), "{log}");
    }

    #[test]
    fn a_second_waiter_on_the_same_target_exits_with_the_first_ones_pid_and_the_lock_is_released_on_exit() {
        use crate::wait_lock::{Locks, acquire};
        use paddock_proc::StartTime;
        let dir = tempfile::tempdir().unwrap();
        let running = |_: ProcessId| Presence::Running;
        let other = ProcessId::new(4242, StartTime::from_micros(7));
        let first = Locks {
            dir: dir.path().into(),
            me: other,
            presence: &running,
        };
        let held = acquire(&first, "file:/l").unwrap();
        let files = FakeFiles::default();
        files.put("/l", "ready\n");
        let a = args(&["--file", "/l", "--line", "ready", "--include-past", "--max", "5s"]);
        let go_with = |locks: Option<Locks<'_>>| {
            let var = |_: &str| None;
            let process = |_: u32| None;
            let mut http = FakeHttp {
                answers: vec![],
                urls: vec![],
            };
            let env = Env {
                var: &var,
                files: &files,
                http: &mut http,
                alive: Box::new(|_| Presence::Running),
                process: &process,
                look: &|_| Presence::Running,
                locks,
            };
            let mut log = Vec::new();
            let r = run_with(&a, env, &mut FakeClock::default(), &mut log);
            (r, String::from_utf8(log).unwrap())
        };
        let mine = ProcessId::new(5, StartTime::from_micros(9));
        let (r, log) = go_with(Some(Locks {
            dir: dir.path().into(),
            me: mine,
            presence: &running,
        }));
        let e = r.unwrap_err();
        assert!(e.message.contains("pid 4242 already waits for file:/l"), "{}", e.message);
        assert!(e.hint.contains("Next:"), "{}", e.hint);
        assert_eq!(log.matches("::error::").count(), 1, "{log}");
        drop(held);
        let (r, _) = go_with(Some(Locks {
            dir: dir.path().into(),
            me: mine,
            presence: &running,
        }));
        assert_eq!(r.unwrap().data["result"], "done");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "the lock is released when the wait ends"
        );
    }
}
