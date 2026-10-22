//! One plugin under guard: started lazily on first use in its sandbox, checked (describe, configure), called with a
//! deadline, and on any failure of the plugin itself (crash, hang, garbage, memory cap) stopped, charged a failure,
//! restarted after a backoff on a later call, or quarantined ([`crate::policy`]). Everything here blocks; the host
//! runs it on blocking threads. Copied in spirit from IMT's `imt-ipc::supervisor`.

use crate::config::HostConfig;
use crate::discover::Discovered;
use crate::error::{ErrorKind, PluginError};
use crate::events::{EventKind, HostEvent};
use crate::policy::Ledger;
use pitwall_ipc::peer::{code as ipc_code, NoCalls};
use pitwall_ipc::process::SpawnSpec;
use pitwall_ipc::sandbox::Note;
use pitwall_ipc::{wire, Child, ChildSpec, IpcError, Json, SandboxSpec, StreamEvent};
use pitwall_plugin_api::{Description, GenerateItem, Reply, Request, CONTRACT_VERSION, DATA_DIR_ENV};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

/// The longest backoff a call waits out itself; a longer one fails the call with [`ErrorKind::Restarting`].
const WAIT_AT_MOST: Duration = Duration::from_secs(1);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// What the user's settings mean for one plugin.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Effective {
    pub enabled: bool,
    pub network: bool,
    pub config: serde_json::Value,
}

/// A running plugin process.
pub(crate) struct Live {
    pub child: Child,
    pub generation: u64,
}

/// The state of a plugin, as the UI shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Not running (started on first use).
    Stopped,
    /// Running.
    Running,
    /// Stopped after repeated failures until the user resets it.
    Quarantined,
    /// Disabled by the user.
    Disabled,
}

/// One plugin's status.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatus {
    /// Its id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// What it does.
    pub description: String,
    /// What it contributes.
    pub kinds: Vec<pitwall_plugin_api::Kind>,
    /// Its state.
    pub state: State,
    /// Its process id while it runs.
    pub pid: Option<u32>,
    /// Failures within the failure window.
    pub failures: usize,
    /// Restarts after failures.
    pub restarts: u32,
    /// Why it is quarantined, or its last failure.
    pub reason: Option<String>,
    /// Whether its manifest asks for the network.
    pub network_requested: bool,
    /// Whether it runs with the network (asked for and granted by the user).
    pub network_granted: bool,
    /// Its memory cap in MiB.
    pub memory_mb: u64,
    /// How this platform sandboxes it.
    pub sandbox: Vec<Note>,
    /// Its tools and endpoints, once it has run.
    pub offers: Option<Description>,
}

/// A loaded model of an engine plugin; stale once the engine restarted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineModel {
    /// The engine plugin.
    pub plugin: String,
    /// The engine's handle.
    pub handle: String,
    /// The engine process it was loaded into.
    pub generation: u64,
}

/// What a generation yields.
#[derive(Debug, Clone, PartialEq)]
pub enum GenerateEvent {
    /// Generated text.
    Text(String),
    /// The end: the engine's summary (finish reason, token counts).
    Done(serde_json::Value),
}

pub(crate) struct Instance {
    pub found: Discovered,
    config: Arc<HostConfig>,
    settings: Mutex<Effective>,
    live: Mutex<Option<Arc<Live>>>,
    launching: Mutex<()>,
    ledger: Mutex<Ledger>,
    generation: AtomicU64,
    offers: Mutex<Option<Description>>,
    events: broadcast::Sender<HostEvent>,
}

impl Instance {
    pub fn new(found: Discovered, config: Arc<HostConfig>, settings: Effective, events: broadcast::Sender<HostEvent>) -> Instance {
        Instance {
            found,
            config,
            settings: Mutex::new(settings),
            live: Mutex::new(None),
            launching: Mutex::new(()),
            ledger: Mutex::new(Ledger::default()),
            generation: AtomicU64::new(0),
            offers: Mutex::new(None),
            events,
        }
    }

