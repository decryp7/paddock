//! Serving (feature 3, ADR 0005, ADR 0006): load a model on first use, keep several hot while they fit, release
//! them when idle or under pressure, queue requests by priority with caps, and never unload a busy model.
//! Every decision (what to evict, what to run next, what is idle) is `pitwall-core`'s; this module carries it out.

use crate::engines::{Engine, EngineError, Running};
use crate::library::{self, Library, LocalFile};
use crate::paths::now_ms;
use crate::system;
use pitwall_core::affinity::{self, SlotTable};
use pitwall_core::api::ApiError;
use pitwall_core::api::ErrorKind;
use pitwall_core::chat::{ChatEvent, ChatRequest, EmbedOutput, EmbedRequest, ModelRef};
use pitwall_core::config::Settings;
use pitwall_core::events::{EventKind, EventLog, Severity};
use pitwall_core::fit::{self, Room};
use pitwall_core::hardware::Hardware;
use pitwall_core::ids::{AppId, ModelId};
use pitwall_core::llama::{self, Launch, ServeKind};
use pitwall_core::memory::{self, Action, Hot, LoadPlan, ReleaseReason};
use pitwall_core::mgmt::{ChosenBy, HotState, LoadedModel, PresetStatus};
use pitwall_core::presets::{self, ModelPreset, PresetError, Presets};
use pitwall_core::scheduler::{Cancelled, Caps, JobId, JobKind, JobSpec, Priority, QueueError, Scheduler};
use pitwall_core::select::{self, Candidate, Requirements};
use pitwall_core::slots::Slots;
use pitwall_core::sse;
use pitwall_core::supervise::{Outcome, Supervisor};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;
use tokio::sync::{mpsc, Notify};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The engine failed or timed out while serving a request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct EngineFail(pub String);

/// A capability the engine build that serves a model cannot provide (ADR 0018).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocked {
    /// The capability.
    pub capability: pitwall_core::select::Capability,
    /// Why, in words that say what to do.
    pub reason: String,
    /// The llama.cpp release number that would serve it (`None`: no build of this kind can).
    pub min_build: Option<u64>,
}

/// Answers whether the engine build that serves a model can provide a capability. Implemented by the llama.cpp
/// manager; pack engines declare their own capabilities and are not gated.
pub trait CapabilityGate: Send + Sync {
    /// `Ok` when the build that serves `model` provides every capability in `needed`; else a typed, explained error.
    /// A machine with no engine installed passes (loading the model then explains that).
    fn check(&self, model: &str, needed: &[pitwall_core::select::Capability]) -> Result<(), ApiError>;
    /// The capabilities among `caps` that the build serving `model` cannot provide.
    fn blocked(&self, model: &str, caps: &[pitwall_core::select::Capability]) -> Vec<Blocked>;
}

/// The clock serving reads: milliseconds since the Unix epoch.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// One model in memory.
pub struct HotModel {
    /// Its id.
    pub id: ModelId,
    running: Running,
    /// Estimated resident RAM.
    pub ram: u64,
    /// Estimated VRAM.
    pub vram: u64,
    /// Context tokens per slot it was loaded with.
    pub context: u32,
    /// The context per slot Pitwall launched it with (what a client may send in one request).
    pub context_per_slot: u32,
    /// Parallel slots it was loaded with (a change in the preferences applies on the next load).
    pub slots: Slots,
    /// When it was loaded.
    pub loaded_at: u64,
    active: AtomicUsize,
    last_used: AtomicU64,
    /// The server's clock (see [`Clock`]).
    clock: Clock,
    /// The context (KV cache and prompt cache) was cleared while idle; reset by the next request.
    context_released: std::sync::atomic::AtomicBool,
    /// Vision input works.
    pub vision: bool,
    /// How long the load took.
    pub cold_start_ms: u64,
    /// The engine that serves it.
    pub engine: (String, pitwall_core::mgmt::EngineKind),
    /// What it can do.
    pub capabilities: Vec<pitwall_core::select::Capability>,
    /// Which slot holds which conversation's prompt prefix (ADR 0019).
    slot_table: Mutex<SlotTable>,
    /// Slots whose prefix belongs to a keep-warm preset: the file to save it to when the model unloads.
    warm: Mutex<HashMap<u32, String>>,
}

/// What a chat request tells Serving about itself beyond the request.
#[derive(Debug, Clone, Default)]
pub struct ChatHint {
    /// The calling app, for slot affinity (`None`: no affinity).
    pub app: Option<String>,
    /// The keep-warm file of the preset the request named, if the preset asks for keep warm.
    pub warm_file: Option<String>,
}

/// Frees a claimed slot when the request ends.
struct SlotGuard {
    model: Arc<HotModel>,
    slot: u32,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        lock(&self.model.slot_table).release(self.slot);
    }
}

impl HotModel {
    fn touch(&self) {
        self.last_used.store((self.clock)(), Ordering::SeqCst);
    }
    fn as_hot(&self, pinned: bool, idle_override: Option<u64>) -> Hot {
        Hot {
            id: self.id.clone(),
            ram: self.ram,
            vram: self.vram,
            pinned,
            active: self.active.load(Ordering::SeqCst),
            loading: false,
            last_used: self.last_used.load(Ordering::SeqCst),
            idle_override,
            context_released: self.context_released.load(Ordering::SeqCst),
        }
    }
}

/// Holds a model busy for the length of a request (a busy model is never unloaded).
pub struct Busy(Arc<HotModel>);

