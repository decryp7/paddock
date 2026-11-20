//! `dev ci host-sample`: samples the Linux host's CPU idle, iowait, load and memory every `--interval` (default 10 s) and says
//! whether the runner has headroom for more job lanes. Built for one question (docs/dev-setup.md, "Sampling the CI host"): before
//! `runner.capacity` of a Linux runner goes from 3 to 6, what did the host look like across one full train CI round?
//!
//! ```text
//! dev ci host-sample --out <file.jsonl> --max <duration> [--interval 10s] [--until-run <gitea run id>] [--lanes 3] [--target-lanes 6]
//! ```
//!
//! Reads only `/proc/stat`, `/proc/loadavg` and `/proc/meminfo` (Linux only: elsewhere one `::error::` line says so). Writes only
//! `--out` (one JSON line per tick: `ts` as Unix seconds in UTC, `cpu_idle_pct`, `iowait_pct`, `load1`, `load5`, `load15`,
//! `mem_avail_mb`, `mem_total_mb`, `swap_used_mb`; flushed per line, so a killed job keeps what it saw) and `$GITHUB_STEP_SUMMARY`
//! (appended, when set). It installs nothing (ER-45). It ends at `--max` (required, ER-32), or earlier when `--until-run` names a
//! Gitea Actions run that has finished (read through [`CiRunProbe`], the reader `dev wait --ci-run` uses; `GITEA_TOKEN` from the
//! environment). Inside a container `/proc` shows the host's values unless lxcfs is used, which is what this wants.
//!
//! # The verdict
//!
//! "headroom for N more lanes" with N = `--target-lanes` - `--lanes` (defaults 6 and 3). It assumes the current lanes were busy for
//! most of the sample (the sampler cannot see that; sample a full train round) and scales the busiest the host got by
//! `target / lanes`:
//!
//! - `not measured` with fewer than [`MIN_TICKS`] ticks (5 minutes at the default interval). Never a pass (ER-33).
//! - `no` when any of: projected CPU busy (p95 of `100 - cpu_idle_pct`, times the scale) is above [`MAX_BUSY_PCT`]; projected iowait
//!   (p95, times the scale) is above [`MAX_IOWAIT_PCT`]; projected memory in use (`mem_total - min mem_avail`, times the scale) is
//!   above [`MAX_MEM_USED_PCT`] of `mem_total`; or swap in use at any tick is above [`MAX_SWAP_PCT`] of `mem_total`.
//! - `yes` otherwise.
//!
//! Load averages are reported (max load1 against the core count) but not part of the verdict: they count blocked threads, which
//! the CPU and iowait numbers already show more directly.

use crate::ci_tool::error_line;
use crate::wait::{CiRunProbe, Clock, CurlHttp, Http, Poll, Probe, SystemClock, fmt_duration, parse_duration};
use clap::Args;
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// Fewer ticks than this give `not measured`.
pub const MIN_TICKS: usize = 30;
/// Projected CPU busy above this (percent) is "no".
pub const MAX_BUSY_PCT: f64 = 90.0;
/// Projected iowait above this (percent) is "no".
pub const MAX_IOWAIT_PCT: f64 = 20.0;
/// Projected memory in use above this share of the total (percent) is "no".
pub const MAX_MEM_USED_PCT: f64 = 90.0;
/// Swap in use above this share of the memory total (percent) at any tick is "no".
pub const MAX_SWAP_PCT: f64 = 5.0;
/// The run is looked at no more often than this.
const RUN_POLL_EVERY: Duration = Duration::from_secs(30);
/// Reads in a row that may fail before the sampler gives up.
const MAX_READ_ERRORS: u32 = 5;
/// The shortest `--interval`.
const MIN_INTERVAL: Duration = Duration::from_secs(1);

/// `dev ci host-sample`.
#[derive(Args, Debug)]
pub struct HostSampleArgs {
    /// The file that gets one JSON line per tick (created or replaced; its folder must exist).
    #[arg(long, value_name = "FILE")]
    pub out: PathBuf,
    /// The longest to sample, for example 3h. Required: a sampler always ends.
    #[arg(long, value_name = "DURATION")]
    pub max: String,
    /// The time between two ticks.
    #[arg(long, value_name = "DURATION", default_value = "10s")]
    pub interval: String,
    /// Stop when this Gitea Actions run has finished (the token is read from GITEA_TOKEN).
    #[arg(long, value_name = "ID")]
    pub until_run: Option<u64>,
    /// How many job lanes the runner has now.
    #[arg(long, default_value_t = 3)]
    pub lanes: u32,
    /// How many lanes the verdict asks about.
    #[arg(long, default_value_t = 6)]
    pub target_lanes: u32,
    /// The Gitea server for --until-run.
    #[arg(long, default_value = "https://dev.decryptology.net", requires = "until_run")]
    pub server: String,
    /// The repository for --until-run.
    #[arg(long, default_value = "decryp7/paddock", requires = "until_run")]
    pub repo: String,
}

