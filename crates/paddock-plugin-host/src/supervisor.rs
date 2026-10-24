//! One plugin's process as the app sees it: started on first use, verified before every start (a changed program is a
//! quarantine-grade fault), restarted with back-off after a crash, quarantined after repeated faults, its settings restored
//! after a restart, and called only through [`paddock_guard::Guard`].
//!
//! # Faults and failures
//!
//! A **fault** is the plugin misbehaving and is counted towards quarantine (3 in 10 minutes by default): its process ended
//! (crash), a contribution panicked, a call outlived its timeout and the cancel grace (hang), a frame was oversized or
//! garbled, a reply or document did not decode or validate. A **typed failure** ([`paddock_plugin_api::Fail`], a refused
//! request) is the plugin answering on purpose: the call fails, nothing is counted. After a fault that ended the process the
//! next call waits for the back-off (an injected clock; [`Status::Restarting`]) and then starts it again.

use crate::config::HostConfig;
use crate::error::{FaultKind, HostError};
use crate::files::HostFiles;
use crate::grants::{Grants, SandboxSpec, sandbox_for};
use crate::installed::Installed;
use crate::output::OutputSink;
use crate::services::{HostServices, RunningTask};
use crate::status::Status;
use paddock_guard::{CallError, Failure, Guard};
use paddock_ipc::{Child, ChildSpec, IpcError, Json, codes};
use paddock_jobs::time::{Backoff, Retry};
use paddock_plugin_api::compat::check_described;
use paddock_plugin_api::extension::Contract;
use paddock_plugin_api::protocol::{
    Described, Fail, McpBudget, PLATFORM_PROTOCOL, PlatformRequest,
};
use paddock_plugin_api::tools::{PathAccess, PathKind, Schema};
use paddock_plugin_package::manifest::Contribution;
use paddock_plugin_package::{Manifest, PackageError, Permission};
use paddock_view::{ViewAction, ViewDoc, ViewPatch};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// The longest a call without a budget of its own is waited for (an hour).
const NO_BUDGET: Duration = Duration::from_secs(3600);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct State {
    status: Status,
    child: Option<Arc<Child>>,
    backoff: Backoff,
    /// The monotonic time (clock) before which the next start waits.
    restart_at: Option<Duration>,
    described: Option<Described>,
    enabled: bool,
    settings: Option<Value>,
}

struct Inner {
    cfg: Arc<HostConfig>,
    id: String,
    manifest: Manifest,
    program: std::path::PathBuf,
    data_dir: std::path::PathBuf,
    sha256: [u8; 32],
    grants: Mutex<Grants>,
    services: Arc<HostServices>,
    files: HostFiles,
    guard: Guard,
    state: Mutex<State>,
    /// Serialises starts so two callers do not launch two processes.
    start: Mutex<()>,
}

/// The supervisor of one installed plugin. Cheap to clone.
#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<Inner>,
}

impl Supervisor {
    /// A supervisor for `installed` (nothing is started). Its status is [`Status::Disabled`] when the user switched it off,
    /// [`Status::Quarantined`] when the quarantine file says so, [`Status::Refused`] when this app cannot serve its
    /// protocol or contracts (a plugin built for protocol 1 says to update it), else [`Status::Stopped`].
    pub fn new(cfg: Arc<HostConfig>, installed: Installed) -> Supervisor {
        let id = installed.manifest.id.to_string();
        let files = HostFiles::new();
        let grants = installed.record.grants.clone();
        let services = Arc::new(HostServices::new(
            installed.manifest.clone(),
            grants.clone(),
            files.clone(),
            OutputSink::new(cfg.outputs_dir.join(&id)),
            cfg.tasks.clone(),
            cfg.elevated.clone(),
        ));
        let status = status_for(&cfg, &installed.manifest, installed.record.enabled);
        let state = State {
            status,
            child: None,
            backoff: Backoff::new(cfg.backoff),
            restart_at: None,
            described: None,
            enabled: installed.record.enabled,
            settings: None,
        };
        Supervisor {
            inner: Arc::new(Inner {
                guard: Guard::new(cfg.quarantine.clone(), cfg.clock.clone()),
                cfg,
                id,
                manifest: installed.manifest,
                program: installed.program,
                data_dir: installed.data_dir,
                sha256: installed.sha256,
                grants: Mutex::new(grants),
                services,
                files,
                state: Mutex::new(state),
                start: Mutex::new(()),
            }),
        }
    }

    /// The plugin's id.
    pub fn id(&self) -> &str {
        &self.inner.id
    }

    /// The manifest the user reviewed.
    pub fn manifest(&self) -> &Manifest {
        &self.inner.manifest
    }

    /// The files the app opens for this plugin.
    pub fn files(&self) -> &HostFiles {
        &self.inner.files
    }

    /// What the plugin said about itself when it last started.
    pub fn described(&self) -> Option<Described> {
        lock(&self.inner.state).described.clone()
    }

    /// The tasks the plugin announced and has not ended.
    pub fn tasks(&self) -> Vec<RunningTask> {
        self.inner.services.tasks()
    }

