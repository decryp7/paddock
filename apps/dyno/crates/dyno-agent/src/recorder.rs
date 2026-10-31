//! The flight recorder: the last ten minutes at one-second resolution in a hard-bounded ring ([`Ring`]), saved as a
//! snapshot on request or when a serious alarm fires. The ring is fed from the sampler's existing system sample (no
//! extra OS calls); the only extra cost is asking the sampler to tick the system sample at one second while Dyno is
//! well inside its CPU budget (`dyno_core::recorder::system_interval_ms`).

use crate::store::Store;
use dyno_core::alarms::{AlarmChange, ChangeKind};
use dyno_core::grouping::AppGroup;
use dyno_core::health::Severity;
use dyno_core::metrics::SystemSample;
use dyno_core::recorder::{auto_capture_allowed, CaptureReason, RecorderSample, RecorderSnapshot, RecorderSnapshotInfo, Ring};
use dyno_core::time::Timestamp;

/// The recorder's state.
pub struct Recorder {
    ring: Ring,
    last_auto: Option<Timestamp>,
}

impl Default for Recorder {
    fn default() -> Self {
        Recorder::new()
    }
}

impl Recorder {
    /// An empty recorder.
    pub fn new() -> Recorder {
        Recorder {
            ring: Ring::new(),
            last_auto: None,
        }
    }

    /// Feeds one system sample with the latest app groups.
    pub fn record(&mut self, sample: &SystemSample, groups: &[AppGroup]) {
        self.ring.push(RecorderSample::from_system(sample, groups));
    }

    /// Estimated memory held, bytes.
    pub fn bytes(&self) -> usize {
        self.ring.bytes()
    }

    /// Samples held.
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// Whether nothing was recorded yet.
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// The ring as a snapshot document without storing it; `None` while nothing was recorded.
    pub fn snapshot(&self, reason: CaptureReason, now: Timestamp) -> Option<RecorderSnapshot> {
        (!self.ring.is_empty()).then(|| RecorderSnapshot::new(now, reason, self.ring.samples()))
    }

    /// Saves the ring as a snapshot.
    pub fn capture(&mut self, store: &Store, reason: CaptureReason, now: Timestamp) -> Result<RecorderSnapshotInfo, String> {
        if self.ring.is_empty() {
            return Err("Dyno has not recorded anything yet; try again in a few seconds.".into());
        }
        store
            .put_recorder_snapshot(RecorderSnapshot::new(now, reason, self.ring.samples()))
            .map_err(|e| format!("The capture could not be saved: {e}"))
    }

    /// Captures automatically for a freshly raised warning or critical alarm, at most every two minutes.
    pub fn on_alarm(&mut self, store: &Store, change: &AlarmChange, now: Timestamp) -> Option<RecorderSnapshotInfo> {
        let serious = change.kind == ChangeKind::Raised && change.alarm.severity >= Severity::Warning && !change.alarm.muted;
        if !serious || !auto_capture_allowed(self.last_auto, now) {
            return None;
        }
        self.last_auto = Some(now);
        let reason = CaptureReason::Alarm {
            alarm_id: change.alarm.id,
            title: change.alarm.title.chars().take(100).collect(),
        };
        self.capture(store, reason, now).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::alarms::{AlarmEngine, AlarmEvent};
    use dyno_platform::fake::quiet_sample as sample;

    fn change(sev: Severity, key: &str) -> AlarmChange {
        AlarmEngine::new(dyno_core::alarms::AlarmConfig::default(), dyno_core::time::FixedZone::shared(0))
            .apply(AlarmEvent::Raise {
                rule: "r".into(),
                key: key.into(),
                severity: sev,
                title: "t".repeat(300),
                detail: String::new(),
                value: None,
                metric: None,
                ttl_ms: None,
                at: Timestamp(1),
            })
            .remove(0)
    }

    #[test]
    fn capture_needs_samples_and_saves_them() {
        let store = Store::memory().unwrap();
        let mut r = Recorder::new();
        assert!(r.is_empty());
        assert!(r
            .capture(&store, CaptureReason::Manual, Timestamp(0))
            .unwrap_err()
            .contains("not recorded"));
        for i in 0..5 {
            r.record(&sample(Timestamp(i * 1_000)), &[]);
        }
        assert_eq!((r.len(), r.bytes() > 0), (5, true));
        let info = r.capture(&store, CaptureReason::Manual, Timestamp(9)).unwrap();
        assert_eq!(info.samples, 5);
        assert_eq!(store.recorder_snapshot(info.id).unwrap().unwrap().samples.len(), 5);
    }

    #[test]
    fn serious_alarms_capture_but_not_too_often() {
        let store = Store::memory().unwrap();
        let mut r = Recorder::new();
        r.record(&sample(Timestamp(0)), &[]);
        assert!(
            r.on_alarm(&store, &change(Severity::Info, "a"), Timestamp(10)).is_none(),
            "info alarms do not capture"
        );
        let first = r.on_alarm(&store, &change(Severity::Warning, "b"), Timestamp(10)).unwrap();
        let CaptureReason::Alarm { title, .. } = first.reason else {
            panic!("alarm")
        };
        assert_eq!(title.chars().count(), 100);
        assert!(
            r.on_alarm(&store, &change(Severity::Critical, "c"), Timestamp(20)).is_none(),
            "rate limited"
        );
        assert!(r
            .on_alarm(&store, &change(Severity::Critical, "c"), Timestamp(10 + 120_000))
            .is_some());
        let mut muted = change(Severity::Critical, "d");
        muted.alarm.muted = true;
        assert!(r.on_alarm(&store, &muted, Timestamp(1_000_000)).is_none());
        let mut repeated = change(Severity::Critical, "e");
        repeated.kind = ChangeKind::Repeated;
        assert!(r.on_alarm(&store, &repeated, Timestamp(1_000_000)).is_none());
    }
}
