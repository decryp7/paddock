//! `dev load`: one line that says whether the coordinator may start more agents (ER-24, ER-36).
//!
//! Three verdicts, from four measurements: the 1- and 5-minute load against the thresholds (settings, scaled to the core count
//! from the 14-core Mac they were chosen on), the free space of the d7 and d8 roots, and the CI queue (queued plus running runs,
//! read-only from the Gitea API; the token is read from `GITEA_TOKEN` and printed nowhere).
//!
//! - `stop`: the 5-minute load is at or above the hold threshold, or a disk is below the free-space floor. Start nothing; the
//!   coordinator considers pausing builders.
//! - `hold`: steady state. The 5-minute load is between the start and hold thresholds, the 1-minute load spiked over the hold
//!   threshold, the CI queue is long, or a measurement is missing. Keep the running agents, start none (ER-33: a number that was
//!   not measured never lets the verdict be `start`).
//! - `start`: everything measured and inside the limits.
//!
//! The verdict is a pure function ([`verdict`]); the readers ([`read_load`], [`free_bytes`], [`ci_queue`]) are thin.

use crate::ci_runs::client::{Client, RunInfo};
use crate::wait::CurlHttp;
use clap::Args;
use paddock_cli::{CliError, Report};
use serde_json::{Value, json};
use std::path::Path;

/// The core count the default thresholds were chosen on (a 14-core Mac, memory `agent-concurrency`).
const REFERENCE_CORES: f64 = 14.0;
/// Default start threshold on the reference machine: start agents only while the 5-minute load is under this.
const START_BELOW_REF: f64 = 12.0;
/// Default hold threshold on the reference machine: at or above this the verdict is `stop`.
const HOLD_ABOVE_REF: f64 = 20.0;
/// Default floor of free space on each root, GiB.
const MIN_FREE_GIB: u64 = 50;
/// Default number of queued plus running runs at which no agent is started.
const CI_HOLD: u64 = 4;
const GIB: u64 = 1 << 30;
const SERVER: &str = "https://dev.decryptology.net";

/// Arguments of `load`. Every threshold is a setting; without one the default is scaled to this machine's core count.
#[derive(Args)]
pub struct LoadArgs {
    /// Start agents only while the 5-minute load is under this (default: 12 on 14 cores, scaled to the core count).
    #[arg(long)]
    start_below: Option<f64>,
    /// Stop starting agents at or above this 5-minute load; a 1-minute load above it holds (default: 20 on 14 cores, scaled).
    #[arg(long)]
    hold_above: Option<f64>,
    /// The roots whose free space is checked, repeatable (default: /Volumes/d7sst7 and /Volumes/d8sst7).
    #[arg(long = "disk")]
    disks: Vec<String>,
    /// Stop below this much free space on a root, GiB.
    #[arg(long, default_value_t = MIN_FREE_GIB)]
    min_free_gib: u64,
    /// Hold when this many CI runs are queued or running.
    #[arg(long, default_value_t = CI_HOLD)]
    ci_hold: u64,
    /// Do not look at the CI queue (a declared skip, not a gap).
    #[arg(long)]
    no_ci: bool,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// The limits of the verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Start only while the 5-minute load is under this.
    pub start_below: f64,
    /// Stop at or above this 5-minute load; hold when the 1-minute load is at or above it.
    pub hold_above: f64,
    /// Stop below this much free space on a root, bytes.
    pub min_free: u64,
    /// Hold at this many queued plus running runs.
    pub ci_hold: u64,
}

impl Settings {
    /// The defaults for a machine with `cores` cores.
    pub fn for_cores(cores: usize) -> Settings {
        let scale = cores.max(1) as f64 / REFERENCE_CORES;
        Settings {
            start_below: START_BELOW_REF * scale,
            hold_above: HOLD_ABOVE_REF * scale,
            min_free: MIN_FREE_GIB * GIB,
            ci_hold: CI_HOLD,
        }
    }
}

/// Refuses thresholds that cannot mean anything: not finite (NaN, infinity), negative, or a start above the stop threshold.
pub fn validate(start_below: f64, hold_above: f64) -> Result<(), String> {
    for (name, v) in [("--start-below", start_below), ("--hold-above", hold_above)] {
        if !v.is_finite() || v < 0.0 {
            return Err(format!("{name} {v} is not a finite number of zero or more."));
        }
    }
    if start_below > hold_above {
        return Err(format!("--start-below {start_below} is above --hold-above {hold_above}."));
    }
    Ok(())
}

