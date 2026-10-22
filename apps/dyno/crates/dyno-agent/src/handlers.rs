//! Answers the app's requests (command–query separation: queries only read; open, act, undo, save and the like
//! mutate). Every argument is validated here even though the app validated it too. Requests that belong to features
//! not built yet answer `Unsupported` with a message saying so.

use crate::agent::{lock, now, Shared, FACTS_TTL};
use dyno_core::activity::{inverse, undo_state, ActivityEntry, ActivityKind, Previous, UndoRecord, UndoState};
use dyno_core::anomaly;
use dyno_core::diagnosis;
use dyno_core::facts::SystemFacts;
use dyno_core::forecast::{disk_full, DiskTrend};
use dyno_core::health::{assess, HealthInput};
use dyno_core::history::{downsample, top_consumers, window, Tier};
use dyno_core::metrics::SystemSample;
use dyno_core::ports::PlatformError;
use dyno_core::process::{Pid, ProcessAction, ProcessRef, ProcessSample};
use dyno_core::protect::Protection;
use dyno_core::protocol::{AgentError, ErrorCode, History, LiveDetail, Message, PluginInfo, PluginState, Reply, Request};
use dyno_core::rules::{Command as RuleCommand, Situation};
use dyno_core::snapshot;
use dyno_core::startup::StartupState;
use dyno_core::time::{Timestamp, DAY, HOUR};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

type Answer = Result<Reply, AgentError>;

fn bad(msg: impl Into<String>) -> AgentError {
    AgentError::new(ErrorCode::BadRequest, msg)
}

fn internal(msg: impl std::fmt::Display) -> AgentError {
    AgentError::new(ErrorCode::Internal, msg.to_string())
}

fn not_built(what: &str) -> AgentError {
    AgentError::new(ErrorCode::Unsupported, format!("{what} is not built yet."))
}

/// What a connection can change about itself.
pub struct Conn {
    /// Its subscription level (see [`crate::agent::Sub`]).
    pub detail: Arc<AtomicU8>,
}

/// Handles one request. Never panics: a panic becomes an internal error for that request only.
pub fn handle(shared: &Arc<Shared>, conn: &Conn, request: Request) -> Answer {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(shared, conn, request))) {
        Ok(a) => a,
        Err(_) => Err(internal("Dyno hit an internal error answering that; the agent keeps running.")),
    }
}