    /// What the user granted now.
    pub fn grants(&self) -> Grants {
        lock(&self.inner.grants).clone()
    }

    /// The sandbox the plugin's process runs under now.
    pub fn sandbox(&self) -> SandboxSpec {
        sandbox_for(&self.inner.manifest, &lock(&self.inner.grants))
    }

    /// Changes the grants. Reads and tool paths follow at once; the sandbox of a running process changes when it restarts.
    pub fn set_grants(&self, grants: Grants) {
        self.inner.services.set_grants(grants.clone());
        *lock(&self.inner.grants) = grants;
    }

    /// Switches the plugin on or off; off stops the process. A plugin that cannot be served or is quarantined stays so.
    pub fn set_enabled(&self, enabled: bool) {
        if !enabled {
            self.end_process();
        }
        let mut st = lock(&self.inner.state);
        st.enabled = enabled;
        if !enabled || st.status == Status::Disabled {
            st.status = status_for(&self.inner.cfg, &self.inner.manifest, enabled);
        }
    }

    /// The user re-enabled a quarantined plugin (or pressed Retry): its failures are forgotten and it may start again.
    pub fn release(&self) {
        self.inner.cfg.quarantine.release(&self.inner.id);
        let mut st = lock(&self.inner.state);
        st.backoff.reset();
        st.restart_at = None;
        if matches!(
            st.status,
            Status::Quarantined { .. } | Status::Restarting { .. }
        ) {
            st.status = status_for(&self.inner.cfg, &self.inner.manifest, st.enabled);
        }
    }

    /// The status for the UI. An unexpected end of the process is noticed here too.
    pub fn status(&self) -> Status {
        self.reap();
        lock(&self.inner.state).status.clone()
    }

    /// Blocks until the process has ended or `guard` of real time has passed (a hang guard, not a timing): true when it ended.
    /// A process that is not running has ended.
    pub fn wait_until_ended(&self, guard: Duration) -> bool {
        let child = lock(&self.inner.state).child.clone();
        child.is_none_or(|c| c.peer().wait_closed(Some(guard)))
    }

    /// Stops the process politely (closes its lifeline, then ends its tree). Not a fault.
    pub fn stop(&self) {
        let child = self.take_child();
        if let Some(child) = child {
            child.stop(self.inner.cfg.stop_grace);
        }
        self.inner.services.process_ended();
        let mut st = lock(&self.inner.state);
        if matches!(st.status, Status::Running | Status::Starting) {
            st.status = Status::Stopped;
        }
    }

    fn take_child(&self) -> Option<Arc<Child>> {
        lock(&self.inner.state).child.take()
    }

    fn end_process(&self) {
        if let Some(child) = self.take_child() {
            child.kill();
        }
        self.inner.services.process_ended();
    }

    /// Notices a process that ended without a call in flight: a counted crash.
    fn reap(&self) {
        let dead = {
            let st = lock(&self.inner.state);
            st.child
                .as_ref()
                .is_some_and(|c| c.exited() || c.peer().is_closed())
        };
        if dead {
            let over = lock(&self.inner.state)
                .child
                .as_ref()
                .and_then(|c| c.stopped_over_memory());
            self.fault(FaultKind::Crash, &ended_text(over), true);
        }
    }

    /// Counts a fault; when `end` the process is ended and the next start waits for the back-off.
    fn fault(&self, kind: FaultKind, message: &str, end: bool) {
        let failure = match kind {
            FaultKind::Crash => Failure::Panic,
            FaultKind::Hang => Failure::Hang,
            FaultKind::Garbled => Failure::Garbled,
        };
        self.inner
            .cfg
            .quarantine
            .record(&self.inner.id, failure, message);
        if kind == FaultKind::Hang {
            // The call that overran has returned (it timed out on the connection), so the plugin is not "hung" any more:
            // the guard marks it so only for a call it abandons itself.
            self.inner.cfg.quarantine.hang_ended(&self.inner.id);
        }
        self.after_fault(message, end);
    }

    /// A fault the guard already counted (a call it abandoned as hung).
    fn after_fault(&self, message: &str, end: bool) {
        let quarantined = self.inner.cfg.quarantine.is_quarantined(&self.inner.id);
        if end || quarantined {
            self.end_process();
        }
        let now = self.inner.cfg.clock.monotonic();
        let mut st = lock(&self.inner.state);
        if quarantined {
            st.status = Status::Quarantined {
                why: message.to_string(),
            };
        } else if end {
            match st.backoff.failed(now) {
                Retry::After(delay) => {
                    st.restart_at = Some(now + delay);
                    st.status = Status::Restarting {
                        why: message.to_string(),
                        delay,
                    };
                }
                Retry::GiveUp => {
                    drop(st);
                    self.quarantine_now(message);
                }
            }
        }
    }

    /// Quarantines the plugin at once (a tampered program, a back-off that gave up): records faults up to the policy.
    fn quarantine_now(&self, why: &str) {
        let q = &self.inner.cfg.quarantine;
        for _ in 0..q.policy().max.max(1) {
            if q.is_quarantined(&self.inner.id) {
                break;
            }
            q.record(&self.inner.id, Failure::Panic, why);
        }
        self.end_process();
        lock(&self.inner.state).status = Status::Quarantined {
            why: why.to_string(),
        };
    }

