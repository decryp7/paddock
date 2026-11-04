//! Starting the daemon from the files `install` wrote: read the configuration and token (fail closed when either is
//! damaged), open the fixed endpoint and serve until stopped.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use crate::audit::AuditLog;
use crate::dispatch::{Dispatcher, Parts};
use crate::logfile::{self, EarlySink, HelperLog};
use crate::paths::HelperPaths;
use crate::policy::{self, HelperPolicy};
use crate::refusal::StartError;
use crate::server::Daemon;
use crate::{orphan, startup};
use dyno_core::btm::LoginItemsReader;
use dyno_core::helper::HelperHost;
use dyno_core::helper_wire::{HelperInfo, StartFailure, HELPER_API};
use dyno_core::network_traffic::TrafficCounter;
use dyno_core::ports::{PlatformError, ProcessTraffic};
use paddock_env::Os;
use paddock_ipc::endpoint::{Access, Listener};
use paddock_ipc::AccountLimits;
use paddock_jobs::time::SystemClock;
use paddock_service::privileged::RealFiles;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Per-program traffic where it needs the helper (Windows), nothing elsewhere.
struct HelperTraffic;

impl ProcessTraffic for HelperTraffic {
    #[cfg(windows)]
    fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
        dyno_platform::windows::traffic::WinTraffic.counters()
    }

    #[cfg(not(windows))]
    fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
        Err(PlatformError::Unsupported(
            "Traffic per program needs no helper on this system.".into(),
        ))
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// The OS the helper runs on, in `paddock-service`'s words.
pub fn os_of(host: HelperHost) -> Os {
    match host {
        HelperHost::Windows => Os::Windows,
        HelperHost::Macos => Os::MacOs,
        HelperHost::Linux => Os::Linux,
    }
}

/// Opens the system endpoint and serves until `stop` is set. Before that it checks, by the facts of the open objects, that the
/// program, the data folder and the configuration are protected (fail closed), reads the policy file (defaults and a note
/// when it is not usable), starts the helper's log and the orphan watch. The endpoint is bound as the first instance in a
/// folder only this account can change (a squatter's earlier pipe or socket is an error, not a hijack), and clients check the
/// same before they send anything.
///
/// # Errors
/// Why the helper will not start, with the kind of refusal (the service's exit code).
pub fn serve(paths: &HelperPaths, stop: Arc<AtomicBool>) -> Result<(), StartError> {
    serve_with(paths, stop, None)
}

/// [`serve`] that also hands the lines written before the data folder is verified to `early` (the service passes the Windows
/// Event Log: it has no console, and the folder's file cannot be used yet).
///
/// # Errors
/// Why the helper will not start, with the kind of refusal. The cause is logged before it is returned.
pub fn serve_with(paths: &HelperPaths, stop: Arc<AtomicBool>, early: Option<EarlySink>) -> Result<(), StartError> {
    // Until the data folder is verified the log goes to standard error and no file in the folder is touched.
    let mut log = HelperLog::new(paths.log.clone()).deferred();
    if let Some(sink) = early {
        crate::refusal::set_event_sink(sink.clone());
        log = log.with_early_sink(sink);
    }
    let log_gate = log.install();
    crate::refusal::install_panic_hook();
    log::info!(version = crate::VERSION, host = format!("{:?}", paths.host).as_str(); "Helper: starting");
    let result = start(paths, stop, &log_gate);
    if let Err(e) = &result {
        log::error!(exit_code = e.exit_code(), reason = e.message.as_str(); "Helper: will not start");
    }
    result
}

