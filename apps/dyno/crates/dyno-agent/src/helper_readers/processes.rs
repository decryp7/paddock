//! Every account's processes through the helper (Dyno helper-wiring plan section 2.4, WP-PROC). A **Live** reading: the process
//! table the sampler fills comes from this computer's own reader (it has the caller's rates), and the helper's sweep adds the
//! processes this computer cannot read (other accounts, protected system processes).
//!
//! - [`Sweeper`] is the seam reading ([`crate::helper_first`]): it asks the helper for the whole list, page by page, and says so
//!   when it cannot (the needs-admin sentence). It runs on its own worker ([`SweepWorker`], every [`SWEEP_PERIOD`]), never on the
//!   sampling thread (ER-17); the sampler takes the latest finished sweep.
//! - [`sample_of`] and [`rates_between`] are pure: a helper entry becomes a [`ProcessSample`], and the rates come from the
//!   difference of the cumulative CPU time and disk bytes between two sweeps (keyed by pid **and** start time, rule 53).
//! - [`merge`] is pure too: the local table plus the latest sweep. The local row wins where it has a value; the helper fills what
//!   is missing and adds the processes local does not have.
//! - [`HelperFirstProcesses`] is the [`ProcessSource`] decorator the sampler runs on.
//!
//! Cost per tick (rule 48, `docs/performance.md`): the sampling thread only clones the latest sweep's samples and merges them in
//! one pass over a hash index (no IPC); the helper pages, at most [`MAX_PAGES`] of [`PAGE`], are read on the worker.

use crate::helper_first::{HelperCalls, HelperFirst, HelperPort, HelperReading, ReadClass};
use crate::helper_link::CallError;
use dyno_core::helper_processes::{OwnerKind, ProcState, ProcessEntry, ProcessPage, MAX_SWEEP};
use dyno_core::helper_requests::{ProcessQuery, RequestClass, MAX_PROCESS_PAGE};
use dyno_core::metrics::Reading;
use dyno_core::others::{OtherUsers, ProcessProvenance};
use dyno_core::ports::{PlatformError, ProcessSource};
use dyno_core::process::{Measured, Owner, Pid, ProcessDetails, ProcessRef, ProcessSample, RunState, UnavailableReason};
use dyno_core::read_via::ReadVia;
use dyno_core::time::Timestamp;
use paddock_jobs::time::Clock;
use paddock_jobs::CancelToken;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How often the worker sweeps while the list is being read.
pub const SWEEP_PERIOD: Duration = Duration::from_secs(5);
/// The worker pauses when nothing has asked for the list for this long (nobody waits for it).
pub const IDLE_AFTER: Duration = Duration::from_secs(20);
/// A sweep older than this is not merged: the worker is stuck or the helper went away.
pub const STALE_AFTER: Duration = Duration::from_secs(30);
/// Entries per page asked of the helper.
pub const PAGE: u16 = MAX_PROCESS_PAGE;
/// The most pages one sweep follows (`MAX_SWEEP` / page size).
pub const MAX_PAGES: usize = MAX_SWEEP.div_ceil(PAGE as usize);
/// What a non-owner's "Other users" row says for an account the helper may not name.
pub const ANOTHER_ACCOUNT: &str = "Another account";
/// What the list says while the helper is off and other accounts' processes are hidden.
pub const OTHERS_NEED_HELPER: &str = "other accounts' processes are not shown";

// ------------------------------------------------------------------------------------------------------------- the sweep

/// One finished read of the helper's whole list.
#[derive(Debug, Clone, PartialEq)]
pub struct RawSweep {
    /// When it finished, on the worker's clock.
    pub at: Duration,
    /// Every entry, in pid order, as the helper scoped them to the caller.
    pub entries: Vec<ProcessEntry>,
    /// Other accounts' total, for a caller who sees them only as a total.
    pub others: Option<dyno_core::helper_processes::OthersTotal>,
    /// The helper or the page cap cut the list.
    pub truncated: bool,
    /// Pages read.
    pub pages: usize,
}

/// The seam reading: the helper's list, else the reason.
pub struct Sweeper {
    clock: Arc<dyn Clock>,
}

impl Sweeper {
    /// A reading that stamps its sweeps with `clock`.
    pub fn new(clock: Arc<dyn Clock>) -> Sweeper {
        Sweeper { clock }
    }
}

impl HelperReading for Sweeper {
    type Value = RawSweep;

