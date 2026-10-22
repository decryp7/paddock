//! Walking a folder: local, a mounted network drive or a UNC share. A share that went away can block for a long
//! time inside the OS, so a scan runs on its own thread with a deadline, and failures become statuses the user can
//! act on ("not reachable", "no access", "mount the share first") rather than raw I/O errors.
//!
//! Cost control, because a share can hold hundreds of thousands of entries:
//! - names are judged by [`FolderSource::judge`] (extension, name pattern, the version pattern) before anything else
//!   is read, and only the matches are asked for size and times (on Windows those come with the directory listing);
//! - the walk is streamed (no vector of everything), stops at the entry cap with "too many files" and at the deadline;
//! - when no folder changed since the last look, the listing is reused and only the matches are looked at again;
//! - a new or changed file is not reported until its size and time stay the same for the settle time.

use spotter_core::checksum::Hasher;
use spotter_core::release::{Candidate, Skip};
use spotter_core::source::folder::{ScanStats, SnapEntry, Snapshot, Verdict, SNAPSHOT_SCHEMA};
use spotter_core::source::{FolderEntry, FolderSource, SourceError};
use spotter_core::time::Timestamp;
use std::collections::BTreeMap;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long listing a folder may take when nothing else is said.
pub const LIST_TIMEOUT: Duration = Duration::from_secs(30);

/// A pure check of whether the version scheme can read some text (so impossible names are dropped early).
pub type Readable = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// What to scan and what is remembered from last time.
pub struct ScanRequest {
    /// The source (path, filters, limits).
    pub source: FolderSource,
    /// Whether the version scheme can read a name; `None` keeps every name that passes the other filters.
    pub readable: Option<Readable>,
    /// The previous look, to skip listing and to settle only what changed.
    pub previous: Option<Snapshot>,
    /// How many ignored names to keep (with their reason) for the wizard; 0 for none.
    pub sample_ignored: usize,
    /// Wait for new or changed files to stop changing (off in the wizard, which only looks).
    pub settle: bool,
}

/// What a scan found.
#[derive(Debug, Clone)]
pub struct Scan {
    /// The matches that are stable, as candidates.
    pub candidates: Vec<Candidate>,
    /// The matches as listed (for previews: sizes and times).
    pub entries: Vec<FolderEntry>,
    /// Some ignored names and why (up to `sample_ignored`).
    pub ignored: Vec<(String, Skip)>,
    /// Counts and timing.
    pub stats: ScanStats,
    /// What to remember for the next look.
    pub snapshot: Snapshot,
    /// Whether the share reports creation times for the matches.
    pub created_reported: bool,
}

/// Why a walk stopped early.
#[derive(Debug)]
enum Stop {
    Io(std::io::Error),
    TooMany,
    TooManyMatches,
    TooSlow,
}

impl From<std::io::Error> for Stop {
    fn from(e: std::io::Error) -> Stop {
        Stop::Io(e)
    }
}

/// Whether a path is on a network: a UNC path (`\\server\share`, `//server/share`), a macOS or Linux mount point
/// of a share (`/Volumes`, `/mnt`, `/media`, `/net`, `/Network`).
pub fn is_network_path(path: &str) -> bool {
    path.starts_with(r"\\")
        || path.starts_with("//")
        || ["/Volumes/", "/mnt/", "/media/", "/run/media/", "/net/", "/Network/"]
            .iter()
            .any(|p| path.starts_with(p))
}

/// Scans a folder (see the module documentation).
pub async fn scan(req: ScanRequest) -> Result<Scan, SourceError> {
    let path = req.source.path.clone();
    let limits = req.source.limits;
    let wait = Duration::from_secs(u64::from(limits.timeout_secs) + if req.settle { u64::from(limits.settle_secs) } else { 0 } + 5);
    let (tx, rx) = tokio::sync::oneshot::channel();
    // A plain thread rather than the blocking pool: a hung share must not hold a pool thread the app needs.
    std::thread::spawn(move || {
        let _ = tx.send(scan_now(&req));
    });
    await_scan(rx, wait, &path, limits.max_entries, limits.timeout_secs).await
}

/// The result of the listing thread, or a message when it ended without one or took longer than `wait`.
async fn await_scan(
    rx: tokio::sync::oneshot::Receiver<Result<Scan, Stop>>,
    wait: Duration,
    path: &str,
    max_entries: u32,
    timeout_secs: u32,
) -> Result<Scan, SourceError> {
    match tokio::time::timeout(wait, rx).await {
        Ok(Ok(Ok(scan))) => Ok(scan),
        Ok(Ok(Err(stop))) => Err(explain_stop(path, stop, max_entries, timeout_secs)),
        Ok(Err(_)) => Err(SourceError::Failed(format!("Listing {path} stopped unexpectedly."))),
        Err(_) => Err(SourceError::Unreachable(format!(
            "{path} did not answer within {} seconds. {} Spotter will try again.",
            wait.as_secs(),
            offline_hint(path)
        ))),
    }
}

