//! Answers the app's requests (command–query separation: queries only read; open, save and the like change only
//! Dyno's own files). Every argument is validated here even though the app validated it too.

use crate::agent::{lock, now, Shared, FACTS_TTL};
use dyno_core::alarms::{Alarm, AlarmEvent, ShelveFor};
use dyno_core::anomaly;
use dyno_core::diagnosis;
use dyno_core::facts::SystemFacts;
use dyno_core::forecast::{disk_full, DiskTrend};
use dyno_core::health::{assess, HealthInput};
use dyno_core::history::{downsample, top_consumers, window, Tier};
use dyno_core::metrics::SystemSample;
use dyno_core::protocol::{AgentError, ErrorCode, History, LiveDetail, Message, PluginInfo, PluginState, Reply, Request};
use dyno_core::recorder::CaptureReason;
use dyno_core::report::{self, ReportDisk, ReportFormat, ReportInput, ReportIssue, SavedReport, ScorePoint, WeeklyReport};
use dyno_core::snapshot;
use dyno_core::time::{Timestamp, DAY, HOUR};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

type Answer = Result<Reply, AgentError>;

fn bad_edit(e: dyno_core::intel_lists::EditError) -> AgentError {
    bad(e.to_string())
}

fn bad(msg: impl Into<String>) -> AgentError {
    AgentError::new(ErrorCode::BadRequest, msg)
}

