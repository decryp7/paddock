//! The agent's shared state: the ports to the OS, the store, the user's settings, the latest measurement
//! and the subscribers. [`Sampler`](crate::sampler) feeds it; [`handlers`](crate::handlers) answer the app's requests
//! from it. Locks are short and never held across a call into the OS, except process control (which must be serial).

use crate::files;
use crate::paths::Dirs;
use crate::rollup::HistoryWriter;
use crate::store::Store;
use dyno_core::activity::{ActivityEntry, ActivityKind};
use dyno_core::alarms::AlarmChange;
use dyno_core::alerts::Alerts;
use dyno_core::facts::SystemFacts;
use dyno_core::grouping::AppGroup;
use dyno_core::metrics::SystemSample;
use dyno_core::ports::{FactsSource, FileLocks, PlatformError, ProcessSource, StartupItems, SystemSensors, UserActivity};
use dyno_core::process::ProcessSample;
use dyno_core::protocol::{LiveDetail, LiveUpdate, Message};
use dyno_core::settings::Settings;
use dyno_core::time::Timestamp;
use dyno_platform::Platform;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

/// Locks, recovering from poisoning: a panic in one request must not wedge the agent.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The current time.
pub fn now() -> Timestamp {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    Timestamp(i64::try_from(ms).unwrap_or(i64::MAX))
}

/// Recent samples kept for diagnosis.
pub const RECENT_SAMPLES: usize = 180;
/// Facts are refreshed at most this often.
pub const FACTS_TTL: Duration = Duration::from_secs(300);

/// Disk scan statuses waiting for one app: the latest of each scan. A status replaces the older one of its scan, so a slow app
/// sees fewer events but always the newest, and a final state (done, cancelled, failed) is never dropped.
#[derive(Default)]
pub struct ScanSlot(Mutex<std::collections::BTreeMap<u64, dyno_core::disk::scan::DiskScanStatus>>);

impl ScanSlot {
    /// Keeps `status` unless a newer event of the same scan is already waiting.
    pub fn put(&self, status: dyno_core::disk::scan::DiskScanStatus) {
        let mut m = lock(&self.0);
        match m.get(&status.scan) {
            Some(old) if old.seq >= status.seq => {}
            _ => {
                m.insert(status.scan, status);
            }
        }
    }

    /// Everything waiting, oldest scan first; the slot is empty afterwards.
    pub fn take(&self) -> Vec<dyno_core::disk::scan::DiskScanStatus> {
        std::mem::take(&mut *lock(&self.0)).into_values().collect()
    }
}

/// Job statuses waiting for one app: the newest of each job. A status replaces the older one of its job, so a slow app sees
/// fewer events but always the newest, and a final state (done, failed, cancelled) is never dropped.
#[derive(Default)]
pub struct JobSlot(Mutex<std::collections::BTreeMap<u64, paddock_jobs::wire::WireSnapshot>>);

impl JobSlot {
    /// Keeps `status` unless a newer event of the same job is already waiting.
    pub fn put(&self, status: paddock_jobs::wire::WireSnapshot) {
        let mut m = lock(&self.0);
        match m.get(&status.id) {
            Some(old) if old.seq >= status.seq => {}
            _ => {
                m.insert(status.id, status);
            }
        }
    }

    /// Everything waiting, oldest job first; the slot is empty afterwards.
    pub fn take(&self) -> Vec<paddock_jobs::wire::WireSnapshot> {
        std::mem::take(&mut *lock(&self.0)).into_values().collect()
    }
}

/// The newest volume list waiting for one app, with the ticket of the listing it came from: an older listing never replaces a
/// newer one, and the list is never dropped for a full queue.
#[derive(Default)]
pub struct VolumesSlot(Mutex<Option<(u64, Vec<dyno_core::disk::volume::Volume>)>>);

impl VolumesSlot {
    /// Keeps the list unless a newer listing's is already waiting.
    pub fn put(&self, ticket: u64, volumes: Vec<dyno_core::disk::volume::Volume>) {
        let mut m = lock(&self.0);
        if m.as_ref().is_none_or(|(old, _)| *old < ticket) {
            *m = Some((ticket, volumes));
        }
    }

    /// The list waiting, if any; the slot is empty afterwards.
    pub fn take(&self) -> Option<Vec<dyno_core::disk::volume::Volume>> {
        lock(&self.0).take().map(|(_, v)| v)
    }
}

/// The newest value of one whole-state event, waiting for a connection's push thread: it replaces an older one that was not sent yet, so
/// a full queue can never lose it (a lost push left a window showing an old state until it asked again).
pub struct LatestSlot<T>(Mutex<Option<T>>);

impl<T> Default for LatestSlot<T> {
    fn default() -> Self {
        LatestSlot(Mutex::new(None))
    }
}

impl<T> LatestSlot<T> {
    /// Keeps `value` as the one waiting, replacing an older one.
    pub fn put(&self, value: T) {
        *lock(&self.0) = Some(value);
    }