fn dispatch(shared: &Arc<Shared>, conn: &Conn, request: Request) -> Answer {
    match request {
        Request::Subscribe { detail } => subscribe(shared, conn, detail),
        Request::ProcessDetails { target } => {
            let d = lock(&shared.ports.details).details(target)?;
            Ok(Reply::Details(Box::new(d)))
        }
        Request::Act { target, action } => act(shared, target, action).map(Reply::Activity),
        Request::ActOnGroup { group_id, action } => act_on_group(shared, &group_id, action).map(Reply::Activity),
        Request::Lockers { path } => {
            let p = Path::new(&path);
            if !p.is_absolute() {
                return Err(bad("Give the full path of the file."));
            }
            Ok(Reply::Lockers(lock(&shared.ports.locks).lockers(p)?))
        }
        Request::OpenLocation { target } => {
            let exe = lock(&shared.core)
                .latest
                .processes
                .iter()
                .find(|p| p.reference() == target)
                .and_then(|p| p.exe.clone())
                .ok_or_else(|| AgentError::new(ErrorCode::NotFound, "Dyno does not know where that program is."))?;
            (shared.reveal)(Path::new(&exe))?;
            Ok(Reply::Ok)
        }
        Request::TakeSnapshot { label } => {
            let mut core = lock(&shared.core);
            let t = now();
            let (mem, cpu) = core.recent.back().map(|s| (s.memory.used, s.cpu.total)).unwrap_or((0, 0.0));
            let id = core.next_id(t);
            let snap = snapshot::take(id, t, &label, mem, cpu, core.latest.processes.clone());
            core.store.put_snapshot(&snap).map_err(internal)?;
            Ok(Reply::Snapshot(snap.info))
        }
        Request::Snapshots => Ok(Reply::Snapshots(lock(&shared.core).store.snapshots().map_err(internal)?)),
        Request::DiffSnapshots { before, after } => {
            let core = lock(&shared.core);
            let get = |id: &str| {
                core.store
                    .snapshot(id)
                    .map_err(internal)?
                    .ok_or_else(|| AgentError::new(ErrorCode::NotFound, "That snapshot no longer exists."))
            };
            Ok(Reply::Diff(Box::new(snapshot::diff(&get(&before)?, &get(&after)?))))
        }
        Request::WhySlow => Ok(Reply::Diagnosis(why_slow(shared))),
        Request::Anomalies => Ok(Reply::Anomalies(anomalies(shared))),
        Request::History { range, end } => history(shared, range, end).map(|h| Reply::History(Box::new(h))),
        Request::Activity { limit } => Ok(Reply::Activities(activity(shared, limit)?)),
        Request::Undo { activity_id } => undo(shared, &activity_id).map(Reply::Activity),
        Request::ReleaseAction { target } => release_action(shared, target).map(Reply::Activity),
        Request::Rules => Ok(Reply::Rules(Box::new(lock(&shared.core).rules.clone().for_display()))),
        Request::SaveRules { rules } => {
            let valid = rules.validate().map_err(bad)?;
            let mut core = lock(&shared.core);
            core.rules = valid;
            core.save_rules(&shared.dirs);
            Ok(Reply::Rules(Box::new(core.rules.clone().for_display())))
        }
        Request::SetProfile { profile } => change_rules(shared, |r| r.profile = profile),
        Request::PauseRules { paused } => change_rules(shared, |r| r.paused = paused),
        Request::Alerts { limit } => Ok(Reply::Alerts(
            lock(&shared.core).store.alerts(limit.clamp(1, 500)).map_err(internal)?,
        )),
        Request::Settings => Ok(Reply::Settings(Box::new(lock(&shared.core).settings.clone()))),
        Request::SaveSettings { settings } => {
            let mut core = lock(&shared.core);
            core.settings = settings.normalised();
            core.save_settings(&shared.dirs);
            let s = core.settings.clone();
            drop(core);
            shared.wake_sampler();
            Ok(Reply::Settings(Box::new(s)))
        }
        Request::Plugins => Ok(Reply::Plugins(plugins(shared))),
        Request::Health => Ok(Reply::Health(Box::new(health(shared)?))),
        Request::StartupEntries => Ok(Reply::Startup(lock(&shared.ports.startup).list()?)),
        Request::SetStartupState { id, state } => set_startup(shared, &id, state).map(Reply::Activity),
        Request::Shutdown => {
            shared.request_stop();
            Ok(Reply::Ok)
        }
        Request::CleanupCategories
        | Request::ScanCleanup { .. }
        | Request::RunCleanup { .. }
        | Request::Quarantine
        | Request::RestoreQuarantine { .. } => Err(not_built("Cleanup")),
        Request::DiskUsage { .. } => Err(not_built("The disk space map")),
        Request::Tuning | Request::ApplyTuning { .. } | Request::RevertTuning { .. } => Err(not_built("Tuning")),
        Request::WeeklyReport => Err(not_built("The weekly report")),
    }
}

fn subscribe(shared: &Arc<Shared>, conn: &Conn, detail: LiveDetail) -> Answer {
    conn.detail
        .store(if detail == LiveDetail::Processes { 2 } else { 1 }, Ordering::Relaxed);
    shared.wake_sampler();
    // Show something at once instead of waiting for the next tick.
    let live = lock(&shared.core).latest.live.clone();
    if let Some(l) = live {
        let l = crate::sampler::for_detail(l, detail);
        for s in lock(&shared.subs).iter().filter(|s| Arc::ptr_eq(&s.detail, &conn.detail)) {
            let _ = s.tx.try_send(Message::Live(Box::new(l.clone())));
        }
    }
    Ok(Reply::Ok)
}

fn change_rules(shared: &Arc<Shared>, f: impl FnOnce(&mut dyno_core::rules::RuleSet)) -> Answer {
    let mut core = lock(&shared.core);
    f(&mut core.rules);
    core.save_rules(&shared.dirs);
    Ok(Reply::Rules(Box::new(core.rules.clone().for_display())))
}

/// Why a process may not be acted on, if it may not.
fn protection_reason(shared: &Shared, p: &ProcessSample) -> Option<String> {
    let core = lock(&shared.core);
    let own = [std::process::id()];
    Protection::new(&core.rules.protected, &own).reason(p)
}

fn find_process(shared: &Shared, target: ProcessRef) -> Result<ProcessSample, AgentError> {
    lock(&shared.core)
        .latest
        .processes
        .iter()
        .find(|p| p.reference() == target)
        .cloned()
        .ok_or_else(|| AgentError::new(ErrorCode::NotFound, "That process has exited or is not in the list any more."))
}