fn internal(msg: impl std::fmt::Display) -> AgentError {
    AgentError::new(ErrorCode::Internal, msg.to_string())
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

/// Starts a benchmark run after validating the request; the load recorded is the machine's as of the click.
fn start_benchmark(
    shared: &Arc<Shared>,
    plan: Result<dyno_core::bench::RunPlan, String>,
    phase: Option<dyno_core::bench::BenchPhase>,
    allow_battery: bool,
) -> Answer {
    let plan = plan.map_err(bad)?;
    let load = {
        let core = lock(&shared.core);
        core.recent
            .back()
            .map(|s| dyno_core::bench::LoadAtRun {
                cpu_percent: s.cpu.total,
                memory_percent: s.memory_percent(),
                on_battery: s.battery.value().map(|b| b.source == dyno_core::metrics::PowerSource::Battery),
            })
            .unwrap_or_default()
    };
    let inv = crate::inventory::get(shared);
    let machine = format!("{} · {}", inv.os, inv.cpu.name);
    shared
        .bench
        .start(plan, phase, load, machine, allow_battery)
        .map(|o| Reply::Benchmarks(Box::new(o)))
}

fn dispatch(shared: &Arc<Shared>, conn: &Conn, request: Request) -> Answer {
    match request {
        Request::Subscribe { detail } => subscribe(shared, conn, detail),
        Request::ProcessDetails { target } => {
            let d = lock(&shared.ports.details).details(target)?;
            Ok(Reply::Details(Box::new(d)))
        }
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
        Request::Activity { limit } => Ok(Reply::Activities(
            lock(&shared.core).store.activity(limit.clamp(1, 500)).map_err(internal)?,
        )),
        Request::Alerts { limit } => Ok(Reply::Alerts(
            lock(&shared.core).store.alerts(limit.clamp(1, 500)).map_err(internal)?,
        )),
        Request::Settings => Ok(Reply::Settings(Box::new(lock(&shared.core).settings.clone()))),
        Request::SaveSettings { settings } => {
            settings.check_extras().map_err(bad)?;
            let mut core = lock(&shared.core);
            // The layouts change only through `EditWidgetLayout`: a window's copy of the settings, taken before an edit,
            // must never put an old layout back.
            let mut incoming = (*settings).normalised();
            incoming.dashboard = core.settings.dashboard.clone();
            incoming.dashboard_v1 = core.settings.dashboard_v1.clone();
            incoming.menu_bar = core.settings.menu_bar.clone();
            incoming.popover = core.settings.popover.clone();
            // What the timed actions did (last day run, done) is the agent's own: a window's copy never rewinds it.
            for e in &mut incoming.power.timed.entries {
                if let Some(cur) = core
                    .settings
                    .power
                    .timed
                    .entries
                    .iter()
                    .find(|c| c.id == e.id && c.at_minute == e.at_minute && c.repeat == e.repeat)
                {
                    (e.last_day, e.done) = (cur.last_day, cur.done);
                }
            }
            core.settings = incoming;
            let alarm_config = core.settings.alarms;
            core.alarms.set_config(alarm_config);
            shared.disk.set_limits(core.settings.disk.limits());
            shared.disk.set_read_threads(core.settings.disk.read_threads);
            core.save_settings(&shared.dirs);
            let s = core.settings.clone();
            drop(core);
            // After the core lock is released: switching saved maps off waits for the cache writer (a save in progress can take
            // seconds), and the sampler takes the core lock every tick.
            shared.disk.apply_saved_maps(s.disk.saved_maps);
            shared.power.configure_idle(shared);
            if !s.power.keep_awake.enabled {
                // Turning the feature off ends a hold already taken (and clears a notice).
                shared.power.stop(shared);
            }
            // Turning scheduled wake on or off sets or clears the OS wakes now.
            shared.wakes.sync_all(shared);
            shared.wake_sampler();
            Ok(Reply::Settings(Box::new(s)))
        }
        Request::UpsertAlertRule { rule } => {
            let mut core = lock(&shared.core);
            dyno_core::alarms::custom::upsert_rule(&mut core.settings.custom_alerts, rule).map_err(bad)?;
            core.save_settings(&shared.dirs);
            let s = core.settings.clone();
            drop(core);
            shared.wake_sampler();
            Ok(Reply::Settings(Box::new(s)))
        }
        Request::DeleteAlertRule { id } => {
            let mut core = lock(&shared.core);
            dyno_core::alarms::custom::delete_rule(&mut core.settings.custom_alerts, &id).map_err(bad)?;
            core.save_settings(&shared.dirs);
            let s = core.settings.clone();
            drop(core);
            shared.wake_sampler();
            Ok(Reply::Settings(Box::new(s)))
        }
        Request::Plugins => Ok(Reply::Plugins(plugins(shared))),
        Request::Network { query } => shared.network.snapshot(shared, query).map(|s| Reply::Network(Box::new(s))),
        Request::ResolveHosts { addresses } => shared.network.resolve(&addresses).map(Reply::HostNames),
        Request::Firewall => shared.firewall.view().map(|v| Reply::Firewall(Box::new(v))),
        Request::IntelStatus => Ok(Reply::Intel(Box::new(shared.intel.status()))),
        Request::IntelEdit { edit } => {
            let outcome = shared.intel.edit(edit.clone()).map_err(bad_edit)?;
            if crate::intel_centre::needs_rebuild(&edit, &outcome) {
                // A rebuild already running covers a second edit shortly after (the job reads the lists when it starts).
                let _ = shared.jobs.start(shared, dyno_core::jobs::JobSpec::RebuildIntel);
            }
            if let dyno_core::intel_lists::IntelEdit::AddList {
                source: dyno_core::intel_lists::NewSource::Url { .. },
                ..
            } = &edit
            {
                if let dyno_core::intel_lists::EditOutcome::Added { id } = &outcome {
                    // A new subscription fetches at once (the user just asked for it).
                    let _ = shared.jobs.start(shared, dyno_core::jobs::JobSpec::UpdateList { id: id.clone() });
                }
            }
            Ok(Reply::IntelEdited(Box::new(outcome)))
        }
        Request::GeoSummary => {
            if shared.intel.wants_geo() {
                let _ = shared.jobs.start(shared, dyno_core::jobs::JobSpec::UpdateGeo);
            }
            let rows = shared.network.remote_rows(shared)?;
            let mut summary = shared.intel.geo_summary(&rows, 0);
            let home = shared.intel.home();
            summary.home_country = home.country;
            summary.home_source = home.source;
            Ok(Reply::Geo(Box::new(summary)))
        }
        Request::RemoveTool => match crate::tool_own_copy::remove(&shared.dirs.tools()) {
            Ok(()) => {
                // The wear reading was made with the tool: read again now, not at the next slow cycle (hours away).
                // Off the request (ER-17): the answer does not wait for smartctl.
                let shared = Arc::clone(shared);
                std::thread::Builder::new()
                    .name("dyno-wear-reread".into())
                    .spawn(move || crate::hardware::slow_cycle(&shared))
                    .map_err(|e| AgentError::new(ErrorCode::Internal, format!("The reading could not be refreshed ({}).", e.kind())))?;
                Ok(Reply::Ok)
            }
            Err(e) => Err(AgentError::new(
                ErrorCode::Internal,
                format!(
                    "The tool could not be removed ({}). Close anything using it and try again.",
                    e.kind()
                ),
            )),
        },
        Request::ToolOffer => {
            let offer = crate::tool_download::offer(shared);
            // Asking again ("Check again", or opening Settings) also reads the disks again, so a tool installed by hand is
            // picked up now and not at the next slow cycle (hours away); at most once in 30 s.
            crate::hardware::request_reread(shared, std::time::Duration::from_secs(30));
            Ok(Reply::ToolOffer(Box::new(offer)))
        }
        Request::LookupIp { ip, template } => {
            let settings = match template {
                Some(t) => dyno_core::ip_lookup::IpLookupSettings {
                    ip_template: t,
                    asn_template: String::new(),
                },
                None => lock(&shared.core).settings.ip_lookup.clone(),
            };
            settings.url(&ip).map(Reply::Url).map_err(|e| bad(e.to_string()))
        }
        Request::CounterObjects => shared.counters.objects().map(|o| Reply::CounterObjects(Box::new(o))),
        Request::CounterObject { name } => shared.counters.object(&name).map(|o| Reply::CounterObject(Box::new(o))),
        Request::CounterSearch { query, limit } => shared.counters.search(&query, limit).map(Reply::CounterHits),
        Request::CounterValues { paths } => shared
            .counters
            .values(&paths, now())
            .map(|readings| Reply::CounterValues(toned_by_dashboard(readings, &lock(&shared.core).settings))),
        Request::CounterHistory { paths, minutes } => shared
            .counters
            .history(&paths, minutes, now(), |from, to| {
                lock(&shared.core).store.events(from, to).map_err(internal)
            })
            .map(|h| Reply::CounterHistory(Box::new(h))),
        Request::Benchmarks => Ok(Reply::Benchmarks(Box::new(shared.bench.overview()))),
        Request::StartBenchmark {
            kinds,
            label,
            group,
            phase,
            disk_mib,
            allow_battery,
            disk_volumes,
            allow_network_drives,
            disk_folders,
            gpu_adapter,
            sustained_minutes,
            sustained_write_gib,
        } => start_benchmark(
            shared,
            dyno_core::bench::plan(&kinds, disk_mib, &label, group.as_deref())
                .and_then(|p| p.with_drives(&disk_volumes, allow_network_drives))
                .and_then(|p| p.with_folders(&disk_folders))
                .map(|mut p| {
                    p.gpu_adapter = gpu_adapter;
                    p
                })
                .and_then(|p| p.with_sustained(sustained_minutes, sustained_write_gib)),
            phase,
            allow_battery,
        ),
        Request::CancelBenchmark => {
            shared.bench.cancel();
            Ok(Reply::Ok)
        }
        Request::DeleteBenchmark { id } => shared.bench.delete(&id).map(|()| Reply::Ok),
        Request::CompareBenchmarks { before, after } => {
            let find = |id: &str| {
                shared
                    .bench
                    .run(id)
                    .ok_or_else(|| AgentError::new(ErrorCode::NotFound, "That benchmark run no longer exists."))
            };
            Ok(Reply::BenchComparison(Box::new(dyno_core::bench::compare(
                &find(&before)?,
                &find(&after)?,
            ))))
        }
        Request::Inventory => Ok(Reply::Inventory(Box::new(crate::inventory::get(shared)))),
        Request::HelperStatus => Ok(Reply::HelperStatus(Box::new(shared.helper.status()))),
        Request::Issues { query } => crate::issues::overview(shared, query).map(|o| Reply::Issues(Box::new(o))),
        Request::StartJob { spec } => shared.jobs.start(shared, spec).map(|s| Reply::Job(Box::new(s))),
        Request::Jobs => Ok(Reply::Jobs(shared.jobs.statuses())),
        Request::CancelJob { id } => Ok(Reply::JobCancelled(shared.jobs.cancel(id))),
        Request::JobResult { id } => shared.jobs.output(id).map(|o| Reply::JobOutput(Box::new(o))),
        Request::SaveIssues { path, days } => crate::issues::save(shared, &path, days).map(Reply::IssuesSaved),
        Request::Unusual => Ok(Reply::Unusual(Box::new(shared.unusual.overview(shared)))),
        Request::ForgetBaseline => shared.unusual.forget(shared).map(|o| Reply::Unusual(Box::new(o))),
        Request::Alarms { filter } => {
            let t = now();
            Ok(Reply::Alarms(lock(&shared.core).alarms.list(&filter, t)))
        }
        Request::AlarmStats => {
            let t = now();
            Ok(Reply::AlarmStats(Box::new(lock(&shared.core).alarms.stats(t))))
        }
        Request::AcknowledgeAlarms { ids } => {
            if ids.is_empty() || ids.len() > 500 {
                return Err(bad("Choose between 1 and 500 alarms to acknowledge."));
            }
            alarm_event(shared, AlarmEvent::Acknowledge { ids, at: now() })
        }
        Request::ShelveAlarm { id, until } => {
            let t = now();
            if let ShelveFor::Custom(end) = until {
                if end <= t {
                    return Err(bad("Choose a time in the future to shelve the alarm until."));
                }
            }
            known_alarm(shared, id)?;
            alarm_event(shared, AlarmEvent::Shelve { id, how: until, at: t })
        }
        Request::UnshelveAlarm { id } => {
            known_alarm(shared, id)?;
            alarm_event(shared, AlarmEvent::Unshelve { id })
        }
        Request::MuteRule { rule, muted } => {
            if rule.trim().is_empty() || rule.chars().count() > dyno_core::alarms::MAX_TEXT {
                return Err(bad("Give the rule to mute (up to 200 characters)."));
            }
            alarm_event(shared, AlarmEvent::Mute { rule, muted })
        }
        Request::NoteAlarm { id, text } => {
            if text.trim().is_empty() {
                return Err(bad("Write something in the note."));
            }
            known_alarm(shared, id)?;
            alarm_event(shared, AlarmEvent::Note { id, text, at: now() })
        }
        Request::Capture => {
            let t = now();
            let mut core = lock(&shared.core);
            let core = &mut *core;
            core.recorder
                .capture(&core.store, CaptureReason::Manual, t)
                .map(Reply::Captured)
                .map_err(|m| AgentError::new(ErrorCode::NotFound, m))
        }
        Request::RecorderSnapshots => Ok(Reply::RecorderSnapshots(
            lock(&shared.core).store.recorder_snapshots().map_err(internal)?,
        )),
        Request::RecorderSnapshot { id } => lock(&shared.core)
            .store
            .recorder_snapshot(id)
            .map_err(internal)?
            .map(|s| Reply::RecorderSnapshot(Box::new(s)))
            .ok_or_else(|| AgentError::new(ErrorCode::NotFound, "That capture no longer exists.")),
        Request::DiagnosticsFiles => Ok(Reply::DiagnosticsFiles(Box::new(crate::diagnostics::collect(shared)))),
        Request::AlarmHistory { alarm_id, limit } => Ok(Reply::AlarmChanges(
            lock(&shared.core).store.alarm_events(alarm_id, limit).map_err(internal)?,
        )),
        Request::Health => Ok(Reply::Health(Box::new(health(shared)?))),
        Request::StartupEntries => {
            // The explicit request reads the OS again; the filter changes and the health report reuse the cached read.
            let mut startup = lock(&shared.ports.startup);
            startup.refresh();
            Ok(Reply::Startup(startup.list()?))
        }
        Request::PowerState => Ok(Reply::Power(Box::new(shared.power.view()))),
        Request::PowerKeepAwake { spec } => {
            let enabled = lock(&shared.core).settings.power.keep_awake.enabled;
            shared
                .power
                .start(shared, spec, enabled)
                .map(|v| Reply::Power(Box::new(v)))
                .map_err(bad)
        }
        Request::EnergyReport { days } => shared
            .power
            .energy(shared, days)
            .map(|r| Reply::Energy(Box::new(r)))
            .map_err(internal),
        Request::PowerIdleCancel => Ok(Reply::Power(Box::new(shared.power.idle_cancel(shared)))),
        Request::PowerIdleSnooze { minutes } => Ok(Reply::Power(Box::new(shared.power.idle_snooze(shared, minutes)))),
        Request::PowerStopKeepAwake => Ok(Reply::Power(Box::new(shared.power.stop(shared)))),
        Request::WolDevices => shared.wol.view(shared).map(|v| Reply::Wol(Box::new(v))),
        Request::WolCandidates => shared.wol.candidates(shared).map(Reply::WolCandidates),
        Request::WolSave { device } => shared.wol.save(shared, device).map(|v| Reply::Wol(Box::new(v))),
        Request::WolRemove { id } => shared.wol.remove(shared, id).map(|v| Reply::Wol(Box::new(v))),
        Request::WolWake { id, test } => {
            shared.wol.check_wake(shared, id)?;
            shared
                .jobs
                .start(shared, dyno_core::jobs::JobSpec::WolWake { id, test })
                .map(|s| Reply::Job(Box::new(s)))
        }
        Request::PowerWakes => shared.wakes.view(shared).map(|v| Reply::PowerWakes(Box::new(v))),
        Request::PowerSetWake { entry } => shared.wakes.set(shared, entry).map(|v| Reply::PowerWakes(Box::new(v))),
        Request::PowerCancelWake { id } => shared.wakes.cancel(shared, id).map(|v| Reply::PowerWakes(Box::new(v))),
        Request::WolReadiness => Ok(Reply::WolReadiness(Box::new(shared.wol.readiness()))),
        Request::Shutdown => {
            log::info!("Shutdown requested by a client");
            shared.request_stop();
            Ok(Reply::Ok)
        }
        Request::Throttling => Ok(Reply::Throttling(Box::new(throttling(shared)))),
        Request::Lifespan => {
            let core = lock(&shared.core);
            Ok(Reply::Lifespan(Box::new(shared.hardware.lifespan_overview(&core.store, now()))))
        }
        Request::Baselines => Ok(Reply::Baselines(Box::new(shared.baselines.overview(shared)))),
        r @ (Request::DiskVolumes
        | Request::StartDiskScan { .. }
        | Request::ForgetSavedDiskMap { .. }
        | Request::ForgetSavedDiskMaps
        | Request::CancelDiskScan { .. }
        | Request::DiskScans
        | Request::DiskChildren { .. }
        | Request::DiskPath { .. }
        | Request::RetryDiskScan { .. }
        | Request::DiskReveal { .. }
        | Request::DiskRings { .. }
        | Request::OpenFullDiskAccessSettings) => crate::disk::handle(shared, r),
        r @ (Request::SearchSeed { .. } | Request::WebSearch { .. } | Request::VirusTotalUrl { .. } | Request::StartupOverview { .. }) => {
            crate::handlers_search::handle(shared, r)
        }
        r @ (Request::Changes { .. } | Request::GpuProcesses | Request::SaveInventoryReport { .. } | Request::Maintenance) => {
            crate::extras::handle(shared, r)
        }
        Request::WidgetLayout { surface } => Ok(Reply::WidgetLayout(Box::new(dyno_core::widgets::layout_view(
            &lock(&shared.core).settings,
            surface,
        )))),
        Request::EditWidgetLayout { edit, base_revision } => edit_widget_layout(shared, edit, base_revision),
        Request::PopoverValues { placements } => {
            crate::popover::values(shared, &placements).map(|v| Reply::PopoverValues(dyno_core::widgets::JsonOnLink(v)))
        }
        Request::WeeklyReport { weeks_ago } => weekly_report(shared, weeks_ago).map(|r| Reply::Report(Box::new(r))),
        Request::BenchEstimate { request } => Ok(Reply::BenchEstimate(dyno_core::bench::estimate::estimate_run(
            &dyno_core::bench::estimate::standard(),
            &request,
        ))),
        Request::BenchGpuAdapters => shared
            .bench
            .gpu_adapters()
            .map(Reply::GpuAdapters)
            .map_err(|e| AgentError::new(ErrorCode::Unsupported, e)),
        Request::SaveBenchmarks { path, format, ids } => save_benchmarks(shared, &path, format, &ids).map(Reply::ReportSaved),
        Request::SaveReport { path, format, weeks_ago } => save_report(shared, &path, format, weeks_ago).map(Reply::ReportSaved),
    }
}

/// Applies a layout edit, saves the settings and tells every app which surface changed. A refused edit changes nothing and
/// answers why.
fn edit_widget_layout(shared: &Arc<Shared>, edit: dyno_core::widgets::SurfaceEdit, base_revision: Option<u64>) -> Answer {
    let surface = edit.surface();
    let mut core = lock(&shared.core);
    let view = dyno_core::widgets::edit_layout(&mut core.settings, edit, base_revision).map_err(|e| match e {
        paddock_widgets::WidgetError::Stale => AgentError::new(ErrorCode::Stale, e.to_string()),
        e => bad(e.to_string()),
    })?;
    core.save_settings(&shared.dirs);
    drop(core);
    shared.broadcast(&Message::WidgetLayoutChanged(surface));
    Ok(Reply::WidgetLayout(Box::new(view)))
}

fn known_alarm(shared: &Shared, id: u64) -> Result<(), AgentError> {
    if lock(&shared.core).alarms.exists(id) {
        Ok(())
    } else {
        Err(AgentError::new(ErrorCode::NotFound, "That alarm is no longer in the list."))
    }
}

/// Applies an alarm event, pushes its changes and answers the changed alarms.
fn alarm_event(shared: &Shared, event: AlarmEvent) -> Answer {
    let changes = {
        let mut core = lock(&shared.core);
        let core = &mut *core;
        core.alarms.apply(&core.store, event)
    };
    let alarms: Vec<Alarm> = changes.iter().map(|c| c.alarm.clone()).collect();
    crate::sampler::push_alarm_changes(shared, changes);
    Ok(Reply::Alarms(alarms))
}

fn subscribe(shared: &Arc<Shared>, conn: &Conn, detail: LiveDetail) -> Answer {
    conn.detail.store(crate::agent::detail_code(detail), Ordering::Relaxed);
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

fn history(shared: &Shared, range: dyno_core::history::HistoryRange, end: Option<Timestamp>) -> Result<History, AgentError> {
    // A live view (no end) includes this very millisecond: a change recorded just before the question is in the answer.
    let w = match end {
        Some(end) => window(range, end),
        None => dyno_core::history::live_window(range, now()),
    };
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
        crashes: dyno_core::issues::crash_days(&s.issues(w.from, w.to, 20_000).map_err(internal)?, w.from, w.to, core.zone.as_ref()),
        core_points: {
            let raw = s.core_points(w.tier, w.from, w.to).map_err(internal)?;
            if w.resolution_ms > w.tier.step_ms() {
                dyno_core::history::downsample_cores(&raw, w.resolution_ms)
            } else {
                raw
            }
        },
    })
}

