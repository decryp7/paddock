//! Tracking the processes a runner starts: which job each belongs to, which ones a finished job left behind, when
//! they are due to be stopped, and which processes may be stopped at all (ADR 0012).
//!
//! The service reads the process table (`sysinfo`), picks the runner's tree ([`crate::usage::tree_members`]) and
//! gives this module [`ProcessMeta`] rows; everything below is pure, with time passed in.
//!
//! # Rules
//!
//! - A process belongs to the job that was running when it started (job spans come from the job tracker and the
//!   history; a few seconds of slack cover that the service notices jobs on a 5-second tick).
//! - A **leftover** is a process (not the runner) that belongs to a job that has finished and is still alive.
//! - A leftover is due `keep` after its job finished (the matching rule's time, else the policy's), plus whatever
//!   the user added with "Keep longer". It is only stopped while the runner is idle and cleanup is on.
//! - A process may be stopped only if it is in the runner's tree *now* (checked against the latest table, with its
//!   start time to catch a reused pid), is not the service, not a system process, and is not the runner itself
//!   unless the caller says so explicitly.

use crate::jobs::{CurrentJob, FinishedJob};
use crate::process_policy::{rule_for, Keep, ProcessPolicy, ProcessRule};
use crate::redact::redact;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Most processes listed for one runner (leftovers first, then the biggest users of memory).
pub const MAX_LISTED: usize = 300;
/// Longest command line shown (after redaction).
pub const MAX_COMMAND: usize = 400;
/// Longest command line looked at before redaction (a Java class path can be huge).
pub const MAX_RAW_COMMAND: usize = 8192;
/// Most job spans considered for attribution.
pub const MAX_JOB_SPANS: usize = 256;
/// Slack when matching a process's start to a job's span (the service sees jobs on a 5 s tick).
pub const ATTRIBUTION_SLACK: Duration = Duration::from_secs(10);
/// Longest "keep longer" extension the user can add in one go, in days (the sum is capped by the same).
pub const MAX_EXTENSION_DAYS: u64 = 30;

/// A process of a runner's tree as the service saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessMeta {
    /// Process id.
    pub pid: u32,
    /// Parent process id.
    pub ppid: Option<u32>,
    /// Program name.
    pub name: String,
    /// Executable path, when known.
    pub exe: Option<String>,
    /// The command line, arguments joined by spaces (raw: only [`display_command`] output leaves the service).
    pub command: String,
    /// When it started (milliseconds since the epoch), when known.
    pub started_ms: Option<u64>,
    /// CPU use, per mille of the whole machine.
    pub cpu_permille: u32,
    /// Resident memory.
    pub mem_bytes: u64,
}

/// The command line as shown: cut, then stripped of secrets, then cut to [`MAX_COMMAND`].
pub fn display_command(raw: &str) -> String {
    let cut: String = raw.chars().take(MAX_RAW_COMMAND).collect();
    let red = redact(&cut);
    if red.chars().count() > MAX_COMMAND {
        let mut s: String = red.chars().take(MAX_COMMAND).collect();
        s.push('…');
        s
    } else {
        red
    }
}

/// When a job ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpan {
    /// When it started.
    pub started: Timestamp,
    /// When it ended; `None` while it runs.
    pub finished: Option<Timestamp>,
    /// What to call it.
    pub label: JobRef,
}

/// Which job, as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRef {
    /// `owner/repo`, when known.
    pub repository: Option<String>,
    /// The job's name in the workflow.
    pub job: Option<String>,
    /// The run's number in its repository.
    pub run_number: Option<u64>,
    /// The job is still running.
    pub running: bool,
}

/// The job spans to attribute against: the newest finished jobs and the one running now.
pub fn spans(finished: impl IntoIterator<Item = FinishedJob>, current: Option<&CurrentJob>) -> Vec<JobSpan> {
    let mut out: Vec<JobSpan> = finished
        .into_iter()
        .map(|j| JobSpan {
            started: j.started_at,
            finished: Some(j.finished_at.max(j.started_at)),
            label: JobRef {
                repository: j.info.repository,
                job: j.info.job,
                run_number: j.info.run_number,
                running: false,
            },
        })
        .collect();
    out.sort_by_key(|s| s.started);
    let skip = out.len().saturating_sub(MAX_JOB_SPANS);
    out.drain(..skip);
    if let Some(c) = current {
        out.push(JobSpan {
            started: c.started_at,
            finished: None,
            label: JobRef {
                repository: c.info.repository.clone(),
                job: c.info.job.clone(),
                run_number: c.info.run_number,
                running: true,
            },
        });
    }
    out
}

