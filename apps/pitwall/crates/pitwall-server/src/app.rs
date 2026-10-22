//! The application state shared by every route: settings, the managers, the live feed, and the request pipeline
//! (authorise, rate-limit, resolve the model, queue, load, run, record) all API families go through.

use crate::auth::{Auth, Principal};
use crate::egress::Egress;
use crate::engines::{Engine, LlamaCpp, Planned};
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
use pitwall_core::mgmt::{self, ChosenBy, EngineKind, LiveUpdate, RequestRecord, Throughput};
use pitwall_core::runtimes::BuildChoice;
use pitwall_core::scheduler::{JobKind, Priority};
use pitwall_core::usage::Usage;
use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

const MAX_RECORDS: usize = 500;

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
    /// All engines (llama.cpp first).
    pub engines: Vec<Arc<dyn Engine>>,
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
    /// Per-app counters.
    pub usage: Mutex<Usage>,
    /// Detected hardware.
    pub hardware: Hardware,
    /// When the server started.
    pub started_at: u64,
    /// Pushed updates, as JSON text.
    pub live: broadcast::Sender<String>,
    /// Recent requests (metadata only).
    pub requests: Mutex<VecDeque<RequestRecord>>,
    /// Recent request speeds for the dashboard tiles.
    pub recent: Mutex<VecDeque<(u64, f64, f64, u64)>>,
    /// Engine installs in progress or finished.
    pub installs: Mutex<std::collections::BTreeMap<String, mgmt::Download>>,
    /// Count of running engine installs' cancel flags.
    pub install_cancel: Mutex<std::collections::BTreeMap<String, crate::egress::CancelFlag>>,
    /// Secrets of just-created apps, handed over once to the UI.
    pub new_secrets: Mutex<std::collections::HashMap<String, String>>,
    /// The port actually bound.
    pub port: std::sync::atomic::AtomicU16,
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
        let engine: Arc<dyn Engine> = engine_override.unwrap_or_else(|| Arc::new(llama.clone()));
        let mut engines: Vec<Arc<dyn Engine>> = vec![engine.clone()];
        let planned = |kind, reason: &str| {
            Arc::new(Planned {
                kind,
                reason: reason.into(),
            }) as Arc<dyn Engine>
        };
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            engines.push(planned(
                EngineKind::Mlx,
                "MLX (Apple Silicon) is planned; llama.cpp with Metal serves models today.",
            ));
        } else {
            engines.push(planned(EngineKind::Mlx, "MLX needs an Apple Silicon Mac."));
        }
        engines.push(planned(
            EngineKind::Vllm,
            if cfg!(target_os = "linux") {
                "vLLM is planned for Linux machines with an NVIDIA GPU."
            } else {
                "vLLM runs on Linux with an NVIDIA GPU only."
            },
        ));
        engines.push(planned(EngineKind::WhisperCpp, "whisper.cpp speech-to-text is planned."));
        let library = Library::new(dirs.clone(), egress.clone());
        let events = Arc::new(Mutex::new(EventLog::default()));
        let settings = Arc::new(RwLock::new(settings));
        let serving = Serving::new(engine, library.clone(), settings.clone(), hardware.clone(), events.clone());
        let auth = Auth::open(dirs.clone());
        let metrics = std::fs::read_to_string(dirs.metrics())
            .ok()
            .filter(|t| t.len() < 32 << 20)
            .map_or_else(|| Metrics::new(30), |t| Metrics::restore(&t, 30));
        let (live, _) = broadcast::channel(256);
        let st = Arc::new(AppState {
            dirs,
            settings,
            offline,
            egress,
            llama,
            engines,
            library,
            serving,
            auth,
            events,
            metrics: Mutex::new(metrics),
            usage: Mutex::new(Usage::default()),
            hardware,
            started_at: now_ms(),
            live,
            requests: Mutex::new(VecDeque::new()),
            recent: Mutex::new(VecDeque::new()),
            installs: Mutex::new(Default::default()),
            install_cancel: Mutex::new(Default::default()),
            new_secrets: Mutex::new(Default::default()),
            port: std::sync::atomic::AtomicU16::new(0),
        });
        st.event(EventKind::EngineStarted, Severity::Info, "Pitwall started".into());
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
        Ok(s)
    }

    /// Write the discovery file (ADR 0009).
    pub fn write_discovery(&self, port: u16) {
        let d = pitwall_core::discovery::Discovery::new(env!("CARGO_PKG_VERSION"), std::process::id(), port, self.started_at);
        if let Ok(b) = serde_json::to_vec_pretty(&d) {
            let _ = paths::write_atomic(&self.dirs.discovery(), &b);
        }
    }

    /// Remove it on shutdown.
    pub fn remove_discovery(&self) {
        let _ = std::fs::remove_file(self.dirs.discovery());
    }

    /// Start the background tasks: release loop, live feed, metrics sampling.
    pub fn spawn_tasks(self: &Arc<Self>) {
        self.serving.spawn_tick_loop();
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
            }
        });
    }

    /// Memory and GPU reading for the dashboard.
    pub fn memory_sample(&self) -> mgmt::MemorySample {
        let sys = system::read_system(&self.hardware);
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

    /// Run the whole pipeline for a chat-like request and start generating.
    pub async fn start_chat(self: &Arc<Self>, who: &Principal, meta: Meta, mut req: ChatRequest) -> Result<Run, ApiError> {
        let cfg = self.cfg();
        let now = now_ms();
        req.check().map_err(ApiError::from)?;
        let (app, perms) = match who {
            Principal::App { id, permissions } => (id.clone(), Some(permissions.clone())),
            Principal::Admin => (AppId::parse("pitwall").map_err(|e| ApiError::invalid(e.to_string()))?, None),
            Principal::Keyless => (AppId::parse("local").map_err(|e| ApiError::invalid(e.to_string()))?, None),
        };
        let resolved = self.serving.resolve(&req.model, req.has_images(), !req.tools.is_empty())?;
        let wanted = meta.priority.or(req.priority).unwrap_or(Priority::Interactive);
        let mut app_cap = None;
        let priority = match &perms {
            Some(p) => {
                let pr = p
                    .authorize(Some(&resolved.id), wanted, !req.tools.is_empty(), meta.extended)
                    .map_err(denied)?;
                self.auth.admit(&app, p.limits.requests_per_min, now).map_err(denied)?;
                let used = lock(&self.usage).today(now, app.as_str()).tokens();
                limits::check_tokens(used, p.limits.tokens_per_day).map_err(denied)?;
                app_cap = p.limits.max_concurrent.map(|c| c as usize);
                pr
            }
            None => wanted,
        };
        if req.has_images() {
            // Checked after loading: the engine reports whether the projector is present.
        }
        let queued = Instant::now();
        let permit = self
            .serving
            .acquire(app.clone(), resolved.id.clone(), priority, meta.kind, app_cap)
            .await?;
        let (busy, cold) = self.serving.ensure(&resolved.id).await?;
        if req.has_images() && !busy.model().vision {
            return Err(ApiError::new(
                ErrorKind::InvalidRequest,
                format!(
                    "the model {} cannot read images. Choose a vision model (one downloaded with its mmproj file).",
                    resolved.id
                ),
            ));
        }
        let queued_ms = queued.elapsed().as_millis() as u64;
        let name = resolved.id.to_string();
        let timeout = Duration::from_secs(cfg.server.request_timeout_secs);
        req.model = ModelRef::Named(name.clone());
        let job = permit.id();
        let chosen_by = resolved.chosen_by;
        let stream = req.stream;
        let rx = self.serving.chat(busy, permit, req, timeout);
        Ok(Run {
            rx,
            resolved,
            cold_start_ms: cold,
            job,
            rec: Recorder {
                st: self.clone(),
                app: app.to_string(),
                family: meta.family,
                endpoint: meta.endpoint,
                priority,
                started: Instant::now(),
                queued_ms,
                ttft: None,
                stream,
                cold_start_ms: cold,
                text: String::new(),
                model: name,
                chosen_by,
                done: false,
            },
        })
    }
}

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
}

