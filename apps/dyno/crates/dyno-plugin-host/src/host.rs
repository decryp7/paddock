//! The plugin host: one slot per plugin with its connection and failure history.
//!
//! Plugins start lazily (on the first call) or with [`PluginHost::start_all`]. A failed plugin is restarted on a later
//! call once its backoff has passed; a quarantined or stopped one is skipped. A slot's lock is held while its plugin
//! starts, never during a call, so a slow scan does not hold up that plugin's sensors.

use crate::clock::{Clock, SystemClock};
use crate::config::HostConfig;
use crate::discovery::{discover, Discovered, Invalid};
use crate::error::{ErrorKind, HostError};
use crate::launcher::{Launcher, PluginConnection, ProcessLauncher};
use crate::supervision::{Supervision, Verdict};
use dyno_plugin_api::protocol::codes;
use dyno_plugin_api::{Contribution, PluginReply, PluginRequest, SensorReading, API_VERSION};
use paddock_ipc::IpcError;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// At most this many sensor readings are taken from one plugin per call.
const MAX_SENSORS: usize = 256;

/// The state of a plugin, as the UI shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// Started and answering.
    Running,
    /// Not started yet, or stopped.
    Stopped,
    /// Failed; it is restarted on a later call after its backoff (or never, if its folder is invalid).
    Failed,
    /// Failed too often; waits for the user to clear it.
    Quarantined,
}

/// One row of the plugin list (the UI's `PluginInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginStatus {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// Its state.
    pub state: PluginState,
    /// The last failure, while it matters.
    pub error: Option<String>,
    /// What it contributes.
    pub contributes: Vec<Contribution>,
}

/// One plugin's answer to a question asked of every plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct FromPlugin<T> {
    /// The plugin's id.
    pub plugin: String,
    /// Its answer or its failure.
    pub result: Result<T, HostError>,
}

struct Slot {
    name: String,
    version: String,
    contributes: Vec<Contribution>,
    state: PluginState,
    error: Option<String>,
    conn: Option<Arc<dyn PluginConnection>>,
    supervision: Supervision,
    /// Stopped by [`PluginHost::stop_all`]: not started lazily.
    held: bool,
}

struct Entry {
    id: String,
    /// `None` for an invalid folder (never started).
    plugin: Option<Discovered>,
    slot: Mutex<Slot>,
}

impl Entry {
    fn lock(&self) -> MutexGuard<'_, Slot> {
        // A slot is plain data, consistent after a panic elsewhere.
        self.slot.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn same(a: &Arc<dyn PluginConnection>, b: &Arc<dyn PluginConnection>) -> bool {
    std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b))
}

/// Runs the plugins of one folder. Dropping it stops them.
pub struct PluginHost {
    config: HostConfig,
    launcher: Arc<dyn Launcher>,
    clock: Arc<dyn Clock>,
    entries: Vec<Entry>,
}

impl PluginHost {
    /// The plugins in `config.plugins_dir`, started as real processes. Nothing starts yet.
    pub fn new(config: HostConfig) -> PluginHost {
        PluginHost::with(config, Arc::new(ProcessLauncher), Arc::new(SystemClock))
    }

    /// [`PluginHost::new`] with another launcher and clock.
    pub fn with(config: HostConfig, launcher: Arc<dyn Launcher>, clock: Arc<dyn Clock>) -> PluginHost {
        let (found, invalid) = discover(Path::new(&config.plugins_dir));
        PluginHost::with_plugins(config, found, invalid, launcher, clock)
    }

    /// A host for plugins found elsewhere (tests, or a caller that discovers itself).
    pub fn with_plugins(
        config: HostConfig,
        plugins: Vec<Discovered>,
        invalid: Vec<Invalid>,
        launcher: Arc<dyn Launcher>,
        clock: Arc<dyn Clock>,
    ) -> PluginHost {
        let mut entries: Vec<Entry> = plugins
            .into_iter()
            .map(|p| Entry {
                id: p.id.clone(),
                slot: Mutex::new(Slot {
                    name: p.name.clone(),
                    version: p.version.clone(),
                    contributes: p.contributes.clone(),
                    state: PluginState::Stopped,
                    error: None,
                    conn: None,
                    supervision: Supervision::default(),
                    held: false,
                }),
                plugin: Some(p),
            })
            .collect();
        entries.extend(invalid.into_iter().map(|i| Entry {
            id: i.folder.clone(),
            plugin: None,
            slot: Mutex::new(Slot {
                name: i.folder,
                version: String::new(),
                contributes: Vec::new(),
                state: PluginState::Failed,
                error: Some(i.error),
                conn: None,
                supervision: Supervision::default(),
                held: true,
            }),
        }));
        PluginHost {
            config,
            launcher,
            clock,
            entries,
        }
    }