/// The job a process started in: the span containing `started_ms` (with slack at both ends), the latest-starting
/// one when several do.
pub fn job_of(started_ms: u64, spans: &[JobSpan]) -> Option<&JobSpan> {
    let slack = u64::try_from(ATTRIBUTION_SLACK.as_millis()).unwrap_or(0);
    spans
        .iter()
        .filter(|s| started_ms + slack >= s.started.0 && s.finished.is_none_or(|f| started_ms <= f.0.saturating_add(slack)))
        .max_by_key(|s| s.started)
}

/// What is going to happen to a leftover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Fate {
    /// It is stopped at this time (if the runner is idle then).
    StopsAt {
        /// When.
        at: Timestamp,
    },
    /// A rule says never.
    Never,
    /// Automatic cleanup is off.
    Off,
}

/// A process a finished job left behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leftover {
    /// When its job ended.
    pub since: Timestamp,
    /// The pattern of the rule that applies; `None`: the default keep time.
    pub rule: Option<String>,
    /// What happens to it.
    pub fate: Fate,
}

/// One row of the Processes tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEntry {
    /// Process id.
    pub pid: u32,
    /// The parent's id.
    pub ppid: Option<u32>,
    /// Program name.
    pub name: String,
    /// The command line, with secrets removed and cut.
    pub command: String,
    /// CPU use, per mille of the whole machine.
    pub cpu_permille: u32,
    /// Resident memory.
    pub mem_bytes: u64,
    /// When it started.
    pub started_at: Option<Timestamp>,
    /// It is the runner process itself.
    pub is_runner: bool,
    /// The job it started in.
    pub job: Option<JobRef>,
    /// Set when a finished job left it behind.
    pub leftover: Option<Leftover>,
}

/// A process with what the rules make of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracked {
    /// The process.
    pub meta: ProcessMeta,
    /// Its job.
    pub job: Option<JobRef>,
    /// Whether and how it is a leftover.
    pub leftover: Option<Leftover>,
    /// How long the rule (or the default) keeps it, for the log line.
    pub keep_label: String,
}

/// Extra time the user added to leftovers, by `(pid, start time)`, in milliseconds.
pub type Extensions = HashMap<(u32, u64), u64>;

/// Adds `extra` to the extension of a process (capped at [`MAX_EXTENSION_DAYS`] days in total).
pub fn extend(ext: &mut Extensions, pid: u32, started_ms: u64, extra: Duration) {
    let cap = MAX_EXTENSION_DAYS * 86_400_000;
    let e = ext.entry((pid, started_ms)).or_insert(0);
    *e = e.saturating_add(u64::try_from(extra.as_millis()).unwrap_or(u64::MAX)).min(cap);
}

/// Drops extensions of processes that are gone.
pub fn prune_extensions(ext: &mut Extensions, alive: &[ProcessMeta]) {
    ext.retain(|(pid, started), _| alive.iter().any(|m| m.pid == *pid && m.started_ms == Some(*started)));
}

/// Processes started within this long of the runner process belong to the runner itself (its console host).
pub const RUNNER_OWN_MS: u64 = 2_000;