    pub fn id(&self) -> &str {
        &self.found.manifest.id
    }

    fn label(&self) -> String {
        format!("plugin \"{}\"", self.id())
    }

    fn err(&self, kind: ErrorKind, message: impl Into<String>) -> PluginError {
        PluginError::new(self.id(), kind, message)
    }

    fn emit(&self, kind: EventKind) {
        let _ = self.events.send(HostEvent::now(self.id(), kind));
    }

    fn sandbox(&self, network: bool) -> SandboxSpec {
        SandboxSpec {
            name: format!("{}.{}", self.config.sandbox_prefix, self.id()),
            memory_limit: Some(self.found.memory_limit),
            network: self.found.manifest.permissions.network && network,
            children: false,
            read: vec![self.found.folder.clone()],
            write: vec![self.data_dir()],
        }
    }

    fn data_dir(&self) -> std::path::PathBuf {
        self.config.data_dir.join(self.id())
    }

    /// Why the plugin may not be called now, if it may not.
    fn refusal(&self) -> Option<PluginError> {
        if !lock(&self.settings).enabled {
            return Some(self.err(ErrorKind::Disabled, format!("{} is disabled", self.label())));
        }
        lock(&self.ledger)
            .quarantined()
            .map(|why| self.err(ErrorKind::Quarantined, format!("{} {why}", self.label())))
    }

    /// The running process, starting one if there is none (after the backoff owed).
    fn lease(&self) -> Result<Arc<Live>, PluginError> {
        if let Some(e) = self.refusal() {
            return Err(e);
        }
        if let Some(l) = self.current_alive() {
            return Ok(l);
        }
        let _one = lock(&self.launching);
        if let Some(l) = self.current_alive() {
            return Ok(l);
        }
        let wait = lock(&self.ledger).retry_after(&self.config.policy, Instant::now());
        if wait > WAIT_AT_MOST {
            return Err(PluginError::restarting(self.id(), wait));
        }
        std::thread::sleep(wait);
        if let Some(e) = self.refusal() {
            return Err(e);
        }
        match self.start() {
            Ok(live) => {
                let live = Arc::new(live);
                if live.generation > 1 {
                    lock(&self.ledger).count_restart();
                }
                *lock(&self.live) = Some(live.clone());
                self.emit(EventKind::Started { pid: live.child.pid() });
                Ok(live)
            }
            Err(e) => {
                if e.is_failure() {
                    self.charge(&e);
                }
                Err(e)
            }
        }
    }

    /// The live process, if it is still connected; one that died while idle is charged a failure.
    fn current_alive(&self) -> Option<Arc<Live>> {
        let live = lock(&self.live).clone()?;
        if !live.child.peer().is_closed() {
            return Some(live);
        }
        let e = self.classify(live.child.peer().closed().unwrap_or(IpcError::Died("closed".into())), Some(&live.child));
        self.fail(&live, &e);
        None
    }

