//! The disk map's engine as the agent runs it: volumes, scans as jobs with live status events, and the queries that
//! answer one level of a scan at a time. Read-only: nothing here changes a file (ADR 0021).

use super::engine::{End, LiveWorkers, Snapshot, Walk, WalkConfig, WalkHooks};
use super::usage;
use super::volumes::{ListedVolume, VolumeLister};
use dyno_core::disk::index::{DiskChildren, NodeId, ScanLimits};
use dyno_core::disk::scan::{walk_threads, DiskScanStatus, DiskTarget, EventGate, ScanFailure, ScanPhase, ScanProgress, ScanState};
use dyno_core::disk::source::{DirSource, ReadError};
use dyno_core::disk::volume::{Volume, VolumeLocation};
use dyno_core::protocol::{AgentError, ErrorCode};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Scans running at once.
pub const MAX_RUNNING: usize = 2;
/// Scans kept (running and finished); the oldest finished one goes first.
pub const MAX_KEPT: usize = 3;
/// Threads of one server still inside calls that never returned before a new scan of it is refused.
pub const MAX_ABANDONED_PER_HOST: usize = 4;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Where status events go.
pub trait StatusSink: Send + Sync {
    /// A scan's status changed.
    fn status(&self, status: DiskScanStatus);
}

/// Milliseconds on a clock that never goes back.
pub trait Clock: Send + Sync {
    /// Now.
    fn now_ms(&self) -> u64;
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
}

struct Pub {
    gate: EventGate,
    seq: u64,
    state: ScanState,
    phase: ScanPhase,
}

/// What a scan's events are made from; also the walk's hooks.
struct Meta {
    scan: u64,
    name: String,
    target: DiskTarget,
    volume: Option<Volume>,
    state: Mutex<Pub>,
    sink: Arc<dyn StatusSink>,
    clock: Arc<dyn Clock>,
}

impl Meta {
    fn build(&self, p: &Pub, snap: Snapshot) -> DiskScanStatus {
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
    cores: usize,
    limits: ScanLimits,
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
        cores: usize,
        limits: ScanLimits,
    ) -> DiskCentre {
        DiskCentre {
            source,
            lister,
            sink,
            clock,
            cores,
            limits,
            next: AtomicU64::new(1),
            scans: Mutex::new(Vec::new()),
            live: Mutex::new(HashMap::new()),
        }
    }

    /// The volumes mounted now.
    pub fn volumes(&self) -> Vec<Volume> {
        self.lister.list().into_iter().map(|l| l.volume).collect()
    }