/// Classifies the runner's processes. `runner_pid` is the runner process itself.
pub fn classify(runner_pid: u32, procs: &[ProcessMeta], spans: &[JobSpan], policy: &ProcessPolicy, ext: &Extensions) -> Vec<Tracked> {
    // What started with the runner process belongs to it, not to a job: Windows gives every console program a
    // `conhost.exe` child, and a runner restarted while a job's window was open must not lose its own to cleanup.
    let runner_start = procs.iter().find(|m| m.pid == runner_pid).and_then(|m| m.started_ms);
    let belongs_to_runner = |started: u64| runner_start.is_some_and(|r| started <= r.saturating_add(RUNNER_OWN_MS));
    procs
        .iter()
        .map(|m| {
            let is_runner = m.pid == runner_pid;
            let span = if is_runner {
                None
            } else {
                m.started_ms.filter(|s| !belongs_to_runner(*s)).and_then(|s| job_of(s, spans))
            };
            let job = span.map(|s| s.label.clone());
            let finished = span.and_then(|s| s.finished);
            let rule: Option<&ProcessRule> = rule_for(&policy.rules, &m.name, m.exe.as_deref(), &m.command);
            let keep = rule.map_or(Keep::For(policy.keep), |r| r.keep);
            let keep_label = match keep {
                Keep::Never => "never".to_string(),
                Keep::For(k) => k.label(),
            };
            let leftover = finished.map(|since| {
                let fate = if !policy.cleanup {
                    Fate::Off
                } else {
                    match keep {
                        Keep::Never => Fate::Never,
                        Keep::For(k) => {
                            let extra = m.started_ms.and_then(|s| ext.get(&(m.pid, s))).copied().unwrap_or(0);
                            Fate::StopsAt {
                                at: Timestamp(since.0.saturating_add(crate::time::duration_ms(k.duration())).saturating_add(extra)),
                            }
                        }
                    }
                };
                Leftover {
                    since,
                    rule: rule.map(|r| r.pattern.clone()),
                    fate,
                }
            });
            Tracked {
                meta: m.clone(),
                job,
                leftover,
                keep_label,
            }
        })
        .collect()
}

/// The leftovers that are due to be stopped now: cleanup on, the runner idle, the time reached.
pub fn due(tracked: &[Tracked], now: Timestamp, idle: bool) -> Vec<&Tracked> {
    if !idle {
        return vec![];
    }
    tracked
        .iter()
        .filter(|t| matches!(t.leftover.as_ref().map(|l| l.fate), Some(Fate::StopsAt { at }) if at <= now))
        .collect()
}

/// The rows to show: leftovers first, then the runner, then by memory; at most [`MAX_LISTED`]. The flag says rows
/// were left out.
pub fn entries(runner_pid: u32, tracked: &[Tracked]) -> (Vec<ProcessEntry>, bool) {
    let mut rows: Vec<&Tracked> = tracked.iter().collect();
    rows.sort_by(|a, b| {
        let key = |t: &Tracked| {
            (
                t.leftover.is_none(),
                t.meta.pid != runner_pid,
                std::cmp::Reverse(t.meta.mem_bytes),
                t.meta.pid,
            )
        };
        key(a).cmp(&key(b))
    });
    let truncated = rows.len() > MAX_LISTED;
    rows.truncate(MAX_LISTED);
    let out = rows
        .into_iter()
        .map(|t| ProcessEntry {
            pid: t.meta.pid,
            ppid: t.meta.ppid,
            name: t.meta.name.clone(),
            command: display_command(&t.meta.command),
            cpu_permille: t.meta.cpu_permille,
            mem_bytes: t.meta.mem_bytes,
            started_at: t.meta.started_ms.map(Timestamp),
            is_runner: t.meta.pid == runner_pid,
            job: t.job.clone(),
            leftover: t.leftover.clone(),
        })
        .collect();
    (out, truncated)
}

/// Why a process may not be stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// It is not in the runner's process tree (any more).
    NotInTree,
    /// It is the runner process and the caller did not choose that.
    IsRunner,
    /// It is the Pitcrew service.
    IsService,
    /// A system process (pid 0 or 1).
    SystemProcess,
    /// The pid now belongs to another process than the one the caller saw.
    Changed,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Refusal::NotInTree => "that process is not part of this runner's process tree (it may have already ended); Pitcrew only stops processes the runner started",
            Refusal::IsRunner => "that is the runner itself; use Stop or Restart on the runner, or confirm that you want to end the runner process",
            Refusal::IsService => "that is the Pitcrew service",
            Refusal::SystemProcess => "that is a system process",
            Refusal::Changed => "that process id now belongs to a different process; refresh the list",
        })
    }
}