    fn unavailable(&self, st: &State) -> Option<HostError> {
        let id = &self.inner.id;
        match &st.status {
            Status::Disabled => Some(HostError::Unavailable(format!(
                "plugin {id} is switched off; turn it on in the plugin list"
            ))),
            Status::Refused(why) => Some(HostError::Unavailable(why.clone())),
            Status::Quarantined { why } => Some(HostError::Unavailable(format!(
                "plugin {id} is disabled after repeated failures (last: {why}); re-enable it in the plugin list"
            ))),
            _ => None,
        }
    }

    /// The running process, started (and verified, and described) when it is not up yet.
    fn ensure_running(&self) -> Result<Arc<Child>, HostError> {
        self.reap();
        match self.ready() {
            Some(ready) => ready,
            None => {
                let _one_start = lock(&self.inner.start);
                // Another caller may have started it while this one waited.
                self.ready().unwrap_or_else(|| self.start())
            }
        }
    }

    /// What can be said without starting anything: the running process, or why the plugin cannot be called now (switched
    /// off, refused, quarantined, or waiting for the back-off after a crash). `None`: it has to be started.
    fn ready(&self) -> Option<Result<Arc<Child>, HostError>> {
        let st = lock(&self.inner.state);
        if let Some(e) = self.unavailable(&st) {
            return Some(Err(e));
        }
        if let Some(child) = &st.child {
            return Some(Ok(child.clone()));
        }
        let now = self.inner.cfg.clock.monotonic();
        let (Some(at), Status::Restarting { why, .. }) = (st.restart_at, &st.status) else {
            return None;
        };
        if now >= at {
            return None;
        }
        let left = at - now;
        let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
        let id = &self.inner.id;
        Some(Err(HostError::Unavailable(format!(
            "plugin {id} ended ({why}) and restarts in {seconds} s"
        ))))
    }

    fn refuse(&self, why: String) -> HostError {
        self.end_process();
        lock(&self.inner.state).status = Status::Refused(why.clone());
        HostError::Unavailable(why)
    }

    fn start(&self) -> Result<Arc<Child>, HostError> {
        let id = self.inner.id.clone();
        lock(&self.inner.state).status = Status::Starting;
        match paddock_plugin_package::archive::verify_installed(
            &self.inner.program,
            &self.inner.sha256,
        ) {
            Ok(()) => {}
            Err(e @ PackageError::Tampered(_)) => {
                // Quarantine-grade: the program is not the one the user installed.
                let why = e.to_string();
                self.quarantine_now(&why);
                return Err(HostError::Unavailable(format!(
                    "plugin {id} was not started: {why}"
                )));
            }
            Err(e) => {
                return Err(self.refuse(format!("plugin {id} cannot start: {e}; reinstall it")));
            }
        }
        if let Err(e) = std::fs::create_dir_all(&self.inner.data_dir) {
            return Err(self.refuse(format!("plugin {id} has no data folder ({e})")));
        }
        let cfg = &self.inner.cfg;
        let sandbox = self.sandbox();
        let mut spec = ChildSpec::new(&self.inner.program, format!("plugin {id}"));
        spec.current_dir = Some(self.inner.data_dir.clone());
        spec.memory_limit = Some(sandbox.memory_limit);
        spec.limit = cfg.max_frame;
        spec.protocol = PLATFORM_PROTOCOL;
        spec.host_name = cfg.info.app.clone();
        spec.start_timeout = cfg.start_timeout;
        spec.extra = serde_json::to_value(&sandbox).ok().map(Json);
        spec.peer.clock = cfg.clock.clone();
        spec.envs = cfg
            .child_env
            .iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        let child = match paddock_ipc::launch(&spec, self.inner.services.clone()) {
            Ok(c) => Arc::new(c),
            // The plugin speaks another protocol: nobody crashed, somebody must update (the text says who).
            Err(IpcError::Version(m)) => return Err(self.refuse(m)),
            Err(e) => {
                let why = format!("plugin {id} could not start ({e})");
                self.fault(FaultKind::Crash, &why, true);
                return Err(HostError::Fault {
                    kind: FaultKind::Crash,
                    message: why,
                });
            }
        };
        let described = match self.describe_child(&child) {
            Ok(d) => d,
            Err(e) => {
                child.kill();
                return Err(e);
            }
        };
        if let Err(r) = check_described(&self.inner.manifest, &described) {
            child.kill();
            return Err(self.refuse(r.to_string()));
        }
        let settings = lock(&self.inner.state).settings.clone();
        if let Some(values) = settings {
            let applied: Result<(), IpcError> = child.peer().request(
                &PlatformRequest::ApplySettings { json: Json(values) },
                Some(cfg.call_timeout),
            );
            if let Err(e) = applied {
                log::warn!(target: "paddock_plugin", "plugin {id} did not take its saved settings: {e}");
            }
        }
        let now = cfg.clock.monotonic();
        let mut st = lock(&self.inner.state);
        st.child = Some(child.clone());
        st.described = Some(described);
        st.status = Status::Running;
        st.restart_at = None;
        st.backoff.healthy(now);
        Ok(child)
    }