    /// The value waiting, if any; the slot is empty afterwards.
    pub fn take(&self) -> Option<T> {
        lock(&self.0).take()
    }
}

/// Most alarms with changes waiting for one connection (a few per rule; a slow app that falls further behind loses the oldest).
pub const MAX_PENDING_ALARMS: usize = 4096;

#[derive(Default)]
struct PendingAlarms {
    /// Per alarm id: the first change that asks to notify the user, and the latest change.
    by_id: std::collections::BTreeMap<u64, (Option<AlarmChange>, AlarmChange)>,
    lost: u32,
}

/// Alarm changes waiting for one connection, per alarm: the first one that notifies (so "raised, then cleared before the app read
/// either" still notifies) and the latest one (so the tray ends with the final state). At most [`MAX_PENDING_ALARMS`] alarms.
#[derive(Default)]
pub struct AlarmSlot(Mutex<PendingAlarms>);

impl AlarmSlot {
    /// Adds a change.
    pub fn put(&self, change: AlarmChange) {
        let mut pending = lock(&self.0);
        let id = change.alarm.id;
        if let Some(entry) = pending.by_id.get_mut(&id) {
            if change.notify && entry.0.is_none() {
                entry.0 = Some(change.clone());
            }
            entry.1 = change;
            return;
        }
        if pending.by_id.len() >= MAX_PENDING_ALARMS {
            pending.by_id.pop_first();
            pending.lost = pending.lost.saturating_add(1);
        }
        pending.by_id.insert(id, (change.notify.then(|| change.clone()), change));
    }

    /// The waiting changes in alarm order (the notifying one before the latest), and how many alarms were given up.
    pub fn take(&self) -> (Vec<AlarmChange>, u32) {
        let mut pending = lock(&self.0);
        let lost = std::mem::take(&mut pending.lost);
        let mut out = Vec::new();
        for (_, (first, last)) in std::mem::take(&mut pending.by_id) {
            if let Some(first) = first.filter(|f| *f != last) {
                out.push(first);
            }
            out.push(last);
        }
        (out, lost)
    }
}

/// Most events (activity rows and alerts) waiting for one connection.
pub const EVENT_QUEUE: usize = 64;

#[derive(Default)]
struct PendingEvents {
    items: std::collections::VecDeque<Message>,
    activity: u32,
    alerts: u32,
}

/// Events that are appended, never merged (activity rows, alerts), waiting for one connection: in order, at most [`EVENT_QUEUE`]; when a
/// slow app falls further behind the oldest are given up and counted.
#[derive(Default)]
pub struct EventQueue(Mutex<PendingEvents>);

impl EventQueue {
    /// Appends an event (`Message::Activity` or `Message::Alert`).
    pub fn push(&self, event: Message) {
        let mut q = lock(&self.0);
        if q.items.len() >= EVENT_QUEUE {
            match q.items.pop_front() {
                Some(Message::Activity(_)) => q.activity = q.activity.saturating_add(1),
                Some(Message::Alert(_)) => q.alerts = q.alerts.saturating_add(1),
                _ => {}
            }
        }
        q.items.push_back(event);
    }

    /// The waiting events in order, and how many activity rows and alerts were given up.
    pub fn take(&self) -> (Vec<Message>, u32, u32) {
        let mut q = lock(&self.0);
        let (activity, alerts) = (std::mem::take(&mut q.activity), std::mem::take(&mut q.alerts));
        (std::mem::take(&mut q.items).into(), activity, alerts)
    }
}

/// Everything one connection has waiting that does not travel through its bounded queue: the queue message only wakes the push thread,
/// which then takes the newest of each state and every waiting event from here, so a full queue loses nothing but what the bounds give up.
#[derive(Default)]
pub struct Pending {
    /// The Wake-on-LAN list.
    pub wol: LatestSlot<dyno_core::wol::WolView>,
    /// The power view.
    pub power: LatestSlot<dyno_core::power::PowerView>,
    /// The scheduled wakes.
    pub wakes: LatestSlot<dyno_core::power::schedule::WakesView>,
    /// A surface's widget layout (the newest edit of any window).
    pub layout: LatestSlot<dyno_core::widgets::DynoSurface>,
    /// Alarm changes.
    pub alarms: AlarmSlot,
    /// Activity rows and alerts.
    pub events: EventQueue,
}

impl Pending {
    /// Sends everything waiting, in a fixed order, through `send`; false when `send` says the app is gone. When a bound gave events up,
    /// a final [`Message::EventsLost`] says how many.
    pub fn drain(&self, send: &mut impl FnMut(Message) -> bool) -> bool {
        let mut lost = dyno_core::protocol::LostEvents::default();
        let mut messages = Vec::new();
        messages.extend(self.wol.take().map(|v| Message::WolChanged(Box::new(v))));
        messages.extend(self.power.take().map(|v| Message::PowerChanged(Box::new(v))));
        messages.extend(self.wakes.take().map(|v| Message::WakesChanged(Box::new(v))));
        messages.extend(self.layout.take().map(Message::WidgetLayoutChanged));
        let (alarms, lost_alarms) = self.alarms.take();
        messages.extend(alarms.into_iter().map(Message::Alarm));
        let (events, lost_activity, lost_alerts) = self.events.take();
        messages.extend(events);
        lost.alarms = lost_alarms;
        lost.activity = lost_activity;
        lost.alerts = lost_alerts;
        if lost != dyno_core::protocol::LostEvents::default() {
            messages.push(Message::EventsLost(lost));
        }
        messages.into_iter().all(send)
    }
}

