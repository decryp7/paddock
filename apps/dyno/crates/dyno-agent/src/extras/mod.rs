//! The extras (WP-13) and backup health (WP-14) on the agent side: runs the read-only OS tools behind a [`ToolRunner`],
//! keeps the few results worth caching, records the changes the "What changed" timeline needs, and answers the requests.
//! Everything is observe-only: the one thing that moves files is removing the finder results the user selected, and that
//! goes through the cleanup policy's admission and the Trash or the quarantine ([`finder`]).

pub mod backup_check;
pub mod finder;

use crate::agent::{lock, now, Shared};
use dyno_core::activity::{ActivityKind, UndoRecord};
use dyno_core::cleanup::{CleanupOutcome, DeleteMode};
use dyno_core::extras::boot::{self, BootReport, BootRow, SettleTracker};
use dyno_core::extras::changes::{diff_software, diff_startup, from_activity, timeline, Change, ChangesQuery};
use dyno_core::extras::files::{FinderQuery, FinderResult};
use dyno_core::extras::gpu::GpuReport;
use dyno_core::extras::software::{render_report, validate_destination, InventoryFormat, SoftwareInventory, SoftwareItem};
use dyno_core::extras::usage::{self, UsageReport};
use dyno_core::protocol::{AgentError, BootRecord, ErrorCode, Reply, Request, SavedInventory};
use dyno_core::time::{Timestamp, DAY, HOUR, SECOND};
use dyno_platform::issues::{OsRunner, ToolRunner};
use dyno_platform::toolpath::PathResolver;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a software list is reused (reading it takes seconds to minutes).
const SOFTWARE_TTL: Duration = Duration::from_secs(10 * 60);
/// Finder results kept for a removal.
const KEEP_RESULTS: usize = 4;
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
    core.settings.maintenance.is_open(now(), core.settings.alarms.utc_offset_minutes)
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
    pub(crate) backup: Mutex<backup_check::Slot>,
    kept: Mutex<VecDeque<finder::Kept>>,
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
            backup: Mutex::new(backup_check::Slot::default()),
            kept: Mutex::new(VecDeque::new()),
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

    /// Tests: pretend to be another OS (`macos`, `windows`, `linux`).
    pub fn use_os(&self, os: &'static str) {
        *lock(&self.os) = os;
    }

    fn tools(&self) -> Arc<dyn ToolRunner> {
        lock(&self.tools).clone()
    }

    fn os(&self) -> &'static str {
        *lock(&self.os)
    }

    fn next_id(&self, t: Timestamp) -> String {
        format!("f{}-{}", t.millis(), self.counter.fetch_add(1, Ordering::Relaxed))
    }
}

/// Answers an extras request.
pub fn handle(shared: &Arc<Shared>, request: Request) -> Result<Reply, AgentError> {
    let x = &shared.extras;
    match request {
        Request::Changes { query } => changes(shared, query).map(|t| Reply::Changes(Box::new(t))),
        Request::SleepBlockers => Ok(Reply::Sleep(Box::new(dyno_platform::extras::sleep_report(
            x.tools().as_ref(),
            x.os(),
        )))),
        Request::BootAnalysis => boot_report(shared).map(|b| Reply::Boot(Box::new(b))),
        Request::AppUsage => Ok(Reply::AppUsage(Box::new(app_usage(shared)))),
        Request::GpuProcesses => Ok(Reply::Gpu(Box::new(gpu(shared)))),
        Request::SoftwareInventory => Ok(Reply::Software(Box::new(software(shared)))),
        Request::SaveInventoryReport { path, format } => save_inventory(shared, &path, format).map(Reply::InventorySaved),
        Request::FindFiles { query } => find_files(shared, &query).map(|r| Reply::FileFinder(Box::new(r))),
        Request::RemoveFoundFiles { scan_id, ids, mode } => remove_found(shared, &scan_id, &ids, mode).map(Reply::Outcome),
        Request::Maintenance => {
            let core = lock(&shared.core);
            let off = core.settings.alarms.utc_offset_minutes;
            Ok(Reply::Maintenance(core.settings.maintenance.status(now(), off)))
        }
        Request::Backup => Ok(Reply::Backup(Box::new(backup_check::report(shared, false)))),
        Request::RefreshBackup => Ok(Reply::Backup(Box::new(backup_check::report(shared, true)))),
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
    let mut all = core.store.changes(from, to).map_err(internal)?;
    let activity = core.store.activity(500).map_err(internal)?;
    let entries: Vec<_> = activity.into_iter().map(|(e, _, _)| e).collect();
    all.extend(from_activity(&entries));
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

fn boot_report(shared: &Shared) -> Result<BootReport, AgentError> {
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

fn app_usage(shared: &Shared) -> UsageReport {
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

fn find_files(shared: &Shared, query: &FinderQuery) -> Result<FinderResult, AgentError> {
    let x = &shared.extras;
    let t = now();
    let id = x.next_id(t);
    let (result, root) = finder::run(query, &id).map_err(bad)?;
    let mut kept = lock(&x.kept);
    kept.push_back(finder::Kept {
        result: result.clone(),
        root,
        at: t,
    });
    while kept.len() > KEEP_RESULTS {
        kept.pop_front();
    }
    Ok(result)
}

fn remove_found(shared: &Arc<Shared>, scan_id: &str, ids: &[String], mode: DeleteMode) -> Result<CleanupOutcome, AgentError> {
    if scan_id.len() > 128 {
        return Err(bad("That request is too large."));
    }
    let x = &shared.extras;
    let t = now();
    let home = home_dir();
    let admitted = {
        let kept = lock(&x.kept);
        let k = kept
            .iter()
            .find(|k| k.result.scan_id == scan_id)
            .ok_or_else(|| bad("That result is no longer available; look again."))?;
        let mut locks = lock(&shared.ports.locks);
        let mut in_use = |p: &std::path::Path| -> Option<String> {
            let mut names: Vec<String> = locks.lockers(p).ok()?.into_iter().map(|l| l.name).collect();
            names.sort();
            names.dedup();
            (!names.is_empty()).then(|| names.join(", "))
        };
        finder::admit_selection(k, ids, &home, t, &mut in_use).map_err(bad)?
    };
    let mut outcome = shared.cleanup.dispose(&admitted.ready, mode, t);
    outcome.failed.extend(admitted.failed);
    if outcome.deleted > 0 {
        let (where_to, undo) = match &outcome.quarantine_id {
            Some(q) => (
                "to the 7-day quarantine",
                UndoRecord::Quarantine {
                    id: q.clone(),
                    expires: dyno_core::activity::quarantine_expiry(t),
                },
            ),
            None => ("to the Recycle Bin / Trash", UndoRecord::None),
        };
        let title = format!("Removed {} file{}", outcome.deleted, if outcome.deleted == 1 { "" } else { "s" });
        let entry = lock(&shared.core).log(
            t,
            ActivityKind::Cleanup,
            title,
            format!("Chosen in the file finder, moved {where_to}"),
            undo,
            Some(outcome.freed_bytes),
        );
        if let Some(q) = &outcome.quarantine_id {
            shared.cleanup.link_activity(q, &entry.id);
        }
    }
    Ok(outcome)
}

fn home_dir() -> std::path::PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
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
    let _ = std::thread::Builder::new().name("dyno-boot".into()).spawn(move || record_boot(&s));
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
