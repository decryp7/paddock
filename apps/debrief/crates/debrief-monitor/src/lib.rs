//! Debrief process monitor: samples processes (and, on Windows, performance counters) and
//! writes CLEF events that Debrief opens directly.
//!
//! * [`ProcessMonitor`] is a [`Collector`] (`"process-monitor"`); the tools plugin (`debrief-plugin-tools`) runs it.
//! * [`config::Config`] is the validated JSON settings, [`source::ProcessSource`] the sampling port,
//!   [`tracker::Tracker`] the pure event logic and [`cli`] the headless `debrief-monitor` tool.
//!
//! Event wording is chosen so the default profile's lifecycle markers fire:
//! `Application started: {ProcessName} ({ProcessId})` (start marker) and
//! `Application exited: {ProcessName} ({ProcessId}) after {UptimeSec}s` (stop marker).
//!
//! With `"dotnet": true`, [`dotnet`] adds .NET runtime fields (`Dotnet*`) to `Process stats`.

pub mod cli;
pub mod config;
pub mod counters;
pub mod dotnet;
#[cfg(windows)]
mod dotnet_win;
#[cfg(windows)]
mod pdh;
pub mod source;
pub mod tracker;

use anyhow::Result;
use config::Config;
use counters::{counter_records, CounterSource};
use debrief_core::export::EventSink;
use debrief_core::model::{Level, Record};
use debrief_core::plugin::Collector;
use dotnet::{DotnetSource, DotnetTracker};
use source::{ProcessSource, SysinfoSource};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracker::{base_fields, stamp, Tracker};

/// Samples the configured processes until stopped. See [`Config`] for the settings.
pub struct ProcessMonitor;

