//! The disk map's engine as the agent runs it: volumes, scans as jobs with live status events, and the queries that
//! answer one level of a scan at a time. Read-only: nothing here changes a file (ADR 0021).

use super::engine::{End, LiveWorkers, RetryError, Snapshot, Walk, WalkConfig, WalkHooks};
use super::folder;
use super::volumes::{ListedVolume, VolumeLister};
use dyno_core::disk::index::{DiskChildren, DiskIndex, DiskRings, NodeId, ScanLimits, MAX_RINGS, MAX_RING_NODES};
use dyno_core::disk::scan::{
    eta_seconds, hidden_bytes, per_second, walk_threads, DiskScanStatus, DiskTarget, EventGate, ScanFailure, ScanPhase, ScanProgress,
    ScanState,
};
use dyno_core::disk::settings::MAX_READ_THREADS;
use dyno_core::disk::source::{DirSource, ReadError};
use dyno_core::disk::volume::{Volume, VolumeLocation};
use dyno_core::protocol::{AgentError, ErrorCode};
use dyno_core::threads::Threads;
use dyno_core::time::Timestamp;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Scans running at once.
pub const MAX_RUNNING: usize = 2;
/// Scans kept (running and finished); the oldest finished one goes first.
pub const MAX_KEPT: usize = 3;
/// The most nodes the kept scans may hold together, counting the room a new scan may take (the memory bound across scans).
pub const MAX_KEPT_NODES: usize = 8_000_000;
/// How long the first look at a scan's root may take before the start is refused (a dead share must not hold the request).
pub const STAT_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);
/// Threads of one server still inside calls that never returned before a new scan of it is refused.
pub const MAX_ABANDONED_PER_HOST: usize = 4;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A host's threads as the file finder needs them: which host, its live-thread count, and how many are stuck now.
pub struct HostThreads {
    /// The host as the disk map names it ("this computer", a server).
    pub host: String,
    /// Its live walk threads: the finder adds the threads of a cancelled search to it until they return.
    pub live: Arc<LiveWorkers>,
    /// Threads of it stuck in calls that never returned, at the moment of asking.
    pub abandoned: usize,
}

/// Where status events go.
pub trait StatusSink: Send + Sync {
    /// A scan's status changed.
    fn status(&self, status: DiskScanStatus);
    /// The volumes mounted now differ from the last listing (one came, one went, one stopped or started answering).
    fn volumes_changed(&self, _ticket: u64, _volumes: Vec<Volume>) {}
}

/// Whether the OS has given Dyno the right to read the whole disk (a privacy permission on macOS).
pub trait AccessProbe: Send + Sync {
    /// True when the permission is known to be missing; false when it is there or the OS has no such permission.
    fn full_disk_access_missing(&self) -> bool;
    /// Opens the system pane where the person gives it. `Unsupported` (with the reason) where there is no such pane.
    fn open_settings(&self) -> Result<(), dyno_core::ports::PlatformError>;
}

/// The real probe.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsAccess;

impl AccessProbe for OsAccess {
    fn full_disk_access_missing(&self) -> bool {
        dyno_platform::access::full_disk_access_missing()
    }

    fn open_settings(&self) -> Result<(), dyno_core::ports::PlatformError> {
        dyno_platform::access::open_full_disk_access_settings()
    }
}

/// Milliseconds on a clock that never goes back.
pub trait Clock: Send + Sync {
    /// Now.
    fn now_ms(&self) -> u64;
    /// The wall-clock time now, in milliseconds since 1970 (for "scanned at"; it may jump, so never used for rates).
    fn wall_ms(&self) -> i64;
}

/// The real clock.
pub struct SystemClock(std::time::Instant);

impl Default for SystemClock {
    fn default() -> Self {
        SystemClock(std::time::Instant::now())
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn wall_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}

struct Pub {
    gate: EventGate,
    seq: u64,
    state: ScanState,
    phase: ScanPhase,
    /// Where the pace is measured from: the pass's start on the clock, and the items and bytes already counted then.
    base: (u64, u64, u64),
    /// The first pass reads the whole tree, so only it can be estimated.
    first_pass: bool,
    scanned_at: Option<Timestamp>,
}

/// What a scan's events are made from; also the walk's hooks.
struct Meta {
    scan: u64,
    name: String,
    target: DiskTarget,
    volume: Option<Volume>,
    /// Whether Full Disk Access was missing when the scan began (asked once).
    fda_missing: bool,
    state: Mutex<Pub>,
    sink: Arc<dyn StatusSink>,
    clock: Arc<dyn Clock>,
}

impl Meta {
    fn build(&self, p: &Pub, snap: Snapshot) -> DiskScanStatus {
        let running = p.state == ScanState::Running;
        let elapsed = self.clock.now_ms().saturating_sub(p.base.0);
        DiskScanStatus {
            scan: self.scan,
            seq: p.seq,
            name: self.name.clone(),
            target: self.target.clone(),
            volume: self.volume.clone(),
            state: p.state.clone(),
            phase: p.phase,
            progress: snap.progress,
            truncated: snap.truncated,
            unreadable_first: snap.unreadable_first,
            needs_full_disk_access: snap.needs_full_disk_access,
            items_per_second: running
                .then(|| per_second(snap.progress.items.saturating_sub(p.base.1), elapsed))
                .flatten(),
            eta_seconds: (running && p.first_pass)
                .then(|| snap.progress.total_bytes.and_then(|t| eta_seconds(snap.progress.bytes, t, elapsed)))
                .flatten(),
            hidden_bytes: (p.state == ScanState::Done)
                .then(|| hidden_bytes(snap.progress.total_bytes, snap.progress.bytes))
                .flatten(),
            scanned_at: p.scanned_at,
        }
    }

    fn current(&self, walk: &Walk) -> DiskScanStatus {
        let p = lock(&self.state);
        self.build(&p, walk.snapshot())
    }

    /// The first event, before any folder is read: the volume's facts are known, nothing is counted yet.
    fn announce(&self, total_bytes: Option<u64>) {
        let snap = Snapshot {
            progress: ScanProgress {
                total_bytes,
                ..ScanProgress::default()
            },
            truncated: false,
            unreadable_first: Vec::new(),
            needs_full_disk_access: false,
        };
        let mut p = lock(&self.state);
        p.gate.allow(self.clock.now_ms());
        p.seq += 1;
        let s = self.build(&p, snap);
        self.sink.status(s);
        p.phase = ScanPhase::ReadingFolders;
    }

    /// The scan could not begin: say so, so no event stream is left hanging.
    fn failed_to_start(&self, reason: &str) {
        let mut p = lock(&self.state);
        p.state = ScanState::Failed {
            failure: ScanFailure::CannotStart { reason: reason.into() },
        };
        p.seq += 1;
        let s = self.build(&p, Snapshot::default());
        self.sink.status(s);
    }

    /// Sends the status with the next sequence number. The sequence is taken together with the snapshot, so a consumer
    /// that keeps the highest never shows numbers going back.
    fn publish(&self, walk: &Walk, only_if_due: bool) {
        let mut p = lock(&self.state);
        if only_if_due && !p.gate.allow(self.clock.now_ms()) {
            return;
        }
        p.seq += 1;
        let s = self.build(&p, walk.snapshot());
        self.sink.status(s);
    }
}

impl WalkHooks for Meta {
    fn progress(&self, walk: &Walk) {
        self.publish(walk, true);
    }

    fn ended(&self, walk: &Walk, end: &End) {
        {
            let mut p = lock(&self.state);
            p.state = match end {
                End::Done => ScanState::Done,
                End::Cancelled => ScanState::Cancelled,
                End::Failed(f) => ScanState::Failed { failure: f.clone() },
            };
            p.scanned_at = Some(Timestamp(self.clock.wall_ms()));
        }
        self.publish(walk, false);
        let (seq, pass_ms, first_pass) = {
            let p = lock(&self.state);
            (p.seq, self.clock.now_ms().saturating_sub(p.base.0), p.first_pass)
        };
        let outcome = match end {
            End::Done => "done",
            End::Cancelled => "cancelled",
            End::Failed(_) => "failed",
        };
        log::info!(Scan = self.scan, Outcome = outcome, Seq = seq, PassMs = pass_ms, Retry = !first_pass; "Disk scan pass ended");
    }

    fn restarted(&self, walk: &Walk) {
        let snap = walk.snapshot();
        {
            let mut p = lock(&self.state);
            p.state = ScanState::Running;
            p.first_pass = false;
            p.base = (self.clock.now_ms(), snap.progress.items, snap.progress.bytes);
        }
        self.publish(walk, false);
    }
}

struct Entry {
    meta: Arc<Meta>,
    walk: Arc<Walk>,
    key: String,
    root: PathBuf,
    host: String,
    threads: usize,
}

impl Entry {
    fn running(&self) -> bool {
        self.walk.outcome().is_none()
    }
}

struct Resolved {
    name: String,
    root: PathBuf,
    volume: Option<Volume>,
    location: VolumeLocation,
    rotational: Option<bool>,
    host: String,
    total_bytes: Option<u64>,
}

/// The disk map engine.
pub struct DiskCentre {
    source: Arc<dyn DirSource>,
    lister: Arc<dyn VolumeLister>,
    sink: Arc<dyn StatusSink>,
    clock: Arc<dyn Clock>,
    access: Arc<dyn AccessProbe>,
    cores: usize,
    limits: Mutex<ScanLimits>,
    /// The Setting `disk.readThreads`: Auto, or exactly this many threads read a scan.
    read_threads: Mutex<Threads>,
    /// The volume list as last asked for.
    known: Mutex<Option<Vec<ListedVolume>>>,
    /// Scans (and retries) that have a slot but are not kept yet: counted as running, so two starts cannot both take the last slot.
    starting: Mutex<HashSet<String>>,
    kept_nodes: AtomicUsize,
    refreshing: std::sync::atomic::AtomicBool,
    listing_ticket: AtomicU64,
    listing_applied: AtomicU64,
    next: AtomicU64,
    scans: Mutex<Vec<Arc<Entry>>>,
    live: Mutex<HashMap<String, Arc<LiveWorkers>>>,
}

fn err(code: ErrorCode, m: impl Into<String>) -> AgentError {
    AgentError::new(code, m)
}

impl DiskCentre {
    /// A centre over the given file system, volume list, event sink and clock.
    pub fn new(
        source: Arc<dyn DirSource>,
        lister: Arc<dyn VolumeLister>,
        sink: Arc<dyn StatusSink>,
        clock: Arc<dyn Clock>,
        access: Arc<dyn AccessProbe>,
        cores: usize,
        limits: ScanLimits,
    ) -> DiskCentre {
        DiskCentre {
            source,
            lister,
            sink,
            clock,
            access,
            cores,
            limits: Mutex::new(limits),
            read_threads: Mutex::new(Threads::Auto),
            known: Mutex::new(None),
            starting: Mutex::new(HashSet::new()),
            kept_nodes: AtomicUsize::new(MAX_KEPT_NODES),
            refreshing: std::sync::atomic::AtomicBool::new(false),
            listing_ticket: AtomicU64::new(0),
            listing_applied: AtomicU64::new(0),
            next: AtomicU64::new(1),
            scans: Mutex::new(Vec::new()),
            live: Mutex::new(HashMap::new()),
        }
    }

