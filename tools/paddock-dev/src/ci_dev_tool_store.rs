//! `dev ci dev-tool-store publish`: the runner-local store of `./dev`'s own binary on a Linux CI runner (user decision 109,
//! 2026-11-27; the Mac and Windows hosts keep the tool by its hash in the CI home already).
//!
//! The Actions cache service of the runners timed out in run 9180 (171 s per Rust job for restore, save and "Confirm the save"), and
//! a miss builds `./dev` from source (158 s). The store is a folder of the already-mounted `/sccache` volume,
//! `/sccache/paddock-dev-tools/<hash>/<files of the tool's cache entry>`, so no runner mount changes. It is kept by the one producer,
//! the plan job (ER-51):
//!
//!  - owner: an entry is written by this command only, and the lock names its holder (`.lock-owner`: pid, run, job, time);
//!  - lock: one writer at a time, a kernel file lock (`<store>/.lock`) that goes with the process, so a crashed writer leaves none;
//!  - atomic: an entry is built in a temp folder of the store and renamed into place, so a reader never sees half an entry; a
//!    reader copies the entry into its own `~/.paddock-dev/bin/<hash>` and runs that copy (ADR 0111: never the shared file), and
//!    `./dev self hash` still verifies it for its hash, so a damaged entry costs a rebuild, never a wrong binary;
//!  - size limit: an entry over [`MAX_ENTRY_BYTES`] is refused; the store keeps at most [`MAX_ENTRIES`] entries and [`MAX_BYTES`];
//!  - eviction: after a publish, the least recently used entries beyond those limits go (a reader marks the entry it used with
//!    `touch`), never the entry just written, and only folders named by a hash or this command's own temp names.
//!
//! The reader is `conformance/ci/dev-tool-cache.sh store-restore` (shell, because it runs before any `./dev` exists). Both read the
//! one layout above. Nothing here fails the job: a store that cannot be written is one `::warning::` and the Actions cache stays
//! the fallback.

use clap::{Args, Subcommand};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Where the store lives when nothing says otherwise: a folder of the runner's `/sccache` volume.
pub const DEFAULT_STORE: &str = "/sccache/paddock-dev-tools";
/// The largest entry accepted (the tool is about 30 MiB).
pub const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
/// The most entries kept: main's hash outlives a stream of branch hashes.
pub const MAX_ENTRIES: usize = 6;
/// The most bytes kept over all entries.
pub const MAX_BYTES: u64 = 1024 * 1024 * 1024;
/// A temp folder this old belongs to a writer that is gone (no publish lasts a day).
const STALE_TEMP: Duration = Duration::from_secs(24 * 3600);

/// `dev ci dev-tool-store`.
#[derive(Args)]
pub struct StoreArgs {
    #[command(subcommand)]
    command: StoreCommand,
}

/// What the store command can do.
#[derive(Subcommand)]
enum StoreCommand {
    /// Keep the verified tool of this hash in the store (the plan job's step). Never fails the job.
    Publish(PublishArgs),
}

/// `publish`'s arguments.
#[derive(Args)]
struct PublishArgs {
    /// The tool's input hash (40 hex digits): the entry's name.
    #[arg(long)]
    hash: String,
    /// The tool's cache entry in this job's home (`<home>/bin/<hash>`): its files are what is kept.
    #[arg(long)]
    from: PathBuf,
    /// The store folder (default: `PADDOCK_DEV_TOOL_STORE`, else `/sccache/paddock-dev-tools`).
    #[arg(long)]
    store: Option<PathBuf>,
    /// Replace an entry that is there (a reader found it unusable).
    #[arg(long)]
    replace: bool,
    /// How long to wait for another writer's lock, in seconds.
    #[arg(long, default_value_t = 120)]
    lock_wait_secs: u64,
}

/// What a publish did.
#[derive(Debug, PartialEq, Eq)]
pub enum Published {
    /// Written; the names of the entries it evicted.
    Written(Vec<String>),
    /// The entry was there already (marked as used).
    Present,
    /// There is no store here (its parent volume is not mounted): nothing to keep the tool in.
    NoStore,
}