// ---- the pure core ----

/// The CPU counters of the first line of `/proc/stat`, in ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTicks {
    idle: u64,
    iowait: u64,
    total: u64,
}

/// What one `/proc` read gives.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    cpu: CpuTicks,
    cores: u32,
    load: [f64; 3],
    mem_avail_mb: u64,
    mem_total_mb: u64,
    swap_used_mb: u64,
}

/// One output line.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    /// Seconds since the Unix epoch.
    pub ts: u64,
    /// CPU idle over the tick, percent.
    pub cpu_idle_pct: f64,
    /// CPU iowait over the tick, percent.
    pub iowait_pct: f64,
    /// The load averages.
    pub load: [f64; 3],
    /// MemAvailable, MB.
    pub mem_avail_mb: u64,
    /// MemTotal, MB.
    pub mem_total_mb: u64,
    /// SwapTotal - SwapFree, MB.
    pub swap_used_mb: u64,
}

impl Tick {
    /// The JSON line (no newline).
    pub fn json_line(&self) -> String {
        json!({
            "ts": self.ts,
            "cpu_idle_pct": self.cpu_idle_pct,
            "iowait_pct": self.iowait_pct,
            "load1": self.load[0],
            "load5": self.load[1],
            "load15": self.load[2],
            "mem_avail_mb": self.mem_avail_mb,
            "mem_total_mb": self.mem_total_mb,
            "swap_used_mb": self.swap_used_mb,
        })
        .to_string()
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Reads `/proc/stat` (the `cpu` line and the count of `cpuN` lines).
pub fn parse_stat(text: &str) -> Result<(CpuTicks, u32), String> {
    let mut cpu = None;
    let mut cores = 0u32;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("cpu") => {
                let nums: Vec<u64> = parts
                    .map(|p| p.parse::<u64>().map_err(|_| format!("/proc/stat: '{p}' is not a number")))
                    .collect::<Result<_, _>>()?;
                if nums.len() < 5 {
                    return Err("/proc/stat: the cpu line has fewer than 5 counters".into());
                }
                // user nice system idle iowait irq softirq steal; guest time is already inside user and nice.
                let total: u64 = nums.iter().take(8).sum();
                cpu = Some(CpuTicks {
                    idle: nums[3],
                    iowait: nums[4],
                    total,
                });
            }
            Some(name) if name.starts_with("cpu") && name[3..].chars().all(|c| c.is_ascii_digit()) && name.len() > 3 => cores += 1,
            _ => {}
        }
    }
    cpu.map(|c| (c, cores.max(1)))
        .ok_or_else(|| "/proc/stat has no cpu line".to_string())
}

/// Reads `/proc/loadavg`.
pub fn parse_loadavg(text: &str) -> Result<[f64; 3], String> {
    let v: Vec<f64> = text
        .split_whitespace()
        .take(3)
        .map(|p| p.parse::<f64>().map_err(|_| format!("/proc/loadavg: '{p}' is not a number")))
        .collect::<Result<_, _>>()?;
    <[f64; 3]>::try_from(v).map_err(|_| "/proc/loadavg has fewer than 3 numbers".to_string())
}

/// Reads `/proc/meminfo`: (MemTotal, MemAvailable, swap in use), all MB.
pub fn parse_meminfo(text: &str) -> Result<(u64, u64, u64), String> {
    let kb = |key: &str| -> Option<u64> {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
            .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
    };
    let need = |key: &str| kb(key).ok_or_else(|| format!("/proc/meminfo has no {key} line"));
    let total = need("MemTotal")?;
    let avail = need("MemAvailable")?;
    let swap_total = need("SwapTotal")?;
    let swap_free = need("SwapFree")?;
    Ok((total / 1024, avail / 1024, swap_total.saturating_sub(swap_free) / 1024))
}

/// The three files, parsed.
pub fn parse_reading(stat: &str, loadavg: &str, meminfo: &str) -> Result<Reading, String> {
    let (cpu, cores) = parse_stat(stat)?;
    let (mem_total_mb, mem_avail_mb, swap_used_mb) = parse_meminfo(meminfo)?;
    Ok(Reading {
        cpu,
        cores,
        load: parse_loadavg(loadavg)?,
        mem_avail_mb,
        mem_total_mb,
        swap_used_mb,
    })
}

