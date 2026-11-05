//! A stand-in for `gitea-runner` in the service's integration tests (`tests/supervision.rs`). Never shipped.
//!
//! Behaviour comes from `fake.txt` next to the configuration file (one `key=value` per line), so parallel tests
//! don't share environment variables:
//!
//! - `mode=ok` (default): declares itself and idles; `exit` exits with `code` after declaring; `stall` logs
//!   "failed to fetch task" lines every 100 ms; `job` logs "task started" and runs a child process for `job_secs`;
//!   `silent` never declares (an update that is not accepted).
//! - `leftover`: like `job`, but the job's shell also starts background processes that outlive it (`leftovers`, a
//!   comma-separated list of `sleep:<seconds>` or program names, default `sleep:600`); `job_secs` is how long the job
//!   itself lasts. They stay in the runner's process group, as a Gradle daemon would.
//! - `job_secs=hold` (in `job` and `leftover` mode): the job lasts until the test creates a file `release` next to the
//!   configuration file, so a test decides when a job ends instead of racing a real-time job against a loaded machine.
//! - Once a job's processes run (and a `leftover` job's background processes run their own programs), it writes
//!   `job.started` next to the configuration file.
//! - `spawn_delay_ms` (in `job` and `leftover` mode): the job's process starts this long after the "task started" line,
//!   as on a loaded machine; a test that takes the line for the process sees the race every time.
//! - The labels it declares are the names under `runner.labels` in its `config.yaml` (default `x`); `declare=a,b` in
//!   `fake.txt` declares those instead (a runner whose registration disagrees with the file).
//! - `--version` prints `gitea-runner version v<version>`, where the version is the text after `FAKEVER:` at the end
//!   of the executable file (tests append it to their copy), else 5.2.0.
//! - Writes `PATH` and `CI_FLAG` from its environment to `env.txt` next to the configuration file.
//! - Ctrl+Break / SIGTERM: logs the shutdown line and exits 0 (waiting for a running job first, a held one until it is
//!   released).
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn log(msg: &str, extra: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, r#"time="2026-10-22T06:57:19+08:00" level=info msg="{msg}"{extra}"#);
    let _ = out.flush();
}

fn own_version() -> String {
    let exe = std::env::current_exe().unwrap();
    let bytes = std::fs::read(exe).unwrap_or_default();
    let tail = &bytes[bytes.len().saturating_sub(64)..];
    let text = String::from_utf8_lossy(tail);
    text.rsplit_once("FAKEVER:")
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_else(|| "5.2.0".into())
}

