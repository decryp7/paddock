//! The agent's shared state: the ports to the OS, the store, the user's settings and rules, the latest measurement
//! and the subscribers. [`Sampler`](crate::sampler) feeds it; [`handlers`](crate::handlers) answer the app's requests
//! from it. Locks are short and never held across a call into the OS, except process control (which must be serial).

use crate::files;
use crate::helper_centre::HelperCentre;
use crate::helper_link::{ElevatedStartup, ElevatedTraffic, ElevatedTuning, HelperLink, IpcHelperLink};
use crate::paths::Dirs;
use crate::rollup::HistoryWriter;
use crate::store::Store;
use dyno_core::activity::{ActivityEntry, ActivityKind, UndoRecord, UndoState};
use dyno_core::alerts::Alerts;
use dyno_core::facts::SystemFacts;
use dyno_core::grouping::AppGroup;
use dyno_core::metrics::SystemSample;
use dyno_core::ports::{FactsSource, FileLocks, PlatformError, ProcessControl, ProcessSource, StartupItems, UserActivity};
use dyno_core::process::{Priority, ProcessRef, ProcessSample};
use dyno_core::protocol::{LiveDetail, LiveUpdate, Message};
use dyno_core::rules::{Engine, RuleSet};
use dyno_core::settings::Settings;
use dyno_core::time::Timestamp;
use dyno_platform::Platform;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
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

/// A connected app that wants live updates.
pub struct Sub {
    /// Connection id.
    pub id: u64,
    /// 0 = not subscribed yet, 1 = summary, 2 = processes.
    pub detail: Arc<AtomicU8>,
    /// Bounded: a slow app loses frames, it never slows the sampler.
    pub tx: SyncSender<Message>,
}

