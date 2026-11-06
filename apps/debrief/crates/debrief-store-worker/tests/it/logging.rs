//! The worker's own log end to end (ER-27): the real worker binary writes `debrief-store.<day>.log` in the folder the app
//! names, the lines survive a kill, and no user content reaches the file at the default level. Temporary folders only.

use super::worker::{confined, kill_and_wait, TempDir};
use debrief_core::ports::Ingest;
use debrief_core::Profile;
use debrief_store_worker::protocol::{LogArgs, LogLevelArg};
use debrief_store_worker::{StoreWorker, WorkerConfig};
use serde_json::Value;
use std::path::Path;

/// The worker's newest log file in `dir` and its lines as JSON.
fn worker_lines(dir: &Path) -> Vec<Value> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_else(|_| Vec::new());
    files.retain(|p| {
        p.file_name().is_some_and(|n| {
            let n = n.to_string_lossy();
            n.starts_with("debrief-store.") && n.ends_with(".log")
        })
    });
    files.sort();
    let Some(newest) = files.last() else { return Vec::new() };
    std::fs::read_to_string(newest)
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn event<'a>(lines: &'a [Value], id: &str) -> Option<&'a Value> {
    lines.iter().find(|l| l["EventId"] == id)
}

fn logged_worker(logs: &Path, data: &Path) -> StoreWorker {
    // The folders of the worker's endpoints are confined to this test (and removed with it).
    let mut cfg: WorkerConfig = confined(env!("CARGO_BIN_EXE_debrief-store-worker"));
    cfg.log = Some(LogArgs {
        dir: logs.to_path_buf(),
        data_dir: Some(data.to_path_buf()),
        level: LogLevelArg::Info,
        max_file_mb: 10,
        keep_files: 5,
    });
    StoreWorker::new(cfg).unwrap()
}

#[test]
fn the_worker_writes_its_own_log_and_the_lines_survive_a_kill() {
    let root = TempDir::new("worker-log");
    let (logs, data, store_dir, source) = (
        root.0.join("logs"),
        root.0.join("data"),
        root.0.join("secret-project"),
        root.0.join("src"),
    );
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("secret-name.log"),
        "2026-10-14 10:00:00 INFO first record MARKER-7f3a\n2026-10-14 10:00:01 ERROR second record MARKER-7f3a\n",
    )
    .unwrap();
    // A failure path with names in it: a file that cannot be read (where the OS lets a test make one) and one that is not text.
    std::fs::write(source.join("secret-binary.log"), [0xff, 0xfe, 0x00, 0x9f, 0x00, 0xc3, 0x28]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let unreadable = source.join("secret-unreadable.log");
        std::fs::write(&unreadable, "2026-10-14 10:00:00 INFO MARKER-7f3a hidden\n").unwrap();
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    let w = logged_worker(&logs, &data);
    let store = w.open_store(&store_dir, Profile::default_profile(), true, None).unwrap();
    store.open(std::slice::from_ref(&source)).unwrap().wait().unwrap();
    let pid = w.status().pid.unwrap();

    let lines = worker_lines(&logs);
    let start = event(&lines, "store.worker.start").unwrap_or_else(|| panic!("no start line in {lines:?}"));
    assert_eq!(start["Process"], "store");
    assert_eq!(start["ProcessId"], pid);
    // The worker's parent is this test (the host): its pid where the OS tells, 0 where the worker cannot ask (Windows).
    let parent = start["ParentPid"].as_u64().unwrap();
    assert_eq!(parent, if cfg!(unix) { u64::from(std::process::id()) } else { 0 });
    assert!(event(&lines, "store.worker.ready").unwrap()["Ms"].is_u64());
    let open = event(&lines, "store.open").unwrap();
    assert!(open["Ms"].is_u64() && open["Store"].as_str().unwrap().len() == 8, "{open}");
    assert_eq!(open["ProcessId"], pid);
    let import = event(&lines, "store.import").unwrap();
    assert_eq!(import["Outcome"], "done");
    assert!(
        import["Events"].as_u64().unwrap() >= 2 && import["Sources"].as_u64().unwrap() >= 1,
        "{import}"
    );
    assert!(import["ParseMs"].is_u64() && import["Ms"].is_u64());

    // No user content at the default level: not a record's text, not the file's name, not the store's folder.
    let text = std::fs::read_dir(&logs)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("debrief-store."))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect::<String>();
    for secret in [
        "MARKER-7f3a",
        "secret-name",
        "secret-project",
        "first record",
        "secret-binary",
        "secret-unreadable",
    ] {
        assert!(!text.contains(secret), "{secret} reached the worker's log:\n{text}");
    }

    // A killed worker leaves what it wrote: every line is already on disk.
    let before = lines.len();
    kill_and_wait(&w);
    let after = worker_lines(&logs);
    assert!(after.len() >= before && event(&after, "store.open").is_some());

    // The bundle reads the worker's file next to the app's (the report's window, per process).
    let found = paddock_diag::window::log_window(&logs, "debrief", 1 << 20);
    assert!(found
        .iter()
        .any(|(name, bytes, _)| name.starts_with("debrief-store.") && String::from_utf8_lossy(bytes).contains("store.open")));
}

