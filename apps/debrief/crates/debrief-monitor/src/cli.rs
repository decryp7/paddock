//! Command line of the headless monitor:
//! `debrief-monitor --config monitor.json --out ./monitor-logs [--duration 10m]`.

use crate::source::{ProcessSource, SysinfoSource};
use crate::tracker::{base_fields, stamp, STATS_TEMPLATE};
use crate::ProcessMonitor;
use anyhow::{anyhow, bail, Context, Result};
use debrief_core::export::{ClefFileSink, EventSink};
use debrief_core::model::{Level, Record};
use debrief_core::plugin::Collector;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub const USAGE: &str = "usage: debrief-monitor --config <monitor.json> --out <dir> [--duration <10m|30s|2h|1500ms>]";
const ROTATE_BYTES: u64 = 10 * 1024 * 1024;
const KEEP_FILES: usize = 10;

/// Parses `500ms`, `30s`, `10m`, `2h` (a bare number means seconds).
pub fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    let (num, unit_ms) = if let Some(n) = s.strip_suffix("ms") {
        (n, 1)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, 1000)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60_000)
    } else if let Some(n) = s.strip_suffix('h') {
        (n, 3_600_000)
    } else {
        (s, 1000)
    };
    let n: u64 = num
        .trim()
        .parse()
        .map_err(|_| anyhow!("invalid duration \"{s}\"; use e.g. 30s, 10m or 2h"))?;
    Ok(Duration::from_millis(n.saturating_mul(unit_ms)))
}

/// Parsed command line.
#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    pub config: PathBuf,
    pub out: PathBuf,
    pub duration: Option<Duration>,
}

pub fn parse_args(args: &[String]) -> Result<Args> {
    let (mut config, mut out, mut duration) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| anyhow!("{name} needs a value\n{USAGE}"));
        match a.as_str() {
            "--config" => config = Some(PathBuf::from(value("--config")?)),
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--duration" => duration = Some(parse_duration(value("--duration")?)?),
            other => bail!("unknown argument \"{other}\"\n{USAGE}"),
        }
    }
    Ok(Args {
        config: config.ok_or_else(|| anyhow!("--config is required\n{USAGE}"))?,
        out: out.ok_or_else(|| anyhow!("--out is required\n{USAGE}"))?,
        duration,
    })
}

/// Message template of the event that ends every run.
pub const STOPPED_TEMPLATE: &str = "Monitoring stopped after {ElapsedMs} ms ({StopReason}) with {StatsEvents} stats events";

/// Passes events through to the file and counts the `Process stats` ones, for the event that ends the run.
struct CountingSink<'a> {
    inner: &'a dyn EventSink,
    stats: AtomicUsize,
}

impl EventSink for CountingSink<'_> {
    fn write(&self, records: &[Record]) -> Result<()> {
        let stats = records.iter().filter(|r| r.template.as_deref() == Some(STATS_TEMPLATE)).count();
        self.stats.fetch_add(stats, Ordering::Relaxed);
        self.inner.write(records)
    }
}

/// The event that ends a run: how long it ran, why it stopped and how many stats samples it wrote, so a short or
/// empty log says whether the run ended early or the samples never came.
fn stopped_record(elapsed: Duration, reason: &str, stats: usize) -> Record {
    let ms = elapsed.as_millis() as u64;
    let mut r = stamp(Record::new(0), crate::now_ms());
    r.level = Level::Information;
    r.message = format!("Monitoring stopped after {ms} ms ({reason}) with {stats} stats events");
    r.template = Some(STOPPED_TEMPLATE.into());
    r.fields = base_fields(&SysinfoSource::new().host_name());
    r.fields.insert("ElapsedMs".into(), ms.into());
    r.fields.insert("StopReason".into(), reason.into());
    r.fields.insert("StatsEvents".into(), (stats as u64).into());
    r
}

