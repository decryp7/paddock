//! The priority job queue (ADR 0005): every request and batch item is a job with a priority (interactive >
//! background > batch). Jobs start when a slot is free, highest effective priority first and in arrival order
//! within a priority, within the global, per-model and per-app caps. Waiting jobs age: every `aging` they wait
//! raises them one level, so batch work never starves. Pure: time is an argument; the server owns the waiting.

use crate::ids::{AppId, ModelId};
use crate::time::Millis;
use serde::{Deserialize, Serialize};

/// How urgent a job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Priority {
    /// Someone is waiting for it (chat, completion as you type). The default.
    #[default]
    Interactive,
    /// The app works on it while the user does something else.
    Background,
    /// Bulk work (batch jobs, indexing).
    Batch,
}

impl Priority {
    /// Parses `interactive`, `background`, `batch` (the `X-Pitwall-Priority` header).
    pub fn parse(s: &str) -> Option<Priority> {
        match s.trim().to_ascii_lowercase().as_str() {
            "interactive" => Some(Priority::Interactive),
            "background" => Some(Priority::Background),
            "batch" => Some(Priority::Batch),
            _ => None,
        }
    }

    /// The more urgent of two.
    fn raise(self, levels: u64) -> Priority {
        match (self, levels) {
            (p, 0) => p,
            (Priority::Batch, 1) => Priority::Background,
            _ => Priority::Interactive,
        }
    }

    /// The level allowed for an app: the requested one, but never more urgent than `max`.
    pub fn capped(self, max: Priority) -> Priority {
        self.max(max)
    }
}

/// What a job does (for the UI and the caps).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    /// A chat request.
    Chat,
    /// A plain completion (Ollama generate).
    Completion,
    /// Embeddings.
    Embedding,
    /// Speech to text.
    Transcription,
    /// A task endpoint (`/x/summarize`, …).
    Task,
    /// An item of a batch job.
    Batch,
    /// A benchmark run.
    Benchmark,
}

/// A job's id, unique for the life of the server.
pub type JobId = u64;

/// What the caller submits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpec {
    /// Who asked.
    pub app: AppId,
    /// Which model, when known before it starts.
    pub model: Option<ModelId>,
    /// How urgent.
    pub priority: Priority,
    /// What it is.
    pub kind: JobKind,
    /// The app's own cap on running jobs.
    pub app_cap: Option<usize>,
}

/// The queue's limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// Jobs running at once over all models.
    pub max_running: usize,
    /// Jobs running at once on one model (llama-server's parallel slots).
    pub per_model: usize,
    /// Jobs waiting at once; more are refused.
    pub max_waiting: usize,
    /// How long a waiting job takes to rise one level.
    pub aging: Millis,
}

impl Default for Caps {
    fn default() -> Self {
        Caps {
            max_running: 4,
            per_model: 2,
            max_waiting: 256,
            aging: 30_000,
        }
    }
}

/// A job's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    /// Waiting for a slot.
    Queued,
    /// Running.
    Running,
}

/// A job as the queue knows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    /// Its id.
    pub id: JobId,
    /// What was submitted.
    pub spec: JobSpec,
    /// Its state.
    pub state: JobState,
    /// When it was submitted.
    pub submitted_at: Millis,
    /// When it started.
    pub started_at: Option<Millis>,
    /// 0–1, when known.
    pub progress: Option<f32>,
    /// Tokens generated so far.
    pub tokens: u64,
}

/// Why a job was not queued.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QueueError {
    /// Too many jobs wait already.
    #[error("Pitwall is busy: {0} requests are waiting. Try again shortly or use a lower priority.")]
    Full(usize),
    /// Serving is paused by the user.
    #[error("Pitwall has paused serving. Resume it in Pitwall's window or tray menu.")]
    Paused,
}

/// What [`Scheduler::cancel`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cancelled {
    /// It was waiting and is gone.
    Queued,
    /// It was running: the caller stops it (the scheduler has freed its slot).
    Running,
    /// No such job (it ended already).
    Unknown,
}

/// The queue.
#[derive(Debug, Clone, Default)]
pub struct Scheduler {
    caps: Caps,
    next: JobId,
    jobs: Vec<Job>,
    paused: bool,
    /// Slots of the models that have their own (loaded with their own count); others use `caps.per_model`.
    slots: std::collections::HashMap<ModelId, usize>,
}