    fn start(&self) -> Result<Live, PluginError> {
        let effective = lock(&self.settings).clone();
        let data = self.data_dir();
        std::fs::create_dir_all(&data)
            .map_err(|e| self.err(ErrorKind::Spawn, format!("cannot create the data folder of {}: {e}", self.label())))?;
        let m = &self.found.manifest;
        let mut spawn = SpawnSpec::new(&self.found.exe, self.label(), self.sandbox(effective.network));
        spawn.args = m.args.clone();
        spawn.env = vec![(DATA_DIR_ENV.to_string(), data.to_string_lossy().into_owned())];
        let mut spec = ChildSpec::new(spawn);
        spec.peer.contract = CONTRACT_VERSION;
        spec.peer.max_frame = self.config.max_frame;
        let child = pitwall_ipc::launch(&spec, Arc::new(NoCalls)).map_err(|e| self.classify(e, None))?;
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let t = self.config.timeouts.start;
        let description = match self.request_on(&child, &Request::Describe, t)? {
            Reply::Described(d) => d,
            other => return Err(self.err(ErrorKind::Protocol, format!("{} answered Describe with {other:?}", self.label()))),
        };
        if description.id != m.id || description.kinds != m.kinds || description.contract != m.contract {
            return Err(self.err(
                ErrorKind::Protocol,
                format!(
                    "{} describes itself as \"{}\" ({:?}, contract {}), which is not what its manifest says",
                    self.label(),
                    description.id,
                    description.kinds,
                    description.contract
                ),
            ));
        }
        let configure = Request::Configure {
            settings: Json(effective.config),
        };
        match self.request_on(&child, &configure, t)? {
            Reply::Configured => {}
            other => return Err(self.err(ErrorKind::Protocol, format!("{} answered Configure with {other:?}", self.label()))),
        }
        *lock(&self.offers) = Some(description);
        Ok(Live { child, generation })
    }

    /// One call on `child`, classified (used while starting, before the child is shared).
    fn request_on(&self, child: &Child, request: &Request, timeout: Duration) -> Result<Reply, PluginError> {
        let body = self.encode(request)?;
        let reply = child.peer().call(body, Some(timeout)).map_err(|e| self.classify(e, Some(child)))?;
        wire::decode::<Reply>(&reply).map_err(|e| self.err(ErrorKind::Protocol, format!("{} answered with {e}", self.label())))
    }

    fn encode(&self, request: &Request) -> Result<Vec<u8>, PluginError> {
        let body = wire::encode(request).map_err(|e| self.err(ErrorKind::TooLarge, e))?;
        // Room for the frame's own fields.
        if body.len().saturating_add(64) > pitwall_ipc::frame::clamp_max(self.config.max_frame) {
            return Err(self.err(
                ErrorKind::TooLarge,
                format!("the request ({} bytes) is larger than a plugin may receive", body.len()),
            ));
        }
        Ok(body)
    }

    /// An IPC error as a plugin error.
    fn classify(&self, e: IpcError, child: Option<&Child>) -> PluginError {
        let label = self.label();
        match e {
            IpcError::Spawn(m) => self.err(ErrorKind::Spawn, m),
            IpcError::Died(m) => match child {
                Some(c) if c.process().stopped_over_memory() => self.err(ErrorKind::MemoryCap, c.exit_text()),
                Some(c) => self.err(ErrorKind::Crashed, format!("{label} crashed: {}", c.exit_text())),
                None => self.err(ErrorKind::Crashed, format!("{label} crashed: {m}")),
            },
            IpcError::Timeout => self.err(ErrorKind::Timeout, format!("{label} did not answer in time and was stopped")),
            IpcError::Protocol(m) => self.err(ErrorKind::Protocol, format!("{label} broke the protocol: {m}")),
            IpcError::Remote(r) if r.code == ipc_code::PANIC => self.err(ErrorKind::Crashed, format!("{label} {}", r.message)),
            IpcError::Remote(r) => self.err(ErrorKind::Plugin { code: r.code }, r.message),
        }
    }

    /// Charges a failure (quarantining when it is one too many).
    fn charge(&self, e: &PluginError) {
        let quarantined = lock(&self.ledger).record_failure(&self.config.policy, Instant::now(), &e.message);
        self.emit(EventKind::Failed { message: e.message.clone() });
        if quarantined {
            let reason = lock(&self.ledger).quarantined().unwrap_or_default().to_string();
            self.emit(EventKind::Quarantined { reason });
        }
    }

    /// The plugin failed on `live`: stop that process (once, however many calls saw it) and charge the failure.
    fn fail(&self, live: &Arc<Live>, e: &PluginError) {
        let dead = {
            let mut current = lock(&self.live);
            match current.as_ref() {
                Some(l) if l.generation == live.generation => current.take(),
                _ => None,
            }
        };
        if let Some(l) = dead {
            l.child.stop();
            self.charge(e);
        }
    }