/// Lists the entries of a folder (no filters, folders included) within `timeout`; a convenience over [`scan`].
pub async fn list(path: &str, timeout: Duration) -> Result<Vec<FolderEntry>, SourceError> {
    use spotter_core::schema::Config;
    let mut source =
        FolderSource::from_config(&Config::new().with("path", path).with("entries", "both")).map_err(SourceError::InvalidConfig)?;
    source.limits.timeout_secs = u32::try_from(timeout.as_secs().max(1)).unwrap_or(u32::MAX);
    let scan = scan(ScanRequest {
        source,
        readable: None,
        previous: None,
        sample_ignored: 0,
        settle: false,
    })
    .await?;
    Ok(scan.entries)
}

fn millis(t: std::time::SystemTime) -> Option<i64> {
    t.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn entry_of(path: &Path, name: String, rel_dir: &str, md: &std::fs::Metadata) -> FolderEntry {
    FolderEntry {
        path: path.to_string_lossy().into_owned(),
        name,
        rel_dir: rel_dir.to_string(),
        is_dir: md.is_dir(),
        size: md.is_file().then_some(md.len()),
        modified: md.modified().ok().and_then(millis).map(Timestamp),
        created: md.created().ok().and_then(millis).map(Timestamp),
    }
}

fn join_rel(rel: &str, name: &str) -> String {
    if rel.is_empty() {
        name.to_string()
    } else {
        format!("{rel}/{name}")
    }
}

fn scan_now(req: &ScanRequest) -> Result<Scan, Stop> {
    let started = Instant::now();
    let src = &req.source;
    let root = PathBuf::from(&src.path);
    let root_md = std::fs::metadata(&root)?;
    if !root_md.is_dir() {
        return Err(Stop::Io(std::io::Error::from(ErrorKind::NotADirectory)));
    }
    let (mut entries, dirs, mut stats, ignored, reused) = match reuse(req, &root) {
        // A reused listing judged nothing, so there is nothing to sample.
        Some((entries, dirs, stats)) => (entries, dirs, stats, Vec::new(), true),
        None => walk(req, &root, started)?,
    };
    let known = req.previous.as_ref();
    let mut settling = 0;
    if req.settle && src.limits.settle_secs > 0 {
        settling = settle(&root, &mut entries, known, Duration::from_secs(u64::from(src.limits.settle_secs)));
    }
    stats.settling = settling;
    if src.limits.hash_below_mib > 0 {
        hash_small(src, &mut entries, known);
    }
    let mut snapshot_entries: Vec<SnapEntry> = entries
        .iter()
        .map(|(e, hash)| SnapEntry {
            name: e.name.clone(),
            rel_dir: e.rel_dir.clone(),
            is_dir: e.is_dir,
            size: e.size,
            modified: e.modified.map(|t| t.0),
            hash: hash.clone(),
        })
        .collect();
    snapshot_entries.sort_by_key(SnapEntry::key);
    stats.matched = snapshot_entries.len() as u64;
    stats.took_ms = started.elapsed().as_millis() as u64;
    stats.reused = reused;
    let listed: Vec<FolderEntry> = entries.into_iter().map(|(e, _)| e).collect();
    let created_reported = listed.iter().any(|e| e.created.is_some());
    let candidates = listed
        .iter()
        .map(|e| src.candidate(e.clone(), src.subject(&e.name, &e.rel_dir)))
        .collect();
    Ok(Scan {
        candidates,
        entries: listed,
        ignored,
        stats,
        snapshot: Snapshot {
            schema: SNAPSHOT_SCHEMA,
            fingerprint: String::new(),
            dirs,
            entries: snapshot_entries,
            // Files still settling mean the next look must list again, or they would never be found.
            since_full: if reused { known.map_or(0, |p| p.since_full + 1) } else { 0 }.max(if settling > 0 {
                src.limits.relist_every
            } else {
                0
            }),
        },
        created_reported,
    })
}

type Matched = Vec<(FolderEntry, Option<String>)>;
type Dirs = BTreeMap<String, Option<i64>>;

/// The previous listing, looked at again, when no folder changed and a full listing is not due.
fn reuse(req: &ScanRequest, root: &Path) -> Option<(Matched, Dirs, ScanStats)> {
    let prev = req.previous.as_ref().filter(|_| req.sample_ignored == 0)?;
    let now_dirs: Dirs = prev
        .dirs
        .keys()
        .map(|rel| {
            let dir = if rel.is_empty() { root.to_path_buf() } else { root.join(rel) };
            (
                rel.clone(),
                std::fs::metadata(dir).ok().and_then(|m| m.modified().ok()).and_then(millis),
            )
        })
        .collect();
    if !prev.can_reuse(&now_dirs, req.source.limits.relist_every) {
        return None;
    }
    let mut out = Vec::with_capacity(prev.entries.len());
    for e in &prev.entries {
        let path = if e.rel_dir.is_empty() {
            root.join(&e.name)
        } else {
            root.join(&e.rel_dir).join(&e.name)
        };
        // A match that vanished or cannot be read means something changed that folder times did not show.
        let md = std::fs::metadata(&path).ok()?;
        out.push((entry_of(&path, e.name.clone(), &e.rel_dir, &md), e.hash.clone()));
    }
    Some((out, now_dirs, ScanStats::default()))
}

type Walked = (Matched, Dirs, ScanStats, Vec<(String, Skip)>, bool);

fn walk(req: &ScanRequest, root: &Path, started: Instant) -> Result<Walked, Stop> {
    let src = &req.source;
    let deadline = started + Duration::from_secs(u64::from(src.limits.timeout_secs));
    let max = u64::from(src.limits.max_entries);
    let mut stack = vec![(root.to_path_buf(), String::new(), 0u32)];
    let (mut dirs, mut matched) = (Dirs::new(), Matched::new());
    let (mut scanned, mut folders) = (0u64, 0u64);
    let mut ignored_seen = Vec::new();
    while let Some((dir, rel, level)) = stack.pop() {
        let dir_md = std::fs::metadata(&dir);
        let read = std::fs::read_dir(&dir);
        let read = match (read, level) {
            (Ok(r), _) => r,
            (Err(e), 0) => return Err(e.into()),
            // A sub-folder that cannot be read is skipped; the rest of the share still counts.
            (Err(_), _) => continue,
        };
        folders += 1;
        dirs.insert(rel.clone(), dir_md.ok().and_then(|m| m.modified().ok()).and_then(millis));
        for item in read {
            let Ok(item) = item else { continue };
            scanned += 1;
            if scanned > max {
                return Err(Stop::TooMany);
            }
            if scanned % 256 == 0 && Instant::now() > deadline {
                return Err(Stop::TooSlow);
            }
            // Names that are not valid Unicode cannot hold a version Spotter can show; skip them.
            let Ok(name) = item.file_name().into_string() else { continue };
            let Ok(ft) = item.file_type() else { continue };
            let is_dir = ft.is_dir();
            if is_dir && level < src.depth {
                stack.push((item.path(), join_rel(&rel, &name), level + 1));
            }
            let is_dir_entry = is_dir || (ft.is_symlink() && std::fs::metadata(item.path()).is_ok_and(|m| m.is_dir()));
            match src.judge(
                &name,
                &rel,
                is_dir_entry,
                req.readable.as_deref().map(|f| f as &dyn Fn(&str) -> bool),
            ) {
                Verdict::Keep(_) => {
                    // Only now is the size and time asked for (on Windows they came with the listing).
                    let md = if ft.is_symlink() {
                        std::fs::metadata(item.path())
                    } else {
                        item.metadata()
                    };
                    if let Ok(md) = md {
                        if matched.len() >= spotter_core::limits::MAX_FOLDER_MATCHES {
                            return Err(Stop::TooManyMatches);
                        }
                        matched.push((entry_of(&item.path(), name, &rel, &md), None));
                    }
                }
                Verdict::Skip(why) if ignored_seen.len() < req.sample_ignored => ignored_seen.push((join_rel(&rel, &name), why)),
                Verdict::Skip(_) | Verdict::NotThisKind => {}
            }
        }
    }
    matched.sort_by(|a, b| (&a.0.rel_dir, &a.0.name).cmp(&(&b.0.rel_dir, &b.0.name)));
    let stats = ScanStats {
        scanned,
        folders,
        ..ScanStats::default()
    };
    Ok((matched, dirs, stats, ignored_seen, false))
}

/// Waits `settle`, looks at the new and changed files again and drops those that changed meanwhile.
/// Returns how many were dropped.
fn settle(root: &Path, entries: &mut Matched, previous: Option<&Snapshot>, settle: Duration) -> u64 {
    let unsettled = |e: &FolderEntry| {
        previous
            .and_then(|p| p.entry(&join_rel(&e.rel_dir, &e.name)))
            .is_none_or(|k| k.size != e.size || k.modified != e.modified.map(|t| t.0))
    };
    if !entries.iter().any(|(e, _)| unsettled(e)) {
        return 0;
    }
    std::thread::sleep(settle);
    let before = entries.len();
    entries.retain(|(e, _)| {
        if !unsettled(e) {
            return true;
        }
        let path = if e.rel_dir.is_empty() {
            root.join(&e.name)
        } else {
            root.join(&e.rel_dir).join(&e.name)
        };
        std::fs::metadata(path).is_ok_and(|md| {
            md.len() == e.size.unwrap_or(md.len()) && millis(md.modified().unwrap_or(std::time::UNIX_EPOCH)) == e.modified.map(|t| t.0)
        })
    });
    (before - entries.len()) as u64
}

/// Hashes small files that changed. A file whose size and hash are as before but whose time moved was only touched:
/// it keeps its old time so it is not taken for a new release.
fn hash_small(src: &FolderSource, entries: &mut Matched, previous: Option<&Snapshot>) {
    let limit = u64::from(src.limits.hash_below_mib) << 20;
    for (e, hash) in entries.iter_mut() {
        let key = join_rel(&e.rel_dir, &e.name);
        let before = previous.and_then(|p| p.entry(&key));
        let unchanged = before.is_some_and(|k| k.size == e.size && k.modified == e.modified.map(|t| t.0));
        if unchanged {
            *hash = before.and_then(|k| k.hash.clone());
            continue;
        }
        if e.is_dir || e.size.is_none_or(|s| s > limit) {
            continue;
        }
        *hash = hash_file(Path::new(&e.path));
        if let (Some(k), Some(h)) = (before, hash.as_ref()) {
            if k.size == e.size && k.hash.as_ref() == Some(h) {
                e.modified = k.modified.map(Timestamp);
            }
        }
    }
}

fn hash_file(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(hasher.finish().to_hex())
}

fn explain_stop(path: &str, stop: Stop, max_entries: u32, timeout_secs: u32) -> SourceError {
    match stop {
        Stop::Io(e) => explain(path, e),
        Stop::TooMany => SourceError::InvalidConfig(format!(
            "Too many files: {path} holds more than {max_entries} entries. Narrow the folder or the folder levels, or raise \"Most entries per check\" under More options."
        )),
        Stop::TooManyMatches => SourceError::InvalidConfig(format!(
            "Too many matches: more than {} files in {path} match. Narrow the name pattern, the file types or the folder levels.",
            spotter_core::limits::MAX_FOLDER_MATCHES
        )),
        Stop::TooSlow => SourceError::Unreachable(format!(
            "Listing {path} took longer than {timeout_secs} seconds. The share is slow or very large: narrow the folder or the folder levels. Spotter will try again."
        )),
    }
}

fn offline_hint(path: &str) -> &'static str {
    if path.starts_with(r"\\") || path.starts_with("//") {
        "The server may be offline, or you may need to connect to the network or VPN."
    } else if is_network_path(path) {
        "The share may be offline or slow; check that it is still mounted."
    } else {
        "The drive may be slow or disconnected."
    }
}