/// The value [`Sub::detail`] holds for `detail` (0 is "not subscribed yet").
pub fn detail_code(detail: LiveDetail) -> u8 {
    match detail {
        LiveDetail::Summary => 1,
        LiveDetail::Processes => 2,
        LiveDetail::Tray => 3,
    }
}

/// A connected app that wants live updates.
pub struct Sub {
    /// Connection id.
    pub id: u64,
    /// 0 = not subscribed yet, otherwise [`detail_code`] of the detail.
    pub detail: Arc<AtomicU8>,
    /// Bounded: a slow app loses frames, it never slows the sampler.
    pub tx: SyncSender<Message>,
    /// Disk scan statuses, latest per scan: never a lossy queue (a final state must arrive).
    pub scans: Arc<ScanSlot>,
    /// The volume list, newest only, same reason.
    pub volumes: Arc<VolumesSlot>,
    /// Job statuses, latest per job, same reason.
    pub jobs: Arc<JobSlot>,
    /// Whole states and events that must not be lost to a full queue.
    pub pending: Arc<Pending>,
}

impl Sub {
    /// What this subscriber wants.
    pub fn wants(&self) -> Option<LiveDetail> {
        match self.detail.load(Ordering::Relaxed) {
            1 => Some(LiveDetail::Summary),
            2 => Some(LiveDetail::Processes),
            3 => Some(LiveDetail::Tray),
            _ => None,
        }
    }
}

/// The measurement most recently taken.
#[derive(Default)]
pub struct Latest {
    /// The process table at the last process tick.
    pub processes: Vec<ProcessSample>,
    /// The same, grouped by app.
    pub groups: Vec<AppGroup>,
    /// The last live update (with processes and groups).
    pub live: Option<LiveUpdate>,
}

/// Everything the lock on [`Shared::core`] protects.
pub struct Core {
    /// The history database.
    pub store: Store,
    /// The user's settings.
    pub settings: Settings,
    /// Alert evaluation state.
    pub alerts: Alerts,
    /// Custom alert rule evaluation state.
    pub custom: dyno_core::alarms::custom::CustomEvaluator,
    /// The last measurement.
    pub latest: Latest,
    /// Recent system samples, oldest first.
    pub recent: VecDeque<SystemSample>,
    /// History writer state.
    pub writer: HistoryWriter,
    /// Facts and when they were read.
    pub facts: Option<(std::time::Instant, SystemFacts)>,
    /// Activity ids made this run.
    pub counter: u64,
    /// The hardware inventory, read once (at start or on first request).
    pub inventory: Option<dyno_core::inventory::SystemInventory>,
    /// The helper epoch the cached inventory's helper readings were made at; when it moves they are read again.
    pub inventory_epoch: u64,
    /// Alarms: the engine and its persistence.
    pub alarms: crate::alarms::AlarmCentre,
    /// The flight recorder.
    pub recorder: crate::recorder::Recorder,
    /// Apps that are crashing repeatedly now (refreshed after each read of the OS logs); the crash-loop alert and the
    /// health report read them.
    pub crash_loops: Vec<dyno_core::anomaly::Anomaly>,
    /// The user's local time zone (from the platform): every local day, hour and minute goes through it.
    pub zone: dyno_core::time::Zone,
}

impl Core {
    /// An id for a new activity entry.
    pub fn next_id(&mut self, at: Timestamp) -> String {
        self.counter += 1;
        format!("{}-{}", at.millis(), self.counter)
    }

    /// Saves settings to disk (best effort: a read-only data folder must not stop the agent).
    pub fn save_settings(&self, dirs: &Dirs) {
        if let Err(e) = files::save(&dirs.settings(), &self.settings) {
            log::error!("dyno-agent: could not save settings: {e}");
        }
    }

    /// Records an entry in the activity log and returns it.
    pub fn log(&mut self, at: Timestamp, kind: ActivityKind, title: String, detail: String) -> ActivityEntry {
        let id = self.next_id(at);
        let entry = ActivityEntry {
            id,
            at,
            kind,
            title,
            detail,
        };
        if let Err(e) = self.store.put_activity(&entry) {
            log::error!("dyno-agent: could not record activity: {e}");
        }
        entry
    }
}