impl Sub {
    /// What this subscriber wants.
    pub fn wants(&self) -> Option<LiveDetail> {
        match self.detail.load(Ordering::Relaxed) {
            1 => Some(LiveDetail::Summary),
            2 => Some(LiveDetail::Processes),
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
    /// The user's rules.
    pub rules: RuleSet,
    /// The rules engine's state.
    pub engine: Engine,
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
    /// Priorities rules replaced, so the undo restores them.
    pub previous_priority: HashMap<ProcessRef, Priority>,
    /// Facts and when they were read.
    pub facts: Option<(std::time::Instant, SystemFacts)>,
    /// Activity ids made this run.
    pub counter: u64,
    /// The hardware inventory, read once (at start or on first request).
    pub inventory: Option<dyno_core::inventory::SystemInventory>,
    /// Alarms: the engine and its persistence.
    pub alarms: crate::alarms::AlarmCentre,
    /// The flight recorder.
    pub recorder: crate::recorder::Recorder,
    /// Apps that are crashing repeatedly now (refreshed after each read of the OS logs); the crash-loop alert and the
    /// health report read them.
    pub crash_loops: Vec<dyno_core::anomaly::Anomaly>,
}

impl Core {
    /// An id for a new activity entry.
    pub fn next_id(&mut self, at: Timestamp) -> String {
        self.counter += 1;
        format!("{}-{}", at.millis(), self.counter)
    }

    /// Saves settings and rules to disk (best effort: a read-only data folder must not stop the agent).
    pub fn save_settings(&self, dirs: &Dirs) {
        if let Err(e) = files::save(&dirs.settings(), &self.settings) {
            eprintln!("dyno-agent: could not save settings: {e}");
        }
    }

    /// Saves the rules.
    pub fn save_rules(&self, dirs: &Dirs) {
        if let Err(e) = files::save(&dirs.rules(), &self.rules) {
            eprintln!("dyno-agent: could not save rules: {e}");
        }
    }

    /// Records an entry in the activity log and returns it with its undo state filled in.
    pub fn log(
        &mut self,
        at: Timestamp,
        kind: ActivityKind,
        title: String,
        detail: String,
        undo: UndoRecord,
        bytes: Option<u64>,
    ) -> ActivityEntry {
        let id = self.next_id(at);
        let state = match &undo {
            UndoRecord::None => UndoState::NotUndoable,
            UndoRecord::Process { actions, .. } if actions.is_empty() => UndoState::NotUndoable,
            _ => UndoState::Available,
        };
        let entry = ActivityEntry {
            id,
            at,
            kind,
            title,
            detail,
            undo: state,
            bytes,
        };
        if let Err(e) = self.store.put_activity(&entry, &undo) {
            eprintln!("dyno-agent: could not record activity: {e}");
        }
        entry
    }
}

/// The ports that talk to the OS, each behind its own lock.
pub struct Ports {
    /// Acting on processes.
    pub control: Mutex<Box<dyn ProcessControl>>,
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
    /// The few system settings tuning may change (observe-only until a click).
    pub tuning: Mutex<Box<dyn dyno_core::ports::TuningSystem>>,
    /// The privileged helper's lifecycle: its state, and installing it on a click (WP-01, ADR 0016).
    pub helper: Mutex<Box<dyn crate::helper_centre::HelperControl>>,
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
    /// The OS problem logs (observe-only).
    pub issues: crate::issues::IssueCentre,
    /// The baseline of programs seen on this machine (observe-only).
    pub unusual: crate::unusual::UnusualCentre,
    /// Benchmark runs (only on the user's click).
    pub bench: crate::bench::BenchCentre,
    /// Cleanup: categories, scans and the quarantine (deletes only on the user's confirmed click).
    pub cleanup: crate::cleanup::centre::CleanupCentre,
    /// Logical cores (for validating affinity and normalising overhead).
    pub cores: usize,
    stop: AtomicBool,
    wake: (Mutex<bool>, Condvar),
    next_conn: AtomicU64,
}

impl Shared {
    /// Builds the state: opens the store (moving a damaged one aside), loads settings and rules (falling back to
    /// defaults with a note in the activity log). Returns the sampler's two ports separately. `details` is a second
    /// process source used only for on-demand details, so a slow `lsof` never delays a sampling tick.
    pub fn build(dirs: Dirs, platform: Platform, details: Box<dyn ProcessSource>, reveal: Reveal) -> Result<Built, String> {
        let t = now();
        let mut notes: Vec<String> = Vec::new();
        if let Err(e) = std::fs::create_dir_all(dirs.root()) {
            notes.push(format!(
                "Dyno could not create {} ({e}); history is kept in memory only.",
                dirs.root().display()
            ));
        }
        let (store, note) = match Store::open(&dirs.database(), t) {
            Ok(x) => x,
            Err(e) => {
                notes.push(format!(
                    "The history could not be stored on disk ({e}); Dyno keeps it in memory until it restarts."
                ));
                (Store::memory().map_err(|e| format!("SQLite is unavailable: {e}"))?, None)
            }
        };
        notes.extend(note);
        let (settings, warn) = files::load(&dirs.settings(), dyno_core::settings::parse);
        notes.extend(warn);
        let settings = settings.unwrap_or_default().normalised();
        let (rules, warn) = files::load(&dirs.rules(), |text| {
            let (set, w) = files::json_or_default::<RuleSet>(text);
            match set.validate() {
                Ok(valid) => (valid, w),
                Err(e) => (
                    RuleSet::default(),
                    Some(format!("The saved rules were not valid ({e}); Dyno uses its defaults.")),
                ),
            }
        });
        notes.extend(warn);
        let rules = rules.unwrap_or_default();
        let alarms = crate::alarms::AlarmCentre::load(&store, settings.alarms);
        let mut core = Core {
            store,
            settings,
            rules,
            engine: Engine::new(),
            alerts: Alerts::new(),
            custom: dyno_core::alarms::custom::CustomEvaluator::new(),
            latest: Latest::default(),
            recent: VecDeque::new(),
            writer: HistoryWriter::default(),
            previous_priority: HashMap::new(),
            facts: None,
            counter: 0,
            inventory: None,
            alarms,
            recorder: crate::recorder::Recorder::new(),
            crash_loops: Vec::new(),
        };
        for n in notes {
            core.log(
                t,
                ActivityKind::System,
                "Dyno started with a clean slate".into(),
                n,
                UndoRecord::None,
                None,
            );
        }
        let (bench, bench_note) = crate::bench::BenchCentre::load(
            dirs.benchmarks(),
            std::env::temp_dir(),
            Box::new(crate::bench::ProcessLauncher::beside_agent()),
        );
        if let Some(n) = bench_note {
            core.log(t, ActivityKind::System, "Benchmark history reset".into(), n, UndoRecord::None, None);
        }
        let (cleanup, cleanup_notes) = crate::cleanup::centre::CleanupCentre::load(
            &dirs,
            crate::cleanup::roots::Env::current(),
            Box::new(crate::cleanup::trash::OsTrash),
            t,
        );
        for n in cleanup_notes {
            core.log(
                t,
                ActivityKind::System,
                "A cleanup pack was ignored".into(),
                n,
                UndoRecord::None,
                None,
            );
        }
        let unusual = crate::unusual::UnusualCentre::load(&core.store);
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        // One link to the helper (if it is installed) serves the tuning, startup and traffic wrappers and the lifecycle.
        // Only the agent of the user's own data folder talks to the real helper or opens the real permission prompt; an agent
        // started on another folder (every test) gets a link to nothing and a lifecycle that cannot install (the suite lesson
        // about production names: names derive from the data folder).
        let own_folder = dirs.root() == Dirs::current().root();
        let link: Arc<dyn HelperLink> = if own_folder {
            Arc::new(IpcHelperLink::system())
        } else {
            Arc::new(IpcHelperLink::new(
                dirs.root().join("no-helper.sock").to_string_lossy().into_owned(),
                dirs.root().join("no-helper.token"),
            ))
        };
        let helper_centre = if own_folder {
            HelperCentre::system(link.clone())
        } else {
            HelperCentre::unavailable(
                link.clone(),
                "Dyno was started on a folder of its own, so it does not install the helper.",
            )
        };
        let Platform {
            sensors,
            processes,
            control,
            locks,
            startup,
            facts,
            activity,
            issues,
        } = platform;
        let shared = Shared {
            plugins: dyno_plugin_host::PluginHost::new(dyno_plugin_host::HostConfig::new(dirs.plugins())),
            dirs,
            core: Mutex::new(core),
            ports: Ports {
                control: Mutex::new(control),
                locks: Mutex::new(locks),
                details: Mutex::new(details),
                facts: Mutex::new(facts),
                activity: Mutex::new(activity),
                startup: Mutex::new(Box::new(ElevatedStartup::new(startup, link.clone()))),
                tuning: Mutex::new(Box::new(ElevatedTuning::new(dyno_platform::tuning::os_tuner(), link.clone()))),
                helper: Mutex::new(Box::new(helper_centre)),
            },
            subs: Mutex::new(Vec::new()),
            reveal,
            bench,
            cleanup,
            issues: crate::issues::IssueCentre::new(issues),
            unusual,
            network: crate::network::NetCentre::new(
                Box::new(dyno_platform::network::OsSockets),
                Arc::new(dyno_platform::network::OsReverseDns),
                Box::new(ElevatedTraffic::new(dyno_platform::network::OsTraffic, link)),
            ),
            cores,
            stop: AtomicBool::new(false),
            wake: (Mutex::new(false), Condvar::new()),
            next_conn: AtomicU64::new(1),
        };
        Ok((Arc::new(shared), sensors, processes))
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
        self.wake_sampler();
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
        let subs = lock(&self.subs);
        let mut best = None;
        for s in subs.iter() {
            match s.wants() {
                Some(LiveDetail::Processes) => return Some(LiveDetail::Processes),
                Some(LiveDetail::Summary) => best = Some(LiveDetail::Summary),
                None => {}
            }
        }
        best
    }

    /// Sends `m` to every connected app (dropping it for a slow one).
    pub fn broadcast(&self, m: &Message) {
        for s in lock(&self.subs).iter() {
            let _ = s.tx.try_send(m.clone());
        }
    }
}
