//! Which build a program is, and whether its file was replaced while it ran.
//!
//! A version string alone cannot tell two builds of the same version apart (a release candidate and the release, a rebuild),
//! so a newer app kept talking to an older agent. The build id is the start of the SHA-256 of the program's file: the app
//! hashes the agent file it would start, the agent hashes its own file, and the discovery file carries the result. The same
//! module owns the "my file was replaced" check ([`BinaryWatch`]): an agent whose file changed or vanished exits so its
//! supervisor (launchd, systemd --user, the app) starts the new one. Nothing here kills a process.

use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

/// Longest file hashed (a bound on an unbounded input; the agent is a few tens of MiB).
const MAX_HASHED: u64 = 1 << 30;
/// Hex characters kept from the digest.
const ID_LEN: usize = 16;
/// Separates the version from the build id in a build key.
const SEP: char = '#';

/// The build id of the program file at `path`, or `None` when it cannot be read (or is over the bound).
pub fn file_build_id(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_HASHED {
        return None;
    }
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_HASHED {
            return None;
        }
        hasher.update(&buf[..n]);
    }
    let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Some(hex[..ID_LEN].to_string())
}

/// The build id of the running program's own file, read once.
pub fn own_build_id() -> String {
    static OWN: OnceLock<String> = OnceLock::new();
    OWN.get_or_init(|| std::env::current_exe().ok().and_then(|p| file_build_id(&p)).unwrap_or_default())
        .clone()
}

/// The key the app compares an agent against: the version, plus the build id when the app could read the agent file it
/// would start. An empty id gives the version alone, which accepts any build of that version (the app cannot judge).
pub fn build_key(version: &str, build: &str) -> String {
    if build.is_empty() {
        version.to_string()
    } else {
        format!("{version}{SEP}{build}")
    }
}

/// Whether an agent that reports `version` and `build` is the one `key` (see [`build_key`]) asks for.
pub fn matches_key(key: &str, version: &str, build: &str) -> bool {
    match key.rsplit_once(SEP) {
        Some((want_version, want_build)) => version == want_version && build == want_build,
        None => version == key,
    }
}

/// What a program file looked like at one moment: enough to see that another file stands at the same path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStamp {
    /// Size in bytes.
    pub len: u64,
    /// Last modification, ms since the epoch.
    pub modified_ms: i128,
    /// The file's identity on disk (inode on Unix; none elsewhere, where a running program's file cannot be replaced).
    pub id: Option<u64>,
}

impl FileStamp {
    /// The stamp of the file at `path`, or `None` when it is gone.
    pub fn of(path: &Path) -> Option<FileStamp> {
        let m = std::fs::metadata(path).ok()?;
        let modified_ms = m
            .modified()
            .ok()
            .map(|t| match t.duration_since(std::time::UNIX_EPOCH) {
                Ok(d) => d.as_millis() as i128,
                Err(e) => -(e.duration().as_millis() as i128),
            })
            .unwrap_or(0);
        #[cfg(unix)]
        let id = {
            use std::os::unix::fs::MetadataExt;
            Some(m.ino())
        };
        #[cfg(not(unix))]
        let id = None;
        Some(FileStamp {
            len: m.len(),
            modified_ms,
            id,
        })
    }
}

/// Consecutive looks that must differ before the file counts as replaced (one look can fall in the middle of an install).
const LOOKS_TO_CONFIRM: u32 = 2;

/// What a look at the program file concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The same file as at the start (or a first differing look not yet confirmed).
    Unchanged,
    /// Another file stands there (or none): confirmed by consecutive looks.
    Replaced,
}

/// Remembers the stamp of the program file at start and judges later looks. Pure: the caller reads the file.
#[derive(Debug)]
pub struct BinaryWatch {
    start: Option<FileStamp>,
    differing: u32,
}

impl BinaryWatch {
    /// A watch for a program whose file looked like `start` when it began. With no stamp (the file could not be read at
    /// start) the watch never concludes anything: it cannot tell.
    pub fn new(start: Option<FileStamp>) -> BinaryWatch {
        BinaryWatch { start, differing: 0 }
    }

    /// Judges one look (`now` is `None` when the file is gone).
    pub fn look(&mut self, now: Option<FileStamp>) -> Verdict {
        let Some(start) = &self.start else {
            return Verdict::Unchanged;
        };
        if now.as_ref() == Some(start) {
            self.differing = 0;
            return Verdict::Unchanged;
        }
        self.differing += 1;
        if self.differing >= LOOKS_TO_CONFIRM {
            Verdict::Replaced
        } else {
            Verdict::Unchanged
        }
    }
}