/// The ports that talk to the OS, each behind its own lock.
pub struct Ports {
    /// File locks.
    pub locks: Mutex<Box<dyn FileLocks>>,
    /// Process details (slow calls).
    pub details: Mutex<Box<dyn ProcessSource>>,
    /// Slow-changing facts.
    pub facts: Mutex<Box<dyn FactsSource>>,
    /// User activity.
    pub activity: Mutex<Box<dyn UserActivity>>,
    /// Startup entries.
    pub startup: Mutex<Box<dyn StartupItems>>,
    /// The network interfaces and Wi-Fi network (network change events).
    pub links: Mutex<Box<dyn dyno_core::ports::NetworkLinks>>,
}

/// Shows a file in the file manager (injectable so tests do not open Finder).
pub type Reveal = Box<dyn Fn(&Path) -> Result<(), PlatformError> + Send + Sync>;

/// The shared state and the two ports the sampler thread owns.
pub type Built = (Arc<Shared>, Box<dyn dyno_core::ports::SystemSensors>, Box<dyn ProcessSource>);

/// State shared by the sampler, the server and the request handlers.
pub struct Shared {
    /// The data folder.
    pub dirs: Dirs,
    /// Guarded state.
    pub core: Mutex<Core>,
    /// OS ports.
    pub ports: Ports,
    /// Connected apps.
    pub subs: Mutex<Vec<Sub>>,
    /// Plugin host (started on first use).
    pub plugins: dyno_plugin_host::PluginHost,
    /// Opens the file manager.
    pub reveal: Reveal,
    /// Open ports and connections (observe-only).
    pub network: crate::network::NetCentre,
    /// The user's IP lists, the match index, the location databases and the user's own location.
    pub intel: crate::intel_centre::IntelCentre,
    /// The firewall of this computer (read-only).
    pub firewall: crate::firewall::FirewallCentre,
    /// The performance counters browser (observe-only).
    pub counters: crate::counters::CounterCentre,
    /// The OS problem logs (observe-only).
    pub issues: crate::issues::IssueCentre,
    /// The baseline of programs seen on this machine (observe-only).
    pub unusual: crate::unusual::UnusualCentre,
    /// The extras checks (observe-only; removing finder results is a click).
    pub extras: crate::extras::ExtrasCentre,
    /// Benchmark runs (only on the user's click).
    pub bench: crate::bench::BenchCentre,
    /// Network change events on the timeline (WP-35).
    pub netwatch: crate::netwatch::NetWatchCentre,
    /// Long operations (find files, read the OS logs, hash a file) as jobs with progress and Cancel.
    pub jobs: crate::jobs::JobCentre,
    /// SHA-256 of program files for the VirusTotal search (WP-33); nothing is uploaded.
    pub hasher: crate::filehash::FileHasher,
    /// Throttling and hardware life: the OS's speed limit, SSD wear and battery capacity (observe-only).
    pub hardware: crate::hardware::HardwareCentre,
    /// "Is this normal?" baselines learned from history.
    pub baselines: crate::baselines::BaselineCentre,
    /// The disk map: volumes and scans (read-only).
    pub disk: Arc<crate::disk::centre::DiskCentre>,
    /// The saved-views cache (ADR 0029), when there is one: the disk map, the counter catalog and the software list keep
    /// their last answer in it.
    pub cache: Option<crate::cache::Cache>,
    /// Keep awake: the OS hold and its state machine (ADR 0032).
    pub power: Arc<crate::power::PowerCentre>,
    /// Wake-on-LAN: the saved computers and the wake job (ADR 0032).
    pub wol: crate::wol::WolCentre,
    /// Scheduled wake: the schedules, their OS wakes and what happens at their time (ADR 0032 section 5).
    pub wakes: crate::wakes::WakeCentre,
    /// The privileged helper: its state and Turn on / Repair / Update / Turn off (ADR 0027).
    pub helper: Arc<crate::helper_centre::HelperCentre<Box<dyn crate::helper_link::HelperTransport>>>,
    /// The memory layout: the helper first, then this computer (the seam's second instance, plan section 8.3).
    pub memory: Mutex<crate::helper_first::HelperFirst<crate::helper_readers::memory::MemoryModules>>,
    /// The helper's own log for the diagnostics bundle (plan section 7.1).
    pub helper_log: Mutex<crate::helper_first::HelperFirst<crate::diagnostics::HelperLog>>,
    /// Temperatures, fans and CPU energy from the helper (macOS; plan section 2.7).
    pub climate: Arc<crate::helper_readers::counters::HelperClimate>,
    /// Logical cores (for validating affinity and normalising overhead).
    pub cores: usize,
    /// Whether this agent reads through the helper (its own data folder, or the real-engine rig); tests stay off the machine (ER-14).
    pub helper_first: bool,
    /// Cancelled with the agent's stop: what runs for the agent alone (the process-sweep worker) ends with it.
    pub stop_token: paddock_jobs::CancelToken,
    stop: AtomicBool,
    wake: (Mutex<bool>, Condvar),
    next_conn: AtomicU64,
}

/// What the benchmark centre gets from the agent: a job in the Jobs view and the latest power source.
struct AgentBenchEnv(Weak<Shared>);

