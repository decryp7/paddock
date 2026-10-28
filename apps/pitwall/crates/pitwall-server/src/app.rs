//! The application state shared by every route: settings, the managers, the live feed, and the request pipeline
//! (authorise, rate-limit, resolve the model, queue, load, run, record) all API families go through.

use crate::auth::{Auth, Principal};
use crate::caches::Caches;
use crate::egress::Egress;
use crate::engines::{Engine, LlamaCpp};
use crate::library::Library;
use crate::paths::{self, now_ms, Dirs};
use crate::serving::{EngineFail, Resolved, Serving};
use crate::system;
use pitwall_core::api::{ApiError, ApiFamily, ErrorKind};
use pitwall_core::chat::{ChatEvent, ChatRequest, ModelRef};
use pitwall_core::config::{self, Settings};
use pitwall_core::events::{EventKind, EventLog, Severity};
use pitwall_core::hardware::Hardware;
use pitwall_core::ids::{AppId, ModelId};
use pitwall_core::limits;
use pitwall_core::metrics::{Metrics, Sample};
use pitwall_core::mgmt::{self, ChosenBy, LiveUpdate, RequestRecord, Throughput};
use pitwall_core::runtimes::BuildChoice;
use pitwall_core::scheduler::{JobKind, Priority};
use pitwall_core::select::Capability;
use pitwall_core::usage::{Usage, UsageEvent};
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

const MAX_RECORDS: usize = 500;
/// The server counts as idle after this long without a request (the clean-up pass may then run early).
const IDLE_SWEEP_AFTER: u64 = 5 * 60 * 1000;
/// An idle server runs the pass at most this often.
const IDLE_SWEEP_EVERY: u64 = 15 * 60 * 1000;

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Shared state.
pub struct AppState {
    /// Data folders.
    pub dirs: Dirs,
    /// Live settings.
    pub settings: Arc<RwLock<Settings>>,
    /// Offline switch shared with the egress client.
    pub offline: Arc<AtomicBool>,
    /// The only outbound client.
    pub egress: Egress,
    /// llama.cpp installs.
    pub llama: LlamaCpp,
    /// The engine that serves models (llama.cpp; tests substitute a fake).
    pub engine: Arc<dyn Engine>,
    /// Model catalog packs (ADR 0011, ADR 0020).
    pub packs: pitwall_core::packs::Packs,
    /// Models.
    pub library: Library,
    /// Loading, queue and release.
    pub serving: Serving,
    /// Apps and keys.
    pub auth: Auth,
    /// Recent events.
    pub events: Arc<Mutex<EventLog>>,
    /// Time series.
    pub metrics: Mutex<Metrics>,
    /// Per-app usage over time.
    pub usage: Mutex<Usage>,
    /// Limit warnings already raised (app, limit, start of its period), so each is raised once per period.
    pub limit_warned: Mutex<HashSet<(String, limits::LimitKind, u64)>>,
    /// Serialises read-modify-write of the presets (the list itself lives in [`Serving`]).
    pub presets_gate: Mutex<()>,
    /// Serialises read-modify-write of the engine pins (the pins themselves live in [`LlamaCpp`]).
    pub pins_gate: Mutex<()>,
    /// Detected hardware.
    pub hardware: Hardware,
    /// When the server started.
    pub started_at: u64,
    /// Pushed updates, as JSON text.
    pub live: broadcast::Sender<String>,
    /// Recent requests (metadata only).
    pub requests: Mutex<VecDeque<RequestRecord>>,
    /// Prompts and answers of the latest requests, only while "keep request content" is on: in memory, redacted,
    /// bounded in number and size, gone when Pitwall stops.
    pub contents: Mutex<VecDeque<pitwall_core::mgmt::RequestContent>>,
    /// Recent request speeds for the dashboard tiles.
    pub recent: Mutex<VecDeque<(u64, f64, f64, u64)>>,
    /// Engine installs in progress or finished.
    pub installs: Mutex<std::collections::BTreeMap<String, mgmt::Download>>,
    /// Count of running engine installs' cancel flags.
    pub install_cancel: Mutex<std::collections::BTreeMap<String, crate::egress::CancelFlag>>,
    /// Cancellable operations by the caller's `opId` (a load or a benchmark): `cancel_op` notifies, the operation stops.
    pub ops: Mutex<std::collections::BTreeMap<String, Arc<tokio::sync::Notify>>>,
    /// Benchmark results of this session.
    pub benchmarks: Mutex<Vec<mgmt::BenchmarkResult>>,
    /// Secrets of just-created apps, handed over once to the UI.
    pub new_secrets: Mutex<std::collections::HashMap<String, String>>,
    /// The port actually bound.
    pub port: std::sync::atomic::AtomicU16,
    /// Model update notices and the checker (ADR 0017).
    pub updates: crate::updates::Updates,
    /// Where the checker asks (Hugging Face; tests swap it).
    pub update_source: RwLock<Arc<dyn crate::updates::RepoSource>>,
    /// The response and embeddings caches (ADR 0019).
    pub caches: Caches,
    /// When the last request finished (the clean-up pass runs when the server has been idle).
    pub last_request_at: std::sync::atomic::AtomicU64,
}