fn why_slow(shared: &Shared) -> dyno_core::diagnosis::Diagnosis {
    let core = lock(&shared.core);
    let samples: Vec<SystemSample> = core.recent.iter().rev().take(60).rev().cloned().collect();
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
        system_disk: &disk,
        throttle: shared.hardware.context(),
    })
}

fn throttling(shared: &Shared) -> dyno_core::throttle::ThrottleOverview {
    let core = lock(&shared.core);
    shared.hardware.throttle_overview(&core.store, core.recent.back(), now())
}

fn anomalies(shared: &Shared) -> Vec<dyno_core::anomaly::Anomaly> {
    let core = lock(&shared.core);
    let t = now();
    let rows = core.store.app_minutes(t.minus(6 * HOUR), t).unwrap_or_default();
    let points = core.store.points(Tier::Minute, t.minus(6 * HOUR), t).unwrap_or_default();
    let mut out = anomaly::memory_growth(&rows);
    out.extend(anomaly::periodic_spikes(&points, &rows));
    out.extend(core.crash_loops.clone());
    out
}

pub(crate) fn facts(shared: &Shared) -> SystemFacts {
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
    let lifespans = shared.hardware.lifespan_overview(&core.store, t).forecasts;
    // Shelved alarms are listed too; `assess` leaves them out of the cap.
    let alarms = core.alarms.list(
        &dyno_core::alarms::AlarmFilter {
            states: vec![dyno_core::alarms::AlarmState::Active],
            include_shelved: true,
            limit: 500,
            ..Default::default()
        },
        t,
    );
    let report = assess(&HealthInput {
        now: t,
        sample: &sample,
        facts: &facts,
        system_disk: &system_disk,
        forecasts: &trends,
        startup: &startup,
        anomalies: &anomalies,
        crashes_7d: core
            .store
            .count_issues(dyno_core::issues::IssueKind::AppCrash, t.minus(7 * DAY), t)
            .unwrap_or(0),
        lifespans: &lifespans,
        alarms: &alarms,
    });
    // Bookkeeping for the weekly report's trend (one row per hour; a failed write loses one point, nothing else).
    let _ = core.store.put_score(t, report.score);
    Ok(report)
}