/// Checks that `pid` may be stopped. `members` is the runner's tree from the latest table; `started_ms` is the start
/// time the caller saw (`None` accepts any, for the automatic cleanup that just read it).
pub fn check_stop_target(
    pid: u32,
    started_ms: Option<u64>,
    runner_pid: u32,
    service_pid: u32,
    allow_runner: bool,
    members: &[ProcessMeta],
) -> Result<&ProcessMeta, Refusal> {
    if pid <= 1 {
        return Err(Refusal::SystemProcess);
    }
    if pid == service_pid {
        return Err(Refusal::IsService);
    }
    let m = members.iter().find(|m| m.pid == pid).ok_or(Refusal::NotInTree)?;
    if pid == runner_pid && !allow_runner {
        return Err(Refusal::IsRunner);
    }
    if let (Some(want), Some(have)) = (started_ms, m.started_ms) {
        // Start times come from the OS in whole seconds; allow a second of rounding.
        if want.abs_diff(have) > 1500 {
            return Err(Refusal::Changed);
        }
    }
    Ok(m)
}

/// Where a stop request is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopProgress {
    /// When the process was asked to stop.
    pub asked_at: Timestamp,
    /// How long it gets.
    pub timeout: Duration,
}

/// The next move for a process that was asked to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopStep {
    /// It is gone.
    Done,
    /// Give it more time.
    Wait,
    /// It ignored the request: end it.
    Kill,
}

/// Decides the next step of a stop.
pub fn next_step(p: &StopProgress, still_alive: bool, now: Timestamp) -> StopStep {
    if !still_alive {
        StopStep::Done
    } else if now.since(p.asked_at) >= p.timeout {
        StopStep::Kill
    } else {
        StopStep::Wait
    }
}