impl AppState {
    /// Open the state in `dirs`: settings (damage tolerated), managers, hardware. Starts no tasks.
    pub async fn open(dirs: Dirs, engine_override: Option<Arc<dyn Engine>>) -> Arc<AppState> {
        let _ = std::fs::create_dir_all(&dirs.root);
        let (p, b) = paths::read_with_backup(&dirs.settings(), config::MAX_SETTINGS_BYTES);
        let (settings, loaded) = config::load(p.as_deref(), b.as_deref());
        let hardware = system::detect_hardware().await;
        let offline = Arc::new(AtomicBool::new(settings.network.offline));
        let egress = Egress::new(offline.clone());
        let llama = LlamaCpp::new(
            dirs.clone(),
            egress.clone(),
            hardware.clone(),
            BuildChoice::parse(&settings.engines.build_choice),
        );
        let stored_benchmarks = {
            let (p, b) = paths::read_with_backup(&dirs.benchmarks(), pitwall_core::benchmarks::MAX_BYTES);
            pitwall_core::benchmarks::load(p.as_deref(), b.as_deref())
        };
        let overridden = engine_override.is_some();
        let engine: Arc<dyn Engine> = engine_override.unwrap_or_else(|| Arc::new(llama.clone()));
        let packs = load_packs(&dirs);
        let library = Library::new(dirs.clone(), egress.clone());
        let events = Arc::new(Mutex::new(EventLog::default()));
        let settings = Arc::new(RwLock::new(settings));
        let serving = Serving::new(engine.clone(), library.clone(), settings.clone(), hardware.clone(), events.clone());
        let auth = Auth::open(dirs.clone());
        let metrics = std::fs::read_to_string(dirs.metrics())
            .ok()
            .filter(|t| t.len() < 32 << 20)
            .map_or_else(|| Metrics::new(30), |t| Metrics::restore(&t, 30));
        let usage = paths::read_with_backup(&dirs.usage(), 32 << 20);
        let usage = [usage.0, usage.1]
            .into_iter()
            .flatten()
            .find_map(|t| Usage::restore(&t, now_ms()))
            .unwrap_or_default();
        let stored_presets = {
            let (p, b) = paths::read_with_backup(&dirs.presets(), pitwall_core::presets::MAX_BYTES);
            pitwall_core::presets::load(p.as_deref(), b.as_deref())
        };
        let (updates, updates_damaged) = crate::updates::Updates::open(&dirs);
        let update_source: RwLock<Arc<dyn crate::updates::RepoSource>> =
            RwLock::new(Arc::new(crate::updates::HfSource::new(egress.clone())));
        let (live, _) = broadcast::channel(256);
        let caches = Caches::open(
            &dirs.cache_db(),
            &settings.read().map(|s| s.cache.clone()).unwrap_or_default(),
            now_ms(),
        );
        serving.set_slots_dir(dirs.slots());
        let st = Arc::new(AppState {
            dirs,
            settings,
            offline,
            egress,
            llama,
            engine,
            packs,
            library,
            serving,
            auth,
            events,
            metrics: Mutex::new(metrics),
            usage: Mutex::new(usage),
            limit_warned: Mutex::new(HashSet::new()),
            presets_gate: Mutex::new(()),
            pins_gate: Mutex::new(()),
            hardware,
            started_at: now_ms(),
            live,
            requests: Mutex::new(VecDeque::new()),
            contents: Mutex::new(VecDeque::new()),
            recent: Mutex::new(VecDeque::new()),
            installs: Mutex::new(Default::default()),
            install_cancel: Mutex::new(Default::default()),
            ops: Mutex::new(Default::default()),
            benchmarks: Mutex::new(stored_benchmarks),
            new_secrets: Mutex::new(Default::default()),
            port: std::sync::atomic::AtomicU16::new(0),
            updates,
            update_source,
            caches,
            last_request_at: std::sync::atomic::AtomicU64::new(0),
        });
        if !overridden {
            // The real llama.cpp installs the suggested build by itself when a model needs one and none is installed.
            let weak = Arc::downgrade(&st);
            st.llama.set_kick(Arc::new(move || {
                if let Some(st) = weak.upgrade() {
                    crate::engine_setup::auto_install(&st, Arc::new(st.llama.clone()));
                }
            }));
        }
        st.serving.set_speeds(pitwall_core::benchmarks::speeds(&lock(&st.benchmarks)));
        if stored_presets.damaged || stored_presets.skipped > 0 {
            st.event(
                EventKind::RequestFailed,
                Severity::Warn,
                format!(
                    "The presets file was damaged: {} preset(s) were left out. Check Models > Presets.",
                    stored_presets.skipped
                ),
            );
        }
        if updates_damaged {
            st.event(
                EventKind::RequestFailed,
                Severity::Warn,
                "The model update notices file was damaged and started empty; the next check rebuilds it.".into(),
            );
        }
        st.serving.set_presets(stored_presets.presets);
        st.serving.set_gate(Arc::new(st.llama.clone()));
        let stored_pins = {
            let (p, b) = paths::read_with_backup(&st.dirs.engine_pins(), pitwall_core::runtimes::pins::MAX_BYTES);
            pitwall_core::runtimes::pins::load(p.as_deref(), b.as_deref())
        };
        if stored_pins.damaged || stored_pins.skipped > 0 {
            st.event(
                EventKind::RequestFailed,
                Severity::Warn,
                format!(
                    "The engine pins file was damaged: {} pin(s) were left out. Check the llama.cpp page.",
                    stored_pins.skipped
                ),
            );
        }
        st.llama.set_pins(stored_pins.pins);
        for p in st.llama.pins().problems(&st.llama.facts()) {
            st.event(EventKind::EngineFailed, Severity::Warn, p.to_string());
        }
        st.event(EventKind::EngineStarted, Severity::Info, "Pitwall started".into());
        for p in st.packs.problems.iter() {
            st.event(
                EventKind::RequestFailed,
                Severity::Warn,
                format!("Pack {} was skipped: {}", p.file, p.message),
            );
        }
        match loaded {
            config::Loaded::Backup(why) | config::Loaded::Reset(why) => st.event(
                EventKind::RequestFailed,
                Severity::Warn,
                format!("Settings were damaged and restored from a backup or reset: {why}"),
            ),
            _ => {}
        }
        st
    }

