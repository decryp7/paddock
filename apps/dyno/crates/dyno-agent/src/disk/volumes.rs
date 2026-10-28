//! The volume list of the disk map: every mount the OS lists, classified (internal, external, network) with its capacity.
//! A network mount's capacity is asked on a thread that is given up after a limit, because the call can block in the
//! kernel for as long as the server is gone.

use dyno_core::disk::volume::{classify, is_listed, network_protocol, MediaTraits, RawMount, Volume};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

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
}

/// Runs `f` on its own thread and waits at most `limit` for the answer; `None` when it did not come in time (the thread
/// is left to finish whenever the OS lets it, its answer dropped).
pub fn within<T: Send + 'static>(limit: Duration, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("dyno-disk-probe".into())
        .spawn(move || {
            let _ = tx.send(f());
        })
        .ok()?;
    rx.recv_timeout(limit).ok()
}

/// Classifies and measures `mounts`. `traits` says what the medium is; `capacity` is asked for each volume, and for a
/// network volume on a thread that is given up after `probe_limit` (the volume is then listed as not answering).
pub fn list_mounts(
    mounts: Vec<RawMount>,
    traits: impl Fn(&RawMount) -> MediaTraits,
    capacity: impl Fn(&str) -> Option<(u64, u64)> + Send + Sync + Clone + 'static,
    probe_limit: Duration,
) -> Vec<ListedVolume> {
    mounts
        .into_iter()
        .filter(is_listed)
        .map(|m| {
            let network = network_protocol(&m.fs).is_some();
            let medium = if network { MediaTraits::default() } else { traits(&m) };
            let mount = m.mount.clone();
            let (size, reachable) = if network {
                let ask = capacity.clone();
                match within(probe_limit, move || ask(&mount)) {
                    Some(c) => (c, true),
                    None => (None, false),
                }
            } else {
                (capacity(&mount), true)
            };
            let root = PathBuf::from(m.scan_root.clone().unwrap_or_else(|| m.mount.clone()));
            ListedVolume {
                volume: classify(&m, &medium, size, reachable),
                root,
            }
        })
        .collect()
}

/// The real list.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsVolumes;

impl VolumeLister for OsVolumes {
    fn list(&self) -> Vec<ListedVolume> {
        list_mounts(
            dyno_platform::volumes::mounts(),
            dyno_platform::volumes::media_traits,
            dyno_platform::volumes::capacity,
            NETWORK_PROBE,
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
        );
        assert!(!listed[0].volume.reachable);
        assert_eq!(listed[0].volume.capacity, None);
        release.send(()).unwrap();
    }
}