impl crate::bench::BenchEnv for AgentBenchEnv {
    fn register_job(&self, title: &str, subject: Option<&str>) -> Option<paddock_jobs::JobContext> {
        Some(self.0.upgrade()?.jobs.register_external("benchmark", title, subject))
    }

    fn on_battery(&self) -> Option<bool> {
        let shared = self.0.upgrade()?;
        let core = lock(&shared.core);
        core.recent
            .back()
            .and_then(|s| s.battery.value().map(|b| b.source == dyno_core::metrics::PowerSource::Battery))
    }

    fn volumes(&self) -> Vec<dyno_core::disk::volume::Volume> {
        self.0.upgrade().map(|shared| shared.disk.volumes()).unwrap_or_default()
    }

    fn available_memory(&self) -> Option<u64> {
        let shared = self.0.upgrade()?;
        let core = lock(&shared.core);
        core.recent.back().map(|s| s.memory.available)
    }

    fn throttle_input(&self) -> Option<dyno_core::throttle::ThrottleInput> {
        let shared = self.0.upgrade()?;
        let core = lock(&shared.core);
        core.recent.back().map(|s| {
            // What the OS says about its own limits is read by the hardware centre while the CPU is busy, as a run is.
            let ctx = shared.hardware.context();
            dyno_core::throttle::ThrottleInput::from_sample(s, ctx.power_saver, ctx.os)
        })
    }

    fn space(&self, mount: &str) -> Option<(u64, u64)> {
        dyno_platform::volumes::capacity(mount)
    }
}

/// The helper and the readers that stand in for the machine's own, for a run that serves the helper from somewhere else (the
/// real-engine rig, `tests/common/helper_rig.rs`). The real agent passes none: it uses the system connection and the platform.
pub struct HelperWiring {
    /// The helper's state and actions over the rig's transport.
    pub centre: crate::helper_centre::HelperCentre<Box<dyn crate::helper_link::HelperTransport>>,
    /// What this computer reads of its memory without the helper.
    pub local_memory: crate::helper_readers::memory::LocalMemory,
}

impl Shared {
    /// Builds the state: opens the store (moving a damaged one aside), loads settings (falling back to
    /// defaults with a note in the activity log). Returns the sampler's two ports separately. `details` is a second
    /// process source used only for on-demand details, so a slow `lsof` never delays a sampling tick.
    pub fn build(dirs: Dirs, platform: Platform, details: Box<dyn ProcessSource>, reveal: Reveal) -> Result<Built, String> {
        Shared::build_with(dirs, platform, details, reveal, crate::disk::DiskPorts::none())
    }

    /// [`Shared::build`] with the disk map's machine ports given (tests pass fixed ones).
    pub fn build_with(
        dirs: Dirs,
        platform: Platform,
        details: Box<dyn ProcessSource>,
        reveal: Reveal,
        disk_ports: crate::disk::DiskPorts,
    ) -> Result<Built, String> {
        Shared::build_with_helper(dirs, platform, details, reveal, disk_ports, None)
    }