    /// A model was removed or its engine changed: its cached answers and saved prompt prefixes go now.
    pub fn invalidate_model(&self, id: &ModelId) {
        self.caches.clear(&pitwall_core::cache::Scope::Model(id.to_string()));
        crate::warm::drop_prefix(&self.dirs.slots(), &crate::warm::model_prefix(id.as_str()), None);
    }

    /// The engine build that serves models changed: everything computed by the old one goes now.
    pub fn invalidate_engine(&self) {
        self.caches.clear(&pitwall_core::cache::Scope::All);
        crate::warm::drop_prefix(&self.dirs.slots(), "", None);
    }

    /// A preset changed or was deleted: what was cached under it goes now.
    pub fn invalidate_preset(&self, alias: &str) {
        self.caches.clear(&pitwall_core::cache::Scope::Preset(alias.to_string()));
        crate::warm::drop_alias(&self.dirs.slots(), alias);
    }

    /// What the caches hold, for Settings.
    pub fn cache_info(&self) -> pitwall_core::cache::Info {
        let (files, bytes) = crate::warm::usage(&self.dirs.slots());
        self.caches.info(files, bytes)
    }

    /// One clean-up pass: expired cache entries and stale saved prompt prefixes, bounded. Returns how many went.
    pub fn sweep_caches(&self) -> u64 {
        let now = now_ms();
        let days = self.cfg().cache.keep_warm_days;
        self.caches.sweep(now) + crate::warm::sweep(&self.dirs.slots(), days, now, 10_000)
    }

    /// A copy of the settings.
    pub fn cfg(&self) -> Settings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    /// Record an event and push it to the UI.
    pub fn event(&self, kind: EventKind, sev: Severity, msg: String) {
        let e = lock(&self.events).push(now_ms(), kind, sev, msg).clone();
        self.push(&LiveUpdate::Event { event: e });
    }

    /// Push a live update.
    pub fn push(&self, u: &LiveUpdate) {
        if let Ok(s) = serde_json::to_string(u) {
            let _ = self.live.send(s);
        }
    }

    /// Validate, apply and save new settings.
    pub fn save_settings(&self, s: Settings) -> Result<Settings, String> {
        s.validate()?;
        paths::write_atomic(&self.dirs.settings(), s.to_toml().as_bytes()).map_err(|e| format!("cannot save the settings: {e}"))?;
        self.offline.store(s.network.offline, std::sync::atomic::Ordering::SeqCst);
        self.llama.set_choice(BuildChoice::parse(&s.engines.build_choice));
        if let Ok(mut g) = self.settings.write() {
            *g = s.clone();
        }
        self.serving.settings_changed();
        self.caches.apply(&s.cache);
        Ok(s)
    }

    /// Write the discovery file (ADR 0009).
    pub fn write_discovery(&self, port: u16) {
        let d = pitwall_core::discovery::Discovery::new(env!("CARGO_PKG_VERSION"), std::process::id(), port, self.started_at);
        if let Ok(b) = serde_json::to_vec_pretty(&d) {
            let _ = paths::write_atomic(&self.dirs.discovery(), &b);
        }
    }

    /// On a clean shutdown the file stays, rewritten with `"serving": false`, so apps can tell "installed, not running"
    /// from "not installed".
    pub fn remove_discovery(&self) {
        // Flip the file this process wrote; if it is missing or damaged there is nothing trustworthy to keep.
        let stopped = std::fs::read_to_string(self.dirs.discovery())
            .ok()
            .and_then(|t| pitwall_core::discovery::Discovery::parse(&t).ok())
            .and_then(|d| serde_json::to_vec_pretty(&d.stopped()).ok());
        match stopped {
            Some(b) if paths::write_atomic(&self.dirs.discovery(), &b).is_ok() => {}
            _ => {
                let _ = std::fs::remove_file(self.dirs.discovery());
            }
        }
    }