/// The pieces of a process's state an action may change, so the undo can put them back.
fn previous_of(shared: &Shared, target: ProcessRef) -> Previous {
    match lock(&shared.ports.details).details(target) {
        Ok(d) => Previous {
            priority: d.priority,
            affinity: d.affinity,
            efficiency: d.efficiency_mode,
        },
        Err(_) => Previous::default(),
    }
}

fn act(shared: &Arc<Shared>, target: ProcessRef, action: ProcessAction) -> Result<ActivityEntry, AgentError> {
    action.validate(shared.cores).map_err(bad)?;
    let p = find_process(shared, target)?;
    if let Some(reason) = protection_reason(shared, &p) {
        return Err(AgentError::new(ErrorCode::Protected, format!("{} is protected: {reason}", p.name)));
    }
    let previous = previous_of(shared, target);
    lock(&shared.ports.control).apply(target, &action)?;
    let actions: Vec<ProcessAction> = inverse(&action, previous).into_iter().collect();
    let t = now();
    let entry = lock(&shared.core).log(
        t,
        ActivityKind::Manual,
        format!("{} {}", action.describe(), p.name),
        format!("You did this from the task manager (process {}).", p.pid),
        UndoRecord::Process {
            target,
            actions,
            rule: None,
        },
        None,
    );
    shared.broadcast(&Message::Activity(entry.clone()));
    Ok(entry)
}

fn act_on_group(shared: &Arc<Shared>, group_id: &str, action: ProcessAction) -> Result<ActivityEntry, AgentError> {
    action.validate(shared.cores).map_err(bad)?;
    let (group, members) = {
        let core = lock(&shared.core);
        let g = core
            .latest
            .groups
            .iter()
            .find(|g| g.id == group_id)
            .cloned()
            .ok_or_else(|| AgentError::new(ErrorCode::NotFound, "That app is no longer running."))?;
        let m: Vec<ProcessSample> = core.latest.processes.iter().filter(|p| g.pids.contains(&p.pid)).cloned().collect();
        (g, m)
    };
    let (mut done, mut skipped, mut last_error) = (0usize, 0usize, None);
    let mut undo_all: Vec<(ProcessRef, Vec<ProcessAction>)> = Vec::new();
    for p in &members {
        if protection_reason(shared, p).is_some() {
            skipped += 1;
            continue;
        }
        let previous = previous_of(shared, p.reference());
        match lock(&shared.ports.control).apply(p.reference(), &action) {
            Ok(()) => {
                done += 1;
                undo_all.push((p.reference(), inverse(&action, previous).into_iter().collect()));
            }
            Err(PlatformError::NotFound(_)) => {}
            Err(e) => {
                skipped += 1;
                last_error = Some(e);
            }
        }
    }
    if done == 0 {
        return Err(match last_error {
            Some(e) => e.into(),
            None => AgentError::new(ErrorCode::Protected, format!("{} has no process Dyno may change.", group.name)),
        });
    }
    // One entry per group; its undo covers the first process (the root); the rest are undone with it by the UI's
    // per-process Undo. Keeping the record small and honest beats a half-working bulk undo.
    let (target, actions) = undo_all.first().cloned().unwrap_or((members[0].reference(), Vec::new()));
    let t = now();
    let detail = if skipped > 0 {
        format!("{done} process(es) changed, {skipped} left alone (protected or not allowed).")
    } else {
        format!("{done} process(es) changed.")
    };
    let entry = lock(&shared.core).log(
        t,
        ActivityKind::Manual,
        format!("{} {}", action.describe(), group.name),
        detail,
        UndoRecord::Process {
            target,
            actions,
            rule: None,
        },
        None,
    );
    shared.broadcast(&Message::Activity(entry.clone()));
    Ok(entry)
}

fn activity(shared: &Shared, limit: u32) -> Result<Vec<ActivityEntry>, AgentError> {
    let rows = lock(&shared.core).store.activity(limit.clamp(1, 500)).map_err(internal)?;
    let t = now();
    let alive = |r: ProcessRef| lock(&shared.core).latest.processes.iter().any(|p| p.reference() == r);
    Ok(rows
        .into_iter()
        .map(|(mut e, record, undone)| {
            e.undo = undo_state(&record, undone, t, alive);
            e
        })
        .collect())
}

