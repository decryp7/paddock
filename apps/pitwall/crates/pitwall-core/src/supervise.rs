//! Supervising loaded models (ADR 0004): when an engine process dies the model is restarted after an exponential
//! backoff, and a model that keeps crashing is marked failed (the crash-loop guard) so requests get a clear answer
//! instead of another doomed start. Pure: the server passes crash times in and carries the decisions out.

use crate::backoff::{Backoff, Decision, Limits};
use crate::ids::ModelId;
use crate::mgmt::{EngineIssue, IssueState};
use crate::time::Millis;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    /// Running (or not yet crashed).
    Ok,
    /// A restart is planned for this time.
    Waiting(Millis),
    /// A restart attempt is under way.
    Starting,
    /// Too many crashes: no more automatic attempts until the user retries.
    Failed,
}

#[derive(Debug, Clone)]
struct Entry {
    backoff: Backoff,
    state: State,
    crashes: u32,
}

/// What the supervisor wants done about a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Restart after this many milliseconds.
    RestartIn(Millis),
    /// Crash loop: stop restarting and report.
    GaveUp,
    /// Recorded; nothing is restarted automatically.
    Recorded,
}

/// Crash history and pending restarts of all models.
#[derive(Debug, Clone, Default)]
pub struct Supervisor {
    limits: Limits,
    entries: HashMap<ModelId, Entry>,
}

impl Supervisor {
    /// A supervisor with the given restart limits.
    pub fn new(limits: Limits) -> Supervisor {
        Supervisor {
            limits,
            entries: HashMap::new(),
        }
    }

    fn entry(&mut self, id: &ModelId) -> &mut Entry {
        let limits = self.limits;
        self.entries.entry(id.clone()).or_insert_with(|| Entry {
            backoff: Backoff::new(limits),
            state: State::Ok,
            crashes: 0,
        })
    }

    /// The model's engine is alive at `now` (long enough healthy forgets old crashes' delay).
    pub fn healthy(&mut self, id: &ModelId, now: Millis) {
        if let Some(e) = self.entries.get_mut(id) {
            if e.state != State::Failed {
                e.backoff.healthy(now);
            }
        }
    }

    /// The model's engine crashed, or a start attempt failed, at `now`. With `auto_restart` a restart is planned
    /// (an engine that died while loaded); without it the crash only counts toward the loop guard (a start that
    /// failed while serving a request, which already reports its own error).
    pub fn crashed(&mut self, id: &ModelId, now: Millis, auto_restart: bool) -> Outcome {
        let e = self.entry(id);
        e.crashes = e.crashes.saturating_add(1);
        match e.backoff.crashed(now) {
            Decision::GiveUp => {
                e.state = State::Failed;
                Outcome::GaveUp
            }
            Decision::RestartIn(d) if auto_restart => {
                e.state = State::Waiting(now.saturating_add(d));
                Outcome::RestartIn(d)
            }
            Decision::RestartIn(_) => {
                e.state = State::Ok;
                Outcome::Recorded
            }
        }
    }

    /// Models whose planned restart time has come; they move to "starting" so each is returned once.
    pub fn due(&mut self, now: Millis) -> Vec<ModelId> {
        let mut out: Vec<ModelId> = self
            .entries
            .iter_mut()
            .filter_map(|(id, e)| match e.state {
                State::Waiting(at) if at <= now => {
                    e.state = State::Starting;
                    Some(id.clone())
                }
                _ => None,
            })
            .collect();
        out.sort();
        out
    }

    /// The model is loaded and serving again (a restart worked, or a request loaded it).
    pub fn started(&mut self, id: &ModelId) {
        if let Some(e) = self.entries.get_mut(id) {
            if e.state != State::Failed {
                e.state = State::Ok;
            }
        }
    }

    /// Why requests for this model are refused (the loop guard fired), if they are.
    pub fn failure(&self, id: &ModelId) -> Option<String> {
        let e = self.entries.get(id).filter(|e| e.state == State::Failed)?;
        Some(format!(
            "the engine for {id} crashed repeatedly ({} times in ten minutes) and Pitwall stopped restarting it. \
             Check that the model file is complete and the engine build suits this computer (Engines), then press Retry on the Dashboard.",
            e.backoff.recent_crashes().max(e.crashes as usize)
        ))
    }

    /// Forget everything about a model (the user pressed Retry, or deleted it).
    pub fn clear(&mut self, id: &ModelId) {
        self.entries.remove(id);
    }