    /// The most nodes kept scans may hold together (tests lower it; the default is [`MAX_KEPT_NODES`]).
    pub fn set_kept_nodes(&self, nodes: usize) {
        self.kept_nodes.store(nodes, Ordering::Relaxed);
    }

    fn limits_now(&self) -> ScanLimits {
        (*lock(&self.limits)).validated()
    }

    /// The Setting `disk.readThreads` for the scans that start from now on (a running scan keeps its own).
    pub fn set_read_threads(&self, threads: Threads) {
        *lock(&self.read_threads) = threads;
    }

    /// New limits for the scans that start from now on (a running scan keeps its own).
    pub fn set_limits(&self, limits: ScanLimits) {
        *lock(&self.limits) = limits;
    }

    /// The volumes mounted now. Asks the system (slow on a Mac with a share that has gone away) and remembers the answer, which
    /// starting a scan then uses instead of asking again.
    pub fn volumes(self: &Arc<Self>) -> Vec<Volume> {
        let known = lock(&self.known).clone();
        match known {
            // The last answer now; the system is asked again in the background for the next call.
            Some(k) => {
                self.refresh_in_background();
                k.into_iter().map(|l| l.volume).collect()
            }
            None => self.refresh().into_iter().map(|l| l.volume).collect(),
        }
    }

    /// The volumes as last listed, without asking the system and without starting a listing on the caller's thread: `None`
    /// before the first listing has finished (one is started in the background). The popup reads this on every ask.
    pub fn cached_volumes(self: &Arc<Self>) -> Option<Vec<Volume>> {
        let known = lock(&self.known).clone();
        if known.is_none() {
            self.refresh_in_background();
        }
        known.map(|k| k.into_iter().map(|l| l.volume).collect())
    }

    /// Asks the system for the volumes on a thread of its own and remembers the answer; does nothing while such a call is
    /// already running. Never blocks the caller (the system can take seconds when a share has gone away).
    pub fn refresh_in_background(self: &Arc<Self>) {
        if self.refreshing.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        let spawned = std::thread::Builder::new().name("dyno-volumes".into()).spawn(move || {
            // The Disk map's volume list fills from it: live work (rule 52).
            dyno_platform::priority::set_current_thread(dyno_platform::priority::ThreadRole::Live);
            me.refresh();
            me.refreshing.store(false, Ordering::SeqCst);
        });
        if spawned.is_err() {
            self.refreshing.store(false, Ordering::SeqCst);
        }
    }

    /// Asks the lister and keeps the answer unless a later question has already been answered (two listings can overlap, and the
    /// older one may come back last). Tells the sink when the set of volumes differs from the one kept before.
    fn refresh(&self) -> Vec<ListedVolume> {
        let ticket = self.listing_ticket.fetch_add(1, Ordering::SeqCst) + 1;
        let listed = self.lister.list();
        let changed = {
            let mut known = lock(&self.known);
            if ticket <= self.listing_applied.load(Ordering::SeqCst) {
                return listed;
            }
            self.listing_applied.store(ticket, Ordering::SeqCst);
            let identity = |l: &Vec<ListedVolume>| l.iter().map(|v| (v.volume.id.clone(), v.volume.reachable)).collect::<Vec<_>>();
            let changed = known.as_ref().is_some_and(|old| identity(old) != identity(&listed));
            *known = Some(listed.clone());
            changed
        };
        if changed {
            self.sink.volumes_changed(ticket, listed.iter().map(|l| l.volume.clone()).collect());
        }
        listed
    }

    /// The volumes as last listed; listed now only when never listed (or `fresh`).
    fn listed(&self, fresh: bool) -> Vec<ListedVolume> {
        let known = lock(&self.known).clone();
        match known {
            Some(k) if !fresh => k,
            _ => self.refresh(),
        }
    }

    /// What kind of storage holds `path` and whether it spins, by the volume list as last asked for (internal and unknown when no
    /// volume holds it), with the number of logical cores: what decides how many threads may read it at once.
    pub fn storage_of(&self, path: &Path) -> (VolumeLocation, Option<bool>, usize) {
        let home = self
            .listed(false)
            .into_iter()
            .filter(|l| path.starts_with(&l.root) || path.starts_with(&l.volume.mount))
            .max_by_key(|l| l.volume.mount.len());
        home.map_or((VolumeLocation::Internal, None, self.cores), |l| {
            (l.volume.location, l.volume.rotational, self.cores)
        })
    }

    /// The host a path lives on, by the volume list as last asked for (this computer when no volume holds it).
    fn folder_host(&self, path: &str) -> String {
        self.listed(false)
            .into_iter()
            .filter(|l| Path::new(path).starts_with(&l.root) || Path::new(path).starts_with(&l.volume.mount))
            .max_by_key(|l| l.volume.mount.len())
            .map_or_else(|| host_of(&VolumeLocation::Internal), |l| host_of(&l.volume.location))
    }

    fn resolve(&self, target: &DiskTarget) -> Result<Resolved, AgentError> {
        match target {
            DiskTarget::Volume { id } => {
                let find = |l: Vec<ListedVolume>| l.into_iter().find(|l| &l.volume.id == id);
                // A volume the last listing did not know, or knew as not answering, is looked for once more.
                let ListedVolume { volume, root } = find(self.listed(false))
                    .filter(|l| l.volume.reachable)
                    .or_else(|| find(self.listed(true)))
                    .ok_or_else(|| err(ErrorCode::NotFound, "That volume is not mounted any more."))?;
                if !volume.reachable {
                    return Err(err(ErrorCode::Timeout, format!("{} is not answering.", volume.name)));
                }
                // The used space as of now, not as of the last listing, is what the progress is measured against.
                let listed = ListedVolume { volume, root };
                let used_now = self.lister.capacity_now(&listed).map(|(total, free)| total.saturating_sub(free));
                let ListedVolume { volume, root } = listed;
                Ok(Resolved {
                    name: volume.name.clone(),
                    root,
                    location: volume.location.clone(),
                    rotational: volume.rotational,
                    host: host_of(&volume.location),
                    total_bytes: volume.whole_volume.then(|| used_now.or_else(|| volume.used())).flatten(),
                    volume: Some(volume),
                })
            }
            DiskTarget::Folder { path } => {
                // Resolving the path touches the file system (a share that has gone away can hold the call): it gets the same bounded
                // treatment as the first look at the root. A thread that never returns counts against the host the path lives on (by
                // the volume list as last asked for), so a share that does not answer cannot hold back scans of this computer's disks.
                let host = self.folder_host(path);
                let (alive, p) = (lock(&self.live).entry(host).or_default().enter(), path.clone());
                // The start of the scan waits for it within a deadline: live work (rule 52).
                let root = super::volumes::within(STAT_LIMIT, dyno_platform::priority::ThreadRole::Live, move || {
                    let _alive = alive;
                    folder::check(&p)
                })
                .ok_or_else(|| err(ErrorCode::Timeout, "That folder did not answer in time."))?
                .map_err(|m| err(ErrorCode::BadRequest, m))?;
                let home = self
                    .listed(false)
                    .into_iter()
                    .filter(|l| root.starts_with(&l.root) || root.starts_with(&l.volume.mount))
                    .max_by_key(|l| l.volume.mount.len());
                let (location, rotational) = home.as_ref().map_or((VolumeLocation::Internal, None), |l| {
                    (l.volume.location.clone(), l.volume.rotational)
                });
                Ok(Resolved {
                    name: root
                        .file_name()
                        .map_or_else(|| root.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned()),
                    root,
                    host: host_of(&location),
                    location,
                    rotational,
                    total_bytes: None,
                    volume: None,
                })
            }
        }
    }