impl Busy {
    fn new(m: Arc<HotModel>) -> Busy {
        m.active.fetch_add(1, Ordering::SeqCst);
        m.context_released.store(false, Ordering::SeqCst);
        m.touch();
        Busy(m)
    }
    /// The engine client.
    pub fn client(&self) -> &pitwall_loopback::Client {
        &self.0.running.client
    }
    /// The model.
    pub fn model(&self) -> &HotModel {
        &self.0
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.touch();
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A turn in the queue: released on drop so the next waiting job can start.
pub struct Permit {
    id: JobId,
    serving: Serving,
}

impl Permit {
    /// The job id (for `DELETE /x/v1/jobs/{id}`).
    pub fn id(&self) -> JobId {
        self.id
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        lock(&self.serving.inner.sched).finish(self.id);
        self.serving.inner.wake.notify_waiters();
    }
}

struct Inner {
    engine: Arc<dyn Engine>,
    library: Library,
    settings: Arc<RwLock<Settings>>,
    hardware: RwLock<Hardware>,
    /// Live memory for load and release planning (`system::MemoryReader`); the host's unless a test pins a machine.
    memory: RwLock<system::MemoryReader>,
    /// Milliseconds since the Unix epoch as serving sees them (idle timers, backoff); the system clock unless a test sets one.
    clock: RwLock<Clock>,
    events: Arc<Mutex<EventLog>>,
    hot: Mutex<HashMap<ModelId, Arc<HotModel>>>,
    loading: Mutex<HashMap<ModelId, Arc<tokio::sync::Mutex<()>>>>,
    sched: Mutex<Scheduler>,
    wake: Notify,
    last_activity: AtomicU64,
    next_port: AtomicU64,
    cancelled: Mutex<Vec<JobId>>,
    supervisor: Mutex<Supervisor>,
    /// Measured generation speed by model id (from stored benchmarks), for "fast" selection.
    speeds: RwLock<HashMap<String, f64>>,
    /// Model presets and aliases (ADR 0014); `AppState` loads, saves and replaces them.
    presets: RwLock<Presets>,
    /// Gates requests by what the llama.cpp build in use supports (ADR 0018); set once by `AppState`.
    gate: RwLock<Option<Arc<dyn CapabilityGate>>>,
    /// Where saved prompt prefixes live (`--slot-save-path`); set by `AppState`.
    slots_dir: RwLock<Option<std::path::PathBuf>>,
    /// Serving was stopped because Pitwall was quit with "stop serving" (it is paused as well; opening Pitwall resumes).
    stopped: std::sync::atomic::AtomicBool,
}

/// The serving core.
#[derive(Clone)]
pub struct Serving {
    inner: Arc<Inner>,
}

/// A model resolved for a request.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// The file.
    pub id: ModelId,
    /// How it was chosen.
    pub chosen_by: ChosenBy,
    /// The preset the request named, when it asked for an alias (ADR 0014).
    pub preset: Option<ModelPreset>,
}

/// What a preset changes about loading a model (the context).
#[derive(Debug, Clone, Default)]
struct LoadOverrides {
    context: Option<u32>,
}

/// The installed model a request's `model` text names (id, repository or `repository:quant`).
fn find_model(local: &[LocalFile], name: &str) -> Option<ModelId> {
    let n = name.trim().trim_end_matches(".gguf").to_ascii_lowercase();
    local
        .iter()
        .find(|f| {
            f.id.as_str() == n
                || f.repo.to_ascii_lowercase() == n
                || format!("{}:{}", f.repo, f.info.quant.clone().unwrap_or_default()).to_ascii_lowercase() == n
        })
        .map(|f| f.id.clone())
}

fn api(kind: ErrorKind, m: impl Into<String>) -> ApiError {
    ApiError::new(kind, m)
}

impl Serving {
    /// Serving over an engine, a library and the shared settings.
    pub fn new(
        engine: Arc<dyn Engine>,
        library: Library,
        settings: Arc<RwLock<Settings>>,
        hardware: Hardware,
        events: Arc<Mutex<EventLog>>,
    ) -> Serving {
        let caps = caps_of(&settings.read().map(|s| s.clone()).unwrap_or_default());
        Serving {
            inner: Arc::new(Inner {
                engine,
                library,
                settings,
                hardware: RwLock::new(hardware),
                memory: RwLock::new(system::host_memory()),
                clock: RwLock::new(Arc::new(now_ms) as Clock),
                events,
                hot: Mutex::new(HashMap::new()),
                loading: Mutex::new(HashMap::new()),
                sched: Mutex::new(Scheduler::new(caps)),
                wake: Notify::new(),
                last_activity: AtomicU64::new(now_ms()),
                next_port: AtomicU64::new(0),
                cancelled: Mutex::new(vec![]),
                supervisor: Mutex::new(Supervisor::default()),
                speeds: RwLock::new(HashMap::new()),
                presets: RwLock::new(Presets::default()),
                gate: RwLock::new(None),
                slots_dir: RwLock::new(None),
                stopped: std::sync::atomic::AtomicBool::new(false),
            }),
        }
    }