/// Waiting jobs per priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Depth {
    /// Interactive.
    pub interactive: usize,
    /// Background.
    pub background: usize,
    /// Batch.
    pub batch: usize,
}

impl Scheduler {
    /// An empty queue.
    pub fn new(caps: Caps) -> Scheduler {
        Scheduler {
            caps,
            next: 1,
            jobs: vec![],
            paused: false,
            slots: std::collections::HashMap::new(),
        }
    }

    /// Sets how many jobs one model runs at once (its parallel slots); requests beyond that wait in the queue.
    /// Running jobs are not affected. A count of 0 is read as 1.
    pub fn set_model_slots(&mut self, model: &ModelId, slots: usize) {
        self.slots.insert(model.clone(), slots.max(1));
    }

    /// The jobs one model runs at once.
    pub fn slots_of(&self, model: &ModelId) -> usize {
        self.slots.get(model).copied().unwrap_or(self.caps.per_model)
    }

    /// Changes the limits; running jobs are not affected.
    pub fn set_caps(&mut self, caps: Caps) {
        self.caps = caps;
    }

    /// Pauses or resumes starting jobs; while paused new jobs are refused.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Whether serving is paused.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Queues a job.
    pub fn submit(&mut self, spec: JobSpec, now: Millis) -> Result<JobId, QueueError> {
        if self.paused {
            return Err(QueueError::Paused);
        }
        let waiting = self.waiting().count();
        if waiting >= self.caps.max_waiting {
            return Err(QueueError::Full(waiting));
        }
        let id = self.next;
        self.next += 1;
        self.jobs.push(Job {
            id,
            spec,
            state: JobState::Queued,
            submitted_at: now,
            started_at: None,
            progress: None,
            tokens: 0,
        });
        Ok(id)
    }

    fn effective(&self, j: &Job, now: Millis) -> Priority {
        let waited = now.saturating_sub(j.submitted_at);
        j.spec.priority.raise(waited.checked_div(self.caps.aging).unwrap_or(0))
    }