    /// Takes one of the running slots for `key`, or says why not. `Ok(Some(status))` is a scan of the same folder already running.
    fn reserve(&self, key: &str, host: &str, threads_needed: bool, incoming: u32) -> Result<Option<DiskScanStatus>, AgentError> {
        let scans = lock(&self.scans);
        let mut starting = lock(&self.starting);
        if let Some(e) = scans.iter().find(|e| e.key == key && e.running()) {
            return Ok(Some(e.meta.current(&e.walk)));
        }
        if starting.contains(key) {
            return Err(err(ErrorCode::Busy, "That scan is just starting."));
        }
        if scans.iter().filter(|e| e.running()).count() + starting.len() >= MAX_RUNNING {
            return Err(err(
                ErrorCode::Busy,
                "Two scans are already running. Stop one or wait for it to finish.",
            ));
        }
        // What running scans may still grow to, with this one, has to fit the budget even after every finished scan is dropped.
        let reserved: usize = scans
            .iter()
            .filter(|e| e.running())
            .map(|e| e.walk.with_index(|i| i.limits().max_nodes as usize))
            .sum::<usize>()
            + (starting.len() + usize::from(incoming > 0)) * (incoming as usize);
        if incoming > 0 && reserved > self.kept_nodes.load(Ordering::Relaxed) {
            return Err(err(
                ErrorCode::Busy,
                "There is not room for another scan next to the running ones. Finish or close a scan first.",
            ));
        }
        if threads_needed {
            // Threads of this host that are alive although no running scan owns them: stuck in calls that never returned.
            let abandoned = self.abandoned_among(&scans, host);
            if abandoned >= MAX_ABANDONED_PER_HOST {
                return Err(err(
                    ErrorCode::Busy,
                    format!("{host} still has {abandoned} reads that have not returned, so Dyno will not start another scan of it. Restarting Dyno clears it."),
                ));
            }
        }
        starting.insert(key.to_string());
        Ok(None)
    }

    /// Threads of `host` that are alive although no running scan owns them: stuck in calls that never returned (the disk map's walk
    /// and the file finder's cancelled walks both count here).
    fn abandoned_among(&self, scans: &[Arc<Entry>], host: &str) -> usize {
        let live = lock(&self.live).entry(host.to_string()).or_default().clone();
        let owned: usize = scans.iter().filter(|e| e.running() && e.host == host).map(|e| e.threads).sum();
        live.count().saturating_sub(owned)
    }

    /// For the file finder: the host the folder lives on, that host's live-thread count (a cancelled search whose threads are
    /// still inside a call that does not return stays in it until they do) and how many threads of it are stuck right now.
    pub fn host_threads(&self, path: &Path) -> HostThreads {
        let host = self.folder_host(&path.to_string_lossy());
        let live = lock(&self.live).entry(host.clone()).or_default().clone();
        let abandoned = self.abandoned_among(&lock(&self.scans), &host);
        HostThreads { host, live, abandoned }
    }

    fn release(&self, key: &str) {
        lock(&self.starting).remove(key);
    }

    /// Starts a scan, or returns the one already running for the same folder. Returns at once: the volume list is the one last
    /// asked for, and the slow parts (the access probe, the first look at the root) happen outside the lock on the scans.
    pub fn start(&self, target: DiskTarget) -> Result<DiskScanStatus, AgentError> {
        let r = self.resolve(&target)?;
        let key = format!("disk-scan:{}", r.root.display());
        if let Some(running) = self.reserve(&key, &r.host, true, self.limits_now().max_nodes)? {
            return Ok(running);
        }
        let started = self.begin(target, &r, &key);
        self.release(&key);
        started
    }

    fn begin(&self, target: DiskTarget, r: &Resolved, key: &str) -> Result<DiskScanStatus, AgentError> {
        let live = lock(&self.live).entry(r.host.clone()).or_default().clone();
        let meta = Arc::new(Meta {
            scan: self.next.fetch_add(1, Ordering::Relaxed),
            name: r.name.clone(),
            target,
            volume: r.volume.clone(),
            fda_missing: self.access.full_disk_access_missing(),
            state: Mutex::new(Pub {
                gate: EventGate::default(),
                seq: 0,
                state: ScanState::Running,
                phase: ScanPhase::ReadingVolumeFacts,
                base: (self.clock.now_ms(), 0, 0),
                first_pass: true,
                scanned_at: None,
            }),
            sink: self.sink.clone(),
            clock: self.clock.clone(),
        });
        let threads = lock(&self.read_threads).resolve(walk_threads(&r.location, r.rotational, self.cores), MAX_READ_THREADS);
        let limits = *lock(&self.limits);
        let cfg = WalkConfig {
            threads,
            limits,
            cross_volumes: false,
            total_bytes: r.total_bytes,
            live,
            spawn: super::engine::os_spawn,
            stat_limit: STAT_LIMIT,
            listing_progress_every: super::engine::LISTING_PROGRESS_EVERY,
            reasons: dyno_core::disk::reason::ReasonContext::current(meta.fda_missing),
        };
        meta.announce(r.total_bytes);
        let walk = Walk::start(self.source.clone(), r.root.clone(), &r.name, cfg, meta.clone()).map_err(|e| {
            let (code, message) = match e {
                ReadError::PermissionDenied | ReadError::NotPermitted => {
                    (ErrorCode::AccessDenied, "Dyno is not allowed to read that folder.".to_string())
                }
                ReadError::NotFound => (ErrorCode::NotFound, "That folder does not exist any more.".to_string()),
                ReadError::Unreachable(_) => (ErrorCode::Timeout, "That volume is not answering.".to_string()),
                ReadError::Other(m) => (ErrorCode::BadRequest, m),
            };
            meta.failed_to_start(&message);
            err(code, message)
        })?;
        let status = meta.current(&walk);
        let mut scans = lock(&self.scans);
        // Only now, with the new scan running, do old ones make room: a failed start loses nobody's results.
        self.evict(&mut scans, limits.max_nodes);
        scans.push(Arc::new(Entry {
            meta,
            walk,
            key: key.to_string(),
            root: r.root.clone(),
            host: r.host.clone(),
            threads,
        }));
        Ok(status)
    }

    /// Drops the oldest finished scans until there are at most [`MAX_KEPT`] with the new one, and the nodes they hold, with room
    /// for a new scan of up to `incoming` nodes, stay within [`MAX_KEPT_NODES`]. A running scan is never dropped.
    fn evict(&self, scans: &mut Vec<Arc<Entry>>, incoming: u32) {
        // A finished scan holds what it has; a running one may grow to its own limit, so that much is reserved for it.
        let nodes = |scans: &Vec<Arc<Entry>>| {
            scans
                .iter()
                .map(|e| {
                    if e.running() {
                        e.walk.with_index(|i| i.limits().max_nodes as usize)
                    } else {
                        e.walk.with_index(DiskIndex::len)
                    }
                })
                .sum::<usize>()
                + incoming as usize
        };
        while scans.len() >= MAX_KEPT || nodes(scans) > self.kept_nodes.load(Ordering::Relaxed) {
            match scans.iter().position(|e| !e.running()) {
                Some(old) => {
                    scans.remove(old);
                }
                None => break,
            }
        }
    }

    fn entry(&self, scan: u64) -> Result<Arc<Entry>, AgentError> {
        lock(&self.scans)
            .iter()
            .find(|e| e.meta.scan == scan)
            .cloned()
            .ok_or_else(|| err(ErrorCode::NotFound, "That scan is no longer kept. Scan again."))
    }

    /// Blocks until the scan's current pass has ended (tests wait on this instead of on events or time).
    pub fn wait_finished(&self, scan: u64) -> Result<(), AgentError> {
        self.entry(scan)?.walk.wait();
        Ok(())
    }

    /// Stops a scan and returns at once; what was counted stays. Stopping one that has ended does nothing.
    pub fn cancel(&self, scan: u64) -> Result<(), AgentError> {
        self.entry(scan)?.walk.cancel();
        Ok(())
    }

    /// Reads the folders a finished scan could not open once more, and only those, as the same scan: it runs again as a job
    /// (new events, same id) and the tree completes if nothing else was missing. Returns at once with the status.
    pub fn retry(&self, scan: u64) -> Result<DiskScanStatus, AgentError> {
        let e = self.entry(scan)?;
        let key = format!("disk-retry:{scan}");
        // The retry takes a running slot like a start does, so two of them cannot both take the last one.
        self.reserve(&key, &e.host, false, 0)?;
        let result = e.walk.retry();
        self.release(&key);
        result.map_err(|x| match x {
            RetryError::NotFinished => err(ErrorCode::Busy, "That scan has not finished. Wait for it, or scan again."),
            RetryError::NothingToRetry => err(
                ErrorCode::BadRequest,
                "Every folder of that scan was read; there is nothing to read again.",
            ),
            RetryError::NoThread => err(ErrorCode::Internal, "Dyno could not start a thread to read the folders again."),
        })?;
        // The pass runs on its own threads and may already have ended: the reply says so, and its sequence number places it among
        // the events (a consumer keeps the highest).
        let status = e.meta.current(&e.walk);
        log::info!(Scan = scan, State = state_name(&status.state), Seq = status.seq; "Disk scan retry started");
        Ok(status)
    }

    /// Opens the system pane for Full Disk Access.
    pub fn open_access_settings(&self) -> Result<(), AgentError> {
        self.access.open_settings().map_err(AgentError::from)
    }

    /// Every kept scan, newest last.
    pub fn scans(&self) -> Vec<DiskScanStatus> {
        lock(&self.scans).iter().map(|e| e.meta.current(&e.walk)).collect()
    }

    /// One level of a scan: a page of the children of `node` (the root when `None`), the small ones folded into "other".
    pub fn children(&self, scan: u64, node: Option<u32>, offset: u32, limit: u32, min_fraction: f64) -> Result<DiskChildren, AgentError> {
        if !(0.0..=1.0).contains(&min_fraction) {
            return Err(err(ErrorCode::BadRequest, "The smallest share shown must be between 0 and 1."));
        }
        let e = self.entry(scan)?;
        e.walk
            .with_index(|i| i.children(node.map_or_else(|| i.root(), NodeId), offset, limit, min_fraction))
            .map_err(|x| err(ErrorCode::NotFound, x.to_string()))
    }