    fn settings(&self) -> Settings {
        self.inner.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    /// Live memory as serving plans with it.
    pub(crate) fn memory_now(&self) -> pitwall_core::memory::System {
        let reader = self
            .inner
            .memory
            .read()
            .map(|r| r.clone())
            .unwrap_or_else(|p| p.into_inner().clone());
        reader(&self.hw())
    }

    /// Replaces where live memory is read from (tests pin a machine; the default is the host).
    pub fn set_memory_reader(&self, reader: system::MemoryReader) {
        match self.inner.memory.write() {
            Ok(mut g) => *g = reader,
            Err(p) => *p.into_inner() = reader,
        }
    }

    /// Replaces the clock the idle timers and restart backoff read (tests move time by hand instead of sleeping).
    pub fn set_clock(&self, clock: Clock) {
        match self.inner.clock.write() {
            Ok(mut g) => *g = clock,
            Err(p) => *p.into_inner() = clock,
        }
    }

    fn clock(&self) -> Clock {
        self.inner
            .clock
            .read()
            .map(|c| c.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }

    fn now(&self) -> u64 {
        (self.clock())()
    }

    fn hw(&self) -> Hardware {
        self.inner.hardware.read().map(|h| h.clone()).unwrap_or_default()
    }

    fn event(&self, kind: EventKind, sev: Severity, msg: String) {
        lock(&self.inner.events).push(self.now(), kind, sev, msg);
    }

    /// Apply changed settings (caps, policy).
    pub fn settings_changed(&self) {
        lock(&self.inner.sched).set_caps(caps_of(&self.settings()));
        self.inner.wake.notify_waiters();
    }

    /// Pause or resume accepting work. Resuming also ends a "stopped" state.
    pub fn set_paused(&self, paused: bool) {
        if !paused {
            self.inner.stopped.store(false, Ordering::SeqCst);
        }
        lock(&self.inner.sched).set_paused(paused);
        self.inner.wake.notify_waiters();
    }

    /// Whether serving was stopped by quitting Pitwall (models unloaded, engines ended, requests refused until Pitwall
    /// is opened or serving is resumed).
    pub fn stopped(&self) -> bool {
        self.inner.stopped.load(Ordering::SeqCst)
    }

    /// Stops serving: pauses it, unloads every model and ends every engine process. Each engine is asked to end
    /// (SIGTERM on Unix) and killed when it has not ended after `grace`; where there is no such signal (Windows) it is
    /// killed at once. Returns how many models were unloaded.
    pub async fn stop_all(&self, grace: Duration) -> usize {
        self.inner.stopped.store(true, Ordering::SeqCst);
        lock(&self.inner.sched).set_paused(true);
        self.inner.wake.notify_waiters();
        let models: Vec<Arc<HotModel>> = lock(&self.inner.hot).drain().map(|(_, h)| h).collect();
        // Each engine by its identity, read while its handle is held (rule 53).
        let ids: Vec<paddock_proc::ProcessId> = models
            .iter()
            .filter_map(|h| h.running.child.as_ref().and_then(tokio::process::Child::id))
            .filter_map(paddock_proc::ProcessId::of)
            .collect();
        self.event(
            EventKind::ServingChanged,
            Severity::Info,
            format!("Pitwall was quit with \"stop serving\": unloading {} model(s)", models.len()),
        );
        #[cfg(unix)]
        {
            for id in &ids {
                system::terminate(*id, false);
            }
            let deadline = std::time::Instant::now() + grace;
            while std::time::Instant::now() < deadline && ids.iter().any(|id| system::pid_alive(id.pid())) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            for id in ids.iter().filter(|id| system::pid_alive(id.pid())) {
                system::terminate(*id, true);
            }
        }
        #[cfg(not(unix))]
        let _ = (&ids, grace);
        let n = models.len();
        // Dropping the handles kills anything that is left (`kill_on_drop`).
        drop(models);
        n
    }

    /// Whether serving is paused.
    pub fn paused(&self) -> bool {
        lock(&self.inner.sched).paused()
    }

    /// Queue depth and running count.
    pub fn queue(&self) -> (pitwall_core::scheduler::Depth, usize) {
        let s = lock(&self.inner.sched);
        (s.depth(), s.running().count())
    }

    /// The waiting and running jobs.
    pub fn jobs(&self) -> Vec<pitwall_core::scheduler::Job> {
        let s = lock(&self.inner.sched);
        s.running().chain(s.waiting()).cloned().collect()
    }

    /// Cancel a queued job (a running one is stopped by its connection closing).
    pub fn cancel(&self, id: JobId) -> Cancelled {
        let c = lock(&self.inner.sched).cancel(id);
        if c == Cancelled::Running {
            lock(&self.inner.cancelled).push(id);
        }
        self.inner.wake.notify_waiters();
        c
    }

    /// Whether a running job was asked to stop.
    pub fn was_cancelled(&self, id: JobId) -> bool {
        lock(&self.inner.cancelled).contains(&id)
    }

    /// The presets (a copy).
    pub fn presets(&self) -> Presets {
        self.inner.presets.read().map(|p| p.clone()).unwrap_or_default()
    }

    /// Installs the capability gate for llama.cpp models.
    pub fn set_gate(&self, gate: Arc<dyn CapabilityGate>) {
        if let Ok(mut g) = self.inner.gate.write() {
            *g = Some(gate);
        }
    }

    /// The gate: what the llama.cpp build in use supports.
    fn gate_for(&self) -> Option<Arc<dyn CapabilityGate>> {
        self.inner.gate.read().ok().and_then(|g| g.clone())
    }

    /// Refuses a request that needs what the engine build serving the resolved model lacks: a 503 that names the
    /// build needed (too old), a 400 when no build of this kind can, or a 503 for a broken pin.
    pub fn check_capabilities(&self, r: &Resolved, needed: &[select::Capability]) -> Result<(), ApiError> {
        if needed.is_empty() {
            return Ok(());
        }
        match self.gate_for() {
            Some(g) => g.check(r.id.as_str(), needed),
            None => Ok(()),
        }
    }

    /// The capabilities among `caps` that the build serving `id` cannot provide.
    pub fn blocked(&self, id: &ModelId, caps: &[select::Capability]) -> Vec<Blocked> {
        self.gate_for().map_or_else(Vec::new, |g| g.blocked(id.as_str(), caps))
    }

    /// Replaces the presets (after they were validated and saved).
    pub fn set_presets(&self, presets: Presets) {
        if let Ok(mut g) = self.inner.presets.write() {
            *g = presets;
        }
    }

    /// Picks a model for capability requirements among the installed ones (shared by `auto:` names and presets).
    fn choose_auto(
        &self,
        mut req: Requirements,
        (needs_vision, needs_tools): (bool, bool),
        local: &[LocalFile],
        hot: &HashMap<ModelId, Arc<HotModel>>,
        s: &Settings,
    ) -> Result<select::Choice, String> {
        if needs_vision && !req.capabilities.contains(&select::Capability::Vision) {
            req.capabilities.push(select::Capability::Vision);
        }
        if needs_tools && !req.capabilities.contains(&select::Capability::Tools) {
            req.capabilities.push(select::Capability::Tools);
        }
        let room = system::room_from(&self.memory_now(), &self.hw(), hot.values().map(|h| h.ram).sum());
        let speeds = self.inner.speeds.read().map(|g| g.clone()).unwrap_or_default();
        let cands: Vec<Candidate> = local
            .iter()
            .map(|f| Candidate {
                id: f.id.clone(),
                // Capabilities the engine build that would serve the model lacks do not count: a model is not
                // chosen for what it cannot do here.
                capabilities: {
                    let caps = library::capabilities(f);
                    let out = self.blocked(&f.id, &caps);
                    caps.into_iter().filter(|c| !out.iter().any(|b| b.capability == *c)).collect()
                },
                context_max: f.info.context_length.map(|c| c.min(u64::from(u32::MAX)) as u32),
                size: f.size,
                loaded: hot.contains_key(&f.id),
                gen_tps: speeds.get(f.id.as_str()).copied(),
                fit: fit::assess(Some(&library::size_of(f)), s.models.default_context, &room).verdict,
            })
            .collect();
        select::choose(&req, &cands)
    }

    /// What a preset resolves to now.
    fn resolve_preset(
        &self,
        p: &ModelPreset,
        needs: (bool, bool),
        local: &[LocalFile],
        hot: &HashMap<ModelId, Arc<HotModel>>,
        s: &Settings,
    ) -> Result<presets::Resolution, PresetError> {
        let installed: Vec<ModelId> = local.iter().map(|f| f.id.clone()).collect();
        presets::resolve(p, &installed, |req| self.choose_auto(req.clone(), needs, local, hot, s))
    }

    /// Where a preset (possibly a draft) stands now: resolved, stale, matching nothing, or shadowed by a model.
    pub fn preset_status(&self, p: &ModelPreset) -> PresetStatus {
        let local = self.inner.library.local();
        if find_model(&local, p.alias.as_str()).is_some() {
            return PresetStatus::Shadowed {
                message: format!(
                    "a model named \"{}\" is installed, so requests for that name go to the model, not to this preset; rename the preset",
                    p.alias
                ),
            };
        }
        let s = self.settings();
        let hot = lock(&self.inner.hot);
        let outcome = self.resolve_preset(p, (false, false), &local, &hot, &s);
        PresetStatus::of(outcome, |id| hot.keys().any(|k| k.as_str() == id))
    }

    /// Resolve the request's model: a named file, a preset (alias), `auto:<capabilities>` or the default. A real
    /// model's name wins over a preset of the same name.
    pub fn resolve(&self, model: &ModelRef, needs_vision: bool, needs_tools: bool) -> Result<Resolved, ApiError> {
        let local = self.inner.library.local();
        let s = self.settings();
        let hot = lock(&self.inner.hot);
        let is_auto = |n: &str| n.starts_with("auto:") || n == "auto";
        if let ModelRef::Named(n) = model {
            let alias = if is_auto(n) || find_model(&local, n).is_some() {
                None
            } else {
                self.inner.presets.read().ok().and_then(|g| g.get(n).cloned())
            };
            if let Some(p) = alias {
                let r = self
                    .resolve_preset(&p, (needs_vision, needs_tools), &local, &hot, &s)
                    .map_err(ApiError::from)?;
                return Ok(Resolved {
                    id: r.model,
                    chosen_by: r.chosen_by,
                    preset: Some(p),
                });
            }
        }
        let chat_models: Vec<&LocalFile> = local
            .iter()
            .filter(|f| library::capabilities(f).contains(&select::Capability::Chat))
            .collect();
        if chat_models.is_empty() && !matches!(model, ModelRef::Named(n) if find_model(&local, n).is_some()) {
            return Err(api(
                ErrorKind::NotFound,
                "no models are installed. Open Models in Pitwall, search Hugging Face and download one.",
            ));
        }
        match model {
            ModelRef::Named(n) if is_auto(n) => {
                let spec = n.strip_prefix("auto:").unwrap_or("");
                let req = Requirements::parse_auto(spec).map_err(|e| api(ErrorKind::InvalidRequest, e))?;
                let c = self
                    .choose_auto(req, (needs_vision, needs_tools), &local, &hot, &s)
                    .map_err(|e| api(ErrorKind::NotFound, e))?;
                Ok(Resolved {
                    id: c.id,
                    chosen_by: ChosenBy::Capabilities,
                    preset: None,
                })
            }
            ModelRef::Named(n) => match find_model(&local, n) {
                Some(id) => Ok(Resolved {
                    id,
                    chosen_by: ChosenBy::Explicit,
                    preset: None,
                }),
                None => Err(api(
                    ErrorKind::NotFound,
                    format!(
                        "there is no model named \"{n}\". Installed: {}",
                        local.iter().map(|f| f.id.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                )),
            },
            ModelRef::Default => {
                let pick = s
                    .models
                    .default_model
                    .clone()
                    .filter(|d| local.iter().any(|f| &f.id == d))
                    .or_else(|| hot.keys().next().cloned())
                    .or_else(|| chat_models.first().map(|f| f.id.clone()));
                pick.map(|id| Resolved {
                    id,
                    chosen_by: ChosenBy::Default,
                    preset: None,
                })
                .ok_or_else(|| api(ErrorKind::NotFound, "no chat model is installed"))
            }
        }
    }

    /// Wait for a turn in the queue. `on_queued` is told the job id if the job has to wait.
    pub async fn acquire(
        &self,
        app: AppId,
        model: ModelId,
        priority: Priority,
        kind: JobKind,
        app_cap: Option<usize>,
        on_queued: Option<tokio::sync::oneshot::Sender<u64>>,
    ) -> Result<Permit, ApiError> {
        let mut on_queued = on_queued;
        let slots = self.slots_for(&model);
        let id = {
            let mut sched = lock(&self.inner.sched);
            sched.set_model_slots(&model, slots.count());
            sched
                .submit(
                    JobSpec {
                        app,
                        model: Some(model),
                        priority,
                        kind,
                        app_cap,
                    },
                    self.now(),
                )
                .map_err(|e| match e {
                    QueueError::Full(_) => api(ErrorKind::Overloaded, e.to_string()),
                    QueueError::Paused => api(ErrorKind::Overloaded, e.to_string()),
                })?
        };
        let permit = Permit { id, serving: self.clone() };
        loop {
            let notified = self.inner.wake.notified();
            tokio::pin!(notified);
            // Enable before checking so a wake between the check and the await is not lost.
            notified.as_mut().enable();
            {
                let mut s = lock(&self.inner.sched);
                let started = s.dispatch(self.now());
                if started.contains(&id) || s.job(id).is_some_and(|j| j.state == pitwall_core::scheduler::JobState::Running) {
                    if !started.is_empty() {
                        self.inner.wake.notify_waiters();
                    }
                    return Ok(permit);
                }
                if s.job(id).is_none() {
                    return Err(api(ErrorKind::Cancelled, "the request was cancelled while it waited"));
                }
                // Still waiting: tell the caller the job's id once, so it can answer at once and be cancelled by id.
                if let Some(tx) = on_queued.take() {
                    let _ = tx.send(id);
                }
            }
            tokio::select! { () = &mut notified => {}, () = tokio::time::sleep(Duration::from_millis(500)) => {} }
        }
    }

    /// The engine launch for `context` tokens per slot in each of `slots` slots (`--ctx-size` is the total).
    fn launch_for(&self, f: &LocalFile, context: u32, slots: Slots, port: u16) -> Launch {
        let hw = self.hw();
        Launch {
            model: f.path.to_string_lossy().into_owned(),
            mmproj: f.projector.as_ref().map(|p| p.to_string_lossy().into_owned()),
            alias: f.id.to_string(),
            port,
            context: slots.launch_context(context),
            gpu_layers: None,
            threads: 0,
            parallel: slots.get(),
            kind: if f.info.kind == pitwall_core::gguf::ModelKind::Embedding {
                ServeKind::Embedding
            } else {
                ServeKind::Chat
            },
            cache_ram_mib: affinity::prompt_cache_mib(hw.total_ram, self.settings().cache.prompt_cache_mib),
            slot_save_path: self.slots_dir().map(|p| p.to_string_lossy().into_owned()),
        }
    }

    async fn free_port(&self) -> u16 {
        for _ in 0..50 {
            if let Ok(l) = tokio::net::TcpListener::bind("127.0.0.1:0").await {
                if let Ok(a) = l.local_addr() {
                    self.inner.next_port.fetch_add(1, Ordering::SeqCst);
                    return a.port();
                }
            }
        }
        0
    }

    /// Make sure `id` is loaded (loading it first if needed) and mark it busy. Returns the cold-start time in ms
    /// when the model had to be loaded.
    pub async fn ensure(&self, id: &ModelId) -> Result<(Busy, Option<u64>), ApiError> {
        self.ensure_as(id, false, &LoadOverrides::default()).await
    }

    /// [`Serving::ensure`] for a resolved request: when it came through a preset, the preset's context
    /// applies if the model has to be loaded (a model that is already loaded keeps what it was loaded with).
    pub async fn ensure_for(&self, r: &Resolved) -> Result<(Busy, Option<u64>), ApiError> {
        let o = LoadOverrides {
            context: r.preset.as_ref().and_then(|p| p.context),
        };
        let (busy, cold) = self.ensure_as(&r.id, false, &o).await?;
        if cold.is_some() {
            if let Some(file) = self.warm_file_for(r) {
                self.restore_warm(&busy, &file).await;
            }
        }
        Ok((busy, cold))
    }

    /// Where saved prompt prefixes live.
    pub fn slots_dir(&self) -> Option<std::path::PathBuf> {
        self.inner.slots_dir.read().ok().and_then(|g| g.clone())
    }

    /// Sets where saved prompt prefixes live (and creates the folder, owner-only on Unix).
    pub fn set_slots_dir(&self, dir: std::path::PathBuf) {
        let _ = std::fs::create_dir_all(&dir);
        crate::paths::restrict_dir(&dir);
        if let Ok(mut g) = self.inner.slots_dir.write() {
            *g = Some(dir);
        }
    }

    /// What identifies the thing that computes an answer for `r`: the model file, the engine and its build. Cache keys
    /// and keep-warm files change with it, so nothing stale is ever served.
    pub fn fingerprint(&self, r: &Resolved) -> String {
        let version = self
            .inner
            .library
            .local()
            .into_iter()
            .find(|f| f.id == r.id)
            .map_or_else(String::new, |f| format!("{}-{}", f.size, f.modified));
        let (engine, build) = (self.inner.engine.id(), self.inner.engine.build_tag(r.id.as_str()));
        pitwall_core::cache::fingerprint(r.id.as_str(), &version, &engine, &build)
    }

    /// The keep-warm file of the preset a request named, when the preset asks for keep warm.
    pub fn warm_file_for(&self, r: &Resolved) -> Option<String> {
        let p = r.preset.as_ref().filter(|p| p.keep_warm)?;
        self.slots_dir()?;
        Some(crate::warm::file_name(
            r.id.as_str(),
            p.alias.as_str(),
            &self.fingerprint(r),
            p.system_prompt.as_deref().unwrap_or(""),
        ))
    }

    /// Restores a saved prompt prefix into slot 0 of a just-loaded model. A file the engine refuses is removed.
    async fn restore_warm(&self, busy: &Busy, file: &str) {
        let Some(dir) = self.slots_dir() else { return };
        let path = dir.join(file);
        if !path.is_file() {
            return;
        }
        let body = serde_json::to_vec(&serde_json::json!({ "filename": file })).unwrap_or_default();
        let ok = matches!(
            busy.client().send("POST", "/slots/0?action=restore", Some(body), Duration::from_secs(30)).await,
            Ok(r) if r.status == 200
        );
        if ok {
            crate::warm::touch(&path);
            lock(&busy.model().warm).insert(0, file.to_string());
            self.event(
                EventKind::ContextReleased,
                Severity::Info,
                format!("Restored the saved prompt prefix for {}.", busy.model().id),
            );
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }

    /// Saves the prefixes of keep-warm slots of a model that is being unloaded; the model stays alive until they are
    /// written (or 20 s pass), then it is dropped.
    fn save_warm(&self, h: Arc<HotModel>) {
        let Some(dir) = self.slots_dir() else { return };
        let jobs: Vec<(u32, String)> = if h.context_released.load(Ordering::SeqCst) {
            vec![]
        } else {
            lock(&h.warm).iter().map(|(s, f)| (*s, f.clone())).collect()
        };
        if jobs.is_empty() || !dir.is_dir() {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else { return };
        let me = self.clone();
        rt.spawn(async move {
            for (slot, file) in jobs {
                let body = serde_json::to_vec(&serde_json::json!({ "filename": file })).unwrap_or_default();
                let path = format!("/slots/{slot}?action=save");
                match h.running.client.send("POST", &path, Some(body), Duration::from_secs(20)).await {
                    Ok(r) if r.status == 200 => {}
                    _ => {
                        let _ = std::fs::remove_file(dir.join(&file));
                    }
                }
            }
            me.event(
                EventKind::ContextReleased,
                Severity::Info,
                format!("Saved the prompt prefixes of {}.", h.id),
            );
        });
    }

    /// [`Serving::ensure`]; `auto_restart` marks a background restart, whose failed start plans the next attempt.
    async fn ensure_as(&self, id: &ModelId, auto_restart: bool, o: &LoadOverrides) -> Result<(Busy, Option<u64>), ApiError> {
        let gate = lock(&self.inner.loading).entry(id.clone()).or_default().clone();
        let _g = gate.lock().await;
        self.inner.last_activity.store(self.now(), Ordering::SeqCst);
        if let Some(h) = lock(&self.inner.hot).get(id).cloned() {
            return Ok((Busy::new(h), None));
        }
        if let Some(why) = lock(&self.inner.supervisor).failure(id) {
            return Err(api(ErrorKind::Engine, why));
        }
        let started = std::time::Instant::now();
        let f = self
            .inner
            .library
            .local()
            .into_iter()
            .find(|f| &f.id == id)
            .ok_or_else(|| api(ErrorKind::NotFound, format!("the model {id} is no longer installed")))?;
        let s = self.settings();
        let prefs = s.prefs(id);
        let context = pitwall_core::slots::context_per_slot(o.context, prefs.context, s.models.default_context, f.info.context_length);
        let slots = s.slots_for(id);
        let hw = self.hw();
        let policy = s.memory.policy();
        // Plan: does it fit, what must go first. The KV cache holds every slot's context.
        let tokens = u64::from(slots.launch_context(context));
        let (need, _offload) = {
            let hot = lock(&self.inner.hot);
            let room = Room {
                ram_releasable: hot.values().map(|h| h.ram).sum(),
                ..system::room_from(&self.memory_now(), &hw, 0)
            };
            fit::need_tokens(&library::size_of(&f), tokens, &room)
        };
        for _ in 0..40 {
            let (plan, sys) = {
                let hot = lock(&self.inner.hot);
                let hots: Vec<Hot> = hot
                    .values()
                    .map(|h| h.as_hot(s.prefs(&h.id).pinned, s.prefs(&h.id).idle_secs.map(|x| x * 1000)))
                    .collect();
                let sys = self.memory_now();
                (memory::plan_load(id, need, &hots, &sys, &policy), sys)
            };
            match plan {
                LoadPlan::Proceed { .. } => break,
                LoadPlan::Evict { models, .. } => {
                    for m in models {
                        self.unload(&m, ReleaseReason::Pressure);
                    }
                    break;
                }
                LoadPlan::Wait => tokio::time::sleep(Duration::from_millis(500)).await,
                LoadPlan::Refuse(r) => {
                    self.event(EventKind::RequestFailed, Severity::Warn, r.to_string());
                    let _ = sys;
                    return Err(api(ErrorKind::InsufficientMemory, r.to_string()));
                }
            }
        }
        self.event(EventKind::EngineStarted, Severity::Info, format!("Loading {id}"));
        let engine = self.inner.engine.clone();
        // llama.cpp installs itself when no build is installed: the load waits for it (or fails with its reason).
        engine.ready().await.map_err(|e| match e {
            EngineError::Install(m) => api(ErrorKind::Engine, m),
            e => api(ErrorKind::Engine, e.to_string()),
        })?;
        // The port is probed and released before the engine binds it, so another program can take it in between:
        // an "address in use" failure gets a fresh port (a few times) instead of failing the request.
        let mut attempt = 0;
        let started_engine = loop {
            let port = self.free_port().await;
            let launch = self.launch_for(&f, context, slots, port);
            match engine.start(&launch).await {
                Err(e) if attempt < 3 && port_taken(&e) => attempt += 1,
                other => break other,
            }
        };
        let running = started_engine.map_err(|e| {
            self.event(EventKind::EngineFailed, Severity::Error, format!("{id}: {e}"));
            if lock(&self.inner.supervisor).crashed(id, self.now(), auto_restart) == Outcome::GaveUp {
                self.event(
                    EventKind::EngineFailed,
                    Severity::Error,
                    format!("{id} failed to start repeatedly; Pitwall stopped trying. Press Retry on the Dashboard once it is fixed."),
                );
            }
            match e {
                EngineError::NotInstalled | EngineError::NotAvailable(_) => api(ErrorKind::Engine, e.to_string()),
                EngineError::Pin(_) => api(ErrorKind::EnginePin, e.to_string()),
                _ => api(ErrorKind::Engine, format!("could not load {id}: {e}")),
            }
        })?;
        let pid = running.child.as_ref().and_then(tokio::process::Child::id).unwrap_or(0);
        let measured = if pid > 0 { system::memory(pid) } else { 0 };
        let vision = running.props.vision.unwrap_or(f.projector.is_some());
        let h = Arc::new(HotModel {
            id: id.clone(),
            ram: accounted_ram(measured, need.ram),
            vram: need.vram,
            context: running.props.n_ctx.unwrap_or(context),
            context_per_slot: context,
            slots,
            running,
            loaded_at: self.now(),
            active: AtomicUsize::new(0),
            last_used: AtomicU64::new(self.now()),
            clock: self.clock(),
            context_released: std::sync::atomic::AtomicBool::new(false),
            vision,
            engine: (engine.id(), engine.kind()),
            cold_start_ms: started.elapsed().as_millis() as u64,
            capabilities: library::capabilities(&f),
            slot_table: Mutex::new(SlotTable::new(slots.count())),
            warm: Mutex::new(HashMap::new()),
        });
        lock(&self.inner.hot).insert(id.clone(), h.clone());
        lock(&self.inner.sched).set_model_slots(id, slots.count());
        self.inner.wake.notify_waiters();
        lock(&self.inner.supervisor).started(id);
        let ms = started.elapsed().as_millis() as u64;
        self.event(
            EventKind::ModelLoaded,
            Severity::Info,
            format!("Loaded {id} in {:.1} s", ms as f64 / 1000.0),
        );
        Ok((Busy::new(h), Some(ms)))
    }

    /// Sets the measured speeds selection prefers (from the stored benchmarks).
    pub fn set_speeds(&self, speeds: HashMap<String, f64>) {
        if let Ok(mut g) = self.inner.speeds.write() {
            *g = speeds;
        }
    }

    /// Replaces the restart limits (tests use short delays).
    pub fn set_restart_limits(&self, limits: pitwall_core::backoff::Limits) {
        *lock(&self.inner.supervisor) = Supervisor::new(limits);
    }

    /// The user pressed Retry (or loads the model by hand): forget the crash history so the next load is tried.
    pub fn retry(&self, id: &ModelId) {
        lock(&self.inner.supervisor).clear(id);
    }

    /// Crashed engines being restarted or given up on, for the dashboard.
    pub fn issues(&self) -> Vec<pitwall_core::mgmt::EngineIssue> {
        lock(&self.inner.supervisor).issues()
    }

    /// Unload a model now (a busy model is refused).
    pub fn unload(&self, id: &ModelId, why: ReleaseReason) -> bool {
        let mut hot = lock(&self.inner.hot);
        match hot.get(id) {
            Some(h) if h.active.load(Ordering::SeqCst) == 0 => {
                let gone = hot.remove(id);
                drop(hot);
                if let Some(h) = gone {
                    self.save_warm(h);
                }
                self.event(EventKind::ModelUnloaded, Severity::Info, format!("Released {id} ({why:?})"));
                true
            }
            _ => false,
        }
    }

    /// Release the idle models the policy picks, and forget engines whose process died. Called every few seconds.
    pub fn tick(&self) -> Vec<Action> {
        let s = self.settings();
        let now = self.now();
        let dead: Vec<ModelId> = lock(&self.inner.hot)
            .iter()
            .filter(|(_, h)| h.running.child.is_some() && !h.running.client.port().get().eq(&0) && h.running_exited())
            .map(|(k, _)| k.clone())
            .collect();
        for d in dead {
            lock(&self.inner.hot).remove(&d);
            let outcome = lock(&self.inner.supervisor).crashed(&d, now, true);
            let msg = match outcome {
                Outcome::RestartIn(ms) => format!("The engine for {d} exited unexpectedly; restarting in {} s.", ms.div_ceil(1000)),
                _ => format!("The engine for {d} exited unexpectedly and keeps crashing; Pitwall stopped restarting it."),
            };
            self.event(EventKind::EngineFailed, Severity::Error, msg);
        }
        {
            let alive: Vec<ModelId> = lock(&self.inner.hot).keys().cloned().collect();
            let mut sup = lock(&self.inner.supervisor);
            for id in &alive {
                sup.healthy(id, now);
            }
        }
        let due = lock(&self.inner.supervisor).due(now);
        for id in due {
            self.restart(id);
        }
        let hots: Vec<Hot> = lock(&self.inner.hot)
            .values()
            .map(|h| h.as_hot(s.prefs(&h.id).pinned, s.prefs(&h.id).idle_secs.map(|x| x * 1000)))
            .collect();
        let sys = self.memory_now();
        let actions = memory::plan_release(
            now,
            &hots,
            &sys,
            &s.memory.policy(),
            self.inner.last_activity.load(Ordering::SeqCst),
        );
        // What was actually done: a context that was already released (or whose model became busy) is not in the list.
        let mut taken = Vec::new();
        for a in actions {
            match &a {
                Action::Unload(m, why) => {
                    self.unload(m, *why);
                    taken.push(a);
                }
                Action::ReleaseContext(m, why) => {
                    if self.release_context(m, *why) {
                        taken.push(a);
                    }
                }
            }
        }
        taken
    }

    /// Clears an idle model's context before its weights are released: the engine's slots are erased (KV cache and
    /// cached prompts) in the background. A model that became busy meanwhile is left alone.
    /// `true` when a release was started, `false` when the model is gone, busy or was released already.
    fn release_context(&self, id: &ModelId, why: ReleaseReason) -> bool {
        let Some(h) = lock(&self.inner.hot).get(id).cloned() else {
            return false;
        };
        if h.active.load(Ordering::SeqCst) > 0 || h.context_released.swap(true, Ordering::SeqCst) {
            return false;
        }
        let slots = h.slots.get();
        let me = self.clone();
        tokio::spawn(async move {
            for slot in 0..slots {
                if h.active.load(Ordering::SeqCst) > 0 {
                    return;
                }
                let path = format!("/slots/{slot}?action=erase");
                if let Err(e) = h.running.client.send("POST", &path, None, Duration::from_secs(5)).await {
                    me.event(
                        EventKind::ContextReleased,
                        Severity::Warn,
                        format!("Could not clear the context of {}: {e}", h.id),
                    );
                    return;
                }
            }
            lock(&h.slot_table).forget();
            lock(&h.warm).clear();
            me.event(
                EventKind::ContextReleased,
                Severity::Info,
                format!("Cleared the context of {} ({why:?}); its weights stay loaded.", h.id),
            );
        });
        true
    }

    /// Restarts a crashed model in the background; a failed attempt is another crash (backoff, then the loop guard).
    fn restart(&self, id: ModelId) {
        let me = self.clone();
        tokio::spawn(async move {
            match me.ensure_as(&id, true, &LoadOverrides::default()).await {
                Ok((busy, _)) => {
                    drop(busy);
                    me.event(EventKind::ModelLoaded, Severity::Info, format!("Restarted the engine for {id}."));
                }
                Err(e) => {
                    // A failed start already counted toward the loop guard and planned the next attempt (or gave up).
                    me.event(
                        EventKind::EngineFailed,
                        Severity::Error,
                        format!("Restarting {id} failed: {}", e.message),
                    );
                }
            }
        });
    }

    /// Run the periodic release loop until the process ends.
    pub fn spawn_tick_loop(&self) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                crate::os_signals::refresh().await;
                me.tick();
            }
        })
    }

    /// Models in memory, for the dashboard and `/api/ps`.
    pub fn loaded(&self) -> Vec<LoadedModel> {
        let s = self.settings();
        let sys = self.memory_now();
        let policy = s.memory.policy();
        let now = self.now();
        lock(&self.inner.hot)
            .values()
            .map(|h| {
                let hot = h.as_hot(s.prefs(&h.id).pinned, s.prefs(&h.id).idle_secs.map(|x| x * 1000));
                let busy = h.active.load(Ordering::SeqCst) > 0;
                LoadedModel {
                    model_id: h.id.to_string(),
                    name: h.id.to_string(),
                    engine: h.engine.1,
                    build: h.running.props.build.clone().unwrap_or_else(|| h.engine.0.clone()),
                    state: if busy { HotState::Busy } else { HotState::Ready },
                    ram_bytes: h.ram,
                    vram_bytes: h.vram,
                    context_tokens: h.context,
                    slots: h.slots.get(),
                    pinned: hot.pinned,
                    active_requests: h.active.load(Ordering::SeqCst) as u32,
                    loaded_at: h.loaded_at,
                    last_used_at: h.last_used.load(Ordering::SeqCst),
                    idle_release_at: memory::idle_release_at(&hot, &sys, &policy).filter(|t| *t > now),
                    cold_start_ms: h.cold_start_ms,
                    capabilities: h.capabilities.clone(),
                }
            })
            .collect()
    }

    /// The slots a model serves with: the ones it is loaded with, else its preference or the default. A loaded model
    /// keeps its slots until it is reloaded, so a changed preference never disturbs running work.
    pub fn slots_for(&self, id: &ModelId) -> Slots {
        self.loaded_slots(id).unwrap_or_else(|| self.settings().slots_for(id))
    }

    /// The context each slot gets (or has, when loaded) for a model: what a client may put in one request.
    pub fn context_per_slot(&self, id: &ModelId) -> Option<u32> {
        if let Some(h) = lock(&self.inner.hot).get(id) {
            return Some(h.context_per_slot);
        }
        let f = self.inner.library.local().into_iter().find(|f| &f.id == id)?;
        let s = self.settings();
        Some(pitwall_core::slots::context_per_slot(
            None,
            s.prefs(id).context,
            s.models.default_context,
            f.info.context_length,
        ))
    }

    /// The slots a model is loaded with (`None`: not loaded).
    pub fn loaded_slots(&self, id: &ModelId) -> Option<Slots> {
        lock(&self.inner.hot).get(id).map(|h| h.slots)
    }

    /// Whether a model is in memory.
    pub fn is_loaded(&self, id: &ModelId) -> bool {
        lock(&self.inner.hot).contains_key(id)
    }

    /// The resident footprint (RAM plus video memory, an estimate) of a loaded model; 0 when it is not loaded.
    pub fn footprint(&self, id: &ModelId) -> u64 {
        lock(&self.inner.hot).get(id).map_or(0, |h| h.ram.saturating_add(h.vram))
    }

    /// RAM and VRAM of the loaded models.
    pub fn hot_ram(&self) -> (u64, u64) {
        lock(&self.inner.hot).values().fold((0, 0), |a, h| (a.0 + h.ram, a.1 + h.vram))
    }

    /// Unload everything idle (the "Release memory now" action). Returns how many were released.
    pub fn release_all(&self) -> usize {
        let ids: Vec<ModelId> = lock(&self.inner.hot).keys().cloned().collect();
        ids.iter().filter(|i| self.unload(i, ReleaseReason::Manual)).count()
    }

    /// Run a chat request on a loaded model, streaming events. The permit and busy marks live as long as the stream.
    pub fn chat(
        &self,
        busy: Busy,
        permit: Permit,
        req: ChatRequest,
        timeout: Duration,
        hint: &ChatHint,
    ) -> mpsc::Receiver<Result<ChatEvent, EngineFail>> {
        let (tx, rx) = mpsc::channel(64);
        // Slot affinity (ADR 0019): the conversation's requests go to the slot that holds its prompt prefix. Only
        // llama.cpp understands `id_slot`; when every slot is busy the engine chooses.
        let claim = hint
            .app
            .as_deref()
            .filter(|_| self.settings().cache.prefix_affinity)
            .and_then(|app| lock(&busy.model().slot_table).claim(affinity::prefix_key(app, &req)));
        let slot_guard = claim.map(|c| SlotGuard {
            model: busy.0.clone(),
            slot: c.slot,
        });
        if let Some(c) = claim {
            let mut w = lock(&busy.model().warm);
            match &hint.warm_file {
                Some(f) => w.insert(c.slot, f.clone()),
                None => w.remove(&c.slot),
            };
        }
        let body = llama::chat_body(&req, busy.model().id.as_str(), claim.map(|c| c.slot));
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        let me = self.clone();
        tokio::spawn(async move {
            let _keep = (&permit, &busy, &slot_guard);
            let mut resp = match busy.client().send("POST", "/v1/chat/completions", Some(bytes), timeout).await {
                Ok(r) => r,
                Err(e) => {
                    me.event(EventKind::EngineFailed, Severity::Warn, format!("{}: {e}", busy.model().id));
                    let _ = tx.send(Err(EngineFail(e.to_string()))).await;
                    return;
                }
            };
            if resp.status != 200 {
                let status = resp.status;
                let raw = resp.bytes(64 * 1024).await.unwrap_or_default();
                let msg = serde_json::from_slice::<serde_json::Value>(&raw)
                    .ok()
                    .and_then(|v| v.pointer("/error/message").and_then(|m| m.as_str()).map(str::to_string))
                    .unwrap_or_else(|| format!("the engine answered {status}"));
                let _ = tx.send(Err(EngineFail(msg))).await;
                return;
            }
            let mut dec = sse::Decoder::new();
            let mut stream = llama::Stream::new();
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                let chunk = match tokio::time::timeout_at(deadline, resp.chunk()).await {
                    Err(_) => {
                        let _ = tx.send(Err(EngineFail("the request timed out".into()))).await;
                        return;
                    }
                    Ok(Err(e)) => {
                        let _ = tx.send(Err(EngineFail(e.to_string()))).await;
                        return;
                    }
                    Ok(Ok(None)) => break,
                    Ok(Ok(Some(c))) => c,
                };
                let events = match dec.feed(&chunk) {
                    Ok(e) => e,
                    Err(e) => {
                        let _ = tx.send(Err(EngineFail(e.to_string()))).await;
                        return;
                    }
                };
                for ev in events {
                    if ev.data.trim() == "[DONE]" {
                        continue;
                    }
                    match stream.chunk(&ev.data) {
                        Ok(out) => {
                            for o in out {
                                if tx.send(Ok(o)).await.is_err() {
                                    return; // the client went away: dropping the response cancels generation
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(EngineFail(e.to_string()))).await;
                            return;
                        }
                    }
                }
            }
            if let Some(last) = stream.finish() {
                let _ = tx.send(Ok(last)).await;
            }
        });
        rx
    }

    /// Embeddings on a loaded embedding model.
    pub async fn embed(&self, busy: &Busy, req: &EmbedRequest, timeout: Duration) -> Result<EmbedOutput, EngineFail> {
        let body = llama::embed_body(&req.input, busy.model().id.as_str());
        let (status, v) = busy
            .client()
            .json("POST", "/v1/embeddings", Some(&body), timeout)
            .await
            .map_err(|e| EngineFail(e.to_string()))?;
        if status != 200 {
            return Err(EngineFail(
                v.pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("the engine refused the request")
                    .to_string(),
            ));
        }
        llama::parse_embeddings(&v, req.input.len()).map_err(|e| EngineFail(e.to_string()))
    }
}

impl HotModel {
    fn running_exited(&self) -> bool {
        // `try_wait` needs &mut; a process that exited refuses connections, which the request path also handles.
        // Use the OS: a dead pid means the engine is gone.
        self.running
            .child
            .as_ref()
            .and_then(tokio::process::Child::id)
            .is_none_or(|pid| !system::pid_alive(pid))
    }
}

/// Scheduler caps from the settings.
pub fn caps_of(s: &Settings) -> Caps {
    Caps {
        max_running: s.server.max_concurrent.max(1) as usize,
        per_model: s.models.default_slots.count(),
        ..Caps::default()
    }
}

/// Whether a failed start was the engine's port being taken by someone else in the meantime.
fn port_taken(e: &EngineError) -> bool {
    let EngineError::Start(m) = e else { return false };
    let m = m.to_lowercase();
    m.contains("address already in use") || m.contains("address in use") || m.contains("only one usage of each socket address")
}

/// The RAM a hot model is accounted with: what the engine process really uses (`system::memory`: footprint or PSS, never
/// the resident size) once it is known, but never below the estimate made before loading, which also covers memory the
/// engine has not touched yet. `0` measured (unreadable) keeps the estimate.
fn accounted_ram(measured: u64, estimate: u64) -> u64 {
    if measured > 0 {
        measured.max(estimate)
    } else {
        estimate
    }
}

#[cfg(test)]
mod port_tests {
    use super::*;

    #[test]
    fn a_hot_model_is_accounted_by_its_measured_footprint_never_below_the_estimate() {
        const MB: u64 = 1 << 20;
        // A fake reader that measures the footprint (not the resident size, which would be 1,965 MB here).
        let measured = system::memory_with(42, |_| Some(535 * MB));
        assert_eq!(accounted_ram(measured, 400 * MB), 535 * MB);
        assert_eq!(accounted_ram(measured, 900 * MB), 900 * MB, "the estimate is the floor");
        assert_eq!(
            accounted_ram(system::memory_with(43, |_| None), 400 * MB),
            400 * MB,
            "unreadable keeps the estimate"
        );
    }

    #[test]
    fn a_taken_port_is_recognised_on_every_platform() {
        for m in [
            "x: Address already in use (os error 98)",
            "x: Address already in use (os error 48)",
            "Only one usage of each socket address (protocol/network address/port) is normally permitted. (os error 10048)",
        ] {
            assert!(port_taken(&EngineError::Start(m.into())), "{m}");
        }
        assert!(!port_taken(&EngineError::Start("exited during start".into())));
        assert!(!port_taken(&EngineError::NotInstalled));
    }
}
