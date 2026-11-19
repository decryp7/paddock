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

    /// A tick's recording without a fresh process table (the store-writer contract harness).
    #[cfg(test)]
    pub(crate) fn record_for_test(&mut self, shared: &Arc<Shared>, sample: &SystemSample, overhead: Overhead, t: Timestamp) {
        self.record(shared, sample, None, overhead, t);
    }

    /// A tick's recording with a process table (the store-writer contract harness).
    #[cfg(test)]
    pub(crate) fn record_with_table_for_test(
        &mut self,
        shared: &Arc<Shared>,
        sample: &SystemSample,
        table: (Vec<ProcessSample>, Vec<AppGroup>),
        overhead: Overhead,
        t: Timestamp,
    ) {
        self.record(shared, sample, Some(table), overhead, t);
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
        // The tick's writes (history, disks, prune, alerts, alarms) are one transaction, not one commit per statement under the
        // core lock (rule 85; ADR 0108 DY-7). What they announce waits for the commit: an app is never told of an alert or an
        // alarm change that was not stored, and a tick that was not stored leaves the history writer as it was.
        let mut before = crate::tick_state::TickState::of(core);
        let batch = core.store.batch();
        let mut news: Vec<News> = Vec::new();
        let groups_for_history: &[AppGroup] = if fresh { &core.latest.groups } else { &[] };
        if let Err(e) = core.writer.record(&core.store, sample, groups_for_history) {
            log::error!("dyno-agent: history write failed: {e}");
        }
        if t.millis() % 900_000 < 5_000 {
            let _ = core.writer.record_disks(&core.store, sample);
        }
        let _ = core.writer.prune(&core.store, core.settings.retention, t);
        let settings = core.settings.alerts;
        let groups = core.latest.groups.clone();
        let new_alerts = core.alerts.tick(&settings, sample, &groups, &[]);
        let custom_rules = core.settings.custom_alerts.clone();
        let observations: Vec<_> = core.custom.evaluate(t, &custom_rules, sample).into_iter().collect();
        if !new_alerts.is_empty() || !observations.is_empty() || core.alarms.tick_due(t) {
            before.touch_alarms();
        }
        for a in new_alerts {
            let changes = core.alarms.alert(&core.store, &a);
            for c in &changes {
                let _ = core.recorder.on_alarm(&core.store, c, t);
            }
            news.push(News::Alarms(changes));
            // The alert's rows are one whole, all or none for this alert.
            let stored = core.store.atomic(|| {
                core.store.put_alert(&a)?;
                core.store.put_event(&TimelineEvent {
                    at: a.at,
                    kind: TimelineEventKind::Alert,
                    label: a.title.clone(),
                })
            });
            match stored {
                Ok(()) => news.push(News::Alert(a)),
                Err(e) => {
                    // Undone alone (rule 85): its rows are rolled back, its cooldown is taken back, it is not announced.
                    log::error!("dyno-agent: an alert could not be stored: {e}; it can fire again at the next tick");
                    core.alerts.unfire(&a.id);
                }
            }
        }
        for o in observations {
            let changes = core.alarms.observe(&core.store, o, t);
            for c in &changes {
                let _ = core.recorder.on_alarm(&core.store, c, t);
            }
            news.push(News::Alarms(changes));
        }
        news.push(News::Alarms(core.alarms.tick(&core.store, t)));
        match batch.commit() {
            Ok(()) => {
                for n in news {
                    match n {
                        News::Alarms(changes) => push_alarm_changes(shared, changes),
                        News::Alert(a) => shared.broadcast_alert(&a),
                    }
                }
            }
            Err(e) => {
                before.restore(core);
                let withheld = news
                    .iter()
                    .map(|n| match n {
                        News::Alarms(c) => c.len(),
                        News::Alert(_) => 1,
                    })
                    .sum::<usize>();
                log::error!(
                    withheld = withheld;
                    "dyno-agent: the tick's history, alerts and alarm changes were not stored ({e}); they are not announced, and the tick's state is put back so the next tick writes and announces them again"
                );
            }
        }
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
                processes_hint: self.provenance.hint.clone(),
            },
        };
        core.latest.live = Some(live.clone());
        drop_pressure_unused(pressure);
        drop(groups);
        push_live(shared, &live);
    }
}

fn drop_pressure_unused(_: MemoryPressure) {}

/// What a tick announces to the connected apps once its writes are stored, in the order it happened.
enum News {
    /// Alarm changes.
    Alarms(Vec<dyno_core::alarms::AlarmChange>),
    /// A new alert.
    Alert(dyno_core::alerts::Alert),
}

