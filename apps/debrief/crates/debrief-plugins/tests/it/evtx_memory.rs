//! Large Windows event logs (`.evtx`) through the Windows plugin over the real IPC, under the memory cap Debrief gives
//! a plugin by default (Settings ▸ Performance ▸ Plugin memory cap, 1024 MB) and the engine's import budget.
//!
//! The logs are generated: the MIT/Apache-2.0 sample `security.evtx` (one 64 KiB chunk, see the fixture's NOTICE)
//! repeated until the file has the size asked for. EVTX files are a 4 KiB header plus independent 64 KiB chunks, and
//! the parser counts chunks from the file size, so the result is a valid log of the same shape as a large exported
//! Security or System log (no real user data).
//!
//! `a_256_mb_event_log_imports_within_the_default_plugin_cap` is the regression test of "plugin crash when importing
//! event viewer" (docs/research/2026-10-plugins-memory-and-manager.md): it fails today because the importer holds the
//! whole file twice plus every event before the first one is sent. `measure` prints the plugin's peak memory and the
//! import time for the sizes in `DEBRIEF_EVTX_MEASURE_MB` (e.g. `16,64,128`) and does nothing without it.

use crate::common;

use debrief_core::plugin::PluginStatus;
use debrief_core::{PluginRegistry, Profile, Workspace};
use debrief_ipc::memory_watch::{MemoryEvent, MemoryListener};
use debrief_plugin_host::{Builtins, PluginLimits};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MB: u64 = 1 << 20;
const HEADER: usize = 4096;
const CHUNK: usize = 65536;
/// The cap a plugin gets when the user changed nothing (`PerformanceSettings::default().plugin_memory_mb`).
const DEFAULT_CAP_MB: u64 = 1024;

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let d = std::env::temp_dir().join(format!("debrief-evtx-memory-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    TempDir(d)
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// The sample's chunk filled up like a real one: its four events are followed by copies of the last two (which only
/// reference the templates the first two define, by offsets within the chunk, so the copies stay valid) until the
/// chunk is full, about 130 events per 64 KiB as in a real Security log. Returns the chunk and the offsets of its
/// records. Checksums are not updated: the importer does not check them (`validate_checksums(false)`).
fn dense_chunk(sample: &[u8]) -> (Vec<u8>, Vec<usize>) {
    let mut c = sample[HEADER..HEADER + CHUNK].to_vec();
    let free = u32_at(&c, 0x30) as usize;
    let mut records = Vec::new();
    let mut at = 512;
    while at < free {
        records.push(at);
        at += u32_at(&c, at + 4) as usize;
    }
    let reusable: Vec<(usize, usize)> = records[records.len() - 2..]
        .iter()
        .map(|&r| (r, u32_at(&c, r + 4) as usize))
        .collect();
    let mut end = free;
    'fill: loop {
        for &(from, len) in &reusable {
            if end + len > CHUNK {
                break 'fill;
            }
            c.copy_within(from..from + len, end);
            records.push(end);
            end += len;
        }
    }
    let last = *records.last().unwrap();
    c[0x2C..0x30].copy_from_slice(&(last as u32).to_le_bytes());
    c[0x30..0x34].copy_from_slice(&(end as u32).to_le_bytes());
    (c, records)
}

/// Writes an `.evtx` of about `mb` MiB: the sample's header, then dense chunks with record numbers and ids running on.
fn generate(dir: &Path, mb: u64) -> PathBuf {
    let sample = std::fs::read(common::fixture("debrief-plugin-windows", "security.evtx")).unwrap();
    assert_eq!(sample.len(), HEADER + CHUNK, "the sample is one header and one chunk");
    let (chunk, records) = dense_chunk(&sample);
    let chunks = ((mb * MB) as usize).div_ceil(CHUNK).max(1);
    let path = dir.join(format!("Security-{mb}MB.evtx"));
    let mut out = Vec::with_capacity(HEADER + chunks * CHUNK);
    out.extend_from_slice(&sample[..HEADER]);
    let mut id = 1u64;
    for _ in 0..chunks {
        let mut c = chunk.clone();
        let first = id;
        for &r in &records {
            c[r + 8..r + 16].copy_from_slice(&id.to_le_bytes());
            id += 1;
        }
        for (at, v) in [(0x08, first), (0x10, id - 1), (0x18, first), (0x20, id - 1)] {
            c[at..at + 8].copy_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&c);
    }
    std::fs::write(&path, &out).unwrap();
    path
}

/// What the host's memory watcher reported of the plugin process: the highest peak and whether it stopped it.
#[derive(Default)]
struct Heard {
    peak: AtomicU64,
    stopped: Mutex<Option<u64>>,
}

/// Imports `path` with the Windows plugin in a process of its own, capped at `cap_mb`.
fn import(path: &Path, cap_mb: u64) -> (Result<(), String>, u64, Option<u64>, usize, Duration) {
    let heard = Arc::new(Heard::default());
    let h = heard.clone();
    let builtins = Builtins::load(common::binary());
    builtins.set_limits(&PluginLimits {
        default_memory: Some(cap_mb * MB),
        memory_listener: Some(MemoryListener::new(move |_label, e| {
            let u = e.usage();
            h.peak.fetch_max(u.peak.max(u.used), Ordering::Relaxed);
            if let MemoryEvent::Stopped(u) = e {
                *h.stopped.lock().unwrap() = Some(u.used);
            }
        })),
        ..PluginLimits::default()
    });
    let mut r = PluginRegistry::new();
    for p in builtins.plugins() {
        if p.id() == common::WINDOWS {
            r.install_shared(p).unwrap();
        }
    }
    r.resolve();
    let mut ws = Workspace::with_registry(Profile::default_profile(), r).unwrap();
    let started = Instant::now();
    let opened = ws.open(&[path.to_path_buf()]);
    let took = started.elapsed();
    let plugin = ws.plugins().into_iter().find(|p| p.id == common::WINDOWS).unwrap();
    let records = ws.summary().sources.first().map_or(0, |s| s.records);
    let outcome = match (opened, plugin.status, plugin.failures) {
        (Ok(_), PluginStatus::Active, 0) if records > 0 => Ok(()),
        (o, _, _) => Err(format!(
            "open: {:?}; plugin: {:?}, {} failures, last error: {:?}; unrecognised reasons: {}",
            o.err().map(|e| e.to_string()),
            plugin.status,
            plugin.failures,
            plugin.last_error,
            ws.summary()
                .unrecognized
                .iter()
                .map(|u| u.reason.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        )),
    };
    drop(ws);
    builtins.shutdown();
    // The watcher looks every 200 ms and reports the end with the peak.
    std::thread::sleep(Duration::from_millis(600));
    let stopped = *heard.stopped.lock().unwrap();
    (outcome, heard.peak.load(Ordering::Relaxed), stopped, records, took)
}

#[test]
#[ignore = "fails today: the root cause of 'plugin crash when importing event viewer' (see the module docs)"]
fn a_256_mb_event_log_imports_within_the_default_plugin_cap() {
    let dir = temp("256");
    let path = generate(&dir.0, 256);
    let (outcome, peak, stopped, records, took) = import(&path, DEFAULT_CAP_MB);
    assert_eq!(stopped, None, "stopped at the {DEFAULT_CAP_MB} MB cap (peak {} MB)", peak / MB);
    assert!(outcome.is_ok(), "{outcome:?} after {took:?}");
    // Streaming bounds the plugin by its chunk pipeline, not by the file: well under half the cap.
    assert!(
        peak < DEFAULT_CAP_MB * MB / 2,
        "peak {} MB for a 256 MB log ({records} events)",
        peak / MB
    );
}

#[test]
fn a_plugin_stopped_at_its_cap_says_so_instead_of_crashed() {
    // 16 MB of events need about 210 MB in the plugin today; a 128 MB cap stops it.
    let dir = temp("cap-text");
    let path = generate(&dir.0, 16);
    let (outcome, _peak, stopped, _records, _took) = import(&path, 128);
    assert!(stopped.is_some(), "the watcher stopped the plugin at its cap");
    let error = outcome.expect_err("the import failed");
    assert!(error.contains("memory cap of 128 MB"), "the import's error names the cap: {error}");
    assert!(!error.contains("crashed"), "a stop at the cap is not a crash: {error}");
    assert!(error.contains("Plugin memory cap"), "the reason says what to do next: {error}");
}

/// Resident memory of `pid` in bytes, from `ps` (measurement only).
fn rss_of(pid: u32) -> u64 {
    let out = debrief_plugin_api::process::quiet_command("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse::<u64>().unwrap_or(0) * 1024
}

/// The resident memory of every built-in plugin's process right after Debrief warms them up (as the app does at
/// start), printed when `DEBRIEF_PLUGIN_BASELINE` is set.
#[test]
fn idle_baseline() {
    if std::env::var_os("DEBRIEF_PLUGIN_BASELINE").is_none() {
        eprintln!("skipped: set DEBRIEF_PLUGIN_BASELINE=1 to measure");
        return;
    }
    let builtins = Builtins::load(common::binary());
    builtins.warm_up();
    let started = Instant::now();
    while builtins.processes().iter().any(|p| p.status().pid.is_none()) && started.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(2));
    let mut total = 0;
    for p in builtins.processes() {
        let rss = p.status().pid.map_or(0, rss_of);
        total += rss;
        eprintln!(
            "idle {:<36} pid {:>6?} rss {:>5.1} MB",
            p.id(),
            p.status().pid,
            rss as f64 / MB as f64
        );
    }
    eprintln!(
        "idle total: {} processes, {:.1} MB",
        builtins.processes().len(),
        total as f64 / MB as f64
    );
    builtins.shutdown();
}

#[test]
fn measure() {
    let Ok(sizes) = std::env::var("DEBRIEF_EVTX_MEASURE_MB") else {
        eprintln!("skipped: set DEBRIEF_EVTX_MEASURE_MB=16,64 to measure");
        return;
    };
    let cap: u64 = std::env::var("DEBRIEF_EVTX_MEASURE_CAP_MB")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(4096);
    let dir = temp("measure");
    for mb in sizes.split(',').filter_map(|s| s.trim().parse::<u64>().ok()) {
        let path = generate(&dir.0, mb);
        let (outcome, peak, stopped, records, took) = import(&path, cap);
        eprintln!(
            "evtx {mb:>5} MB: cap {cap} MB, peak {:>5} MB, stopped {:?}, {records} events, {:.1} s, {}",
            peak / MB,
            stopped.map(|u| u / MB),
            took.as_secs_f64(),
            match outcome {
                Ok(()) => "ok".to_string(),
                Err(e) => e,
            }
        );
        let _ = std::fs::remove_file(&path);
    }
}
