//! Who holds a socket or a file (Linux): the kernel shows `/proc/<pid>/fd` of other accounts only to root, so the helper reads it
//! (`SocketOwners`, `FileHolders`; helper-wiring plan section 2.6). Two instances of the seam:
//!
//! * [`SocketOwnerStep`] fills the program of the sockets whose owner this account cannot see (`pid: None`, with the inode the
//!   Linux reader keeps), from the helper's inode-to-program map. Without the helper the rows stay as they are and the page says
//!   why.
//! * [`HelperLocks`] answers "what is holding this file?" through the helper by device and inode (no path crosses the wire) and
//!   falls back to the agent's own reader, which sees the account's own programs only.

use crate::helper_first::{HelperCalls, HelperFirst, HelperReading, ReadClass};
use crate::helper_link::CallError;
use dyno_core::helper_holders::{FileHolders, Holder, SocketOwners};
use dyno_core::helper_policy::RequestKind;
use dyno_core::helper_processes::OwnerKind;
use dyno_core::metrics::Reading;
use dyno_core::network::SocketRow;
use dyno_core::ports::{FileLocks, PlatformError};
use dyno_core::process::{Locker, LockerList, ProcessRef};
use dyno_core::read_via::ReadVia;
use dyno_core::time::{Timestamp, SECOND};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long the helper's socket map is reused. The helper sweeps every process's descriptors, so the map is read at most this
/// often, however often the page asks (rule 51: cheaper). A socket opened since shows its owner at the next refresh.
pub const OWNERS_TTL: Duration = Duration::from_secs(10);

/// The sentence for sockets whose program the kernel hides, before the shared "turn on Administrator readings" sentence.
const HIDDEN: &str = "Some sockets belong to other accounts, and Linux hides which program holds them.";

/// The socket-owner reading: the helper's map, else nothing (the rows keep their hidden owner).
pub struct SocketOwnerReading;

impl HelperReading for SocketOwnerReading {
    type Value = SocketOwners;
    const KINDS: &'static [RequestKind] = &[RequestKind::SocketOwners];

    fn name(&self) -> &'static str {
        "socket owners"
    }

    fn class(&self) -> ReadClass {
        // Read on the network page's refresh (kept 10 s), so the helper gets the short live wait.
        ReadClass::Live
    }

    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<SocketOwners, CallError> {
        calls.socket_owners()
    }

    fn local(&mut self) -> Reading<SocketOwners> {
        // The agent has no other way to learn them: its own reader already gave every owner it can see.
        Reading::Available(SocketOwners::default())
    }

    fn partial(&self, _: &SocketOwners) -> Option<String> {
        Some(HIDDEN.to_string())
    }
}

/// What one helper read of the socket map kept.
struct OwnerMap {
    at: Instant,
    by_inode: HashMap<u64, Vec<u32>>,
    via: ReadVia,
}

/// Looks up the parents of processes (for folding a master's workers into it); a parameter so tests need no `/proc`.
pub type Parents = Box<dyn Fn(&[u32]) -> HashMap<u32, u32> + Send + Sync>;

/// Fills the hidden owners of socket rows from the helper.
pub struct SocketOwnerStep {
    seam: HelperFirst<SocketOwnerReading>,
    parents: Parents,
    map: Option<OwnerMap>,
}

impl SocketOwnerStep {
    /// A step over `seam`, folding holders that inherited a socket from their parent with `parents`.
    pub fn new(seam: HelperFirst<SocketOwnerReading>, parents: Parents) -> SocketOwnerStep {
        SocketOwnerStep { seam, parents, map: None }
    }

    /// A step that reads the parents from this computer's process table.
    pub fn for_this_os(seam: HelperFirst<SocketOwnerReading>) -> SocketOwnerStep {
        SocketOwnerStep::new(seam, Box::new(os_parents))
    }

    /// A person asked to try again: the helper is asked at the next fill, whatever was kept or left alone.
    pub fn retry_now(&mut self) {
        self.map = None;
        self.seam.retry_now();
    }