/// Platform error numbers for the cases that need their own advice.
mod errno {
    #[cfg(windows)]
    pub const STALE: &[i32] = &[];
    #[cfg(target_os = "macos")]
    pub const STALE: &[i32] = &[70, 5];
    #[cfg(all(unix, not(target_os = "macos")))]
    pub const STALE: &[i32] = &[116, 5];

    #[cfg(windows)]
    pub const HOST_UNREACHABLE: &[i32] = &[53, 64, 67, 1222, 1231, 1232];
    #[cfg(target_os = "macos")]
    pub const HOST_UNREACHABLE: &[i32] = &[64, 65, 51, 60, 61];
    #[cfg(all(unix, not(target_os = "macos")))]
    pub const HOST_UNREACHABLE: &[i32] = &[112, 113, 101, 110, 111];

    /// Credentials missing or refused (Windows network logon errors).
    #[cfg(windows)]
    pub const LOGON: &[i32] = &[86, 1219, 1244, 1326, 1327, 1328, 1330, 1331];
    #[cfg(not(windows))]
    pub const LOGON: &[i32] = &[];
}

pub(crate) fn explain(path: &str, e: std::io::Error) -> SourceError {
    let code = e.raw_os_error();
    let has = |list: &[i32]| code.is_some_and(|c| list.contains(&c));
    if has(errno::LOGON) {
        return SourceError::Unauthorized(format!(
            "{path} needs credentials. Open it once in Explorer and sign in (tick \"Remember my credentials\"), or connect it with \"net use\"; Spotter uses your own sign-in."
        ));
    }
    if has(errno::STALE) && is_network_path(path) {
        return SourceError::Unreachable(format!(
            "{path} is a stale mount ({e}). Disconnect and mount the share again; Spotter will try again."
        ));
    }
    if has(errno::HOST_UNREACHABLE) {
        return SourceError::Unreachable(format!(
            "{path} is not reachable ({e}). {} Spotter will try again.",
            offline_hint(path)
        ));
    }
    match e.kind() {
        ErrorKind::PermissionDenied => SourceError::Unauthorized(format!(
            "You have no access to {path}. Spotter uses your own sign-in: open the folder once in Explorer or Finder to connect, ask for access{}.",
            if cfg!(target_os = "macos") {
                ", or allow Spotter under System Settings > Privacy & Security > Files and Folders"
            } else {
                ""
            }
        )),
        ErrorKind::NotFound if path.starts_with(r"\\") || path.starts_with("//") => SourceError::Unreachable(format!(
            "{path} is not reachable. Check the server and share names, that you are on the network or VPN, and that the share is online; Spotter will try again."
        )),
        ErrorKind::NotFound if is_network_path(path) => SourceError::Unreachable(format!(
            "{path} is not there. Mount the share first (Finder: Go > Connect to Server; Linux: mount it or add it to fstab); Spotter will try again."
        )),
        ErrorKind::NotFound => SourceError::NotFound(format!("The folder {path} does not exist.")),
        ErrorKind::NotADirectory => SourceError::InvalidConfig(format!("{path} is a file, not a folder.")),
        _ if is_network_path(path) => SourceError::Unreachable(format!("{path} is not reachable ({e}). {} Spotter will try again.", offline_hint(path))),
        _ => SourceError::Failed(format!("{path} could not be read ({e}).")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spotter_core::schema::Config;

    fn source(dir: &Path, extra: &[(&str, &str)]) -> FolderSource {
        let mut c = Config::new().with("path", &dir.to_string_lossy()).with("settleSeconds", "0");
        for (k, v) in extra {
            c = c.with(k, v);
        }
        FolderSource::from_config(&c).unwrap()
    }

    fn request(source: FolderSource) -> ScanRequest {
        ScanRequest {
            source,
            readable: None,
            previous: None,
            sample_ignored: 0,
            settle: true,
        }
    }

    fn touch(dir: &Path, rel: &str, body: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn names(s: &Scan) -> Vec<String> {
        s.candidates.iter().map(|c| c.name.clone()).collect()
    }

    #[tokio::test]
    async fn folders_list_with_metadata() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "R1.01.00T10.txt", "abc");
        std::fs::create_dir(d.path().join("R1.02.00T01")).unwrap();
        let mut entries = list(&d.path().to_string_lossy(), LIST_TIMEOUT).await.unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].size, entries[0].is_dir), (Some(3), false));
        assert!(entries[0].modified.is_some());
        assert_eq!((entries[1].size, entries[1].is_dir), (None, true));
    }

    #[tokio::test]
    async fn only_matching_names_are_kept_and_depth_is_honoured() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "Acme-PRO-R3.10.00T32.iso", "iso");
        touch(d.path(), "Acme-PRO-R3.10.00T32.txt", "txt");
        touch(d.path(), "notes.md", "x");
        touch(d.path(), "old/Acme-PRO-R2.00.00T01.iso", "old");
        touch(d.path(), "old/deeper/Acme-PRO-R1.00.00T01.iso", "older");
        let flat = scan(request(source(d.path(), &[("extensions", "iso")]))).await.unwrap();
        assert_eq!(names(&flat), ["Acme-PRO-R3.10.00T32.iso"]);
        assert_eq!(
            (flat.stats.scanned, flat.stats.matched, flat.stats.folders),
            (4, 1, 1),
            "{:?}",
            flat.stats
        );
        let one = scan(request(source(d.path(), &[("extensions", "iso"), ("depth", "1")])))
            .await
            .unwrap();
        assert_eq!(
            names(&one),
            ["Acme-PRO-R3.10.00T32.iso", "Acme-PRO-R2.00.00T01.iso"],
            "top level first"
        );
        let two = scan(request(source(d.path(), &[("extensions", "iso"), ("depth", "2")])))
            .await
            .unwrap();
        assert_eq!(two.candidates.len(), 3);
        // The version is read from the folder name when asked.
        touch(d.path(), "R4.00.00T01/setup.exe", "s");
        let by_folder = scan(request(source(
            d.path(),
            &[("depth", "1"), ("matchAgainst", "folder"), ("pattern", "setup.exe")],
        )))
        .await
        .unwrap();
        assert_eq!(by_folder.candidates[0].version_text, "R4.00.00T01");
        assert_eq!(by_folder.candidates[0].name, "setup.exe");
        let by_path = scan(request(source(
            d.path(),
            &[("depth", "1"), ("matchAgainst", "path"), ("pattern", "setup.exe")],
        )))
        .await
        .unwrap();
        assert_eq!(by_path.candidates[0].version_text, "R4.00.00T01/setup.exe");
    }

    #[tokio::test]
    async fn the_version_pattern_drops_names_before_they_are_looked_at_and_ignored_names_are_sampled() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "Acme-PRO-R3.10.00T32.iso", "a");
        touch(d.path(), "readme.txt", "b");
        touch(d.path(), "other.zip", "c");
        let mut req = request(source(d.path(), &[("extensions", "iso,txt")]));
        req.readable = Some(Arc::new(|t: &str| t.starts_with("Acme")));
        req.sample_ignored = 10;
        let s = scan(req).await.unwrap();
        assert_eq!(names(&s), ["Acme-PRO-R3.10.00T32.iso"]);
        let mut why: Vec<_> = s.ignored.iter().map(|(n, w)| (n.as_str(), w.clone())).collect();
        why.sort_by_key(|x| x.0);
        assert_eq!(why.len(), 2);
        assert_eq!(why[0].0, "other.zip");
        assert_eq!(why[0].1, Skip::Extension);
        assert!(matches!(why[1].1, Skip::Unread(_)));
        let mut few = request(source(d.path(), &[]));
        few.sample_ignored = 1;
        assert_eq!(scan(few).await.unwrap().ignored.len(), 0, "everything matches");
    }

    #[tokio::test]
    async fn too_many_entries_is_a_message_not_a_silent_miss() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..150 {
            touch(d.path(), &format!("f{i}.txt"), "x");
        }
        let mut src = source(d.path(), &[]);
        src.limits.max_entries = 100;
        let e = scan(request(src.clone())).await.unwrap_err();
        assert!(matches!(e, SourceError::InvalidConfig(_)), "{e:?}");
        assert!(e.message().contains("Too many files") && e.message().contains("Narrow"), "{e}");
        src.limits.max_entries = 150;
        assert_eq!(scan(request(src)).await.unwrap().stats.scanned, 150);
    }

    #[tokio::test]
    async fn a_slow_listing_stops_at_its_deadline() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..600 {
            touch(d.path(), &format!("f{i}.txt"), "x");
        }
        let mut src = source(d.path(), &[]);
        src.limits.timeout_secs = 0;
        let e = scan(request(src)).await.unwrap_err();
        assert!(
            matches!(e, SourceError::Unreachable(_)) && e.message().contains("took longer"),
            "{e:?}"
        );
    }

    #[tokio::test]
    async fn an_unchanged_share_is_not_listed_again_but_changes_are_found() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "a-1.zip", "one");
        touch(d.path(), "sub/b-2.zip", "two");
        let src = source(d.path(), &[("depth", "1"), ("relistEvery", "4")]);
        let first = scan(request(src.clone())).await.unwrap();
        assert!(!first.stats.reused && first.stats.scanned == 3, "{:?}", first.stats);
        let mut again = request(src.clone());
        again.previous = Some(first.snapshot.clone());
        let second = scan(again).await.unwrap();
        assert!(second.stats.reused, "no folder changed");
        assert_eq!((second.stats.scanned, second.candidates.len()), (0, 2));
        // A file overwritten in place does not change the folder, but is seen: it was re-looked at.
        std::thread::sleep(Duration::from_millis(20));
        touch(d.path(), "a-1.zip", "one, longer");
        let mut third = request(src.clone());
        third.previous = Some(second.snapshot.clone());
        let third = scan(third).await.unwrap();
        assert!(third.stats.reused);
        assert_eq!(third.entries[0].size, Some(11));
        // A new file changes the folder: listed again.
        std::thread::sleep(Duration::from_millis(1100));
        touch(d.path(), "c-3.zip", "three");
        let mut fourth = request(src.clone());
        fourth.previous = Some(third.snapshot.clone());
        let fourth = scan(fourth).await.unwrap();
        assert!(!fourth.stats.reused);
        assert_eq!(fourth.candidates.len(), 3);
        // A full listing at least every N checks.
        let mut snap = fourth.snapshot.clone();
        for round in 0..4 {
            let mut r = request(src.clone());
            r.previous = Some(snap.clone());
            let s = scan(r).await.unwrap();
            assert_eq!(s.stats.reused, round < 3, "round {round}");
            snap = s.snapshot;
        }
        // A deleted match forces a listing.
        std::fs::remove_file(d.path().join("c-3.zip")).unwrap();
        let mut r = request(src);
        r.previous = Some(fourth.snapshot);
        let gone = scan(r).await.unwrap();
        assert_eq!(gone.candidates.len(), 2);
    }

    #[tokio::test]
    async fn a_file_that_is_still_being_copied_is_not_announced() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "stable-1.zip", "done");
        touch(d.path(), "growing-2.zip", "a");
        let path = d.path().join("growing-2.zip");
        let writer = std::thread::spawn(move || {
            use std::io::Write;
            for _ in 0..8 {
                std::thread::sleep(Duration::from_millis(60));
                let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
                f.write_all(b"more").unwrap();
            }
        });
        let mut src = source(d.path(), &[]);
        src.limits.settle_secs = 1;
        let first = scan(request(src.clone())).await.unwrap();
        writer.join().unwrap();
        assert_eq!(names(&first), ["stable-1.zip"], "{:?}", first.stats);
        assert_eq!(first.stats.settling, 1);
        assert!(
            first.snapshot.since_full >= src.limits.relist_every,
            "the next look must list again"
        );
        // It has stopped changing now: the next look announces it.
        let mut next = request(src);
        next.previous = Some(first.snapshot);
        let second = scan(next).await.unwrap();
        assert_eq!(names(&second), ["growing-2.zip", "stable-1.zip"]);
        assert_eq!(second.stats.settling, 0);
    }

    #[tokio::test]
    async fn a_touched_small_file_keeps_its_time_when_hashing_is_on() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "setup.exe", "same bytes");
        let src = source(d.path(), &[("hashBelowMib", "1")]);
        let first = scan(request(src.clone())).await.unwrap();
        assert!(first.snapshot.entries[0].hash.is_some());
        std::thread::sleep(Duration::from_millis(1100));
        touch(d.path(), "setup.exe", "same bytes");
        let mut r = request(src.clone());
        r.previous = Some(first.snapshot.clone());
        let touched = scan(r).await.unwrap();
        assert_eq!(
            touched.entries[0].modified, first.entries[0].modified,
            "a touch is not a new release"
        );
        std::thread::sleep(Duration::from_millis(1100));
        touch(d.path(), "setup.exe", "other bytes!");
        let mut r = request(src);
        r.previous = Some(touched.snapshot);
        let changed = scan(r).await.unwrap();
        assert_ne!(changed.entries[0].modified, first.entries[0].modified);
    }

    #[tokio::test]
    async fn a_big_folder_is_prefiltered_cheaply() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..30_000 {
            let name = if i % 1000 == 0 {
                format!("Acme-PRO-R3.{:02}.00T01.zip", i / 1000)
            } else {
                format!("noise-{i}.log")
            };
            std::fs::write(d.path().join(name), b"").unwrap();
        }
        let mut req = request(source(d.path(), &[("extensions", "zip")]));
        req.source.limits.settle_secs = 0;
        let started = Instant::now();
        let s = scan(req).await.unwrap();
        let took = started.elapsed();
        eprintln!("scanned 30000 entries in {took:?}");
        assert_eq!((s.stats.scanned, s.stats.matched), (30_000, 30));
        // Lenient: a local folder of this size takes a fraction of a second; only a pathological slowdown fails.
        assert!(took < Duration::from_secs(30), "{took:?}");
        let mut capped = request(source(d.path(), &[("maxEntries", "20000")]));
        capped.source.limits.settle_secs = 0;
        assert!(matches!(scan(capped).await.unwrap_err(), SourceError::InvalidConfig(_)));
    }

    #[tokio::test]
    async fn missing_folders_files_and_shares_explain_themselves() {
        let d = tempfile::tempdir().unwrap();
        let missing = d.path().join("nope").to_string_lossy().into_owned();
        assert!(matches!(list(&missing, LIST_TIMEOUT).await, Err(SourceError::NotFound(_))));
        let file = d.path().join("f");
        std::fs::write(&file, b"x").unwrap();
        let e = list(&file.to_string_lossy(), LIST_TIMEOUT).await.unwrap_err();
        assert!(matches!(e, SourceError::InvalidConfig(_) | SourceError::Failed(_)), "{e:?}");
    }

    #[test]
    fn errors_map_to_statuses_with_a_next_step() {
        let io = |k: ErrorKind| std::io::Error::from(k);
        let denied = explain("/x", io(ErrorKind::PermissionDenied));
        assert!(matches!(denied, SourceError::Unauthorized(_)) && denied.message().contains("access"));
        let unc = explain(r"\\srv\share", io(ErrorKind::NotFound));
        assert!(
            matches!(unc, SourceError::Unreachable(_)) && unc.message().contains("server and share names"),
            "{unc}"
        );
        for mount in ["/Volumes/Builds", "/mnt/builds", "/media/u/builds"] {
            let e = explain(mount, io(ErrorKind::NotFound));
            assert!(
                matches!(e, SourceError::Unreachable(_)) && e.message().contains("Mount the share first"),
                "{mount}: {e}"
            );
        }
        assert!(matches!(explain("/x", io(ErrorKind::NotFound)), SourceError::NotFound(_)));
        assert!(matches!(explain("/x", io(ErrorKind::NotADirectory)), SourceError::InvalidConfig(_)));
        assert!(matches!(
            explain("//srv/share", io(ErrorKind::TimedOut)),
            SourceError::Unreachable(_)
        ));
        assert!(matches!(explain("/x", io(ErrorKind::TimedOut)), SourceError::Failed(_)));
        assert!(is_network_path(r"\\s\x") && !is_network_path("C:\\x") && is_network_path("/mnt/x") && !is_network_path("/home/x"));
        // Stale mounts and unreachable hosts by OS error number.
        #[cfg(unix)]
        {
            let stale = explain("/Volumes/Builds", std::io::Error::from_raw_os_error(errno::STALE[0]));
            assert!(stale.message().contains("stale mount"), "{stale}");
            let down = explain("/mnt/x", std::io::Error::from_raw_os_error(errno::HOST_UNREACHABLE[0]));
            assert!(matches!(down, SourceError::Unreachable(_)), "{down}");
            let local_stale = explain("/home/x", std::io::Error::from_raw_os_error(errno::STALE[0]));
            assert!(!local_stale.message().contains("stale mount"));
        }
        #[cfg(windows)]
        {
            let logon = explain(r"\\srv\share", std::io::Error::from_raw_os_error(1326));
            assert!(matches!(logon, SourceError::Unauthorized(_)) && logon.message().contains("credentials"));
        }
        for (path, hint) in [(r"\\srv\s", "VPN"), ("/mnt/x", "mounted"), ("/home/x", "disconnected")] {
            assert!(offline_hint(path).contains(hint), "{path}");
        }
        let stop = |s| explain_stop("/p", s, 10, 5);
        assert!(stop(Stop::TooMany).message().contains("Too many files"));
        assert!(stop(Stop::TooSlow).message().contains("5 seconds"));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn an_unreachable_share_is_a_status_not_a_hang() {
        // A reserved, never-resolving host name: the OS reports it quickly or the deadline does.
        let e = list(r"\\spotter-test-host.invalid\share", Duration::from_secs(20))
            .await
            .unwrap_err();
        assert!(matches!(e, SourceError::Unreachable(_)), "{e:?}");
    }

    #[tokio::test]
    async fn a_listing_thread_that_vanishes_or_hangs_is_a_message() {
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<Scan, Stop>>();
        drop(tx);
        let e = await_scan(rx, Duration::from_secs(1), "/x", 10, 5).await.unwrap_err();
        assert!(matches!(&e, SourceError::Failed(m) if m.contains("stopped unexpectedly")), "{e:?}");
        let (_tx, rx) = tokio::sync::oneshot::channel::<Result<Scan, Stop>>();
        let e = await_scan(rx, Duration::from_millis(20), "//server/share", 10, 5)
            .await
            .unwrap_err();
        assert!(
            matches!(&e, SourceError::Unreachable(m) if m.contains("did not answer") && m.contains("offline")),
            "{e:?}"
        );
    }

    #[test]
    fn stop_reasons_are_explained() {
        let e = explain_stop("/x", Stop::TooManyMatches, 10, 5);
        assert!(
            matches!(&e, SourceError::InvalidConfig(m) if m.contains("Too many matches") && m.contains("/x")),
            "{e:?}"
        );
        assert!(matches!(explain_stop("/x", Stop::TooMany, 10, 5), SourceError::InvalidConfig(_)));
        assert!(matches!(explain_stop("/x", Stop::TooSlow, 10, 5), SourceError::Unreachable(_)));
    }

    #[tokio::test]
    async fn more_matches_than_the_cap_is_a_message() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..=spotter_core::limits::MAX_FOLDER_MATCHES {
            std::fs::write(d.path().join(format!("f{i}.bin")), b"").unwrap();
        }
        let src = source(d.path(), &[("maxEntries", "1000000")]);
        let e = scan(request(src)).await.unwrap_err();
        assert!(
            matches!(&e, SourceError::InvalidConfig(m) if m.contains("Too many matches")),
            "{e:?}"
        );
    }

    #[tokio::test]
    async fn unchanged_files_keep_their_hash_and_settling_is_skipped() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "a/small.bin", "abc");
        touch(d.path(), "a/big.bin", &"x".repeat(1_100_000));
        touch(d.path(), "top.bin", "top");
        let extra = [("depth", "1"), ("hashBelowMib", "1"), ("settleSeconds", "1")];
        let first = scan(request(source(d.path(), &extra))).await.unwrap();
        let hashed = first.snapshot.entries.iter().filter(|e| e.hash.is_some()).count();
        assert_eq!(hashed, 2, "the file above the limit is not hashed");
        let mut again = request(source(d.path(), &extra));
        again.previous = Some(first.snapshot.clone());
        let second = scan(again).await.unwrap();
        assert_eq!(second.stats.settling, 0);
        assert_eq!(second.snapshot.entries.iter().filter(|e| e.hash.is_some()).count(), 2);
    }

    #[tokio::test]
    async fn new_files_in_sub_folders_settle_before_they_count() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "sub/new.bin", "abc");
        let mut req = request(source(d.path(), &[("depth", "1"), ("settleSeconds", "1")]));
        req.settle = true;
        let s = scan(req).await.unwrap();
        assert_eq!(names(&s), ["new.bin"]);
        assert_eq!(s.stats.settling, 0, "a file that did not change while waiting stays");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinks_count_and_unreadable_folders_are_handled() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "real.bin", "abc");
        symlink(d.path().join("real.bin"), d.path().join("link.bin")).unwrap();
        touch(d.path(), "locked/inner.bin", "x");
        let locked = d.path().join("locked");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable = std::fs::read_dir(&locked).is_ok();
        let s = scan(request(source(d.path(), &[("depth", "1")]))).await.unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(names(&s).contains(&"link.bin".to_string()), "{:?}", names(&s));
        if !readable {
            assert!(!names(&s).contains(&"inner.bin".to_string()), "an unreadable sub-folder is skipped");
        }
        // An unreadable root is an access message.
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o000)).unwrap();
        let denied = std::fs::read_dir(d.path()).is_err();
        let r = scan(request(source(d.path(), &[]))).await;
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        if denied {
            assert!(matches!(r, Err(SourceError::Unauthorized(_))), "{r:?}");
        }
    }
}