    /// [`Shared::build_with`] with the helper given (`None`: the system's). Readings are composed over the helper when it is the
    /// real one for this data folder or a given one; a test agent with neither never asks a helper (ER-14).
    pub fn build_with_helper(
        dirs: Dirs,
        platform: Platform,
        details: Box<dyn ProcessSource>,
        reveal: Reveal,
        disk_ports: crate::disk::DiskPorts,
        wiring: Option<HelperWiring>,
    ) -> Result<Built, String> {
        let t = now();
        let mut notes: Vec<String> = Vec::new();
        if let Err(e) = crate::paths::prepare_root(dirs.root()) {
            notes.push(format!("{e}; history is kept in memory only."));
        }
        let (store, note, store_kind) = match Store::open(&dirs.database(), t) {
            Ok((s, n)) => (s, n, "file"),
            Err(e) => {
                notes.push(format!(
                    "The history could not be stored on disk ({e}); Dyno keeps it in memory until it restarts."
                ));
                (
                    Store::memory().map_err(|e| format!("SQLite is unavailable: {e}"))?,
                    None,
                    "memory (open failed)",
                )
            }
        };
        notes.extend(note);
        let (settings, warn) = files::load(&dirs.settings(), dyno_core::settings::parse);
        notes.extend(warn);
        let mut settings = settings.unwrap_or_default().normalised();
        // A dashboard read from Dyno 1.0's span layout: written at once under its new key, logged once (ADR 0044 §4, ER-27).
        let upgraded = settings.dashboard.take_upgraded().map(|from| {
            let dropped: u32 = settings
                .dashboard
                .notes()
                .iter()
                .map(|n| match n.code {
                    paddock_widgets::NoteCode::Dropped { count } => count,
                    _ => 0,
                })
                .sum();
            format!(
                "The dashboard layout was upgraded from version {from} to 2: {} placements, {dropped} dropped.",
                settings.dashboard.layout().placements.len()
            )
        });
        // The saved-maps cache (ADR 0029): a second, rebuildable file in the OS cache folder. Off for this run (said once) when its
        // folder cannot be written or another process holds the file; the disk map then works without it.
        let started = crate::cache::Cache::start(
            dirs.cache_db(),
            crate::cache::CacheSettings::from(settings.disk.saved_maps),
            crate::cache::CacheDeps {
                clock: Arc::new(crate::disk::centre::SystemClock::default()),
                free: Arc::new(crate::cache::db::OsFreeSpace),
            },
        );
        notes.extend(started.note);
        let saved_maps = started.cache;
        let saved_maps_settings = settings.disk.saved_maps;
        let settings_limits = settings.disk.limits();
        let (job_workers, read_threads, space_refresh) = (
            settings.jobs.worker_count(),
            settings.disk.read_threads,
            settings.disk.space_refresh_secs,
        );
        let zone = platform.zone.clone();
        let first_seen = crate::baselines::on_start(&store, t);
        crate::baselines::log_start(dirs.kind(), store_kind, first_seen, &settings.baselines, t, zone.as_ref());
        let alarms = crate::alarms::AlarmCentre::load(&store, settings.alarms, zone.clone());
        let mut core = Core {
            store,
            settings,
            alerts: Alerts::new(),
            custom: dyno_core::alarms::custom::CustomEvaluator::new(),
            latest: Latest::default(),
            recent: VecDeque::new(),
            writer: HistoryWriter::default(),
            facts: None,
            counter: 0,
            inventory: None,
            inventory_epoch: 0,
            alarms,
            recorder: crate::recorder::Recorder::new(),
            crash_loops: Vec::new(),
            zone,
        };
        for n in notes {
            core.log(t, ActivityKind::System, "Dyno started with a clean slate".into(), n);
        }
        if let Some(detail) = upgraded {
            core.log(t, ActivityKind::System, "Dashboard layout upgraded".into(), detail);
            core.save_settings(&dirs);
        }
        let (bench, bench_note) = crate::bench::BenchCentre::load(
            dirs.benchmarks(),
            std::env::temp_dir(),
            Box::new(crate::bench::ProcessLauncher::beside_agent(&dirs.benchmarks())),
        );
        if let Some(n) = bench_note {
            core.log(t, ActivityKind::System, "Benchmark history reset".into(), n);
        }
        let unusual = crate::unusual::UnusualCentre::load(&core.store);
        let saved_cpu_energy = crate::helper_readers::counters::parse_saved(
            core.store.meta(crate::helper_readers::counters::META_KEY).ok().flatten().as_deref(),
        );
        let power_marker = dirs.root().join("keep-awake.active");
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let own_folder = Dirs::try_current().is_ok_and(|current| dirs.root() == current.root());
        let own_tools = dirs.tools();
        let helper_first = own_folder || wiring.is_some();
        let (helper_centre, local_memory) = match wiring {
            Some(w) => (w.centre, w.local_memory),
            None => (
                crate::helper_os::centre(),
                Box::new(dyno_platform::inventory::read_memory_layout) as crate::helper_readers::memory::LocalMemory,
            ),
        };
        let host = helper_centre.host();
        let Platform {
            sensors,
            processes,
            locks,
            startup,
            facts,
            activity,
            issues,
            links,
            zone: _,
            counters,
            network,
            hardware,
            power,
            wol,
        } = platform;
        // The readings' port: the helper of this agent, or none where a test agent must not touch the machine (ER-14).
        let port = |me: &std::sync::Weak<Shared>| -> Box<dyn crate::helper_first::HelperPort> {
            if helper_first {
                Box::new(crate::helper_first::SharedPort(me.clone()))
            } else {
                Box::new(crate::helper_first::OffPort)
            }
        };
        let shared = Arc::new_cyclic(|me| Shared {
            firewall: crate::firewall::FirewallCentre::for_this_os(Arc::new(paddock_jobs::time::SystemClock)),
            intel: crate::intel_centre::IntelCentre::new(dirs.clone(), Arc::new(paddock_jobs::time::SystemClock)),
            plugins: dyno_plugin_host::PluginHost::new(dyno_plugin_host::HostConfig::new(dirs.plugins())),
            dirs,
            core: Mutex::new(core),
            ports: Ports {
                // Linux: the helper first (it sees every account's descriptors), then the agent's own reader.
                locks: Mutex::new(if helper_first && host == dyno_core::helper::HelperHost::Linux {
                    Box::new(crate::helper_readers::holders::HelperLocks::new(locks, port(me)))
                } else {
                    locks
                }),
                details: Mutex::new(details),
                facts: Mutex::new(facts),
                activity: Mutex::new(activity),
                startup: Mutex::new(startup),
                links: Mutex::new(links),
            },
            subs: Mutex::new(Vec::new()),
            reveal,
            bench,
            netwatch: crate::netwatch::NetWatchCentre::new(),
            jobs: crate::jobs::JobCentre::new(me.clone(), job_workers),
            hasher: crate::filehash::FileHasher::new(),
            counters: crate::counters::CounterCentre::new(counters),
            issues: crate::issues::IssueCentre::new(issues),
            unusual,
            extras: crate::extras::ExtrasCentre::new(own_folder),
            network: {
                // Windows reads per-program traffic through the helper first (its counters need administrator rights).
                let traffic = if host == dyno_core::helper::HelperHost::Windows {
                    Box::new(crate::helper_readers::traffic::HelperTraffic::new(network.traffic, port(me)))
                } else {
                    network.traffic
                };
                let centre = crate::network::NetCentre::new(network.sockets, network.dns, traffic);
                if helper_first && host == dyno_core::helper::HelperHost::Linux {
                    // Linux hides the program of other accounts' sockets: the helper's map fills them.
                    centre.with_owners(crate::helper_readers::holders::SocketOwnerStep::for_this_os(
                        crate::helper_first::HelperFirst::new(crate::helper_readers::holders::SocketOwnerReading, port(me)),
                    ))
                } else {
                    centre
                }
            },
            hardware: crate::hardware::HardwareCentre::new(
                hardware.signals,
                if helper_first {
                    // The real machine: the helper first, then the local reader (which also finds a person's own install).
                    Box::new(crate::helper_wear::helper_first_wear(
                        port(me),
                        dyno_platform::hardware::disk_wear_with_own_tools(&own_tools),
                    ))
                } else {
                    hardware.wear
                },
                hardware.battery,
            ),
            baselines: crate::baselines::BaselineCentre::new(),
            cache: saved_maps.clone(),
            disk: Arc::new(crate::disk::centre::DiskCentre::new(
                Arc::new(crate::disk::fs::OsDirSource),
                disk_ports.volumes,
                Arc::new(crate::disk::BroadcastStatus(me.clone())),
                Arc::new(crate::disk::centre::SystemClock::default()),
                disk_ports.access,
                cores,
                settings_limits,
            )),
            power: Arc::new(crate::power::PowerCentre::new(
                power.os,
                power.events,
                power.actions,
                power.signals,
                power_marker,
            )),
            wakes: crate::wakes::WakeCentre::new(wol.wake_timer.clone(), Arc::new(crate::wakes::SharedHelper::new(me.clone()))),
            wol: crate::wol::WolCentre::new({
                let mut wol = wol;
                // Linux: each card that may wake the computer is judged by its own mode, read by the helper (plan section 7.1).
                if host == dyno_core::helper::HelperHost::Linux && helper_first {
                    wol.readiness = Arc::new(crate::helper_readers::wol::HelperReadiness::new(wol.readiness.clone(), port(me)));
                }
                wol
            }),
            helper: Arc::new(helper_centre),
            memory: Mutex::new(crate::helper_first::HelperFirst::new(
                crate::helper_readers::memory::MemoryModules::new(host, local_memory),
                port(me),
            )),
            helper_log: Mutex::new(crate::helper_first::HelperFirst::new(crate::diagnostics::HelperLog, port(me))),
            climate: Arc::new(crate::helper_readers::counters::HelperClimate::new(
                port(me),
                helper_first && host == dyno_core::helper::HelperHost::Macos,
                saved_cpu_energy,
            )),
            cores,
            helper_first,
            stop_token: paddock_jobs::CancelToken::new(),
            stop: AtomicBool::new(false),
            wake: (Mutex::new(false), Condvar::new()),
            next_conn: AtomicU64::new(1),
        });
        // What keeps Windows awake is read helper first.
        if helper_first {
            shared
                .extras
                .use_helper(Arc::new(crate::helper_first::SharedPort(Arc::downgrade(&shared))));
            // The firewall's sources only the helper reads (`pf`, `nft`) are asked through the same port.
            shared
                .firewall
                .bind_helper(Arc::new(crate::helper_first::SharedPort(Arc::downgrade(&shared))));
            // macOS reads login items and background tasks only through the helper; other systems' startup lists ignore the link.
            lock(&shared.ports.startup).use_login_items(Box::new(crate::helper_readers::login_items::HelperLoginItems::new(Box::new(
                crate::helper_first::SharedPort(Arc::downgrade(&shared)),
            ))));
        }
        shared.disk.set_read_threads(read_threads);
        shared.disk.set_space_refresh(space_refresh);
        shared.disk.attach_cache(saved_maps.clone());
        shared.counters.attach_cache(saved_maps);
        shared.disk.apply_saved_maps(saved_maps_settings);
        shared.bench.attach(Arc::new(AgentBenchEnv(Arc::downgrade(&shared))));
        let sensors: Box<dyn SystemSensors> = if shared.climate.active() {
            Box::new(crate::helper_readers::counters::HelperSensors::new(sensors, shared.climate.clone()))
        } else {
            sensors
        };
        Ok((shared, sensors, processes))
    }