/// How many processes (other than the runner) that started during `job` are still alive: the ones it left behind.
pub fn survivors(job: &FinishedJob, procs: &[ProcessMeta], runner_pid: u32) -> u32 {
    let span = [JobSpan {
        started: job.started_at,
        finished: Some(job.finished_at),
        label: JobRef::default(),
    }];
    let n = procs
        .iter()
        .filter(|m| m.pid != runner_pid && m.started_ms.and_then(|s| job_of(s, &span)).is_some())
        .count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// What the log and the history say about the rule behind a leftover: `rule *GradleDaemon* (3 h)` or
/// `default keep time (10 min)`.
pub fn rule_label(t: &Tracked) -> String {
    match t.leftover.as_ref().and_then(|l| l.rule.as_deref()) {
        Some(p) => format!("rule {p} ({})", t.keep_label),
        None => format!("default keep time ({})", t.keep_label),
    }
}

/// Counts the leftovers among `tracked`.
pub fn leftover_count(tracked: &[Tracked]) -> u32 {
    u32::try_from(tracked.iter().filter(|t| t.leftover.is_some()).count()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::JobInfo;
    use crate::process_policy::{KeepFor, ProcessPolicy};

    fn t(s: u64) -> Timestamp {
        Timestamp(s * 1000)
    }

    fn proc(pid: u32, name: &str, started_s: Option<u64>) -> ProcessMeta {
        ProcessMeta {
            pid,
            ppid: Some(1),
            name: name.into(),
            exe: Some(format!("/usr/bin/{name}")),
            command: format!("{name} --flag"),
            started_ms: started_s.map(|s| s * 1000),
            cpu_permille: 10,
            mem_bytes: u64::from(pid) * 100,
        }
    }

    fn job(from: u64, to: u64, repo: &str) -> FinishedJob {
        FinishedJob {
            info: JobInfo {
                repository: Some(repo.into()),
                ..Default::default()
            },
            started_at: t(from),
            finished_at: t(to),
            outcome: None,
            peak_cpu_permille: 0,
            peak_mem_bytes: 0,
            leftovers: 0,
        }
    }

    #[test]
    fn a_process_belongs_to_the_job_running_when_it_started() {
        let s = spans(vec![job(100, 200, "a/one"), job(300, 400, "a/two")], None);
        let repo = |at: u64| job_of(at * 1000, &s).and_then(|x| x.label.repository.clone());
        assert_eq!(repo(150).as_deref(), Some("a/one"));
        assert_eq!(repo(350).as_deref(), Some("a/two"));
        assert_eq!(repo(205).as_deref(), Some("a/one"), "slack after the end");
        assert_eq!(repo(95).as_deref(), Some("a/one"), "slack before the start");
        assert_eq!(repo(250), None);
        assert_eq!(repo(50), None);
        let cur = CurrentJob {
            info: JobInfo::default(),
            started_at: t(500),
        };
        let s = spans(vec![job(100, 200, "a/one")], Some(&cur));
        assert!(job_of(600_000, &s).unwrap().label.running);
        // Overlapping spans (slack): the later job wins.
        let s = spans(vec![job(100, 200, "a/one"), job(203, 300, "a/two")], None);
        assert_eq!(job_of(204_000, &s).unwrap().label.repository.as_deref(), Some("a/two"));
        // Bounded.
        let many: Vec<FinishedJob> = (0..400).map(|n| job(n * 10, n * 10 + 5, "x/y")).collect();
        assert_eq!(spans(many, None).len(), MAX_JOB_SPANS);
        // A job that ended before it started is a zero-length span.
        assert_eq!(spans(vec![job(100, 50, "a/b")], None)[0].finished, Some(t(100)));
    }

    #[test]
    fn what_starts_with_the_runner_is_never_a_leftover() {
        // The runner was restarted (pid 50, started at 10_000) inside a job's window that never got its end in
        // time; its console host (pid 51) starts with it and must not be stopped as that job's leftover.
        let meta = |pid: u32, started: u64, name: &str| ProcessMeta {
            pid,
            ppid: Some(50),
            name: name.into(),
            exe: None,
            command: String::new(),
            started_ms: Some(started),
            cpu_permille: 0,
            mem_bytes: 1,
        };
        let procs = [
            meta(50, 10_000, "gitea-runner.exe"),
            meta(51, 10_400, "conhost.exe"),
            meta(60, 15_000, "node.exe"),
        ];
        let spans = [JobSpan {
            label: JobRef {
                job: Some("build".into()),
                ..JobRef::default()
            },
            started: Timestamp(5_000),
            finished: Some(Timestamp(20_000)),
        }];
        let tracked = classify(50, &procs, &spans, &ProcessPolicy::default(), &Extensions::default());
        assert!(tracked[1].leftover.is_none() && tracked[1].job.is_none(), "{:?}", tracked[1]);
        assert!(tracked[2].leftover.is_some(), "a job's process is still a leftover");
    }

    #[test]
    fn leftovers_are_processes_of_finished_jobs_and_get_their_keep_time() {
        let spans = spans(vec![job(100, 200, "a/one")], None);
        let procs = vec![
            proc(10, "gitea-runner", Some(1)),
            proc(11, "sleep", Some(150)),
            proc(12, "java", Some(150)),
            proc(13, "tool", None),
            proc(14, "adb", Some(160)),
        ];
        let mut procs = procs;
        procs[2].command = "java org.gradle.launcher.daemon.bootstrap.GradleDaemon".into();
        let policy = ProcessPolicy::default();
        let tr = classify(10, &procs, &spans, &policy, &Extensions::new());
        let by = |pid: u32| tr.iter().find(|x| x.meta.pid == pid).unwrap();
        assert!(by(10).leftover.is_none() && by(10).job.is_none(), "the runner is nobody's leftover");
        assert_eq!(by(11).leftover.as_ref().unwrap().fate, Fate::StopsAt { at: t(200 + 600) });
        assert_eq!(by(11).leftover.as_ref().unwrap().rule, None);
        assert_eq!(by(12).leftover.as_ref().unwrap().fate, Fate::StopsAt { at: t(200 + 3 * 3600) });
        assert_eq!(by(12).leftover.as_ref().unwrap().rule.as_deref(), Some("*GradleDaemon*"));
        assert_eq!(by(12).keep_label, "3 h");
        assert!(by(13).leftover.is_none(), "unknown start: no job, no leftover");
        assert_eq!(by(14).leftover.as_ref().unwrap().fate, Fate::Never);
        assert_eq!(leftover_count(&tr), 3);
        assert_eq!(by(11).job.as_ref().unwrap().repository.as_deref(), Some("a/one"));
        // Cleanup off: still listed as leftovers, with no countdown.
        let off = ProcessPolicy {
            cleanup: false,
            ..ProcessPolicy::default()
        };
        let tr = classify(10, &procs, &spans, &off, &Extensions::new());
        assert_eq!(
            tr.iter().find(|x| x.meta.pid == 11).unwrap().leftover.as_ref().unwrap().fate,
            Fate::Off
        );
        // A process of a job that is still running is not a leftover.
        let cur = CurrentJob {
            info: JobInfo::default(),
            started_at: t(300),
        };
        let running = super::spans(vec![], Some(&cur));
        let tr = classify(10, &[proc(20, "cc", Some(310))], &running, &policy, &Extensions::new());
        assert!(tr[0].leftover.is_none() && tr[0].job.as_ref().unwrap().running);
    }

    #[test]
    fn keep_longer_adds_time_and_is_capped() {
        let spans = spans(vec![job(100, 200, "a/one")], None);
        let procs = vec![proc(11, "sleep", Some(150))];
        let mut ext = Extensions::new();
        extend(&mut ext, 11, 150_000, Duration::from_secs(3600));
        let tr = classify(10, &procs, &spans, &ProcessPolicy::default(), &ext);
        assert_eq!(tr[0].leftover.as_ref().unwrap().fate, Fate::StopsAt { at: t(200 + 600 + 3600) });
        extend(&mut ext, 11, 150_000, Duration::from_secs(40 * 86_400));
        assert_eq!(ext[&(11, 150_000)], MAX_EXTENSION_DAYS * 86_400_000);
        prune_extensions(&mut ext, &procs);
        assert_eq!(ext.len(), 1);
        prune_extensions(&mut ext, &[proc(11, "sleep", Some(999))]);
        assert!(ext.is_empty(), "a reused pid does not inherit the extension");
    }

    #[test]
    fn only_idle_runners_stop_due_leftovers() {
        let spans = spans(vec![job(100, 200, "a/one")], None);
        let procs = vec![proc(11, "sleep", Some(150)), proc(14, "adb", Some(150))];
        let tr = classify(10, &procs, &spans, &ProcessPolicy::default(), &Extensions::new());
        let pids = |d: Vec<&Tracked>| d.iter().map(|x| x.meta.pid).collect::<Vec<_>>();
        assert!(due(&tr, t(799), true).is_empty(), "not yet");
        assert_eq!(pids(due(&tr, t(800), true)), vec![11], "adb is never stopped");
        assert!(due(&tr, t(10_000), false).is_empty(), "never while a job runs");
        let off = classify(
            10,
            &procs,
            &spans,
            &ProcessPolicy {
                cleanup: false,
                ..Default::default()
            },
            &Extensions::new(),
        );
        assert!(due(&off, t(10_000_000), true).is_empty());
    }

    #[test]
    fn the_list_is_bounded_and_ordered_and_shows_redacted_commands() {
        let spans = spans(vec![job(100, 200, "a/one")], None);
        let mut procs: Vec<ProcessMeta> = (20..20 + MAX_LISTED as u32 + 20).map(|p| proc(p, "work", Some(10))).collect();
        procs.push(proc(10, "gitea-runner", Some(1)));
        let mut leftover = proc(11, "leftover", Some(150));
        leftover.command = "tool --token 0123456789abcdef0123456789abcdef01234567 --x".into();
        procs.push(leftover);
        let tr = classify(10, &procs, &spans, &ProcessPolicy::default(), &Extensions::new());
        let (rows, truncated) = entries(10, &tr);
        assert!(truncated);
        assert_eq!(rows.len(), MAX_LISTED);
        assert_eq!(rows[0].pid, 11, "leftovers first");
        assert_eq!(rows[1].pid, 10, "then the runner");
        assert!(rows[0].command.contains("[redacted]") && !rows[0].command.contains("0123456789abcdef"));
        assert!(rows[1].is_runner);
        let (few, truncated) = entries(10, &tr[..3]);
        assert!(!truncated && few.len() == 3);
    }

    #[test]
    fn commands_are_cut_and_cleaned() {
        assert_eq!(display_command("a b"), "a b");
        let long = format!("java -cp {}", "x".repeat(5000));
        let shown = display_command(&long);
        assert_eq!(shown.chars().count(), MAX_COMMAND + 1);
        assert!(shown.ends_with('…'));
        let huge = "y".repeat(MAX_RAW_COMMAND * 3);
        assert!(display_command(&huge).chars().count() <= MAX_COMMAND + 1);
        assert_eq!(
            display_command("curl -H 'Authorization: Bearer abc.def'"),
            "curl -H 'Authorization: [redacted]'"
        );
    }

    #[test]
    fn a_stop_is_refused_outside_the_tree_and_for_the_wrong_targets() {
        let members = vec![proc(10, "gitea-runner", Some(1)), proc(11, "sleep", Some(150))];
        let ok = check_stop_target(11, Some(150_000), 10, 99, false, &members);
        assert_eq!(ok.unwrap().pid, 11);
        assert_eq!(
            check_stop_target(11, Some(150_500), 10, 99, false, &members).unwrap().pid,
            11,
            "seconds rounding"
        );
        assert_eq!(check_stop_target(11, None, 10, 99, false, &members).unwrap().pid, 11);
        assert_eq!(
            check_stop_target(500, None, 10, 99, false, &members).unwrap_err(),
            Refusal::NotInTree
        );
        assert_eq!(check_stop_target(10, None, 10, 99, false, &members).unwrap_err(), Refusal::IsRunner);
        assert_eq!(
            check_stop_target(10, None, 10, 99, true, &members).unwrap().pid,
            10,
            "explicitly chosen"
        );
        assert_eq!(check_stop_target(99, None, 10, 99, true, &members).unwrap_err(), Refusal::IsService);
        assert_eq!(
            check_stop_target(1, None, 10, 99, true, &members).unwrap_err(),
            Refusal::SystemProcess
        );
        assert_eq!(
            check_stop_target(0, None, 10, 99, true, &members).unwrap_err(),
            Refusal::SystemProcess
        );
        assert_eq!(
            check_stop_target(11, Some(900_000), 10, 99, false, &members).unwrap_err(),
            Refusal::Changed
        );
        for r in [
            Refusal::NotInTree,
            Refusal::IsRunner,
            Refusal::IsService,
            Refusal::SystemProcess,
            Refusal::Changed,
        ] {
            assert!(!r.to_string().is_empty());
        }
        assert!(Refusal::NotInTree.to_string().contains("only stops processes the runner started"));
    }

    #[test]
    fn survivors_are_the_job_s_processes_still_alive() {
        let j = job(100, 200, "a/one");
        let procs = vec![
            proc(10, "gitea-runner", Some(1)),
            proc(11, "sleep", Some(150)),
            proc(12, "other", Some(500)),
            proc(13, "x", None),
        ];
        assert_eq!(survivors(&j, &procs, 10), 1);
        assert_eq!(survivors(&j, &[], 10), 0);
        let tr = classify(10, &procs, &spans(vec![j], None), &ProcessPolicy::default(), &Extensions::new());
        let t = tr.iter().find(|t| t.meta.pid == 11).unwrap();
        assert_eq!(rule_label(t), "default keep time (10 min)");
        let mut g = proc(14, "java", Some(150));
        g.command = "GradleDaemon".into();
        let tr = classify(
            10,
            &[g],
            &spans(vec![job(100, 200, "a/one")], None),
            &ProcessPolicy::default(),
            &Extensions::new(),
        );
        assert_eq!(rule_label(&tr[0]), "rule *GradleDaemon* (3 h)");
    }

    #[test]
    fn a_stop_waits_then_ends_what_ignores_it() {
        let p = StopProgress {
            asked_at: t(100),
            timeout: Duration::from_secs(15),
        };
        assert_eq!(next_step(&p, true, t(110)), StopStep::Wait);
        assert_eq!(next_step(&p, true, t(115)), StopStep::Kill);
        assert_eq!(next_step(&p, false, t(101)), StopStep::Done);
        assert_eq!(
            next_step(&p, true, t(50)),
            StopStep::Wait,
            "a clock that went back never kills early"
        );
        let _ = KeepFor::minutes(1);
    }
}
