//! What the app decides for all its plugins.

use crate::services::{ElevatedRunner, NoTasks, TaskObserver};
use paddock_guard::Quarantine;
use paddock_ipc::FrameLimit;
use paddock_jobs::time::{BackoffPolicy, Clock};
use paddock_plugin_api::compat::HostInfo;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Shared settings of every plugin supervisor of an app.
#[derive(Clone)]
pub struct HostConfig {
    /// The clock every limit runs on (a `FakeClock` in tests).
    pub clock: Arc<dyn Clock>,
    /// What the app tells the platform about itself: its name, the protocol and the contracts it serves.
    pub info: HostInfo,
    /// Counts faults and quarantines repeat offenders (persistent in the app, in memory in tests).
    pub quarantine: Arc<Quarantine>,
    /// The delays between restarts (retry forever by default: the quarantine decides when to stop).
    pub backoff: BackoffPolicy,
    /// How long a call may take unless its operation has a budget of its own.
    pub call_timeout: Duration,
    /// How much longer than the call's timeout and the cancel grace a call is waited for before it is abandoned.
    pub hang_grace: Duration,
    /// How long a process has to start and describe itself.
    pub start_timeout: Duration,
    /// How long a stopping process has to exit by itself before its tree is ended.
    pub stop_grace: Duration,
    /// The largest frame on a plugin connection.
    pub max_frame: FrameLimit,
    /// Where the plugins' output files land (`<dir>/<plugin id>/`).
    pub outputs_dir: PathBuf,
    /// Receives background tasks of plugins.
    pub tasks: Arc<dyn TaskObserver>,
    /// Runs elevated operations, when the app can.
    pub elevated: Option<Arc<dyn ElevatedRunner>>,
    /// Environment variables added to every plugin process on top of the few it keeps (plugins start with a clean
    /// environment). The tests of this workspace use it to keep the plugins' coverage data out of the way.
    pub child_env: Vec<(String, String)>,
}

/// The time one call has unless the app sets another (30 s): [`HostConfig::call_timeout`] and the row `plugin.call_deadline`.
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// The time a plugin has to end a call after the cancel unless the app sets another (5 s): [`HostConfig::hang_grace`] and the row
/// `plugin.hang_grace`.
pub const DEFAULT_HANG_GRACE: Duration = Duration::from_secs(5);
/// The time a plugin has to start unless the app sets another (15 s): [`HostConfig::start_timeout`] and the row `plugin.start`.
pub const DEFAULT_START_TIMEOUT: Duration = Duration::from_secs(15);

impl HostConfig {
    /// Defaults: restart with 1 s doubling to 60 s (reset after five healthy minutes), 30 s calls, a 5 s hang grace, 15 s to
    /// start, 2 s to stop, 16 MiB frames.
    pub fn new(
        info: HostInfo,
        clock: Arc<dyn Clock>,
        quarantine: Arc<Quarantine>,
        outputs_dir: impl Into<PathBuf>,
    ) -> HostConfig {
        HostConfig {
            clock,
            info,
            quarantine,
            backoff: BackoffPolicy::retry_forever(
                Duration::from_secs(1),
                Duration::from_secs(60),
                Duration::from_secs(300),
            ),
            call_timeout: DEFAULT_CALL_TIMEOUT,
            hang_grace: DEFAULT_HANG_GRACE,
            start_timeout: DEFAULT_START_TIMEOUT,
            stop_grace: Duration::from_secs(2),
            max_frame: FrameLimit::default(),
            outputs_dir: outputs_dir.into(),
            tasks: Arc::new(NoTasks),
            elevated: None,
            child_env: Vec::new(),
        }
    }
}