    /// Counts a fault and gives it back as the error of the call.
    fn faulted(&self, kind: FaultKind, message: String, end: bool) -> HostError {
        self.fault(kind, &message, end);
        HostError::Fault { kind, message }
    }

    fn describe_child(&self, child: &Child) -> Result<Described, HostError> {
        let id = &self.inner.id;
        let timeout = Some(self.inner.cfg.start_timeout);
        let asked: Result<Json, IpcError> =
            child.peer().request(&PlatformRequest::Describe, timeout);
        let json = match asked {
            Ok(json) => json,
            Err(IpcError::Timeout) => {
                return Err(self.faulted(
                    FaultKind::Hang,
                    format!("plugin {id} did not describe itself in time"),
                    true,
                ));
            }
            Err(e) => {
                return Err(self.faulted(
                    FaultKind::Crash,
                    format!("plugin {id} did not describe itself ({e})"),
                    true,
                ));
            }
        };
        serde_json::from_value::<Described>(json.into_inner()).map_err(|e| {
            self.faulted(
                FaultKind::Garbled,
                format!("plugin {id} described itself in a way this app cannot read ({e})"),
                true,
            )
        })
    }

    /// Sends one request through the guard and returns the reply bytes. The process is started first when needed.
    fn call(
        &self,
        kind: &str,
        request: &PlatformRequest,
        timeout: Duration,
    ) -> Result<Vec<u8>, HostError> {
        let body = paddock_ipc::wire::encode(request).map_err(HostError::Invalid)?;
        let child = self.ensure_running()?;
        let peer = child.peer().clone();
        let budget = timeout
            .saturating_add(peer_cancel_grace())
            .saturating_add(self.inner.cfg.hang_grace);
        let id = &self.inner.id;
        let outcome = self.inner.guard.call(id, kind, Some(budget), move || {
            peer.call(body, Some(timeout))
        });
        match outcome {
            Ok(bytes) => Ok(bytes),
            Err(CallError::Failed(e)) => Err(self.classify(e)),
            // The plugin is quarantined or not responding (or no thread could be started): the call was not made.
            Err(e @ (CallError::Refused(_) | CallError::Unavailable(_))) => {
                self.sync_quarantine(&e.to_string());
                Err(HostError::Unavailable(e.to_string()))
            }
            // The guard counted a panic or an abandoned call itself; the supervisor only ends the process of a hung call.
            Err(e) => {
                let hung = matches!(e, CallError::Hung { .. });
                let kind = if hung {
                    FaultKind::Hang
                } else {
                    FaultKind::Crash
                };
                Err(self.counted(kind, e.to_string(), hung))
            }
        }
    }

    /// A fault the guard already counted, as the error of the call.
    fn counted(&self, kind: FaultKind, message: String, end: bool) -> HostError {
        self.after_fault(&message, end);
        HostError::Fault { kind, message }
    }

    fn sync_quarantine(&self, why: &str) {
        if self.inner.cfg.quarantine.is_quarantined(&self.inner.id) {
            lock(&self.inner.state).status = Status::Quarantined {
                why: why.to_string(),
            };
        }
    }

    /// An IPC failure as a typed failure or a counted fault.
    fn classify(&self, e: IpcError) -> HostError {
        let id = &self.inner.id;
        match e {
            IpcError::Remote(r) if r.code == codes::CANCELLED => HostError::Cancelled,
            IpcError::Remote(r) if r.code == codes::PANIC => self.faulted(
                FaultKind::Crash,
                format!("plugin {id} panicked: {}", r.message),
                false,
            ),
            IpcError::Remote(r) if r.code == codes::TOO_LARGE => self.faulted(
                FaultKind::Garbled,
                format!(
                    "plugin {id} sent a message over the size limit: {}",
                    r.message
                ),
                false,
            ),
            IpcError::Remote(r) => HostError::Remote(r),
            // Our own request did not fit a frame: the plugin did nothing wrong.
            IpcError::TooLarge { len, max } => HostError::Invalid(format!(
                "the request is {len} bytes; a message to plugin {id} can have {max}"
            )),
            IpcError::Timeout => self.faulted(
                FaultKind::Hang,
                format!("plugin {id} stopped responding"),
                true,
            ),
            IpcError::Protocol(m) => self.faulted(
                FaultKind::Garbled,
                format!("plugin {id} sent an invalid message ({m})"),
                true,
            ),
            IpcError::Version(m) | IpcError::Denied(m) | IpcError::Auth(m) => {
                HostError::Unavailable(m)
            }
            other => self.faulted(
                FaultKind::Crash,
                format!("plugin {id} ended: {other}"),
                true,
            ),
        }
    }

