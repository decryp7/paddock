//! The volume list of the disk map: every mount the OS lists, classified (internal, external, network) with its capacity.
//! A network mount's capacity is asked on a thread that is given up after a limit, because the call can block in the
//! kernel for as long as the server is gone.

use dyno_core::disk::volume::{classify, is_listed, network_protocol, MediaTraits, RawMount, Volume};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How long a network volume has to answer for its capacity.
pub const NETWORK_PROBE: Duration = Duration::from_secs(10);

/// A volume with the folder a scan of it starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedVolume {
    /// What the picker shows.
    pub volume: Volume,
    /// Where the walk starts (the mount point, except on the macOS startup disk).
    pub root: PathBuf,
}

/// Lists volumes. The real one asks the OS; tests give a fixed list.
pub trait VolumeLister: Send + Sync {
    /// The volumes mounted now.
    fn list(&self) -> Vec<ListedVolume>;
    /// The capacity and free bytes of one local volume right now (a scan's total is the used space as of its start, not as of
    /// the last listing). `None` when the lister cannot say quickly; the listing's numbers are used then.
    fn capacity_now(&self, _volume: &ListedVolume) -> Option<(u64, u64)> {
        None
    }
}

/// Runs `f` on its own thread and waits at most `limit` for the answer; `None` when it did not come in time (the thread
/// is left to finish whenever the OS lets it, its answer dropped).
pub fn within<T: Send + 'static>(limit: Duration, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("dyno-disk-probe".into())
        .spawn(move || {
            dyno_platform::priority::lower_current_thread();
            let _ = tx.send(f());
        })
        .ok()?;
    rx.recv_timeout(limit).ok()
}

/// Mounts whose capacity is being asked right now. A probe of a dead share can stay inside the kernel for as long as the server
/// is gone; while one is, the mount is listed as not answering and no second thread is started for it.
pub type InFlight = Arc<Mutex<HashSet<String>>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Removes the mount from the in-flight set when the probe thread ends, whatever the way.
struct Marker(InFlight, String);

impl Drop for Marker {
    fn drop(&mut self) {
        lock(&self.0).remove(&self.1);
    }
}

/// The answer of a capacity probe that is still running.
type Probe = mpsc::Receiver<Option<(u64, u64)>>;