    /// Starts every job that can start now and returns their ids (in start order).
    pub fn dispatch(&mut self, now: Millis) -> Vec<JobId> {
        let mut started = vec![];
        if self.paused {
            return started;
        }
        loop {
            let running: Vec<&Job> = self.running().collect();
            if running.len() >= self.caps.max_running {
                break;
            }
            let pick = self
                .waiting()
                .filter(|j| {
                    let model_ok = j
                        .spec
                        .model
                        .as_ref()
                        .is_none_or(|m| running.iter().filter(|r| r.spec.model.as_ref() == Some(m)).count() < self.slots_of(m));
                    let app_ok = j
                        .spec
                        .app_cap
                        .is_none_or(|cap| running.iter().filter(|r| r.spec.app == j.spec.app).count() < cap);
                    model_ok && app_ok
                })
                .min_by_key(|j| (self.effective(j, now), j.submitted_at, j.id))
                .map(|j| j.id);
            let Some(id) = pick else { break };
            if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                j.state = JobState::Running;
                j.started_at = Some(now);
            }
            started.push(id);
        }
        started
    }

    /// Sets the model of a job once it is known (capability selection happens before dispatch).
    pub fn set_model(&mut self, id: JobId, model: ModelId) {
        if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
            j.spec.model = Some(model);
        }
    }

    /// Records progress of a running job.
    pub fn progress(&mut self, id: JobId, progress: Option<f32>, tokens: u64) {
        if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
            j.progress = progress.map(|p| p.clamp(0.0, 1.0));
            j.tokens = tokens;
        }
    }

    /// Removes a job that ended (done or failed). Returns whether it was known.
    pub fn finish(&mut self, id: JobId) -> bool {
        let before = self.jobs.len();
        self.jobs.retain(|j| j.id != id);
        before != self.jobs.len()
    }

    /// Cancels a job.
    pub fn cancel(&mut self, id: JobId) -> Cancelled {
        match self.jobs.iter().position(|j| j.id == id) {
            Some(i) => {
                let j = self.jobs.remove(i);
                match j.state {
                    JobState::Queued => Cancelled::Queued,
                    JobState::Running => Cancelled::Running,
                }
            }
            None => Cancelled::Unknown,
        }
    }

    /// A job by id.
    pub fn job(&self, id: JobId) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    /// Running jobs.
    pub fn running(&self) -> impl Iterator<Item = &Job> {
        self.jobs.iter().filter(|j| j.state == JobState::Running)
    }

    /// Waiting jobs, in arrival order.
    pub fn waiting(&self) -> impl Iterator<Item = &Job> {
        self.jobs.iter().filter(|j| j.state == JobState::Queued)
    }

    /// Waiting jobs per (submitted) priority.
    pub fn depth(&self) -> Depth {
        let mut d = Depth::default();
        for j in self.waiting() {
            match j.spec.priority {
                Priority::Interactive => d.interactive += 1,
                Priority::Background => d.background += 1,
                Priority::Batch => d.batch += 1,
            }
        }
        d
    }

    /// Whether a model has running jobs (memory release never touches it then).
    pub fn model_busy(&self, model: &ModelId) -> bool {
        self.running().any(|j| j.spec.model.as_ref() == Some(model))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(app: &str, model: Option<&str>, p: Priority) -> JobSpec {
        JobSpec {
            app: AppId::parse(app).unwrap(),
            model: model.map(|m| ModelId::parse(m).unwrap()),
            priority: p,
            kind: JobKind::Chat,
            app_cap: None,
        }
    }

    #[test]
    fn priorities_parse_order_and_cap() {
        assert_eq!(Priority::parse(" Batch "), Some(Priority::Batch));
        assert_eq!(Priority::parse("interactive"), Some(Priority::Interactive));
        assert_eq!(Priority::parse("background"), Some(Priority::Background));
        assert_eq!(Priority::parse("urgent"), None);
        assert!(Priority::Interactive < Priority::Batch);
        assert_eq!(Priority::Interactive.capped(Priority::Background), Priority::Background);
        assert_eq!(Priority::Batch.capped(Priority::Interactive), Priority::Batch);
        assert_eq!(Priority::Batch.raise(1), Priority::Background);
        assert_eq!(Priority::Batch.raise(5), Priority::Interactive);
        assert_eq!(Priority::Background.raise(1), Priority::Interactive);
    }

    #[test]
    fn interactive_goes_first_then_fifo() {
        let mut s = Scheduler::new(Caps {
            max_running: 1,
            ..Caps::default()
        });
        let b = s.submit(spec("a", None, Priority::Batch), 0).unwrap();
        let bg = s.submit(spec("a", None, Priority::Background), 1).unwrap();
        let i1 = s.submit(spec("a", None, Priority::Interactive), 2).unwrap();
        let i2 = s.submit(spec("a", None, Priority::Interactive), 3).unwrap();
        assert_eq!(
            s.depth(),
            Depth {
                interactive: 2,
                background: 1,
                batch: 1
            }
        );
        assert_eq!(s.dispatch(10), vec![i1]);
        assert_eq!(s.dispatch(10), Vec::<JobId>::new(), "one slot");
        assert!(s.finish(i1));
        assert!(!s.finish(i1));
        assert_eq!(s.dispatch(11), vec![i2]);
        s.finish(i2);
        assert_eq!(s.dispatch(12), vec![bg]);
        s.finish(bg);
        assert_eq!(s.dispatch(13), vec![b]);
        assert_eq!(s.job(b).unwrap().started_at, Some(13));
    }

    #[test]
    fn waiting_jobs_age_so_batch_never_starves() {
        let mut s = Scheduler::new(Caps {
            max_running: 1,
            aging: 100,
            ..Caps::default()
        });
        let old = s.submit(spec("a", None, Priority::Batch), 0).unwrap();
        let fresh = s.submit(spec("a", None, Priority::Interactive), 250).unwrap();
        // After 250 ms the batch job has risen two levels and is older: it goes first.
        assert_eq!(s.dispatch(250), vec![old]);
        s.finish(old);
        assert_eq!(s.dispatch(251), vec![fresh]);
        let z = Scheduler::new(Caps {
            aging: 0,
            ..Caps::default()
        });
        let j = Job {
            id: 1,
            spec: spec("a", None, Priority::Batch),
            state: JobState::Queued,
            submitted_at: 0,
            started_at: None,
            progress: None,
            tokens: 0,
        };
        assert_eq!(z.effective(&j, 1_000_000), Priority::Batch, "aging 0 disables aging");
    }

    #[test]
    fn requests_beyond_the_models_slots_queue() {
        let mut s = Scheduler::new(Caps {
            max_running: 10,
            per_model: 1,
            ..Caps::default()
        });
        let m = ModelId::parse("m").unwrap();
        assert_eq!(s.slots_of(&m), 1, "the default cap");
        s.set_model_slots(&m, 3);
        let ids: Vec<JobId> = (0..5)
            .map(|i| s.submit(spec("a", Some("m"), Priority::Interactive), i).unwrap())
            .collect();
        let other = s.submit(spec("a", Some("n"), Priority::Interactive), 9).unwrap();
        assert_eq!(
            s.dispatch(10),
            vec![ids[0], ids[1], ids[2], other],
            "three slots, the rest queue; others use the default"
        );
        s.finish(ids[0]);
        assert_eq!(s.dispatch(11), vec![ids[3]]);
        s.set_model_slots(&m, 0);
        assert_eq!(s.slots_of(&m), 1, "zero is read as one");
        s.finish(ids[1]);
        assert!(s.dispatch(12).is_empty(), "two still running, above the new count");
    }

    #[test]
    fn caps_per_model_and_per_app() {
        let mut s = Scheduler::new(Caps {
            max_running: 10,
            per_model: 1,
            ..Caps::default()
        });
        let a1 = s.submit(spec("a", Some("m"), Priority::Interactive), 0).unwrap();
        let a2 = s.submit(spec("a", Some("m"), Priority::Interactive), 1).unwrap();
        let other = s.submit(spec("a", Some("n"), Priority::Batch), 2).unwrap();
        assert_eq!(s.dispatch(3), vec![a1, other]);
        assert!(s.model_busy(&ModelId::parse("m").unwrap()));
        s.finish(a1);
        assert_eq!(s.dispatch(4), vec![a2]);

        let mut s = Scheduler::new(Caps {
            max_running: 10,
            ..Caps::default()
        });
        let capped = |app: &str| JobSpec {
            app_cap: Some(1),
            ..spec(app, None, Priority::Interactive)
        };
        let x1 = s.submit(capped("x"), 0).unwrap();
        let _x2 = s.submit(capped("x"), 1).unwrap();
        let y1 = s.submit(capped("y"), 2).unwrap();
        assert_eq!(s.dispatch(3), vec![x1, y1]);
    }

    #[test]
    fn full_paused_cancel_and_progress() {
        let mut s = Scheduler::new(Caps {
            max_running: 1,
            max_waiting: 1,
            ..Caps::default()
        });
        let a = s.submit(spec("a", None, Priority::Interactive), 0).unwrap();
        let e = s.submit(spec("a", None, Priority::Interactive), 0).unwrap_err();
        assert_eq!(e, QueueError::Full(1));
        assert!(e.to_string().contains("busy"));
        s.dispatch(0);
        let b = s.submit(spec("a", None, Priority::Interactive), 1).unwrap();
        s.progress(a, Some(1.7), 12);
        assert_eq!(s.job(a).unwrap().progress, Some(1.0));
        assert_eq!(s.job(a).unwrap().tokens, 12);
        s.progress(999, None, 0);
        s.set_model(b, ModelId::parse("m").unwrap());
        s.set_model(999, ModelId::parse("m").unwrap());
        assert_eq!(s.job(b).unwrap().spec.model.as_ref().unwrap().as_str(), "m");
        assert_eq!(s.cancel(b), Cancelled::Queued);
        assert_eq!(s.cancel(a), Cancelled::Running);
        assert_eq!(s.cancel(a), Cancelled::Unknown);
        s.set_paused(true);
        assert!(s.paused());
        assert_eq!(s.submit(spec("a", None, Priority::Interactive), 2), Err(QueueError::Paused));
        assert!(QueueError::Paused.to_string().contains("paused"));
        s.set_paused(false);
        let c = s.submit(spec("a", None, Priority::Interactive), 3).unwrap();
        s.set_paused(true);
        assert!(s.dispatch(3).is_empty(), "paused: nothing starts");
        s.set_paused(false);
        s.set_caps(Caps {
            max_running: 0,
            ..Caps::default()
        });
        assert!(s.dispatch(3).is_empty());
        s.set_caps(Caps::default());
        assert_eq!(s.dispatch(3), vec![c]);
    }
}