    /// A reply of type `T`; one that does not decode is a counted fault.
    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, HostError> {
        paddock_ipc::wire::decode::<T>(bytes).map_err(|m| {
            self.faulted(
                FaultKind::Garbled,
                format!(
                    "plugin {} sent a reply this app cannot read ({m})",
                    self.inner.id
                ),
                false,
            )
        })
    }

    /// A JSON reply `Result<Json, Fail>`: the plugin's typed failure is [`HostError::Failed`].
    fn json_reply(&self, bytes: &[u8]) -> Result<Value, HostError> {
        match self.decode::<Result<Json, Fail>>(bytes)? {
            Ok(j) => Ok(j.into_inner()),
            Err(f) => Err(HostError::Failed(f)),
        }
    }

    /// What the plugin described, starting it when it is not running.
    fn described_or_start(&self) -> Result<Described, HostError> {
        self.ensure_running()?;
        let described = lock(&self.inner.state).described.clone();
        Ok(described.unwrap_or_default())
    }

    /// Gives the plugin its settings after validating them against the section it described; they are kept and given again
    /// after a restart.
    pub fn apply_settings(&self, values: Value) -> Result<(), HostError> {
        let id = &self.inner.id;
        let Some(section) = self.described_or_start()?.settings else {
            return Err(HostError::Invalid(format!("plugin {id} has no settings")));
        };
        let valid = paddock_settings::validate(&section, &values)
            .map_err(|errors| HostError::Invalid(field_errors(&errors)))?;
        let request = PlatformRequest::ApplySettings {
            json: Json(valid.to_json()),
        };
        let timeout = self.inner.cfg.call_timeout;
        let bytes = self.call("settings", &request, timeout)?;
        self.decode::<()>(&bytes)?;
        lock(&self.inner.state).settings = Some(valid.to_json());
        Ok(())
    }

    /// Runs operation `op` of tool `tool`: the arguments are validated against the operation's schema, and path arguments
    /// must lie inside the folders the plugin was granted (they arrive canonicalised). The answer is the operation's result;
    /// a failure it reports on purpose is [`HostError::Failed`].
    pub fn invoke(&self, tool: &str, op: &str, mut args: Value) -> Result<Value, HostError> {
        let id = &self.inner.id;
        let described = self.described_or_start()?;
        let Some(info) = described.tools.iter().find(|t| t.id == tool) else {
            return Err(HostError::Invalid(format!(
                "plugin {id} has no tool \"{tool}\""
            )));
        };
        let Some(spec) = info.ops.iter().find(|o| o.name == op) else {
            return Err(HostError::Invalid(format!(
                "tool \"{tool}\" has no operation \"{op}\""
            )));
        };
        if spec.elevated {
            return Err(HostError::Invalid(format!(
                "\"{tool} {op}\" runs with administrator rights; the app starts that process"
            )));
        }
        spec.args
            .validate(&args)
            .map_err(|e| HostError::Invalid(e.to_string()))?;
        self.check_paths(&spec.args, &mut args)?;
        let request = PlatformRequest::Invoke {
            tool: tool.into(),
            op: op.into(),
            args: Json(args),
        };
        let timeout = spec.budget.unwrap_or(NO_BUDGET);
        let label = format!("tool {tool} {op}");
        let bytes = self.call(&label, &request, timeout)?;
        self.json_reply(&bytes)
    }

    fn check_paths(&self, schema: &Schema, args: &mut Value) -> Result<(), HostError> {
        let grants = lock(&self.inner.grants).clone();
        let policy = grants.path_policy(&self.inner.manifest);
        let check = |p: &str, k: PathKind, a: PathAccess| policy(p, k, a);
        schema
            .check_paths(args, &check)
            .map_err(|e| HostError::Invalid(e.to_string()))
    }

    /// Stops the background task `task`.
    pub fn stop_task(&self, task: &str) -> Result<(), HostError> {
        let request = PlatformRequest::Stop { task: task.into() };
        let timeout = self.inner.cfg.call_timeout;
        let bytes = self.call("stop", &request, timeout)?;
        let reply: Result<(), Fail> = self.decode(&bytes)?;
        reply.map_err(HostError::Failed)
    }

    /// The document of view `view`, validated; one that breaks the limits is a counted fault of the plugin.
    pub fn render_view(&self, view: &str, ctx: Value) -> Result<ViewDoc, HostError> {
        let described = self.described_or_start()?;
        if !described.views.iter().any(|v| v.id == view) {
            return Err(HostError::Invalid(format!(
                "plugin {} has no view \"{view}\"",
                self.inner.id
            )));
        }
        let request = PlatformRequest::RenderView {
            view: view.into(),
            ctx: Json(ctx),
        };
        let timeout = self.inner.cfg.call_timeout;
        let label = format!("view {view}");
        let bytes = self.call(&label, &request, timeout)?;
        let value = self.json_reply(&bytes)?;
        self.view_value(value, |d: &ViewDoc| paddock_view::validate(d))
    }