    /// Start the background tasks: release loop, live feed, metrics sampling.
    pub fn spawn_tasks(self: &Arc<Self>) {
        // First start (or every start with no build): install the suggested llama.cpp in the background.
        self.llama.install_if_missing();
        self.serving.spawn_tick_loop();
        self.spawn_cache_sweeper();
        self.start_update_schedule();
        let me = self.clone();
        tokio::spawn(async move {
            let mut n = 0u64;
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                n += 1;
                let sample = me.memory_sample();
                me.push(&LiveUpdate::Memory { sample: sample.clone() });
                me.push(&LiveUpdate::Models {
                    models: me.serving.loaded(),
                });
                me.push(&LiveUpdate::Issues {
                    issues: me.serving.issues(),
                });
                me.push(&LiveUpdate::Queue {
                    queue: me.queue_snapshot(),
                });
                let t = me.throughput();
                me.push(&LiveUpdate::Throughput { throughput: t });
                if n.is_multiple_of(5) {
                    let (hot_ram, _) = me.serving.hot_ram();
                    let gpu = sample.gpus.first();
                    lock(&me.metrics).sample(&Sample {
                        at: sample.at,
                        ram_used: sample.ram_total.saturating_sub(sample.ram_available),
                        ram_total: sample.ram_total,
                        vram_used: gpu.and_then(|g| g.vram_used),
                        vram_total: gpu.and_then(|g| g.vram_total),
                        pitwall_rss: sample.pitwall_rss + hot_ram,
                        cpu_percent: sample.cpu_percent,
                        prompt_tps: t.prompt_tps,
                        gen_tps: t.gen_tps,
                    });
                }
                if n.is_multiple_of(60) {
                    let json = lock(&me.metrics).to_json();
                    let _ = paths::write_atomic(&me.dirs.metrics(), json.as_bytes());
                }
                if n.is_multiple_of(15) {
                    me.save_usage();
                }
            }
        });
    }

    /// Writes the presets (atomic, with a backup).
    pub fn save_presets(&self, presets: &pitwall_core::presets::Presets) -> std::io::Result<()> {
        paths::write_atomic(&self.dirs.presets(), presets.to_json().as_bytes())
    }

    /// Writes the engine pins (atomic, with a backup).
    pub fn save_pins(&self, pins: &pitwall_core::runtimes::pins::EnginePins) -> std::io::Result<()> {
        paths::write_atomic(&self.dirs.engine_pins(), pins.to_json().as_bytes())
    }

    /// Writes the per-app usage (atomic, with a backup). Failures are ignored: usage is rebuilt as requests arrive.
    pub fn save_usage(&self) {
        let json = lock(&self.usage).to_json();
        let _ = paths::write_atomic(&self.dirs.usage(), json.as_bytes());
    }

    /// Where each configured limit of an app stands now.
    pub fn limit_statuses(&self, app: &str, limits: &limits::AppLimits, now: u64) -> Vec<limits::LimitStatus> {
        let (used, peak) = {
            let u = lock(&self.usage);
            (u.budget_use(now, app), u.today(now, app).peak_memory_bytes)
        };
        let requests_last_minute = AppId::parse(app).map_or(0, |id| self.auth.requests_last_minute(&id, now));
        limits::statuses(
            limits,
            &limits::StatusInput {
                used,
                requests_last_minute,
                peak_memory_today: peak,
                machine_bytes: self.serving.memory_now().total,
            },
        )
    }

    /// Raises an event the first time an app reaches 80% of a limit in a period.
    pub(crate) fn warn_limits(&self, app: &str, limits: &limits::AppLimits, now: u64) {
        for s in self.limit_statuses(app, limits, now) {
            let (limits::LimitState::Warning, Some(period)) = (s.state, s.kind.period_start(now)) else {
                continue;
            };
            let first = {
                let mut w = lock(&self.limit_warned);
                if w.len() >= 1024 {
                    w.clear();
                }
                w.insert((app.to_string(), s.kind, period))
            };
            if first {
                let e = {
                    let mut log = lock(&self.events);
                    let ev = log.push(
                        now,
                        EventKind::LimitWarning,
                        Severity::Warn,
                        format!("{app}: {}", s.message.unwrap_or_default()),
                    );
                    ev.app_id = Some(app.to_string());
                    ev.clone()
                };
                self.push(&LiveUpdate::Event { event: e });
            }
        }
    }

    /// Memory and GPU reading for the dashboard.
    pub fn memory_sample(&self) -> mgmt::MemorySample {
        let sys = self.serving.memory_now();
        let mut s = sysinfo::System::new();
        s.refresh_memory();
        let (hot_ram, _) = self.serving.hot_ram();
        let pressure = if sys.total > 0 && sys.available * 100 / sys.total < 5 {
            mgmt::MemoryPressure::Critical
        } else if sys.total > 0 && sys.available * 100 / sys.total < 12 {
            mgmt::MemoryPressure::Elevated
        } else {
            mgmt::MemoryPressure::Normal
        };
        mgmt::MemorySample {
            at: now_ms(),
            ram_total: sys.total,
            ram_available: sys.available,
            commit_used: None,
            commit_limit: None,
            swap_used: s.used_swap(),
            pressure,
            cpu_percent: 0.0,
            gpus: self
                .hardware
                .gpus
                .iter()
                .map(|g| mgmt::GpuSample {
                    name: g.name.clone(),
                    vram_total: g.vram_total,
                    vram_used: if g.unified { Some(hot_ram) } else { g.vram_used },
                    utilisation: g.utilisation,
                    unified: g.unified,
                })
                .collect(),
            pitwall_rss: system::rss(std::process::id()),
            foreground_busy: false,
        }
    }

    /// The queue for the UI.
    pub fn queue_snapshot(&self) -> mgmt::QueueSnapshot {
        let jobs = self.serving.jobs();
        let conv = |j: &pitwall_core::scheduler::Job| mgmt::Job {
            id: j.id.to_string(),
            app_id: j.spec.app.to_string(),
            model_id: j.spec.model.as_ref().map(ToString::to_string),
            priority: j.spec.priority,
            kind: j.spec.kind,
            state: if j.state == pitwall_core::scheduler::JobState::Running {
                mgmt::JobState::Running
            } else {
                mgmt::JobState::Queued
            },
            submitted_at: j.submitted_at,
            started_at: j.started_at,
            progress: j.progress,
            tokens: j.tokens,
        };
        let (running, waiting): (Vec<_>, Vec<_>) = jobs.iter().partition(|j| j.state == pitwall_core::scheduler::JobState::Running);
        mgmt::QueueSnapshot {
            running: running.into_iter().map(conv).collect(),
            waiting: waiting.into_iter().map(conv).collect(),
            depth: self.serving.queue().0,
        }
    }

    /// Speeds over the last minute.
    pub fn throughput(&self) -> Throughput {
        let now = now_ms();
        let mut r = lock(&self.recent);
        while r.front().is_some_and(|x| now.saturating_sub(x.0) > 60_000) {
            r.pop_front();
        }
        let n = r.len().max(1) as f64;
        let mut ttfts: Vec<u64> = r.iter().map(|x| x.3).collect();
        ttfts.sort_unstable();
        Throughput {
            prompt_tps: r.iter().map(|x| x.1).sum::<f64>() / n,
            gen_tps: r.iter().map(|x| x.2).sum::<f64>() / n,
            ttft_p50_ms: ttfts.get(ttfts.len() / 2).copied().unwrap_or(0) as f64,
            requests_per_min: r.len() as f64,
            active_requests: self.serving.queue().1 as u32,
        }
    }

    /// The clean-up pass: at start-up, then hourly, and when the server has been idle for a while (checked every
    /// minute; the pass itself is bounded and runs off the request path).
    pub fn spawn_cache_sweeper(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            let st = me.clone();
            let _ = tokio::task::spawn_blocking(move || st.sweep_caches()).await;
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let now = now_ms();
                let idle = now.saturating_sub(me.last_request_at.load(std::sync::atomic::Ordering::SeqCst)) >= IDLE_SWEEP_AFTER;
                let since = now.saturating_sub(me.caches.next_sweep_at().saturating_sub(crate::caches::SWEEP_EVERY));
                if me.caches.sweep_due(now) || (idle && since >= IDLE_SWEEP_EVERY) {
                    let st = me.clone();
                    let _ = tokio::task::spawn_blocking(move || st.sweep_caches()).await;
                }
            }
        });
    }

    /// Run the whole pipeline for a chat-like request and start generating.
    pub async fn start_chat(self: &Arc<Self>, who: &Principal, meta: Meta, req: ChatRequest) -> Result<Run, ApiError> {
        self.start_chat_queued(who, meta, req, None).await
    }

    /// [`Self::start_chat`] that also tells `on_queued` the job id when the request has to wait in the queue, so the
    /// route can answer at once with `X-Pitwall-Job-Id` (cancel by id while queued).
    pub async fn start_chat_queued(
        self: &Arc<Self>,
        who: &Principal,
        meta: Meta,
        mut req: ChatRequest,
        on_queued: Option<tokio::sync::oneshot::Sender<u64>>,
    ) -> Result<Run, ApiError> {
        let cfg = self.cfg();
        let now = now_ms();
        req.check().map_err(ApiError::from)?;
        let (app, perms) = match who {
            Principal::App { id, permissions } => (id.clone(), Some(permissions.clone())),
            Principal::Admin => (AppId::parse("pitwall").map_err(|e| ApiError::invalid(e.to_string()))?, None),
            Principal::Keyless => (AppId::parse("local").map_err(|e| ApiError::invalid(e.to_string()))?, None),
        };
        let resolved = self.serving.resolve(&req.model, req.has_images(), !req.tools.is_empty())?;
        if let Some(p) = &resolved.preset {
            pitwall_core::presets::apply(p, &mut req);
        }
        let wanted = meta.priority.or(req.priority).unwrap_or(Priority::Interactive);
        let mut app_cap = None;
        let priority = match &perms {
            Some(p) => {
                let pr = p
                    .authorize(Some(&resolved.id), wanted, !req.tools.is_empty(), meta.extended)
                    .map_err(denied)?;
                self.auth.admit(&app, p.limits.requests_per_min, now).map_err(denied)?;
                let used = lock(&self.usage).budget_use(now, app.as_str());
                limits::check_budget(&used, &p.limits).map_err(denied)?;
                app_cap = p.limits.max_concurrent.map(|c| c as usize);
                pr
            }
            None => wanted,
        };
        // What the request needs from the engine build that would serve the model (ADR 0018): refused up front with
        // the build needed instead of failing obscurely inside the engine.
        let mut needed = vec![];
        if req.has_images() {
            needed.push(Capability::Vision);
        }
        if !req.tools.is_empty() {
            needed.push(Capability::Tools);
        }
        if matches!(req.format, pitwall_core::chat::OutputFormat::JsonSchema(_)) {
            needed.push(Capability::Json);
        }
        self.serving.check_capabilities(&resolved, &needed)?;
        let name = resolved.id.to_string();
        req.model = ModelRef::Named(name.clone());
        let chosen_by = resolved.chosen_by;
        let alias = resolved.preset.as_ref().map(|p| p.alias.to_string());
        let stream = req.stream;
        let request_text = if cfg.privacy.keep_request_content {
            Some(request_text(&req))
        } else {
            None
        };
        // The response cache (ADR 0019): only for deterministic requests, only for apps and presets that opted in.
        let opted_in = perms.as_ref().is_some_and(|p| p.cache_responses) || resolved.preset.as_ref().is_some_and(|p| p.cache_responses);
        let mut outcome = None;
        let mut cache_store = None;
        let cache_meta = pitwall_core::cache::Meta {
            model: name.clone(),
            app: app.to_string(),
            preset: alias.clone().unwrap_or_default(),
        };
        if opted_in && pitwall_core::cache::cacheable(&req) {
            if meta.no_cache {
                outcome = Some(pitwall_core::cache::Outcome::Bypass);
            } else {
                let key = pitwall_core::cache::key(&self.serving.fingerprint(&resolved), &req);
                match self.caches.response(&key, now) {
                    Some(out) => {
                        return Ok(self.cached_run(CachedAnswer {
                            out,
                            resolved,
                            meta,
                            app: app.to_string(),
                            priority,
                            limits: perms.map(|p| p.limits),
                            stream,
                            request_text,
                            alias,
                        }));
                    }
                    None => outcome = Some(pitwall_core::cache::Outcome::Miss),
                }
                cache_store = Some(CacheStore {
                    key,
                    meta: cache_meta,
                    acc: pitwall_core::chat::ChatOutput::default(),
                });
            }
        }
        let queued = Instant::now();
        let permit = self
            .serving
            .acquire(app.clone(), resolved.id.clone(), priority, meta.kind, app_cap, on_queued)
            .await?;
        let (busy, cold) = self.serving.ensure_for(&resolved).await?;
        if req.has_images() && !busy.model().vision {
            return Err(ApiError::new(
                ErrorKind::InvalidRequest,
                format!(
                    "the model {} cannot read images. Choose a vision model (one downloaded with its mmproj file).",
                    resolved.id
                ),
            ));
        }
        let memory_bytes = self.serving.footprint(&resolved.id);
        if let Some(p) = &perms {
            limits::check_memory(memory_bytes, self.serving.memory_now().total, p.limits.max_memory_percent).map_err(denied)?;
            self.warn_limits(app.as_str(), &p.limits, now);
        }
        let queued_ms = queued.elapsed().as_millis() as u64;
        let timeout = Duration::from_secs(cfg.server.request_timeout_secs);
        let job = permit.id();
        let engine = busy.model().engine.0.clone();
        let hint = crate::serving::ChatHint {
            app: Some(app.to_string()),
            warm_file: self.serving.warm_file_for(&resolved),
        };
        let rx = self.serving.chat(busy, permit, req, timeout, &hint);
        Ok(Run {
            rx,
            resolved,
            cold_start_ms: cold,
            engine,
            job,
            cache: outcome,
            rec: Recorder {
                st: self.clone(),
                app: app.to_string(),
                family: meta.family,
                endpoint: meta.endpoint,
                priority,
                started: Instant::now(),
                memory_bytes,
                limits: perms.as_ref().map(|p| p.limits.clone()),
                queued_ms,
                ttft: None,
                stream,
                cold_start_ms: cold,
                text: String::new(),
                request_text,
                model: name,
                chosen_by,
                alias,
                cache: outcome,
                cache_hit: false,
                cache_store,
                done: false,
            },
        })
    }

    /// A [`Run`] that replays a stored answer: no queue, no model load, no engine.
    fn cached_run(self: &Arc<Self>, a: CachedAnswer) -> Run {
        let events = pitwall_core::cache::replay(&a.out);
        let (tx, rx) = mpsc::channel(events.len() + 1);
        for e in events {
            let _ = tx.try_send(Ok(e));
        }
        let name = a.resolved.id.to_string();
        Run {
            rx,
            cold_start_ms: None,
            engine: "cache".into(),
            job: 0,
            cache: Some(pitwall_core::cache::Outcome::Hit),
            rec: Recorder {
                st: self.clone(),
                app: a.app,
                family: a.meta.family,
                endpoint: a.meta.endpoint,
                priority: a.priority,
                started: Instant::now(),
                memory_bytes: 0,
                limits: a.limits,
                queued_ms: 0,
                ttft: None,
                stream: a.stream,
                cold_start_ms: None,
                text: String::new(),
                request_text: a.request_text,
                model: name,
                chosen_by: a.resolved.chosen_by,
                alias: a.alias,
                cache: Some(pitwall_core::cache::Outcome::Hit),
                cache_hit: true,
                cache_store: None,
                done: false,
            },
            resolved: a.resolved,
        }
    }
}