    /// `rows` with the programs of the sockets the kernel hid from this account filled in from the helper, and where they came
    /// from. The helper is asked only when some row needs it and at most every [`OWNERS_TTL`]. The inode, an input of this step,
    /// is cleared from every row, and rows that differ only by it are one again.
    pub fn fill(&mut self, rows: Vec<SocketRow>, now: Instant) -> (Vec<SocketRow>, ReadVia) {
        let hidden = rows.iter().filter(|r| r.pid.is_none() && r.inode.is_some()).count();
        if rows.iter().all(|r| r.inode.is_none()) {
            return (rows, ReadVia::Local);
        }
        if hidden == 0 {
            return (fold(rows), ReadVia::Local);
        }
        if self.map.as_ref().is_none_or(|m| now.saturating_duration_since(m.at) >= OWNERS_TTL) {
            let got = self.seam.read();
            let by_inode = match &got.value {
                Reading::Available(o) => self.index(o),
                Reading::Unavailable(_) => HashMap::new(),
            };
            self.map = Some(OwnerMap {
                at: now,
                by_inode,
                via: got.via,
            });
        }
        let map = self.map.as_ref();
        let via = map.map_or(ReadVia::Local, |m| m.via.clone());
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            match (row.pid, row.inode.and_then(|i| map?.by_inode.get(&i))) {
                (None, Some(pids)) => out.extend(pids.iter().map(|&pid| SocketRow {
                    pid: Some(pid),
                    ..row.clone()
                })),
                _ => out.push(row),
            }
        }
        (fold(out), via)
    }

    /// The owners of each socket: every holder whose parent does not hold it too (the shared rule of the Linux reader).
    fn index(&self, owners: &SocketOwners) -> HashMap<u64, Vec<u32>> {
        let mut holders: HashMap<u64, Vec<u32>> = HashMap::new();
        for o in &owners.owners {
            holders.entry(o.inode).or_default().push(o.holder.pid);
        }
        let shared: Vec<u32> = holders.values().filter(|p| p.len() > 1).flatten().copied().collect();
        let parents = if shared.is_empty() {
            HashMap::new()
        } else {
            (self.parents)(&shared)
        };
        holders
            .into_iter()
            .map(|(inode, pids)| {
                (
                    inode,
                    dyno_platform::parse::socket_owner::owners(&pids, |p| parents.get(&p).copied()),
                )
            })
            .collect()
    }
}

/// Clears the inode and folds rows that are equal without it (the sockets of other accounts the OS cannot tell apart).
fn fold(rows: Vec<SocketRow>) -> Vec<SocketRow> {
    let mut seen = HashSet::new();
    rows.into_iter()
        .map(|r| SocketRow { inode: None, ..r })
        .filter(|r| seen.insert(r.clone()))
        .collect()
}

#[cfg(target_os = "linux")]
fn os_parents(pids: &[u32]) -> HashMap<u32, u32> {
    dyno_platform::network::parents_of(pids.iter().copied())
}

#[cfg(not(target_os = "linux"))]
fn os_parents(_: &[u32]) -> HashMap<u32, u32> {
    HashMap::new()
}