    /// Delivers the user's action on a view and returns the patch, validated. The action must be one the manifest lists
    /// for the view.
    pub fn view_action(&self, action: &ViewAction) -> Result<ViewPatch, HostError> {
        paddock_view::validate_action(action).map_err(|e| HostError::Invalid(e.to_string()))?;
        if !declares_action(&self.inner.manifest, action) {
            let id = &self.inner.id;
            return Err(HostError::Invalid(format!(
                "plugin {id} does not declare the action \"{}\" on view \"{}\"",
                action.action, action.view
            )));
        }
        let request = PlatformRequest::ViewAction {
            view: action.view.clone(),
            action: action.action.clone(),
            args: action.args.clone(),
        };
        let timeout = self.inner.cfg.call_timeout;
        let label = format!("view {} {}", action.view, action.action);
        let bytes = self.call(&label, &request, timeout)?;
        let value = self.json_reply(&bytes)?;
        self.view_value(value, check_patch)
    }

    fn view_value<T: DeserializeOwned>(
        &self,
        value: Value,
        validate: impl Fn(&T) -> Result<(), paddock_view::ViewError>,
    ) -> Result<T, HostError> {
        let garbled = |m: String| {
            self.faulted(
                FaultKind::Garbled,
                format!(
                    "plugin {} returned a view the app refuses ({m})",
                    self.inner.id
                ),
                false,
            )
        };
        let parsed: T =
            paddock_view::from_json(&value.to_string()).map_err(|e| garbled(e.to_string()))?;
        validate(&parsed).map_err(|e| garbled(e.to_string()))?;
        Ok(parsed)
    }

    /// Calls an MCP tool the plugin contributed. Needs the `mcpExpose` permission asked **and** granted; the answer is cut
    /// to the budget (an answer over it is a typed failure of the call).
    pub fn mcp_tool(&self, tool: &str, args: Value, budget: McpBudget) -> Result<Value, HostError> {
        let id = &self.inner.id;
        let asked = self
            .inner
            .manifest
            .permissions
            .iter()
            .any(|p| p.permission == Permission::McpExpose);
        if !(asked
            && lock(&self.inner.grants)
                .permissions
                .contains(&Permission::McpExpose))
        {
            return Err(HostError::Invalid(format!(
                "plugin {id} may not offer tools over MCP; allow it first"
            )));
        }
        let described = self.described_or_start()?;
        let Some(info) = described.mcp_tools.iter().find(|t| t.name == tool) else {
            return Err(HostError::Invalid(format!(
                "plugin {id} has no MCP tool \"{tool}\""
            )));
        };
        info.args
            .validate(&args)
            .map_err(|e| HostError::Invalid(e.to_string()))?;
        let timeout = Duration::from_millis(budget.max_ms).min(NO_BUDGET);
        let request = PlatformRequest::McpTool {
            tool: tool.into(),
            args: Json(args),
            budget,
        };
        let label = format!("mcp {tool}");
        let bytes = self.call(&label, &request, timeout)?;
        let value = self.json_reply(&bytes)?;
        let size = value.to_string().len() as u64;
        if size > budget.max_bytes {
            let message = format!(
                "the answer is {size} bytes; the budget is {}",
                budget.max_bytes
            );
            return Err(HostError::Failed(Fail::new(message)));
        }
        Ok(value)
    }

    /// A typed proxy for the contract `C` of this plugin. The plugin must declare `C::NS` in its manifest, at a minor this
    /// code knows.
    pub fn remote<C: Contract>(&self) -> Result<Remote<C>, HostError> {
        let id = &self.inner.id;
        let declared = self
            .inner
            .manifest
            .namespaces()
            .find(|n| n.ns() == C::NS)
            .ok_or_else(|| {
                HostError::Invalid(format!("plugin {id} does not speak \"{}\"", C::NS))
            })?;
        if declared.minor > C::MINOR {
            let refusal = paddock_plugin_api::compat::Refusal::AppTooOld {
                plugin: id.clone(),
                app: self.inner.cfg.info.app.clone(),
                needs: None,
                what: format!(
                    "\"{}\" version {}, this {} serves up to {}",
                    C::NS,
                    declared.minor,
                    self.inner.cfg.info.app,
                    C::MINOR
                ),
            };
            return Err(HostError::Unavailable(refusal.to_string()));
        }
        Ok(Remote {
            sup: self.clone(),
            minor: declared.minor,
            _contract: PhantomData,
        })
    }

    /// The program the supervisor starts.
    pub fn program(&self) -> &Path {
        &self.inner.program
    }
}