    fn name(&self) -> &'static str {
        "processes of every account"
    }

    /// A person looks at the list and it refreshes every few seconds: Live (a page must answer within the Live deadline).
    fn class(&self) -> ReadClass {
        ReadClass::Live
    }

    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<RawSweep, CallError> {
        let mut entries: Vec<ProcessEntry> = Vec::new();
        let mut others = None;
        let mut truncated = false;
        let mut after: Option<Pid> = None;
        let mut pages = 0;
        loop {
            let page: ProcessPage = calls.processes(ProcessQuery {
                after,
                limit: PAGE,
                class: RequestClass::User,
            })?;
            pages += 1;
            if pages == 1 {
                others = page.others;
            }
            truncated |= page.truncated;
            entries.extend(page.items);
            match page.next {
                // The cursor must move forward, or a broken helper would keep us here (bounded input, PRINCIPLES).
                Some(next) if after.is_none_or(|a| next > a) && pages < MAX_PAGES && entries.len() < MAX_SWEEP => after = Some(next),
                Some(_) => {
                    truncated = true;
                    break;
                }
                None => break,
            }
        }
        entries.truncate(MAX_SWEEP);
        Ok(RawSweep {
            at: self.clock.monotonic(),
            entries,
            others,
            truncated,
            pages,
        })
    }

    fn local(&mut self) -> Reading<RawSweep> {
        Reading::Unavailable(OTHERS_NEED_HELPER.into())
    }
}

// ---------------------------------------------------------------------------------------------------------- pure pieces

/// How fast a process used the CPU and the disk between two sweeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    /// CPU, percent of the whole machine.
    pub cpu: Option<f32>,
    /// Bytes read per second.
    pub read_bps: Option<f64>,
    /// Bytes written per second.
    pub write_bps: Option<f64>,
}

/// The key that identifies a process across sweeps: pid plus start time in whole seconds. A process with no start time has no
/// key (rule 53: never the pid alone), so it gets no rates and no match.
fn key_of(e: &ProcessEntry) -> Option<(Pid, i64)> {
    e.started.map(|s| (e.pid, s))
}

/// The rates of `now` since `before` over `dt`, on a machine with `cores` cores. `None` when the two are not the same process
/// or no time passed. A counter that went backwards (a reused handle) is not a rate.
pub fn rates_between(before: &ProcessEntry, now: &ProcessEntry, dt: Duration, cores: usize) -> Option<Rates> {
    let (a, b) = (key_of(before)?, key_of(now)?);
    let secs = dt.as_secs_f64();
    if a != b || secs <= 0.0 {
        return None;
    }
    let per_second = |x: Option<u64>, y: Option<u64>| Some(y?.checked_sub(x?)? as f64 / secs);
    let cpu = per_second(before.cpu_time_ms, now.cpu_time_ms)
        .map(|ms_per_s| (ms_per_s / 1_000.0 / cores.max(1) as f64 * 100.0).clamp(0.0, 100.0) as f32);
    Some(Rates {
        cpu,
        read_bps: per_second(before.disk_read_bytes, now.disk_read_bytes),
        write_bps: per_second(before.disk_written_bytes, now.disk_written_bytes),
    })
}

fn owner_of(kind: &OwnerKind) -> Owner {
    match kind {
        OwnerKind::Caller => Owner::Own,
        OwnerKind::System | OwnerKind::Service(_) => Owner::System,
        OwnerKind::OtherUser => Owner::Other,
    }
}

fn state_of(s: ProcState) -> RunState {
    match s {
        ProcState::Running => RunState::Running,
        ProcState::Sleeping => RunState::Sleeping,
        ProcState::Stopped => RunState::Suspended,
        ProcState::Other => RunState::Other,
    }
}

/// A helper entry as the agent's sample. An account the helper did not name for another person stays "Another account".
pub fn sample_of(e: &ProcessEntry, rates: Option<Rates>) -> ProcessSample {
    let account = match (&e.account, &e.owner) {
        (Some(name), _) if !name.is_empty() => Measured::Value(name.clone()),
        (_, OwnerKind::OtherUser) => Measured::Value(ANOTHER_ACCOUNT.into()),
        (_, OwnerKind::Service(name)) => Measured::Value(name.clone()),
        (_, OwnerKind::System) => Measured::Value("SYSTEM".into()),
        _ => Measured::unavailable(UnavailableReason::NotSupported),
    };
    // The first sweep has no earlier one to compare with: the rate is "not read yet", never 0.
    let or_unread = |v: Option<f64>| v.map_or(Measured::unavailable(UnavailableReason::NotSupported), Measured::Value);
    ProcessSample {
        pid: e.pid,
        parent: e.parent,
        name: e.name.clone(),
        exe: e.path.clone(),
        user: account,
        cpu: rates
            .and_then(|r| r.cpu)
            .map_or(Measured::unavailable(UnavailableReason::NotSupported), Measured::Value),
        memory: e
            .memory_bytes
            .map_or(Measured::unavailable(UnavailableReason::NotPermitted), Measured::Value),
        disk_read_bps: or_unread(rates.and_then(|r| r.read_bps)),
        disk_write_bps: or_unread(rates.and_then(|r| r.write_bps)),
        started: e.started.map_or(Measured::unavailable(UnavailableReason::NotSupported), |s| {
            Measured::Value(Timestamp(s.saturating_mul(1_000)))
        }),
        owner: owner_of(&e.owner),
        state: state_of(e.state),
    }
}