/// The report for the seven days ending `weeks_ago` weeks before now, from what the store holds. A week with nothing in
/// it is still a report (`nothing_to_report`).
fn weekly_report(shared: &Shared, weeks_ago: u32) -> Result<WeeklyReport, AgentError> {
    if weeks_ago > MAX_WEEKS_AGO {
        return Err(bad(format!("Dyno keeps at most {MAX_WEEKS_AGO} weeks; choose a more recent one.")));
    }
    let to = now().minus(i64::from(weeks_ago) * 7 * DAY);
    let from = to.minus(7 * DAY);
    // The score now is measured (and recorded for the trend); earlier weeks use what was recorded.
    let live_score = if weeks_ago == 0 {
        health(shared).ok().map(|h| h.score)
    } else {
        None
    };
    let core = lock(&shared.core);
    let s = &core.store;
    let points = s.points(Tier::QuarterHour, from, to).map_err(internal)?;
    let top_apps = top_consumers(&s.app_minutes(from, to).map_err(internal)?, 5);
    let score = live_score.or(s.score_at(to).map_err(internal)?);
    // (from, to]: an alert raised in the very millisecond the report is built still counts, and adjacent weeks never share one.
    let in_week = |at: Timestamp| at > from && at <= to;
    let issues: Vec<ReportIssue> = s
        .alerts(1_000)
        .map_err(internal)?
        .into_iter()
        .filter(|a| in_week(a.at))
        .map(|a| ReportIssue {
            at: a.at,
            severity: a.severity,
            title: a.title,
            detail: a.detail,
        })
        .collect();
    let crashes = s.count_issues(dyno_core::issues::IssueKind::AppCrash, from, to).map_err(internal)?;
    let disks = s
        .disk_free(from.minus(14 * DAY), to)
        .map_err(internal)?
        .into_iter()
        .filter_map(|(disk, total, pts)| {
            let free = pts.iter().max_by_key(|p| p.at)?.free;
            Some(ReportDisk {
                forecast: disk_full(&pts),
                disk,
                total,
                free,
            })
        })
        .collect();
    Ok(report::weekly(ReportInput {
        from,
        to,
        points: &points,
        top_apps,
        score,
        score_before: s.score_at(from).map_err(internal)?,
        crashes,
        alerts: issues.len() as u32,
        score_trend: s
            .scores(from, to)
            .map_err(internal)?
            .into_iter()
            .map(|(at, score)| ScorePoint { at, score })
            .collect(),
        disks,
        issues,
        hardware: dyno_core::report::hardware_lines(
            &s.throttle_episodes(from, MAX_HARDWARE_EPISODES).map_err(internal)?,
            from,
            to,
            &shared.hardware.lifespan_overview(s, to.min(now())).forecasts,
        ),
    }))
}