/// The tick between two readings, `None` when the counters did not advance (or went backwards).
pub fn tick_between(prev: &Reading, cur: &Reading, ts: u64) -> Option<Tick> {
    let total = cur.cpu.total.checked_sub(prev.cpu.total).filter(|t| *t > 0)? as f64;
    let idle = cur.cpu.idle.checked_sub(prev.cpu.idle)? as f64;
    let iowait = cur.cpu.iowait.checked_sub(prev.cpu.iowait)? as f64;
    Some(Tick {
        ts,
        cpu_idle_pct: round1(100.0 * idle / total),
        iowait_pct: round1(100.0 * iowait / total),
        load: cur.load,
        mem_avail_mb: cur.mem_avail_mb,
        mem_total_mb: cur.mem_total_mb,
        swap_used_mb: cur.swap_used_mb,
    })
}

/// The answer to "is there headroom".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Headroom {
    /// The rule in the module doc holds.
    Yes,
    /// The rule failed; the summary says which part.
    No,
    /// Too few ticks to say.
    NotMeasured,
}

impl Headroom {
    fn word(self) -> &'static str {
        match self {
            Headroom::Yes => "yes",
            Headroom::No => "no",
            Headroom::NotMeasured => "not measured",
        }
    }
}

/// The numbers of a sample.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Ticks taken.
    pub ticks: usize,
    /// Seconds between the first and the last tick.
    pub span_secs: u64,
    /// Lowest CPU idle.
    pub min_idle: f64,
    /// Median CPU idle.
    pub median_idle: f64,
    /// 95th percentile of CPU idle.
    pub p95_idle: f64,
    /// 95th percentile of CPU busy (100 - idle).
    pub p95_busy: f64,
    /// 95th percentile of iowait.
    pub p95_iowait: f64,
    /// Highest load1.
    pub max_load1: f64,
    /// Cores of the host.
    pub cores: u32,
    /// Lowest MemAvailable, MB.
    pub min_mem_avail_mb: u64,
    /// MemTotal, MB.
    pub mem_total_mb: u64,
    /// Highest swap in use, MB.
    pub max_swap_used_mb: u64,
    /// Lanes now.
    pub lanes: u32,
    /// Lanes asked about.
    pub target_lanes: u32,
    /// The verdict.
    pub headroom: Headroom,
    /// Why, in words.
    pub reason: String,
}