/// The CI queue as measured.
#[derive(Debug, Clone, PartialEq)]
pub enum CiQueue {
    /// `--no-ci`.
    Skipped,
    /// The queue could not be read; the text says why.
    NotMeasured(String),
    /// Runs in the newest pages of the run list (at most [`QUEUE_PAGES`] pages of 50).
    Counts {
        /// Runs that are executing.
        running: u64,
        /// Runs that wait for a runner.
        queued: u64,
        /// Runs whose status this tool does not know: counted as a hold, never ignored.
        unknown: u64,
    },
}

/// One root's free space.
#[derive(Debug, Clone, PartialEq)]
pub struct Disk {
    /// The path asked for.
    pub path: String,
    /// Free bytes, or why they are unknown.
    pub free: Result<u64, String>,
}

/// What was measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Inputs {
    /// Cores of this machine.
    pub cores: usize,
    /// 1- and 5-minute load, or why they are unknown.
    pub load: Result<(f64, f64), String>,
    /// The roots.
    pub disks: Vec<Disk>,
    /// The CI queue.
    pub ci: CiQueue,
}

/// The three answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Agents may be started.
    Start,
    /// Keep what runs, start nothing.
    Hold,
    /// Start nothing; consider pausing.
    Stop,
}

impl Kind {
    /// The word printed.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Start => "start",
            Kind::Hold => "hold",
            Kind::Stop => "stop",
        }
    }
}

/// The verdict and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// start, hold or stop.
    pub kind: Kind,
    /// One reason per limit that was reached or measurement that is missing (empty for `start`).
    pub reasons: Vec<String>,
}

/// The pure verdict: `stop` beats `hold` beats `start`; a missing measurement is a hold reason, never a pass (ER-33).
pub fn verdict(i: &Inputs, s: &Settings) -> Verdict {
    let mut stop = Vec::new();
    let mut hold = Vec::new();
    match &i.load {
        Err(e) => hold.push(format!("load not measured: {e}")),
        Ok((l1, l5)) => {
            if *l5 >= s.hold_above {
                stop.push(format!("5-minute load {l5:.1} is at or above {:.1}", s.hold_above));
            } else if *l5 >= s.start_below {
                hold.push(format!("5-minute load {l5:.1} is at or above {:.1}", s.start_below));
            }
            if *l1 >= s.hold_above && *l5 < s.hold_above {
                hold.push(format!("1-minute load {l1:.1} spiked to {:.1} or more", s.hold_above));
            }
        }
    }
    for d in &i.disks {
        match &d.free {
            Err(e) => hold.push(format!("free space of {} not measured: {e}", d.path)),
            Ok(f) if *f < s.min_free => stop.push(format!("{} has {} GiB free, under {} GiB", d.path, f / GIB, s.min_free / GIB)),
            Ok(_) => {}
        }
    }
    match &i.ci {
        CiQueue::Skipped => {}
        CiQueue::NotMeasured(e) => hold.push(format!("CI queue not measured: {e}")),
        CiQueue::Counts { running, queued, unknown } => {
            if running + queued >= s.ci_hold {
                hold.push(format!("CI has {running} running and {queued} queued runs (limit {})", s.ci_hold));
            }
            if *unknown > 0 {
                hold.push(format!(
                    "{unknown} CI run(s) have a status this tool does not know: the queue is not measured"
                ));
            }
        }
    }
    if !stop.is_empty() {
        stop.extend(hold);
        return Verdict {
            kind: Kind::Stop,
            reasons: stop,
        };
    }
    let kind = if hold.is_empty() { Kind::Start } else { Kind::Hold };
    Verdict { kind, reasons: hold }
}

/// The 1- and 5-minute load from `sysctl vm.loadavg` (`{ 1.2 3.4 5.6 }`) or `/proc/loadavg` (`1.2 3.4 5.6 1/200 99`).
pub fn parse_loadavg(text: &str) -> Option<(f64, f64)> {
    let mut nums = text
        .split(|c: char| c.is_whitespace() || c == '{' || c == '}')
        .filter(|t| !t.is_empty())
        .map(str::parse::<f64>);
    let l1 = nums.next()?.ok()?;
    let l5 = nums.next()?.ok()?;
    (l1.is_finite() && l5.is_finite() && l1 >= 0.0 && l5 >= 0.0).then_some((l1, l5))
}