fn settings(config: Option<&str>) -> std::collections::HashMap<String, String> {
    let dir = config
        .and_then(|c| std::path::Path::new(c).parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| ".".into());
    std::fs::read_to_string(dir.join("fake.txt"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// The label names under `labels:` in the runner's configuration file.
fn labels_from_config(config: Option<&str>) -> Vec<String> {
    let text = config.and_then(|c| std::fs::read_to_string(c).ok()).unwrap_or_default();
    let mut out = vec![];
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("labels:") {
            inside = true;
        } else if inside && t.starts_with('-') {
            let v = t.trim_start_matches('-').trim().trim_matches(['"', '\'']);
            out.push(v.split(':').next().unwrap_or(v).to_string());
        } else if inside && !t.is_empty() && !t.starts_with('#') {
            inside = false;
        }
    }
    if out.is_empty() {
        out.push("x".into());
    }
    out
}

/// How long the job lasts, in seconds; `hold` keeps it running until the test releases it.
fn job_secs(s: &std::collections::HashMap<String, String>, default: &str) -> String {
    match s.get("job_secs").map(String::as_str) {
        Some("hold") => "100000".into(),
        Some(v) => v.into(),
        None => default.into(),
    }
}

/// Waits `spawn_delay_ms` before the job's process starts.
fn spawn_delay(s: &std::collections::HashMap<String, String>) {
    if let Some(ms) = s.get("spawn_delay_ms").and_then(|v| v.parse().ok()) {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

/// Marks the job's processes as started (`job.started` next to the configuration file): a test waits for it before
/// it lets the supervisor look, so the first look sees them.
fn job_started(config: Option<&str>) {
    if let Some(dir) = config.and_then(|c| std::path::Path::new(c).parent()) {
        std::fs::write(dir.join("job.started"), "").unwrap();
    }
}

/// Whether process `pid` runs its own program now (no longer the shell that forked it), or is gone.
#[allow(clippy::disallowed_methods, reason = "the fake runner starts the programs of its jobs")]
fn exec_done(pid: &str) -> bool {
    if pid.is_empty() {
        return true;
    }
    let out = std::process::Command::new("ps").args(["-o", "comm=", "-p", pid]).output().unwrap();
    let comm = String::from_utf8_lossy(&out.stdout);
    let comm = comm.trim();
    comm.is_empty() || !(comm == "sh" || comm.ends_with("/sh"))
}

#[allow(clippy::disallowed_methods, reason = "the fake runner starts the programs of its jobs")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = args.iter().position(|a| a == "--config").and_then(|i| args.get(i + 1)).cloned();
    match args
        .iter()
        .find(|a| !a.starts_with('-') && Some(*a) != config.as_ref())
        .map(String::as_str)
    {
        Some("generate-config") => {
            println!("log:\n  level: info\ncache:\n  enabled: true\n  dir: \"\"\nhost:\n  workdir_parent:\n");
            return;
        }
        Some("register") => {
            let token = args
                .iter()
                .position(|a| a == "--token")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_default();
            if token == "bad" {
                eprintln!("level=error msg=\"registration failed: token {token} is not valid\"");
                std::process::exit(1);
            }
            std::fs::write(".runner", "{\"fake\":true}").unwrap();
            println!("Runner registered successfully.");
            return;
        }
        Some("daemon") => {}
        _ => {
            if args.iter().any(|a| a == "--version") {
                println!("gitea-runner version v{}", own_version());
            }
            return;
        }
    }

    let s = settings(config.as_deref());
    // The environment it was started with, for tests of what the service passes (`PATH`, `CI_FLAG`).
    if let Some(dir) = config.as_deref().and_then(|c| std::path::Path::new(c).parent()) {
        let dump: String = ["PATH", "CI_FLAG"]
            .iter()
            .map(|k| format!("{k}={}\n", std::env::var(k).unwrap_or_default()))
            .collect();
        let _ = std::fs::write(dir.join("env.txt"), dump);
        let _ = std::fs::remove_file(dir.join("job.started"));
    }
    let mode = s.get("mode").cloned().unwrap_or_else(|| "ok".into());
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst)).unwrap();
    log("Starting runner daemon", "");
    if mode != "silent" {
        let labels = s
            .get("declare")
            .map(|d| d.split(',').map(str::to_string).collect())
            .unwrap_or_else(|| labels_from_config(config.as_deref()));
        log(
            &format!(
                "runner: fake, with version: v5.2.0, with labels: [{}], declare successfully",
                labels.join(" ")
            ),
            "",
        );
    }
    if mode == "exit" {
        std::thread::sleep(Duration::from_millis(200));
        std::process::exit(s.get("code").and_then(|c| c.parse().ok()).unwrap_or(3));
    }
    let mut child = None;
    if mode == "leftover" && !cfg!(windows) {
        log(
            "task started",
            " address=\"https://git.example.com\" job=build job_id=7 repository=owner/app run_id=9 run_number=3 task_id=11",
        );
        let secs = job_secs(&s, "2");
        let leftovers = s.get("leftovers").cloned().unwrap_or_else(|| "sleep:600".into());
        let mut script = String::new();
        let mut count = 0;
        for l in leftovers.split(',') {
            match l.split_once(':') {
                Some(("sleep", n)) => script.push_str(&format!("sleep {n} >/dev/null 2>&1 & echo $!; ")),
                _ => script.push_str(&format!("{l} -f /dev/null >/dev/null 2>&1 & echo $!; ")),
            }
            count += 1;
        }
        // `exec`: the job's shell becomes its last step, so ending a held job ends that one process.
        script.push_str(&format!("exec sleep {secs} >/dev/null"));
        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", &script]).stdout(std::process::Stdio::piped());
        spawn_delay(&s);
        let mut c = cmd.spawn().unwrap();
        // The leftovers' ids; each counts as started once it runs its own program (a process seen between its fork
        // and its exec keeps the shell's name in the process table).
        let mut lines = std::io::BufRead::lines(std::io::BufReader::new(c.stdout.take().unwrap()));
        for _ in 0..count {
            let pid = lines.next().and_then(Result::ok).unwrap_or_default();
            while !exec_done(pid.trim()) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        child = Some(c);
        job_started(config.as_deref());
    }
    if mode == "job" {
        log(
            "task started",
            " address=\"https://git.example.com\" job=build job_id=7 repository=owner/app run_id=9 run_number=3 task_id=11",
        );
        let secs = job_secs(&s, "3");
        let mut cmd = if cfg!(windows) {
            let mut c = std::process::Command::new("ping");
            c.args(["-n", &(secs.parse::<u32>().unwrap_or(3) + 1).to_string(), "127.0.0.1"]);
            c
        } else {
            let mut c = std::process::Command::new("sleep");
            c.arg(&secs);
            c
        };
        cmd.stdout(std::process::Stdio::null());
        spawn_delay(&s);
        child = Some(cmd.spawn().unwrap());
        job_started(config.as_deref());
    }
    let held = s.get("job_secs").is_some_and(|v| v == "hold");
    let release = config
        .as_deref()
        .and_then(|c| std::path::Path::new(c).parent())
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("release");
    let mut stopping = false;
    loop {
        if held && release.exists() {
            if let Some(mut c) = child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
        if stop.load(Ordering::SeqCst) && !stopping {
            stopping = true;
            log(
                "runner: fake shutdown initiated, waiting 0s for running jobs to complete before shutting down",
                "",
            );
        }
        if stopping && child.is_none() {
            std::process::exit(0);
        }
        if mode == "stall" {
            let _ = writeln!(
                std::io::stdout(),
                r#"time="2026-10-22T06:57:19+08:00" level=error msg="failed to fetch task" error="deadline_exceeded: context deadline exceeded""#
            );
            let _ = std::io::stdout().flush();
        }
        if let Some(c) = child.as_mut() {
            if let Ok(Some(_)) = c.try_wait() {
                child = None;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
