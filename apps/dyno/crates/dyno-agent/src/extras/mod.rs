//! The extras (WP-13) and backup health (WP-14) on the agent side: runs the read-only OS tools behind a [`ToolRunner`],
//! keeps the few results worth caching, records the changes the "What changed" timeline needs, and answers the requests.
//! Everything is observe-only: nothing here moves or removes a file ([`finder`] only lists).

pub mod backup_check;
pub mod finder;

use crate::agent::{lock, now, Shared};
use dyno_core::extras::boot::{self, BootReport, BootRow, SettleTracker};
use dyno_core::extras::changes::{diff_software, diff_startup, timeline, Change, ChangesQuery};
use dyno_core::extras::gpu::GpuReport;
use dyno_core::extras::software::{render_report, validate_destination, InventoryFormat, SoftwareInventory, SoftwareItem};
use dyno_core::extras::usage::{self, UsageReport};
use dyno_core::protocol::{AgentError, BootRecord, ErrorCode, Reply, Request, SavedInventory};
use dyno_core::time::{Timestamp, DAY, HOUR, SECOND};
use dyno_platform::issues::{OsRunner, ToolRunner};
use dyno_platform::toolpath::PathResolver;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a software list is reused (reading it takes seconds to minutes).
const SOFTWARE_TTL: Duration = Duration::from_secs(10 * 60);
/// How often the background work runs (backup check, change recording).
pub const BACKGROUND_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// The first background run waits this long after the agent starts.
pub const FIRST_RUN_AFTER: Duration = Duration::from_secs(90);
/// How long to look for the desktop settling after start, and what counts as settled.
const SETTLE_WITHIN: Duration = Duration::from_secs(10 * 60);
const SETTLE_CPU: f32 = 30.0;
const SETTLE_HOLD_MS: i64 = 30 * SECOND;
/// A boot is recorded only when the agent starts this soon after it (otherwise the agent was merely restarted).
const BOOT_WINDOW_MS: i64 = 15 * 60 * SECOND;

const KEY_SOFTWARE: &str = "changes.software";
const KEY_STARTUP: &str = "changes.startup";
const KEY_SINCE: &str = "changes.since";

/// Whether Dyno may start heavier background work now (always, unless the user set maintenance windows).
pub fn maintenance_open(shared: &Shared) -> bool {
    let core = lock(&shared.core);
    core.settings.maintenance.is_open(now(), core.zone.as_ref())
}

fn bad(msg: impl Into<String>) -> AgentError {
    AgentError::new(ErrorCode::BadRequest, msg)
}

fn internal(msg: impl std::fmt::Display) -> AgentError {
    AgentError::new(ErrorCode::Internal, msg.to_string())
}

/// See the module docs.
pub struct ExtrasCentre {
    tools: Mutex<Arc<dyn ToolRunner>>,
    resolver: Mutex<Arc<PathResolver>>,
    /// Tests: runs every backup tool instead of resolving programs.
    backup_tools: Mutex<Option<Arc<dyn ToolRunner>>>,
    os: Mutex<&'static str>,
    own_folder: bool,
    software: Mutex<Option<(Instant, SoftwareInventory)>>,
    pub(crate) backup: Arc<Mutex<backup_check::Slot>>,
    /// Signalled when a backup check ends and when a job waiting for one is cancelled.
    pub(crate) backup_done: Arc<std::sync::Condvar>,
    counter: AtomicU64,
}

impl ExtrasCentre {
    /// The real tools. `own_folder` is whether this agent uses the real data folder; only then do background checks run.
    pub fn new(own_folder: bool) -> ExtrasCentre {
        ExtrasCentre {
            tools: Mutex::new(Arc::new(OsRunner)),
            resolver: Mutex::new(Arc::new(PathResolver::system())),
            backup_tools: Mutex::new(None),
            os: Mutex::new(std::env::consts::OS),
            own_folder,
            software: Mutex::new(None),
            backup: Arc::new(Mutex::new(backup_check::Slot::default())),
            backup_done: Arc::new(std::sync::Condvar::new()),
            counter: AtomicU64::new(1),
        }
    }

    /// Tests: replaces the tool runner for system tools.
    pub fn use_tools(&self, tools: Arc<dyn ToolRunner>) {
        *lock(&self.tools) = tools;
        *lock(&self.software) = None;
    }

    /// Tests: replaces the runner every backup tool goes through.
    pub fn use_backup_tools(&self, tools: Arc<dyn ToolRunner>) {
        *lock(&self.backup_tools) = Some(tools);
    }

    /// The backup report as it stands, without starting a check (tests).
    pub fn backup_report(&self) -> dyno_core::backup::BackupReport {
        backup_check::peek(self)
    }

    /// Tests: pretend to be another OS (`macos`, `windows`, `linux`).
    pub fn use_os(&self, os: &'static str) {
        *lock(&self.os) = os;
    }

    pub(crate) fn tools(&self) -> Arc<dyn ToolRunner> {
        lock(&self.tools).clone()
    }