/// Free bytes from `df -Pk <path>` output (POSIX format: the fourth column of the last line is available KiB).
pub fn parse_df(text: &str) -> Option<u64> {
    let line = text.lines().rfind(|l| !l.trim().is_empty())?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 6 {
        return None;
    }
    cols[3].parse::<u64>().ok()?.checked_mul(1024)
}

/// Counts the running, queued and unrecognised runs of a run list. A finished run is one of the API's final statuses.
pub fn count_queue(runs: &[RunInfo]) -> (u64, u64, u64) {
    let (mut running, mut queued, mut unknown) = (0, 0, 0);
    for r in runs {
        match r.status.as_str() {
            "in_progress" | "running" => running += 1,
            "queued" | "waiting" | "blocked" | "pending" => queued += 1,
            "completed" | "success" | "failure" | "cancelled" | "skipped" => {}
            _ => unknown += 1,
        }
    }
    (running, queued, unknown)
}

/// Pages of the run list read for the queue (50 runs each): the newest 200 runs. Queued and running runs are the newest ones; a
/// queue older than that is not seen, and the documentation says so.
pub const QUEUE_PAGES: u32 = 4;

/// How long `df` and `sysctl` may take: a hung network volume must not hang the verdict.
const TOOL_SECONDS: u64 = 10;

fn output_of(program: &str, args: &[&str]) -> Result<String, String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = paddock_proc::command(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start {program}: {e}"))?;
    let mut stdout = child.stdout.take().ok_or_else(|| format!("{program} has no output"))?;
    // The output is small; a reader thread keeps a full pipe from blocking the child while this thread watches the clock.
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if started.elapsed().as_secs() >= TOOL_SECONDS => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} did not answer within {TOOL_SECONDS} s"));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(e) => return Err(format!("{program} failed: {e}")),
        }
    };
    let text = reader.join().unwrap_or_default();
    if !status.success() {
        return Err(format!("{program} exited with {status}"));
    }
    Ok(text)
}

/// The 1- and 5-minute load of this machine.
pub fn read_load() -> Result<(f64, f64), String> {
    let text = match std::fs::read_to_string("/proc/loadavg") {
        Ok(t) => t,
        Err(_) => output_of("sysctl", &["-n", "vm.loadavg"])?,
    };
    parse_loadavg(&text).ok_or_else(|| format!("the load average could not be read from {:?}", text.trim()))
}

/// Free bytes on the volume of `path`.
pub fn free_bytes(path: &str) -> Result<u64, String> {
    if !Path::new(path).exists() {
        return Err("the path does not exist".into());
    }
    let text = output_of("df", &["-Pk", path])?;
    parse_df(&text).ok_or_else(|| "the df output could not be read".into())
}

/// The CI queue from the newest pages of the run list (read-only): pages are read until one has no unfinished run, at most
/// [`QUEUE_PAGES`].
pub fn ci_queue(client: &mut Client<'_>) -> Result<(u64, u64, u64), String> {
    let (mut running, mut queued, mut unknown) = (0, 0, 0);
    for page in 1..=QUEUE_PAGES {
        let runs = client.runs_page(page)?.runs;
        let (r, q, u) = count_queue(&runs);
        (running, queued, unknown) = (running + r, queued + q, unknown + u);
        if runs.len() < 50 || r + q + u == 0 {
            break;
        }
    }
    Ok((running, queued, unknown))
}

/// The text of a verdict: one line.
pub fn line(v: &Verdict, i: &Inputs, s: &Settings) -> String {
    let load = match &i.load {
        Ok((l1, l5)) => format!(
            "load {l1:.1}/{l5:.1} (1m/5m, {} cores, start under {:.0}, stop at {:.0})",
            i.cores, s.start_below, s.hold_above
        ),
        Err(_) => "load not measured".to_string(),
    };
    let disks: Vec<String> = i
        .disks
        .iter()
        .map(|d| match &d.free {
            Ok(f) => format!("{} {} GiB free", d.path, f / GIB),
            Err(_) => format!("{} not measured", d.path),
        })
        .collect();
    let ci = match &i.ci {
        CiQueue::Skipped => "ci skipped".to_string(),
        CiQueue::NotMeasured(_) => "ci not measured".to_string(),
        CiQueue::Counts {
            running,
            queued,
            unknown: 0,
        } => format!("ci {running} running {queued} queued"),
        CiQueue::Counts { running, queued, unknown } => format!("ci {running} running {queued} queued {unknown} unknown"),
    };
    let mut out = format!("{}: {load}; {}; {ci}", v.kind.word(), disks.join(", "));
    if !v.reasons.is_empty() {
        out.push_str(&format!(" [{}]", v.reasons.join("; ")));
    }
    out
}