#[test]
fn the_start_line_names_the_memory_mode_and_the_tuning_without_a_path() {
    use debrief_store_worker::{MemoryMode, StoreTuning};
    let root = TempDir::new("worker-start");
    let (logs, data) = (root.0.join("logs"), root.0.join("data"));
    let mut cfg: WorkerConfig = confined(env!("CARGO_BIN_EXE_debrief-store-worker"));
    cfg.log = Some(LogArgs {
        dir: logs.clone(),
        data_dir: Some(data),
        level: LogLevelArg::Debug,
        max_file_mb: 10,
        keep_files: 5,
    });
    cfg.memory_mode = MemoryMode::Low;
    cfg.tuning = StoreTuning {
        parse_threads: Some(2),
        spill_dir: Some(root.0.join("secret-spill")),
        ..StoreTuning::default()
    };
    let w = StoreWorker::new(cfg).unwrap();
    w.open_store(&root.0.join("store"), Profile::default_profile(), true, None).unwrap();
    let lines = worker_lines(&logs);
    let start = event(&lines, "store.worker.start").unwrap_or_else(|| panic!("no start line in {lines:?}"));
    assert_eq!(
        (start["MemoryMode"].as_str(), start["LogLevel"].as_str()),
        (Some("low"), Some("debug"))
    );
    assert_eq!(start["Tuning"], "parseThreads=2 spillDir=set");
    assert!(start["Arch"].is_string() && start["Version"].is_string());
    assert!(!start.to_string().contains("secret-spill"));
    // At debug the worker also writes every request.
    assert!(lines.iter().any(|l| l["EventId"] == "store.request"), "{lines:?}");
}

/// A worker that cannot read its bootstrap (nothing on stdin) ends at once, and says why in its log as well as on standard error.
/// Deterministic: no host, no race with a kill (the supervisor kills a worker it stops, which leaves no time for a line).
#[test]
fn a_worker_that_cannot_connect_says_why_in_its_log_and_on_standard_error() {
    let root = TempDir::new("worker-exit");
    let (logs, data) = (root.0.join("logs"), root.0.join("data"));
    let out = debrief_core::process::quiet_command(env!("CARGO_BIN_EXE_debrief-store-worker"))
        .args(
            LogArgs {
                dir: logs.clone(),
                data_dir: Some(data),
                level: LogLevelArg::Info,
                max_file_mb: 10,
                keep_files: 5,
            }
            .args(),
        )
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("debrief-store-worker:"),
        "standard error stays the second channel"
    );
    let lines = worker_lines(&logs);
    let exit = event(&lines, "store.worker.exit").unwrap_or_else(|| panic!("no exit line in {lines:?}"));
    assert_eq!(
        (exit["Why"].as_str(), exit["Code"].as_i64(), exit["@l"].as_str()),
        (Some("connect-failed"), Some(1), Some("Error"))
    );
    assert!(
        exit["UptimeMs"].is_u64() && !exit["ErrorChain"].as_str().unwrap_or("").is_empty(),
        "{exit}"
    );
    // The start line came first, and the file is the worker's own.
    assert!(event(&lines, "store.worker.start").is_some());
    assert!(event(&lines, "store.worker.ready").is_none(), "it never connected");
}