/// Pushes alarm changes to every connected app.
pub fn push_alarm_changes(shared: &Shared, changes: Vec<dyno_core::alarms::AlarmChange>) {
    for c in changes {
        shared.broadcast_alarm(&c);
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

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::time::HOUR;
    use dyno_platform::fake::{process, quiet_sample, Fake};

    fn always_on() -> dyno_core::alarms::custom::CustomAlertRule {
        dyno_core::alarms::custom::CustomAlertRule {
            id: "always".into(),
            name: "Always".into(),
            metric: dyno_core::alarms::custom::CustomMetric::CpuTotal,
            op: dyno_core::alarms::custom::CustomOp::Below,
            limit: 1_000.0,
            for_secs: 0,
            severity: dyno_core::health::Severity::Warning,
            enabled: true,
        }
    }

    /// A tick that raises an alarm, with a connected app listening; `refuse` makes the tick's commit fail. Returns the alarm
    /// messages the app was sent and the alarms stored.
    fn tick_with_alarm(refuse: bool) -> (usize, usize) {
        ticks_with_alarm(&[refuse])
    }

    /// Ticks one second apart on one agent, each refused or not; returns what the app was sent and the alarms stored at the end.
    fn ticks_with_alarm(refused: &[bool]) -> (usize, usize) {
        let (sent, stored) = ticks_with_alarm_kinds(refused);
        (sent.len(), stored)
    }

    /// [`ticks_with_alarm`], with the kind of each alarm change the app was sent.
    fn ticks_with_alarm_kinds(refused: &[bool]) -> (Vec<dyno_core::alarms::ChangeKind>, usize) {
        let tmp = paddock_testkit::private_tempdir().unwrap();
        let fake = Fake::new();
        let (shared, sensors, processes) = Shared::build(
            crate::paths::Dirs::at(tmp.path().join("data")),
            fake.platform(),
            Box::new(fake.clone()),
            Box::new(|_| Ok(())),
        )
        .unwrap();
        lock(&shared.core).settings.custom_alerts = vec![always_on()];
        let (tx, _rx) = std::sync::mpsc::sync_channel(64);
        let pending = Arc::new(crate::agent::Pending::default());
        lock(&shared.subs).push(crate::agent::Sub {
            id: 1,
            detail: Arc::new(std::sync::atomic::AtomicU8::new(1)),
            tx,
            scans: Default::default(),
            volumes: Default::default(),
            jobs: Default::default(),
            pending: pending.clone(),
        });
        let mut sampler = Sampler::new(sensors, processes);
        let commits = lock(&shared.core).store.count_commits();
        let overhead = Overhead {
            agent_cpu: 0.0,
            agent_memory: 0,
            sampling_interval_ms: 2_000,
            process_interval_ms: 10_000,
        };
        for (i, refuse) in refused.iter().enumerate() {
            commits.fail(*refuse);
            let t = Timestamp(HOUR + 1 + i as i64 * 1_000);
            sampler.record(&shared, &quiet_sample(t), None, overhead, t);
        }
        commits.fail(false);
        let mut alarms = Vec::new();
        pending.drain(&mut |m| {
            if let Message::Alarm(c) = m {
                alarms.push(c.kind);
            }
            true
        });
        let stored = lock(&shared.core).store.alarms(10).unwrap().len();
        (alarms, stored)
    }

    /// Review P2-1: a tick announces its alarms after they are stored, and not at all when its commit fails.
    #[test]
    fn a_tick_announces_its_alarms_only_once_they_are_stored() {
        assert_eq!(tick_with_alarm(false), (1, 1), "stored, then announced");
        assert_eq!(tick_with_alarm(true), (0, 0), "not stored, so not announced");
    }

    /// Third and fourth review D2, a measurement (apps/dyno/docs/performance.md, "Tick state with 5,000 alarms"): one tick under
    /// the core lock with `LOAD_LIMIT` alarms loaded, with and without alarm work (no copy of the engine either way), and the
    /// failed-commit path's reading of the alarms back from the store. Run: `cargo test --release -p dyno-agent --lib tick_state_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement; run in release"]
    fn tick_state_cost_with_5000_alarms() {
        use dyno_core::alarms::{AlarmConfig, AlarmEngine, AlarmEvent};
        let tmp = paddock_testkit::private_tempdir().unwrap();
        let fake = Fake::new();
        let (shared, sensors, processes) = Shared::build(
            crate::paths::Dirs::at(tmp.path().join("data")),
            fake.platform(),
            Box::new(fake.clone()),
            Box::new(|_| Ok(())),
        )
        .unwrap();
        {
            let mut core = lock(&shared.core);
            let mut e = AlarmEngine::new(AlarmConfig::default(), dyno_core::time::FixedZone::shared(0));
            let b = core.store.batch();
            for i in 0..5_000 {
                for c in e.apply(AlarmEvent::Raise {
                    rule: format!("alert:r{i}"),
                    key: format!("alert:r{i}:k"),
                    severity: dyno_core::health::Severity::Warning,
                    title: format!("Alarm number {i} with a title of a usual length"),
                    detail: "Some detail about what was seen, as a person reads it.".into(),
                    value: None,
                    metric: None,
                    ttl_ms: None,
                    at: Timestamp(1),
                }) {
                    core.store.put_alarm(&c.alarm).unwrap();
                }
            }
            b.commit().unwrap();
            core.alarms = crate::alarms::AlarmCentre::load(&core.store, AlarmConfig::default(), dyno_core::time::FixedZone::shared(0));
            assert_eq!(core.store.alarms(10_000).unwrap().len(), 5_000, "precondition: 5,000 alarms stored");
            assert!(
                core.alarms.exists(1) && core.alarms.exists(5_000),
                "precondition: all 5,000 alarms loaded"
            );
        }
        let mut sampler = Sampler::new(sensors, processes);
        let overhead = Overhead {
            agent_cpu: 0.0,
            agent_memory: 0,
            sampling_interval_ms: 2_000,
            process_interval_ms: 10_000,
        };
        let median = |mut v: Vec<std::time::Duration>| {
            v.sort();
            v[v.len() / 2]
        };
        // Warm: the first tick runs the alarm tick (due at once); the ones within 30 s after it have no alarm work.
        let t0 = HOUR + 1;
        sampler.record_for_test(&shared, &quiet_sample(Timestamp(t0)), overhead, Timestamp(t0));
        let quiet: Vec<_> = (1..=20)
            .map(|i| {
                let t = Timestamp(t0 + i * 1_000);
                let started = Instant::now();
                sampler.record_for_test(&shared, &quiet_sample(t), overhead, t);
                started.elapsed()
            })
            .collect();
        let reloads: Vec<_> = (0..20)
            .map(|_| {
                let mut core = lock(&shared.core);
                let started = Instant::now();
                let state = crate::tick_state::TickState::of(&core);
                assert!(!state.touched_alarms());
                let crate::agent::Core { alarms, store, .. } = &mut *core;
                alarms.reload(store);
                started.elapsed()
            })
            .collect();
        lock(&shared.core).settings.custom_alerts = vec![always_on()];
        let busy: Vec<_> = (21..=40)
            .map(|i| {
                let t = Timestamp(t0 + i * 1_000);
                let started = Instant::now();
                sampler.record_for_test(&shared, &quiet_sample(t), overhead, t);
                started.elapsed()
            })
            .collect();
        println!(
            "tick_state_cost: 5000 alarms; median of 20: quiet tick {:?}, tick with alarm work {:?}, tick state plus reading the alarms back (the failed-commit path) {:?}",
            median(quiet),
            median(busy),
            median(reloads)
        );
    }

    /// Fourth review P1: a writer that keeps failing (every alert row refused) costs only its own rows: 60 ticks still store
    /// their 60 raw points (the batch was failed by the alert and stored none).
    #[test]
    fn a_writer_that_keeps_failing_does_not_stop_the_history() {
        let tmp = paddock_testkit::private_tempdir().unwrap();
        let fake = Fake::new();
        let (shared, sensors, processes) = Shared::build(
            crate::paths::Dirs::at(tmp.path().join("data")),
            fake.platform(),
            Box::new(fake.clone()),
            Box::new(|_| Ok(())),
        )
        .unwrap();
        let mut sampler = Sampler::new(sensors, processes);
        let overhead = Overhead {
            agent_cpu: 0.0,
            agent_memory: 0,
            sampling_interval_ms: 2_000,
            process_interval_ms: 10_000,
        };
        lock(&shared.core)
            .store
            .exec_for_test("CREATE TRIGGER no_alert BEFORE INSERT ON alerts BEGIN SELECT RAISE(ABORT, 'refused'); END;");
        let start = HOUR + 1;
        for i in 0..60 {
            let t = Timestamp(start + i * dyno_core::history::RAW_STEP_MS);
            let mut s = quiet_sample(t);
            s.disks.push(dyno_core::metrics::DiskSample {
                id: "d".into(),
                name: "Disk".into(),
                mount: "/".into(),
                total: 100,
                free: 1,
                removable: false,
            });
            sampler.record(&shared, &s, None, overhead, t);
        }
        let raw = lock(&shared.core)
            .store
            .points(dyno_core::history::Tier::Raw, Timestamp(start - 1), Timestamp(start + HOUR))
            .unwrap()
            .len();
        assert_eq!(raw, 60, "the history kept going");
    }

    /// Third review D1: an alert whose row is refused is not announced, and its cooldown is not kept: the next good tick fires it.
    #[test]
    fn an_alert_that_was_not_stored_fires_again_on_the_next_tick() {
        let tmp = paddock_testkit::private_tempdir().unwrap();
        let fake = Fake::new();
        let (shared, sensors, processes) = Shared::build(
            crate::paths::Dirs::at(tmp.path().join("data")),
            fake.platform(),
            Box::new(fake.clone()),
            Box::new(|_| Ok(())),
        )
        .unwrap();
        let (tx, _rx) = std::sync::mpsc::sync_channel(64);
        let pending = Arc::new(crate::agent::Pending::default());
        lock(&shared.subs).push(crate::agent::Sub {
            id: 1,
            detail: Arc::new(std::sync::atomic::AtomicU8::new(1)),
            tx,
            scans: Default::default(),
            volumes: Default::default(),
            jobs: Default::default(),
            pending: pending.clone(),
        });
        let mut sampler = Sampler::new(sensors, processes);
        let overhead = Overhead {
            agent_cpu: 0.0,
            agent_memory: 0,
            sampling_interval_ms: 2_000,
            process_interval_ms: 10_000,
        };
        let full = |t: Timestamp| {
            let mut s = quiet_sample(t);
            s.disks.push(dyno_core::metrics::DiskSample {
                id: "d".into(),
                name: "Disk".into(),
                mount: "/".into(),
                total: 100,
                free: 1,
                removable: false,
            });
            s
        };
        let alerts = |pending: &crate::agent::Pending| {
            let mut n = 0;
            pending.drain(&mut |m| {
                n += usize::from(matches!(m, Message::Alert(_)));
                true
            });
            n
        };
        lock(&shared.core)
            .store
            .exec_for_test("CREATE TRIGGER no_alert BEFORE INSERT ON alerts BEGIN SELECT RAISE(ABORT, 'refused'); END;");
        let t = Timestamp(HOUR + 1);
        sampler.record(&shared, &full(t), None, overhead, t);
        assert_eq!(alerts(&pending), 0, "not stored, not announced");
        lock(&shared.core).store.exec_for_test("DROP TRIGGER no_alert;");
        let t = Timestamp(HOUR + 2_001);
        sampler.record(&shared, &full(t), None, overhead, t);
        assert_eq!(alerts(&pending), 1, "the cooldown of the alert that was not stored was not kept");
    }

    /// Second review: a tick that was not stored leaves the alarm engine, the alert and custom evaluators and the recorder as
    /// they were, so the next good tick raises, stores and announces the alarm.
    #[test]
    fn a_good_tick_after_a_failed_one_raises_the_alarm_again() {
        assert_eq!(ticks_with_alarm(&[true, false]), (1, 1));
        // The app never heard of it, so it is told of a new alarm, not a repeat of one the engine kept from the failed tick.
        assert_eq!(ticks_with_alarm_kinds(&[true, false]).0, [dyno_core::alarms::ChangeKind::Raised]);
    }

    /// DY-7 (ADR 0108, guard D): one sampler tick under the core lock writes its history, disks, prune, alerts and alarms in
    /// one transaction, not one autocommit statement each.
    #[test]
    fn a_sampler_tick_is_one_commit() {
        let tmp = paddock_testkit::private_tempdir().unwrap();
        let fake = Fake::new();
        fake.add_process(process(100, "busy", 60.0));
        let (shared, sensors, processes) = Shared::build(
            crate::paths::Dirs::at(tmp.path().join("data")),
            fake.platform(),
            Box::new(fake.clone()),
            Box::new(|_| Ok(())),
        )
        .unwrap();
        let mut sampler = Sampler::new(sensors, processes);
        // A quarter-hour mark more than an hour after the epoch: the disks are due, the prune is due, every rollup tier runs.
        let t = Timestamp(4 * 900_000 + HOUR);
        let mut sample = quiet_sample(t);
        for i in 0..3 {
            sample.disks.push(dyno_core::metrics::DiskSample {
                id: format!("disk{i}"),
                name: "Disk".into(),
                mount: format!("/d{i}"),
                total: 100,
                free: 40,
                removable: false,
            });
        }
        let procs = vec![process(100, "busy", 60.0)];
        let groups = group(&procs);
        let overhead = Overhead {
            agent_cpu: 0.0,
            agent_memory: 0,
            sampling_interval_ms: 2_000,
            process_interval_ms: 10_000,
        };
        let commits = lock(&shared.core).store.count_commits();
        sampler.record(&shared, &sample, Some((procs, groups)), overhead, t);
        assert_eq!(commits.take(), 1, "one tick, one transaction");
        let core = lock(&shared.core);
        assert_eq!(core.store.disk_free(Timestamp(0), Timestamp(t.millis() + HOUR)).unwrap().len(), 3);
    }
}