/// The helper's "Other users" total as the agent's row. The CPU is a rate from the earlier total, when there is one.
pub fn others_of(
    total: &dyno_core::helper_processes::OthersTotal,
    before: Option<(&dyno_core::helper_processes::OthersTotal, Duration)>,
    cores: usize,
) -> OtherUsers {
    let cpu = before
        .and_then(|(b, dt)| {
            let secs = dt.as_secs_f64();
            let ms = total.cpu_time_ms.checked_sub(b.cpu_time_ms)?;
            (secs > 0.0).then(|| (ms as f64 / 1_000.0 / secs / cores.max(1) as f64 * 100.0).clamp(0.0, 100.0) as f32)
        })
        .map_or(Measured::unavailable(UnavailableReason::NotSupported), Measured::Value);
    OtherUsers {
        count: total.processes,
        cpu,
        memory: Measured::Value(total.memory_bytes),
        partial: false,
    }
}

/// A finished sweep with its rates, ready to merge.
#[derive(Debug, Clone)]
pub struct SweepView {
    /// When it finished.
    pub at: Duration,
    /// Its entries as samples.
    pub samples: Vec<ProcessSample>,
    /// The helper's total for other accounts, as the agent's row.
    pub others: Option<OtherUsers>,
    /// Whether the entries include other accounts' processes one by one.
    pub lists_others: bool,
}

/// Turns `now` into a view, with rates from `before` (the previous sweep).
pub fn view_of(now: &RawSweep, before: Option<&RawSweep>, cores: usize) -> SweepView {
    let index: HashMap<(Pid, i64), &ProcessEntry> = before
        .map(|b| b.entries.iter().filter_map(|e| key_of(e).map(|k| (k, e))).collect())
        .unwrap_or_default();
    let dt = before.map_or(Duration::ZERO, |b| now.at.saturating_sub(b.at));
    let samples = now
        .entries
        .iter()
        .map(|e| {
            let rates = key_of(e)
                .and_then(|k| index.get(&k))
                .and_then(|old| rates_between(old, e, dt, cores));
            sample_of(e, rates)
        })
        .collect();
    let others = now
        .others
        .as_ref()
        .map(|t| others_of(t, before.and_then(|b| b.others.as_ref()).map(|o| (o, dt)), cores));
    SweepView {
        at: now.at,
        samples,
        others,
        lists_others: now.entries.iter().any(|e| e.owner == OwnerKind::OtherUser),
    }
}

/// Whether two start times name the same process: pid plus start time, within a second of rounding between the two readers.
fn same_start(a: &Measured<Timestamp>, b: &Measured<Timestamp>) -> bool {
    match (a.copied(), b.copied()) {
        (Some(x), Some(y)) => (x.millis() - y.millis()).abs() <= 1_000,
        _ => false,
    }
}

/// The local table with the helper's sweep merged in. A local value wins; the helper fills a value local could not read (and the
/// owner and account of a row local could not place) and adds the processes local does not have.
pub fn merge(mut local: Vec<ProcessSample>, sweep: &SweepView) -> Vec<ProcessSample> {
    let mut by_pid: HashMap<Pid, Vec<usize>> = HashMap::new();
    for (i, p) in local.iter().enumerate() {
        by_pid.entry(p.pid).or_default().push(i);
    }
    for h in &sweep.samples {
        let twin = by_pid
            .get(&h.pid)
            .and_then(|ix| ix.iter().copied().find(|&i| same_start(&local[i].started, &h.started)));
        let Some(i) = twin else {
            local.push(h.clone());
            continue;
        };
        let l = &mut local[i];
        if l.cpu.is_unavailable() {
            l.cpu = h.cpu;
        }
        if l.memory.is_unavailable() {
            l.memory = h.memory;
        }
        if l.disk_read_bps.is_unavailable() {
            l.disk_read_bps = h.disk_read_bps;
        }
        if l.disk_write_bps.is_unavailable() {
            l.disk_write_bps = h.disk_write_bps;
        }
        if l.user.is_unavailable() {
            l.user = h.user.clone();
        }
        if l.exe.is_none() {
            l.exe = h.exe.clone();
        }
        if l.parent.is_none() {
            l.parent = h.parent;
        }
        if l.owner != Owner::Own {
            l.owner = h.owner;
        }
    }
    local
}