/// The limits of one store, so a test can use small ones.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Largest single entry.
    pub entry_bytes: u64,
    /// Most entries.
    pub entries: usize,
    /// Most bytes over all entries.
    pub bytes: u64,
    /// Longest wait for the writer lock.
    pub lock_wait: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            entry_bytes: MAX_ENTRY_BYTES,
            entries: MAX_ENTRIES,
            bytes: MAX_BYTES,
            lock_wait: Duration::from_secs(120),
        }
    }
}

/// Whether `s` names a tool by its input hash (40 lowercase hex digits).
pub fn is_hash(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn size_of(dir: &Path) -> std::io::Result<u64> {
    let mut n = 0;
    for e in fs::read_dir(dir)? {
        let e = e?;
        let m = e.metadata()?;
        n += if m.is_dir() { size_of(&e.path())? } else { m.len() };
    }
    Ok(n)
}

fn touch(dir: &Path) {
    // A hint only (the eviction order): a failure costs a rebuild later, never a wrong binary.
    let _ = File::open(dir).and_then(|f| f.set_modified(SystemTime::now()));
}

/// The writer lock: the kernel's file lock on `<store>/.lock` plus an owner note beside it. Released when dropped.
struct WriterLock {
    file: File,
}

impl WriterLock {
    fn take(store: &Path, wait: Duration, owner: &str) -> Result<WriterLock, String> {
        let path = store.join(".lock");
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| format!("cannot open the store lock {}: {e}", path.display()))?;
        let t0 = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) => {
                    if t0.elapsed() >= wait {
                        return Err(format!(
                            "another writer held the store lock {} for {} s",
                            path.display(),
                            wait.as_secs()
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
                Err(fs::TryLockError::Error(e)) => return Err(format!("cannot lock {}: {e}", path.display())),
            }
        }
        // The owner note is information for a person who asks who holds the store; the lock itself is the kernel's.
        let _ = fs::write(store.join(".lock-owner"), owner);
        Ok(WriterLock { file })
    }
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn copy_entry(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir(to)?;
    for e in fs::read_dir(from)? {
        let e = e?;
        let m = e.metadata()?;
        // Files only: the tool's entry holds the binary (and a temp copy a crashed run left, which starts with a dot and is skipped).
        if m.is_file() && !e.file_name().to_string_lossy().starts_with('.') {
            fs::copy(e.path(), to.join(e.file_name()))?;
        }
    }
    Ok(())
}

/// Keeps the tool of `hash` from the folder `from` in `store` under the lock (the module docs). `owner` is the note the lock carries.
pub fn publish(store: &Path, hash: &str, from: &Path, replace: bool, limits: Limits, owner: &str) -> Result<Published, String> {
    if !is_hash(hash) {
        return Err(format!("{hash:?} is not an input hash (40 lowercase hex digits)"));
    }
    match store.parent() {
        Some(p) if !p.as_os_str().is_empty() && p.is_dir() => {}
        _ => return Ok(Published::NoStore),
    }
    let size = size_of(from).map_err(|e| format!("cannot read the tool entry {}: {e}", from.display()))?;
    if size == 0 {
        return Err(format!("the tool entry {} is empty", from.display()));
    }
    if size > limits.entry_bytes {
        return Err(format!(
            "the tool entry {} is {size} bytes, over the entry limit of {}",
            from.display(),
            limits.entry_bytes
        ));
    }
    fs::create_dir_all(store).map_err(|e| format!("cannot make the store {}: {e}", store.display()))?;
    let _lock = WriterLock::take(store, limits.lock_wait, owner)?;
    let entry = store.join(hash);
    if entry.is_dir() && !replace {
        touch(&entry);
        return Ok(Published::Present);
    }
    let tmp = store.join(format!(".tmp-{hash}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    copy_entry(from, &tmp).map_err(|e| {
        let _ = fs::remove_dir_all(&tmp);
        format!("cannot copy the tool into {}: {e}", tmp.display())
    })?;
    let gone = store.join(format!(".gone-{hash}-{}", std::process::id()));
    if entry.is_dir() {
        // A reader that opened a file of the old entry keeps reading it: the folder is renamed away, deleted after the new one is in.
        fs::rename(&entry, &gone).map_err(|e| format!("cannot move the old entry {} away: {e}", entry.display()))?;
    }
    fs::rename(&tmp, &entry).map_err(|e| {
        let _ = fs::remove_dir_all(&tmp);
        format!("cannot put the entry {} in place: {e}", entry.display())
    })?;
    let _ = fs::remove_dir_all(&gone);
    Ok(Published::Written(evict(store, &entry, limits)))
}

/// Removes what the limits and the temp rule say, oldest use first, never `keep`; returns the evicted names.
fn evict(store: &Path, keep: &Path, limits: Limits) -> Vec<String> {
    let Ok(rd) = fs::read_dir(store) else { return Vec::new() };
    let mut entries: Vec<(SystemTime, String, u64)> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(m) = e.metadata() else { continue };
        if !m.is_dir() {
            continue;
        }
        let when = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if name.starts_with(".tmp-") || name.starts_with(".gone-") {
            // A temp folder of a writer that is gone (this command is the only writer, under the lock, so none is live).
            if SystemTime::now().duration_since(when).is_ok_and(|age| age >= STALE_TEMP) {
                let _ = fs::remove_dir_all(e.path());
            }
        } else if is_hash(&name) {
            entries.push((when, name, size_of(&e.path()).unwrap_or(0)));
        }
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let keep_name = keep.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (mut count, mut bytes) = (0usize, 0u64);
    let mut evicted = Vec::new();
    for (_, name, size) in entries {
        count += 1;
        bytes += size;
        if name != keep_name && (count > limits.entries || bytes > limits.bytes) && fs::remove_dir_all(store.join(&name)).is_ok() {
            count -= 1;
            bytes -= size;
            evicted.push(name);
        }
    }
    evicted
}

/// `dev ci dev-tool-store`: a CI step's contract (plain lines, `::warning::` on a problem, exit 0 always for publish).
pub fn run(a: &StoreArgs, env: &dyn Fn(&str) -> Option<String>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match &a.command {
        StoreCommand::Publish(p) => {
            let store = p
                .store
                .clone()
                .or_else(|| env("PADDOCK_DEV_TOOL_STORE").filter(|s| !s.is_empty()).map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from(DEFAULT_STORE));
            let owner = format!(
                "pid {} run {} job {} unix {}\n",
                std::process::id(),
                env("GITHUB_RUN_ID").unwrap_or_default(),
                env("GITHUB_JOB").unwrap_or_default(),
                SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs())
            );
            let limits = Limits {
                lock_wait: Duration::from_secs(p.lock_wait_secs),
                ..Limits::default()
            };
            let t0 = Instant::now();
            match publish(&store, &p.hash, &p.from, p.replace, limits, &owner) {
                Ok(Published::Written(ev)) => {
                    let _ = writeln!(
                        out,
                        "dev-tool-store: kept {} in {} ({} ms); evicted {}",
                        p.hash,
                        store.display(),
                        t0.elapsed().as_millis(),
                        ev.len()
                    );
                }
                Ok(Published::Present) => {
                    let _ = writeln!(out, "dev-tool-store: {} is in {} already", p.hash, store.display());
                }
                Ok(Published::NoStore) => {
                    let _ = writeln!(
                        out,
                        "dev-tool-store: not kept: {} has no mounted volume above it (the Actions cache stays the only copy). Not measured.",
                        store.display()
                    );
                }
                Err(e) => {
                    let _ = writeln!(
                        err,
                        "::warning::dev-tool-store: the tool is not kept in {}: {e}. Next step: the Actions cache still serves it; check the /sccache volume (space, permissions).",
                        store.display()
                    );
                }
            }
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H1: &str = "1111111111111111111111111111111111111111";
    const H2: &str = "2222222222222222222222222222222222222222";
    const H3: &str = "3333333333333333333333333333333333333333";

    fn tool(dir: &Path, bytes: &[u8]) -> PathBuf {
        let d = dir.join("home-bin");
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("paddock-dev-test"), bytes).unwrap();
        d
    }

    fn lim() -> Limits {
        Limits {
            entry_bytes: 1000,
            entries: 2,
            bytes: 10_000,
            lock_wait: Duration::from_millis(300),
        }
    }

    #[test]
    fn a_published_tool_is_in_the_store_by_its_hash_and_a_second_publish_leaves_it() {
        let t = tempfile::tempdir().unwrap();
        let store = t.path().join("sccache").join("paddock-dev-tools");
        fs::create_dir_all(store.parent().unwrap()).unwrap();
        let from = tool(t.path(), b"binary-one");
        assert_eq!(publish(&store, H1, &from, false, lim(), "o").unwrap(), Published::Written(vec![]));
        assert_eq!(fs::read(store.join(H1).join("paddock-dev-test")).unwrap(), b"binary-one");
        // The same hash again: nothing is rewritten (the producer writes once).
        let other = tool(t.path(), b"binary-two");
        assert_eq!(publish(&store, H1, &other, false, lim(), "o").unwrap(), Published::Present);
        assert_eq!(fs::read(store.join(H1).join("paddock-dev-test")).unwrap(), b"binary-one");
        // --replace is how a reader's rejection is repaired.
        assert!(matches!(
            publish(&store, H1, &other, true, lim(), "o").unwrap(),
            Published::Written(_)
        ));
        assert_eq!(fs::read(store.join(H1).join("paddock-dev-test")).unwrap(), b"binary-two");
        // No temp folder is left behind.
        let names: Vec<String> = fs::read_dir(&store)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| n == H1 || n == ".lock" || n == ".lock-owner"), "{names:?}");
    }

    #[test]
    fn no_volume_above_the_store_is_no_store_and_never_creates_one() {
        let t = tempfile::tempdir().unwrap();
        let store = t.path().join("missing-volume").join("paddock-dev-tools");
        let from = tool(t.path(), b"x");
        assert_eq!(publish(&store, H1, &from, false, lim(), "o").unwrap(), Published::NoStore);
        assert!(!t.path().join("missing-volume").exists());
    }

    #[test]
    fn a_bad_hash_an_empty_entry_and_an_oversized_entry_are_refused() {
        let t = tempfile::tempdir().unwrap();
        let store = t.path().join("tools");
        let from = tool(t.path(), b"x");
        assert!(
            publish(&store, "../escape", &from, false, lim(), "o")
                .unwrap_err()
                .contains("not an input hash")
        );
        assert!(
            publish(&store, &"A".repeat(40), &from, false, lim(), "o").is_err(),
            "upper case is not a hash"
        );
        let empty = t.path().join("empty");
        fs::create_dir(&empty).unwrap();
        assert!(publish(&store, H1, &empty, false, lim(), "o").unwrap_err().contains("empty"));
        let big = tool(t.path(), &[0u8; 2000]);
        assert!(
            publish(&store, H1, &big, false, lim(), "o")
                .unwrap_err()
                .contains("over the entry limit")
        );
        assert!(!store.join(H1).exists(), "a refused entry leaves nothing");
    }

    #[test]
    fn the_least_recently_used_entry_beyond_the_limit_is_evicted_and_the_new_one_never() {
        let t = tempfile::tempdir().unwrap();
        let store = t.path().join("tools");
        let from = tool(t.path(), b"binary");
        publish(&store, H1, &from, false, lim(), "o").unwrap();
        publish(&store, H2, &from, false, lim(), "o").unwrap();
        // H1 is the older use.
        let old = SystemTime::now() - Duration::from_secs(3600);
        File::open(store.join(H1)).unwrap().set_modified(old).unwrap();
        let Published::Written(ev) = publish(&store, H3, &from, false, lim(), "o").unwrap() else {
            panic!("written")
        };
        assert_eq!(ev, vec![H1.to_string()]);
        assert!(!store.join(H1).exists() && store.join(H2).exists() && store.join(H3).exists());
        // Positive control of the rule: with room for three, nothing goes.
        let roomy = Limits { entries: 3, ..lim() };
        let from2 = tool(t.path(), b"binary");
        publish(&store, H1, &from2, false, roomy, "o").unwrap();
        assert!(store.join(H1).exists() && store.join(H2).exists() && store.join(H3).exists());
    }

    #[test]
    fn the_byte_limit_evicts_too_but_keeps_the_new_entry_even_when_it_alone_is_over() {
        let t = tempfile::tempdir().unwrap();
        let store = t.path().join("tools");
        let from = tool(t.path(), &[1u8; 600]);
        let tight = Limits {
            bytes: 1000,
            entries: 9,
            ..lim()
        };
        publish(&store, H1, &from, false, tight, "o").unwrap();
        File::open(store.join(H1))
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(60))
            .unwrap();
        let Published::Written(ev) = publish(&store, H2, &from, false, tight, "o").unwrap() else {
            panic!("written")
        };
        assert_eq!(ev, vec![H1.to_string()]);
        assert!(store.join(H2).exists());
    }

    #[test]
    fn only_hash_named_folders_and_the_commands_own_temp_names_are_ever_removed() {
        let t = tempfile::tempdir().unwrap();
        let store = t.path().join("tools");
        let from = tool(t.path(), b"b");
        publish(&store, H1, &from, false, lim(), "o").unwrap();
        fs::create_dir(store.join("someone-elses-folder")).unwrap();
        fs::create_dir(store.join(format!(".tmp-{H2}-1"))).unwrap();
        File::open(store.join(format!(".tmp-{H2}-1")))
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(2 * 24 * 3600))
            .unwrap();
        fs::create_dir(store.join(format!(".tmp-{H3}-1"))).unwrap();
        publish(&store, H2, &from, false, lim(), "o").unwrap();
        assert!(
            store.join("someone-elses-folder").exists(),
            "a folder this command did not make stays"
        );
        assert!(!store.join(format!(".tmp-{H2}-1")).exists(), "a day-old temp folder goes");
        assert!(store.join(format!(".tmp-{H3}-1")).exists(), "a young temp folder stays");
    }

    #[test]
    fn a_second_writer_waits_for_the_lock_then_gives_up_with_a_message_and_leaves_the_store_alone() {
        let t = tempfile::tempdir().unwrap();
        let store = t.path().join("tools");
        fs::create_dir_all(&store).unwrap();
        let held = WriterLock::take(&store, Duration::from_millis(100), "holder").unwrap();
        let from = tool(t.path(), b"b");
        let e = publish(&store, H1, &from, false, lim(), "o").unwrap_err();
        assert!(e.contains("held the store lock"), "{e}");
        assert!(!store.join(H1).exists());
        assert_eq!(
            fs::read_to_string(store.join(".lock-owner")).unwrap(),
            "holder",
            "the lock names its holder"
        );
        drop(held);
        // Positive control: once released, the same publish works.
        assert!(matches!(
            publish(&store, H1, &from, false, lim(), "o").unwrap(),
            Published::Written(_)
        ));
    }

    #[test]
    fn the_command_never_fails_the_job_and_says_why_in_one_warning() {
        let t = tempfile::tempdir().unwrap();
        let a = StoreArgs {
            command: StoreCommand::Publish(PublishArgs {
                hash: "nothash".into(),
                from: t.path().to_path_buf(),
                store: Some(t.path().join("s")),
                replace: false,
                lock_wait_secs: 1,
            }),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(run(&a, &|_| None, &mut out, &mut err), 0);
        let err = String::from_utf8(err).unwrap();
        assert!(
            err.starts_with("::warning::dev-tool-store:") && err.contains("Next step:") && err.lines().count() == 1,
            "{err}"
        );
    }
}