    /// A new connection id.
    pub fn new_conn_id(&self) -> u64 {
        self.next_conn.fetch_add(1, Ordering::Relaxed)
    }

    /// Whether the agent should stop.
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Asks every loop to stop.
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.stop_token.cancel();
        self.wake_sampler();
        self.power.wake();
        self.wakes.wake();
    }

    /// Cuts the sampler's sleep short (a subscriber arrived, settings changed, the agent is stopping).
    pub fn wake_sampler(&self) {
        *lock(&self.wake.0) = true;
        self.wake.1.notify_all();
    }

    /// Sleeps up to `d` or until woken.
    pub fn sleep(&self, d: Duration) {
        let guard = lock(&self.wake.0);
        let (mut woken, _) = self
            .wake
            .1
            .wait_timeout_while(guard, d, |w| !*w && !self.stopped())
            .unwrap_or_else(PoisonError::into_inner);
        *woken = false;
    }

    /// The most detailed live view anyone wants.
    pub fn watching(&self) -> Option<LiveDetail> {
        lock(&self.subs).iter().filter_map(Sub::wants).max_by_key(|d| d.rank())
    }

    /// Hands a scan status to every connected app: it replaces the older one of the same scan that has not been sent yet, and
    /// the connection's push thread is woken. Nothing is lost but intermediate events.
    pub fn broadcast_scan(&self, status: &dyno_core::disk::scan::DiskScanStatus) {
        for s in lock(&self.subs).iter() {
            s.scans.put(status.clone());
            // The message only wakes the push thread; the content is in the slot. A full queue means the thread is awake
            // anyway and looks at the slot after every message.
            let _ = s.tx.try_send(Message::DiskScan(Box::new(status.clone())));
        }
    }

    /// Hands a job's status to every connected app: it replaces the older one of the same job that has not been sent yet, and
    /// the connection's push thread is woken. Nothing is lost but intermediate events.
    pub fn broadcast_job(&self, status: &paddock_jobs::wire::WireSnapshot) {
        for s in lock(&self.subs).iter() {
            s.jobs.put(status.clone());
            let _ = s.tx.try_send(Message::Job(Box::new(status.clone())));
        }
    }

    /// Hands a changed volume list to every connected app, newest listing wins; like scan statuses it is never lost to a full queue.
    pub fn broadcast_volumes(&self, ticket: u64, volumes: &[dyno_core::disk::volume::Volume]) {
        for s in lock(&self.subs).iter() {
            s.volumes.put(ticket, volumes.to_vec());
            let _ = s.tx.try_send(Message::DiskVolumes(Vec::new()));
        }
    }

    /// Hands `wake` to every connected app after `store` put the event into that connection's [`Pending`]: the message only wakes the
    /// connection's push thread, so a full queue cannot lose the event.
    fn publish(&self, store: impl Fn(&Pending), wake: &Message) {
        for s in lock(&self.subs).iter() {
            store(&s.pending);
            let _ = s.tx.try_send(wake.clone());
        }
    }

    /// The Wake-on-LAN list changed; the newest replaces an older one that was not sent yet.
    pub fn broadcast_wol(&self, view: &dyno_core::wol::WolView) {
        self.publish(|p| p.wol.put(view.clone()), &Message::WolChanged(Box::new(view.clone())));
    }

    /// The power view changed; the newest replaces an older one that was not sent yet.
    pub fn broadcast_power(&self, view: &dyno_core::power::PowerView) {
        self.publish(|p| p.power.put(view.clone()), &Message::PowerChanged(Box::new(view.clone())));
    }

    /// The scheduled wakes changed; the newest replaces an older one that was not sent yet.
    pub fn broadcast_wakes(&self, view: &dyno_core::power::schedule::WakesView) {
        self.publish(|p| p.wakes.put(view.clone()), &Message::WakesChanged(Box::new(view.clone())));
    }

    /// A surface's widget layout changed; the newest replaces an older one that was not sent yet.
    pub fn broadcast_layout(&self, surface: &dyno_core::widgets::DynoSurface) {
        self.publish(|p| p.layout.put(*surface), &Message::WidgetLayoutChanged(*surface));
    }

    /// An alarm changed: every connection keeps the first change that notifies and the latest one, per alarm.
    pub fn broadcast_alarm(&self, change: &AlarmChange) {
        self.publish(|p| p.alarms.put(change.clone()), &Message::Alarm(change.clone()));
    }

    /// An activity row was appended: every connection queues it, in order, up to [`EVENT_QUEUE`].
    pub fn broadcast_activity(&self, entry: &dyno_core::activity::ActivityEntry) {
        self.publish(
            |p| p.events.push(Message::Activity(entry.clone())),
            &Message::Activity(entry.clone()),
        );
    }

    /// An alert fired: every connection queues it, in order, up to [`EVENT_QUEUE`].
    pub fn broadcast_alert(&self, alert: &dyno_core::alerts::Alert) {
        self.publish(|p| p.events.push(Message::Alert(alert.clone())), &Message::Alert(alert.clone()));
    }

    /// Sends `m` to every connected app, dropping it for a slow one: only for frames that are re-sent anyway (the live update). State and
    /// events go through the `broadcast_*` functions above (a test keeps it that way).
    pub fn broadcast(&self, m: &Message) {
        for s in lock(&self.subs).iter() {
            let _ = s.tx.try_send(m.clone());
        }
    }
}