/// The report: the line, and the measurements and verdict as JSON.
pub fn report(i: &Inputs, s: &Settings) -> Report {
    let v = verdict(i, s);
    let disks: Vec<Value> = i
        .disks
        .iter()
        .map(|d| json!({"path": d.path, "free_bytes": d.free.as_ref().ok(), "error": d.free.as_ref().err()}))
        .collect();
    let ci = match &i.ci {
        CiQueue::Skipped => json!({"status": "skipped"}),
        CiQueue::NotMeasured(e) => json!({"status": "not measured", "error": e}),
        CiQueue::Counts { running, queued, unknown } => {
            json!({"status": "measured", "running": running, "queued": queued, "unknown": unknown, "pages": QUEUE_PAGES})
        }
    };
    let data = json!({
        "verdict": v.kind.word(),
        "reasons": v.reasons,
        "cores": i.cores,
        "load": {"one": i.load.as_ref().ok().map(|l| l.0), "five": i.load.as_ref().ok().map(|l| l.1), "error": i.load.as_ref().err()},
        "disks": disks,
        "ci": ci,
        "settings": {"start_below": s.start_below, "hold_above": s.hold_above, "min_free_bytes": s.min_free, "ci_hold": s.ci_hold},
    });
    Report::new(data, line(&v, i, s))
}

/// The settings the flags give, refused when they mean nothing.
pub fn settings_of(a: &LoadArgs, cores: usize) -> Result<Settings, CliError> {
    let mut s = Settings::for_cores(cores);
    if let Some(x) = a.start_below {
        s.start_below = x;
    }
    if let Some(x) = a.hold_above {
        s.hold_above = x;
    }
    validate(s.start_below, s.hold_above)
        .map_err(|e| CliError::usage(e).with_hint("Use finite numbers with start below stop: for example 12 and 20."))?;
    s.min_free = a.min_free_gib.saturating_mul(GIB);
    s.ci_hold = a.ci_hold;
    Ok(s)
}

