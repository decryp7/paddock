//! The sampling loop (ADR 0002): measures, writes history, lets the rules and alerts react, and pushes a live update
//! to every subscriber. One thread; a panic in a tick is caught and the loop goes on.

use crate::agent::{lock, now, Shared, RECENT_SAMPLES};
use crate::handlers::situation;
use dyno_core::activity::{ActivityKind, UndoRecord};
use dyno_core::grouping::{group, AppGroup};
use dyno_core::metrics::{derive_pressure, MemoryPressure, PowerSource, SystemSample};
use dyno_core::ports::{ProcessSource, SystemSensors};
use dyno_core::process::{ProcessAction, ProcessSample};
use dyno_core::protect::Protection;
use dyno_core::protocol::{LiveDetail, LiveUpdate, Message, Overhead, TimelineEvent, TimelineEventKind};
use dyno_core::rules::{Command, StepAction, Tick};
use dyno_core::sampling::{intervals, Intervals, Load};
use dyno_core::time::Timestamp;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The most groups sent as "top" lists.
const TOP: usize = 6;

/// What the loop keeps between ticks.
pub struct Sampler {
    sensors: Box<dyn SystemSensors>,
    processes: Box<dyn ProcessSource>,
    last_processes: Option<Instant>,
    own_cpu: f32,
    own_memory: u64,
    ticks: u64,
}

impl Sampler {
    /// A sampler over the two ports.
    pub fn new(sensors: Box<dyn SystemSensors>, processes: Box<dyn ProcessSource>) -> Sampler {
        Sampler {
            sensors,
            processes,
            last_processes: None,
            own_cpu: 0.0,
            own_memory: 0,
            ticks: 0,
        }
    }

