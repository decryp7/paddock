//! Starting the daemon from the files `install` wrote: read the configuration and token (fail closed when either is
//! damaged), open the fixed endpoint and serve until stopped.

use crate::audit::AuditLog;
use crate::config::HelperConfig;
use crate::dispatch::{Dispatcher, Parts};
use crate::paths::HelperPaths;
use crate::server::Daemon;
use dyno_core::helper::HelperHost;
use dyno_core::helper_wire::{HelperInfo, HELPER_API};
use dyno_core::network_traffic::TrafficCounter;
use dyno_core::ports::{PlatformError, ProcessTraffic};
use paddock_env::Os;
use paddock_ipc::endpoint::{Access, Listener};
use paddock_ipc::AccountLimits;
use paddock_jobs::time::SystemClock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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

/// Opens the system endpoint and serves until `stop` is set. The endpoint is bound as the first instance in a folder only
/// this account can change (a squatter's earlier pipe or socket is an error, not a hijack), and clients check the same
/// before they send anything.
///
/// # Errors
/// Why the helper will not start.
pub fn serve(paths: &HelperPaths, stop: Arc<AtomicBool>) -> Result<(), String> {
    let config = HelperConfig::load(&paths.config).map_err(|e| format!("The helper will not start: {e}"))?;
    if let Some(dir) = &paths.endpoint_dir {
        // systemd creates it (RuntimeDirectory); elsewhere the daemon does, root's and closed to others.
        create_endpoint_dir(dir).map_err(|e| format!("The helper cannot create {}: {e}", dir.display()))?;
    }
    let listener = Listener::bind_system(&paths.endpoint, &Access::system_for_all_users())
        .map_err(|e| format!("The helper cannot open its endpoint {}: {e}", paths.endpoint))?;
    log::info!(endpoint = paths.endpoint.as_str(), owner = config.owner.to_arg().as_str(), version = crate::VERSION; "Helper: serving");
    let (remove_paths, stop_on_removal) = (paths.clone(), stop.clone());
    let dispatcher = Arc::new(Dispatcher::new(Parts {
        info: HelperInfo {
            version: crate::VERSION.to_string(),
            api: HELPER_API,
            host: paths.host,
            agent_exe: String::new(),
        },
        traffic: Box::new(HelperTraffic),
        audit: AuditLog::new(paths.audit.clone()),
        clock: Box::new(now),
        on_uninstall: Box::new(move || {
            let (paths, stop) = (remove_paths.clone(), stop_on_removal.clone());
            // The caller gets its answer first; the removal runs a moment later and ends with the stop of this very job.
            let _ = std::thread::Builder::new().name("helper-remove".into()).spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if let Err(e) = crate::setup::uninstall_self(os_of(paths.host)) {
                    log::error!(error = e.as_str(); "Helper: removing itself failed");
                }
                stop.store(true, Ordering::SeqCst);
            });
        }),
    }));
    Daemon {
        listener,
        owner: config.owner,
        admin_rule: crate::sys::admin_rule(),
        limits: AccountLimits::new(Arc::new(SystemClock)),
        dispatcher,
        stop,
    }
    .run()
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