/// A started request.
pub struct Run {
    /// The events as the engine produces them.
    pub rx: mpsc::Receiver<Result<ChatEvent, EngineFail>>,
    /// The model and how it was chosen.
    pub resolved: Resolved,
    /// Set when the model had to be loaded.
    pub cold_start_ms: Option<u64>,
    /// The queue job id.
    pub job: u64,
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
    queued_ms: u64,
    ttft: Option<u64>,
    stream: bool,
    cold_start_ms: Option<u64>,
    text: String,
    model: String,
    chosen_by: ChosenBy,
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
        lock(&self.st.usage).record(
            now,
            &self.app,
            u64::from(usage.prompt_tokens),
            u64::from(usage.completion_tokens),
            total_ms,
            failed,
        );
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
        let rec = RequestRecord {
            id: format!("r{now}-{}", lock(&self.st.requests).len()),
            at: now,
            app_id: self.app.clone(),
            api: self.family,
            endpoint: self.endpoint.clone(),
            model_id: Some(self.model.clone()),
            chosen_by: self.chosen_by,
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
            has_content: false,
        };
        let mut q = lock(&self.st.requests);
        q.push_front(rec);
        q.truncate(MAX_RECORDS);
    }
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

/// The model for `ModelId` strings from the UI.
pub fn model_id(s: &str) -> Result<ModelId, ApiError> {
    ModelId::parse(s).map_err(|e| ApiError::invalid(e.to_string()))
}