// ------------------------------------------------------------------------------------------------------------ the worker

/// What the worker last finished, shared with the decorator.
#[derive(Default)]
struct Latest {
    view: Option<SweepView>,
    /// Who answered the last attempt.
    via: ReadVia,
}

/// The shared state between the worker and the decorator.
#[derive(Clone)]
pub struct SweepStore {
    latest: Arc<Mutex<Latest>>,
    /// Monotonic milliseconds of the last time the sampler took the list; the worker pauses when it is old.
    pulled_ms: Arc<AtomicU64>,
    /// A person asked to try again: the worker's next sweep asks the helper at once, ending any quiet window.
    retry: Arc<std::sync::atomic::AtomicBool>,
    clock: Arc<dyn Clock>,
}

impl SweepStore {
    /// An empty store on `clock`.
    pub fn new(clock: Arc<dyn Clock>) -> SweepStore {
        SweepStore {
            latest: Arc::default(),
            pulled_ms: Arc::new(AtomicU64::new(0)),
            retry: Arc::default(),
            clock,
        }
    }

    /// Asks the worker to try the helper again at once (a person pressed "Try again").
    pub fn retry_now(&self) {
        self.retry.store(true, Ordering::SeqCst);
    }

    /// Publishes a sweep, or (`view` none) the reason there is none; `clear` also drops the kept sweep (the helper is off).
    fn put(&self, view: Option<SweepView>, via: ReadVia, clear: bool) {
        let mut l = self.latest.lock().unwrap_or_else(PoisonError::into_inner);
        if view.is_some() || clear {
            l.view = view;
        }
        l.via = via;
    }

    /// The latest sweep when it is fresh, and who answered last.
    fn take(&self) -> (Option<SweepView>, ReadVia) {
        self.pulled_ms.store(self.clock.now_ms().max(1), Ordering::SeqCst);
        let l = self.latest.lock().unwrap_or_else(PoisonError::into_inner);
        let fresh = l
            .view
            .clone()
            .filter(|v| self.clock.monotonic().saturating_sub(v.at) <= STALE_AFTER);
        (fresh, l.via.clone())
    }

    /// Whether the sampler has taken the list lately (a first sweep is also due when it never has: it is about to).
    fn wanted(&self) -> bool {
        let at = self.pulled_ms.load(Ordering::SeqCst);
        at == 0 || self.clock.now_ms().saturating_sub(at) <= IDLE_AFTER.as_millis() as u64
    }
}

/// One sweep at a time, on the worker's own thread.
pub struct SweepWorker {
    first: HelperFirst<Sweeper>,
    store: SweepStore,
    cores: usize,
    before: Option<RawSweep>,
    /// The helper epoch of the last sweep: a flip (turned on or off, repaired) drops what was kept.
    epoch: u64,
}

impl SweepWorker {
    /// A worker that reads through `port` into `store`.
    pub fn new(port: Box<dyn HelperPort>, store: SweepStore, cores: usize) -> SweepWorker {
        let reading = Sweeper::new(store.clock.clone());
        SweepWorker {
            first: HelperFirst::new(reading, port),
            store,
            cores,
            before: None,
            epoch: 0,
        }
    }

    /// One sweep: asks the helper (or learns that it cannot be asked) and publishes the result. A failed sweep keeps the
    /// previous view until it goes stale; a helper that is off clears nothing but says why.
    pub fn sweep_once(&mut self) {
        let started = self.store.clock.monotonic();
        if self.store.retry.swap(false, Ordering::SeqCst) {
            self.first.retry_now();
        }
        let got = self.first.read();
        let epoch = self.first.epoch();
        let flipped = std::mem::replace(&mut self.epoch, epoch) != epoch;
        let took = self.store.clock.monotonic().saturating_sub(started);
        match got.value {
            Reading::Available(raw) => {
                let view = view_of(&raw, self.before.as_ref(), self.cores);
                log::info!(
                    target: "dyno::helper",
                    Processes = raw.entries.len(), Pages = raw.pages, Truncated = raw.truncated, Ms = took.as_millis() as u64;
                    "Processes of every account: sweep done"
                );
                self.before = Some(raw);
                self.store.put(Some(view), got.via, false);
            }
            Reading::Unavailable(_) => {
                // A failed sweep keeps the last good one until it goes stale; a helper that is off or changed does not.
                let clear = flipped || matches!(got.via, ReadVia::NeedsHelper { .. });
                if clear {
                    self.before = None;
                }
                self.store.put(None, got.via, clear);
            }
        }
    }