fn start(paths: &HelperPaths, stop: Arc<AtomicBool>, log_gate: &crate::logfile::LogGate) -> Result<(), StartError> {
    let mut files = RealFiles;
    let trust = Arc::new(crate::protect::TrustGuard::system(paths.host, &paths.data_dir));
    lock_folders(&trust, paths).map_err(|m| StartError::new(StartFailure::NotProtected, m))?;
    let program = std::env::current_exe().unwrap_or_else(|_| paths.binary.clone());
    let pending = paths.config.with_extension("json.new");
    let checks = startup::verdicts(
        &mut files,
        &mut paddock_service::privileged::trust_real::RealTrust,
        &program,
        &paths.data_dir,
        &paths.config,
        &[
            ("the policy file", paths.policy.as_path()),
            ("the audit log", paths.audit.as_path()),
            ("the pending configuration", pending.as_path()),
        ],
    );
    for v in &checks {
        log::info!(what = v.what, protected = v.result.is_ok(), detail = v.result.as_ref().err().map_or("", String::as_str); "Helper: protected-location check");
    }
    startup::require_all(&checks).map_err(|m| StartError::new(StartFailure::NotProtected, m))?;
    // The data folder and what is in it are verified: the log may open its file there now.
    log_gate.arm();
    let config = startup::load_config(&mut files, &paths.config).map_err(|m| StartError::new(StartFailure::BadConfig, m))?;
    let loaded = policy::load(&mut files, &paths.policy);
    let audit = AuditLog::new(paths.audit.clone());
    // Stopped with the log before the data folder is removed, so no audited request writes after that.
    crate::logfile::register_gate(audit.gate());
    if let Some(note) = &loaded.note {
        log::info!(note = note.as_str(); "Helper: policy file");
        if loaded.policy == HelperPolicy::default() && note.contains("ignored") {
            if let Err(why) = audit.record(&dyno_core::helper_wire::AuditEntry {
                at: now(),
                peer: None,
                what: "The policy file was not used".into(),
                verdict: dyno_core::helper_wire::AuditVerdict::Refused,
                detail: note.chars().take(300).collect(),
            }) {
                log::error!(why = why.as_str(); "Helper: the audit log could not record that the policy file was not used");
            }
        }
    }
    let policy_in_force = Arc::new(Mutex::new(loaded.policy));
    let _policy_watch = watch_policy(paths, policy_in_force.clone());
    if let Some(dir) = &paths.endpoint_dir {
        // systemd creates it (RuntimeDirectory); elsewhere the daemon does, root's and closed to others.
        create_endpoint_dir(dir)
            .map_err(|e| StartError::new(StartFailure::Endpoint, format!("The helper cannot create {}: {e}", dir.display())))?;
    }
    let listener = Listener::bind_system(&paths.endpoint, &Access::system_for_all_users()).map_err(|e| {
        StartError::new(
            StartFailure::Endpoint,
            format!("The helper cannot open its endpoint {}: {e}", paths.endpoint),
        )
    })?;
    log::info!(endpoint = paths.endpoint.as_str(), owner = config.owner.to_arg().as_str(), version = crate::VERSION; "Helper: serving");
    let remove = removal(paths.clone(), stop.clone(), "an Uninstall request from the app");
    let orphan_remove = removal(paths.clone(), stop.clone(), "the app it serves has been gone for an hour");
    let _orphan = start_orphan_watch(config.app.clone(), orphan_remove);
    let log_path = paths.log.clone();
    let dispatcher = Arc::new(Dispatcher::new(Parts {
        info: HelperInfo {
            version: crate::VERSION.to_string(),
            api: HELPER_API,
            host: paths.host,
            agent_exe: String::new(),
        },
        traffic: Box::new(HelperTraffic),
        audit,
        clock: Box::new(now),
        login_items: login_items_reader(),
        holders: crate::tools::holder_source(paths.host),
        extras: crate::tools::extras_reader(paths.host),
        counters: crate::tools::counter_reader(paths.host),
        memory: Box::new(dyno_platform::helper_counters::SystemMemory),
        processes: crate::tools::process_sweeper(paths.host),
        smart: crate::tools::smart_reader(paths.host, &paths.data_dir.join("tools"), Arc::new(SystemClock), trust.clone()),
        tools: crate::tools::installer(paths.host, &paths.data_dir, Arc::new(SystemClock), trust),
        firewall: crate::tools::firewall_reader(paths.host, Arc::new(SystemClock)),
        wakes: crate::tools::wake_scheduler(paths.host),
        wol: crate::tools::wol_reader(paths.host),
        log_tail: Box::new(move |n| logfile::tail(&log_path, n)),
        on_uninstall: Box::new(move || remove()),
    }));
    Daemon {
        listener,
        owner: config.owner,
        admin_rule: crate::sys::admin_rule(),
        limits: AccountLimits::new(Arc::new(SystemClock)),
        dispatcher,
        policy: policy_in_force,
        stop,
    }
    .run()
    .map_err(|m| StartError::new(StartFailure::Other, m))
}

