//! The log end to end without touching process-wide state: each test runs its own subscriber on its own thread
//! (`Logging::scoped`), a temporary folder and a fake clock.

use chrono::{Duration, TimeZone, Utc};
use log::Log;
use paddock_env::AppId;
use paddock_log::{FakeClock, LogConfig, LogLevel, LogSettings, Logging, Redactor, TracingBridge};
use serde_json::{Value, json};
use std::sync::Arc;

struct Rig {
    dir: tempfile::TempDir,
    clock: Arc<FakeClock>,
}

impl Rig {
    fn new() -> Rig {
        Rig {
            dir: tempfile::tempdir().unwrap(),
            clock: Arc::new(FakeClock::at_wall(
                Utc.with_ymd_and_hms(2026, 10, 20, 9, 0, 0).unwrap().into(),
            )),
        }
    }
    fn config(&self) -> LogConfig {
        let app = AppId::new("demo", "Demo").unwrap();
        LogConfig::new(&app, "app", "1.2.3")
            .with_log_dir(self.dir.path())
            .with_clock(self.clock.clone())
            .with_stdout(false)
            .with_commit("abc1234")
    }
    fn lines(&self) -> Vec<serde_json::Map<String, Value>> {
        let mut out = Vec::new();
        let mut files: Vec<_> = std::fs::read_dir(self.dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        files.sort();
        for f in files {
            for l in std::fs::read_to_string(f).unwrap().lines() {
                out.push(
                    serde_json::from_str::<Value>(l)
                        .unwrap()
                        .as_object()
                        .unwrap()
                        .clone(),
                );
            }
        }
        out
    }
    fn text(&self) -> String {
        let mut all = String::new();
        for e in std::fs::read_dir(self.dir.path()).unwrap() {
            all.push_str(&std::fs::read_to_string(e.unwrap().path()).unwrap());
        }
        all
    }
}

#[test]
fn spans_and_bridged_records_become_clef_properties_in_the_days_file() {
    let rig = Rig::new();
    rig.clone_run(|| {
        let span = paddock_log::ipc_span("w-1", "op-7", "run_query");
        let _e = span.enter();
        tracing::warn!(Rows = 3u64, "native {}", "event");
        let kvs: &[(&str, log::kv::Value)] = &[
            ("Sources", log::kv::Value::from(6u64)),
            ("SourceContext", log::kv::Value::from("Demo.Activity")),
        ];
        TracingBridge.log(
            &log::Record::builder()
                .level(log::Level::Info)
                .target("demo::insights")
                .key_values(&kvs)
                .args(format_args!("Workspace opened"))
                .build(),
        );
        TracingBridge.log(
            &log::Record::builder()
                .level(log::Level::Error)
                .target("hyper::client")
                .args(format_args!("boom"))
                .build(),
        );
    });
    let got = rig.lines();
    assert_eq!(got.len(), 3, "{got:?}");
    assert_eq!(
        (got[0]["@l"].as_str(), got[0]["@m"].as_str()),
        (Some("Warning"), Some("native event"))
    );
    assert_eq!(
        (
            got[0]["Window"].as_str(),
            got[0]["OperationId"].as_str(),
            got[0]["Command"].as_str()
        ),
        (Some("w-1"), Some("op-7"), Some("run_query"))
    );
    assert_eq!(
        (
            got[0]["Application"].as_str(),
            got[0]["Process"].as_str(),
            got[0]["Commit"].as_str()
        ),
        (Some("Demo"), Some("app"), Some("abc1234"))
    );
    assert_eq!(got[0]["Rows"], 3);
    assert!(!got[1].contains_key("@l"));
    assert_eq!(
        (
            got[1]["Sources"].as_i64(),
            got[1]["SourceContext"].as_str(),
            got[1]["Window"].as_str()
        ),
        (Some(6), Some("Demo.Activity"), Some("w-1"))
    );
    assert!(!got[1].contains_key("log.kv") && !got[1].contains_key("log.target"));
    assert_eq!(
        (got[2]["@l"].as_str(), got[2]["SourceContext"].as_str()),
        (Some("Error"), Some("hyper::client"))
    );
    assert!(rig.dir.path().join("demo.2026-10-20.log").is_file());
}

impl Rig {
    fn clone_run<R>(&self, f: impl FnOnce() -> R) -> R {
        Logging::new(self.config()).scoped(|_| f())
    }
}

#[test]
fn file_contents_never_reach_the_file_by_default() {
    let rig = Rig::new();
    rig.clone_run(|| {
        let secret = "contents-secret-7f3a";
        tracing::info!(
            Content = secret,
            Line = secret,
            Query = secret,
            Text = secret,
            Body = secret,
            "Opened a file"
        );
        tracing::info!(
            Token = "tok-secret-9",
            ApiKey = "key-secret-9",
            password = "pw-secret-9",
            "Signed in"
        );
        log::set_max_level(log::LevelFilter::Info);
        let kvs: &[(&str, log::kv::Value)] = &[
            ("Prompt", log::kv::Value::from("prompt-secret-1")),
            ("Rows", log::kv::Value::from(4u64)),
        ];
        TracingBridge.log(
            &log::Record::builder()
                .level(log::Level::Info)
                .target("t")
                .key_values(&kvs)
                .args(format_args!("Asked"))
                .build(),
        );
        paddock_log::log_from_webview("main", "info", &"m".repeat(10_000));
    });
    let text = rig.text();
    for secret in [
        "contents-secret-7f3a",
        "tok-secret-9",
        "key-secret-9",
        "pw-secret-9",
        "prompt-secret-1",
    ] {
        assert!(!text.contains(secret), "{secret} leaked");
    }
    assert!(
        text.contains("<omitted: 20 characters>")
            && text.contains("\"Token\":\"***\"")
            && text.contains("\"Rows\":4")
    );
    assert!(text.len() < 8_000, "a huge message is cut");
}

#[test]
fn an_app_hook_can_redact_paths() {
    let rig = Rig::new();
    let cfg = rig
        .config()
        .with_redactor(Redactor::new().with_hook(|k, v| {
            (k == "Path")
                .then(|| Some(json!(v.as_str().unwrap_or("").replace("/Users/alice", "~"))))
        }));
    Logging::new(cfg).scoped(|_| tracing::info!(Path = "/Users/alice/logs/a.log", "Opened"));
    assert!(rig.text().contains("\"Path\":\"~/logs/a.log\""));
}

#[test]
fn the_level_changes_at_run_time() {
    let rig = Rig::new();
    let cfg = rig.config().with_settings(LogSettings {
        level: LogLevel::Warn,
        ..LogSettings::default()
    });
    Logging::new(cfg).scoped(|log| {
        tracing::info!("hidden");
        tracing::warn!("shown");
        assert_eq!(log.level(), LogLevel::Warn);
        log.set_level(LogLevel::Info);
        assert_eq!(log.level(), LogLevel::Info);
        tracing::info!("now shown");
        tracing::debug!("still hidden");
    });
    let msgs: Vec<String> = rig
        .lines()
        .iter()
        .map(|l| l["@m"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(msgs, ["shown", "now shown"]);
}

#[test]
fn an_error_carries_breadcrumbs_and_repeats_are_summarised() {
    let rig = Rig::new();
    Logging::new(rig.config()).scoped(|log| {
        let span = paddock_log::ipc_span("main", "op-1", "open");
        let _e = span.enter();
        tracing::info!(Operation = "OpenFile", Rows = 2u64, "Opened");
        tracing::error!("disk exploded");
        rig.clock.advance(Duration::seconds(5).to_std().unwrap());
        tracing::error!("disk exploded");
        tracing::error!("disk exploded");
        rig.clock.advance(Duration::seconds(60).to_std().unwrap());
        tracing::error!("disk exploded");
        tracing::error!("other trouble");
        // Pending repeats are written on request (at exit).
        rig.clock.advance(Duration::seconds(1).to_std().unwrap());
        tracing::error!("other trouble");
        log.flush_repeats();
        assert_eq!(log.breadcrumbs(Some("main"), 10).len(), 1);
    });
    let lines = rig.lines();
    let msgs: Vec<&str> = lines.iter().map(|l| l["@m"].as_str().unwrap()).collect();
    assert_eq!(
        msgs,
        [
            "Opened",
            "disk exploded",
            "disk exploded (repeated 2 times in 60s)",
            "disk exploded",
            "other trouble",
            "other trouble (repeated 1 times in 60s)"
        ]
    );
    let crumbs: Vec<Value> =
        serde_json::from_str(lines[1]["Breadcrumbs"].as_str().unwrap()).unwrap();
    assert_eq!(
        (
            crumbs[0]["operation"].as_str(),
            crumbs[0]["params"]["Rows"].as_str()
        ),
        (Some("OpenFile"), Some("2"))
    );
    assert_eq!(lines[2]["Repeats"], 2);
}

#[test]
fn a_fatal_error_is_never_suppressed() {
    let rig = Rig::new();
    Logging::new(rig.config()).scoped(|_| {
        for _ in 0..3 {
            tracing::error!(Severity = "Fatal", "crashed");
        }
    });
    let lines = rig.lines();
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|l| l["@l"] == "Fatal"));
}

#[test]
fn after_the_daily_budget_only_errors_are_written_and_old_files_go() {
    let rig = Rig::new();
    let cfg = rig.config().with_settings(LogSettings {
        max_file_mb: 1,
        keep_files: 2,
        ..LogSettings::default()
    });
    Logging::new(cfg).scoped(|_| {
        let big = "x".repeat(900);
        for _ in 0..1500 {
            tracing::info!(Note = big.as_str(), "fill");
        }
        tracing::error!("an error still lands");
        tracing::info!("dropped after the limit");
    });
    let text = rig.text();
    assert!(text.contains("daily limit of 1 MB") && text.contains("an error still lands"));
    assert!(!text.contains("dropped after the limit"));
    assert!(
        text.len() < 1_200_000,
        "the file stays near the budget, was {}",
        text.len()
    );

    let rig = Rig::new();
    Logging::new(rig.config().with_settings(LogSettings {
        keep_files: 2,
        ..LogSettings::default()
    }))
    .scoped(|_| {
        for _ in 0..5 {
            tracing::info!("day");
            rig.clock.advance(Duration::days(1).to_std().unwrap());
        }
        tracing::info!("today");
    });
    assert_eq!(std::fs::read_dir(rig.dir.path()).unwrap().count(), 3);
}

#[test]
fn webview_lines_arrive_with_their_window_and_level() {
    let rig = Rig::new();
    Logging::new(rig.config()).scoped(|_| {
        paddock_log::log_from_webview("settings", "error", "it broke");
        paddock_log::log_from_webview("settings", "3", "plugin numbered info");
        paddock_log::log_from_webview("settings", "nonsense", "unknown level is info");
        paddock_log::log_from_webview("settings", "trace", "below the level");
    });
    let got = rig.lines();
    assert_eq!(got.len(), 3);
    assert_eq!(
        (
            got[0]["Window"].as_str(),
            got[0]["SourceContext"].as_str(),
            got[0]["@l"].as_str()
        ),
        (Some("settings"), Some("webview"), Some("Error"))
    );
    assert!(!got[1].contains_key("@l") && !got[2].contains_key("@l"));
}

#[test]
fn a_child_process_stream_is_logged_line_by_line_and_bounded() {
    let rig = Rig::new();
    let n = Logging::new(rig.config()).scoped(|_| {
        let mut input = Vec::new();
        input.extend_from_slice(b"first\r\nsecond\n");
        input.extend(std::iter::repeat_n(b'z', 50_000));
        input.extend_from_slice(b"\nbad \xff byte\nno newline at the end");
        paddock_log::pump_child_lines("runner", &input[..])
    });
    assert_eq!(n, 5);
    let got = rig.lines();
    assert_eq!(got.len(), 5);
    assert_eq!(
        (
            got[0]["@m"].as_str(),
            got[0]["Child"].as_str(),
            got[0]["SourceContext"].as_str()
        ),
        (Some("first"), Some("runner"), Some("child:runner"))
    );
    assert_eq!(got[2]["Truncated"], true);
    assert!(got[2]["@m"].as_str().unwrap().len() <= paddock_log::MAX_CHILD_LINE);
    assert_eq!(got[3]["@m"].as_str().unwrap(), "bad \u{fffd} byte");
    assert_eq!(got[4]["@m"], "no newline at the end");
}

#[test]
fn child_capture_runs_on_a_named_thread() {
    let rig = Rig::new();
    let n = Logging::new(rig.config()).scoped(|_| {
        // A thread has no scoped subscriber, so only the count is checked here; lines reach the file when installed.
        paddock_log::spawn_child_capture("sidecar", &b"a\nb\n"[..])
            .unwrap()
            .join()
            .unwrap()
    });
    assert_eq!(n, 2);
}

#[test]
fn an_unusable_log_folder_does_not_stop_the_app_and_is_reported() {
    let rig = Rig::new();
    std::fs::write(rig.dir.path().join("blocker"), "x").unwrap();
    let cfg = rig
        .config()
        .with_log_dir(&rig.dir.path().join("blocker").join("logs"));
    Logging::new(cfg).scoped(|log| {
        tracing::info!("nowhere to go");
        tracing::error!("not even this");
        assert!(log.file_problem().unwrap().contains("cannot be written"));
        assert!(log.active_file().unwrap().ends_with("demo.2026-10-20.log"));
    });
}

#[test]
fn a_panic_between_log_calls_does_not_stop_the_log() {
    let rig = Rig::new();
    Logging::new(rig.config()).scoped(|_| {
        let r = std::panic::catch_unwind(|| {
            tracing::info!("before");
            panic!("a panic between log calls");
        });
        assert!(r.is_err());
        tracing::info!("after");
    });
    assert_eq!(rig.lines().len(), 2);
}

#[test]
fn every_value_kind_level_and_a_later_span_record_reach_the_file() {
    let rig = Rig::new();
    let cfg = rig.config().with_settings(LogSettings {
        level: LogLevel::Trace,
        ..LogSettings::default()
    });
    Logging::new(cfg).scoped(|_| {
        tracing::trace!(
            Ratio = 0.5f64,
            Delta = -3i64,
            Count = 4u64,
            Flag = true,
            "trace event"
        );
        // A span field declared empty and filled in later reaches the events inside the span.
        let span = tracing::info_span!("work", Phase = tracing::field::Empty);
        span.record("Phase", "pack");
        let _in = span.enter();
        tracing::info!("inside");
        // Records from `log` at every level, with and without key/values, reach the same file.
        let kvs: &[(&str, log::kv::Value)] = &[("K", log::kv::Value::from(1u64))];
        for level in [
            log::Level::Error,
            log::Level::Warn,
            log::Level::Info,
            log::Level::Debug,
            log::Level::Trace,
        ] {
            assert!(TracingBridge.enabled(&log::Metadata::builder().level(level).build()));
            TracingBridge.log(
                &log::Record::builder()
                    .level(level)
                    .target("dep")
                    .key_values(&kvs)
                    .args(format_args!("kv {level}"))
                    .build(),
            );
        }
        TracingBridge.flush();
    });
    let got = rig.lines();
    assert_eq!(got[0]["@l"], "Verbose");
    assert_eq!(
        (
            got[0]["Ratio"].as_f64(),
            got[0]["Delta"].as_i64(),
            got[0]["Count"].as_u64(),
            got[0]["Flag"].as_bool()
        ),
        (Some(0.5), Some(-3), Some(4), Some(true))
    );
    assert_eq!(got[1]["Phase"], "pack");
    let levels: Vec<_> = got[2..]
        .iter()
        .map(|l| {
            l.get("@l")
                .and_then(Value::as_str)
                .unwrap_or("Information")
                .to_string()
        })
        .collect();
    assert_eq!(
        levels,
        ["Error", "Warning", "Information", "Debug", "Verbose"]
    );
}

/// A stream that is interrupted once, delivers a line and then breaks (a pipe whose child died badly).
struct Breaking(u8);

impl std::io::Read for Breaking {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0 += 1;
        match self.0 {
            1 => Err(std::io::ErrorKind::Interrupted.into()),
            2 => {
                buf[..5].copy_from_slice(b"one\nt");
                Ok(5)
            }
            _ => Err(std::io::Error::other("pipe broke")),
        }
    }
}

#[test]
fn a_child_stream_that_is_interrupted_or_breaks_keeps_what_it_delivered() {
    let rig = Rig::new();
    let n =
        Logging::new(rig.config()).scoped(|_| paddock_log::pump_child_lines("runner", Breaking(0)));
    assert_eq!(n, 2, "the whole line and the partial one before the break");
    let got = rig.lines();
    assert_eq!(
        (got[0]["@m"].as_str(), got[1]["@m"].as_str()),
        (Some("one"), Some("t"))
    );
}

#[test]
fn other_processes_write_their_own_file_and_the_handle_is_the_one_installed() {
    let rig = Rig::new();
    let app = AppId::new("demo", "Demo").unwrap();
    let cfg = LogConfig::new(&app, "server", "1.2.3")
        .with_log_dir(rig.dir.path())
        .with_clock(rig.clock.clone())
        .with_stdout(false);
    let logging = Logging::new(cfg);
    let handle = logging.handle();
    logging.scoped(|log| {
        tracing::info!("from the server");
        assert_eq!(log.active_file(), handle.active_file());
    });
    assert!(rig.dir.path().join("demo-server.2026-10-20.log").is_file());
}