    /// Runs until the agent is stopped, then releases everything rules and clicks applied.
    pub fn run(mut self, shared: &Arc<Shared>) {
        while !shared.stopped() {
            let started = Instant::now();
            let iv = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.tick(shared))) {
                Ok(iv) => iv,
                Err(_) => {
                    eprintln!("dyno-agent: a sampling tick panicked; continuing");
                    Intervals {
                        system_ms: 2_000,
                        process_ms: 10_000,
                    }
                }
            };
            let spent = started.elapsed();
            shared.sleep(
                Duration::from_millis(u64::from(iv.system_ms))
                    .saturating_sub(spent)
                    .max(Duration::from_millis(50)),
            );
        }
        shutdown(shared);
    }

    /// One tick: returns the intervals to use next.
    pub fn tick(&mut self, shared: &Arc<Shared>) -> Intervals {
        let t = now();
        let (settings, rules_active, on_battery) = {
            let core = lock(&shared.core);
            (
                core.settings.sampling,
                core.rules.rules.iter().any(|r| r.enabled) && !core.rules.paused,
                core.recent
                    .back()
                    .and_then(|s| s.battery.value())
                    .is_some_and(|b| b.source == PowerSource::Battery),
            )
        };
        let watching = shared.watching();
        let iv = intervals(
            settings,
            Load {
                watching,
                rules_active,
                on_battery,
                own_cpu: self.own_cpu,
            },
        );
        let mut sample = self.sensors.sample(t);
        sample.at = t;
        let process_due = self
            .last_processes
            .is_none_or(|l| l.elapsed() >= Duration::from_millis(u64::from(iv.process_ms)))
            || (watching == Some(LiveDetail::Processes)
                && self
                    .last_processes
                    .is_none_or(|l| l.elapsed() >= Duration::from_millis(u64::from(iv.system_ms))));
        let table = process_due.then(|| {
            self.last_processes = Some(Instant::now());
            let procs = self.processes.processes();
            let groups = group(&procs);
            self.measure_self(&procs, shared.cores);
            (procs, groups)
        });
        let overhead = Overhead {
            agent_cpu: self.own_cpu,
            agent_memory: self.own_memory,
            sampling_interval_ms: iv.system_ms,
            process_interval_ms: iv.process_ms,
        };
        let commands = self.record(shared, &sample, table, overhead, t);
        apply_commands(shared, commands, t);
        self.ticks += 1;
        iv
    }

    /// Own CPU as a share of one core, and resident memory, from the process table.
    fn measure_self(&mut self, procs: &[ProcessSample], cores: usize) {
        if let Some(me) = procs.iter().find(|p| p.pid == std::process::id()) {
            self.own_cpu = me.cpu * cores as f32;
            self.own_memory = me.memory;
        }
    }

    fn record(
        &mut self,
        shared: &Arc<Shared>,
        sample: &SystemSample,
        table: Option<(Vec<ProcessSample>, Vec<AppGroup>)>,
        overhead: Overhead,
        t: Timestamp,
    ) -> Vec<Command> {
        let fresh = table.is_some();
        let mut commands = Vec::new();
        let (rules_run, power) = {
            let core = lock(&shared.core);
            (
                core.rules.rules.iter().any(|r| r.enabled) && fresh,
                sample.battery.value().map(|b| b.source),
            )
        };
        // Foreground and idle time are OS calls: only when a rule might use them and the table is fresh.
        let sit = rules_run.then(|| situation(shared, power));
        let mut core = lock(&shared.core);
        if let Some((procs, groups)) = table {
            core.latest.processes = procs;
            core.latest.groups = groups;
        }
        core.recent.push_back(sample.clone());
        while core.recent.len() > RECENT_SAMPLES {
            core.recent.pop_front();
        }
        let core = &mut *core;
        let groups_for_history: &[AppGroup] = if fresh { &core.latest.groups } else { &[] };
        if let Err(e) = core.writer.record(&mut core.store, sample, groups_for_history) {
            eprintln!("dyno-agent: history write failed: {e}");
        }
        if t.millis() % 900_000 < 5_000 {
            let _ = core.writer.record_disks(&core.store, sample);
        }
        let _ = core.writer.prune(&core.store, core.settings.retention, t);
        if let (true, Some(sit)) = (fresh, sit) {
            let own = [std::process::id()];
            let protection = Protection::new(&core.rules.protected, &own);
            let rules = core.rules.clone();
            commands = core.engine.tick(
                &rules,
                &protection,
                &Tick {
                    now: t,
                    groups: &core.latest.groups,
                    processes: &core.latest.processes,
                    situation: sit,
                },
            );
        }
        let settings = core.settings.alerts;
        let groups = core.latest.groups.clone();
        let new_alerts = core.alerts.tick(&settings, sample, &groups, &[]);
        for a in new_alerts {
            if core.store.put_alert(&a).is_ok() {
                let _ = core.store.put_event(&TimelineEvent {
                    at: a.at,
                    kind: TimelineEventKind::Alert,
                    label: a.title.clone(),
                });
                shared.broadcast(&Message::Alert(a));
            }
        }
        let pressure = sample
            .memory
            .pressure
            .value()
            .copied()
            .unwrap_or_else(|| derive_pressure(&sample.memory));
        let live = LiveUpdate {
            system: sample.clone(),
            pressure,
            top_cpu: top(&core.latest.groups, |g| f64::from(g.cpu)),
            top_memory: top(&core.latest.groups, |g| g.memory as f64),
            overhead,
            profile: core.rules.profile,
            rules_paused: core.rules.paused,
            active_actions: core.engine.active(&core.rules),
            processes: Some(core.latest.processes.clone()),
            groups: Some(core.latest.groups.clone()),
        };
        core.latest.live = Some(live.clone());
        drop_pressure_unused(pressure);
        drop(groups);
        push_live(shared, &live);
        commands
    }
}

fn drop_pressure_unused(_: MemoryPressure) {}

fn top(groups: &[AppGroup], key: impl Fn(&AppGroup) -> f64) -> Vec<AppGroup> {
    let mut v: Vec<AppGroup> = groups.to_vec();
    v.sort_by(|a, b| key(b).total_cmp(&key(a)));
    v.truncate(TOP);
    v
}

/// The update trimmed to what a subscriber asked for.
pub fn for_detail(mut live: LiveUpdate, detail: LiveDetail) -> LiveUpdate {
    if detail == LiveDetail::Summary {
        live.processes = None;
        live.groups = None;
    }
    live
}