/// Windows: the data root (which holds `config.json`) must be protected before it is read, or the helper does not start (a
/// folder a user made is left for Repair to remove); `tools` and `staging` are locked, and emptied if a user made them.
/// Failing to lock those two only disables installing and running tools (they are checked again before every use).
fn lock_folders(trust: &crate::protect::TrustGuard, paths: &HelperPaths) -> Result<(), String> {
    use crate::protect::OnUntrusted;
    if paths.host != dyno_core::helper::HelperHost::Windows {
        return Ok(());
    }
    trust
        .ensure_root(&paths.data_dir, OnUntrusted::Refuse)
        .map_err(|e| format!("The helper will not start: {e}"))?;
    for sub in ["tools", "staging"] {
        if let Err(e) = trust.ensure_root(&paths.data_dir.join(sub), OnUntrusted::Purge) {
            log::error!(Folder = sub, Error = e.to_string().as_str(); "Helper: a folder could not be locked; tools stay off until Repair");
        }
    }
    Ok(())
}

/// Removes the helper after a moment (the caller of `Uninstall` gets its answer first; the removal ends with the stop of this
/// very job).
fn removal(paths: HelperPaths, stop: Arc<AtomicBool>, why: &'static str) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        // The log is stopped before the data folder is removed, so the reason is said first, to the log and to the event sink.
        log::warn!(why = why; "Helper: removing itself: {why}");
        crate::refusal::event(
            log::Level::Warn,
            &format!("The Dyno helper is removing itself and its service: {why}."),
        );
        let (paths, stop) = (paths.clone(), stop.clone());
        let spawned = std::thread::Builder::new().name("helper-remove".into()).spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if let Err(e) = crate::setup::uninstall_self(os_of(paths.host)) {
                log::error!(error = e.as_str(); "Helper: removing itself failed");
            }
            stop.store(true, Ordering::SeqCst);
        });
        if let Err(e) = spawned {
            log::error!(error = e.to_string().as_str(); "Helper: the removal thread could not be started; the helper stays");
        }
    })
}

#[cfg(target_os = "macos")]
fn login_items_reader() -> Box<dyn LoginItemsReader> {
    Box::new(dyno_platform::btm_reader::SfltoolReader::new(
        Box::new(dyno_platform::runner::SystemRunner),
        Arc::new(SystemClock),
    ))
}

/// Login items exist on macOS only; elsewhere the request says so.
#[cfg(not(target_os = "macos"))]
fn login_items_reader() -> Box<dyn LoginItemsReader> {
    struct NoLoginItems;
    impl LoginItemsReader for NoLoginItems {
        fn read(&self, _uid: u32) -> Result<dyno_core::btm::LoginItemsReply, PlatformError> {
            Err(PlatformError::Unsupported("Login items are a macOS reading.".into()))
        }
    }
    Box::new(NoLoginItems)
}

/// Reloads the policy when the policy file's folder changes. A watcher that cannot start leaves the policy read at start.
fn watch_policy(paths: &HelperPaths, policy: Arc<Mutex<HelperPolicy>>) -> Option<paddock_watch::WatchHandle> {
    let (path, parent) = (paths.policy.clone(), paths.policy.parent()?.to_path_buf());
    let debouncer = Arc::new(paddock_watch::Debouncer::new(
        Arc::new(SystemClock),
        std::time::Duration::from_millis(500),
        std::time::Duration::from_secs(5),
    ));
    paddock_watch::spawn(debouncer, &paddock_watch::NotifySource, &parent, false, move || {
        let loaded = policy::load(&mut RealFiles, &path);
        if let Some(note) = &loaded.note {
            log::info!(note = note.as_str(); "Helper: policy file reloaded");
        }
        *policy.lock().unwrap_or_else(|p| p.into_inner()) = loaded.policy;
    })
    .map_err(|e| log::warn!(error = e.to_string().as_str(); "Helper: the policy file is read at start only"))
    .ok()
}

/// Starts the orphan watch over the app's location, when the install recorded one.
fn start_orphan_watch(app: Option<std::path::PathBuf>, remove: Arc<dyn Fn() + Send + Sync>) -> Option<orphan::Watch> {
    let watch = orphan::Watch::new(app?, |p| p.exists(), Arc::new(SystemClock), orphan::GRACE, move || remove());
    watch.check();
    if let Err(e) = watch.watch_parent(&paddock_watch::NotifySource) {
        log::warn!(error = e.to_string().as_str(); "Helper: the app's folder is checked at start only");
    }
    Some(watch)
}

#[cfg(unix)]
fn create_endpoint_dir(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o755).create(dir)
}

#[cfg(not(unix))]
fn create_endpoint_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}
