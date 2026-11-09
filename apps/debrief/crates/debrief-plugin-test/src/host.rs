//! The fake host: answers what a plugin calls back while it works (reading the files the harness lends it, an
//! annotator's view of records, tool task reports), as Debrief's host does.

use crate::view;
use debrief_ipc::peer::{code, decode_request, encode_reply, CallCtx, Handler};
use debrief_ipc::RemoteError;
use debrief_plugin_api::protocol::{HostRequest, WireViewRecord, READ_CHUNK};
use debrief_plugin_api::tools::ToolTask;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

/// A tool task report the plugin sent.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskReport {
    /// A task started.
    Started {
        /// The tool.
        tool: String,
        /// The task.
        task: ToolTask,
    },
    /// A task made progress.
    Progress {
        /// The tool.
        tool: String,
        /// The task id.
        task: String,
        /// How far it is (0–1), when known.
        fraction: Option<f32>,
    },
    /// A task ended.
    Finished {
        /// The tool.
        tool: String,
        /// The task id.
        task: String,
    },
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// What the harness lends the plugin, by id.
#[derive(Default)]
pub(crate) struct FakeHost {
    files: Mutex<HashMap<u64, Arc<Vec<u8>>>>,
    views: Mutex<HashMap<u64, Arc<Vec<WireViewRecord>>>>,
    next: AtomicU64,
    pub(crate) tasks: Mutex<Vec<TaskReport>>,
    /// Bytes the plugin read through `ReadFile`, of all files (what the stall fault hook counts).
    bytes_read: AtomicU64,
    /// Bytes the plugin read through `ReadFile`, by lent file: whatever it read before a check began waiting counts.
    read_of: Mutex<HashMap<u64, u64>>,
    /// Signalled after every read, so a waiter wakes on the bytes arriving instead of polling a clock.
    read_signal: (Mutex<()>, Condvar),
    /// Fault hook (see `Launch::stall_reads_after`): reads past this many bytes wait until released.
    stall_after: AtomicU64,
    released: AtomicBool,
}

impl FakeHost {
    pub(crate) fn stall_reads_after(&self, bytes: Option<u64>) {
        self.stall_after.store(bytes.unwrap_or(u64::MAX), Ordering::Release);
    }

    /// Lets stalled reads go (the plugin is being dropped).
    pub(crate) fn release_reads(&self) {
        self.released.store(true, Ordering::Release);
        drop(lock(&self.read_signal.0));
        self.read_signal.1.notify_all();
    }

    fn id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Lends `bytes` as a host file; returns its id.
    pub(crate) fn lend_file(&self, bytes: Vec<u8>) -> u64 {
        let id = self.id();
        lock(&self.files).insert(id, Arc::new(bytes));
        id
    }

    /// Lends `records` as an annotator's view; returns its ticket.
    pub(crate) fn lend_view(&self, records: Vec<WireViewRecord>) -> u64 {
        let id = self.id();
        lock(&self.views).insert(id, Arc::new(records));
        id
    }

    /// Takes back what was lent under `id`.
    pub(crate) fn take_back(&self, id: u64) {
        lock(&self.files).remove(&id);
        lock(&self.views).remove(&id);
        lock(&self.read_of).remove(&id);
    }

    /// Bytes the plugin read of lent file `id` so far.
    pub(crate) fn read_of(&self, id: u64) -> u64 {
        lock(&self.read_of).get(&id).copied().unwrap_or(0)
    }

    /// Waits up to `slice` for the plugin to have read `target` bytes of lent file `id`; true once it has. Woken by
    /// each read.
    pub(crate) fn wait_read_of(&self, id: u64, target: u64, slice: Duration) -> bool {
        let (mutex, cv) = &self.read_signal;
        let guard = lock(mutex);
        let (_guard, _) = cv
            .wait_timeout_while(guard, slice, |_| self.read_of(id) < target)
            .unwrap_or_else(|p| p.into_inner());
        self.read_of(id) >= target
    }