/// How long between looks at the program file.
pub const WATCH_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// The thread body: looks at the agent's own file every [`WATCH_EVERY`] and, once it was replaced, logs why and asks the
/// agent to stop (exit code 0), so the supervisor starts the new file. Ends when the agent stops.
pub fn run(shared: &crate::agent::Shared, path: std::path::PathBuf, start: Option<FileStamp>) {
    let mut watch = BinaryWatch::new(start);
    let mut waited = std::time::Duration::ZERO;
    while !shared.stopped() {
        std::thread::sleep(std::time::Duration::from_millis(500));
        waited += std::time::Duration::from_millis(500);
        if waited < WATCH_EVERY {
            continue;
        }
        waited = std::time::Duration::ZERO;
        if watch.look(FileStamp::of(&path)) == Verdict::Replaced {
            log::warn!(
                Reason = "the agent program file was replaced or removed while it ran",
                Version = env!("CARGO_PKG_VERSION"),
                Build = own_build_id().as_str(),
                ExitCode = 0;
                "Agent exiting so the supervisor starts the new build"
            );
            shared.request_stop();
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(len: u64, id: u64) -> Option<FileStamp> {
        Some(FileStamp {
            len,
            modified_ms: 5,
            id: Some(id),
        })
    }

    #[test]
    fn a_replaced_file_is_confirmed_by_the_second_differing_look() {
        let mut w = BinaryWatch::new(stamp(10, 1));
        assert_eq!(w.look(stamp(10, 1)), Verdict::Unchanged);
        assert_eq!(
            w.look(stamp(10, 2)),
            Verdict::Unchanged,
            "one differing look can be an install in progress"
        );
        assert_eq!(w.look(stamp(10, 2)), Verdict::Replaced);
    }

    #[test]
    fn a_single_odd_look_is_forgotten_once_the_file_is_back() {
        let mut w = BinaryWatch::new(stamp(10, 1));
        assert_eq!(w.look(None), Verdict::Unchanged);
        assert_eq!(w.look(stamp(10, 1)), Verdict::Unchanged);
        assert_eq!(w.look(None), Verdict::Unchanged, "the count restarted");
        assert_eq!(
            w.look(None),
            Verdict::Replaced,
            "a file that stays gone (the old bundle was trashed) is a replacement"
        );
    }

    #[test]
    fn a_watch_without_a_start_stamp_never_concludes() {
        let mut w = BinaryWatch::new(None);
        for _ in 0..5 {
            assert_eq!(w.look(stamp(1, 1)), Verdict::Unchanged);
            assert_eq!(w.look(None), Verdict::Unchanged);
        }
    }

    #[test]
    fn a_file_swapped_by_rename_gets_another_stamp_and_the_build_id_follows_the_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("agent");
        std::fs::write(&p, b"build one").unwrap();
        let (s1, id1) = (FileStamp::of(&p), file_build_id(&p).unwrap());
        let next = dir.path().join("agent.new");
        std::fs::write(&next, b"build two!").unwrap();
        std::fs::rename(&next, &p).unwrap();
        let (s2, id2) = (FileStamp::of(&p), file_build_id(&p).unwrap());
        assert_ne!(s1, s2, "the replaced file is told apart");
        assert_ne!(id1, id2);
        assert_eq!(id1.len(), ID_LEN);
        assert_eq!(file_build_id(&dir.path().join("none")), None);
    }

    #[test]
    fn the_key_names_the_build_only_when_it_is_known() {
        assert_eq!(build_key("1.1.0", ""), "1.1.0");
        assert_eq!(build_key("1.1.0", "ab"), "1.1.0#ab");
        assert!(
            matches_key("1.1.0", "1.1.0", "anything"),
            "an app that cannot read its agent file judges by version"
        );
        assert!(matches_key("1.1.0#ab", "1.1.0", "ab"));
        assert!(!matches_key("1.1.0#ab", "1.1.0", "cd"), "same version, other build");
        assert!(!matches_key("1.1.0#ab", "1.1.0", ""), "an older agent that wrote no build id");
        assert!(!matches_key("1.1.0#ab", "1.0.0", "ab"));
    }
}
