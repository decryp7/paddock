//! Serving (feature 3, ADR 0005, ADR 0006): load a model on first use, keep several hot while they fit, release
//! them when idle or under pressure, queue requests by priority with caps, and never unload a busy model.
//! Every decision (what to evict, what to run next, what is idle) is `pitwall-core`'s; this module carries it out.

use crate::engines::{Engine, EngineError, Running};
use crate::library::{self, Library, LocalFile};
use crate::paths::now_ms;
use crate::system;
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

/// One model in memory.
pub struct HotModel {
    /// Its id.
    pub id: ModelId,
    running: Running,
    /// Estimated resident RAM.
    pub ram: u64,
    /// Estimated VRAM.
    pub vram: u64,
    /// Context tokens it was loaded with.
    pub context: u32,
    /// When it was loaded.
    pub loaded_at: u64,
    active: AtomicUsize,
    last_used: AtomicU64,
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
}

impl HotModel {
    fn touch(&self) {
        self.last_used.store(now_ms(), Ordering::SeqCst);
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
    packs: RwLock<HashMap<String, Arc<dyn Engine>>>,
    library: Library,
    settings: Arc<RwLock<Settings>>,
    hardware: RwLock<Hardware>,
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

/// What a preset changes about loading a model (the context and the engine).
#[derive(Debug, Clone, Default)]
struct LoadOverrides {
    context: Option<u32>,
    engine: Option<String>,
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
                packs: RwLock::new(HashMap::new()),
                library,
                settings,
                hardware: RwLock::new(hardware),
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
            }),
        }
    }

    /// Register the engines declared by packs (models choose one through their preferences).
    pub fn set_pack_engines(&self, engines: Vec<Arc<dyn Engine>>) {
        if let Ok(mut g) = self.inner.packs.write() {
            *g = engines.into_iter().map(|e| (e.id(), e)).collect();
        }
    }

    fn engine_for(&self, id: Option<&str>) -> Result<Arc<dyn Engine>, ApiError> {
        match id {
            None | Some("llama.cpp") => Ok(self.inner.engine.clone()),
            Some(i) => self.inner.packs.read().ok().and_then(|g| g.get(i).cloned()).ok_or_else(|| {
                api(
                    ErrorKind::NotAvailable,
                    format!("the engine pack \"{i}\" is not loaded; fix or remove the model's engine choice in Models"),
                )
            }),
        }
    }

    fn settings(&self) -> Settings {
        self.inner.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    fn hw(&self) -> Hardware {
        self.inner.hardware.read().map(|h| h.clone()).unwrap_or_default()
    }

    fn event(&self, kind: EventKind, sev: Severity, msg: String) {
        lock(&self.inner.events).push(now_ms(), kind, sev, msg);
    }

    /// Apply changed settings (caps, policy).
    pub fn settings_changed(&self) {
        lock(&self.inner.sched).set_caps(caps_of(&self.settings()));
        self.inner.wake.notify_waiters();
    }

    /// Pause or resume accepting work.
    pub fn set_paused(&self, paused: bool) {
        lock(&self.inner.sched).set_paused(paused);
        self.inner.wake.notify_waiters();
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
        let room = system::room(&self.hw(), hot.values().map(|h| h.ram).sum());
        let speeds = self.inner.speeds.read().map(|g| g.clone()).unwrap_or_default();
        let cands: Vec<Candidate> = local
            .iter()
            .map(|f| Candidate {
                id: f.id.clone(),
                capabilities: library::capabilities(f),
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

    /// Wait for a turn in the queue.
    pub async fn acquire(
        &self,
        app: AppId,
        model: ModelId,
        priority: Priority,
        kind: JobKind,
        app_cap: Option<usize>,
    ) -> Result<Permit, ApiError> {
        let id = lock(&self.inner.sched)
            .submit(
                JobSpec {
                    app,
                    model: Some(model),
                    priority,
                    kind,
                    app_cap,
                },
                now_ms(),
            )
            .map_err(|e| match e {
                QueueError::Full(_) => api(ErrorKind::Overloaded, e.to_string()),
                QueueError::Paused => api(ErrorKind::Overloaded, e.to_string()),
            })?;
        let permit = Permit { id, serving: self.clone() };
        loop {
            let notified = self.inner.wake.notified();
            tokio::pin!(notified);
            // Enable before checking so a wake between the check and the await is not lost.
            notified.as_mut().enable();
            {
                let mut s = lock(&self.inner.sched);
                let started = s.dispatch(now_ms());
                if started.contains(&id) || s.job(id).is_some_and(|j| j.state == pitwall_core::scheduler::JobState::Running) {
                    if !started.is_empty() {
                        self.inner.wake.notify_waiters();
                    }
                    return Ok(permit);
                }
                if s.job(id).is_none() {
                    return Err(api(ErrorKind::Overloaded, "the request was cancelled while it waited"));
                }
            }
            tokio::select! { () = &mut notified => {}, () = tokio::time::sleep(Duration::from_millis(500)) => {} }
        }
    }

    fn launch_for(&self, f: &LocalFile, context: u32, port: u16) -> Launch {
        let hw = self.hw();
        Launch {
            model: f.path.to_string_lossy().into_owned(),
            mmproj: f.projector.as_ref().map(|p| p.to_string_lossy().into_owned()),
            alias: f.id.to_string(),
            port,
            context,
            gpu_layers: None,
            threads: 0,
            parallel: self.settings().server.max_concurrent.clamp(1, 4),
            kind: if f.info.kind == pitwall_core::gguf::ModelKind::Embedding {
                ServeKind::Embedding
            } else {
                ServeKind::Chat
            },
            cache_ram_mib: if hw.total_ram >= 16 << 30 { 1024 } else { 256 },
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

    /// [`Serving::ensure`] for a resolved request: when it came through a preset, the preset's context and engine
    /// apply if the model has to be loaded (a model that is already loaded keeps what it was loaded with).
    pub async fn ensure_for(&self, r: &Resolved) -> Result<(Busy, Option<u64>), ApiError> {
        let o = LoadOverrides {
            context: r.preset.as_ref().and_then(|p| p.context),
            engine: r.preset.as_ref().and_then(|p| p.engine.clone()),
        };
        self.ensure_as(&r.id, false, &o).await
    }

    /// [`Serving::ensure`]; `auto_restart` marks a background restart, whose failed start plans the next attempt.
    async fn ensure_as(&self, id: &ModelId, auto_restart: bool, o: &LoadOverrides) -> Result<(Busy, Option<u64>), ApiError> {
        let gate = lock(&self.inner.loading).entry(id.clone()).or_default().clone();
        let _g = gate.lock().await;
        self.inner.last_activity.store(now_ms(), Ordering::SeqCst);
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
        let context = o
            .context
            .or(prefs.context)
            .unwrap_or(s.models.default_context)
            .min(f.info.context_length.unwrap_or(u64::MAX).min(u64::from(u32::MAX)) as u32)
            .max(512);
        let hw = self.hw();
        let policy = s.memory.policy();
        // Plan: does it fit, what must go first.
        let (need, _offload) = {
            let hot = lock(&self.inner.hot);
            let room = Room {
                ram_releasable: hot.values().map(|h| h.ram).sum(),
                ..system::room(&hw, 0)
            };
            fit::need(&library::size_of(&f), context, &room)
        };
        for _ in 0..40 {
            let (plan, sys) = {
                let hot = lock(&self.inner.hot);
                let hots: Vec<Hot> = hot
                    .values()
                    .map(|h| h.as_hot(s.prefs(&h.id).pinned, s.prefs(&h.id).idle_secs.map(|x| x * 1000)))
                    .collect();
                let sys = system::read_system(&hw);
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
        let port = self.free_port().await;
        let launch = self.launch_for(&f, context, port);
        self.event(EventKind::EngineStarted, Severity::Info, format!("Loading {id}"));
        let engine = self.engine_for(o.engine.as_deref().or(prefs.engine.as_deref()))?;
        let running = engine.start(&launch).await.map_err(|e| {
            self.event(EventKind::EngineFailed, Severity::Error, format!("{id}: {e}"));
            if lock(&self.inner.supervisor).crashed(id, now_ms(), auto_restart) == Outcome::GaveUp {
                self.event(
                    EventKind::EngineFailed,
                    Severity::Error,
                    format!("{id} failed to start repeatedly; Pitwall stopped trying. Press Retry on the Dashboard once it is fixed."),
                );
            }
            match e {
                EngineError::NotInstalled | EngineError::NotAvailable(_) => api(ErrorKind::Engine, e.to_string()),
                _ => api(ErrorKind::Engine, format!("could not load {id}: {e}")),
            }
        })?;
        let pid = running.child.as_ref().and_then(tokio::process::Child::id).unwrap_or(0);
        let rss = if pid > 0 { system::rss(pid) } else { 0 };
        let vision = running.props.vision.unwrap_or(f.projector.is_some());
        let h = Arc::new(HotModel {
            id: id.clone(),
            ram: if rss > 0 { rss.max(need.ram) } else { need.ram },
            vram: need.vram,
            context: running.props.n_ctx.unwrap_or(context),
            running,
            loaded_at: now_ms(),
            active: AtomicUsize::new(0),
            last_used: AtomicU64::new(now_ms()),
            context_released: std::sync::atomic::AtomicBool::new(false),
            vision,
            engine: (engine.id(), engine.kind()),
            cold_start_ms: started.elapsed().as_millis() as u64,
            capabilities: library::capabilities(&f),
        });
        lock(&self.inner.hot).insert(id.clone(), h.clone());
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
                hot.remove(id);
                drop(hot);
                self.event(EventKind::ModelUnloaded, Severity::Info, format!("Released {id} ({why:?})"));
                true
            }
            _ => false,
        }
    }

    /// Release the idle models the policy picks, and forget engines whose process died. Called every few seconds.
    pub fn tick(&self) -> Vec<Action> {
        let s = self.settings();
        let now = now_ms();
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
        let sys = system::read_system(&self.hw());
        let actions = memory::plan_release(
            now,
            &hots,
            &sys,
            &s.memory.policy(),
            self.inner.last_activity.load(Ordering::SeqCst),
        );
        for a in &actions {
            match a {
                Action::Unload(m, why) => {
                    self.unload(m, *why);
                }
                Action::ReleaseContext(m, why) => self.release_context(m, *why),
            }
        }
        actions
    }

    /// Clears an idle model's context before its weights are released: the engine's slots are erased (KV cache and
    /// cached prompts) in the background. A model that became busy meanwhile is left alone.
    fn release_context(&self, id: &ModelId, why: ReleaseReason) {
        let Some(h) = lock(&self.inner.hot).get(id).cloned() else { return };
        if h.active.load(Ordering::SeqCst) > 0 || h.context_released.swap(true, Ordering::SeqCst) {
            return;
        }
        let slots = self.settings().server.max_concurrent.clamp(1, 4);
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
            me.event(
                EventKind::ContextReleased,
                Severity::Info,
                format!("Cleared the context of {} ({why:?}); its weights stay loaded.", h.id),
            );
        });
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
        let sys = system::read_system(&self.hw());
        let policy = s.memory.policy();
        let now = now_ms();
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
    pub fn chat(&self, busy: Busy, permit: Permit, req: ChatRequest, timeout: Duration) -> mpsc::Receiver<Result<ChatEvent, EngineFail>> {
        let (tx, rx) = mpsc::channel(64);
        let body = llama::chat_body(&req, busy.model().id.as_str());
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        let me = self.clone();
        tokio::spawn(async move {
            let _keep = (&permit, &busy);
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
        per_model: (s.server.max_concurrent.max(1) as usize).min(4),
        ..Caps::default()
    }
}
