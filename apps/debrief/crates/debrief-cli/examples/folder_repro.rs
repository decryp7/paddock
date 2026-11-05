//! Opens a folder the way the app's `open_paths` job does, and logs what the job would show.
//!
//! Same path as the app below the job: a `StoreWorker` with the app's default launch values (Settings ▸ Performance,
//! `app/src-tauri/src/performance/tuning.rs`, on a machine with at least 9 cores), `RemoteStore::open`, the session's
//! progress polled and `wait`. Not run here: the annotators and the report's messages, which come after the import.
//!
//! usage: folder_repro FOLDER STORE [--store-cap-mb N] [--plugin-cap-mb N] [--parse-threads N] [--duck-mb N]
//! Environment: as `perf-import.sh` (`DEBRIEF_STORE_WORKER`, `DYLD_LIBRARY_PATH`, `DEBRIEF_PLUGINS`). Invented data only.

use debrief_core::ports::{Ingest, Query, Search};
use debrief_core::Profile;
use debrief_store_client::{MemoryMode, StoreTuning, StoreWorker, WorkerConfig};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const MB: u64 = 1 << 20;

fn arg(args: &[String], name: &str, default: u64) -> u64 {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (folder, store) = (PathBuf::from(&args[0]), PathBuf::from(&args[1]));
    let exe = debrief_cli::host::worker_binary();
    let mut cfg = WorkerConfig::new(&exe);
    cfg.memory_cap = Some(arg(&args, "--store-cap-mb", 2048) * MB);
    cfg.memory_mode = MemoryMode::Normal;
    cfg.child_log = true;
    cfg.lib_dir = std::env::var_os("DEBRIEF_LIB_DIR")
        .map(PathBuf::from)
        .or_else(|| exe.parent().map(PathBuf::from));
    cfg.tuning = StoreTuning {
        parse_threads: Some(arg(&args, "--parse-threads", 8) as usize),
        duck_threads: Some(4),
        memory_limit_mb: Some(arg(&args, "--duck-mb", 192)),
        import_memory_limit_mb: Some(768),
        index_heap_mb: Some(256),
        index_threads: Some(4),
        allocator_flush_mb: Some(32),
        allocator_background_threads: Some(true),
        plugin_memory_mb: Some(arg(&args, "--plugin-cap-mb", 1024)),
        ..StoreTuning::default()
    };
    if let Some(d) = std::env::var_os("DEBRIEF_CHILD_LOGS") {
        let _ = std::fs::create_dir_all(&d);
        debrief_ipc::child_log::set_dir(PathBuf::from(d));
    }
    let worker = StoreWorker::new(cfg).expect("the worker");
    let t0 = Instant::now();
    let opened = worker
        .open_store(&store, Profile::default_profile(), true, None)
        .expect("the store");
    println!("{:9.1} store open", t0.elapsed().as_secs_f64());
    let session = match opened.open(std::slice::from_ref(&folder)) {
        Ok(s) => s,
        Err(e) => {
            println!("{:9.1} OPEN FAILED: {e}", t0.elapsed().as_secs_f64());
            std::process::exit(2);
        }
    };
    let mut last = String::new();
    let mut last_print = Instant::now() - Duration::from_secs(60);
    while !session.is_finished() {
        let p = session.progress();
        let key = format!("{:?} {:?} finding={}", p.phase, p.stage, p.finding);
        if key != last || last_print.elapsed() > Duration::from_secs(1) {
            let name = p.current.as_deref().and_then(|c| c.rsplit('/').next()).unwrap_or("-");
            println!(
                "{:9.1} {key} files {}/{} MB {}/{} eta_ms {:?} current {name}",
                t0.elapsed().as_secs_f64(),
                p.done,
                p.total,
                p.bytes_done / MB,
                p.bytes_total / MB,
                p.eta_ms
            );
            last = key;
            last_print = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let report = session.wait();
    println!("{:9.1} session ended", t0.elapsed().as_secs_f64());
    let report = match report {
        Ok(r) => r,
        Err(e) => {
            println!("IMPORT FAILED: {e}");
            println!("worker status: {:?}", worker.status().state);
            std::process::exit(3);
        }
    };
    println!(
        "events {} bytes {} sources {} unrecognized {} notices {} elapsed_ms {}",
        report.events,
        report.bytes,
        report.sources.len(),
        report.unrecognized.len(),
        report.notices.len(),
        report.elapsed_ms
    );
    let mut reasons: BTreeMap<String, (u64, bool)> = BTreeMap::new();
    for u in &report.unrecognized {
        let e = reasons.entry(u.reason.chars().take(200).collect()).or_default();
        e.0 += 1;
        e.1 |= u.crashed;
    }
    for (r, (n, crashed)) in &reasons {
        println!("not imported x{n} crashed={crashed}: {r}");
    }
    let mut importers: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
    for s in &report.sources {
        let e = importers.entry(s.importer.as_str()).or_default();
        e.0 += 1;
        e.1 += s.records as u64;
    }
    for (i, (n, ev)) in &importers {
        println!("importer {i}: sources {n} events {ev}");
    }
    println!(
        "sources with an error: {}",
        report.sources.iter().filter(|s| s.error.is_some()).count()
    );
    println!(
        "per-file timestamp notices the app would post: {}",
        report.sources.iter().filter(|s| s.timestamp_issue.is_some()).count()
    );
    println!(
        "report as JSON: {} bytes",
        serde_json::to_vec(&report.sources).map_or(0, |v| v.len())
    );
    for n in report.notices.iter().take(5) {
        println!("notice: {n}");
    }
    let t = Instant::now();
    match opened.summary() {
        Ok(s) => println!(
            "summary {} ms total {} sources {} fields {} JSON {} bytes",
            t.elapsed().as_millis(),
            s.total,
            s.sources.len(),
            s.fields.len(),
            serde_json::to_vec(&s).map_or(0, |v| v.len())
        ),
        Err(e) => println!("SUMMARY FAILED: {e}"),
    }
    // The background work after the import: search is ready when the index caught up.
    let after = Instant::now();
    let mut last_status = String::new();
    while after.elapsed() < Duration::from_secs(40 * 60) {
        match opened.index_status() {
            Ok(st) => {
                let text = format!("{st:?}");
                if text != last_status && (last_status.is_empty() || after.elapsed().as_secs() % 15 == 0) {
                    println!("{:9.1} index {text}", t0.elapsed().as_secs_f64());
                    last_status = text;
                }
                if st.ready {
                    println!("{:9.1} index complete", t0.elapsed().as_secs_f64());
                    break;
                }
            }
            Err(e) => {
                println!("{:9.1} INDEX STATUS FAILED: {e}", t0.elapsed().as_secs_f64());
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    match worker.worker_status() {
        Ok(s) => println!("worker status: {s:?}"),
        Err(e) => println!("worker status failed: {e}"),
    }
    println!("{:9.1} done", t0.elapsed().as_secs_f64());
}