/// Most throttling episodes read for one report.
const MAX_HARDWARE_EPISODES: u32 = 500;

/// Writes the report to the path the user chose in the save dialog (Markdown or HTML).
/// Writes the chosen benchmark runs as JSON or CSV. The path was chosen in the OS dialog and is checked again here.
fn save_benchmarks(
    shared: &Shared,
    path: &str,
    format: dyno_core::bench::export::ExportFormat,
    ids: &[String],
) -> Result<SavedReport, AgentError> {
    use dyno_core::bench::export;
    let dest = export::validate_destination(path, format).map_err(bad)?;
    if !dest.parent().is_some_and(Path::is_dir) {
        return Err(bad("The folder to save the results in does not exist."));
    }
    if ids.len() > export::MAX_EXPORT_RUNS {
        return Err(bad(format!("Export at most {} runs at once.", export::MAX_EXPORT_RUNS)));
    }
    let runs: Vec<dyno_core::bench::BenchRun> = ids.iter().filter_map(|id| shared.bench.run(id)).collect();
    if runs.is_empty() {
        return Err(AgentError::new(ErrorCode::NotFound, "None of those runs exists any more."));
    }
    let refs: Vec<&dyno_core::bench::BenchRun> = runs.iter().collect();
    let text = export::export(&refs, format);
    std::fs::write(&dest, text.as_bytes()).map_err(|e| {
        let code = if e.kind() == std::io::ErrorKind::PermissionDenied {
            ErrorCode::AccessDenied
        } else {
            ErrorCode::Internal
        };
        AgentError::new(code, format!("Dyno could not write the results ({e}). Choose another folder."))
    })?;
    log::info!("dyno-agent: exported {} benchmark runs as {}", runs.len(), format.extension());
    Ok(SavedReport {
        path: dest.to_string_lossy().into_owned(),
        bytes: text.len() as u64,
    })
}