/// What a cache hit needs to become a [`Run`].
struct CachedAnswer {
    out: pitwall_core::chat::ChatOutput,
    resolved: Resolved,
    meta: Meta,
    app: String,
    priority: Priority,
    limits: Option<limits::AppLimits>,
    stream: bool,
    request_text: Option<String>,
    alias: Option<String>,
}

/// The answer of a cache-eligible request, collected as it streams, to be stored when it completes.
struct CacheStore {
    key: pitwall_core::cache::Key,
    meta: pitwall_core::cache::Meta,
    acc: pitwall_core::chat::ChatOutput,
}

/// The largest answer the response cache collects (bytes of text).
const MAX_CACHED_ANSWER: usize = 1024 * 1024;

fn denied(d: limits::Denied) -> ApiError {
    ApiError::new(
        if d.is_rate() { ErrorKind::RateLimit } else { ErrorKind::Permission },
        d.to_string(),
    )
}

/// Per-request facts the route supplies.
#[derive(Debug, Clone)]
pub struct Meta {
    /// Which API family.
    pub family: ApiFamily,
    /// The path.
    pub endpoint: String,
    /// Priority from the header.
    pub priority: Option<Priority>,
    /// Whether this is an `/x/` request.
    pub extended: bool,
    /// What kind of job.
    pub kind: JobKind,
    /// The caller asked not to use the response cache (`Cache-Control: no-cache`).
    pub no_cache: bool,
}

