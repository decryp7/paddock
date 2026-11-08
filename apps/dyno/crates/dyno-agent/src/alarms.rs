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

/// The alarm engine with its store.
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
        AlarmCentre {
            engine: AlarmEngine::restore(config, zone, alarms, muted),
            last_tick: Timestamp(0),
        }
    }

    /// Applies the user's settings.
    pub fn set_config(&mut self, config: AlarmConfig) {
        self.engine.set_config(config);
    }

    fn persist(store: &Store, changes: &[AlarmChange]) {
        for c in changes {
            let r = if c.kind == ChangeKind::Pruned {
                store.delete_alarm(c.alarm.id)
            } else {
                store.put_alarm(&c.alarm)
            };
            if let Err(e) = r.and_then(|()| store.put_alarm_event(c)) {
                log::error!("dyno-agent: could not store an alarm change: {e}");
            }
        }
    }

    /// Applies an event and stores its changes; the caller pushes them.
    pub fn apply(&mut self, store: &Store, event: AlarmEvent) -> Vec<AlarmChange> {
        let is_mute = matches!(event, AlarmEvent::Mute { .. });
        let changes = self.engine.apply(event);
        Self::persist(store, &changes);
        if is_mute {
            if let Err(e) = store.set_muted_rules(&self.engine.muted_rules()) {
                log::error!("dyno-agent: could not store the muted rules: {e}");
            }
        }
        changes
    }

    /// Raises (or returns) what `observation` describes.
    pub fn observe(&mut self, store: &Store, observation: Observation, at: Timestamp) -> Vec<AlarmChange> {
        self.apply(store, observation.into_event(at))
    }

    /// An alert the alert evaluation just produced.
    pub fn alert(&mut self, store: &Store, alert: &Alert) -> Vec<AlarmChange> {
        self.observe(store, from_alert(alert), alert.at)
    }

    /// Time passing; does work at most every 30 s.
    pub fn tick(&mut self, store: &Store, now: Timestamp) -> Vec<AlarmChange> {
        if now.since(self.last_tick) < TICK_EVERY_MS {
            return Vec::new();
        }
        self.last_tick = now;
        let changes = self.engine.tick(now);
        Self::persist(store, &changes);
        changes
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
}