/// Nearest-rank percentile of `sorted` (ascending, not empty).
fn percentile(sorted: &[f64], p: f64) -> f64 {
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Summarises ticks and applies the rule of the module doc. `cores` is the host's core count.
pub fn summarize(ticks: &[Tick], cores: u32, lanes: u32, target_lanes: u32) -> Summary {
    let n = ticks.len();
    let mut idle: Vec<f64> = ticks.iter().map(|t| t.cpu_idle_pct).collect();
    let mut busy: Vec<f64> = ticks.iter().map(|t| 100.0 - t.cpu_idle_pct).collect();
    let mut iow: Vec<f64> = ticks.iter().map(|t| t.iowait_pct).collect();
    for v in [&mut idle, &mut busy, &mut iow] {
        v.sort_by(|a, b| a.total_cmp(b));
    }
    let (min_idle, median_idle, p95_idle, p95_busy, p95_iowait) = if n == 0 {
        (0.0, 0.0, 0.0, 0.0, 0.0)
    } else {
        (
            idle[0],
            percentile(&idle, 50.0),
            percentile(&idle, 95.0),
            percentile(&busy, 95.0),
            percentile(&iow, 95.0),
        )
    };
    let mem_total_mb = ticks.iter().map(|t| t.mem_total_mb).max().unwrap_or(0);
    let min_mem_avail_mb = ticks.iter().map(|t| t.mem_avail_mb).min().unwrap_or(0);
    let max_swap_used_mb = ticks.iter().map(|t| t.swap_used_mb).max().unwrap_or(0);
    let max_load1 = ticks.iter().map(|t| t.load[0]).fold(0.0, f64::max);
    let span_secs = match (ticks.first(), ticks.last()) {
        (Some(a), Some(b)) => b.ts.saturating_sub(a.ts),
        _ => 0,
    };
    let scale = f64::from(target_lanes) / f64::from(lanes.max(1));
    let total = mem_total_mb.max(1) as f64;
    let proj_busy = p95_busy * scale;
    let proj_iow = p95_iowait * scale;
    let proj_mem = 100.0 * (mem_total_mb.saturating_sub(min_mem_avail_mb) as f64) * scale / total;
    let swap_pct = 100.0 * max_swap_used_mb as f64 / total;
    let (headroom, reason) = if n < MIN_TICKS {
        (Headroom::NotMeasured, format!("{n} ticks, at least {MIN_TICKS} are needed"))
    } else {
        let mut why = Vec::new();
        if proj_busy > MAX_BUSY_PCT {
            why.push(format!("CPU busy would reach {proj_busy:.0}% (limit {MAX_BUSY_PCT:.0}%)"));
        }
        if proj_iow > MAX_IOWAIT_PCT {
            why.push(format!("iowait would reach {proj_iow:.0}% (limit {MAX_IOWAIT_PCT:.0}%)"));
        }
        if proj_mem > MAX_MEM_USED_PCT {
            why.push(format!("memory in use would reach {proj_mem:.0}% (limit {MAX_MEM_USED_PCT:.0}%)"));
        }
        if swap_pct > MAX_SWAP_PCT {
            why.push(format!("swap in use reached {swap_pct:.0}% of memory (limit {MAX_SWAP_PCT:.0}%)"));
        }
        if why.is_empty() {
            (
                Headroom::Yes,
                format!("at {target_lanes} lanes: CPU busy {proj_busy:.0}%, iowait {proj_iow:.0}%, memory in use {proj_mem:.0}%"),
            )
        } else {
            (Headroom::No, why.join("; "))
        }
    };
    Summary {
        ticks: n,
        span_secs,
        min_idle,
        median_idle,
        p95_idle,
        p95_busy,
        p95_iowait,
        max_load1,
        cores,
        min_mem_avail_mb,
        mem_total_mb,
        max_swap_used_mb,
        lanes,
        target_lanes,
        headroom,
        reason,
    }
}

impl Summary {
    /// The verdict line.
    pub fn verdict_line(&self) -> String {
        format!(
            "headroom for {} more lanes: {} ({})",
            self.target_lanes.saturating_sub(self.lanes),
            self.headroom.word(),
            self.reason
        )
    }

    /// Plain text for standard output.
    pub fn text(&self) -> String {
        format!(
            "host-sample: {} ticks over {}\n\
             host-sample: CPU idle min {:.1}%, median {:.1}%, p95 {:.1}% (p95 busy {:.1}%, p95 iowait {:.1}%)\n\
             host-sample: max load1 {:.2} on {} cores\n\
             host-sample: min MemAvailable {} MB of {} MB, max swap used {} MB\n\
             host-sample: {}\n",
            self.ticks,
            fmt_duration(Duration::from_secs(self.span_secs)),
            self.min_idle,
            self.median_idle,
            self.p95_idle,
            self.p95_busy,
            self.p95_iowait,
            self.max_load1,
            self.cores,
            self.min_mem_avail_mb,
            self.mem_total_mb,
            self.max_swap_used_mb,
            self.verdict_line()
        )
    }

    /// Markdown for `$GITHUB_STEP_SUMMARY`.
    pub fn markdown(&self) -> String {
        format!(
            "### CI host sample\n\n\
             | | |\n|---|---|\n\
             | Duration | {} |\n| Ticks | {} |\n\
             | CPU idle min / median / p95 | {:.1}% / {:.1}% / {:.1}% |\n\
             | CPU busy p95, iowait p95 | {:.1}%, {:.1}% |\n\
             | Max load1 (cores) | {:.2} ({}) |\n\
             | Min MemAvailable | {} MB of {} MB |\n\
             | Max swap used | {} MB |\n\n\
             **{}**\n\n\
             Report only: the rule is in `tools/paddock-dev/src/ci_host_sample.rs`; the lane count is changed by the owner of the runner.\n",
            fmt_duration(Duration::from_secs(self.span_secs)),
            self.ticks,
            self.min_idle,
            self.median_idle,
            self.p95_idle,
            self.p95_busy,
            self.p95_iowait,
            self.max_load1,
            self.cores,
            self.min_mem_avail_mb,
            self.mem_total_mb,
            self.max_swap_used_mb,
            self.verdict_line()
        )
    }
}

// ---- the edge ----

/// Where the three files come from.
pub trait ProcSource {
    /// The text of `/proc/<name>`.
    fn read(&self, name: &str) -> Result<String, String>;
}

/// The real `/proc`.
pub struct RealProc;

impl ProcSource for RealProc {
    fn read(&self, name: &str) -> Result<String, String> {
        std::fs::read_to_string(format!("/proc/{name}")).map_err(|e| format!("cannot read /proc/{name}: {e}"))
    }
}

fn read_all(src: &dyn ProcSource) -> Result<Reading, String> {
    parse_reading(&src.read("stat")?, &src.read("loadavg")?, &src.read("meminfo")?)
}

/// The command over the real machine.
pub fn run(args: &HostSampleArgs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let var = |k: &str| crate::process_env::vars().find(|(name, _)| name == k).map(|(_, v)| v);
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    };
    let mut http = CurlHttp;
    let mut clock = SystemClock::new();
    run_with(
        args,
        cfg!(target_os = "linux"),
        &var,
        &RealProc,
        &mut clock,
        &mut http,
        &now,
        out,
        err,
    )
}