/// A started request.
pub struct Run {
    /// The events as the engine produces them.
    pub rx: mpsc::Receiver<Result<ChatEvent, EngineFail>>,
    /// The model and how it was chosen.
    pub resolved: Resolved,
    /// Set when the model had to be loaded.
    pub cold_start_ms: Option<u64>,
    /// The engine that serves it (`llama.cpp`, or the id of the engine pack).
    pub engine: String,
    /// The queue job id (0 for an answer from the cache).
    pub job: u64,
    /// What the response cache did, when the request was eligible for it.
    pub cache: Option<pitwall_core::cache::Outcome>,
    /// Records the outcome when finished.
    pub rec: Recorder,
}

/// Collects what a request did and writes the records when it ends (also when the client disconnects).
pub struct Recorder {
    st: Arc<AppState>,
    app: String,
    family: ApiFamily,
    endpoint: String,
    priority: Priority,
    started: Instant,
    /// The footprint of the model that serves the request (bytes), attributed to the app as its peak memory.
    memory_bytes: u64,
    /// The app's limits, for the 80% warning after the request is counted.
    limits: Option<limits::AppLimits>,
    queued_ms: u64,
    ttft: Option<u64>,
    stream: bool,
    cold_start_ms: Option<u64>,
    text: String,
    /// The redacted request, only when content is kept.
    request_text: Option<String>,
    model: String,
    chosen_by: ChosenBy,
    /// The preset the request named, if any.
    alias: Option<String>,
    /// What the response cache did.
    cache: Option<pitwall_core::cache::Outcome>,
    /// The answer came from the cache.
    cache_hit: bool,
    /// Collects the answer for the cache; dropped when the answer turns out too large or the request fails.
    cache_store: Option<CacheStore>,
    done: bool,
}