impl Collector for ProcessMonitor {
    fn id(&self) -> &str {
        "process-monitor"
    }
    fn name(&self) -> &str {
        "Process monitor"
    }
    fn run(&self, config: &serde_json::Value, sink: &dyn EventSink, stop: &AtomicBool) -> Result<()> {
        let cfg = Config::from_json(config)?;
        let mut source = SysinfoSource::new();
        let probes = Probes {
            counters: (!cfg.counters.is_empty()).then(|| counters::open(&cfg.counters)),
            dotnet: cfg.dotnet.then(dotnet::open),
        };
        run_loop(&cfg, &mut source, probes, cfg.interval(), sink, stop, &now_ms)
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn warning(host: &str, ts: i64, message: String) -> Record {
    let mut r = stamp(Record::new(0), ts);
    r.level = Level::Warning;
    r.message = message;
    r.fields = base_fields(host);
    r
}

/// Sleeps `d`, waking early when `stop` is set.
fn sleep_unless_stopped(d: Duration, stop: &AtomicBool) {
    let end = Instant::now() + d;
    while !stop.load(Ordering::Relaxed) {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(Duration::from_millis(20)));
    }
}

/// Optional measurement sources. Each is `None` when not configured, `Some(Err)` when it
/// could not be opened (reported once as a warning).
#[derive(Default)]
pub(crate) struct Probes {
    pub counters: Option<Result<Box<dyn CounterSource>>>,
    pub dotnet: Option<Result<dotnet::Opened>>,
}

/// Message template of the event that ends every run.
pub const STOPPED_TEMPLATE: &str =
    "Monitoring stopped after {ElapsedMs} ms ({StopReason}) with {Samples} samples and {StatsEvents} stats events";

/// What one run did, for the event that ends it.
#[derive(Default)]
struct Tally {
    samples: u64,
    stats: u64,
}

/// The event that ends a run: how long it ran, why it stopped and how many samples and stats events it wrote, so a
/// short or empty log says whether the run ended early, failed, or the samples never came.
fn stopped_record(host: &str, now: i64, elapsed_ms: i64, reason: &str, tally: &Tally) -> Record {
    let ms = elapsed_ms.max(0);
    let mut r = stamp(Record::new(0), now);
    r.level = if reason.starts_with("error") {
        Level::Error
    } else {
        Level::Information
    };
    r.message = format!(
        "Monitoring stopped after {ms} ms ({reason}) with {} samples and {} stats events",
        tally.samples, tally.stats
    );
    r.template = Some(STOPPED_TEMPLATE.into());
    r.fields = base_fields(host);
    r.fields.insert("ElapsedMs".into(), ms.into());
    r.fields.insert("StopReason".into(), reason.into());
    r.fields.insert("Samples".into(), tally.samples.into());
    r.fields.insert("StatsEvents".into(), tally.stats.into());
    r
}

/// The sampling loop, with every dependency injected. Every run ends with the [`STOPPED_TEMPLATE`] event, also when
/// it fails (its `StopReason` is then `error: ...`; the original error is returned even if that event cannot be
/// written).
pub(crate) fn run_loop(
    cfg: &Config,
    source: &mut dyn ProcessSource,
    probes: Probes,
    interval: Duration,
    sink: &dyn EventSink,
    stop: &AtomicBool,
    clock: &dyn Fn() -> i64,
) -> Result<()> {
    let host = source.host_name();
    let started = clock();
    let mut tally = Tally::default();
    let result = sample_until_stopped(cfg, source, &host, probes, interval, sink, stop, clock, &mut tally);
    let reason = match &result {
        Ok(()) => "stop requested".to_string(),
        Err(e) => format!("error: {e:#}"),
    };
    let now = clock();
    let end = stopped_record(&host, now, now - started, &reason, &tally);
    match result {
        Ok(()) => sink.write(&[end]),
        Err(e) => {
            let _ = sink.write(&[end]);
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sample_until_stopped(
    cfg: &Config,
    source: &mut dyn ProcessSource,
    host: &str,
    probes: Probes,
    interval: Duration,
    sink: &dyn EventSink,
    stop: &AtomicBool,
    clock: &dyn Fn() -> i64,
    tally: &mut Tally,
) -> Result<()> {
    let host = host.to_string();
    let mut tracker = Tracker::new(cfg.cpu, source.cpu_count(), &host);
    let mut counter_source = match probes.counters {
        Some(Ok(c)) => Some(c),
        Some(Err(e)) => {
            sink.write(&[warning(&host, clock(), format!("Performance counters unavailable: {e}"))])?;
            None
        }
        None => None,
    };
    let mut dotnet_source: Option<Box<dyn DotnetSource>> = match probes.dotnet {
        Some(Ok(o)) => {
            let notes: Vec<Record> = o.problems.into_iter().map(|p| warning(&host, clock(), p)).collect();
            if !notes.is_empty() {
                sink.write(&notes)?;
            }
            Some(o.source)
        }
        Some(Err(e)) => {
            sink.write(&[warning(&host, clock(), format!(".NET runtime counters unavailable: {e}"))])?;
            None
        }
        None => None,
    };
    let mut dotnet_tracker = DotnetTracker::new();
    let filter_processes = !cfg.targets.is_empty();
    while !stop.load(Ordering::Relaxed) {
        let now = clock();
        let samples = source.sample(&cfg.targets, cfg.include_children);
        let mut batch = tracker.tick(now, &samples);
        if let Some(d) = dotnet_source.as_mut() {
            match d.collect() {
                Ok(values) => {
                    let tick = dotnet_tracker.tick(now, &host, &samples, &values, &mut |pid| d.runtime(pid));
                    dotnet::enrich(&mut batch, &tick.fields);
                    batch.extend(tick.notices);
                }
                Err(e) => {
                    batch.push(warning(&host, now, format!(".NET runtime counter collection failed: {e}")));
                    dotnet_source = None;
                }
            }
        }
        if let Some(c) = counter_source.as_mut() {
            match c.collect() {
                Ok(values) => batch.extend(counter_records(
                    &values,
                    &cfg.counters,
                    filter_processes,
                    &|p| tracker.is_tracked(p),
                    now,
                    &host,
                )),
                Err(e) => {
                    batch.push(warning(&host, now, format!("Performance counter collection failed: {e}")));
                    counter_source = None;
                }
            }
        }
        tally.samples += 1;
        if !batch.is_empty() {
            sink.write(&batch)?;
            tally.stats += batch
                .iter()
                .filter(|r| r.template.as_deref() == Some(tracker::STATS_TEMPLATE))
                .count() as u64;
        }
        sleep_unless_stopped(interval, stop);
    }
    let rest = tracker.finish(clock());
    if !rest.is_empty() {
        sink.write(&rest)?;
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod dotnet_win_tests;
#[cfg(test)]
mod tests;