    /// Sweeps every [`SWEEP_PERIOD`] on `clock` until `cancel`, pausing while nobody takes the list.
    pub fn run(mut self, cancel: &CancelToken) {
        let clock = self.store.clock.clone();
        while !cancel.is_cancelled() {
            if self.store.wanted() && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.sweep_once())).is_err() {
                log::error!(target: "dyno::helper", "Processes of every account: a sweep panicked; continuing");
            }
            if clock.wait(SWEEP_PERIOD, cancel).is_err() {
                break;
            }
        }
    }
}

// ----------------------------------------------------------------------------------------------------- the decorator

/// The process source the sampler runs on: this computer's table plus what the helper's worker last swept.
pub struct HelperFirstProcesses {
    local: Box<dyn ProcessSource>,
    store: SweepStore,
    provenance: ProcessProvenance,
}

impl HelperFirstProcesses {
    /// The decorator over `local`, reading the worker's `store`.
    pub fn new(local: Box<dyn ProcessSource>, store: SweepStore) -> HelperFirstProcesses {
        HelperFirstProcesses {
            local,
            store,
            provenance: ProcessProvenance::default(),
        }
    }

    /// Starts the worker thread; it stops when `cancel` is cancelled. Returns the decorator over a store the worker fills.
    pub fn start(
        local: Box<dyn ProcessSource>,
        port: Box<dyn HelperPort>,
        clock: Arc<dyn Clock>,
        cores: usize,
        cancel: CancelToken,
    ) -> HelperFirstProcesses {
        let store = SweepStore::new(clock);
        let worker = SweepWorker::new(port, store.clone(), cores);
        if let Err(e) = std::thread::Builder::new()
            .name("dyno-process-sweeps".into())
            .spawn(move || worker.run(&cancel))
        {
            log::error!(target: "dyno::helper", "Processes of every account: the worker did not start: {e}");
        }
        HelperFirstProcesses::new(local, store)
    }

    /// What the table says about where it came from, given the local rows and the worker's last word.
    fn provenance_of(local: &[ProcessSample], view: Option<&SweepView>, worker_via: ReadVia) -> ProcessProvenance {
        match view {
            Some(v) => ProcessProvenance {
                processes: ReadVia::Helper,
                others: if v.others.is_some() || v.lists_others {
                    ReadVia::Helper
                } else {
                    ReadVia::Local
                },
                lists_others: v.lists_others,
                others_total: v.others.clone(),
            },
            None => {
                // Without the helper the list is this computer's. It asks for the helper only when something is hidden.
                let hidden = local
                    .iter()
                    .any(|p| p.owner == Owner::Other || p.cpu.is_unavailable() || p.memory.is_unavailable());
                ProcessProvenance {
                    processes: if hidden && !matches!(worker_via, ReadVia::Helper) {
                        worker_via
                    } else {
                        ReadVia::Local
                    },
                    others: ReadVia::Local,
                    lists_others: false,
                    others_total: None,
                }
            }
        }
    }
}

impl ProcessSource for HelperFirstProcesses {
    fn processes(&mut self) -> Vec<ProcessSample> {
        let local = self.local.processes();
        let (view, via) = self.store.take();
        self.provenance = Self::provenance_of(&local, view.as_ref(), via);
        match view {
            Some(v) => merge(local, &v),
            None => local,
        }
    }

    fn details(&mut self, target: ProcessRef) -> Result<ProcessDetails, PlatformError> {
        self.local.details(target)
    }

    fn provenance(&self) -> ProcessProvenance {
        self.provenance.clone()
    }