impl Recorder {
    /// Note how the model was chosen.
    pub fn chosen(&mut self, by: ChosenBy) {
        self.chosen_by = by;
    }

    /// Note an event (first-token time, text kept only when the user allows it).
    pub fn event(&mut self, e: &ChatEvent) {
        if self.ttft.is_none() && matches!(e, ChatEvent::Delta(_) | ChatEvent::Reasoning(_) | ChatEvent::ToolCall { .. }) {
            self.ttft = Some(self.started.elapsed().as_millis() as u64);
        }
        if let Some(c) = &mut self.cache_store {
            c.acc.push(e);
            if c.acc.text.len() + c.acc.reasoning.len() > MAX_CACHED_ANSWER {
                self.cache_store = None;
            }
        }
        if let ChatEvent::Delta(t) = e {
            if self.st.cfg().privacy.keep_request_content && self.text.len() < 64 * 1024 {
                self.text.push_str(t);
            }
        }
    }

    /// Finish with usage (or an error message).
    pub fn finish(&mut self, usage: pitwall_core::chat::Usage, timings: Option<pitwall_core::chat::Timings>, error: Option<String>) {
        if self.done {
            return;
        }
        self.done = true;
        let now = now_ms();
        let total_ms = self.started.elapsed().as_millis() as u64;
        let failed = error.is_some();
        let reused = timings.map_or(0, |t| t.cached_tokens);
        let hit = self.cache_hit && !failed;
        lock(&self.st.usage).record(&UsageEvent {
            at: now,
            app: &self.app,
            // A hit cost the model nothing: its tokens are "saved", not used.
            prompt_tokens: if hit { 0 } else { u64::from(usage.prompt_tokens) },
            completion_tokens: if hit { 0 } else { u64::from(usage.completion_tokens) },
            busy_ms: total_ms,
            failed,
            memory_bytes: self.memory_bytes,
            cache_hit: hit,
            cache_miss: self.cache == Some(pitwall_core::cache::Outcome::Miss),
            prefix_tokens: u64::from(reused),
            saved_tokens: if hit {
                u64::from(usage.prompt_tokens) + u64::from(usage.completion_tokens)
            } else {
                0
            },
        });
        if let Some(c) = self.cache_store.take().filter(|_| !failed) {
            self.st.caches.store_response(c.key, &c.acc, c.meta, now);
        }
        self.st.last_request_at.store(now, std::sync::atomic::Ordering::SeqCst);
        if let Some(l) = &self.limits {
            self.st.warn_limits(&self.app, l, now);
        }
        lock(&self.st.metrics).request(now, &self.app, &self.model, total_ms);
        if let Some(t) = timings {
            let mut r = lock(&self.st.recent);
            r.push_back((now, t.prompt_tps, t.gen_tps, self.ttft.unwrap_or(0)));
            while r.len() > 600 {
                r.pop_front();
            }
        } else if !failed {
            lock(&self.st.recent).push_back((
                now,
                0.0,
                if total_ms > 0 {
                    f64::from(usage.completion_tokens) * 1000.0 / total_ms as f64
                } else {
                    0.0
                },
                self.ttft.unwrap_or(0),
            ));
        }
        if let Some(e) = &error {
            self.st
                .event(EventKind::RequestFailed, Severity::Warn, format!("{}: {e}", self.model));
        }
        let id = format!("r{now}-{}", lock(&self.st.requests).len());
        let has_content = if let Some(request) = self.request_text.take() {
            let mut c = lock(&self.st.contents);
            c.push_front(pitwall_core::mgmt::RequestContent {
                id: id.clone(),
                request,
                response: pitwall_core::redact::redact(&self.text),
            });
            c.truncate(MAX_CONTENTS);
            true
        } else {
            false
        };
        let rec = RequestRecord {
            id,
            at: now,
            app_id: self.app.clone(),
            api: self.family,
            endpoint: self.endpoint.clone(),
            model_id: Some(self.model.clone()),
            chosen_by: self.chosen_by,
            alias: self.alias.clone(),
            priority: self.priority,
            status: if failed { 502 } else { 200 },
            stream: self.stream,
            queued_ms: self.queued_ms,
            ttft_ms: self.ttft,
            total_ms,
            cold_start_ms: self.cold_start_ms,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            error,
            has_content,
            cache: self.cache,
            cached_tokens: reused,
        };
        let mut q = lock(&self.st.requests);
        q.push_front(rec);
        q.truncate(MAX_RECORDS);
        // Content outlives its record by nothing: drop what no record points to any more.
        let keep: std::collections::HashSet<&str> = q.iter().filter(|r| r.has_content).map(|r| r.id.as_str()).collect();
        lock(&self.st.contents).retain(|c| keep.contains(c.id.as_str()));
    }
}

