//! Starting the daemon from the files `install` wrote: read the configuration and token (fail closed when either is
//! damaged), open the fixed endpoint and serve until stopped.

use crate::audit::AuditLog;
use crate::dispatch::{Dispatcher, Parts};
use crate::logfile::{self, HelperLog};
use crate::paths::HelperPaths;
use crate::policy::{self, HelperPolicy};
use crate::server::Daemon;
use crate::{orphan, startup};
use dyno_core::btm::LoginItemsReader;
use dyno_core::helper::HelperHost;
use dyno_core::helper_wire::{HelperInfo, HELPER_API};
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
/// Why the helper will not start.
pub fn serve(paths: &HelperPaths, stop: Arc<AtomicBool>) -> Result<(), String> {
    HelperLog::new(paths.log.clone()).install();
    let mut files = RealFiles;
    let program = std::env::current_exe().unwrap_or_else(|_| paths.binary.clone());
    let checks = startup::verdicts(&mut files, &program, &paths.data_dir, &paths.config);
    for v in &checks {
        log::info!(what = v.what, protected = v.result.is_ok(), detail = v.result.as_ref().err().map_or("", String::as_str); "Helper: protected-location check");
    }
    startup::require_all(&checks)?;
    let config = startup::load_config(&mut files, &paths.config)?;
    let loaded = policy::load(&mut files, &paths.policy);
    let audit = AuditLog::new(paths.audit.clone());
    if let Some(note) = &loaded.note {
        log::info!(note = note.as_str(); "Helper: policy file");
        if loaded.policy == HelperPolicy::default() && note.contains("ignored") {
            let _ = audit.record(&dyno_core::helper_wire::AuditEntry {
                at: now(),
                peer: None,
                what: "The policy file was not used".into(),
                verdict: dyno_core::helper_wire::AuditVerdict::Refused,
                detail: note.chars().take(300).collect(),
            });
        }
    }
    let policy_in_force = Arc::new(Mutex::new(loaded.policy));
    let _policy_watch = watch_policy(paths, policy_in_force.clone());
    if let Some(dir) = &paths.endpoint_dir {
        // systemd creates it (RuntimeDirectory); elsewhere the daemon does, root's and closed to others.
        create_endpoint_dir(dir).map_err(|e| format!("The helper cannot create {}: {e}", dir.display()))?;
    }
    let listener = Listener::bind_system(&paths.endpoint, &Access::system_for_all_users())
        .map_err(|e| format!("The helper cannot open its endpoint {}: {e}", paths.endpoint))?;
    log::info!(endpoint = paths.endpoint.as_str(), owner = config.owner.to_arg().as_str(), version = crate::VERSION; "Helper: serving");
    let remove = removal(paths.clone(), stop.clone());
    let _orphan = start_orphan_watch(config.app.clone(), remove.clone());
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
        processes: crate::tools::process_sweeper(paths.host),
        smart: crate::tools::smart_reader(paths.host, &paths.data_dir.join("tools"), Arc::new(SystemClock)),
        tools: crate::tools::installer(paths.host, &paths.data_dir, Arc::new(SystemClock)),
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
}

/// Removes the helper after a moment (the caller of `Uninstall` gets its answer first; the removal ends with the stop of this
/// very job).
fn removal(paths: HelperPaths, stop: Arc<AtomicBool>) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        let (paths, stop) = (paths.clone(), stop.clone());
        let _ = std::thread::Builder::new().name("helper-remove".into()).spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if let Err(e) = crate::setup::uninstall_self(os_of(paths.host)) {
                log::error!(error = e.as_str(); "Helper: removing itself failed");
            }
            stop.store(true, Ordering::SeqCst);
        });
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
    let _ = watch
        .watch_parent(&paddock_watch::NotifySource)
        .map_err(|e| log::warn!(error = e.to_string().as_str(); "Helper: the app's folder is checked at start only"));
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