/// The device and inode of `path`, the way the helper compares them (`stat`'s `st_dev` and `st_ino`).
#[cfg(unix)]
fn device_and_inode(path: &Path) -> Result<(u64, u64), PlatformError> {
    use std::os::unix::fs::MetadataExt;
    let canonical = path
        .canonicalize()
        .map_err(|_| PlatformError::NotFound(format!("{} no longer exists.", path.display())))?;
    let meta = std::fs::metadata(&canonical).map_err(|e| PlatformError::Failed(format!("Could not read {}: {e}", canonical.display())))?;
    Ok((meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
fn device_and_inode(_: &Path) -> Result<(u64, u64), PlatformError> {
    Err(PlatformError::Unsupported(
        "File holders are read through the helper on Linux only.".into(),
    ))
}

/// A program the helper named, as the file dialog lists it.
pub fn locker_of(h: &Holder) -> Locker {
    let whose = match &h.owner {
        OwnerKind::Caller => "Has it open or holds a lock",
        OwnerKind::System => "System program: has it open or holds a lock",
        OwnerKind::Service(_) => "Service: has it open or holds a lock",
        OwnerKind::OtherUser => "Another account's program: has it open or holds a lock",
    };
    Locker {
        process: ProcessRef {
            pid: h.pid,
            started: h.started.map(|s| Timestamp(s.saturating_mul(SECOND))),
        },
        name: h.name.clone(),
        kind: Some(whose.to_string()),
        via: ReadVia::Helper,
    }
}

/// The reading behind [`HelperLocks`]: one file, by path for the agent and by device and inode for the helper.
pub(crate) struct FileHolderReading {
    local: Box<dyn FileLocks>,
    path: PathBuf,
    dev: u64,
    ino: u64,
    /// What the agent's own reader refused with, to report it unchanged when nothing else answers (its code and next step).
    refused: Option<PlatformError>,
}

impl FileHolderReading {
    /// A reading over the agent's own reader `local`; the file is set before each read.
    pub(crate) fn new(local: Box<dyn FileLocks>) -> FileHolderReading {
        FileHolderReading {
            local,
            path: PathBuf::new(),
            dev: 0,
            ino: 0,
            refused: None,
        }
    }
}

impl HelperReading for FileHolderReading {
    type Value = LockerList;
    const KINDS: &'static [RequestKind] = &[RequestKind::FileHolders];

    fn name(&self) -> &'static str {
        "file holders"
    }

    fn class(&self) -> ReadClass {
        // A person pressed Find and waits for it; the helper's sweep has its own budget.
        ReadClass::Bulk
    }

    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<LockerList, CallError> {
        let FileHolders {
            holders,
            others,
            truncated,
        } = calls.file_holders(self.dev, self.ino)?;
        Ok(LockerList {
            lockers: holders.iter().map(locker_of).collect(),
            not_listed: others,
            truncated,
        })
    }

    fn local(&mut self) -> Reading<LockerList> {
        self.refused = None;
        match self.local.lockers_with_more(&self.path) {
            Ok(v) => Reading::Available(v),
            Err(e) => {
                let why = e.to_string();
                self.refused = Some(e);
                Reading::Unavailable(why)
            }
        }
    }
}

/// "What is holding this file?" with the helper first, then the agent's own reader.
pub struct HelperLocks {
    seam: HelperFirst<FileHolderReading>,
}

impl HelperLocks {
    /// `local` is the platform's reader; `seam_port` says where the helper is.
    pub fn new(local: Box<dyn FileLocks>, port: Box<dyn crate::helper_first::HelperPort>) -> HelperLocks {
        HelperLocks {
            seam: HelperFirst::new(FileHolderReading::new(local), port),
        }
    }
}

impl FileLocks for HelperLocks {
    fn lockers(&mut self, path: &Path) -> Result<Vec<Locker>, PlatformError> {
        self.lockers_with_more(path).map(|l| l.lockers)
    }

    fn lockers_with_more(&mut self, path: &Path) -> Result<LockerList, PlatformError> {
        // A person pressed Find: a helper left alone after a failure is asked again now.
        self.seam.retry_now();
        let reading = self.seam.reading_mut();
        let (dev, ino) = match device_and_inode(path) {
            Ok(id) => id,
            // A path that is gone or unreadable is the agent's own reader's to report.
            Err(_) => return reading.local.lockers_with_more(path),
        };
        reading.path = path.to_path_buf();
        (reading.dev, reading.ino) = (dev, ino);
        let got = self.seam.read();
        match got.value {
            Reading::Available(mut found) => {
                for l in &mut found.lockers {
                    l.via = got.via.clone();
                }
                Ok(found)
            }
            Reading::Unavailable(why) => Err(self.seam.reading_mut().refused.take().unwrap_or(PlatformError::Failed(why))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_first::testkit::{centre, port, Net};
    use dyno_core::helper_holders::SocketOwner;
    use dyno_core::helper_wire::HelperReply;
    use dyno_core::network::{Proto, SocketState};
    use paddock_jobs::time::FakeClock;
    use std::net::{IpAddr, Ipv4Addr};

    fn row(port: u16, pid: Option<u32>, inode: Option<u64>) -> SocketRow {
        SocketRow {
            proto: Proto::Tcp,
            local: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            local_port: port,
            remote: None,
            state: SocketState::Listen,
            pid,
            inode,
        }
    }

    fn holder(pid: u32) -> Holder {
        Holder {
            pid,
            started: Some(1_700_000_000),
            name: format!("p{pid}"),
            owner: OwnerKind::OtherUser,
        }
    }

    fn owners(list: &[(u64, u32)]) -> HelperReply {
        HelperReply::SocketOwners(SocketOwners {
            owners: list
                .iter()
                .map(|&(inode, pid)| SocketOwner {
                    inode,
                    holder: holder(pid),
                })
                .collect(),
            ..SocketOwners::default()
        })
    }

    fn step(up: bool, answers: Vec<Result<HelperReply, crate::helper_link::TransportError>>) -> (SocketOwnerStep, std::sync::Arc<Net>) {
        let net = Net::new(up, answers);
        let c = centre(&net, &FakeClock::new());
        (
            SocketOwnerStep::new(HelperFirst::new(SocketOwnerReading, port(&c)), Box::new(|_| HashMap::new())),
            net,
        )
    }

    #[test]
    fn a_hidden_socket_owner_is_filled_from_the_helper() {
        let (mut s, _) = step(true, vec![Ok(owners(&[(900, 7001)]))]);
        let rows = vec![row(80, Some(5), Some(100)), row(5432, None, Some(900))];
        let (out, via) = s.fill(rows, Instant::now());
        assert_eq!(
            out.iter().map(|r| (r.local_port, r.pid)).collect::<Vec<_>>(),
            [(80, Some(5)), (5432, Some(7001))]
        );
        assert!(out.iter().all(|r| r.inode.is_none()), "the inode does not leave the step");
        assert_eq!(via, ReadVia::Helper);
    }

    #[test]
    fn without_the_helper_the_owner_stays_hidden_and_the_source_asks_for_the_helper() {
        let (mut s, _) = step(false, vec![]);
        let (out, via) = s.fill(vec![row(5432, None, Some(900))], Instant::now());
        assert_eq!(out[0].pid, None);
        let ReadVia::NeedsHelper { reason, .. } = via else {
            panic!("{via:?}")
        };
        assert!(reason.contains("Linux hides which program"), "{reason}");
    }

    #[test]
    fn when_every_owner_is_visible_the_helper_is_not_asked() {
        let (mut s, net) = step(true, vec![]);
        let (out, via) = s.fill(vec![row(80, Some(5), Some(100))], Instant::now());
        assert_eq!((out[0].pid, via), (Some(5), ReadVia::Local));
        assert_eq!(
            net.infos.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "not even its readiness was asked"
        );
    }

    #[test]
    fn the_helper_map_is_reused_for_ten_seconds() {
        let (mut s, _) = step(true, vec![Ok(owners(&[(900, 7001)])), Ok(owners(&[(900, 7002)]))]);
        let t0 = Instant::now();
        let hidden = || vec![row(5432, None, Some(900))];
        assert_eq!(s.fill(hidden(), t0).0[0].pid, Some(7001));
        assert_eq!(
            s.fill(hidden(), t0 + Duration::from_secs(9)).0[0].pid,
            Some(7001),
            "still the first map"
        );
        assert_eq!(s.fill(hidden(), t0 + OWNERS_TTL).0[0].pid, Some(7002), "read again after the TTL");
    }

    #[test]
    fn a_person_asking_to_try_again_reads_the_helper_map_again_at_once() {
        let (mut s, _) = step(true, vec![Ok(owners(&[(900, 7001)])), Ok(owners(&[(900, 7002)]))]);
        let t0 = Instant::now();
        assert_eq!(s.fill(vec![row(5432, None, Some(900))], t0).0[0].pid, Some(7001));
        s.retry_now();
        assert_eq!(
            s.fill(vec![row(5432, None, Some(900))], t0).0[0].pid,
            Some(7002),
            "no waiting for the ten seconds"
        );
    }

    #[test]
    fn workers_that_inherited_a_listener_fold_into_their_master() {
        let net = Net::new(true, vec![Ok(owners(&[(900, 7001), (900, 7002), (900, 7003)]))]);
        let c = centre(&net, &FakeClock::new());
        let parents: Parents = Box::new(|_| HashMap::from([(7002, 7001), (7003, 7001), (7001, 1)]));
        let mut s = SocketOwnerStep::new(HelperFirst::new(SocketOwnerReading, port(&c)), parents);
        let (out, _) = s.fill(vec![row(443, None, Some(900))], Instant::now());
        assert_eq!(out.iter().map(|r| r.pid).collect::<Vec<_>>(), [Some(7001)]);
    }

    #[test]
    fn sockets_the_helper_does_not_list_stay_hidden_and_identical_ones_are_one_row() {
        let (mut s, _) = step(true, vec![Ok(owners(&[]))]);
        let (out, via) = s.fill(vec![row(53, None, Some(1)), row(53, None, Some(2))], Instant::now());
        assert_eq!(out.len(), 1, "the OS gave nothing that tells them apart");
        assert_eq!((out[0].pid, via), (None, ReadVia::Helper));
    }

    /// A reader of the agent's own: the lockers it is told, or the refusal it is told.
    struct Own(Result<Vec<Locker>, PlatformError>);
    impl FileLocks for Own {
        fn lockers(&mut self, _: &Path) -> Result<Vec<Locker>, PlatformError> {
            self.0.clone()
        }
    }

    fn locks(
        up: bool,
        own: Result<Vec<Locker>, PlatformError>,
        answers: Vec<Result<HelperReply, crate::helper_link::TransportError>>,
    ) -> HelperLocks {
        let c = centre(&Net::new(up, answers), &FakeClock::new());
        HelperLocks::new(Box::new(Own(own)), port(&c))
    }

    #[cfg(unix)]
    #[test]
    fn holders_come_from_the_helper_by_device_and_inode() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("held.log");
        std::fs::write(&file, "x").unwrap();
        let reply = HelperReply::FileHolders(FileHolders {
            holders: vec![holder(4321)],
            ..FileHolders::default()
        });
        let got = locks(true, Ok(vec![]), vec![Ok(reply)]).lockers(&file).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].process.pid, got[0].via.clone()), (4321, ReadVia::Helper));
        assert_eq!(got[0].process.started, Some(Timestamp(1_700_000_000 * SECOND)));
        assert!(got[0].kind.as_deref().unwrap().contains("Another account"));
    }

    #[cfg(unix)]
    #[test]
    fn programs_the_helper_counts_but_does_not_name_are_carried_so_an_empty_list_is_not_read_as_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("held.log");
        std::fs::write(&file, "x").unwrap();
        let reply = HelperReply::FileHolders(FileHolders {
            holders: vec![],
            others: 3,
            truncated: true,
        });
        let got = locks(true, Ok(vec![]), vec![Ok(reply)]).lockers_with_more(&file).unwrap();
        assert_eq!((got.lockers.len(), got.not_listed, got.truncated), (0, 3, true));
    }

    #[cfg(unix)]
    #[test]
    fn without_the_helper_the_agents_own_lockers_come_back_as_local_and_its_refusal_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.log");
        std::fs::write(&file, "x").unwrap();
        let mine = Locker::new(ProcessRef { pid: 9, started: None }, "me".into(), None);
        let got = locks(false, Ok(vec![mine.clone()]), vec![]).lockers(&file).unwrap();
        assert_eq!(got, [mine]);
        let denied = PlatformError::AccessDenied("others are out of sight".into());
        let err = locks(false, Err(denied.clone()), vec![]).lockers(&file).unwrap_err();
        assert_eq!(err, denied, "the code and words the platform gave");
    }

    #[test]
    fn a_missing_path_is_reported_by_the_agents_own_reader() {
        let gone = PlatformError::NotFound("gone".into());
        let err = locks(true, Err(gone.clone()), vec![])
            .lockers(Path::new("/definitely/not/here"))
            .unwrap_err();
        assert_eq!(err, gone);
    }
}