/// The most requests whose text is kept.
const MAX_CONTENTS: usize = 50;
/// The most bytes of one request's text that are kept.
const MAX_CONTENT_BYTES: usize = 64 * 1024;

/// The request as readable, redacted text for the inspector, bounded.
fn request_text(req: &ChatRequest) -> String {
    use pitwall_core::chat::Part;
    let mut out = String::new();
    for m in &req.messages {
        out.push_str(&format!("{:?}: ", m.role).to_lowercase());
        for p in &m.parts {
            match p {
                Part::Text(t) => out.push_str(t),
                Part::Image(_) => out.push_str("[image]"),
            }
        }
        out.push('\n');
        if out.len() >= MAX_CONTENT_BYTES {
            break;
        }
    }
    let mut end = out.len().min(MAX_CONTENT_BYTES);
    while !out.is_char_boundary(end) {
        end -= 1;
    }
    out.truncate(end);
    pitwall_core::redact::redact(&out)
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // A client that went away still counts as a (cancelled) request.
        self.finish(
            pitwall_core::chat::Usage::default(),
            None,
            Some("the client closed the connection".into()),
        );
    }
}

/// Built-in packs plus the user's (`<data>/packs/*.toml`, bounded in number and size; unreadable files are reported).
pub fn load_packs(dirs: &Dirs) -> pitwall_core::packs::Packs {
    let mut files: Vec<(String, String)> = vec![];
    let mut unreadable = vec![];
    if let Ok(rd) = std::fs::read_dir(dirs.root.join("packs")) {
        let mut names: Vec<_> = rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_file()).collect();
        names.sort();
        for p in names.into_iter().take(64) {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match std::fs::metadata(&p)
                .ok()
                .filter(|m| m.len() as usize <= pitwall_core::packs::MAX_PACK_BYTES)
                .and_then(|_| std::fs::read_to_string(&p).ok())
            {
                Some(t) => files.push((name, t)),
                None => unreadable.push(name),
            }
        }
    }
    let builtin = [(
        "starter.models.toml",
        include_str!("../../pitwall-core/src/packs/starter.models.toml"),
    )];
    let all: Vec<(&str, &str)> = builtin
        .iter()
        .copied()
        .chain(files.iter().map(|(a, b)| (a.as_str(), b.as_str())))
        .collect();
    let mut p = pitwall_core::packs::Packs::load(all);
    p.problems.extend(unreadable.into_iter().map(|file| pitwall_core::packs::Problem {
        file,
        message: "the file is unreadable or larger than 256 KB".into(),
    }));
    p
}

/// The model for `ModelId` strings from the UI.
pub fn model_id(s: &str) -> Result<ModelId, ApiError> {
    ModelId::parse(s).map_err(|e| ApiError::invalid(e.to_string()))
}