/// The command with its outside world passed in (tests).
#[allow(clippy::too_many_arguments)]
pub fn run_with(
    args: &HostSampleArgs,
    linux: bool,
    var: &dyn Fn(&str) -> Option<String>,
    src: &dyn ProcSource,
    clock: &mut dyn Clock,
    http: &mut dyn Http,
    now: &dyn Fn() -> u64,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let fail = |err: &mut dyn Write, cause: &str, next: &str| {
        let _ = writeln!(err, "{}", error_line(cause, next));
        1
    };
    if !linux {
        return fail(
            err,
            "host-sample reads /proc and runs on Linux only.",
            "run it on the Linux runner (the host-sample workflow), not on this machine.",
        );
    }
    let max = match parse_duration(&args.max) {
        Ok(d) if !d.is_zero() => d,
        Ok(_) => return fail(err, "--max must be longer than zero.", "give a duration like 3h."),
        Err(e) => return fail(err, &format!("--max: {e}."), "give a duration like 3h."),
    };
    let interval = match parse_duration(&args.interval) {
        Ok(d) if d >= MIN_INTERVAL => d,
        Ok(_) => return fail(err, "--interval is below 1s.", "use 1s or more."),
        Err(e) => return fail(err, &format!("--interval: {e}."), "give a duration like 10s."),
    };
    if args.lanes == 0 || args.target_lanes <= args.lanes {
        return fail(
            err,
            &format!(
                "--target-lanes ({}) must be larger than --lanes ({}, at least 1).",
                args.target_lanes, args.lanes
            ),
            "pass the lanes the runner has now and the larger number to ask about.",
        );
    }
    let mut probe = match args.until_run {
        Some(run) => {
            let Some(token) = var("GITEA_TOKEN").filter(|t| !t.trim().is_empty()) else {
                return fail(
                    err,
                    "--until-run needs GITEA_TOKEN to ask the Gitea API.",
                    "set GITEA_TOKEN (the workflow passes github.token), or drop --until-run.",
                );
            };
            Some(CiRunProbe::new(http, &args.server, &args.repo, run, token.trim().to_string()))
        }
        None => None,
    };
    let mut file = match std::fs::File::create(&args.out) {
        Ok(f) => f,
        Err(e) => {
            return fail(
                err,
                &format!("cannot create {}: {e}.", args.out.display()),
                "make sure its folder exists and is writable.",
            );
        }
    };
    let mut prev = match read_all(src) {
        Ok(r) => r,
        Err(e) => {
            return fail(
                err,
                &format!("the first read failed: {e}."),
                "check that /proc is mounted; inside a container it must not be masked.",
            );
        }
    };
    let cores = prev.cores;
    let _ = writeln!(
        out,
        "host-sample: sampling every {} for at most {}{} into {}",
        fmt_duration(interval),
        fmt_duration(max),
        args.until_run.map(|r| format!(", until run {r} finishes")).unwrap_or_default(),
        args.out.display()
    );
    let mut ticks: Vec<Tick> = Vec::new();
    let mut read_errors = 0u32;
    let mut next_poll = Duration::ZERO;
    let ended = loop {
        let elapsed = clock.elapsed();
        if elapsed >= max {
            break format!("--max {} reached", fmt_duration(max));
        }
        if let Some(p) = probe.as_mut()
            && elapsed >= next_poll
        {
            next_poll = elapsed + RUN_POLL_EVERY;
            match p.poll() {
                Ok(Poll::Done(t)) | Ok(Poll::Failed(t)) => break t,
                Ok(Poll::Pending(_)) => {}
                // A failed look never stops the sampling: --max bounds it.
                Err(e) => {
                    let _ = writeln!(
                        err,
                        "::warning::host-sample: the run could not be read ({e}); sampling goes on until --max."
                    );
                }
            }
        }
        clock.sleep(interval.min(max - elapsed));
        match read_all(src) {
            Ok(cur) => {
                read_errors = 0;
                if let Some(t) = tick_between(&prev, &cur, now()) {
                    if let Err(e) = writeln!(file, "{}", t.json_line()).and_then(|()| file.flush()) {
                        return fail(
                            err,
                            &format!("cannot write {}: {e}.", args.out.display()),
                            "free space or pick another --out.",
                        );
                    }
                    ticks.push(t);
                }
                prev = cur;
            }
            Err(e) => {
                read_errors += 1;
                let _ = writeln!(err, "::warning::host-sample: read failed ({read_errors} of {MAX_READ_ERRORS}): {e}");
                if read_errors >= MAX_READ_ERRORS {
                    return fail(
                        err,
                        &format!("{MAX_READ_ERRORS} reads of /proc failed in a row: {e}."),
                        "check the host; the ticks so far are in the output file.",
                    );
                }
            }
        }
    };
    let _ = writeln!(out, "host-sample: ended: {ended}");
    let summary = summarize(&ticks, cores, args.lanes, args.target_lanes);
    let _ = write!(out, "{}", summary.text());
    if let Some(path) = var("GITHUB_STEP_SUMMARY").filter(|p| !p.is_empty()) {
        let appended = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .and_then(|mut f| f.write_all(summary.markdown().as_bytes()));
        if let Err(e) = appended {
            let _ = writeln!(err, "::warning::host-sample: cannot append to GITHUB_STEP_SUMMARY ({path}): {e}");
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    // Invented, same-shape data (ER-46): 4 cores.
    fn stat(user: u64, idle: u64, iowait: u64) -> String {
        format!(
            "cpu  {user} 0 100 {idle} {iowait} 0 10 0 0 0\ncpu0 1 0 1 1 0 0 0 0 0 0\ncpu1 1 0 1 1 0 0 0 0 0 0\ncpu2 1 0 1 1 0 0 0 0 0 0\ncpu3 1 0 1 1 0 0 0 0 0 0\nintr 5\nctxt 7\n"
        )
    }
    fn meminfo(total_mb: u64, avail_mb: u64, swap_total_mb: u64, swap_free_mb: u64) -> String {
        format!(
            "MemTotal:       {} kB\nMemFree:        1000 kB\nMemAvailable:   {} kB\nBuffers:        10 kB\nSwapTotal:      {} kB\nSwapFree:       {} kB\n",
            total_mb * 1024,
            avail_mb * 1024,
            swap_total_mb * 1024,
            swap_free_mb * 1024
        )
    }
    const LOAD: &str = "1.50 2.25 3.00 2/300 12345\n";

    fn tick(ts: u64, idle: f64, iowait: f64, avail: u64) -> Tick {
        Tick {
            ts,
            cpu_idle_pct: idle,
            iowait_pct: iowait,
            load: [1.0, 1.0, 1.0],
            mem_avail_mb: avail,
            mem_total_mb: 64_000,
            swap_used_mb: 0,
        }
    }
    fn ticks(n: usize, idle: f64, iowait: f64, avail: u64) -> Vec<Tick> {
        (0..n).map(|i| tick(1000 + 10 * i as u64, idle, iowait, avail)).collect()
    }

    #[test]
    fn parses_the_three_files() {
        let r = parse_reading(&stat(1000, 8000, 500), LOAD, &meminfo(64_000, 40_000, 2048, 2000)).unwrap();
        assert_eq!(r.cores, 4);
        assert_eq!(r.cpu.idle, 8000);
        assert_eq!(r.cpu.iowait, 500);
        assert_eq!(r.cpu.total, 1000 + 100 + 8000 + 500 + 10);
        assert_eq!(r.load, [1.5, 2.25, 3.0]);
        assert_eq!((r.mem_total_mb, r.mem_avail_mb, r.swap_used_mb), (64_000, 40_000, 48));
    }

    #[test]
    fn bad_text_is_an_error_not_a_panic() {
        assert!(parse_stat("intr 5\n").is_err());
        assert!(parse_stat("cpu  1 2 x 4 5\n").is_err());
        assert!(parse_stat("cpu  1 2\n").is_err());
        assert!(parse_loadavg("1.0 2.0\n").is_err());
        assert!(parse_loadavg("a b c\n").is_err());
        assert!(parse_meminfo("MemTotal: 5 kB\n").unwrap_err().contains("MemAvailable"));
        assert!(parse_meminfo("").is_err());
    }

    #[test]
    fn a_tick_is_the_delta_between_two_readings() {
        let a = parse_reading(&stat(1000, 8000, 500), LOAD, &meminfo(64_000, 40_000, 0, 0)).unwrap();
        // Between: +200 user, +700 idle, +100 iowait = 1000 ticks.
        let b = parse_reading(&stat(1200, 8700, 600), LOAD, &meminfo(64_000, 39_000, 0, 0)).unwrap();
        let t = tick_between(&a, &b, 7).unwrap();
        assert_eq!((t.cpu_idle_pct, t.iowait_pct, t.mem_avail_mb), (70.0, 10.0, 39_000));
        assert!(t.json_line().contains("\"cpu_idle_pct\":70.0"));
        // No advance, or a counter that went backwards (a reset), gives no tick.
        assert!(tick_between(&a, &a, 7).is_none());
        assert!(tick_between(&b, &a, 7).is_none());
    }

    #[test]
    fn percentiles_use_the_nearest_rank() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&v, 50.0), 50.0);
        assert_eq!(percentile(&v, 95.0), 95.0);
        assert_eq!(percentile(&[4.0], 95.0), 4.0);
    }

    #[test]
    fn a_calm_host_has_headroom() {
        // 3 -> 6 lanes: busy 30% x 2 = 60%, memory in use (64000-48000)/64000 = 25% x 2 = 50%.
        let s = summarize(&ticks(40, 70.0, 1.0, 48_000), 16, 3, 6);
        assert_eq!(s.headroom, Headroom::Yes, "{}", s.reason);
        assert!(s.verdict_line().starts_with("headroom for 3 more lanes: yes"));
        assert_eq!((s.min_idle, s.median_idle, s.ticks, s.span_secs), (70.0, 70.0, 40, 390));
    }

    // Positive controls (ER-33): each part of the rule alone turns the verdict to "no", and too few ticks is never a pass.
    #[test]
    fn a_busy_host_has_no_headroom() {
        let s = summarize(&ticks(40, 40.0, 1.0, 48_000), 16, 3, 6); // 60% busy x 2 = 120%
        assert_eq!(s.headroom, Headroom::No);
        assert!(s.reason.contains("CPU busy"), "{}", s.reason);
    }

    #[test]
    fn iowait_memory_and_swap_each_give_no() {
        let io = summarize(&ticks(40, 70.0, 12.0, 48_000), 16, 3, 6);
        assert!(io.headroom == Headroom::No && io.reason.contains("iowait"), "{}", io.reason);
        let mem = summarize(&ticks(40, 70.0, 1.0, 20_000), 16, 3, 6); // 44000 used x 2 > 90% of 64000
        assert!(mem.headroom == Headroom::No && mem.reason.contains("memory"), "{}", mem.reason);
        let mut t = ticks(40, 70.0, 1.0, 48_000);
        t[5].swap_used_mb = 4000;
        let sw = summarize(&t, 16, 3, 6);
        assert!(sw.headroom == Headroom::No && sw.reason.contains("swap"), "{}", sw.reason);
    }

    #[test]
    fn too_few_ticks_is_not_measured_even_when_the_host_is_idle() {
        let s = summarize(&ticks(MIN_TICKS - 1, 99.0, 0.0, 60_000), 16, 3, 6);
        assert_eq!(s.headroom, Headroom::NotMeasured);
        let none = summarize(&[], 16, 3, 6);
        assert_eq!(none.headroom, Headroom::NotMeasured);
        assert!(none.verdict_line().contains("not measured"));
        assert_eq!(summarize(&ticks(MIN_TICKS, 99.0, 0.0, 60_000), 16, 3, 6).headroom, Headroom::Yes);
    }

    #[test]
    fn the_markdown_carries_the_verdict() {
        let s = summarize(&ticks(40, 70.0, 1.0, 48_000), 16, 3, 6);
        assert!(s.markdown().contains(&s.verdict_line()));
        assert!(s.text().contains("40 ticks"));
    }

    // ---- the edge, with fakes ----

    struct FakeClock(RefCell<Duration>);
    impl Clock for FakeClock {
        fn elapsed(&self) -> Duration {
            *self.0.borrow()
        }
        fn sleep(&mut self, d: Duration) {
            *self.0.borrow_mut() += d;
        }
    }

    /// Each read of `stat` advances the counters by 1000 ticks, 25% busy.
    struct FakeProc {
        reads: RefCell<u64>,
        fail_after: Option<u64>,
    }
    impl ProcSource for FakeProc {
        fn read(&self, name: &str) -> Result<String, String> {
            match name {
                "stat" => {
                    let mut n = self.reads.borrow_mut();
                    *n += 1;
                    if self.fail_after.is_some_and(|f| *n > f) {
                        return Err("cannot read /proc/stat: gone".into());
                    }
                    Ok(stat(250 * *n, 750 * *n, 0))
                }
                "loadavg" => Ok(LOAD.into()),
                _ => Ok(meminfo(64_000, 48_000, 0, 0)),
            }
        }
    }

    struct FakeHttp(RefCell<u32>);
    impl Http for FakeHttp {
        fn get_json(&mut self, _url: &str, _token: &str) -> Result<serde_json::Value, String> {
            let mut n = self.0.borrow_mut();
            *n += 1;
            Ok(if *n >= 3 {
                json!({"status": "completed", "conclusion": "success"})
            } else {
                json!({"status": "running"})
            })
        }
    }

    fn args(extra: &[&str]) -> (tempfile::TempDir, HostSampleArgs) {
        use clap::Parser;
        #[derive(Parser)]
        struct W {
            #[command(flatten)]
            a: HostSampleArgs,
        }
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.jsonl");
        let mut v = vec!["w", "--out", out.to_str().unwrap()];
        v.extend_from_slice(extra);
        (dir, W::try_parse_from(v).unwrap().a)
    }

    fn go(a: &HostSampleArgs, linux: bool, token: bool, fail_after: Option<u64>) -> (i32, String, String, FakeClock) {
        let var = move |k: &str| (k == "GITEA_TOKEN" && token).then(|| "t0ken".to_string());
        let proc = FakeProc {
            reads: RefCell::new(0),
            fail_after,
        };
        let mut clock = FakeClock(RefCell::new(Duration::ZERO));
        let mut http = FakeHttp(RefCell::new(0));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with(a, linux, &var, &proc, &mut clock, &mut http, &|| 5000, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap(), clock)
    }

    #[test]
    fn it_samples_until_max_and_writes_one_line_per_tick() {
        let (_dir, a) = args(&["--max", "10m"]);
        let (code, out, _err, clock) = go(&a, true, false, None);
        assert_eq!(code, 0, "{out}");
        assert_eq!(clock.elapsed(), Duration::from_secs(600));
        let text = std::fs::read_to_string(&a.out).unwrap();
        assert_eq!(text.lines().count(), 60);
        assert!(text.lines().next().unwrap().contains("\"cpu_idle_pct\":75.0"));
        assert!(out.contains("60 ticks"), "{out}");
        assert!(out.contains("headroom for 3 more lanes: yes"), "{out}");
    }

    #[test]
    fn a_short_sample_says_not_measured() {
        let (code, out, _, _) = go(&args(&["--max", "2m", "--interval", "10s"]).1, true, false, None);
        assert_eq!(code, 0);
        assert!(out.contains("not measured"), "{out}");
    }

    #[test]
    fn it_stops_when_the_run_has_finished() {
        let (code, out, _, clock) = go(&args(&["--max", "3h", "--until-run", "42"]).1, true, true, None);
        assert_eq!(code, 0, "{out}");
        // Looked at 0 s, 30 s, 60 s: finished at the third look.
        assert_eq!(clock.elapsed(), Duration::from_secs(60));
        assert!(out.contains("ended: run 42 finished: success"), "{out}");
    }

    #[test]
    fn until_run_needs_the_token() {
        let (code, _, err, _) = go(&args(&["--max", "1h", "--until-run", "42"]).1, true, false, None);
        assert_eq!(code, 1);
        assert!(
            err.starts_with("::error::") && err.contains("GITEA_TOKEN") && err.contains("Next step:"),
            "{err}"
        );
    }

    #[test]
    fn other_systems_get_one_error_line() {
        let (code, out, err, _) = go(&args(&["--max", "1h"]).1, false, false, None);
        assert_eq!(code, 1);
        assert!(out.is_empty());
        assert_eq!(err.lines().count(), 1);
        assert!(err.contains("Linux only") && err.contains("Next step:"), "{err}");
    }

    #[test]
    fn bad_arguments_are_refused_with_the_next_step() {
        for extra in [
            vec!["--max", "soon"],
            vec!["--max", "1h", "--interval", "100ms"],
            vec!["--max", "1h", "--lanes", "6", "--target-lanes", "3"],
            vec!["--max", "1h", "--lanes", "0"],
        ] {
            let (code, _, err, _) = go(&args(&extra).1, true, false, None);
            assert_eq!(code, 1, "{extra:?}");
            assert!(err.contains("::error::") && err.contains("Next step:"), "{extra:?}: {err}");
        }
    }

    #[test]
    fn repeated_read_failures_end_the_sampler_but_keep_the_ticks() {
        let (_dir, a) = args(&["--max", "1h"]);
        let (code, _, err, _) = go(&a, true, false, Some(4));
        assert_eq!(code, 1);
        assert!(err.contains("::error::") && err.contains("failed in a row"), "{err}");
        assert_eq!(std::fs::read_to_string(&a.out).unwrap().lines().count(), 3);
    }
}