    pub(crate) fn os(&self) -> &'static str {
        *lock(&self.os)
    }

    /// An id for one finder result.
    pub fn next_scan_id(&self, t: Timestamp) -> String {
        format!("f{}-{}", t.millis(), self.counter.fetch_add(1, Ordering::Relaxed))
    }
}

/// Answers an extras request.
pub fn handle(shared: &Arc<Shared>, request: Request) -> Result<Reply, AgentError> {
    match request {
        Request::Changes { query } => changes(shared, query).map(|t| Reply::Changes(Box::new(t))),
        Request::GpuProcesses => Ok(Reply::Gpu(Box::new(gpu(shared)))),
        Request::SaveInventoryReport { path, format } => save_inventory(shared, &path, format).map(Reply::InventorySaved),
        Request::Maintenance => {
            let core = lock(&shared.core);
            Ok(Reply::Maintenance(core.settings.maintenance.status(now(), core.zone.as_ref())))
        }
        Request::Backup => Ok(Reply::Backup(Box::new(backup_check::report(shared)))),
        other => Err(bad(format!("{other:?} is not an extras request."))),
    }
}

/// The software list, from the cache when fresh.
pub fn software(shared: &Shared) -> SoftwareInventory {
    let x = &shared.extras;
    if let Some((at, s)) = lock(&x.software).as_ref() {
        if at.elapsed() < SOFTWARE_TTL {
            return s.clone();
        }
    }
    let list = dyno_platform::extras::software_list(x.tools().as_ref(), x.os());
    if list.unavailable.is_none() {
        *lock(&x.software) = Some((Instant::now(), list.clone()));
    }
    list
}

fn changes(shared: &Shared, query: ChangesQuery) -> Result<dyno_core::extras::changes::ChangeTimeline, AgentError> {
    let t = now();
    let (from, to) = query.window(t);
    let core = lock(&shared.core);
    let all = core.store.changes(from, to).map_err(internal)?;
    let since = core
        .store
        .meta(KEY_SINCE)
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i64>().ok())
        .map(Timestamp);
    Ok(timeline(&query, t, since, all))
}

/// Compares the software and startup lists with the last ones and stores what changed. The first call stores the lists
/// as the baseline (a baseline is not a change). Slow (it reads the software list): background only.
pub fn record_changes(shared: &Shared) {
    let t = now();
    let list = software(shared);
    let startup: Option<Vec<(String, String)>> = lock(&shared.ports.startup)
        .list()
        .ok()
        .map(|v| v.into_iter().map(|e| (e.id, e.name)).collect());
    let core = lock(&shared.core);
    let store = &core.store;
    let mut found: Vec<Change> = Vec::new();
    let mut baseline = false;
    if list.unavailable.is_none() {
        match store
            .meta(KEY_SOFTWARE)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<Vec<SoftwareItem>>(&s).ok())
        {
            Some(prev) => found.extend(diff_software(&prev, &list.items, t)),
            None => baseline = true,
        }
        if let Ok(json) = serde_json::to_string(&list.items) {
            let _ = store.set_meta(KEY_SOFTWARE, &json);
        }
    }
    if let Some(now_list) = startup {
        match store
            .meta(KEY_STARTUP)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<Vec<(String, String)>>(&s).ok())
        {
            Some(prev) => found.extend(diff_startup(&prev, &now_list, t)),
            None => baseline = true,
        }
        if let Ok(json) = serde_json::to_string(&now_list) {
            let _ = store.set_meta(KEY_STARTUP, &json);
        }
    }
    if baseline && store.meta(KEY_SINCE).ok().flatten().is_none() {
        let _ = store.set_meta(KEY_SINCE, &t.millis().to_string());
    }
    if !found.is_empty() {
        if let Err(e) = store.put_changes(&found, t) {
            eprintln!("dyno-agent: could not store changes: {e}");
        }
    }
}

/// Boot history and what slowed the last start (runs OS tools).
pub fn boot_report(shared: &Shared) -> Result<BootReport, AgentError> {
    let x = &shared.extras;
    let facts = dyno_platform::extras::boot_facts(x.tools().as_ref(), x.os());
    let t = now();
    let mut rows: Vec<BootRow> = lock(&shared.core)
        .store
        .boots(t.minus(90 * DAY), t.plus(HOUR))
        .map_err(internal)?
        .into_iter()
        .map(|b| BootRow {
            at: b.at,
            boot_secs: b.boot_secs,
            ready_secs: b.ready_secs,
        })
        .collect();
    rows.sort_by_key(|b| std::cmp::Reverse(b.at));
    let verdict = boot::verdict(&rows);
    let last_total = facts.total_secs.or_else(|| rows.first().and_then(|b| b.ready_secs.or(b.boot_secs)));
    Ok(BootReport {
        boots: rows,
        slowest: facts.steps,
        last_total_secs: last_total,
        verdict,
        steps_unavailable: facts.unavailable,
    })
}