    /// One call with a deadline.
    pub fn call(&self, request: &Request, timeout: Duration) -> Result<Reply, PluginError> {
        let body = self.encode(request)?;
        let live = self.lease()?;
        self.call_live(&live, body, timeout)
    }

    fn call_live(&self, live: &Arc<Live>, body: Vec<u8>, timeout: Duration) -> Result<Reply, PluginError> {
        let result = live
            .child
            .peer()
            .call(body, Some(timeout))
            .map_err(|e| self.classify(e, Some(&live.child)))
            .and_then(|b| {
                wire::decode::<Reply>(&b).map_err(|e| self.err(ErrorKind::Protocol, format!("{} answered with {e}", self.label())))
            });
        if let Err(e) = &result {
            if e.is_failure() {
                self.fail(live, e);
            }
        }
        result
    }

    /// A reply that is not the one `request` expects is a protocol failure.
    pub fn unexpected(&self, live_generation: Option<u64>, what: &str, reply: &Reply) -> PluginError {
        let e = self.err(ErrorKind::Protocol, format!("{} answered {what} with {reply:?}", self.label()));
        if let Some(g) = live_generation {
            let live = lock(&self.live).clone();
            if let Some(l) = live.filter(|l| l.generation == g) {
                self.fail(&l, &e);
            }
        }
        e
    }

    /// The generation of the running process, if any.
    pub fn generation(&self) -> Option<u64> {
        lock(&self.live).as_ref().map(|l| l.generation)
    }

    /// Loads a model (engine).
    pub fn engine_load(&self, request: &Request, timeout: Duration) -> Result<EngineModel, PluginError> {
        let body = self.encode(request)?;
        let live = self.lease()?;
        match self.call_live(&live, body, timeout)? {
            Reply::Loaded { handle } => Ok(EngineModel {
                plugin: self.id().to_string(),
                handle,
                generation: live.generation,
            }),
            other => Err(self.unexpected(Some(live.generation), "EngineLoad", &other)),
        }
    }

    /// The live process a model was loaded into, or why it is gone.
    fn live_for(&self, model: &EngineModel) -> Result<Arc<Live>, PluginError> {
        if let Some(e) = self.refusal() {
            return Err(e);
        }
        match self.current_alive() {
            Some(l) if l.generation == model.generation => Ok(l),
            _ => Err(self.err(
                ErrorKind::StaleHandle,
                format!("{} restarted since the model was loaded; load it again", self.label()),
            )),
        }
    }

    /// Unloads a model (engine).
    pub fn engine_unload(&self, model: &EngineModel, timeout: Duration) -> Result<(), PluginError> {
        let body = self.encode(&Request::EngineUnload {
            handle: model.handle.clone(),
        })?;
        let live = self.live_for(model)?;
        match self.call_live(&live, body, timeout)? {
            Reply::Unloaded => Ok(()),
            other => Err(self.unexpected(Some(live.generation), "EngineUnload", &other)),
        }
    }