fn undo(shared: &Arc<Shared>, id: &str) -> Result<ActivityEntry, AgentError> {
    let (entry, record, undone) = lock(&shared.core)
        .store
        .activity_entry(id)
        .map_err(internal)?
        .ok_or_else(|| AgentError::new(ErrorCode::NotFound, "That entry is no longer in the log."))?;
    let t = now();
    let alive = |r: ProcessRef| lock(&shared.core).latest.processes.iter().any(|p| p.reference() == r);
    match undo_state(&record, undone, t, alive) {
        UndoState::Available => {}
        UndoState::Undone => return Err(bad("That was already undone.")),
        UndoState::Expired => return Err(bad("That can no longer be undone: the process has exited.")),
        UndoState::NotUndoable => return Err(bad("That cannot be undone.")),
    }
    match &record {
        UndoRecord::Process { target, actions, rule } => {
            for a in actions {
                lock(&shared.ports.control).apply(*target, a)?;
            }
            if let Some((rule_id, app)) = rule {
                let mut core = lock(&shared.core);
                let procs = core.latest.processes.clone();
                let rules = core.rules.clone();
                // The user said no to this rule for this app until it calms down.
                let _ = core.engine.snooze(&rules, rule_id, app, &procs);
            }
        }
        UndoRecord::Startup { entry_id, previous } => {
            let mut startup = lock(&shared.ports.startup);
            let list = startup.list()?;
            let e = list
                .iter()
                .find(|e| &e.id == entry_id)
                .ok_or_else(|| AgentError::new(ErrorCode::NotFound, "That startup item no longer exists."))?;
            startup.set_state(e, *previous)?;
        }
        UndoRecord::None | UndoRecord::Tuning { .. } | UndoRecord::Quarantine { .. } => return Err(bad("That cannot be undone here yet.")),
    }
    let core = lock(&shared.core);
    core.store.mark_undone(id).map_err(internal)?;
    let mut e = entry;
    e.undo = UndoState::Undone;
    Ok(e)
}

fn release_action(shared: &Arc<Shared>, target: ProcessRef) -> Result<ActivityEntry, AgentError> {
    let (rule_id, app, name) = {
        let core = lock(&shared.core);
        let active = core.engine.active(&core.rules);
        let a = active
            .into_iter()
            .find(|a| a.target == target)
            .ok_or_else(|| AgentError::new(ErrorCode::NotFound, "No rule is acting on that process."))?;
        let group_id = core
            .latest
            .groups
            .iter()
            .find(|g| g.pids.contains(&target.pid))
            .map(|g| g.id.clone())
            .unwrap_or_else(|| a.app.clone());
        (a.rule_id, group_id, a.app)
    };
    let commands = {
        let mut core = lock(&shared.core);
        let rules = core.rules.clone();
        let procs = core.latest.processes.clone();
        core.engine.snooze(&rules, &rule_id, &app, &procs)
    };
    crate::sampler::apply_commands(shared, commands, now());
    let t = now();
    let entry = lock(&shared.core).log(
        t,
        ActivityKind::Release,
        format!("Released {name}"),
        "You released what the rule was doing; the rule leaves this app alone until it calms down.".into(),
        UndoRecord::None,
        None,
    );
    shared.broadcast(&Message::Activity(entry.clone()));
    Ok(entry)
}

fn set_startup(shared: &Arc<Shared>, id: &str, state: StartupState) -> Result<ActivityEntry, AgentError> {
    let mut startup = lock(&shared.ports.startup);
    let list = startup.list()?;
    let e = list
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| AgentError::new(ErrorCode::NotFound, "That startup item no longer exists."))?
        .clone();
    startup.set_state(&e, state)?;
    drop(startup);
    let verb = match state {
        StartupState::Enabled => "Turned on",
        StartupState::Disabled => "Turned off",
        StartupState::Delayed(_) => "Delayed",
    };
    Ok(lock(&shared.core).log(
        now(),
        ActivityKind::Startup,
        format!("{verb} {} at login", e.name),
        "You changed this in the startup manager.".into(),
        UndoRecord::Startup {
            entry_id: e.id,
            previous: e.state,
        },
        None,
    ))
}

fn history(shared: &Shared, range: dyno_core::history::HistoryRange, end: Option<Timestamp>) -> Result<History, AgentError> {
    let w = window(range, end.unwrap_or_else(now));
    let core = lock(&shared.core);
    let s = &core.store;
    let raw = s.points(w.tier, w.from, w.to).map_err(internal)?;
    let points = if w.resolution_ms > w.tier.step_ms() {
        downsample(&raw, w.resolution_ms)
    } else {
        raw
    };
    let rows = s.app_minutes(w.from, w.to).map_err(internal)?;
    let disk_trends = s
        .disk_free(w.from.minus(14 * DAY).min(w.from), w.to)
        .map_err(internal)?
        .into_iter()
        .map(|(disk, total, points)| DiskTrend {
            forecast: disk_full(&points),
            disk,
            total,
            points,
        })
        .collect();
    Ok(History {
        range,
        from: w.from,
        to: w.to,
        resolution_ms: w.resolution_ms,
        points,
        events: s.events(w.from, w.to).map_err(internal)?,
        top_consumers: top_consumers(&rows, 8),
        disk_trends,
        boots: s.boots(w.from.minus(30 * DAY), w.to).map_err(internal)?,
        crashes: Vec::new(),
    })
}