/// Starts the capacity probe of a network `mount` on its own thread. `None` when one is already in flight or no thread could
/// be started (the volume is then listed as not answering).
fn probe_network(
    mount: String,
    capacity: &(impl Fn(&str) -> Option<(u64, u64)> + Send + Sync + Clone + 'static),
    inflight: &InFlight,
) -> Option<Probe> {
    if !lock(inflight).insert(mount.clone()) {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    let ask = capacity.clone();
    let marker = Marker(inflight.clone(), mount.clone());
    let spawned = std::thread::Builder::new().name("dyno-disk-probe".into()).spawn(move || {
        dyno_platform::priority::lower_current_thread();
        let _marker = marker;
        let _ = tx.send(ask(&mount));
    });
    // A thread that did not start dropped its marker with the closure.
    spawned.ok().map(|_| rx)
}

/// Classifies and measures `mounts`. `traits` says what the medium is; `capacity` is asked for each volume. Network volumes
/// are all asked at once, on threads that are given up after `probe_limit` in total (the volume is then listed as not
/// answering) and never started twice for the same mount while one is still inside the kernel.
pub fn list_mounts(
    mounts: Vec<RawMount>,
    traits: impl Fn(&RawMount) -> MediaTraits,
    capacity: impl Fn(&str) -> Option<(u64, u64)> + Send + Sync + Clone + 'static,
    probe_limit: Duration,
    inflight: &InFlight,
) -> Vec<ListedVolume> {
    let mounts: Vec<RawMount> = mounts.into_iter().filter(is_listed).collect();
    let probes: Vec<Option<Probe>> = mounts
        .iter()
        .map(|m| network_protocol(&m.fs).and_then(|_| probe_network(m.mount.clone(), &capacity, inflight)))
        .collect();
    let deadline = Instant::now() + probe_limit;
    mounts
        .into_iter()
        .zip(probes)
        .map(|(m, probe)| {
            let network = network_protocol(&m.fs).is_some();
            let medium = if network { MediaTraits::default() } else { traits(&m) };
            let (size, reachable) = match (network, probe) {
                (false, _) => (capacity(&m.mount), true),
                (true, Some(rx)) => match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(c) => (c, true),
                    Err(_) => (None, false),
                },
                (true, None) => (None, false),
            };
            let root = PathBuf::from(m.scan_root.clone().unwrap_or_else(|| m.mount.clone()));
            ListedVolume {
                volume: classify(&m, &medium, size, reachable),
                root,
            }
        })
        .collect()
}

/// The real list. What a medium is does not change while it stays mounted, so `diskutil` and the like are asked once per mount.
#[derive(Default)]
pub struct OsVolumes {
    inflight: InFlight,
    media: Mutex<HashMap<(String, String), MediaTraits>>,
}

impl VolumeLister for OsVolumes {
    fn capacity_now(&self, volume: &ListedVolume) -> Option<(u64, u64)> {
        // A network volume may block in the kernel for as long as the server is gone: only local volumes are asked here.
        if matches!(volume.volume.location, dyno_core::disk::volume::VolumeLocation::Network { .. }) {
            return None;
        }
        dyno_platform::volumes::capacity(&volume.volume.mount)
    }

    fn list(&self) -> Vec<ListedVolume> {
        list_mounts(
            dyno_platform::volumes::mounts(),
            |m| {
                let key = (m.mount.clone(), m.source.clone());
                if let Some(t) = lock(&self.media).get(&key) {
                    return t.clone();
                }
                let t = dyno_platform::volumes::media_traits(m);
                lock(&self.media).insert(key, t.clone());
                t
            },
            dyno_platform::volumes::capacity,
            NETWORK_PROBE,
            &self.inflight,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::disk::volume::VolumeLocation;
    use std::sync::mpsc::channel;

    #[test]
    fn an_answer_in_time_is_returned() {
        assert_eq!(within(Duration::from_secs(30), || 7), Some(7));
    }

    #[test]
    fn a_call_that_never_returns_is_given_up() {
        let (release, wait) = channel::<()>();
        assert_eq!(within(Duration::from_millis(0), move || wait.recv().ok()), None);
        release.send(()).unwrap();
    }

    fn raw(mount: &str, fs: &str) -> RawMount {
        RawMount {
            source: "//alex@nas.local/media".into(),
            mount: mount.into(),
            fs: fs.into(),
            ..RawMount::default()
        }
    }

    #[test]
    fn mounts_are_filtered_classified_and_measured() {
        let mut mac = raw("/", "apfs");
        mac.scan_root = Some("/System/Volumes/Data".into());
        let listed = list_mounts(
            vec![
                mac,
                raw("/proc", "proc"),
                raw("/Volumes/usb", "exfat"),
                raw("/Volumes/media", "smbfs"),
            ],
            |m| MediaTraits {
                internal: Some(m.mount == "/"),
                ..MediaTraits::default()
            },
            |m| Some((1000, if m == "/" { 250 } else { 900 })),
            Duration::from_secs(30),
            &InFlight::default(),
        );
        let names: Vec<&str> = listed.iter().map(|l| l.volume.id.as_str()).collect();
        assert_eq!(names, ["/", "/Volumes/usb", "/Volumes/media"], "proc is not a volume");
        assert_eq!(listed[0].root, PathBuf::from("/System/Volumes/Data"));
        assert_eq!(listed[0].volume.used(), Some(750));
        assert!(matches!(listed[1].volume.location, VolumeLocation::External { .. }));
        assert_eq!(listed[1].root, PathBuf::from("/Volumes/usb"));
        assert!(matches!(listed[2].volume.location, VolumeLocation::Network { .. }));
        assert!(listed[2].volume.reachable && listed[2].volume.capacity == Some(1000));
    }

    #[test]
    fn a_share_that_does_not_answer_is_listed_as_not_answering() {
        let (release, wait) = channel::<()>();
        let wait = std::sync::Arc::new(std::sync::Mutex::new(wait));
        let listed = list_mounts(
            vec![raw("/Volumes/media", "smbfs")],
            |_| MediaTraits::default(),
            move |_| {
                let _ = wait.lock().unwrap().recv();
                Some((1, 1))
            },
            Duration::from_millis(0),
            &InFlight::default(),
        );
        assert!(!listed[0].volume.reachable);
        assert_eq!(listed[0].volume.capacity, None);
        release.send(()).unwrap();
    }

    #[test]
    fn shares_are_asked_at_once_not_one_after_the_other() {
        // Each probe waits for the other: serial probing could never get past the first.
        let both = Arc::new(std::sync::Barrier::new(2));
        let listed = list_mounts(
            vec![raw("/Volumes/a", "smbfs"), raw("/Volumes/b", "smbfs")],
            |_| MediaTraits::default(),
            move |_| {
                both.wait();
                Some((10, 5))
            },
            Duration::from_secs(30),
            &InFlight::default(),
        );
        assert!(listed.iter().all(|l| l.volume.reachable && l.volume.capacity == Some(10)));
    }

    #[test]
    fn a_share_that_is_still_being_asked_is_not_asked_again() {
        let (release, wait) = channel::<()>();
        let wait = Arc::new(Mutex::new(wait));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (n, w) = (calls.clone(), wait.clone());
        let asked = move |_: &str| {
            n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = lock(&w).recv();
            Some((1, 1))
        };
        let inflight = InFlight::default();
        let first = list_mounts(
            vec![raw("/Volumes/media", "smbfs")],
            |_| MediaTraits::default(),
            asked.clone(),
            Duration::from_millis(0),
            &inflight,
        );
        let second = list_mounts(
            vec![raw("/Volumes/media", "smbfs")],
            |_| MediaTraits::default(),
            asked,
            Duration::from_millis(0),
            &inflight,
        );
        assert!(!first[0].volume.reachable && !second[0].volume.reachable);
        assert!(
            lock(&inflight).contains("/Volumes/media"),
            "the first probe is still inside the call"
        );
        release.send(()).unwrap();
        // The second listing started no thread of its own: at most the first probe ever called the function.
        assert!(calls.load(std::sync::atomic::Ordering::SeqCst) <= 1);
    }

    #[test]
    fn a_probe_marker_clears_when_its_thread_ends() {
        let inflight = InFlight::default();
        let rx = probe_network("/Volumes/x".into(), &|_: &str| Some((2, 1)), &inflight).unwrap();
        assert_eq!(rx.recv().unwrap(), Some((2, 1)));
        // The marker goes with the thread, just after the answer: a new probe is accepted once it has.
        while lock(&inflight).contains("/Volumes/x") {
            std::thread::yield_now();
        }
        assert!(probe_network("/Volumes/x".into(), &|_: &str| None, &inflight).is_some());
    }
}
