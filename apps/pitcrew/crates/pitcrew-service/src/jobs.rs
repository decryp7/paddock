//! The service's own long jobs (runner downloads) as `paddock-jobs` jobs (rules 10-12, ADR 0008): the registry reports every
//! snapshot to the supervisor, which pushes it to the app as `ServiceEvent::Job`; the app shows it in the shell's Jobs list and
//! asks the service to cancel with `Request::CancelJob`. Nothing here times a job out.

use crate::supervisor::Msg;
use paddock_jobs::{EventSink, JobId, JobRegistry, JobSnapshot};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// Where the registry's snapshots go: the supervisor's inbox, once it exists.
#[derive(Default)]
struct HubSink {
    inbox: Mutex<Option<Sender<Msg>>>,
}

impl EventSink for HubSink {
    fn emit(&self, snapshot: &JobSnapshot) {
        let inbox = self.inbox.lock().unwrap_or_else(|p| p.into_inner());
        // The supervisor is gone (the service is stopping): the snapshot has nobody to tell.
        if let Some(tx) = inbox.as_ref() {
            let _ = tx.send(Msg::Job(Box::new(snapshot.clone())));
        }
    }
}

/// The service's jobs: the registry downloads register on, and the way its snapshots reach the supervisor.
#[derive(Clone)]
pub struct JobHub {
    sink: Arc<HubSink>,
    registry: Arc<JobRegistry>,
}

impl JobHub {
    /// A hub that reports nowhere until [`attach`](Self::attach).
    pub fn new() -> Self {
        let sink = Arc::new(HubSink::default());
        let registry = Arc::new(JobRegistry::new(sink.clone()));
        JobHub { sink, registry }
    }

    /// Sends every snapshot to the supervisor's inbox from now on.
    pub fn attach(&self, inbox: Sender<Msg>) {
        *self.sink.inbox.lock().unwrap_or_else(|p| p.into_inner()) = Some(inbox);
    }

    /// The registry to register jobs on.
    pub fn registry(&self) -> &Arc<JobRegistry> {
        &self.registry
    }

    /// Asks job `id` to stop; false when it is unknown or already ended.
    pub fn cancel(&self, id: u64) -> bool {
        self.registry.cancel(JobId(id))
    }
}

impl Default for JobHub {
    fn default() -> Self {
        Self::new()
    }
}