fn save_report(shared: &Shared, path: &str, format: ReportFormat, weeks_ago: u32) -> Result<SavedReport, AgentError> {
    let dest = report::validate_destination(path, format).map_err(bad)?;
    if !dest.parent().is_some_and(Path::is_dir) {
        return Err(bad("The folder to save the report in does not exist."));
    }
    let zone = lock(&shared.core).zone.clone();
    let text = format.render(&weekly_report(shared, weeks_ago)?, zone.as_ref());
    std::fs::write(&dest, text.as_bytes()).map_err(|e| {
        let code = if e.kind() == std::io::ErrorKind::PermissionDenied {
            ErrorCode::AccessDenied
        } else {
            ErrorCode::Internal
        };
        AgentError::new(code, format!("Dyno could not write the report ({e}). Choose another folder."))
    })?;
    Ok(SavedReport {
        path: dest.to_string_lossy().into_owned(),
        bytes: text.len() as u64,
    })
}

/// Reports go back at most this many weeks (the store keeps about a year of quarter-hour rows; older weeks are mostly empty).
const MAX_WEEKS_AGO: u32 = 52;

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

/// Counter readings toned with the thresholds of the dashboard widget that shows each one (not the popup's or the menu bar's).
fn toned_by_dashboard(
    readings: Vec<dyno_core::counters::CounterReading>,
    settings: &dyno_core::settings::Settings,
) -> Vec<dyno_core::counters::CounterReading> {
    dyno_core::widgets::with_dashboard_thresholds(readings, &settings.dashboard.layout().placements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::counters::{CounterReading, CounterUnit};
    use dyno_core::metrics::Reading;
    use dyno_core::settings::Settings;
    use dyno_core::widgets::dashboard::{Thresholds, ValueTone};
    use dyno_core::widgets::{DashboardKind, DashboardWidget, Metric};
    use paddock_widgets::Edit;

    const PATH: &str = r"\Processor(_Total)\% Processor Time";

    fn reading() -> CounterReading {
        CounterReading::new(PATH.into(), CounterUnit::Percent, Reading::Available(60.0))
    }

    /// The `CounterValues` reply tones a counter with the dashboard widget's own thresholds: the handler reads the dashboard layout
    /// of the settings (wiring), and a counter no dashboard widget shows keeps its default tone.
    #[test]
    fn counter_values_are_toned_by_the_dashboard_widget_that_shows_them() {
        let mut settings = Settings::default();
        let mut w = DashboardWidget::new(DashboardKind::Graph, Metric::PerfCounter(PATH.into()));
        w.thresholds = Thresholds {
            warning: Some(50.0),
            critical: Some(80.0),
        };
        settings
            .dashboard
            .edit(Edit::Add {
                widget: w,
                span: None,
                at: None,
            })
            .unwrap();
        let toned = toned_by_dashboard(vec![reading()], &Settings::default());
        assert_eq!(toned[0].tone, ValueTone::Ok, "no widget shows it: the default thresholds");
        let toned = toned_by_dashboard(vec![reading()], &settings);
        assert_eq!(toned[0].tone, ValueTone::Warn, "the widget's thresholds");
    }
}