/// Runs the monitor until `stop` is set or the optional duration elapses, then writes the event that ends the run.
pub fn run(args: &[String], stop: &AtomicBool) -> Result<()> {
    let args = parse_args(args)?;
    let text = std::fs::read_to_string(&args.config).with_context(|| format!("cannot read {}", args.config.display()))?;
    let config: serde_json::Value = serde_json::from_str(&text).with_context(|| format!("{} is not valid JSON", args.config.display()))?;
    let file = ClefFileSink::new(&args.out, "process-monitor", ROTATE_BYTES, KEEP_FILES)?;
    let sink = CountingSink {
        inner: &file,
        stats: AtomicUsize::new(0),
    };
    let local_stop = AtomicBool::new(false);
    let timed_out = AtomicBool::new(false);
    let start = Instant::now();
    std::thread::scope(|s| {
        s.spawn(|| {
            while !stop.load(Ordering::Relaxed) && !local_stop.load(Ordering::Relaxed) {
                if args.duration.is_some_and(|d| start.elapsed() >= d) {
                    timed_out.store(true, Ordering::Relaxed);
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            local_stop.store(true, Ordering::Relaxed);
        });
        let result = ProcessMonitor.run(&config, &sink, &local_stop);
        local_stop.store(true, Ordering::Relaxed);
        result
    })?;
    let reason = if timed_out.load(Ordering::Relaxed) {
        "duration elapsed"
    } else {
        "stop requested"
    };
    file.write(&[stopped_record(start.elapsed(), reason, sink.stats.load(Ordering::Relaxed))])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("10m").unwrap(), Duration::from_secs(600));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
        assert_eq!(parse_duration("5").unwrap(), Duration::from_secs(5));
        assert!(parse_duration("soon").is_err());
    }

    #[test]
    fn arguments() {
        let a = parse_args(&v(&["--config", "c.json", "--out", "o", "--duration", "1s"])).unwrap();
        assert_eq!(
            a,
            Args {
                config: "c.json".into(),
                out: "o".into(),
                duration: Some(Duration::from_secs(1))
            }
        );
        for bad in [
            v(&[]),
            v(&["--config", "c"]),
            v(&["--out", "o"]),
            v(&["--config"]),
            v(&["--bogus"]),
            v(&["--config", "c", "--out", "o", "--duration", "x"]),
        ] {
            assert!(parse_args(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn run_reports_unreadable_and_invalid_config() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out").to_string_lossy().into_owned();
        let stop = AtomicBool::new(false);
        let missing = dir.path().join("none.json").to_string_lossy().into_owned();
        assert!(run(&v(&["--config", &missing, "--out", &out]), &stop)
            .unwrap_err()
            .to_string()
            .contains("cannot read"));
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "{nope").unwrap();
        let bad = bad.to_string_lossy().into_owned();
        assert!(run(&v(&["--config", &bad, "--out", &out]), &stop)
            .unwrap_err()
            .to_string()
            .contains("not valid JSON"));
    }

    /// A config that samples this test process every 250 ms.
    fn own_process_config(dir: &std::path::Path) -> PathBuf {
        let cfg = dir.join("m.json");
        std::fs::write(
            &cfg,
            format!(r#"{{"targets":[{{"pid":{}}}],"intervalMs":250}}"#, std::process::id()),
        )
        .unwrap();
        cfg
    }

    /// `Process stats` needs a second sample (CPU and IO are deltas), one interval after the first. A run that ends
    /// sooner writes only the start event; CI run 7287 hit this with a fixed 700 ms run on a busy runner, where the first
    /// sample came too late. So the duration test asserts only that the run stops (how many samples fit depends on the
    /// scheduler), and the events test waits for the events.
    #[test]
    fn run_stops_after_its_duration() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = own_process_config(dir.path());
        let out = dir.path().join("logs");
        let stop = AtomicBool::new(false);
        run(
            &v(&[
                "--config",
                cfg.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
                "--duration",
                "1ms",
            ]),
            &stop,
        )
        .unwrap();
        let text = std::fs::read_to_string(out.join("process-monitor.clef")).unwrap();
        assert!(text.contains(r#""StopReason":"duration elapsed""#), "{text}");
        // An external stop also ends the run.
        let stop = AtomicBool::new(true);
        run(&v(&["--config", cfg.to_str().unwrap(), "--out", out.to_str().unwrap()]), &stop).unwrap();
        let text = std::fs::read_to_string(out.join("process-monitor.clef")).unwrap();
        assert!(text.lines().last().unwrap().contains(r#""StopReason":"stop requested""#), "{text}");
    }

    /// Runs until the output holds a `Process stats` event, however long the machine takes to get there.
    #[test]
    fn run_writes_events_until_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = own_process_config(dir.path());
        let out = dir.path().join("logs");
        let file = out.join("process-monitor.clef");
        let stop = AtomicBool::new(false);
        let has_stats = || std::fs::read_to_string(&file).is_ok_and(|t| t.contains("Process stats"));
        std::thread::scope(|s| {
            s.spawn(|| {
                // Waits on the condition (the event in the file), not on a clock; stops when `run` returned early.
                while !stop.load(Ordering::Relaxed) && !has_stats() {
                    std::thread::sleep(Duration::from_millis(20));
                }
                stop.store(true, Ordering::Relaxed);
            });
            let result = run(&v(&["--config", cfg.to_str().unwrap(), "--out", out.to_str().unwrap()]), &stop);
            stop.store(true, Ordering::Relaxed);
            result
        })
        .unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("Now monitoring") && text.contains("Process stats"), "{text}");
        let end = text.lines().last().unwrap();
        assert!(
            end.contains("Monitoring stopped after") && end.contains(r#""StopReason":"stop requested""#),
            "{end}"
        );
        assert!(!end.contains(r#""StatsEvents":0"#), "{end}");
    }

    #[test]
    fn invalid_monitor_config_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("m.json");
        std::fs::write(&cfg, r#"{"targets":[]}"#).unwrap();
        let stop = AtomicBool::new(false);
        let err = run(
            &v(&["--config", cfg.to_str().unwrap(), "--out", dir.path().to_str().unwrap()]),
            &stop,
        )
        .unwrap_err();
        assert!(err.to_string().contains("at least one"));
    }
}