    /// What the dashboard shows: models waiting to restart and models given up on.
    pub fn issues(&self) -> Vec<EngineIssue> {
        let mut v: Vec<EngineIssue> = self
            .entries
            .iter()
            .filter_map(|(id, e)| match e.state {
                State::Waiting(at) => Some(EngineIssue {
                    model_id: id.to_string(),
                    state: IssueState::Restarting,
                    crashes: e.crashes,
                    restart_at: Some(at),
                    message: format!("The engine for {id} stopped unexpectedly; Pitwall is restarting it."),
                }),
                State::Starting => Some(EngineIssue {
                    model_id: id.to_string(),
                    state: IssueState::Restarting,
                    crashes: e.crashes,
                    restart_at: None,
                    message: format!("Restarting the engine for {id}."),
                }),
                State::Failed => Some(EngineIssue {
                    model_id: id.to_string(),
                    state: IssueState::Failed,
                    crashes: e.crashes,
                    restart_at: None,
                    message: self.failure(id).unwrap_or_default(),
                }),
                State::Ok => None,
            })
            .collect();
        v.sort_by(|a, b| a.model_id.cmp(&b.model_id));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{MINUTE, SECOND};

    fn id(s: &str) -> ModelId {
        ModelId::parse(s).unwrap()
    }

    #[test]
    fn a_crash_plans_a_restart_that_becomes_due_once() {
        let mut s = Supervisor::new(Limits::default());
        assert_eq!(s.crashed(&id("m"), 1000, true), Outcome::RestartIn(SECOND));
        assert!(s.due(1500).is_empty());
        assert_eq!(s.issues()[0].state, IssueState::Restarting);
        assert_eq!(s.issues()[0].restart_at, Some(2000));
        assert_eq!(s.due(2000), vec![id("m")]);
        assert!(s.due(9000).is_empty(), "returned once");
        assert_eq!(s.issues()[0].restart_at, None, "starting");
        s.started(&id("m"));
        assert!(s.issues().is_empty());
    }

    #[test]
    fn delays_double_and_a_crash_loop_is_given_up_with_a_reason() {
        let mut s = Supervisor::new(Limits::default());
        let m = id("m");
        assert_eq!(s.crashed(&m, 0, true), Outcome::RestartIn(SECOND));
        assert_eq!(s.crashed(&m, 10, true), Outcome::RestartIn(2 * SECOND));
        for t in 20..23 {
            assert!(matches!(s.crashed(&m, t, true), Outcome::RestartIn(_)));
        }
        assert_eq!(s.crashed(&m, 30, true), Outcome::GaveUp);
        let why = s.failure(&m).unwrap();
        assert!(why.contains("stopped restarting") && why.contains("Retry"), "{why}");
        assert!(s.due(10 * MINUTE).is_empty(), "no more attempts");
        let issue = &s.issues()[0];
        assert_eq!(issue.state, IssueState::Failed);
        assert_eq!(issue.crashes, 6);
        // started/healthy do not resurrect a failed model; only clear does.
        s.started(&m);
        s.healthy(&m, 99 * MINUTE);
        assert!(s.failure(&m).is_some());
        s.clear(&m);
        assert!(s.failure(&m).is_none());
        assert!(s.issues().is_empty());
    }

    #[test]
    fn a_failed_start_without_auto_restart_only_counts_toward_the_guard() {
        let mut s = Supervisor::new(Limits::default());
        let m = id("m");
        for t in 0..5 {
            assert_eq!(s.crashed(&m, t, false), Outcome::Recorded);
        }
        assert!(s.issues().is_empty());
        assert!(s.due(MINUTE).is_empty());
        assert_eq!(s.crashed(&m, 6, false), Outcome::GaveUp);
        assert!(s.failure(&m).is_some());
    }

    #[test]
    fn models_are_independent_and_healthy_time_resets_the_delay() {
        let mut s = Supervisor::new(Limits::default());
        s.crashed(&id("a"), 0, true);
        s.crashed(&id("b"), 0, true);
        assert_eq!(s.due(SECOND), vec![id("a"), id("b")]);
        s.started(&id("a"));
        s.healthy(&id("a"), 10);
        s.healthy(&id("a"), 10 + 5 * MINUTE);
        assert_eq!(s.crashed(&id("a"), 11 + 5 * MINUTE, true), Outcome::RestartIn(SECOND));
        s.healthy(&id("unknown"), 5);
        s.started(&id("unknown"));
    }
}