fn why_slow(shared: &Shared) -> dyno_core::diagnosis::Diagnosis {
    let core = lock(&shared.core);
    let samples: Vec<SystemSample> = core.recent.iter().rev().take(60).rev().cloned().collect();
    let own = [std::process::id()];
    let protection = Protection::new(&core.rules.protected, &own);
    let disk = samples
        .last()
        .and_then(|s| s.disks.first())
        .map(|d| d.id.clone())
        .unwrap_or_default();
    diagnosis::diagnose(&diagnosis::Input {
        now: now(),
        samples: &samples,
        groups: &core.latest.groups,
        processes: &core.latest.processes,
        protection: &protection,
        system_disk: &disk,
    })
}

fn anomalies(shared: &Shared) -> Vec<dyno_core::anomaly::Anomaly> {
    let core = lock(&shared.core);
    let t = now();
    let rows = core.store.app_minutes(t.minus(6 * HOUR), t).unwrap_or_default();
    let points = core.store.points(Tier::Minute, t.minus(6 * HOUR), t).unwrap_or_default();
    let mut out = anomaly::memory_growth(&rows);
    out.extend(anomaly::periodic_spikes(&points, &rows));
    out
}

fn facts(shared: &Shared) -> SystemFacts {
    {
        let core = lock(&shared.core);
        if let Some((at, f)) = &core.facts {
            if at.elapsed() < FACTS_TTL {
                return f.clone();
            }
        }
    }
    let f = lock(&shared.ports.facts).facts();
    lock(&shared.core).facts = Some((std::time::Instant::now(), f.clone()));
    f
}

fn health(shared: &Shared) -> Result<dyno_core::health::HealthReport, AgentError> {
    let facts = facts(shared);
    let startup = lock(&shared.ports.startup).list().unwrap_or_default();
    let anomalies = anomalies(shared);
    let core = lock(&shared.core);
    let sample = core.recent.back().cloned().ok_or_else(|| {
        AgentError::new(
            ErrorCode::Busy,
            "Dyno has not taken its first measurement yet; try again in a moment.",
        )
    })?;
    let t = now();
    let trends: Vec<(String, dyno_core::forecast::Forecast)> = core
        .store
        .disk_free(t.minus(14 * DAY), t)
        .unwrap_or_default()
        .into_iter()
        .map(|(disk, _, pts)| (disk, disk_full(&pts)))
        .collect();
    let system_disk = sample.disks.first().map(|d| d.id.clone()).unwrap_or_default();
    Ok(assess(&HealthInput {
        now: t,
        sample: &sample,
        facts: &facts,
        system_disk: &system_disk,
        forecasts: &trends,
        startup: &startup,
        anomalies: &anomalies,
        crashes_7d: 0,
        tuning: &[],
    }))
}

fn plugins(shared: &Shared) -> Vec<PluginInfo> {
    // Started on first use: most users have none, and discovery is a directory scan.
    let _ = shared.plugins.start_all();
    shared
        .plugins
        .list()
        .into_iter()
        .map(|p| PluginInfo {
            id: p.id,
            name: p.name,
            version: p.version,
            state: match p.state {
                dyno_plugin_host::PluginState::Running => PluginState::Running,
                dyno_plugin_host::PluginState::Stopped => PluginState::Stopped,
                dyno_plugin_host::PluginState::Failed => PluginState::Failed,
                dyno_plugin_host::PluginState::Quarantined => PluginState::Quarantined,
            },
            error: p.error,
            contributes: p.contributes.iter().map(|c| c.as_str().to_string()).collect(),
        })
        .collect()
}

/// The situation rules reason about (None fields when the OS does not tell).
pub fn situation(shared: &Shared, power: Option<dyno_core::metrics::PowerSource>) -> Situation {
    let mut a = lock(&shared.ports.activity);
    Situation {
        user_active: a.idle_secs().map(|s| s < 60),
        power,
        foreground: a.foreground_pid(),
    }
}

/// Used by the sampler's rule handling; re-exported for clarity.
pub type Commands = Vec<RuleCommand>;

/// Whether `pid` is the agent itself.
pub fn is_self(pid: Pid) -> bool {
    pid == std::process::id()
}