    /// Starts every plugin that is not running, quarantined or invalid, in parallel. Returns the failures.
    pub fn start_all(&self) -> Vec<HostError> {
        let targets: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| {
                let mut s = e.lock();
                s.held = e.plugin.is_none();
                e.plugin.is_some() && !s.supervision.is_quarantined()
            })
            .collect();
        std::thread::scope(|scope| {
            let handles: Vec<_> = targets.iter().map(|e| (e, scope.spawn(|| self.ensure(e)))).collect();
            handles
                .into_iter()
                .filter_map(|(e, h)| match h.join() {
                    Ok(r) => r.err(),
                    Err(_) => Some(HostError::new(&e.id, ErrorKind::Crashed, "starting it failed unexpectedly")),
                })
                .collect()
        })
    }

    /// Every plugin and its state, valid or not.
    pub fn list(&self) -> Vec<PluginStatus> {
        self.entries
            .iter()
            .map(|e| {
                let s = e.lock();
                PluginStatus {
                    id: e.id.clone(),
                    name: s.name.clone(),
                    version: s.version.clone(),
                    state: s.state,
                    error: s.error.clone(),
                    contributes: s.contributes.clone(),
                }
            })
            .collect()
    }

    /// Readings of every sensor plugin, asked in parallel. Non-finite values are dropped.
    pub fn sensors(&self) -> Vec<FromPlugin<Vec<SensorReading>>> {
        self.gather(Contribution::Sensors, |e| self.readings(e))
    }

    /// The readings of one plugin (the same call as [`PluginHost::sensors`], for the plugin `plugin` only).
    pub fn sensors_of(&self, plugin: &str) -> Result<Vec<SensorReading>, HostError> {
        self.readings(self.entry(plugin)?)
    }

    fn readings(&self, e: &Entry) -> Result<Vec<SensorReading>, HostError> {
        match self.call(e, PluginRequest::Sensors, self.config.timeouts.sensors)? {
            PluginReply::Sensors(mut s) => {
                s.retain(|r| r.value.is_finite());
                s.truncate(MAX_SENSORS);
                Ok(s)
            }
            other => Err(self.wrong_reply(e, &other)),
        }
    }

    /// Clears a quarantine (the user asked) and starts the plugin again.
    pub fn unquarantine(&self, id: &str) -> Result<(), HostError> {
        let e = self.entry(id)?;
        {
            let mut s = e.lock();
            s.supervision.clear();
            s.held = false;
            if s.state == PluginState::Quarantined {
                s.state = PluginState::Stopped;
                s.error = None;
            }
        }
        self.ensure(e).map(drop)
    }

    /// Stops every plugin (closes its connection, then kills it after the grace period). Stopped plugins are not
    /// started again until [`PluginHost::start_all`] or [`PluginHost::unquarantine`].
    pub fn stop_all(&self) {
        let grace = self.config.timeouts.stop_grace;
        std::thread::scope(|scope| {
            for e in &self.entries {
                let conn = {
                    let mut s = e.lock();
                    s.held = true;
                    if s.state == PluginState::Running {
                        s.state = PluginState::Stopped;
                    }
                    s.conn.take()
                };
                if let Some(conn) = conn {
                    scope.spawn(move || conn.stop(grace));
                }
            }
        });
    }

    fn entry(&self, id: &str) -> Result<&Entry, HostError> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| HostError::new(id, ErrorKind::NotFound, "no such plugin"))
    }

    /// Asks every plugin contributing `c` that may run now, in parallel.
    fn gather<T: Send>(&self, c: Contribution, ask: impl Fn(&Entry) -> Result<T, HostError> + Sync) -> Vec<FromPlugin<T>> {
        let now = self.clock.now();
        let targets: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| {
                let s = e.lock();
                e.plugin.is_some() && !s.held && s.contributes.contains(&c) && (s.conn.is_some() || s.supervision.may_start(now))
            })
            .collect();
        let ask = &ask;
        std::thread::scope(|scope| {
            let handles: Vec<_> = targets.iter().map(|e| (e, scope.spawn(move || ask(e)))).collect();
            handles
                .into_iter()
                .map(|(e, h)| FromPlugin {
                    plugin: e.id.clone(),
                    result: h
                        .join()
                        .unwrap_or_else(|_| Err(HostError::new(&e.id, ErrorKind::Crashed, "asking it failed unexpectedly"))),
                })
                .collect()
        })
    }

    /// The running connection, starting the plugin if it may start.
    fn ensure(&self, e: &Entry) -> Result<Arc<dyn PluginConnection>, HostError> {
        let mut s = e.lock();
        if let Some(conn) = &s.conn {
            return Ok(conn.clone());
        }
        let Some(plugin) = &e.plugin else {
            return Err(HostError::new(&e.id, ErrorKind::Invalid, s.error.clone().unwrap_or_default()));
        };
        if s.supervision.is_quarantined() {
            return Err(HostError::new(
                &e.id,
                ErrorKind::Quarantined,
                format!("quarantined: {}", s.error.clone().unwrap_or_default()),
            ));
        }
        if s.held {
            return Err(HostError::new(&e.id, ErrorKind::NotRunning, "stopped"));
        }
        let now = self.clock.now();
        if !s.supervision.may_start(now) {
            let wait = s
                .supervision
                .retry_at()
                .map(|t| t.saturating_duration_since(now))
                .unwrap_or_default();
            return Err(HostError::new(
                &e.id,
                ErrorKind::NotRunning,
                format!(
                    "restarting in {} s after: {}",
                    wait.as_secs().max(1),
                    s.error.clone().unwrap_or_default()
                ),
            ));
        }
        let conn = match self.launcher.launch(plugin, &self.config) {
            Ok(conn) => conn,
            Err(err) => {
                let (kind, message) = describe_failure(&err, None, "could not start", self.config.timeouts.start);
                return Err(self.record_failure(e, &mut s, kind, message));
            }
        };
        let manifest = match conn.call(PluginRequest::Describe, self.config.timeouts.describe) {
            Ok(PluginReply::Manifest(m)) => m,
            Ok(other) => {
                conn.kill();
                let msg = format!("answered Describe with {}", reply_name(&other));
                return Err(self.record_failure(e, &mut s, ErrorKind::Protocol, msg));
            }
            Err(err) => {
                conn.kill();
                let (kind, message) =
                    describe_failure(&err, conn.exit_status(), "failed to describe itself", self.config.timeouts.describe);
                return Err(self.record_failure(e, &mut s, kind, message));
            }
        };
        if manifest.id != plugin.id || manifest.api_version != API_VERSION {
            conn.kill();
            let msg = if manifest.id != plugin.id {
                format!("says it is {:?}, but its plugin.toml says {:?}", manifest.id, plugin.id)
            } else {
                format!(
                    "speaks plugin API version {}, this Dyno version {API_VERSION}: update the plugin or Dyno",
                    manifest.api_version
                )
            };
            return Err(self.record_failure(e, &mut s, ErrorKind::Protocol, msg));
        }
        s.name = manifest.name;
        s.version = manifest.version;
        s.contributes = manifest.contributes;
        s.state = PluginState::Running;
        s.error = None;
        s.supervision.record_success();
        s.conn = Some(conn.clone());
        Ok(conn)
    }

    /// Calls the plugin; a crash, hang or garbage is counted and the plugin killed.
    fn call(&self, e: &Entry, req: PluginRequest, timeout: Duration) -> Result<PluginReply, HostError> {
        let conn = self.ensure(e)?;
        match conn.call(req, timeout) {
            Ok(reply) => Ok(reply),
            Err(IpcError::Remote(r)) if r.code != paddock_ipc::codes::PANIC => {
                let kind = match r.code.as_str() {
                    codes::UNSUPPORTED => ErrorKind::Unsupported,
                    _ => ErrorKind::Failed,
                };
                Err(HostError::new(&e.id, kind, r.message))
            }
            Err(err) => {
                let status = if matches!(err, IpcError::Closed(_) | IpcError::Io { .. }) {
                    conn.exit_status()
                } else {
                    None
                };
                let (kind, message) = describe_failure(&err, status, "failed", timeout);
                Err(self.fail_connection(e, &conn, kind, message))
            }
        }
    }

    fn wrong_reply(&self, e: &Entry, reply: &PluginReply) -> HostError {
        let conn = e.lock().conn.clone();
        let message = format!("answered with {}", reply_name(reply));
        match conn {
            Some(conn) => self.fail_connection(e, &conn, ErrorKind::Protocol, message),
            None => HostError::new(&e.id, ErrorKind::Protocol, message),
        }
    }

    /// Kills `conn` and counts the failure, unless another thread already did for this connection.
    fn fail_connection(&self, e: &Entry, conn: &Arc<dyn PluginConnection>, kind: ErrorKind, message: String) -> HostError {
        conn.kill();
        let mut s = e.lock();
        if !s.conn.as_ref().is_some_and(|c| same(c, conn)) {
            return HostError::new(&e.id, kind, message);
        }
        s.conn = None;
        self.record_failure(e, &mut s, kind, message)
    }

    fn record_failure(&self, e: &Entry, s: &mut Slot, kind: ErrorKind, message: String) -> HostError {
        match s.supervision.record_failure(self.clock.now(), &self.config.policy) {
            Verdict::Quarantine => {
                s.state = PluginState::Quarantined;
                let p = self.config.policy;
                s.error = Some(format!(
                    "{message} (quarantined after {} failures in {} minutes)",
                    p.max_failures,
                    p.window.as_secs() / 60
                ));
                HostError::new(&e.id, ErrorKind::Quarantined, s.error.clone().unwrap_or_default())
            }
            Verdict::RetryAt(_) => {
                s.state = PluginState::Failed;
                s.error = Some(message.clone());
                HostError::new(&e.id, kind, message)
            }
        }
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn describe_failure(err: &IpcError, status: Option<String>, what: &str, timeout: Duration) -> (ErrorKind, String) {
    match err {
        IpcError::Timeout => (
            ErrorKind::Timeout,
            format!("did not answer within {} ms and was stopped", timeout.as_millis()),
        ),
        IpcError::Remote(r) => (ErrorKind::Crashed, format!("panicked: {}", r.message)),
        IpcError::Protocol(m) => (ErrorKind::Protocol, format!("sent an invalid message ({m}) and was stopped")),
        IpcError::TooLarge { .. } => (
            ErrorKind::Protocol,
            format!("sent a message over the size limit and was stopped ({err})"),
        ),
        IpcError::Version(_) | IpcError::Auth(_) | IpcError::Denied(_) => (ErrorKind::Protocol, format!("{what}: {err}")),
        IpcError::Spawn(_) | IpcError::Closed(_) | IpcError::Io { .. } => match status {
            Some(status) => (ErrorKind::Crashed, format!("crashed ({status})")),
            None => (ErrorKind::Crashed, format!("{what}: {err}")),
        },
    }
}

fn reply_name(r: &PluginReply) -> &'static str {
    match r {
        PluginReply::Manifest(_) => "a manifest",
        PluginReply::Sensors(_) => "sensor readings",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use crate::config::Policy;
    use dyno_plugin_api::Manifest;
    use paddock_ipc::RemoteError;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// What a fake plugin answers to anything but `Describe`.
    type Script = Arc<Mutex<VecDeque<Result<PluginReply, IpcError>>>>;

    struct FakeConn {
        id: String,
        script: Script,
        killed: Arc<AtomicUsize>,
    }

    impl PluginConnection for FakeConn {
        fn call(&self, req: PluginRequest, _: Duration) -> Result<PluginReply, IpcError> {
            match req {
                PluginRequest::Describe => Ok(PluginReply::Manifest(
                    Manifest::new(&self.id, "Fake", "2.0").contributing(Contribution::Sensors),
                )),
                _ => self.script.lock().unwrap().pop_front().unwrap_or(Ok(PluginReply::Sensors(vec![]))),
            }
        }
        fn stop(&self, _: Duration) {
            self.killed.fetch_add(1, Ordering::SeqCst);
        }
        fn kill(&self) {
            self.killed.fetch_add(1, Ordering::SeqCst);
        }
        fn exit_status(&self) -> Option<String> {
            Some("exit code 3".into())
        }
    }

    struct FakeLauncher {
        /// Launch failures to hand out before succeeding.
        fail_launches: AtomicUsize,
        launches: AtomicUsize,
        id: Mutex<String>,
        script: Script,
        killed: Arc<AtomicUsize>,
    }

    impl Launcher for FakeLauncher {
        fn launch(&self, _: &Discovered, _: &HostConfig) -> Result<Arc<dyn PluginConnection>, IpcError> {
            self.launches.fetch_add(1, Ordering::SeqCst);
            if self.fail_launches.load(Ordering::SeqCst) > 0 {
                self.fail_launches.fetch_sub(1, Ordering::SeqCst);
                return Err(IpcError::Spawn("plugin \"p\" exited before it connected".into()));
            }
            Ok(Arc::new(FakeConn {
                id: self.id.lock().unwrap().clone(),
                script: self.script.clone(),
                killed: self.killed.clone(),
            }))
        }
    }

    fn setup(fail_launches: usize) -> (PluginHost, Arc<FakeLauncher>, Arc<ManualClock>) {
        let launcher = Arc::new(FakeLauncher {
            fail_launches: AtomicUsize::new(fail_launches),
            launches: AtomicUsize::new(0),
            id: Mutex::new("p".into()),
            script: Arc::new(Mutex::new(VecDeque::new())),
            killed: Arc::new(AtomicUsize::new(0)),
        });
        let clock = Arc::new(ManualClock::new());
        let plugin = Discovered {
            id: "p".into(),
            name: "P".into(),
            version: "1".into(),
            contributes: vec![Contribution::Sensors],
            dir: PathBuf::from("/plugins/p"),
            executable: PathBuf::from("/plugins/p/p"),
        };
        let invalid = Invalid {
            folder: "broken".into(),
            error: "plugin.toml is not valid".into(),
        };
        let host = PluginHost::with_plugins(
            HostConfig::new("/plugins"),
            vec![plugin],
            vec![invalid],
            launcher.clone(),
            clock.clone(),
        );
        (host, launcher, clock)
    }

    fn state(host: &PluginHost, id: &str) -> PluginStatus {
        host.list().into_iter().find(|p| p.id == id).unwrap()
    }

    fn push(l: &FakeLauncher, r: Result<PluginReply, IpcError>) {
        l.script.lock().unwrap().push_back(r);
    }

    #[test]
    fn a_plugin_starts_lazily_and_takes_its_manifest() {
        let (host, launcher, _) = setup(0);
        assert_eq!(state(&host, "p").state, PluginState::Stopped);
        assert_eq!(state(&host, "broken").state, PluginState::Failed);
        assert_eq!(state(&host, "broken").error.as_deref(), Some("plugin.toml is not valid"));
        push(
            &launcher,
            Ok(PluginReply::Sensors(vec![
                SensorReading::new("a", "A", 1.0, ""),
                SensorReading::new("n", "N", f64::NAN, ""),
            ])),
        );
        let got = host.sensors();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].result.as_ref().unwrap().len(), 1, "NaN is dropped");
        let p = state(&host, "p");
        assert_eq!((p.state, p.version.as_str()), (PluginState::Running, "2.0"));
        assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);
        assert!(host.start_all().is_empty());
        assert_eq!(launcher.launches.load(Ordering::SeqCst), 1, "a running plugin is not started twice");
        assert_eq!(host.sensors_of("nope").unwrap_err().kind, ErrorKind::NotFound);
        assert_eq!(host.sensors_of("broken").unwrap_err().kind, ErrorKind::Invalid);
    }

    #[test]
    fn crashes_restart_with_backoff_and_quarantine_after_three_in_ten_minutes() {
        let (host, launcher, clock) = setup(0);
        push(&launcher, Err(IpcError::Closed("gone".into())));
        let e = host.sensors().remove(0).result.unwrap_err();
        assert_eq!((e.plugin.as_str(), e.kind), ("p", ErrorKind::Crashed));
        assert!(e.message.contains("exit code 3"), "{}", e.message);
        assert_eq!(state(&host, "p").state, PluginState::Failed);
        assert_eq!(launcher.killed.load(Ordering::SeqCst), 1);
        // Within the backoff the plugin is skipped, and a direct call says when it restarts.
        assert!(host.sensors().is_empty());
        let wait = host.sensors_of("p").unwrap_err();
        assert_eq!(wait.kind, ErrorKind::NotRunning);
        assert!(wait.message.contains("restarting in 1 s"), "{}", wait.message);
        clock.advance(Duration::from_secs(1));
        push(&launcher, Err(IpcError::Timeout));
        assert_eq!(host.sensors().remove(0).result.unwrap_err().kind, ErrorKind::Timeout);
        clock.advance(Duration::from_secs(1));
        push(&launcher, Err(IpcError::Remote(RemoteError::new("panic", "boom"))));
        let e = host.sensors().remove(0).result.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Quarantined);
        assert!(
            e.message.contains("panicked: boom") && e.message.contains("quarantined"),
            "{}",
            e.message
        );
        assert_eq!(state(&host, "p").state, PluginState::Quarantined);
        clock.advance(Duration::from_secs(3600));
        assert!(host.sensors().is_empty(), "a quarantined plugin is not asked");
        assert_eq!(host.sensors_of("p").unwrap_err().kind, ErrorKind::Quarantined);
        host.unquarantine("p").unwrap();
        assert_eq!(state(&host, "p").state, PluginState::Running);
        assert!(state(&host, "p").error.is_none());
    }

    #[test]
    fn start_failures_count_too() {
        let (host, launcher, clock) = setup(5);
        let errs = host.start_all();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].kind, ErrorKind::Crashed);
        assert!(errs[0].message.contains("before it connected"));
        clock.advance(Duration::from_secs(1));
        assert_eq!(host.start_all()[0].kind, ErrorKind::Crashed);
        clock.advance(Duration::from_secs(2));
        assert_eq!(host.start_all()[0].kind, ErrorKind::Quarantined);
        assert!(host.start_all().is_empty(), "quarantined plugins are not started");
        assert_eq!(launcher.launches.load(Ordering::SeqCst), 3);
        assert!(host.unquarantine("p").is_err(), "it still fails to start");
        assert_eq!(state(&host, "p").state, PluginState::Failed);
    }

    #[test]
    fn typed_errors_are_not_failures_but_wrong_replies_are() {
        let (host, launcher, _) = setup(0);
        push(
            &launcher,
            Err(IpcError::Remote(RemoteError::new(codes::BAD_REQUEST, "odd request"))),
        );
        let e = host.sensors_of("p").unwrap_err();
        assert_eq!((e.kind, e.message.as_str()), (ErrorKind::Failed, "odd request"));
        push(&launcher, Err(IpcError::Remote(RemoteError::new(codes::UNSUPPORTED, "no sensors"))));
        assert_eq!(host.sensors_of("p").unwrap_err().kind, ErrorKind::Unsupported);
        assert_eq!(state(&host, "p").state, PluginState::Running);
        // A reply of the wrong kind is a protocol failure: killed and counted.
        push(&launcher, Ok(PluginReply::Manifest(Manifest::new("p", "P", "1"))));
        let e = host.sensors_of("p").unwrap_err();
        assert_eq!(e.kind, ErrorKind::Protocol);
        assert!(e.message.contains("manifest"));
        assert_eq!(state(&host, "p").state, PluginState::Failed);
    }

    #[test]
    fn a_manifest_that_disagrees_is_refused() {
        let (host, launcher, _) = setup(0);
        *launcher.id.lock().unwrap() = "someone-else".into();
        let e = host.start_all().remove(0);
        assert_eq!(e.kind, ErrorKind::Protocol);
        assert!(e.message.contains("someone-else"));
        assert_eq!(launcher.killed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn stopped_plugins_stay_stopped_until_started() {
        let (host, launcher, _) = setup(0);
        assert!(host.start_all().is_empty());
        host.stop_all();
        assert_eq!(launcher.killed.load(Ordering::SeqCst), 1);
        assert_eq!(state(&host, "p").state, PluginState::Stopped);
        assert!(host.sensors().is_empty());
        assert_eq!(host.sensors_of("p").unwrap_err().kind, ErrorKind::NotRunning);
        assert!(host.start_all().is_empty());
        assert_eq!(state(&host, "p").state, PluginState::Running);
        let _ = Policy::default();
    }
}
