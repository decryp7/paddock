//! The sampling loop (ADR 0002): measures, writes history, lets the alerts react, and pushes a live update
//! to every subscriber. One thread; a panic in a tick is caught and the loop goes on.

use crate::agent::{lock, now, Shared, RECENT_SAMPLES};
use dyno_core::grouping::{group, AppGroup};
use dyno_core::metrics::{derive_pressure, MemoryPressure, PowerSource, SystemSample};
use dyno_core::ports::{ProcessSource, SystemSensors};
use dyno_core::process::ProcessSample;
use dyno_core::protocol::{LiveDetail, LiveUpdate, Message, Overhead, TimelineEvent, TimelineEventKind};
use dyno_core::recorder::system_interval_ms;
use dyno_core::sampling::{intervals, Intervals, Load};
use dyno_core::time::Timestamp;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The most groups sent as "top" lists.
const TOP: usize = 6;
/// Other users' processes are folded into one "Other users" row (off while Dyno runs as administrator).
const GROUP_OTHER_USERS: bool = true;

/// What the loop keeps between ticks.
pub struct Sampler {
    sensors: Box<dyn SystemSensors>,
    processes: Box<dyn ProcessSource>,
    last_processes: Option<Instant>,
    own_cpu: f32,
    own_memory: u64,
    ticks: u64,
    last_pinned: Option<Instant>,
    /// Where the process table came from (the helper's part of it), kept between the ticks that read it.
    provenance: dyno_core::others::ProcessProvenance,
    last_watching: Option<LiveDetail>,
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
            last_pinned: None,
            provenance: Default::default(),
            last_watching: None,
        }
    }

    /// Runs until the agent is stopped.
    pub fn run(mut self, shared: &Arc<Shared>) {
        while !shared.stopped() {
            let started = Instant::now();
            let iv = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.tick(shared))) {
                Ok(iv) => iv,
                Err(_) => {
                    log::error!("dyno-agent: a sampling tick panicked; continuing");
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
    }

    /// One tick: returns the intervals to use next.
    pub fn tick(&mut self, shared: &Arc<Shared>) -> Intervals {
        let t = now();
        let (settings, recorder_on, on_battery) = {
            let core = lock(&shared.core);
            (
                core.settings.sampling,
                core.settings.flight_recorder,
                core.recent
                    .back()
                    .and_then(|s| s.battery.value())
                    .is_some_and(|b| b.source == PowerSource::Battery),
            )
        };
        let watching = shared.watching();
        // A process page opened (or opened again after "Try again"): the helper is asked at once, not after its quiet window.
        if watching == Some(LiveDetail::Processes) && self.last_watching != Some(LiveDetail::Processes) {
            self.processes.retry_helper();
        }
        self.last_watching = watching;
        let iv = intervals(
            settings,
            Load {
                watching,
                on_battery,
                own_cpu: self.own_cpu,
            },
        );
        // The flight recorder wants a one-second system sample while that is cheap; the process table keeps its pace.
        let iv = Intervals {
            system_ms: system_interval_ms(iv.system_ms, recorder_on, on_battery, self.own_cpu),
            process_ms: iv.process_ms,
        };
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
            self.provenance = self.processes.provenance();
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
        // Throttling is judged here (an external tool may be read, so never under the core lock).
        let (throttle_events, throttle_confidence) = shared.hardware.on_sample(&sample);
        self.record(shared, &sample, table, overhead, t);
        if !throttle_events.is_empty() {
            crate::hardware::handle_events(shared, throttle_events, throttle_confidence, t);
        }
        self.read_pinned_counters(shared, t);
        self.ticks += 1;
        iv
    }

    /// Gives the pinned performance counters history in the background (WP-12): a slow, bounded reading that never
    /// waits for the OS and stretches when Dyno's own CPU is over its budget.
    fn read_pinned_counters(&mut self, shared: &Arc<Shared>, t: Timestamp) {
        if self
            .last_pinned
            .is_some_and(|l| l.elapsed() < crate::counters::pinned_period(self.own_cpu))
        {
            return;
        }
        let pinned = lock(&shared.core).settings.counters.pinned.clone();
        if pinned.is_empty() {
            return;
        }
        self.last_pinned = Some(Instant::now());
        shared.counters.poll_pinned(&pinned, t);
    }

    /// Own CPU as a share of one core, and resident memory, from the process table.
    fn measure_self(&mut self, procs: &[ProcessSample], cores: usize) {
        if let Some(me) = procs.iter().find(|p| p.pid == std::process::id()) {
            self.own_cpu = me.cpu.copied().unwrap_or(0.0) * cores as f32;
            self.own_memory = me.memory.copied().unwrap_or(0);
        }
    }

    fn record(
        &mut self,
        shared: &Arc<Shared>,
        sample: &SystemSample,
        table: Option<(Vec<ProcessSample>, Vec<AppGroup>)>,
        overhead: Overhead,
        t: Timestamp,
    ) {
        let fresh = table.is_some();
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
        if core.settings.flight_recorder {
            core.recorder.record(sample, &core.latest.groups);
        }
        let groups_for_history: &[AppGroup] = if fresh { &core.latest.groups } else { &[] };
        if let Err(e) = core.writer.record(&mut core.store, sample, groups_for_history) {
            log::error!("dyno-agent: history write failed: {e}");
        }
        if t.millis() % 900_000 < 5_000 {
            let _ = core.writer.record_disks(&core.store, sample);
        }
        let _ = core.writer.prune(&core.store, core.settings.retention, t);
        let settings = core.settings.alerts;
        let groups = core.latest.groups.clone();
        let new_alerts = core.alerts.tick(&settings, sample, &groups, &[]);
        for a in new_alerts {
            let changes = core.alarms.alert(&core.store, &a);
            for c in &changes {
                let _ = core.recorder.on_alarm(&core.store, c, t);
            }
            push_alarm_changes(shared, changes);
            if core.store.put_alert(&a).is_ok() {
                let _ = core.store.put_event(&TimelineEvent {
                    at: a.at,
                    kind: TimelineEventKind::Alert,
                    label: a.title.clone(),
                });
                shared.broadcast(&Message::Alert(a));
            }
        }
        let custom_rules = core.settings.custom_alerts.clone();
        for o in core.custom.evaluate(t, &custom_rules, sample) {
            let changes = core.alarms.observe(&core.store, o, t);
            for c in &changes {
                let _ = core.recorder.on_alarm(&core.store, c, t);
            }
            push_alarm_changes(shared, changes);
        }
        let ticked = core.alarms.tick(&core.store, t);
        push_alarm_changes(shared, ticked);
        let pressure = sample
            .memory
            .pressure
            .value()
            .copied()
            .unwrap_or_else(|| derive_pressure(&sample.memory));
        let (listed, other_users) = self.provenance.split(&core.latest.processes, GROUP_OTHER_USERS);
        let listed_groups = if other_users.is_some() {
            group(&listed)
        } else {
            core.latest.groups.clone()
        };
        let live = LiveUpdate {
            system: sample.clone(),
            pressure,
            top_cpu: top(&listed_groups, |g| f64::from(g.cpu_read())),
            top_memory: top(&listed_groups, |g| g.memory_read() as f64),
            overhead,
            processes: Some(listed),
            groups: Some(listed_groups),
            other_users,
            // The temperatures are WP-CNT's; the process list and the "Other users" total say who read them (WP-PROC).
            via: dyno_core::read_via::LiveVia {
                temperatures: shared.climate.live_via(),
                processes: self.provenance.processes.clone(),
                others: self.provenance.others.clone(),
                ..Default::default()
            },
        };
        core.latest.live = Some(live.clone());
        drop_pressure_unused(pressure);
        drop(groups);
        push_live(shared, &live);
    }
}

fn drop_pressure_unused(_: MemoryPressure) {}

/// Pushes alarm changes to every connected app.
pub fn push_alarm_changes(shared: &Shared, changes: Vec<dyno_core::alarms::AlarmChange>) {
    for c in changes {
        shared.broadcast(&Message::Alarm(c));
    }
}

fn top(groups: &[AppGroup], key: impl Fn(&AppGroup) -> f64) -> Vec<AppGroup> {
    let mut v: Vec<AppGroup> = groups.to_vec();
    v.sort_by(|a, b| key(b).total_cmp(&key(a)));
    v.truncate(TOP);
    v
}

/// The update trimmed to what a subscriber asked for.
pub fn for_detail(mut live: LiveUpdate, detail: LiveDetail) -> LiveUpdate {
    if detail != LiveDetail::Processes {
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