    fn retry_helper(&mut self) {
        self.store.retry_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_first::testkit::{centre, port, Net};
    use dyno_core::helper_processes::OthersTotal;
    use dyno_core::helper_wire::HelperReply;
    use paddock_jobs::time::FakeClock;

    fn entry(pid: u32, owner: OwnerKind, account: Option<&str>, cpu_ms: u64) -> ProcessEntry {
        ProcessEntry {
            pid,
            started: Some(i64::from(pid)),
            parent: None,
            name: format!("p{pid}"),
            path: None,
            owner,
            account: account.map(str::to_string),
            state: ProcState::Running,
            threads: Some(1),
            cpu_time_ms: Some(cpu_ms),
            memory_bytes: Some(10 << 20),
            disk_read_bytes: Some(cpu_ms * 100),
            disk_written_bytes: Some(0),
        }
    }

    /// A caller-owned process the local reader read completely (pid, parent, name, CPU percent, memory in MiB).
    fn proc(pid: Pid, parent: Option<Pid>, name: &str, cpu: f32, mem_mib: u64) -> ProcessSample {
        ProcessSample {
            pid,
            parent,
            name: name.into(),
            exe: Some(format!("/apps/{name}")),
            user: Measured::Value("me".into()),
            cpu: Measured::Value(cpu),
            memory: Measured::Value(mem_mib << 20),
            disk_read_bps: Measured::Value(0.0),
            disk_write_bps: Measured::Value(0.0),
            started: Measured::Value(Timestamp(i64::from(pid) * 1_000)),
            owner: Owner::Own,
            state: RunState::Running,
        }
    }

    fn page(items: Vec<ProcessEntry>, next: Option<u32>, others: Option<OthersTotal>) -> HelperReply {
        HelperReply::Processes(ProcessPage {
            taken_at: 1,
            items,
            next,
            others,
            truncated: false,
        })
    }

    fn worker(up: bool, script: Vec<Result<HelperReply, crate::helper_link::TransportError>>) -> (SweepWorker, SweepStore, FakeClock) {
        let clock = FakeClock::new();
        let c = centre(&Net::new(up, script), &clock);
        let store = SweepStore::new(Arc::new(clock.clone()));
        (SweepWorker::new(port(&c), store.clone(), 4), store, clock)
    }

    /// A local table that sees the caller's process and cannot read another account's.
    fn local_table() -> Vec<ProcessSample> {
        let mut hidden = proc(7, None, "p7", 0.0, 0);
        hidden.owner = Owner::Other;
        hidden.cpu = Measured::unavailable(UnavailableReason::NotPermitted);
        hidden.memory = Measured::unavailable(UnavailableReason::NotPermitted);
        hidden.user = Measured::unavailable(UnavailableReason::NotPermitted);
        hidden.started = Measured::Value(Timestamp(7_000));
        vec![proc(1, None, "mine", 3.0, 5), hidden]
    }

    struct Local(Vec<ProcessSample>);
    impl ProcessSource for Local {
        fn processes(&mut self) -> Vec<ProcessSample> {
            self.0.clone()
        }
        fn details(&mut self, _: ProcessRef) -> Result<ProcessDetails, PlatformError> {
            Err(PlatformError::Unsupported("test".into()))
        }
    }

    #[test]
    fn another_accounts_process_appears_when_the_helper_is_ready() {
        let (mut w, store, _) = worker(
            true,
            vec![Ok(page(
                vec![
                    entry(1, OwnerKind::Caller, Some("me"), 10),
                    entry(9, OwnerKind::OtherUser, Some("sam"), 10),
                    entry(20, OwnerKind::System, Some("SYSTEM"), 10),
                ],
                None,
                None,
            ))],
        );
        w.sweep_once();
        let mut src = HelperFirstProcesses::new(Box::new(Local(local_table())), store);
        let table = src.processes();
        let sam = table.iter().find(|p| p.pid == 9).expect("the other account's process is listed");
        assert_eq!((sam.user_name(), sam.owner), (Some("sam"), Owner::Other));
        assert!(
            table.iter().any(|p| p.pid == 20 && p.owner == Owner::System),
            "a protected system process is listed"
        );
        let mine = table.iter().find(|p| p.pid == 1).unwrap();
        assert_eq!(mine.cpu, Measured::Value(3.0), "the local row keeps its own rate");
        let prov = src.provenance();
        assert_eq!((&prov.processes, prov.lists_others), (&ReadVia::Helper, true));
        assert!(!prov.fold_others(true), "listed one by one, not folded into Other users");
    }

    #[test]
    fn the_helper_fills_what_local_could_not_read_and_does_not_add_a_second_row() {
        let (mut w, store, _) = worker(
            true,
            vec![Ok(page(vec![entry(7, OwnerKind::OtherUser, Some("sam"), 10)], None, None))],
        );
        w.sweep_once();
        let table = HelperFirstProcesses::new(Box::new(Local(local_table())), store).processes();
        assert_eq!(table.iter().filter(|p| p.pid == 7).count(), 1);
        let p = table.iter().find(|p| p.pid == 7).unwrap();
        assert_eq!((p.memory, p.user_name()), (Measured::Value(10 << 20), Some("sam")));
    }

    #[test]
    fn a_reused_pid_is_a_different_process() {
        let (mut w, store, _) = worker(
            true,
            vec![Ok(page(vec![entry(7, OwnerKind::OtherUser, Some("sam"), 10)], None, None))],
        );
        w.sweep_once();
        let mut local = local_table();
        local[1].started = Measured::Value(Timestamp(900_000));
        let table = HelperFirstProcesses::new(Box::new(Local(local)), store).processes();
        assert_eq!(table.iter().filter(|p| p.pid == 7).count(), 2, "same pid, other start time");
    }

    #[test]
    fn rates_come_from_two_sweeps_and_the_first_is_not_zero() {
        let (mut w, store, clock) = worker(
            true,
            vec![
                Ok(page(vec![entry(9, OwnerKind::OtherUser, Some("sam"), 1_000)], None, None)),
                Ok(page(vec![entry(9, OwnerKind::OtherUser, Some("sam"), 3_000)], None, None)),
            ],
        );
        w.sweep_once();
        let first = HelperFirstProcesses::new(Box::new(Local(vec![])), store.clone()).processes();
        assert!(first[0].cpu.is_unavailable(), "no earlier sweep: not read, never 0");
        clock.advance(Duration::from_secs(5));
        w.sweep_once();
        let second = HelperFirstProcesses::new(Box::new(Local(vec![])), store).processes();
        // 2 000 ms of CPU in 5 s on 4 cores = 10 % of the machine.
        assert_eq!(second[0].cpu, Measured::Value(10.0));
        assert_eq!(second[0].disk_read_bps, Measured::Value(40_000.0));
    }

    #[test]
    fn rates_need_the_same_process_not_just_the_same_pid() {
        let (a, mut b) = (entry(9, OwnerKind::Caller, None, 100), entry(9, OwnerKind::Caller, None, 900));
        b.started = Some(99);
        assert_eq!(rates_between(&a, &b, Duration::from_secs(1), 1), None);
        b.started = None;
        assert_eq!(rates_between(&a, &b, Duration::from_secs(1), 1), None);
        let mut back = entry(9, OwnerKind::Caller, None, 50);
        back.disk_read_bytes = Some(0);
        let r = rates_between(&a, &back, Duration::from_secs(1), 1).unwrap();
        assert_eq!((r.cpu, r.read_bps), (None, None), "a counter that went backwards is not a rate");
    }

    #[test]
    fn a_non_owner_gets_the_others_total_instead_of_rows() {
        let total = |ms| OthersTotal {
            processes: 3,
            memory_bytes: 3 << 30,
            cpu_time_ms: ms,
        };
        let (mut w, store, clock) = worker(
            true,
            vec![
                Ok(page(vec![entry(1, OwnerKind::Caller, Some("me"), 1)], None, Some(total(1_000)))),
                Ok(page(vec![entry(1, OwnerKind::Caller, Some("me"), 1)], None, Some(total(5_000)))),
            ],
        );
        w.sweep_once();
        clock.advance(Duration::from_secs(5));
        w.sweep_once();
        let mut src = HelperFirstProcesses::new(Box::new(Local(local_table())), store);
        let table = src.processes();
        let prov = src.provenance();
        let (listed, other) = prov.split(&table, true);
        assert!(listed.iter().all(|p| p.owner != Owner::Other), "folded");
        let o = other.expect("the helper's total");
        // 4 000 ms in 5 s on 4 cores = 20 %.
        assert_eq!(
            (o.count, o.memory, o.cpu, o.partial),
            (3, Measured::Value(3 << 30), Measured::Value(20.0), false)
        );
        assert_eq!(prov.others, ReadVia::Helper);
    }

    #[test]
    fn without_the_helper_the_local_table_stands_and_asks_for_it_only_when_something_is_hidden() {
        let (mut w, store, _) = worker(false, vec![]);
        w.sweep_once();
        let mut src = HelperFirstProcesses::new(Box::new(Local(local_table())), store.clone());
        let n = src.processes().len();
        assert_eq!(n, 2, "nothing is lost");
        let via = src.provenance().processes;
        assert!(matches!(via, ReadVia::NeedsHelper { .. }), "{via:?}");
        assert!(
            via.reason().unwrap().contains("Other accounts' processes are not shown."),
            "{via:?}"
        );
        let mut all_read = HelperFirstProcesses::new(Box::new(Local(vec![proc(1, None, "mine", 1.0, 1)])), store);
        all_read.processes();
        assert_eq!(
            all_read.provenance().processes,
            ReadVia::Local,
            "nothing hidden, nothing to ask for"
        );
    }

    #[test]
    fn turning_the_helper_off_drops_the_kept_sweep() {
        let net = Net::new(
            true,
            vec![Ok(page(vec![entry(9, OwnerKind::OtherUser, Some("sam"), 1)], None, None))],
        );
        let clock = FakeClock::new();
        let c = centre(&net, &clock);
        let store = SweepStore::new(Arc::new(clock.clone()));
        let mut w = SweepWorker::new(port(&c), store.clone(), 1);
        w.sweep_once();
        net.set(crate::helper_first::testkit::Mode::Absent);
        c.forget_ready();
        w.sweep_once();
        let table = HelperFirstProcesses::new(Box::new(Local(vec![])), store).processes();
        assert!(table.is_empty());
    }

    #[test]
    fn a_stale_sweep_is_not_merged() {
        let (mut w, store, clock) = worker(
            true,
            vec![Ok(page(vec![entry(9, OwnerKind::OtherUser, Some("sam"), 1)], None, None))],
        );
        w.sweep_once();
        clock.advance(STALE_AFTER + Duration::from_secs(1));
        assert!(HelperFirstProcesses::new(Box::new(Local(vec![])), store).processes().is_empty());
    }

    #[test]
    fn pages_are_followed_and_capped() {
        let a = page(vec![entry(1, OwnerKind::Caller, None, 1)], Some(1), None);
        let b = page(vec![entry(2, OwnerKind::Caller, None, 1)], Some(2), None);
        let c = page(vec![entry(3, OwnerKind::Caller, None, 1)], None, None);
        let (mut w, store, _) = worker(true, vec![Ok(a), Ok(b), Ok(c)]);
        w.sweep_once();
        assert_eq!(HelperFirstProcesses::new(Box::new(Local(vec![])), store).processes().len(), 3);
        // A helper whose cursor does not move forward ends the sweep, marked cut, instead of looping.
        let stuck = |pid| page(vec![entry(pid, OwnerKind::Caller, None, 1)], Some(5), None);
        let (mut w, store, _) = worker(true, vec![Ok(stuck(1)), Ok(stuck(2)), Ok(stuck(3))]);
        w.sweep_once();
        assert_eq!(HelperFirstProcesses::new(Box::new(Local(vec![])), store).processes().len(), 2);
    }

    #[test]
    fn a_helper_entry_converts_like_a_local_sample() {
        // Contract: the same process through the helper path and the local path agree on name, pid, start, owner and memory.
        let e = entry(5, OwnerKind::Caller, Some("me"), 1);
        let h = sample_of(&e, None);
        let l = proc(5, None, "p5", 0.0, 10);
        assert_eq!(
            (h.pid, h.name.clone(), h.started, h.owner, h.memory),
            (l.pid, l.name, l.started, l.owner, l.memory)
        );
        let nameless = sample_of(&entry(6, OwnerKind::OtherUser, None, 1), None);
        assert_eq!(nameless.user_name(), Some(ANOTHER_ACCOUNT));
    }

    #[test]
    fn try_again_asks_the_helper_at_once_after_a_hang() {
        use crate::helper_link::TransportError;
        let clock = FakeClock::new();
        let net = Net::new(
            true,
            vec![
                Err(TransportError::Silent("no answer".into())),
                Ok(page(vec![entry(9, OwnerKind::OtherUser, Some("sam"), 1)], None, None)),
            ],
        );
        let c = centre(&net, &clock);
        let store = SweepStore::new(Arc::new(clock.clone()));
        let mut w = SweepWorker::new(port(&c), store.clone(), 1);
        w.sweep_once();
        let mut src = HelperFirstProcesses::new(Box::new(Local(vec![])), store.clone());
        assert!(src.processes().is_empty(), "the call hung");
        clock.advance(Duration::from_secs(1));
        w.sweep_once();
        assert!(src.processes().is_empty(), "the helper is left alone for a while");
        src.retry_helper();
        w.sweep_once();
        assert_eq!(src.processes().len(), 1, "a person asked: it is asked at once");
    }

    /// Rule 48: what the sampling thread pays per tick for the merge, and the worker per sweep for the rates, at the sweep cap.
    /// Run by hand: `cargo test -p dyno-agent --release --lib measure_merge_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement, not a test (rule 48)"]
    fn measure_merge_cost() {
        let n = MAX_SWEEP as u32;
        let mk = |cpu: u64| -> RawSweep {
            RawSweep {
                at: Duration::from_secs(if cpu == 0 { 0 } else { 5 }),
                entries: (1..=n)
                    .map(|p| entry(p, OwnerKind::OtherUser, Some("sam"), cpu * u64::from(p % 7)))
                    .collect(),
                others: None,
                truncated: false,
                pages: MAX_PAGES,
            }
        };
        let (a, b) = (mk(0), mk(1_000));
        let t = std::time::Instant::now();
        let view = view_of(&b, Some(&a), 8);
        let rates = t.elapsed();
        let local: Vec<ProcessSample> = (1..=n / 2).map(|p| proc(p, None, "x", 1.0, 1)).collect();
        let t = std::time::Instant::now();
        let merged = merge(local, &view);
        let merging = t.elapsed();
        println!("entries={n} view_of={rates:?} merge={merging:?} rows={}", merged.len());
    }
}