    /// Generates (engine): items go to `tx` until the end, a failure, or the receiver going away (which cancels).
    pub fn engine_generate(&self, model: &EngineModel, request: serde_json::Value, tx: mpsc::Sender<Result<GenerateEvent, PluginError>>) {
        let send_err = |e: PluginError| {
            let _ = tx.blocking_send(Err(e));
        };
        let body = match self.encode(&Request::EngineGenerate {
            handle: model.handle.clone(),
            request: Json(request),
        }) {
            Ok(b) => b,
            Err(e) => return send_err(e),
        };
        let live = match self.live_for(model) {
            Ok(l) => l,
            Err(e) => return send_err(e),
        };
        let mut stream = match live.child.peer().stream(body, pitwall_ipc::peer::DEFAULT_CREDIT * 4) {
            Ok(s) => s,
            Err(e) => {
                let e = self.classify(e, Some(&live.child));
                if e.is_failure() {
                    self.fail(&live, &e);
                }
                return send_err(e);
            }
        };
        let idle = self.config.timeouts.generate_idle;
        loop {
            let event = match stream.next(Some(idle)) {
                Ok(StreamEvent::Item(b)) => match wire::decode::<GenerateItem>(&b) {
                    Ok(GenerateItem::Text(t)) => Ok(GenerateEvent::Text(t)),
                    Err(m) => Err(self.err(ErrorKind::Protocol, format!("{} sent an item that is not valid: {m}", self.label()))),
                },
                Ok(StreamEvent::Done(b)) => match wire::decode::<Reply>(&b) {
                    Ok(Reply::Generated(summary)) => Ok(GenerateEvent::Done(summary.into_inner())),
                    Ok(other) => Err(self.err(ErrorKind::Protocol, format!("{} ended a generation with {other:?}", self.label()))),
                    Err(m) => Err(self.err(ErrorKind::Protocol, format!("{} answered with {m}", self.label()))),
                },
                Ok(StreamEvent::Idle) => {
                    stream.cancel();
                    Err(self.err(
                        ErrorKind::Timeout,
                        format!("{} sent nothing for {} s and was stopped", self.label(), idle.as_secs()),
                    ))
                }
                Err(e) => Err(self.classify(e, Some(&live.child))),
            };
            let end = !matches!(event, Ok(GenerateEvent::Text(_)));
            if let Err(e) = &event {
                if e.is_failure() {
                    self.fail(&live, e);
                }
            }
            if tx.blocking_send(event).is_err() {
                // The caller went away: dropping the stream cancels the call.
                return;
            }
            if end {
                return;
            }
        }
    }

    /// Applies the user's settings; a change stops the running process (the next call starts it with them).
    pub fn apply(&self, effective: Effective) {
        let changed = {
            let mut s = lock(&self.settings);
            let changed = *s != effective;
            *s = effective;
            changed
        };
        if changed {
            self.stop();
        }
    }

    /// Lifts the quarantine.
    pub fn reset(&self) {
        lock(&self.ledger).reset();
    }

    /// Stops the running process, if any.
    pub fn stop(&self) {
        let taken = lock(&self.live).take();
        if let Some(l) = taken {
            l.child.stop();
            self.emit(EventKind::Stopped);
        }
    }

    /// The last lines the plugin wrote to standard error (diagnostics only).
    pub fn stderr_tail(&self) -> Vec<String> {
        lock(&self.live).as_ref().map(|l| l.child.process().stderr_tail()).unwrap_or_default()
    }

    pub fn status(&self) -> PluginStatus {
        let m = &self.found.manifest;
        let effective = lock(&self.settings).clone();
        let live = lock(&self.live).clone();
        let (quarantined, failures, restarts, last) = {
            let mut l = lock(&self.ledger);
            let failures = l.recent(&self.config.policy, Instant::now());
            (l.quarantined().map(String::from), failures, l.restarts(), l.last_failure().map(String::from))
        };
        let state = if !effective.enabled {
            State::Disabled
        } else if quarantined.is_some() {
            State::Quarantined
        } else if live.as_ref().is_some_and(|l| !l.child.peer().is_closed()) {
            State::Running
        } else {
            State::Stopped
        };
        let sandbox = self.sandbox(effective.network);
        PluginStatus {
            id: m.id.clone(),
            name: m.name.clone(),
            version: m.version.clone(),
            description: m.description.clone(),
            kinds: m.kinds.clone(),
            state,
            pid: live.as_ref().map(|l| l.child.pid()),
            failures,
            restarts,
            reason: quarantined.or(last),
            network_requested: m.permissions.network,
            network_granted: sandbox.network,
            memory_mb: self.found.memory_limit >> 20,
            sandbox: sandbox.enforcement(),
            offers: lock(&self.offers).clone(),
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        self.stop();
    }
}
