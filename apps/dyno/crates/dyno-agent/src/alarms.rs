//! The agent's alarm centre: the pure [`AlarmEngine`] plus persistence (`alarms`, `alarm_events`) and the push of
//! every change to connected apps. Raised from the sampler's alert evaluation and changed by
//! the app's acknowledge / shelve / mute / note requests.

use crate::store::Store;
use dyno_core::alarms::feed::{from_alert, Observation};
use dyno_core::alarms::{Alarm, AlarmChange, AlarmConfig, AlarmEngine, AlarmEvent, AlarmFilter, AlarmStats, ChangeKind, ShelveFor};
use dyno_core::alerts::Alert;
use dyno_core::time::{Timestamp, Zone};

/// How often [`AlarmCentre::tick`] does any work.
const TICK_EVERY_MS: i64 = 30_000;
/// Most alarms loaded at start.
const LOAD_LIMIT: u32 = 5_000;

/// Rules of checks that no longer exist (Backup health, removed in 1.1.0): their saved alarms and mutes are dropped on load.
fn is_removed(rule: &str) -> bool {
    rule == "alert:backup"
}

/// The alarm engine with its store. Every change is stored before it is kept and pushed (Dyno ADR 0008, "Store writers"): a
/// change whose write or commit fails is undone by reading the alarms back from the store, and not returned to push.
#[derive(Clone)]
pub struct AlarmCentre {
    engine: AlarmEngine,
    last_tick: Timestamp,
}

impl AlarmCentre {
    /// Restores the open alarms and muted rules from `store`.
    pub fn load(store: &Store, config: AlarmConfig, zone: Zone) -> AlarmCentre {
        let (alarms, gone): (Vec<_>, Vec<_>) = store
            .alarms(LOAD_LIMIT)
            .unwrap_or_default()
            .into_iter()
            .partition(|a| !is_removed(&a.rule));
        for a in &gone {
            if let Err(e) = store.delete_alarm(a.id) {
                log::error!("dyno-agent: could not drop the alarm of a removed check ({}): {e}", a.rule);
            }
        }
        let mut muted = store.muted_rules().unwrap_or_default();
        if muted.iter().any(|r| is_removed(r)) {
            muted.retain(|r| !is_removed(r));
            if let Err(e) = store.set_muted_rules(&muted) {
                log::error!("dyno-agent: could not drop the mutes of a removed check: {e}");
            }
        }
        let mut engine = AlarmEngine::restore(config, zone, alarms, muted);
        // Past every id the history names, also those of pruned alarms whose change log stays (rule 53).
        match store.highest_alarm_id() {
            Ok(Some(id)) => engine.reserve_ids_through(id),
            Ok(None) => {}
            Err(e) => log::error!("dyno-agent: the highest alarm id could not be read ({e}); new ids follow the loaded alarms"),
        }
        AlarmCentre {
            engine,
            last_tick: Timestamp(0),
        }
    }

    /// Applies the user's settings.
    pub fn set_config(&mut self, config: AlarmConfig) {
        self.engine.set_config(config);
    }

    /// Writes a batch of changes (the caller's batch commits them: rule 85; ADR 0108 DY-7).
    /// All or none (a savepoint): a failed write fails the open batch (Dyno ADR 0008, "Store writers").
    fn persist(store: &Store, changes: &[AlarmChange]) -> crate::store::Result<()> {
        store.atomic(|| {
            for c in changes {
                if c.kind == ChangeKind::Pruned {
                    store.delete_alarm(c.alarm.id)?;
                } else {
                    store.put_alarm(&c.alarm)?;
                }
                store.put_alarm_event(c)?;
            }
            Ok(())
        })
    }

    /// Runs `change` on the engine and its writes (rule 85; Dyno ADR 0008, "Store writers"): its writes are one savepoint, so
    /// it is all or none for this call, and a failure stays inside it (inside a sampler tick, the tick's other writers
    /// commit). The changes are returned to push only once stored. When its writes or the commit fail, the centre is read
    /// back from the store ([`AlarmCentre::reload`]): the store is what was stored, so nothing is copied on the way that
    /// succeeds. `change` reports a failed write through its `Result`.
    fn stored_try(
        &mut self,
        store: &Store,
        change: impl FnOnce(&mut Self, &Store) -> crate::store::Result<Vec<AlarmChange>>,
    ) -> Vec<AlarmChange> {
        let batch = store.batch();
        let written = store.atomic(|| change(&mut *self, store));
        let stored = match written {
            Ok(changes) => batch.commit().map(|()| changes),
            Err(e) => {
                // The savepoint is already rolled back; the batch holds nothing of this call.
                let _ = batch.commit();
                Err(e)
            }
        };
        match stored {
            Ok(changes) => changes,
            Err(e) => {
                log::error!(
                    "dyno-agent: alarm changes were not stored ({e}); the alarms are read back from the store and nothing is announced"
                );
                self.reload(store);
                Vec::new()
            }
        }
    }