    /// Several levels of a scan under `node` (the root when `None`) in one answer. The index is locked once, for one pass that
    /// visits at most `max_nodes` nodes and the children of the folders it opens.
    pub fn rings(&self, scan: u64, node: Option<u32>, rings: u8, min_share: f64, max_nodes: u32) -> Result<DiskRings, AgentError> {
        if !(1..=MAX_RINGS).contains(&rings) {
            return Err(err(ErrorCode::BadRequest, format!("Ask for 1 to {MAX_RINGS} rings.")));
        }
        if !(0.0..=1.0).contains(&min_share) {
            return Err(err(ErrorCode::BadRequest, "The smallest share shown must be between 0 and 1."));
        }
        if !(1..=MAX_RING_NODES).contains(&max_nodes) {
            return Err(err(ErrorCode::BadRequest, format!("Ask for 1 to {MAX_RING_NODES} nodes.")));
        }
        let e = self.entry(scan)?;
        e.walk
            .with_index(|i| i.rings(node.map_or_else(|| i.root(), NodeId), rings, min_share, max_nodes))
            .map_err(|x| err(ErrorCode::NotFound, x.to_string()))
    }

    /// The full path of a node as text (for Copy path; a name that is not text is shown with replacement characters).
    pub fn path(&self, scan: u64, node: u32) -> Result<String, AgentError> {
        self.path_buf(scan, node).map(|p| p.to_string_lossy().into_owned())
    }

    /// The exact path of a node (for Reveal).
    pub fn path_buf(&self, scan: u64, node: u32) -> Result<PathBuf, AgentError> {
        let e = self.entry(scan)?;
        let names = e
            .walk
            .with_index(|i| i.path_components(NodeId(node)))
            .map_err(|x| err(ErrorCode::NotFound, x.to_string()))?;
        // The root's own name is the scan's name, not part of the path: its path is the folder the scan started from.
        Ok(super::names::join_components(&e.root, &names))
    }
}

/// A state's name for the log (no paths or reasons: those can name the person's folders).
fn state_name(s: &ScanState) -> &'static str {
    match s {
        ScanState::Running => "running",
        ScanState::Done => "done",
        ScanState::Cancelled => "cancelled",
        ScanState::Failed { .. } => "failed",
    }
}