    fn resolve(&self, target: &DiskTarget) -> Result<Resolved, AgentError> {
        let listed = self.lister.list();
        match target {
            DiskTarget::Volume { id } => {
                let ListedVolume { volume, root } = listed
                    .into_iter()
                    .find(|l| &l.volume.id == id)
                    .ok_or_else(|| err(ErrorCode::NotFound, "That volume is not mounted any more."))?;
                if !volume.reachable {
                    return Err(err(ErrorCode::Timeout, format!("{} is not answering.", volume.name)));
                }
                Ok(Resolved {
                    name: volume.name.clone(),
                    root,
                    location: volume.location.clone(),
                    rotational: volume.rotational,
                    host: host_of(&volume.location),
                    total_bytes: volume.whole_volume.then(|| volume.used()).flatten(),
                    volume: Some(volume),
                })
            }
            DiskTarget::Folder { path } => {
                let root = usage::check(path).map_err(|m| err(ErrorCode::BadRequest, m))?;
                let home = listed
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

    /// Starts a scan, or returns the one already running for the same folder. Returns at once.
    pub fn start(&self, target: DiskTarget) -> Result<DiskScanStatus, AgentError> {
        let r = self.resolve(&target)?;
        let key = format!("disk-scan:{}", r.root.display());
        let live = lock(&self.live).entry(r.host.clone()).or_default().clone();
        let mut scans = lock(&self.scans);
        if let Some(e) = scans.iter().find(|e| e.key == key && e.running()) {
            return Ok(e.meta.current(&e.walk));
        }
        if scans.iter().filter(|e| e.running()).count() >= MAX_RUNNING {
            return Err(err(
                ErrorCode::Busy,
                "Two scans are already running. Stop one or wait for it to finish.",
            ));
        }
        // Threads of this host that are alive although no running scan owns them: stuck in calls that never returned.
        let owned: usize = scans.iter().filter(|e| e.running() && e.host == r.host).map(|e| e.threads).sum();
        let abandoned = live.count().saturating_sub(owned);
        if abandoned >= MAX_ABANDONED_PER_HOST {
            return Err(err(
                ErrorCode::Busy,
                format!(
                    "{} still has {abandoned} reads that have not returned, so Dyno will not start another scan of it.",
                    r.host
                ),
            ));
        }
        while scans.len() >= MAX_KEPT {
            // At most MAX_RUNNING (< MAX_KEPT) scans run, so a finished one is always there to drop.
            let old = scans.iter().position(|e| !e.running()).unwrap_or(0);
            scans.remove(old);
        }
        let meta = Arc::new(Meta {
            scan: self.next.fetch_add(1, Ordering::Relaxed),
            name: r.name.clone(),
            target,
            volume: r.volume,
            state: Mutex::new(Pub {
                gate: EventGate::default(),
                seq: 0,
                state: ScanState::Running,
                phase: ScanPhase::ReadingVolumeFacts,
            }),
            sink: self.sink.clone(),
            clock: self.clock.clone(),
        });
        let threads = walk_threads(&r.location, r.rotational, self.cores);
        let cfg = WalkConfig {
            threads,
            limits: self.limits,
            cross_volumes: false,
            total_bytes: r.total_bytes,
            live,
        };
        meta.announce(r.total_bytes);
        let walk = Walk::start(self.source.clone(), r.root.clone(), &r.name, cfg, meta.clone()).map_err(|e| {
            let (code, message) = match e {
                ReadError::PermissionDenied => (ErrorCode::AccessDenied, "Dyno is not allowed to read that folder.".to_string()),
                ReadError::NotFound => (ErrorCode::NotFound, "That folder does not exist any more.".to_string()),
                ReadError::Unreachable(_) => (ErrorCode::Timeout, "That volume is not answering.".to_string()),
                ReadError::Other(m) => (ErrorCode::BadRequest, m),
            };
            meta.failed_to_start(&message);
            err(code, message)
        })?;
        let status = meta.current(&walk);
        scans.push(Arc::new(Entry {
            meta,
            walk,
            key,
            root: r.root,
            host: r.host,
            threads,
        }));
        Ok(status)
    }

    fn entry(&self, scan: u64) -> Result<Arc<Entry>, AgentError> {
        lock(&self.scans)
            .iter()
            .find(|e| e.meta.scan == scan)
            .cloned()
            .ok_or_else(|| err(ErrorCode::NotFound, "That scan is no longer kept. Scan again."))
    }

    /// Stops a scan and returns at once; what was counted stays. Stopping one that has ended does nothing.
    pub fn cancel(&self, scan: u64) -> Result<(), AgentError> {
        self.entry(scan)?.walk.cancel();
        Ok(())
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

    /// The full path of a node (for Reveal and Copy path).
    pub fn path(&self, scan: u64, node: u32) -> Result<String, AgentError> {
        let e = self.entry(scan)?;
        let names = e
            .walk
            .with_index(|i| i.path_names(NodeId(node)))
            .map_err(|x| err(ErrorCode::NotFound, x.to_string()))?;
        let mut p: &Path = &e.root;
        let owned;
        if names.len() > 1 {
            owned = names[1..].iter().fold(p.to_path_buf(), |acc, n| acc.join(n));
            p = &owned;
        }
        Ok(p.to_string_lossy().into_owned())
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
    use std::sync::atomic::AtomicU64;

    const MB: u64 = 1024 * 1024;

    #[derive(Default)]
    struct Events(Mutex<Vec<DiskScanStatus>>);

    impl StatusSink for Events {
        fn status(&self, s: DiskScanStatus) {
            lock(&self.0).push(s);
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
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    struct Fixed(Mutex<Vec<ListedVolume>>);

    impl VolumeLister for Fixed {
        fn list(&self) -> Vec<ListedVolume> {
            lock(&self.0).clone()
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

    struct Rig {
        centre: DiskCentre,
        events: Arc<Events>,
        clock: Arc<Manual>,
    }

    fn rig(source: Arc<dyn DirSource>, volumes: Vec<ListedVolume>) -> Rig {
        let (events, clock) = (Arc::new(Events::default()), Arc::new(Manual::default()));
        let centre = DiskCentre::new(
            source,
            Arc::new(Fixed(Mutex::new(volumes))),
            events.clone(),
            clock.clone(),
            8,
            ScanLimits {
                max_nodes: 10_000,
                notable_bytes: MB,
                max_hard_links: 100,
            },
        );
        Rig { centre, events, clock }
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
            let d = fs.dir(&root, &format!("d{i}"));
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
                let sub = fs.dir(&d, &format!("d{k}"));
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
    fn the_system_clock_moves_forward() {
        let c = SystemClock::default();
        let a = c.now_ms();
        assert!(c.now_ms() >= a);
        let _ = LiveWorkers::default();
    }
}