/// Network and energy use per app (runs OS tools).
pub fn app_usage(shared: &Shared) -> UsageReport {
    let x = &shared.extras;
    let apps: Vec<(String, Vec<u32>)> = lock(&shared.core)
        .latest
        .groups
        .iter()
        .map(|g| (g.name.clone(), g.pids.clone()))
        .collect();
    let (counters, network_unavailable) = match shared.network.counters() {
        Ok(c) => (Some(c), None),
        Err(e) => (None, Some(e.to_string())),
    };
    let (energy, energy_unavailable) = match dyno_platform::extras::energy(x.tools().as_ref(), x.os()) {
        Ok(e) => (Some(e), None),
        Err(why) => (None, Some(why)),
    };
    UsageReport {
        apps: usage::build(&apps, counters.as_deref(), energy.as_deref()),
        network_unavailable,
        energy_unavailable,
    }
}

fn gpu(shared: &Shared) -> GpuReport {
    let x = &shared.extras;
    let mut report = dyno_platform::extras::gpu_report(x.tools().as_ref(), x.os(), Duration::from_secs(1));
    let core = lock(&shared.core);
    for p in report.processes.iter_mut().filter(|p| p.name.is_empty()) {
        if let Some(s) = core.latest.processes.iter().find(|s| s.pid == p.pid) {
            p.name = s.name.clone();
        }
    }
    report
}

fn save_inventory(shared: &Shared, path: &str, format: InventoryFormat) -> Result<SavedInventory, AgentError> {
    let dest = validate_destination(path, format).map_err(bad)?;
    if !dest.parent().is_some_and(std::path::Path::is_dir) {
        return Err(bad("The folder to save the report in does not exist."));
    }
    let text = render_report(format, now(), &crate::inventory::get(shared), &software(shared));
    std::fs::write(&dest, text.as_bytes()).map_err(|e| {
        let code = if e.kind() == std::io::ErrorKind::PermissionDenied {
            ErrorCode::AccessDenied
        } else {
            ErrorCode::Internal
        };
        AgentError::new(code, format!("Dyno could not write the report ({e}). Choose another folder."))
    })?;
    Ok(SavedInventory {
        path: dest.to_string_lossy().into_owned(),
        bytes: text.len() as u64,
    })
}

/// Records this boot (once) and finds when the desktop settled after login, so Boot analysis has a history. Runs once at
/// agent start; a restart of the agent in the middle of a session records nothing.
fn record_boot(shared: &Shared) {
    let boot_ms = i64::try_from(sysinfo::System::boot_time()).unwrap_or(0).saturating_mul(SECOND);
    let started = now();
    if boot_ms <= 0 || started.millis() - boot_ms > BOOT_WINDOW_MS || started.millis() < boot_ms {
        return;
    }
    let at = Timestamp(boot_ms);
    {
        let core = lock(&shared.core);
        // The OS clock jitters by a second between reads on some systems: the same boot is one row.
        let recent = core
            .store
            .boots(at.minus(5 * 60 * SECOND), at.plus(5 * 60 * SECOND))
            .unwrap_or_default();
        if !recent.is_empty() {
            return;
        }
        let record = BootRecord {
            at,
            boot_secs: Some(((started.millis() - boot_ms) as f32) / 1000.0),
            ready_secs: None,
        };
        if core.store.put_boot(&record).is_err() {
            return;
        }
    }
    let mut tracker = SettleTracker::new(started, SETTLE_CPU, SETTLE_HOLD_MS);
    let deadline = Instant::now() + SETTLE_WITHIN;
    while Instant::now() < deadline && !shared.stopped() {
        let cpu = lock(&shared.core).recent.back().map(|s| (s.at, s.cpu.total));
        if let Some((sample_at, total)) = cpu {
            if let Some(secs) = tracker.observe(sample_at, total) {
                let secs = secs + ((started.millis() - boot_ms) as f32) / 1000.0;
                let _ = lock(&shared.core).store.set_boot_ready(at, secs);
                return;
            }
        }
        wait(shared, Duration::from_secs(5));
    }
}

fn wait(shared: &Shared, d: Duration) {
    let mut waited = Duration::ZERO;
    while waited < d && !shared.stopped() {
        std::thread::sleep(Duration::from_millis(250));
        waited += Duration::from_millis(250);
    }
}

/// The background thread: records the boot, then every [`BACKGROUND_EVERY`] (inside the maintenance windows) checks the
/// backups and compares the software lists. Does nothing on a data folder of its own (tests, `DYNO_DATA_DIR`).
pub fn run(shared: &Arc<Shared>) {
    if !shared.extras.own_folder {
        return;
    }
    // Watching for the desktop to settle can take minutes; it must not delay the checks below.
    let s = shared.clone();
    let _ = std::thread::Builder::new().name("dyno-boot".into()).spawn(move || {
        dyno_platform::priority::lower_current_thread();
        record_boot(&s)
    });
    wait(shared, FIRST_RUN_AFTER);
    while !shared.stopped() {
        if maintenance_open(shared) {
            if lock(&shared.core).settings.backup.alert_when_stale {
                backup_check::check_now(shared);
            }
            record_changes(shared);
            wait(shared, BACKGROUND_EVERY);
        } else {
            // Closed: look again in a few minutes.
            wait(shared, Duration::from_secs(5 * 60));
        }
    }
}
