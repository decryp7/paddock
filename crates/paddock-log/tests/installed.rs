//! The one process-wide install: this file has a single test so it owns the process's subscriber, `log` logger and panic
//! hook. Folders come from `paddock-env` resolved under a temporary `DEMO_HOME`; nothing touches the user's folders.

use paddock_env::{AppDirs, AppId, MapVars, Os};
use paddock_log::{CrashMarker, LogConfig, LogError, LogLevel, Logging};

#[test]
fn install_logs_log_records_and_a_worker_panic_with_a_crash_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let app = AppId::new("demo", "Demo").unwrap();
    let vars = MapVars::new().with("DEMO_HOME", tmp.path().as_os_str());
    let dirs = AppDirs::resolve(&app, Os::MacOs, &vars, None, true).unwrap();
    let marker = CrashMarker::path_for(dirs.data(), "app");

    let log = Logging::new(
        LogConfig::new(&app, "app", "1.2.3")
            .with_dirs(&dirs)
            .with_stdout(false),
    )
    .install()
    .unwrap();
    assert!(
        matches!(
            Logging::new(LogConfig::new(&app, "app", "1.2.3").with_stdout(false)).install(),
            Err(LogError::SubscriberTaken)
        ),
        "installing twice is refused"
    );

    // `log` records, with the level changed at run time.
    log::info!("hello from the log facade");
    log::debug!("hidden at Info");
    log.set_level(LogLevel::Debug);
    log::debug!("shown at Debug");

    // A panic on a worker thread, inside an IPC span.
    let worker = std::thread::Builder::new()
        .name("worker-7".into())
        .spawn(|| {
            let span = paddock_log::ipc_span("main", "op-9", "run_query");
            let _e = span.enter();
            tracing::info!(Operation = "RunQuery", "Query started");
            panic!("worker went wrong");
        })
        .unwrap();
    assert!(worker.join().is_err());

    let file = log.active_file().unwrap();
    assert!(
        file.starts_with(dirs.logs())
            && file
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("demo.")
    );
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("hello from the log facade")
            && text.contains("shown at Debug")
            && !text.contains("hidden at Info")
    );
    let fatal: serde_json::Value = text
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|l| l["@l"] == "Fatal")
        .expect("the panic is in the file");
    assert_eq!(
        (
            fatal["Thread"].as_str(),
            fatal["Window"].as_str(),
            fatal["OperationId"].as_str()
        ),
        (Some("worker-7"), Some("main"), Some("op-9"))
    );
    assert!(fatal["@m"].as_str().unwrap().contains("worker went wrong"));
    assert!(
        fatal["Backtrace"].as_str().is_some()
            && fatal["Location"].as_str().unwrap().contains("installed.rs")
    );
    assert!(
        fatal["Breadcrumbs"].as_str().unwrap().contains("RunQuery"),
        "the latest activity rides on the crash"
    );

    let m = CrashMarker::read(&marker).expect("the marker was written");
    assert_eq!((m.thread.as_str(), m.process.as_str()), ("worker-7", "app"));
    assert!(
        m.log_file
            .unwrap()
            .ends_with(file.file_name().unwrap().to_str().unwrap())
    );
    CrashMarker::clear(&marker).unwrap();
}