fn host_of(l: &VolumeLocation) -> String {
    match l {
        VolumeLocation::Network { host, .. } => host.clone(),
        _ => "this computer".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::engine::LiveWorkers;
    use crate::disk::fs::OsDirSource;
    use crate::disk::testing::{FakeFs, Gate};
    use dyno_core::disk::index::NodeKind;
    use dyno_core::disk::volume::NetworkProtocol;
    use std::path::Path;
    use std::sync::atomic::AtomicU64;

    const MB: u64 = 1024 * 1024;

    #[derive(Default)]
    struct Events(Mutex<Vec<DiskScanStatus>>, Mutex<Vec<Vec<Volume>>>);

    impl StatusSink for Events {
        fn status(&self, s: DiskScanStatus) {
            lock(&self.0).push(s);
        }

        fn volumes_changed(&self, _ticket: u64, v: Vec<Volume>) {
            lock(&self.1).push(v);
        }
    }

    impl Events {
        fn all(&self) -> Vec<DiskScanStatus> {
            lock(&self.0).clone()
        }
    }

    #[derive(Default)]
    struct Manual(AtomicU64);

    impl Clock for Manual {
        fn wall_ms(&self) -> i64 {
            1_700_000_000_000 + self.0.load(Ordering::SeqCst) as i64
        }
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    /// A fixed volume list that counts how often it was asked.
    struct Fixed(Mutex<Vec<ListedVolume>>, AtomicUsize, Mutex<Option<(u64, u64)>>);

    impl VolumeLister for Fixed {
        fn list(&self) -> Vec<ListedVolume> {
            self.1.fetch_add(1, Ordering::SeqCst);
            lock(&self.0).clone()
        }

        fn capacity_now(&self, _: &ListedVolume) -> Option<(u64, u64)> {
            *lock(&self.2)
        }
    }

    fn vol(id: &str, root: &Path, location: VolumeLocation) -> ListedVolume {
        let network = matches!(location, VolumeLocation::Network { .. });
        ListedVolume {
            volume: Volume {
                id: id.into(),
                name: id.into(),
                mount: root.to_string_lossy().into_owned(),
                fs: "apfs".into(),
                location,
                capacity: Some(1000 * MB),
                free: Some(400 * MB),
                rotational: Some(false),
                read_only: false,
                reachable: true,
                whole_volume: !network,
            },
            root: root.to_path_buf(),
        }
    }

    fn net(host: &str) -> VolumeLocation {
        VolumeLocation::Network {
            protocol: NetworkProtocol::Smb,
            host: host.into(),
        }
    }

    #[derive(Default)]
    struct Access(std::sync::atomic::AtomicBool);

    impl AccessProbe for Access {
        fn full_disk_access_missing(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }

        fn open_settings(&self) -> Result<(), dyno_core::ports::PlatformError> {
            if self.0.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(dyno_core::ports::PlatformError::Unsupported("not here".into()))
            }
        }
    }

    struct Rig {
        centre: Arc<DiskCentre>,
        events: Arc<Events>,
        clock: Arc<Manual>,
        access: Arc<Access>,
        lister: Arc<Fixed>,
    }

    fn rig(source: Arc<dyn DirSource>, volumes: Vec<ListedVolume>) -> Rig {
        let (events, clock, access) = (
            Arc::new(Events::default()),
            Arc::new(Manual::default()),
            Arc::new(Access::default()),
        );
        let lister = Arc::new(Fixed(Mutex::new(volumes), AtomicUsize::new(0), Mutex::new(None)));
        let centre = DiskCentre::new(
            source,
            lister.clone(),
            events.clone(),
            clock.clone(),
            access.clone(),
            8,
            ScanLimits {
                max_nodes: 10_000,
                notable_bytes: MB,
                max_hard_links: 100,
            },
        );
        Rig {
            centre: Arc::new(centre),
            events,
            clock,
            access,
            lister,
        }
    }

    /// The cached list is `None` before the first listing (the request itself never lists), starts one in the background and
    /// is the listed volumes afterwards.
    #[test]
    fn the_cached_volumes_never_list_on_the_request() {
        let fs = Arc::new(FakeFs::new());
        let r = rig(fs.clone(), vec![vol("Macintosh HD", &fs.root(), VolumeLocation::Internal)]);
        assert_eq!(
            r.centre.cached_volumes(),
            None,
            "the first call lists nothing on the caller's thread"
        );
        let began = std::time::Instant::now();
        let listed = loop {
            if let Some(v) = r.centre.cached_volumes() {
                break v;
            }
            assert!(
                began.elapsed() < std::time::Duration::from_secs(60),
                "the background listing never finished"
            );
            std::thread::yield_now();
        };
        assert_eq!(listed.len(), 1);
        let after = r.lister.1.load(Ordering::SeqCst);
        assert_eq!(r.centre.cached_volumes().map(|v| v.len()), Some(1));
        assert_eq!(
            r.lister.1.load(Ordering::SeqCst),
            after,
            "a cached read never asks the system again"
        );
    }

    /// root with a (5 MB + 100 B), b (3 MB), c (2 KB), loose 7 B.
    fn tree(fs: &FakeFs, root: &Path) {
        let a = fs.dir(root, "a");
        fs.file(&a, "big", 5 * MB);
        fs.file(&a, "tiny", 100);
        let b = fs.dir(root, "b");
        fs.file(&b, "mid", 3 * MB);
        let c = fs.dir(root, "c");
        fs.file(&c, "k", 2048);
        fs.file(root, "loose", 7);
    }

    fn final_status(r: &Rig, scan: u64) -> DiskScanStatus {
        r.events
            .all()
            .into_iter()
            .rev()
            .find(|s| s.scan == scan && s.state.is_final())
            .unwrap()
    }

    fn wait_final(r: &Rig, scan: u64) -> DiskScanStatus {
        let e = lock(&r.centre.scans).iter().find(|e| e.meta.scan == scan).cloned().unwrap();
        e.walk.wait();
        final_status(r, scan)
    }

    #[test]
    fn a_volume_scan_reports_phases_progress_and_ends_done() {
        let fs = Arc::new(FakeFs::new());
        tree(&fs, &fs.root());
        let r = rig(fs.clone(), vec![vol("Macintosh HD", &fs.root(), VolumeLocation::Internal)]);
        let started = r.centre.start(DiskTarget::Volume { id: "Macintosh HD".into() }).unwrap();
        let done = wait_final(&r, started.scan);
        assert_eq!(done.state, ScanState::Done);
        assert_eq!(done.progress.bytes, 8 * MB + 2048 + 107);
        assert_eq!(
            done.progress.total_bytes,
            Some(600 * MB),
            "a whole volume: used bytes are the scale"
        );
        assert_eq!(done.phase, ScanPhase::ReadingFolders);
        assert_eq!(done.name, "Macintosh HD");
        assert!(done.volume.is_some());
        let events = r.events.all();
        assert_eq!(events[0].phase, ScanPhase::ReadingVolumeFacts, "facts first, nothing counted");
        assert_eq!(events[0].progress.bytes, 0);
        assert_eq!(events[0].progress.total_bytes, Some(600 * MB));
        assert!(events.windows(2).all(|w| w[0].seq < w[1].seq), "sequence numbers only rise");
        assert!(
            events.windows(2).all(|w| w[0].progress.bytes <= w[1].progress.bytes),
            "bytes never go back"
        );
        assert!(events.iter().filter(|s| s.state.is_final()).count() == 1, "one final event");
    }

    #[test]
    fn events_are_at_most_a_few_per_second() {
        let fs = Arc::new(FakeFs::new());
        let root = fs.root();
        for i in 0..8 {
            let d = fs.dir(&root, format!("d{i}"));
            fs.file(&d, "x", 10);
        }
        let mut v = vol("V", &root, VolumeLocation::Internal);
        v.volume.rotational = Some(true);
        let r = rig(fs.clone(), vec![v]);
        // Time stands still: only the first and the final event may go out.
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        wait_final(&r, s.scan);
        assert_eq!(r.events.all().len(), 2);

        // Time moves 300 ms at every read: every folder is reported.
        let (clock, r2) = (r.clock.clone(), rig(fs.clone(), vec![vol("V", &root, VolumeLocation::Internal)]));
        let c2 = r2.clock.clone();
        fs.on_read(move |_| {
            c2.0.fetch_add(300, Ordering::SeqCst);
        });
        let _ = clock;
        let s = r2.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        wait_final(&r2, s.scan);
        assert!(r2.events.all().len() >= 5, "got {}", r2.events.all().len());
    }

    #[test]
    fn starting_the_same_scan_twice_returns_the_running_one() {
        let fs = Arc::new(FakeFs::new());
        tree(&fs, &fs.root());
        let gate = Gate::new();
        let g = gate.clone();
        fs.on_read(move |_| g.pass());
        let r = rig(fs.clone(), vec![vol("V", &fs.root(), VolumeLocation::Internal)]);
        let a = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        gate.wait_arrived(1);
        let b = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        assert_eq!(a.scan, b.scan);
        assert_eq!(r.centre.scans().len(), 1);
        gate.open();
        assert_eq!(wait_final(&r, a.scan).state, ScanState::Done);
        let again = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        assert_ne!(again.scan, a.scan, "a finished scan is scanned afresh");
    }

    #[test]
    fn cancel_ends_the_scan_at_once_and_keeps_the_partial_tree() {
        let fs = Arc::new(FakeFs::new());
        tree(&fs, &fs.root());
        let gate = Gate::new();
        let g = gate.clone();
        fs.on_read(move |p| {
            if p.ends_with("a") || p.ends_with("b") || p.ends_with("c") {
                g.pass();
            }
        });
        let mut v = vol("V", &fs.root(), VolumeLocation::Internal);
        v.volume.rotational = Some(true);
        let r = rig(fs.clone(), vec![v]);
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        gate.wait_arrived(1);
        r.centre.cancel(s.scan).unwrap();
        let st = final_status(&r, s.scan);
        assert_eq!(st.state, ScanState::Cancelled);
        assert_eq!(st.progress.bytes, 7, "only the root's own file was counted");
        let page = r.centre.children(s.scan, None, 0, 10, 0.0).unwrap();
        assert!(!page.node.complete);
        assert_eq!(page.loose.bytes, 7);
        assert_eq!(page.children.len(), 3, "a, b and c are listed, unfinished");
        assert!(page.children.iter().all(|c| !c.complete));
        r.centre.cancel(s.scan).unwrap();
        gate.open();
        let lw = lock(&r.centre.live).values().next().unwrap().clone();
        lw.wait_zero();
        assert_eq!(r.centre.scans()[0].progress.bytes, 7, "late answers change nothing");
        assert_eq!(r.events.all().iter().filter(|s| s.state.is_final()).count(), 1);
    }

    #[test]
    fn a_dropped_share_is_a_failure_with_its_cause() {
        let fs = Arc::new(FakeFs::new());
        let root = fs.root();
        let a = fs.dir(&root, "a");
        fs.fail_open(&a, ReadError::Unreachable("ETIMEDOUT".into()));
        fs.drop_volume_at(&a);
        let r = rig(fs.clone(), vec![vol("nas", &root, net("nas.local"))]);
        let s = r.centre.start(DiskTarget::Volume { id: "nas".into() }).unwrap();
        let st = wait_final(&r, s.scan);
        assert_eq!(
            st.state,
            ScanState::Failed {
                failure: dyno_core::disk::scan::ScanFailure::VolumeUnreachable {
                    detail: "ETIMEDOUT".into()
                }
            }
        );
        assert_eq!(st.progress.total_bytes, None, "a share has no known total: the count just grows");
        assert!(r.centre.children(s.scan, None, 0, 10, 0.0).is_ok(), "the partial tree is kept");
    }

    #[test]
    fn only_two_scans_run_at_once() {
        let fs = Arc::new(FakeFs::new());
        let root = fs.root();
        let names = ["v1", "v2", "v3"];
        let mut vols = Vec::new();
        for n in names {
            let d = fs.dir(&root, n);
            fs.file(&d, "f", 1);
            vols.push(vol(n, &d, VolumeLocation::Internal));
        }
        let gate = Gate::new();
        let g = gate.clone();
        fs.on_read(move |_| g.pass());
        let r = rig(fs.clone(), vols);
        let a = r.centre.start(DiskTarget::Volume { id: "v1".into() }).unwrap();
        let b = r.centre.start(DiskTarget::Volume { id: "v2".into() }).unwrap();
        let busy = r.centre.start(DiskTarget::Volume { id: "v3".into() }).unwrap_err();
        assert_eq!(busy.code, ErrorCode::Busy);
        gate.open();
        wait_final(&r, a.scan);
        wait_final(&r, b.scan);
        assert!(r.centre.start(DiskTarget::Volume { id: "v3".into() }).is_ok());
    }

    #[test]
    fn only_the_last_three_scans_are_kept() {
        let fs = Arc::new(FakeFs::new());
        fs.file(&fs.root(), "f", 1);
        let r = rig(fs.clone(), vec![vol("V", &fs.root(), VolumeLocation::Internal)]);
        let mut ids = Vec::new();
        for _ in 0..4 {
            let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
            wait_final(&r, s.scan);
            ids.push(s.scan);
        }
        let kept: Vec<u64> = r.centre.scans().iter().map(|s| s.scan).collect();
        assert_eq!(kept, ids[1..], "the oldest finished scan was dropped");
        assert_eq!(r.centre.children(ids[0], None, 0, 1, 0.0).unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn threads_stuck_on_one_server_stop_further_scans_of_it() {
        let fs = Arc::new(FakeFs::new());
        let root = fs.root();
        let mut vols = Vec::new();
        for n in ["s1", "s2", "s3"] {
            let d = fs.dir(&root, n);
            for k in 0..2 {
                let sub = fs.dir(&d, format!("d{k}"));
                fs.file(&sub, "f", 1);
            }
            vols.push(vol(n, &d, net("nas.local")));
        }
        let gate = Gate::new();
        let g = gate.clone();
        fs.on_read(move |p| {
            if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('d')) {
                g.pass();
            }
        });
        let r = rig(fs.clone(), vols);
        let a = r.centre.start(DiskTarget::Volume { id: "s1".into() }).unwrap();
        gate.wait_arrived(2);
        r.centre.cancel(a.scan).unwrap();
        let b = r.centre.start(DiskTarget::Volume { id: "s2".into() }).unwrap();
        gate.wait_arrived(4);
        r.centre.cancel(b.scan).unwrap();
        let refused = r.centre.start(DiskTarget::Volume { id: "s3".into() }).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Busy);
        assert!(refused.message.contains("nas.local") && refused.message.contains("4 reads"));
        gate.open();
        let lw = lock(&r.centre.live).get("nas.local").unwrap().clone();
        lw.wait_zero();
        assert!(
            r.centre.start(DiskTarget::Volume { id: "s3".into() }).is_ok(),
            "once the reads return, scans start again"
        );
    }

    #[test]
    fn a_folder_the_person_chose_is_scanned_like_a_volume_without_a_scale() {
        let t = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(t.path()).unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/f"), vec![1u8; 30_000]).unwrap();
        let r = rig(
            Arc::new(OsDirSource),
            vec![vol("V", Path::new("/definitely-not-here"), VolumeLocation::Internal)],
        );
        let s = r
            .centre
            .start(DiskTarget::Folder {
                path: dir.to_string_lossy().into_owned(),
            })
            .unwrap();
        let done = wait_final(&r, s.scan);
        assert_eq!(done.state, ScanState::Done);
        assert_eq!(done.progress.total_bytes, None);
        assert!(done.volume.is_none());
        assert!(done.progress.bytes >= 30_000);
        let sub = r.centre.children(s.scan, None, 0, 10, 0.0).unwrap().children[0].clone();
        assert_eq!(sub.name, "sub");
        assert_eq!(PathBuf::from(r.centre.path(s.scan, sub.id).unwrap()), dir.join("sub"));
        assert_eq!(PathBuf::from(r.centre.path(s.scan, 0).unwrap()), dir);
        assert_eq!(r.centre.path(s.scan, 99).unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn a_folder_inside_a_volume_takes_the_threads_of_that_volume() {
        let t = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(t.path()).unwrap();
        let mut hdd = vol("HDD", &dir, VolumeLocation::External { bus: Some("USB".into()) });
        hdd.volume.rotational = Some(true);
        let r = rig(
            Arc::new(OsDirSource),
            vec![vol("Other", Path::new("/zzz"), VolumeLocation::Internal), hdd],
        );
        let s = r
            .centre
            .start(DiskTarget::Folder {
                path: dir.to_string_lossy().into_owned(),
            })
            .unwrap();
        wait_final(&r, s.scan);
        let e = lock(&r.centre.scans)[0].clone();
        assert_eq!(e.threads, 1, "a spinning disk is read by one thread");
        assert_eq!(e.host, "this computer");
    }

    #[test]
    fn the_read_threads_setting_replaces_the_volume_table_and_is_validated() {
        let t = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(t.path()).unwrap();
        let mut hdd = vol("HDD", &dir, VolumeLocation::External { bus: Some("USB".into()) });
        hdd.volume.rotational = Some(true);
        let r = rig(Arc::new(OsDirSource), vec![hdd]);
        let scan = |r: &Rig| {
            let s = r
                .centre
                .start(DiskTarget::Folder {
                    path: dir.to_string_lossy().into_owned(),
                })
                .unwrap();
            wait_final(r, s.scan);
            lock(&r.centre.scans).last().unwrap().threads
        };
        assert_eq!(scan(&r), 1, "Auto: a spinning disk is read by one thread");
        r.centre.set_read_threads(Threads::Fixed(3));
        assert_eq!(scan(&r), 3, "the setting wins over the table");
        r.centre.set_read_threads(Threads::Fixed(200));
        assert_eq!(scan(&r), usize::from(MAX_READ_THREADS), "and is held to its own most");
        r.centre.set_read_threads(Threads::Auto);
        assert_eq!(scan(&r), 1);
    }

    #[test]
    fn bad_requests_are_refused_with_a_reason() {
        let fs = Arc::new(FakeFs::new());
        let mut down = vol("down", &fs.root(), net("nas.local"));
        down.volume.reachable = false;
        let r = rig(fs.clone(), vec![down, vol("V", &fs.root().join("gone"), VolumeLocation::Internal)]);
        let code = |t: DiskTarget| r.centre.start(t).unwrap_err().code;
        assert_eq!(code(DiskTarget::Volume { id: "nope".into() }), ErrorCode::NotFound);
        assert_eq!(code(DiskTarget::Volume { id: "down".into() }), ErrorCode::Timeout);
        assert_eq!(code(DiskTarget::Folder { path: "relative".into() }), ErrorCode::BadRequest);
        assert_eq!(
            code(DiskTarget::Volume { id: "V".into() }),
            ErrorCode::NotFound,
            "a root that vanished"
        );
        assert_eq!(r.centre.cancel(42).unwrap_err().code, ErrorCode::NotFound);
        assert_eq!(r.centre.children(42, None, 0, 1, 0.0).unwrap_err().code, ErrorCode::NotFound);
        assert_eq!(r.centre.path(42, 0).unwrap_err().code, ErrorCode::NotFound);
        let failed = r.events.all();
        assert!(
            matches!(
                failed.last().unwrap().state,
                ScanState::Failed {
                    failure: dyno_core::disk::scan::ScanFailure::CannotStart { .. }
                }
            ),
            "a scan that could not begin is closed with an event"
        );
    }

    #[test]
    fn every_start_failure_has_its_own_code() {
        let fs = Arc::new(FakeFs::new());
        let root = fs.root();
        for (name, e, code) in [
            ("denied", ReadError::PermissionDenied, ErrorCode::AccessDenied),
            ("gone", ReadError::NotFound, ErrorCode::NotFound),
            ("dead", ReadError::Unreachable("x".into()), ErrorCode::Timeout),
            ("odd", ReadError::Other("strange".into()), ErrorCode::BadRequest),
        ] {
            let d = fs.dir(&root, name);
            fs.fail_stat(&d, e);
            let r = rig(fs.clone(), vec![vol(name, &d, VolumeLocation::Internal)]);
            assert_eq!(
                r.centre.start(DiskTarget::Volume { id: name.into() }).unwrap_err().code,
                code,
                "{name}"
            );
        }
        let file = {
            fs.file(&root, "plain", 1);
            root.join("plain")
        };
        let r = rig(fs.clone(), vec![vol("file", &file, VolumeLocation::Internal)]);
        assert_eq!(
            r.centre.start(DiskTarget::Volume { id: "file".into() }).unwrap_err().code,
            ErrorCode::BadRequest
        );
    }

    #[test]
    fn drilling_down_returns_one_level_and_an_other_bucket() {
        let fs = Arc::new(FakeFs::new());
        tree(&fs, &fs.root());
        let r = rig(fs.clone(), vec![vol("V", &fs.root(), VolumeLocation::Internal)]);
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        wait_final(&r, s.scan);
        let top = r.centre.children(s.scan, None, 0, 10, 0.1).unwrap();
        assert_eq!(top.children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(top.other.count, 1, "c is under 10 percent of the volume");
        assert_eq!(top.other.bytes, 2048);
        assert_eq!(top.loose.bytes, 7);
        let a = &top.children[0];
        let level2 = r.centre.children(s.scan, Some(a.id), 0, 10, 0.0).unwrap();
        assert_eq!(level2.children.len(), 1);
        assert_eq!(level2.children[0].kind, NodeKind::File);
        assert_eq!(level2.loose.bytes, 100);
        assert_eq!(r.centre.children(s.scan, None, 0, 10, 2.0).unwrap_err().code, ErrorCode::BadRequest);
        assert_eq!(
            r.centre.children(s.scan, None, 0, 10, f64::NAN).unwrap_err().code,
            ErrorCode::BadRequest
        );
        assert_eq!(
            r.centre.children(s.scan, Some(9999), 0, 10, 0.0).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(r.centre.volumes().len(), 1);
    }

    #[test]
    fn new_limits_apply_to_the_next_scan() {
        let fs = Arc::new(FakeFs::new());
        let root = fs.root();
        for i in 0..6 {
            let d = fs.dir(&root, format!("d{i}"));
            fs.file(&d, "x", 1);
        }
        let r = rig(fs.clone(), vec![vol("V", &root, VolumeLocation::Internal)]);
        r.centre.set_limits(ScanLimits {
            max_nodes: 3,
            notable_bytes: MB,
            max_hard_links: 1,
        });
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        assert!(wait_final(&r, s.scan).truncated, "the node cap of the setting was used");
    }

    /// root: ok (30 B), Library (refused until allowed; 5 MB inside).
    fn refused_library() -> (Arc<FakeFs>, PathBuf) {
        let fs = Arc::new(FakeFs::new());
        let root = fs.root();
        let ok = fs.dir(&root, "ok");
        fs.file(&ok, "x", 30);
        let lib = fs.dir(&root, "Library");
        fs.file(&lib, "big", 5 * MB);
        // What macOS answers when privacy protection refuses: EPERM.
        fs.fail_open(&lib, ReadError::NotPermitted);
        (fs, lib)
    }

    // macOS only: the scan asks this machine's OS how to read a refusal; the rules per OS are tested in `dyno_core::disk::reason`.
    #[cfg(target_os = "macos")]
    #[test]
    fn missing_full_disk_access_is_named_only_when_folders_were_refused() {
        let (fs, _) = refused_library();
        let root = fs.root();
        let r = rig(fs.clone(), vec![vol("V", &root, VolumeLocation::Internal)]);
        r.access.0.store(true, Ordering::SeqCst);
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        let done = wait_final(&r, s.scan);
        assert_eq!(done.progress.unreadable, 1);
        assert!(done.needs_full_disk_access, "refused folders and the permission is missing");
        assert!(!r.events.all()[0].needs_full_disk_access, "nothing was refused yet");

        r.access.0.store(false, Ordering::SeqCst);
        let again = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        assert!(
            !wait_final(&r, again.scan).needs_full_disk_access,
            "refused for another reason: no claim"
        );

        let clean = Arc::new(FakeFs::new());
        tree(&clean, &clean.root());
        let r = rig(clean.clone(), vec![vol("V", &clean.root(), VolumeLocation::Internal)]);
        r.access.0.store(true, Ordering::SeqCst);
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        assert!(!wait_final(&r, s.scan).needs_full_disk_access, "nothing refused: nothing to say");
    }

    #[test]
    fn a_retry_runs_the_same_scan_again_over_the_refused_folders_only() {
        let (fs, lib) = refused_library();
        let root = fs.root();
        let r = rig(fs.clone(), vec![vol("V", &root, VolumeLocation::Internal)]);
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        let first = wait_final(&r, s.scan);
        assert_eq!((first.progress.bytes, first.progress.unreadable), (30, 1));
        let reads = fs.reads.load(Ordering::SeqCst);

        fs.allow_open(&lib);
        // The pass is held at its one read until the reply is in: a pass this small can end before the reply is built, and
        // then the reply is honestly Done (see the next test).
        let gate = Gate::new();
        let g = gate.clone();
        fs.on_read(move |_| g.pass());
        let again = r.centre.retry(s.scan).unwrap();
        assert_eq!((again.scan, again.state.clone()), (s.scan, ScanState::Running));
        assert_eq!(again.seq, first.seq + 1, "the reply is the status the restart announced");
        gate.open();
        let second = wait_final(&r, s.scan);
        assert_eq!(second.state, ScanState::Done);
        assert_eq!((second.progress.bytes, second.progress.unreadable), (30 + 5 * MB, 0));
        assert_eq!(fs.reads.load(Ordering::SeqCst), reads + 1, "only Library was read");
        assert!(second.seq > again.seq);
        let page = r.centre.children(s.scan, None, 0, 10, 0.0).unwrap();
        assert!(page.node.complete);
    }

    #[test]
    fn the_reply_to_a_retry_is_running_or_done_with_what_was_read_and_never_newer_than_the_end() {
        // Whether the pass ends before or after the reply is built depends on the scheduler; either way the reply is true, and
        // its sequence number lies between the restart and the end, so a consumer that keeps the highest number ends on Done.
        let (fs, lib) = refused_library();
        let root = fs.root();
        let r = rig(fs.clone(), vec![vol("V", &root, VolumeLocation::Internal)]);
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        let first = wait_final(&r, s.scan);
        fs.allow_open(&lib);
        let again = r.centre.retry(s.scan).unwrap();
        let second = wait_final(&r, s.scan);
        assert_eq!(second.state, ScanState::Done);
        assert!(again.seq > first.seq && again.seq <= second.seq);
        match again.state {
            ScanState::Running => assert!(again.seq < second.seq, "a running reply is followed by the end"),
            ScanState::Done => assert_eq!(
                (again.progress.bytes, again.progress.unreadable),
                (30 + 5 * MB, 0),
                "done means read"
            ),
            other => panic!("unexpected state {other:?}"),
        }
    }

    #[test]
    fn a_retry_is_refused_when_there_is_nothing_to_retry_or_the_scan_is_not_finished() {
        let fs = Arc::new(FakeFs::new());
        tree(&fs, &fs.root());
        let gate = Gate::new();
        let g = gate.clone();
        fs.on_read(move |_| g.pass());
        let r = rig(fs.clone(), vec![vol("V", &fs.root(), VolumeLocation::Internal)]);
        assert_eq!(r.centre.retry(99).unwrap_err().code, ErrorCode::NotFound);
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        gate.wait_arrived(1);
        assert_eq!(r.centre.retry(s.scan).unwrap_err().code, ErrorCode::Busy, "still running");
        gate.open();
        wait_final(&r, s.scan);
        assert_eq!(
            r.centre.retry(s.scan).unwrap_err().code,
            ErrorCode::BadRequest,
            "every folder was read"
        );
    }

    #[test]
    fn a_retry_waits_when_two_scans_are_running() {
        let (fs, _) = refused_library();
        let root = fs.root();
        let (w, x) = (fs.dir(&root, "other"), fs.dir(&root, "other2"));
        let r = rig(
            fs.clone(),
            vec![
                vol("V", &root, VolumeLocation::Internal),
                vol("W", &w, VolumeLocation::Internal),
                vol("X", &x, VolumeLocation::Internal),
            ],
        );
        let first = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        wait_final(&r, first.scan);
        let gate = Gate::new();
        let g = gate.clone();
        fs.on_read(move |_| g.pass());
        let a = r.centre.start(DiskTarget::Volume { id: "W".into() }).unwrap();
        let b = r.centre.start(DiskTarget::Volume { id: "X".into() }).unwrap();
        gate.wait_arrived(2);
        assert_eq!(r.centre.retry(first.scan).unwrap_err().code, ErrorCode::Busy);
        gate.open();
        wait_final(&r, a.scan);
        wait_final(&r, b.scan);
    }

    #[test]
    fn the_settings_pane_is_opened_through_the_probe_and_its_refusal_is_typed() {
        let fs = Arc::new(FakeFs::new());
        let r = rig(fs, vec![]);
        r.access.0.store(true, Ordering::SeqCst);
        assert!(r.centre.open_access_settings().is_ok());
        r.access.0.store(false, Ordering::SeqCst);
        assert_eq!(r.centre.open_access_settings().unwrap_err().code, ErrorCode::Unsupported);
    }

    #[test]
    fn the_path_of_the_root_is_the_folder_the_scan_started_from() {
        let fs = Arc::new(FakeFs::new());
        tree(&fs, &fs.root());
        let r = rig(fs.clone(), vec![vol("V", &fs.root(), VolumeLocation::Internal)]);
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        wait_final(&r, s.scan);
        let root = r.centre.path(s.scan, 0).unwrap();
        assert_eq!(PathBuf::from(&root), fs.root(), "never empty");
        let top = r.centre.children(s.scan, None, 0, 10, 0.0).unwrap();
        let child = r.centre.path(s.scan, top.children[0].id).unwrap();
        assert_eq!(PathBuf::from(child), fs.root().join(&top.children[0].name));
    }

    #[test]
    fn rate_estimate_hidden_space_and_scan_time_are_given_only_when_they_can_be_known() {
        let fs = Arc::new(FakeFs::new());
        let root = fs.root();
        let a = fs.dir(&root, "a");
        let b = fs.dir(&root, "b");
        fs.file(&a, "x", 100 * MB);
        fs.file(&b, "y", 100 * MB);
        let r = rig(fs.clone(), vec![vol("V", &root, VolumeLocation::Internal)]);
        // Time passes 3 s at every folder read: from the second folder on, a pace is known.
        let c = r.clock.clone();
        fs.on_read(move |_| {
            c.0.fetch_add(3_000, Ordering::SeqCst);
        });
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        // The first event is sent before anything is read: nothing to say yet. (The status returned by `start` may already be
        // later, because the walk runs on its own threads.)
        let first = r.events.all().remove(0);
        assert_eq!(
            (first.items_per_second, first.eta_seconds, first.hidden_bytes, first.scanned_at),
            (None, None, None, None)
        );
        let done = wait_final(&r, s.scan);
        assert_eq!(done.items_per_second, None, "a finished scan has no pace");
        assert_eq!(done.eta_seconds, None);
        assert_eq!(done.hidden_bytes, Some(600 * MB - 200 * MB), "used minus counted");
        assert_eq!(
            done.scanned_at,
            Some(Timestamp(1_700_000_000_000 + r.clock.0.load(Ordering::SeqCst) as i64))
        );
        let mid: Vec<_> = r
            .events
            .all()
            .into_iter()
            .filter(|e| e.state == ScanState::Running && e.progress.bytes > 0)
            .collect();
        assert!(
            mid.iter().any(|e| e.items_per_second.is_some()),
            "a pace once enough time has passed"
        );
        assert!(mid.iter().any(|e| e.eta_seconds.is_some()), "an estimate for a whole volume");
        assert!(mid.iter().all(|e| e.hidden_bytes.is_none() && e.scanned_at.is_none()));
    }

    #[test]
    fn a_retry_has_a_pace_of_its_own_and_no_estimate() {
        let (fs, lib) = refused_library();
        let root = fs.root();
        let r = rig(fs.clone(), vec![vol("V", &root, VolumeLocation::Internal)]);
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        let first = wait_final(&r, s.scan);
        fs.allow_open(&lib);
        let c = r.clock.clone();
        fs.on_read(move |_| {
            c.0.fetch_add(5_000, Ordering::SeqCst);
        });
        r.centre.retry(s.scan).unwrap();
        let second = wait_final(&r, s.scan);
        assert!(second.scanned_at > first.scanned_at, "the latest pass");
        let during: Vec<_> = r
            .events
            .all()
            .into_iter()
            .filter(|e| e.seq > first.seq && e.state == ScanState::Running)
            .collect();
        assert!(during.iter().all(|e| e.eta_seconds.is_none()), "no estimate for a retry");
    }

    #[test]
    fn starting_a_scan_uses_the_volume_list_already_asked_for() {
        let fs = Arc::new(FakeFs::new());
        tree(&fs, &fs.root());
        let r = rig(fs.clone(), vec![vol("V", &fs.root(), VolumeLocation::Internal)]);
        assert_eq!(r.centre.volumes().len(), 1);
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        wait_final(&r, s.scan);
        assert_eq!(
            r.lister.1.load(Ordering::SeqCst),
            1,
            "the system was asked once, not again by the start"
        );
        // A volume the list did not know is looked for once more; one that is still unknown is not found.
        assert_eq!(
            r.centre.start(DiskTarget::Volume { id: "gone".into() }).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(r.lister.1.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_known_volume_list_is_answered_at_once_and_refreshed_in_the_background() {
        let fs = Arc::new(FakeFs::new());
        let r = rig(fs.clone(), vec![vol("V", &fs.root(), VolumeLocation::Internal)]);
        assert_eq!(r.centre.volumes().len(), 1);
        assert_eq!(r.lister.1.load(Ordering::SeqCst), 1, "never listed: asked now");
        lock(&r.lister.0).push(vol("W", &fs.root(), VolumeLocation::Internal));
        assert_eq!(
            r.centre.volumes().len(),
            1,
            "the answer of the last listing, without waiting for the system"
        );
        while r.centre.refreshing.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        assert_eq!(r.lister.1.load(Ordering::SeqCst), 2);
        assert_eq!(r.centre.volumes().len(), 2, "the background listing arrived");
    }

    #[test]
    fn a_start_held_at_the_first_look_does_not_block_the_other_requests() {
        let fs = Arc::new(FakeFs::new());
        tree(&fs, &fs.root());
        let gate = Gate::new();
        let g = gate.clone();
        fs.on_stat(move |_| g.pass());
        let r = Arc::new(rig(fs.clone(), vec![vol("V", &fs.root(), VolumeLocation::Internal)]));
        let starting = r.clone();
        let t = std::thread::spawn(move || starting.centre.start(DiskTarget::Volume { id: "V".into() }));
        gate.wait_arrived(1);
        // The start is inside the first look at the root: everything else still answers.
        assert!(r.centre.scans().is_empty());
        assert_eq!(r.centre.retry(1).unwrap_err().code, ErrorCode::NotFound);
        let second = r.centre.start(DiskTarget::Volume { id: "V".into() });
        assert_eq!(second.unwrap_err().code, ErrorCode::Busy, "the same scan is just starting");
        gate.open();
        let s = t.join().unwrap().unwrap();
        wait_final(&r, s.scan);
    }

    #[test]
    fn a_start_that_fails_keeps_every_finished_scan_and_a_good_one_makes_room() {
        let fs = Arc::new(FakeFs::new());
        let root = fs.root();
        let dirs: Vec<PathBuf> = ["a", "b", "c", "d"].iter().map(|n| fs.dir(&root, n)).collect();
        let r = rig(
            fs.clone(),
            dirs.iter()
                .enumerate()
                .map(|(i, d)| vol(&format!("V{i}"), d, VolumeLocation::Internal))
                .chain([vol("Dead", Path::new("fake-root/missing"), VolumeLocation::Internal)])
                .collect(),
        );
        for i in 0..MAX_KEPT {
            let s = r.centre.start(DiskTarget::Volume { id: format!("V{i}") }).unwrap();
            wait_final(&r, s.scan);
        }
        assert_eq!(r.centre.scans().len(), MAX_KEPT);
        assert_eq!(
            r.centre.start(DiskTarget::Volume { id: "Dead".into() }).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(r.centre.scans().len(), MAX_KEPT, "a start that failed took nobody's results");
        let s = r.centre.start(DiskTarget::Volume { id: "V3".into() }).unwrap();
        wait_final(&r, s.scan);
        let kept: Vec<u64> = r.centre.scans().iter().map(|s| s.scan).collect();
        assert_eq!(kept.len(), MAX_KEPT);
        assert!(kept.contains(&s.scan) && !kept.contains(&1), "the oldest made room");
    }

    #[test]
    fn kept_scans_together_are_held_to_a_node_budget() {
        let fs = Arc::new(FakeFs::new());
        let root = fs.root();
        let (a, b) = (fs.dir(&root, "a"), fs.dir(&root, "b"));
        for d in [&a, &b] {
            for n in 0..5 {
                fs.dir(d, format!("d{n}"));
            }
        }
        let r = rig(
            fs.clone(),
            vec![vol("A", &a, VolumeLocation::Internal), vol("B", &b, VolumeLocation::Internal)],
        );
        // A kept scan holds 6 nodes and a new one may take 10_000: with a budget of 10_003 nothing fits next to it, so the older goes.
        r.centre.set_kept_nodes(10_003);
        let first = r.centre.start(DiskTarget::Volume { id: "A".into() }).unwrap();
        wait_final(&r, first.scan);
        let second = r.centre.start(DiskTarget::Volume { id: "B".into() }).unwrap();
        wait_final(&r, second.scan);
        let kept: Vec<u64> = r.centre.scans().iter().map(|s| s.scan).collect();
        assert_eq!(kept, vec![second.scan]);
    }

    #[test]
    fn a_change_in_the_volumes_is_announced_but_not_a_change_in_free_space() {
        let fs = Arc::new(FakeFs::new());
        let r = rig(fs.clone(), vec![vol("V", &fs.root(), VolumeLocation::Internal)]);
        r.centre.refresh();
        assert!(lock(&r.events.1).is_empty(), "the first listing is not a change");
        lock(&r.lister.0)[0].volume.free = Some(1);
        r.centre.refresh();
        assert!(lock(&r.events.1).is_empty(), "free space moves all the time");
        lock(&r.lister.0).push(vol("W", &fs.root(), VolumeLocation::External { bus: None }));
        r.centre.refresh();
        let seen = lock(&r.events.1).clone();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["V", "W"]);
        lock(&r.lister.0)[1].volume.reachable = false;
        r.centre.refresh();
        assert_eq!(lock(&r.events.1).len(), 2, "a volume that stopped answering is a change");
    }

    /// A lister whose first answer is held (an older question that comes back last).
    struct Overlapping {
        calls: AtomicUsize,
        hold: Arc<Gate>,
        first_asked: Mutex<std::sync::mpsc::Sender<()>>,
        old: Vec<ListedVolume>,
        new: Vec<ListedVolume>,
    }

    impl VolumeLister for Overlapping {
        fn list(&self) -> Vec<ListedVolume> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                let _ = lock(&self.first_asked).send(());
                self.hold.pass();
                self.old.clone()
            } else {
                self.new.clone()
            }
        }
    }

    #[test]
    fn an_older_listing_that_comes_back_last_does_not_overwrite_a_newer_one() {
        let fs = Arc::new(FakeFs::new());
        let (tx, asked) = std::sync::mpsc::channel();
        let hold = Gate::new();
        let lister = Arc::new(Overlapping {
            calls: AtomicUsize::new(0),
            hold: hold.clone(),
            first_asked: Mutex::new(tx),
            old: vec![vol("OLD", &fs.root(), VolumeLocation::Internal)],
            new: vec![vol("NEW", &fs.root(), VolumeLocation::Internal)],
        });
        let centre = Arc::new(DiskCentre::new(
            fs.clone(),
            lister,
            Arc::new(Events::default()),
            Arc::new(Manual::default()),
            Arc::new(Access::default()),
            8,
            ScanLimits::default(),
        ));
        let first = centre.clone();
        let t = std::thread::spawn(move || first.refresh());
        asked.recv().unwrap();
        centre.refresh();
        hold.open();
        t.join().unwrap();
        let names: Vec<String> = lock(&centre.known).clone().unwrap().iter().map(|l| l.volume.id.clone()).collect();
        assert_eq!(names, ["NEW"]);
    }

    #[test]
    fn a_whole_volume_scan_measures_against_the_used_space_of_now() {
        let fs = Arc::new(FakeFs::new());
        tree(&fs, &fs.root());
        let r = rig(fs.clone(), vec![vol("V", &fs.root(), VolumeLocation::Internal)]);
        *lock(&r.lister.2) = Some((1000 * MB, 100 * MB));
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        assert_eq!(
            r.events.all()[0].progress.total_bytes,
            Some(900 * MB),
            "not the 600 MB of the listing"
        );
        wait_final(&r, s.scan);
    }

    /// A finished scan (A), a running one held at its first read (B); returns them with the rig and the gate.
    fn finished_and_running(cap: usize) -> (Rig, u64, u64, Arc<Gate>) {
        let fs = Arc::new(FakeFs::new());
        let root = fs.root();
        let (a, b, c) = (fs.dir(&root, "a"), fs.dir(&root, "b"), fs.dir(&root, "c"));
        let r = rig(
            fs.clone(),
            vec![
                vol("A", &a, VolumeLocation::Internal),
                vol("B", &b, VolumeLocation::Internal),
                vol("C", &c, VolumeLocation::Internal),
            ],
        );
        r.centre.set_kept_nodes(cap);
        let done = r.centre.start(DiskTarget::Volume { id: "A".into() }).unwrap();
        wait_final(&r, done.scan);
        let gate = Gate::new();
        let g = gate.clone();
        fs.on_read(move |_| g.pass());
        let running = r.centre.start(DiskTarget::Volume { id: "B".into() }).unwrap();
        gate.wait_arrived(1);
        (r, done.scan, running.scan, gate)
    }

    #[test]
    fn running_scans_keep_room_for_their_growth_in_the_node_budget() {
        // The running scan may grow to 10_000 and the new one may take 10_000: they fit in 20_000 exactly, the finished one (1 node) does not.
        let (r, done, running, gate) = finished_and_running(20_000);
        let third = r.centre.start(DiskTarget::Volume { id: "C".into() }).unwrap();
        let kept: Vec<u64> = r.centre.scans().iter().map(|s| s.scan).collect();
        assert!(!kept.contains(&done), "the finished scan made room");
        assert!(kept.contains(&running) && kept.contains(&third.scan));
        gate.open();
        wait_final(&r, running);
        wait_final(&r, third.scan);
    }

    #[test]
    fn a_start_that_cannot_fit_next_to_the_running_scans_is_refused_with_a_typed_error() {
        let (r, done, running, gate) = finished_and_running(15_000);
        let refused = r.centre.start(DiskTarget::Volume { id: "C".into() }).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Busy);
        assert!(refused.message.contains("Finish or close a scan first"), "{}", refused.message);
        let kept: Vec<u64> = r.centre.scans().iter().map(|s| s.scan).collect();
        assert_eq!(kept, vec![done, running], "refusing took nothing from anyone");
        gate.open();
        wait_final(&r, running);
        // With the running scan finished, the same start fits: finished scans give way.
        let ok = r.centre.start(DiskTarget::Volume { id: "C".into() }).unwrap();
        wait_final(&r, ok.scan);
    }

    #[test]
    fn rings_answer_several_levels_at_once_and_refuse_what_is_out_of_range() {
        let fs = Arc::new(FakeFs::new());
        tree(&fs, &fs.root());
        let r = rig(fs.clone(), vec![vol("V", &fs.root(), VolumeLocation::Internal)]);
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        wait_final(&r, s.scan);
        let all = r.centre.rings(s.scan, None, 3, 0.0, 100).unwrap();
        let one = r.centre.children(s.scan, None, 0, 256, 0.0).unwrap();
        assert_eq!(
            all.root.children.len(),
            one.children.len(),
            "the first ring is the one-level answer"
        );
        assert_eq!(all.root.node.bytes, one.node.bytes);
        let inner = r.centre.rings(s.scan, Some(one.children[0].id), 1, 0.0, 10).unwrap();
        assert_eq!(inner.root.node.id, one.children[0].id);
        let bad = |rings, share, nodes| r.centre.rings(s.scan, None, rings, share, nodes).unwrap_err().code;
        assert_eq!(bad(0, 0.0, 10), ErrorCode::BadRequest);
        assert_eq!(bad(7, 0.0, 10), ErrorCode::BadRequest);
        assert_eq!(bad(2, 1.5, 10), ErrorCode::BadRequest);
        assert_eq!(bad(2, 0.0, 0), ErrorCode::BadRequest);
        assert_eq!(bad(2, 0.0, 2001), ErrorCode::BadRequest);
        assert_eq!(r.centre.rings(99, None, 2, 0.0, 10).unwrap_err().code, ErrorCode::NotFound);
        assert_eq!(
            r.centre.rings(s.scan, Some(9999), 2, 0.0, 10).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn rings_of_a_cancelled_scan_are_its_partial_tree() {
        let fs = Arc::new(FakeFs::new());
        tree(&fs, &fs.root());
        let gate = Gate::new();
        let g = gate.clone();
        fs.on_read(move |_| g.pass());
        let r = rig(fs.clone(), vec![vol("V", &fs.root(), VolumeLocation::Internal)]);
        let s = r.centre.start(DiskTarget::Volume { id: "V".into() }).unwrap();
        gate.wait_arrived(1);
        r.centre.cancel(s.scan).unwrap();
        gate.open();
        wait_final(&r, s.scan);
        let out = r.centre.rings(s.scan, None, 4, 0.0, 50).unwrap();
        assert!(!out.root.node.complete, "a cancelled scan is incomplete");
        assert_eq!(
            out.root.node.bytes,
            out.root.children.iter().map(|c| c.node.bytes).sum::<u64>() + out.root.other.bytes + out.root.loose.bytes
        );
    }

    #[test]
    fn a_folder_is_counted_against_the_host_it_lives_on() {
        let fs = Arc::new(FakeFs::new());
        let r = rig(
            fs,
            vec![
                vol("Local", Path::new("/data"), VolumeLocation::Internal),
                vol("Share", Path::new("/data/nas"), net("nas.local")),
            ],
        );
        r.centre.volumes();
        assert_eq!(r.centre.folder_host("/data/projects"), "this computer");
        assert_eq!(
            r.centre.folder_host("/data/nas/films"),
            "nas.local",
            "the deepest volume holding the path"
        );
        assert_eq!(r.centre.folder_host("/elsewhere"), "this computer");
    }

    #[test]
    fn the_system_clock_moves_forward() {
        let c = SystemClock::default();
        let a = c.now_ms();
        assert!(c.now_ms() >= a);
        assert!(c.wall_ms() > 1_700_000_000_000, "a wall-clock time of this century");
        let _ = LiveWorkers::default();
    }
}