    /// [`AlarmCentre::stored_try`] for a change that reports no write errors of its own (its writes still go through the
    /// store, and a failed commit still undoes it).
    #[allow(
        dead_code,
        reason = "the commit-then-return rule's simple form, kept with its signature for the agent's other request paths"
    )]
    fn stored(&mut self, store: &Store, change: impl FnOnce(&mut Self, &Store) -> Vec<AlarmChange>) -> Vec<AlarmChange> {
        self.stored_try(store, |me, store| Ok(change(me, store)))
    }

    /// Reads the alarms back from the store, keeping the settings and the 30 s tick mark: what a change that was not stored is
    /// undone to. The engine's in-memory counts for chattering and acknowledgement statistics start again from the stored
    /// alarms (they are not stored).
    pub fn reload(&mut self, store: &Store) {
        let last_tick = self.last_tick;
        *self = AlarmCentre::load(store, self.engine.config(), self.engine.zone());
        self.last_tick = last_tick;
    }

    /// Applies an event and stores its changes; the caller pushes them (none when they were not stored).
    pub fn apply(&mut self, store: &Store, event: AlarmEvent) -> Vec<AlarmChange> {
        let is_mute = matches!(event, AlarmEvent::Mute { .. });
        self.stored_try(store, |me, store| {
            let changes = me.engine.apply(event);
            Self::persist(store, &changes)?;
            if is_mute {
                store.set_muted_rules(&me.engine.muted_rules())?;
            }
            Ok(changes)
        })
    }

    /// Raises (or returns) what `observation` describes.
    pub fn observe(&mut self, store: &Store, observation: Observation, at: Timestamp) -> Vec<AlarmChange> {
        self.apply(store, observation.into_event(at))
    }

    /// An alert the alert evaluation just produced.
    pub fn alert(&mut self, store: &Store, alert: &Alert) -> Vec<AlarmChange> {
        self.observe(store, from_alert(alert), alert.at)
    }

    /// Whether [`AlarmCentre::tick`] at `now` has work (it does at most every 30 s).
    pub fn tick_due(&self, now: Timestamp) -> bool {
        now.since(self.last_tick) >= TICK_EVERY_MS
    }

    /// Time passing; does work at most every 30 s.
    pub fn tick(&mut self, store: &Store, now: Timestamp) -> Vec<AlarmChange> {
        if now.since(self.last_tick) < TICK_EVERY_MS {
            return Vec::new();
        }
        self.last_tick = now;
        self.stored_try(store, |me, store| {
            let changes = me.engine.tick(now);
            Self::persist(store, &changes)?;
            Ok(changes)
        })
    }

    /// Alarms matching `filter`, newest first.
    pub fn list(&self, filter: &AlarmFilter, now: Timestamp) -> Vec<Alarm> {
        filter.apply(self.engine.alarms(), now)
    }

    /// The statistics.
    pub fn stats(&self, now: Timestamp) -> AlarmStats {
        self.engine.stats(now)
    }

    /// Shelves an alarm.
    pub fn shelve(&mut self, store: &Store, id: u64, how: ShelveFor, at: Timestamp) -> Vec<AlarmChange> {
        self.apply(store, AlarmEvent::Shelve { id, how, at })
    }

    /// Whether the alarm exists.
    pub fn exists(&self, id: u64) -> bool {
        self.engine.alarm(id).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::alarms::AlarmEvent;
    use dyno_core::health::Severity;

    fn raise(rule: &str) -> AlarmEvent {
        AlarmEvent::Raise {
            rule: rule.into(),
            key: format!("{rule}:x"),
            severity: Severity::Warning,
            title: "t".into(),
            detail: String::new(),
            value: None,
            metric: None,
            ttl_ms: None,
            at: Timestamp(1),
        }
    }

    #[test]
    fn alarms_and_mutes_of_the_removed_backup_check_are_dropped_on_load() {
        let s = Store::memory().unwrap();
        let mut e = AlarmEngine::new(AlarmConfig::default(), dyno_core::time::FixedZone::shared(0));
        for c in [raise("alert:backup"), raise("alert:lowDisk")]
            .into_iter()
            .flat_map(|ev| e.apply(ev))
        {
            s.put_alarm(&c.alarm).unwrap();
        }
        s.set_muted_rules(&["alert:backup".into(), "alert:lowDisk".into()]).unwrap();
        let c = AlarmCentre::load(&s, AlarmConfig::default(), dyno_core::time::FixedZone::shared(0));
        assert_eq!(
            c.engine.alarms().iter().map(|a| a.rule.as_str()).collect::<Vec<_>>(),
            ["alert:lowDisk"]
        );
        assert_eq!(c.engine.muted_rules(), ["alert:lowDisk"]);
        assert_eq!(s.alarms(10).unwrap().len(), 1);
        assert_eq!(s.muted_rules().unwrap(), ["alert:lowDisk"]);
    }

    /// Fifth review P2-1 (rule 53): an alarm id is never given again, also after the newest alarm was pruned (its change log
    /// stays) and the alarms were read back, at a restart or after a change that was not stored.
    #[test]
    fn an_alarm_id_is_never_given_again_after_a_prune() {
        let s = Store::memory().unwrap();
        let mut c = AlarmCentre::load(&s, AlarmConfig::default(), dyno_core::time::FixedZone::shared(0));
        assert_eq!(c.apply(&s, raise("alert:a"))[0].alarm.id, 1);
        assert_eq!(c.apply(&s, raise("alert:b"))[0].alarm.id, 2);
        // A prune drops the alarm row and keeps its change log.
        s.delete_alarm(2).unwrap();
        c.reload(&s);
        assert_eq!(c.apply(&s, raise("alert:c"))[0].alarm.id, 3, "reload");
        s.delete_alarm(3).unwrap();
        let mut restarted = AlarmCentre::load(&s, AlarmConfig::default(), dyno_core::time::FixedZone::shared(0));
        assert_eq!(restarted.apply(&s, raise("alert:d"))[0].alarm.id, 4, "restart");
    }

    /// Fourth review: inside a batch, a change whose write fails is undone in the centre too, and the batch's other writes
    /// commit.
    #[test]
    fn inside_a_batch_a_change_whose_write_fails_is_undone_alone() {
        let s = Store::memory().unwrap();
        let mut c = AlarmCentre::load(&s, AlarmConfig::default(), dyno_core::time::FixedZone::shared(0));
        s.exec_for_test("CREATE TRIGGER no_alarm BEFORE INSERT ON alarms BEGIN SELECT RAISE(ABORT, 'refused'); END;");
        let b = s.batch();
        s.set_meta("other", "kept").unwrap();
        assert!(c.apply(&s, raise("alert:a")).is_empty(), "nothing to push");
        assert!(!c.exists(1), "the centre did not keep it");
        b.commit().unwrap();
        assert_eq!(s.meta("other").unwrap().as_deref(), Some("kept"));
        assert!(s.alarms(10).unwrap().is_empty());
    }

    /// Third review D1: a change whose write fails (the alarm row refused, the commit itself fine) is not kept, stored or pushed.
    #[test]
    fn a_change_whose_write_fails_is_not_kept_or_pushed() {
        let s = Store::memory().unwrap();
        let mut c = AlarmCentre::load(&s, AlarmConfig::default(), dyno_core::time::FixedZone::shared(0));
        s.exec_for_test("CREATE TRIGGER no_alarm BEFORE INSERT ON alarms BEGIN SELECT RAISE(ABORT, 'refused'); END;");
        let pushed = c.apply(&s, raise("alert:a")).len();
        assert_eq!((pushed, s.alarms(10).unwrap().len(), c.exists(1)), (0, 0, false));
        s.exec_for_test("DROP TRIGGER no_alarm;");
        assert_eq!(c.apply(&s, raise("alert:a")).len(), 1, "asking again stores it");
    }

    /// Second review: a request (ack, mute, shelve, raise) whose changes were not stored returns none to push and leaves the
    /// engine as it was, so asking again works.
    #[test]
    fn a_request_whose_changes_are_not_stored_changes_nothing() {
        let mut s = Store::memory().unwrap();
        let mut c = AlarmCentre::load(&s, AlarmConfig::default(), dyno_core::time::FixedZone::shared(0));
        let commits = s.count_commits();
        commits.fail(true);
        assert!(c.apply(&s, raise("alert:a")).is_empty(), "nothing to push");
        assert!(!c.exists(1), "the engine did not keep the alarm");
        commits.fail(false);
        assert_eq!(c.apply(&s, raise("alert:a")).len(), 1);
        assert_eq!(s.alarms(10).unwrap().len(), 1);
    }

    /// DY-7 (ADR 0108, guard D): a batch of alarm changes (each an alarm row and a change-log row) is one commit.
    #[test]
    fn an_alarm_batch_is_one_commit() {
        let mut s = Store::memory().unwrap();
        let mut e = AlarmEngine::new(AlarmConfig::default(), dyno_core::time::FixedZone::shared(0));
        let changes: Vec<AlarmChange> = ["alert:a", "alert:b", "alert:c"]
            .into_iter()
            .flat_map(|r| e.apply(raise(r)))
            .collect();
        assert_eq!(changes.len(), 3);
        let mut centre = AlarmCentre::load(&s, AlarmConfig::default(), dyno_core::time::FixedZone::shared(0));
        let commits = s.count_commits();
        let kept = centre.stored(&s, |_, store| {
            AlarmCentre::persist(store, &changes).unwrap();
            changes.clone()
        });
        assert_eq!((commits.take(), kept.len()), (1, 3));
        assert_eq!(s.alarms(10).unwrap().len(), 3);
        assert_eq!(s.alarm_events(None, 10).unwrap().len(), 3);
    }
}