/// The refused fields of a settings change as one text.
fn field_errors(errors: &[paddock_settings::FieldError]) -> String {
    errors
        .iter()
        .map(|e| format!("{}: {}", e.key, e.message))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Whether the manifest lists `action` on its view.
fn declares_action(manifest: &Manifest, action: &ViewAction) -> bool {
    manifest.contributions.iter().any(|c| match c {
        Contribution::View { id, actions } => {
            *id == action.view && actions.iter().any(|a| a.id == action.action)
        }
        _ => false,
    })
}

/// A patch the app would apply: a whole document must validate; a replacement node is validated inside an empty document.
fn check_patch(patch: &ViewPatch) -> Result<(), paddock_view::ViewError> {
    match patch {
        ViewPatch::Doc(doc) => paddock_view::validate(doc),
        ViewPatch::Replace { node, .. } => paddock_view::validate(&ViewDoc {
            schema: paddock_view::SCHEMA,
            title: "patch".into(),
            root: node.clone(),
            etag: 0,
        }),
    }
}

/// Why a process ended by itself: over its memory cap (the limit in bytes) or for no reason the app knows.
fn ended_text(over_memory: Option<u64>) -> String {
    match over_memory {
        Some(limit) => format!(
            "it used more than {} MiB of memory and was stopped",
            limit >> 20
        ),
        None => "its process ended unexpectedly".to_string(),
    }
}

/// How long the peer gives a callee to end a cancelled call (the IPC default; the guard budget adds it).
fn peer_cancel_grace() -> Duration {
    paddock_ipc::peer::CANCEL_GRACE
}

/// The status a plugin has when nothing has happened yet: switched off, refused (this app cannot serve its protocol or
/// contracts), quarantined (the quarantine file says so), else stopped.
fn status_for(cfg: &HostConfig, manifest: &Manifest, enabled: bool) -> Status {
    let label = manifest.id.to_string();
    let compat = cfg
        .info
        .check_protocol(&label, manifest.protocol)
        .and_then(|()| {
            manifest
                .namespaces()
                .try_for_each(|n| cfg.info.check_extension(&label, n.ns(), n.minor))
        });
    if !enabled {
        Status::Disabled
    } else if let Err(r) = compat {
        Status::Refused(r.to_string())
    } else if cfg.quarantine.is_quarantined(&label) {
        Status::Quarantined {
            why: cfg.quarantine.status(&label).last_error.unwrap_or_default(),
        }
    } else {
        Status::Stopped
    }
}

/// A typed proxy for one app contract of one plugin. It can only be made by [`Supervisor::remote`], so every call goes
/// through the supervisor's guard: a panic, a hang, an oversized or garbled frame is a fault attributed to the plugin and
/// counted; a typed failure the contract returns in its own reply type is not.
pub struct Remote<C: Contract> {
    sup: Supervisor,
    minor: u16,
    _contract: PhantomData<fn() -> C>,
}

impl<C: Contract> Clone for Remote<C> {
    fn clone(&self) -> Self {
        Remote {
            sup: self.sup.clone(),
            minor: self.minor,
            _contract: PhantomData,
        }
    }
}

impl<C: Contract> Remote<C> {
    /// The plugin this proxy talks to.
    pub fn plugin(&self) -> &str {
        self.sup.id()
    }

    /// Asks `request`, waiting at most the app's call timeout.
    pub fn call(&self, request: &C::Request) -> Result<C::Reply, HostError> {
        self.call_within(request, self.sup.inner.cfg.call_timeout)
    }

    /// Asks `request`, waiting at most `timeout` (on the app's clock).
    pub fn call_within(
        &self,
        request: &C::Request,
        timeout: Duration,
    ) -> Result<C::Reply, HostError> {
        let body = paddock_ipc::wire::encode(request).map_err(HostError::Invalid)?;
        let req = PlatformRequest::Extension {
            ns: C::NS.into(),
            minor: self.minor,
            body,
        };
        let bytes = self.sup.call(C::NS, &req, timeout)?;
        // The reply of an extension is the contract's own postcard bytes.
        self.sup.decode::<C::Reply>(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installed::InstallRecord;
    use paddock_guard::{Quarantine, QuarantinePolicy};
    use paddock_ipc::RemoteError;
    use paddock_jobs::time::{Clock, FakeClock};
    use paddock_plugin_api::compat::HostInfo;
    use paddock_view::{Node, SCHEMA};

    fn supervisor() -> (Supervisor, Arc<Quarantine>) {
        let manifest = Manifest::parse(
            "format = 1\n[plugin]\nid = \"net.example.p\"\nname = \"P\"\nversion = \"1.0.0\"\npublisher = \"E\"\nprotocol = \"2.0\"\nbinary = \"p\"\ntargets = [\"x86_64-unknown-linux-gnu\"]\n",
        )
        .unwrap();
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::new());
        let quarantine = Arc::new(Quarantine::new(QuarantinePolicy::default(), clock.clone()));
        let dir = std::env::temp_dir();
        let cfg = Arc::new(HostConfig::new(
            HostInfo::new("TestApp"),
            clock,
            quarantine.clone(),
            dir.join("paddock-plugin-host-unit-out"),
        ));
        let record = InstallRecord {
            id: "net.example.p".into(),
            version: "1.0.0".into(),
            signature: "unsigned".into(),
            trust: "unsigned".into(),
            signer: None,
            warning: None,
            digest: String::new(),
            folder: "v".into(),
            program: "p".into(),
            sha256: String::new(),
            requested: Vec::new(),
            grants: Grants::none(),
            enabled: true,
            installed_at_ms: 0,
        };
        let installed = Installed {
            record,
            manifest,
            program: "/does/not/exist".into(),
            data_dir: "/does/not/exist-data".into(),
            sha256: [0; 32],
        };
        (Supervisor::new(cfg, installed), quarantine)
    }

    #[test]
    fn a_view_the_app_refuses_is_a_counted_fault_of_the_plugin() {
        let (s, q) = supervisor();
        let doc = |title: &str| ViewDoc {
            schema: SCHEMA,
            title: title.into(),
            root: Node::text("x"),
            etag: 1,
        };
        // Valid: passes through.
        let ok = s
            .view_value::<ViewDoc>(serde_json::to_value(doc("T")).unwrap(), |d| {
                paddock_view::validate(d)
            })
            .unwrap();
        assert_eq!(ok.title, "T");
        assert_eq!(q.status("net.example.p").total, 0);
        // Not a document at all.
        let e = s
            .view_value::<ViewDoc>(serde_json::json!({"bogus": 1}), paddock_view::validate)
            .unwrap_err();
        assert!(
            matches!(&e, HostError::Fault { kind: FaultKind::Garbled, message } if message.contains("returned a view the app refuses")),
            "{e}"
        );
        // A document over the limits.
        let mut bad = doc("T");
        bad.schema = 99;
        let e = s
            .view_value::<ViewDoc>(serde_json::to_value(bad).unwrap(), |d| {
                paddock_view::validate(d)
            })
            .unwrap_err();
        assert!(matches!(
            e,
            HostError::Fault {
                kind: FaultKind::Garbled,
                ..
            }
        ));
        assert_eq!(q.status("net.example.p").total, 2);
        // A reply that does not decode.
        let e = s.decode::<Vec<String>>(&[0xff, 0xff, 0xff]).unwrap_err();
        assert!(matches!(
            e,
            HostError::Fault {
                kind: FaultKind::Garbled,
                ..
            }
        ));
        assert_eq!(q.status("net.example.p").total, 3);
        assert!(q.is_quarantined("net.example.p"));
        assert!(matches!(s.status(), Status::Quarantined { .. }));
    }

    #[test]
    fn a_process_that_ended_by_itself_is_noticed_as_a_crash_when_nothing_is_calling() {
        let (s, q) = supervisor();
        // No process: nothing to notice.
        assert_eq!(s.status(), Status::Stopped);
        s.reap();
        assert_eq!(q.status("net.example.p").total, 0);
        // The back-off that gives up quarantines at once.
        s.quarantine_now("it keeps failing");
        assert!(q.is_quarantined("net.example.p"));
        assert_eq!(
            s.status(),
            Status::Quarantined {
                why: "it keeps failing".into()
            }
        );
        s.quarantine_now("again");
        assert_eq!(q.status("net.example.p").total, 3);
    }

    #[test]
    fn every_ipc_failure_is_a_typed_failure_a_refusal_or_a_counted_fault() {
        let (s, q) = supervisor();
        let cancelled = s.classify(IpcError::Remote(RemoteError::new(codes::CANCELLED, "stop")));
        assert_eq!(cancelled, HostError::Cancelled);
        let refused = s.classify(IpcError::Remote(RemoteError::new("x.failed", "no")));
        assert!(matches!(refused, HostError::Remote(r) if r.code == "x.failed"));
        let big = s.classify(IpcError::TooLarge { len: 10, max: 5 });
        assert!(
            matches!(&big, HostError::Invalid(m) if m.contains("the request is 10 bytes")),
            "{big}"
        );
        for e in [
            IpcError::Version("v".into()),
            IpcError::Denied("d".into()),
            IpcError::Auth("a".into()),
        ] {
            assert!(matches!(s.classify(e), HostError::Unavailable(_)));
        }
        assert_eq!(q.status("net.example.p").total, 0);
        let garbled = s.classify(IpcError::Protocol("junk".into()));
        assert!(
            matches!(&garbled, HostError::Fault { kind: FaultKind::Garbled, message } if message.contains("(junk)")),
            "{garbled}"
        );
        let too_large = s.classify(IpcError::Remote(RemoteError::new(codes::TOO_LARGE, "big")));
        assert!(matches!(
            too_large,
            HostError::Fault {
                kind: FaultKind::Garbled,
                ..
            }
        ));
        assert_eq!(q.status("net.example.p").total, 2);
    }

    #[test]
    fn a_process_over_its_memory_cap_says_so() {
        assert_eq!(ended_text(None), "its process ended unexpectedly");
        assert_eq!(
            ended_text(Some(256 << 20)),
            "it used more than 256 MiB of memory and was stopped"
        );
    }

    #[test]
    fn a_disabled_plugin_that_was_installed_off_starts_off() {
        let (s, _q) = supervisor();
        s.set_enabled(false);
        assert_eq!(s.status(), Status::Disabled);
        s.set_enabled(false);
        assert_eq!(s.status(), Status::Disabled);
        assert!(
            matches!(s.ensure_running(), Err(HostError::Unavailable(m)) if m.contains("switched off"))
        );
        assert_eq!(s.sandbox(), SandboxSpec::locked());
        assert!(s.tasks().is_empty() && s.described().is_none());
    }
}