fn push_live(shared: &Shared, live: &LiveUpdate) {
    for s in lock(&shared.subs).iter() {
        if let Some(d) = s.wants() {
            let _ = s.tx.try_send(Message::Live(Box::new(for_detail(live.clone(), d))));
        }
    }
}

/// Carries out what the rules engine decided and logs it. A failure is logged, never retried in a tight loop: the
/// engine asks again on its next tick if the condition still holds.
pub fn apply_commands(shared: &Arc<Shared>, commands: Vec<Command>, t: Timestamp) {
    for c in commands {
        match c {
            Command::Apply {
                rule_id,
                app,
                target,
                step,
            } => {
                let Some(action) = step.apply() else { continue };
                let name = process_name(shared, target.pid);
                let previous = previous_priority(shared, target);
                let result = lock(&shared.ports.control).apply(target, &action);
                let mut core = lock(&shared.core);
                match result {
                    Ok(()) => {
                        if let ProcessAction::SetPriority(_) = action {
                            if let Some(p) = previous {
                                core.previous_priority.insert(target, p);
                            }
                        }
                        let undo = action_undo(&action, previous);
                        let entry = core.log(
                            t,
                            ActivityKind::Rule,
                            format!("A rule {} for {name}", step.describe()),
                            format!("Rule \"{rule_id}\" acted because the app stayed busy. Undo it here or turn the rule off in Rules."),
                            UndoRecord::Process {
                                target,
                                actions: undo,
                                rule: Some((rule_id.clone(), app.clone())),
                            },
                            None,
                        );
                        let kind = if matches!(step, StepAction::CpuCap(_)) {
                            TimelineEventKind::Cap
                        } else {
                            TimelineEventKind::Throttle
                        };
                        let _ = core.store.put_event(&TimelineEvent {
                            at: t,
                            kind,
                            label: format!("{name}: {}", step.describe()),
                        });
                        drop(core);
                        shared.broadcast(&Message::Activity(entry));
                    }
                    Err(e) => eprintln!("dyno-agent: rule step on {name} failed: {e}"),
                }
            }
            Command::Undo { target, step, .. } => {
                let previous = lock(&shared.core).previous_priority.remove(&target);
                let Some(action) = step.undo(previous) else { continue };
                if lock(&shared.ports.control).apply(target, &action).is_ok() {
                    let name = process_name(shared, target.pid);
                    let core = lock(&shared.core);
                    let _ = core.store.put_event(&TimelineEvent {
                        at: t,
                        kind: TimelineEventKind::Release,
                        label: format!("{name}: released"),
                    });
                }
            }
            Command::Notify { rule_name, app, cpu } => {
                let mut core = lock(&shared.core);
                let entry = core.log(
                    t,
                    ActivityKind::Alert,
                    format!("{app} is using {cpu:.0}% CPU"),
                    format!("Rule \"{rule_name}\" asked Dyno to tell you."),
                    UndoRecord::None,
                    None,
                );
                drop(core);
                shared.broadcast(&Message::Activity(entry));
            }
        }
    }
}

fn action_undo(action: &ProcessAction, previous: Option<dyno_core::process::Priority>) -> Vec<ProcessAction> {
    dyno_core::activity::inverse(
        action,
        dyno_core::activity::Previous {
            priority: previous,
            ..Default::default()
        },
    )
    .into_iter()
    .collect()
}

fn previous_priority(shared: &Shared, target: dyno_core::process::ProcessRef) -> Option<dyno_core::process::Priority> {
    lock(&shared.ports.details).details(target).ok().and_then(|d| d.priority)
}

fn process_name(shared: &Shared, pid: u32) -> String {
    lock(&shared.core)
        .latest
        .processes
        .iter()
        .find(|p| p.pid == pid)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| format!("process {pid}"))
}

/// Releases everything rules and clicks applied (also what a crashed earlier run left).
fn shutdown(shared: &Arc<Shared>) {
    let (commands, t) = {
        let mut core = lock(&shared.core);
        let rules = core.rules.clone();
        let procs = core.latest.processes.clone();
        (core.engine.release_all(&rules, &procs), now())
    };
    apply_commands(shared, commands, t);
    lock(&shared.ports.control).release_all();
}
