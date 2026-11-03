//! Starting the daemon from the files `install` wrote: read the configuration and token (fail closed when either is
//! damaged), open the fixed endpoint and serve until stopped.

use crate::audit::AuditLog;
use crate::auth::{OsImages, Policy};
use crate::config::{load_token, HelperConfig};
use crate::dispatch::{Dispatcher, Parts};
use crate::install::{execute, uninstall_plan, SystemMachine};
use crate::paths::{endpoint_access, HelperPaths};
use crate::server::{wire, Daemon};
use dyno_core::helper_wire::{HelperInfo, HELPER_API, MAX_FRAME};
use dyno_core::network_traffic::TrafficCounter;
use dyno_core::ports::{PlatformError, ProcessTraffic};
use paddock_ipc::{FrameLimit, Host};
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

/// Opens the endpoint and serves until `stop` is set.
pub fn serve(paths: &HelperPaths, stop: Arc<AtomicBool>) -> Result<(), String> {
    let config = HelperConfig::load(&paths.config).map_err(|e| format!("The helper will not start: {e}"))?;
    let token = load_token(&paths.token).map_err(|e| format!("The helper will not start: {e}"))?;
    if let Some(dir) = &paths.endpoint_dir {
        // systemd creates it (RuntimeDirectory); elsewhere the daemon does.
        std::fs::create_dir_all(dir).map_err(|e| format!("The helper cannot create {}: {e}", dir.display()))?;
    }
    let host = Host::listen_at(
        &paths.endpoint,
        &endpoint_access(&config.user),
        token,
        FrameLimit::new(MAX_FRAME),
        wire(),
    )
    .map(|host| host.named("Dyno helper"))
    .map_err(|e| format!("The helper cannot open its endpoint {}: {e}", paths.endpoint))?;
    let host_kind = paths.host;
    let (remove_paths, stop_on_removal) = (paths.clone(), stop.clone());
    let dispatcher = Arc::new(Dispatcher::new(Parts {
        info: HelperInfo {
            version: crate::VERSION.to_string(),
            api: HELPER_API,
            host: host_kind,
            agent_exe: config.agent_exe.to_string_lossy().into_owned(),
        },
        traffic: Box::new(HelperTraffic),
        audit: AuditLog::new(paths.audit.clone()),
        clock: Box::new(now),
        on_uninstall: Box::new(move || {
            let (paths, stop) = (remove_paths.clone(), stop_on_removal.clone());
            // The caller gets its answer first; the removal runs a moment later and its last step ends this process.
            let _ = std::thread::Builder::new().name("helper-remove".into()).spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if let Ok(mut machine) = SystemMachine::new() {
                    let _ = execute(&mut machine, &uninstall_plan(&paths, &dyno_platform::elevate::windows_roots()));
                }
                stop.store(true, Ordering::SeqCst);
            });
        }),
    }));
    Daemon {
        host,
        policy: Policy {
            user: config.user,
            agent_exe: config.agent_exe,
        },
        images: Arc::new(OsImages),
        dispatcher,
        stop,
    }
    .run()
}