/// `load`, against the real machine.
pub fn command(a: &LoadArgs) -> Result<Report, CliError> {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let s = settings_of(a, cores)?;
    let roots: Vec<String> = if a.disks.is_empty() {
        vec!["/Volumes/d7sst7".into(), "/Volumes/d8sst7".into()]
    } else {
        a.disks.clone()
    };
    let ci = if a.no_ci {
        CiQueue::Skipped
    } else {
        match std::env::var("GITEA_TOKEN").ok().filter(|t| !t.is_empty()) {
            None => CiQueue::NotMeasured("GITEA_TOKEN is not set (run `source ~/.zshenv`, or pass --no-ci)".into()),
            Some(token) => {
                let mut http = CurlHttp;
                let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
                match ci_queue(&mut client) {
                    Ok((running, queued, unknown)) => CiQueue::Counts { running, queued, unknown },
                    Err(e) => CiQueue::NotMeasured(e),
                }
            }
        }
    };
    let inputs = Inputs {
        cores,
        load: read_load(),
        disks: roots
            .into_iter()
            .map(|p| {
                let free = free_bytes(&p);
                Disk { path: p, free }
            })
            .collect(),
        ci,
    };
    Ok(report(&inputs, &s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wait::Http;

    /// Answers every URL with one body.
    struct One(Value);
    impl Http for One {
        fn get_json(&mut self, _url: &str, _token: &str) -> Result<Value, String> {
            Ok(self.0.clone())
        }
    }

    fn s() -> Settings {
        Settings::for_cores(14)
    }

    fn ok_inputs() -> Inputs {
        Inputs {
            cores: 14,
            load: Ok((8.0, 9.0)),
            disks: vec![Disk {
                path: "/d7".into(),
                free: Ok(500 * GIB),
            }],
            ci: CiQueue::Counts {
                running: 1,
                queued: 0,
                unknown: 0,
            },
        }
    }

    #[test]
    fn defaults_are_12_and_20_on_fourteen_cores_and_scale_with_cores() {
        let d = s();
        assert_eq!((d.start_below, d.hold_above), (12.0, 20.0));
        assert_eq!(Settings::for_cores(7).start_below, 6.0);
    }

    #[test]
    fn everything_inside_the_limits_starts() {
        let v = verdict(&ok_inputs(), &s());
        assert_eq!(v.kind, Kind::Start);
        assert!(v.reasons.is_empty());
    }

    #[test]
    fn a_load_between_the_thresholds_holds_and_the_limits_are_exact() {
        let mut i = ok_inputs();
        i.load = Ok((13.0, 12.0));
        assert_eq!(verdict(&i, &s()).kind, Kind::Hold);
        i.load = Ok((10.0, 11.99));
        assert_eq!(verdict(&i, &s()).kind, Kind::Start);
        i.load = Ok((10.0, 20.0));
        assert_eq!(verdict(&i, &s()).kind, Kind::Stop);
    }

    #[test]
    fn a_spike_of_the_one_minute_load_holds() {
        let mut i = ok_inputs();
        i.load = Ok((27.0, 9.0));
        let v = verdict(&i, &s());
        assert_eq!(v.kind, Kind::Hold);
        assert!(v.reasons[0].contains("1-minute"), "{v:?}");
    }

    #[test]
    fn a_full_disk_stops_even_when_the_load_is_low() {
        let mut i = ok_inputs();
        i.disks[0].free = Ok(10 * GIB);
        let v = verdict(&i, &s());
        assert_eq!(v.kind, Kind::Stop);
        assert!(v.reasons[0].contains("/d7"), "{v:?}");
    }

    #[test]
    fn a_long_ci_queue_holds() {
        let mut i = ok_inputs();
        i.ci = CiQueue::Counts {
            running: 2,
            queued: 2,
            unknown: 0,
        };
        assert_eq!(verdict(&i, &s()).kind, Kind::Hold);
    }

    #[test]
    fn a_missing_measurement_never_starts() {
        let mut i = ok_inputs();
        i.ci = CiQueue::NotMeasured("no token".into());
        assert_eq!(verdict(&i, &s()).kind, Kind::Hold);
        let mut i = ok_inputs();
        i.load = Err("no sysctl".into());
        assert_eq!(verdict(&i, &s()).kind, Kind::Hold);
        let mut i = ok_inputs();
        i.disks[0].free = Err("gone".into());
        assert_eq!(verdict(&i, &s()).kind, Kind::Hold);
        // A declared skip is not a gap.
        let mut i = ok_inputs();
        i.ci = CiQueue::Skipped;
        assert_eq!(verdict(&i, &s()).kind, Kind::Start);
    }

    #[test]
    fn a_run_with_an_unknown_status_holds() {
        let mut i = ok_inputs();
        i.ci = CiQueue::Counts {
            running: 0,
            queued: 0,
            unknown: 1,
        };
        let v = verdict(&i, &s());
        assert_eq!(v.kind, Kind::Hold);
        assert!(v.reasons[0].contains("does not know"), "{v:?}");
        let (r, q, u) = count_queue(&[run("in_progress"), run("completed"), run("brand_new_status")]);
        assert_eq!((r, q, u), (1, 0, 1));
    }

    #[test]
    fn thresholds_that_are_not_finite_or_are_negative_are_refused() {
        assert!(validate(12.0, 20.0).is_ok());
        assert!(validate(0.0, 0.0).is_ok());
        for (a, b) in [
            (f64::NAN, 20.0),
            (12.0, f64::NAN),
            (f64::INFINITY, f64::INFINITY),
            (12.0, f64::INFINITY),
            (-1.0, 20.0),
            (12.0, -5.0),
        ] {
            assert!(validate(a, b).is_err(), "{a} {b}");
        }
        assert!(validate(21.0, 20.0).is_err());
    }

    #[test]
    fn the_flags_go_through_the_same_check() {
        let args = |start: Option<f64>, hold: Option<f64>| LoadArgs {
            start_below: start,
            hold_above: hold,
            disks: Vec::new(),
            min_free_gib: 50,
            ci_hold: 4,
            no_ci: true,
            repo: "example/paddock".into(),
            server: "https://gitea.invalid".into(),
        };
        assert!(settings_of(&args(Some(f64::NAN), None), 14).is_err());
        assert!(settings_of(&args(None, Some(-1.0)), 14).is_err());
        assert!(settings_of(&args(Some(25.0), None), 14).is_err());
        assert_eq!(settings_of(&args(Some(10.0), Some(18.0)), 14).unwrap().hold_above, 18.0);
    }

    #[test]
    fn the_queue_is_paged_until_a_page_has_no_unfinished_run() {
        // 50 runs on page 1 with one unfinished one: page 2 is read; it is all finished, so reading stops there.
        struct Pages(std::cell::RefCell<Vec<String>>);
        impl Http for Pages {
            fn get_json(&mut self, url: &str, _token: &str) -> Result<Value, String> {
                self.0.borrow_mut().push(url.to_string());
                let page: u32 = url.rsplit("page=").next().and_then(|p| p.parse().ok()).unwrap_or(0);
                let runs: Vec<Value> = (0..50)
                    .map(|i| run_json(if page == 1 && i == 0 { "queued" } else { "completed" }))
                    .collect();
                Ok(json!({"total_count": 500, "workflow_runs": runs}))
            }
        }
        let mut h = Pages(Default::default());
        let mut c = Client::new(&mut h, "https://gitea.invalid", "example/paddock", "t");
        assert_eq!(ci_queue(&mut c), Ok((0, 1, 0)));
        assert_eq!(h.0.borrow().len(), 2);
    }

    #[test]
    fn a_tool_that_hangs_is_cut_off_with_a_reason() {
        // `sleep 30` stands for a df on a dead network volume; the call must come back in about TOOL_SECONDS, not 30.
        let started = std::time::Instant::now();
        let e = output_of("sleep", &["30"]).unwrap_err();
        assert!(e.contains("did not answer"), "{e}");
        assert!(started.elapsed().as_secs() < 20);
    }

    #[test]
    fn stop_beats_hold_and_lists_every_reason() {
        let mut i = ok_inputs();
        i.load = Ok((30.0, 25.0));
        i.ci = CiQueue::Counts {
            running: 5,
            queued: 5,
            unknown: 0,
        };
        let v = verdict(&i, &s());
        assert_eq!(v.kind, Kind::Stop);
        assert_eq!(v.reasons.len(), 2, "{v:?}");
    }

    #[test]
    fn both_load_formats_parse() {
        assert_eq!(parse_loadavg("{ 22.26 24.70 23.47 }\n"), Some((22.26, 24.70)));
        assert_eq!(parse_loadavg("0.52 0.58 0.59 1/467 12345"), Some((0.52, 0.58)));
        assert_eq!(parse_loadavg("garbage"), None);
        assert_eq!(parse_loadavg("{ NaN 1 2 }"), None);
        assert_eq!(parse_loadavg(""), None);
    }

    #[test]
    fn df_output_parses_to_bytes() {
        let t =
            "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk9s1 3906813744 3369256228 537083648 87% /Volumes/d8sst7\n";
        assert_eq!(parse_df(t), Some(537_083_648 * 1024));
        assert_eq!(parse_df("Filesystem\n"), None);
        assert_eq!(parse_df(""), None);
    }

    fn run(status: &str) -> RunInfo {
        RunInfo {
            id: 1,
            path: "ci.yml@refs/heads/main".into(),
            event: "push".into(),
            status: status.into(),
            conclusion: String::new(),
            head_sha: "a".into(),
            created_at: String::new(),
            run_attempt: 1,
        }
    }

    fn run_json(status: &str) -> Value {
        json!({"id": 1, "path": "ci.yml@refs/heads/main", "event": "push", "status": status, "conclusion": "", "head_sha": "a", "created_at": ""})
    }

    #[test]
    fn the_queue_is_counted_from_the_run_list() {
        let mut f = One(json!({"total_count": 5, "workflow_runs": [
            run_json("in_progress"), run_json("queued"), run_json("waiting"), run_json("completed"), run_json("in_progress")
        ]}));
        let mut c = Client::new(&mut f, "https://gitea.invalid", "decryp7/paddock", "t");
        assert_eq!(ci_queue(&mut c), Ok((2, 2, 0)));
    }

    #[test]
    fn the_json_carries_the_verdict_and_the_line_leads_with_it() {
        let mut i = ok_inputs();
        i.load = Ok((27.4, 25.8));
        let r = report(&i, &s());
        assert_eq!(r.data["verdict"], "stop");
        assert_eq!(r.data["load"]["five"], 25.8);
        assert!(r.text.starts_with("stop: load 27.4/25.8"), "{}", r.text);
        assert!(!r.text.contains('\n'));
    }
}