    /// The fault hook: holds a read until released once `stall_after` bytes were served.
    fn wait_while_stalled(&self) {
        let (mutex, cv) = &self.read_signal;
        let guard = lock(mutex);
        drop(
            cv.wait_while(guard, |_| {
                self.bytes_read.load(Ordering::Acquire) >= self.stall_after.load(Ordering::Acquire)
                    && !self.released.load(Ordering::Acquire)
            })
            .unwrap_or_else(|p| p.into_inner()),
        );
    }

    /// Serves a plugin's `ReadFile`: up to `len` (at most [`READ_CHUNK`]) bytes of lent file `id` at `offset`.
    pub(crate) fn read_file(&self, id: u64, offset: u64, len: u32) -> Result<Vec<u8>, RemoteError> {
        self.wait_while_stalled();
        let data = lock(&self.files)
            .get(&id)
            .cloned()
            .ok_or_else(|| RemoteError::new(code::INVALID, "unknown file"))?;
        let start = usize::try_from(offset).unwrap_or(usize::MAX).min(data.len());
        let end = start.saturating_add(len.min(READ_CHUNK) as usize).min(data.len());
        self.bytes_read.fetch_add((end - start) as u64, Ordering::Release);
        *lock(&self.read_of).entry(id).or_default() += (end - start) as u64;
        // Taking the lock orders this notify after a waiter's check, so no wake-up is lost.
        drop(lock(&self.read_signal.0));
        self.read_signal.1.notify_all();
        Ok(data[start..end].to_vec())
    }

    fn view(&self, ticket: u64) -> Result<Arc<Vec<WireViewRecord>>, RemoteError> {
        lock(&self.views)
            .get(&ticket)
            .cloned()
            .ok_or_else(|| RemoteError::new(code::INVALID, format!("no view {ticket}")))
    }
}

impl Handler for FakeHost {
    fn call(&self, _ctx: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError> {
        let req: HostRequest = decode_request(body)?;
        match req {
            HostRequest::ReadFile { id, offset, len } => encode_reply(&self.read_file(id, offset, len)?),
            HostRequest::ViewSelect { ticket, filter, limit } => {
                let rows = self.view(ticket)?;
                encode_reply(&view::select(&rows, &filter.0, limit as usize))
            }
            HostRequest::ViewInstances { ticket } => {
                let rows = self.view(ticket)?;
                let mut out: Vec<String> = Vec::new();
                for r in rows.iter() {
                    if !out.contains(&r.instance) {
                        out.push(r.instance.clone());
                    }
                }
                encode_reply(&out)
            }
            HostRequest::ViewTimeRange { ticket } => {
                let rows = self.view(ticket)?;
                let times = rows.iter().filter_map(|r| r.event.ts);
                let range = times.clone().min().zip(times.max());
                encode_reply(&range)
            }
            HostRequest::TaskStarted { tool, task } => {
                lock(&self.tasks).push(TaskReport::Started { tool, task });
                encode_reply(&())
            }
            HostRequest::TaskProgress { tool, task, fraction } => {
                lock(&self.tasks).push(TaskReport::Progress { tool, task, fraction });
                encode_reply(&())
            }
            HostRequest::TaskFinished { tool, task } => {
                lock(&self.tasks).push(TaskReport::Finished { tool, task });
                encode_reply(&())
            }
            HostRequest::InvokeElevated { tool, op, .. } => Err(RemoteError::new(
                code::INVALID,
                format!("the test harness runs no elevated process (\"{tool} {op}\")"),
            )),
            // Only the elevated process sends these, and the harness runs none.
            HostRequest::SaveOutput { .. } | HostRequest::Log { .. } => Err(RemoteError::new(
                code::INVALID,
                "the test harness runs no elevated process, so it saves no output files",
            )),
        }
    }
}
