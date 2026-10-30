//! The large and duplicate file finder, as a job: walks a folder the user chose in parallel (bounded in files, depth and bytes
//! compared, never following links), then hashes only files that share a size, the heads first and the contents of those whose
//! heads agree, also in parallel. Every phase reports a growing count on its own scale and checks Cancel per entry and per
//! chunk; results come back in input order whatever the scheduling, so the answer does not depend on timing. It lists and
//! deletes nothing and builds its answer on the side: a cancelled scan leaves nothing behind.
//!
//! A folder on a dead network share can hold a `read_dir` or a `stat` inside the kernel for as long as the server is gone. The
//! work therefore runs on a thread of its own and the job waits for it with Cancel able to wake the wait: Cancel returns at once,
//! the thread is left to finish when the OS lets it, and its threads count on the host in the disk map's own per-host count
//! (`LiveWorkers`) until the call returns, so a host with four stuck threads gets no new search or scan.

use crate::disk::centre::{HostThreads, MAX_ABANDONED_PER_HOST};
use crate::disk::engine::{LiveGuard, LiveWorkers};
use dyno_core::extras::files::{
    duplicate_groups, largest, same_size_groups, FinderLimit, FinderMode, FinderQuery, FinderResult, LimitHit, Walked,
};
use dyno_core::time::Timestamp;
use paddock_jobs::{
    copy_hashed, par_map_ordered, par_walk, CancelReason, Decision, EntryKind, HashingWriter, JobContext, JobError, Phase, SymlinkPolicy,
    WalkOptions, Workers,
};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

/// The first bytes hashed to rule out most non-duplicates cheaply.
const HEAD: u64 = 64 * 1024;
/// How long the first look at the chosen folder may take before the search is refused (a dead share must not hold the job).
pub const STAT_LIMIT: Duration = Duration::from_secs(5);

/// What one scan may spend: the Settings `disk.finderFiles`, `finderDepth` and `finderHashGib` (their defaults when nobody changed them).
pub use dyno_core::extras::files::FinderLimits as Limits;

/// The folder as the finder will use it, or what to fix.
pub fn resolve_root(folder: &str) -> Result<PathBuf, String> {
    let p = Path::new(folder);
    if !p.is_absolute() {
        return Err("Give the full path of the folder.".into());
    }
    let root = std::fs::canonicalize(p).map_err(|e| format!("That folder cannot be opened ({e}). Choose another one."))?;
    if !root.is_dir() {
        return Err("Choose a folder, not a file.".into());
    }
    if root.parent().is_none() {
        return Err("Choose a folder, not a whole drive: a scan of everything takes too long and finds mostly system files.".into());
    }
    Ok(root)
}

/// [`resolve_root`] that gives up after `limit`: a dead share can hold the look at it for as long as the server is gone.
fn resolve_within(folder: &str, limit: Duration) -> Result<PathBuf, String> {
    let folder = folder.to_string();
    crate::disk::volumes::within(limit, move || resolve_root(&folder)).unwrap_or_else(|| {
        Err("That folder did not answer in time. Check that the drive or share is reachable, or choose another folder.".into())
    })
}

/// Which host a search reads and the threads of it that are stuck: a search Cancel abandoned while the OS still holds a call of
/// its keeps its threads counted on that host (the disk map's count, `LiveWorkers`) until the calls return, and a new search of a
/// host with [`MAX_ABANDONED_PER_HOST`] of them is refused.
pub struct Linger {
    host: String,
    live: Arc<LiveWorkers>,
    /// Stuck threads of the host when the search started.
    stuck: usize,
    /// The most that may be stuck before a search is refused.
    most: usize,
    /// How many threads this search reads with (they all count if it is abandoned: they are inside its walk).
    weight: usize,
}

impl Linger {
    /// The host's count as the disk map keeps it, for a search that reads with `threads` threads.
    pub fn of(host: HostThreads, threads: usize) -> Linger {
        Linger {
            host: host.host,
            live: host.live,
            stuck: host.abandoned,
            most: MAX_ABANDONED_PER_HOST,
            weight: threads.max(1),
        }
    }

    /// A count of its own and its own maximum (tests).
    pub fn counting(live: Arc<LiveWorkers>, stuck: usize, most: usize, weight: usize) -> Linger {
        Linger {
            host: "this computer".into(),
            live,
            stuck,
            most,
            weight,
        }
    }
}

/// Runs `work` on a thread of its own and waits for it; Cancel wakes the wait at once and abandons the thread (it ends when
/// the call it is stuck in returns, and it stops at its next cancel check). While it lingers, its `weight` threads count on the
/// host.
fn abandonable<T: Send + 'static>(
    ctx: &JobContext,
    linger: Linger,
    work: impl FnOnce(&JobContext) -> Result<T, JobError> + Send + 'static,
) -> Result<T, JobError> {
    if linger.stuck >= linger.most {
        return Err(JobError::Failed(format!(
            "{} still has {} reads that have not returned, so Dyno will not start another search there. Restarting Dyno clears it.",
            linger.host, linger.stuck
        )));
    }
    enum Wake<T> {
        Done(Result<T, JobError>),
        Cancelled,
    }
    let (tx, rx) = mpsc::channel::<Wake<T>>();
    // The guards of an abandoned search: taken when Cancel gives up on the thread, dropped when the thread ends.
    let held: Arc<Mutex<Option<Vec<LiveGuard>>>> = Arc::new(Mutex::new(Some(Vec::new())));
    let thread = {
        let (ctx, tx, held) = (ctx.clone(), tx.clone(), held.clone());
        std::thread::Builder::new().name("dyno-finder".into()).spawn(move || {
            lowered("search");
            let result = work(&ctx);
            // Over: whatever was counted for it comes off, and nothing may be counted afterwards.
            *held.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            let _ = tx.send(Wake::Done(result));
        })
    };
    if thread.is_err() {
        return Err(JobError::Failed("Dyno could not start the search.".into()));
    }
    let _wake = ctx.cancel_token().on_cancel(move || {
        let _ = tx.send(Wake::Cancelled);
    });
    match rx.recv() {
        Ok(Wake::Done(result)) => result,
        Ok(Wake::Cancelled) | Err(_) => {
            if let Some(guards) = held.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_mut() {
                guards.extend((0..linger.weight).map(|_| linger.live.enter()));
            }
            Err(JobError::Cancelled(ctx.cancel_token().reason().unwrap_or(CancelReason::User)))
        }
    }
}

/// Lowers the thread a part of the search runs on, gently and once (rule 52): the search's own thread and the workers of the
/// shared walk and map, which start their threads themselves and do not pass on the class of the thread that asked.
fn lowered(_part: &'static str) {
    dyno_platform::priority::lower_current_thread_once();
    #[cfg(all(test, target_os = "macos"))]
    tests::saw(_part, dyno_platform::priority::current_qos_class());
}

/// What the walk found.
struct Walk {
    files: Vec<Walked>,
    scanned: u32,
    unreadable: u32,
    limit: Option<FinderLimit>,
}

/// Walks `root` on `workers` threads and collects the files of at least `min_bytes` (path and size, sorted by path), counting every
/// file found as it goes. Stops at a limit and says which; stops with an error when cancelled.
fn walk(ctx: &JobContext, root: &Path, min_bytes: u64, limits: Limits, workers: Workers) -> Result<Walk, JobError> {
    ctx.phase(Phase::discovering("walk", "Listing files"));
    let seen = Arc::new(AtomicUsize::new(0));
    let too_many = Arc::new(AtomicBool::new(false));
    let too_deep = Arc::new(AtomicBool::new(false));
    let (s, many, deep, base, max_files, max_depth) = (
        seen.clone(),
        too_many.clone(),
        too_deep.clone(),
        root.to_path_buf(),
        limits.files,
        limits.depth,
    );
    let options = WalkOptions {
        workers,
        symlinks: SymlinkPolicy::Ignore,
        // Folders more than `depth` levels down are not read; the filter flags that one was left out.
        max_depth: Some(max_depth as usize),
        include_dirs: false,
        with_size: true,
        with_modified: true,
        issue_cap: 0,
        ..WalkOptions::default()
    }
    .with_filter(move |e| {
        lowered("walk");
        if s.fetch_add(1, Ordering::Relaxed) >= max_files {
            many.store(true, Ordering::Relaxed);
            return Decision::SkipSubtree;
        }
        if e.kind == EntryKind::Dir && e.path.strip_prefix(&base).map_or(0, |p| p.components().count()) > max_depth as usize {
            deep.store(true, Ordering::Relaxed);
        }
        Decision::Include
    });
    let found = par_walk(root, &options, ctx.cancel_token(), ctx)?;
    let files = found
        .entries
        .into_iter()
        .filter(|e| e.kind == EntryKind::File)
        .filter_map(|e| {
            let bytes = e.size.filter(|b| *b >= min_bytes)?;
            Some(Walked {
                path: e.path.to_string_lossy().into_owned(),
                bytes,
                modified: e.modified.map_or(Timestamp(0), timestamp),
            })
        })
        .collect();
    let limit = if too_many.load(Ordering::Relaxed) {
        Some(FinderLimit::Files)
    } else if too_deep.load(Ordering::Relaxed) {
        Some(FinderLimit::Depth)
    } else {
        None
    };
    Ok(Walk {
        files,
        scanned: u32::try_from(seen.load(Ordering::Relaxed).min(max_files)).unwrap_or(u32::MAX),
        unreadable: u32::try_from(found.issues_total).unwrap_or(u32::MAX),
        limit,
    })
}

/// A modification time as a [`Timestamp`].
fn timestamp(t: std::time::SystemTime) -> Timestamp {
    t.duration_since(std::time::UNIX_EPOCH)
        .map_or(Timestamp(0), |d| Timestamp(i64::try_from(d.as_millis()).unwrap_or(i64::MAX)))
}

/// The digest of the start of a file, or `None` when it cannot be read. Only a cancel is an error.
fn head_digest(ctx: &JobContext, path: &Path) -> Result<Option<[u8; 32]>, JobError> {
    let Ok(f) = std::fs::File::open(path) else { return Ok(None) };
    hashed(ctx, f.take(HEAD), false)
}

/// The digest of a whole file, or `None` when it cannot be read. Progress counts the bytes read.
fn full_digest(ctx: &JobContext, path: &Path) -> Result<Option<[u8; 32]>, JobError> {
    let Ok(f) = std::fs::File::open(path) else { return Ok(None) };
    hashed(ctx, f, true)
}

fn hashed<R: Read>(ctx: &JobContext, mut reader: R, count_bytes: bool) -> Result<Option<[u8; 32]>, JobError> {
    let result = if count_bytes {
        copy_hashed(&mut reader, std::io::sink(), ctx.cancel_token(), ctx)
    } else {
        copy_hashed(&mut reader, std::io::sink(), ctx.cancel_token(), &())
    };
    match result {
        Ok((digest, _)) => Ok(Some(digest)),
        Err(e) => match HashingWriter::<std::io::Sink>::cancelled(&e) {
            Some(c) => Err(JobError::Cancelled(c.0)),
            None => Ok(None),
        },
    }
}

/// A file (by its index in the walk) and the digest of its content.
type Digest = (usize, [u8; 32]);

/// Hashes the files that share a size: first their heads, then the whole of those whose heads agree, each on `workers` threads
/// with the results in input order. A file that would take the comparison past `limits.hash_bytes` is skipped (and the limit
/// is reported) while smaller ones after it still count; which are skipped is decided up front from the sizes, so it does not
/// depend on scheduling.
fn find_duplicates(
    ctx: &JobContext,
    files: &[Walked],
    min_bytes: u64,
    limits: Limits,
    workers: Workers,
) -> Result<(Vec<Digest>, Option<FinderLimit>), JobError> {
    let groups = same_size_groups(files, min_bytes);
    let candidates: Vec<usize> = groups.iter().flatten().copied().collect();
    ctx.phase(Phase::items("heads", "Comparing the start of files", candidates.len() as u64));
    let heads = par_map_ordered(candidates, workers, ctx.cancel_token(), ctx, |i| {
        lowered("heads");
        head_digest(ctx, Path::new(&files[i].path)).map(|h| (i, h))
    })?;
    let head_of: std::collections::HashMap<usize, [u8; 32]> = heads.into_iter().filter_map(|(i, h)| h.map(|h| (i, h))).collect();
    // The files whose heads agree with another file of the same size, in the order of the groups.
    let mut same_heads: Vec<usize> = Vec::new();
    for group in &groups {
        let mut by_head: std::collections::BTreeMap<[u8; 32], Vec<usize>> = Default::default();
        for i in group {
            if let Some(h) = head_of.get(i) {
                by_head.entry(*h).or_default().push(*i);
            }
        }
        same_heads.extend(by_head.into_values().filter(|g| g.len() > 1).flatten());
    }
    let mut budget = limits.hash_bytes;
    let mut cut = None;
    let to_read: Vec<usize> = same_heads
        .into_iter()
        .filter(|i| {
            let size = files[*i].bytes;
            if size <= budget {
                budget -= size;
                true
            } else {
                cut = Some(FinderLimit::HashBytes);
                false
            }
        })
        .collect();
    let total: u64 = to_read.iter().map(|i| files[*i].bytes).sum();
    ctx.phase(Phase::bytes("contents", "Comparing contents", Some(total)));
    let digests = par_map_ordered(to_read, workers, ctx.cancel_token(), &(), |i| {
        lowered("contents");
        full_digest(ctx, Path::new(&files[i].path)).map(|h| (i, h))
    })?;
    Ok((digests.into_iter().filter_map(|(i, h)| h.map(|h| (i, h))).collect(), cut))
}

/// Runs the finder as the job `ctx` with `workers` threads reading the folder. `scan_id` names the result. A bad folder is a failed
/// job with what to fix; a folder that does not answer within [`STAT_LIMIT`] is refused with a sentence.
pub fn run(
    ctx: &JobContext,
    query: &FinderQuery,
    scan_id: &str,
    limits: Limits,
    workers: Workers,
    linger: Linger,
) -> Result<FinderResult, JobError> {
    let query = query.clone().validated().map_err(JobError::Failed)?;
    let root = resolve_within(&query.folder, STAT_LIMIT).map_err(JobError::Failed)?;
    let scan_id = scan_id.to_string();
    abandonable(ctx, linger, move |ctx| {
        let walked = walk(ctx, &root, query.min_bytes, limits, workers)?;
        let mut limit = walked.limit;
        let files = walked.files;
        let large = if query.mode == FinderMode::Duplicates {
            Vec::new()
        } else {
            largest(&files, query.min_bytes, &scan_id)
        };
        let duplicates = if query.mode == FinderMode::Large {
            Vec::new()
        } else {
            let (hashed, cut) = find_duplicates(ctx, &files, query.min_bytes, limits, workers)?;
            limit = limit.or(cut);
            duplicate_groups(&files, &hashed, &scan_id)
        };
        let wasted_bytes = duplicates.iter().fold(0u64, |a, g| a.saturating_add(g.wasted_bytes));
        Ok(FinderResult {
            scan_id,
            folder: root.to_string_lossy().into_owned(),
            large,
            duplicates,
            scanned: walked.scanned,
            unreadable: walked.unreadable,
            limit: limit.map(|l| LimitHit::new(l, &limits)),
            wasted_bytes,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::testing::Run;
    use paddock_jobs::JobState;

    const MB: usize = 1024 * 1024;

    fn lg() -> Linger {
        Linger::counting(Arc::new(LiveWorkers::default()), 0, 4, 4)
    }

    /// Four threads, as on an SSD.
    fn w(n: usize) -> Workers {
        Workers::new(n)
    }

    fn write(dir: &Path, name: &str, byte: u8, len: usize) -> PathBuf {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, vec![byte; len]).unwrap();
        p
    }

    fn query(folder: &Path, mode: FinderMode) -> FinderQuery {
        FinderQuery {
            folder: folder.to_string_lossy().into_owned(),
            mode,
            min_bytes: 0,
        }
    }

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap().join("stuff");
        write(&root, "big.bin", 1, 5 * MB);
        write(&root, "copy1/a.iso", 7, 2 * MB);
        write(&root, "copy2/a-again.iso", 7, 2 * MB);
        // Same size, different content: not a duplicate.
        write(&root, "other/b.iso", 8, 2 * MB);
        write(&root, "tiny.txt", 1, 10);
        (t, root)
    }

    fn find(root: &Path, mode: FinderMode) -> Result<FinderResult, JobError> {
        Run::new().job(|ctx| run(ctx, &query(root, mode), "s", Limits::default(), w(4), lg()))
    }

    /// The QoS class each part of a search ran at, per thread (every finder test adds to it).
    #[cfg(target_os = "macos")]
    static SEEN: Mutex<Vec<(&'static str, u32)>> = Mutex::new(Vec::new());

    #[cfg(target_os = "macos")]
    pub(super) fn saw(part: &'static str, class: u32) {
        SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((part, class));
    }

    /// The person started the search and watches it, so every thread of it (its own, the walk's and the comparison's workers)
    /// is lowered gently, never starved (rule 52): `UTILITY` on macOS.
    #[cfg(target_os = "macos")]
    #[test]
    fn every_thread_of_a_search_runs_at_the_gentle_level() {
        let (_t, root) = fixture();
        assert_eq!(find(&root, FinderMode::Both).unwrap().duplicates.len(), 1);
        let seen = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        for part in ["search", "walk", "heads", "contents"] {
            assert!(seen.iter().any(|(p, _)| *p == part), "{part} never ran: {seen:?}");
        }
        assert!(seen.iter().all(|(_, c)| *c == dyno_platform::priority::UTILITY), "{seen:?}");
    }

    #[test]
    fn finds_the_biggest_files_and_only_real_duplicates() {
        let (_t, root) = fixture();
        let r = find(&root, FinderMode::Both).unwrap();
        assert_eq!(r.large[0].path, root.join("big.bin").to_string_lossy());
        assert_eq!(r.large.len(), 4, "the tiny file is below the 1 MiB floor");
        assert_eq!(r.duplicates.len(), 1, "{:#?}", r.duplicates);
        assert_eq!(r.duplicates[0].files.len(), 2);
        assert_eq!(r.wasted_bytes, 2 * MB as u64);
        assert_eq!(r.limit, None);
        let only_large = find(&root, FinderMode::Large).unwrap();
        assert!(only_large.duplicates.is_empty() && !only_large.large.is_empty());
        let only_dups = find(&root, FinderMode::Duplicates).unwrap();
        assert!(only_dups.large.is_empty() && only_dups.duplicates.len() == 1);
    }

    #[test]
    fn bad_folders_say_what_to_fix() {
        assert!(resolve_root("relative/path").unwrap_err().contains("full path"));
        let (_t, root) = fixture();
        // Absolute on every platform (a bare "/x" has no drive on Windows), and not there.
        assert!(resolve_root(&root.join("not").join("here").to_string_lossy())
            .unwrap_err()
            .contains("cannot be opened"));
        assert!(resolve_root(&root.join("big.bin").to_string_lossy())
            .unwrap_err()
            .contains("not a file"));
        let top = if cfg!(windows) { "C:\\" } else { "/" };
        assert!(resolve_root(top).unwrap_err().contains("whole drive"));
        let blank = FinderQuery {
            folder: " ".into(),
            mode: FinderMode::Both,
            min_bytes: 0,
        };
        let out = Run::new().job(|ctx| run(ctx, &blank, "s", Limits::default(), w(4), lg()));
        assert!(matches!(out, Err(JobError::Failed(m)) if m.contains("Choose a folder")));
    }

    #[cfg(unix)]
    #[test]
    fn links_are_never_followed() {
        let (_t, root) = fixture();
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
        std::os::unix::fs::symlink(root.join("big.bin"), root.join("link.bin")).unwrap();
        let r = find(&root, FinderMode::Both).unwrap();
        assert!(r.large.iter().all(|f| !f.path.contains("loop") && !f.path.ends_with("link.bin")));
    }

    #[test]
    fn each_phase_counts_up_from_the_first_report_on_its_own_scale() {
        let (_t, root) = fixture();
        let job = Run::new();
        job.job(|ctx| run(ctx, &query(&root, FinderMode::Both), "s", Limits::default(), w(4), lg()))
            .unwrap();
        let phases = job.phases();
        assert_eq!(
            phases.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["walk", "heads", "contents"]
        );
        // Queued, started, then the walk at once: a count with no total (a growing count, never a static 0).
        assert!(
            matches!(job.snapshots()[2].state, JobState::Running { phase: Some(ref p) } if p.phase.id == "walk" && p.phase.total.is_none())
        );
        for p in &phases {
            assert!(p.counts.windows(2).all(|w| w[0] <= w[1]), "{} went backwards: {:?}", p.id, p.counts);
        }
        // The walk counts every file it finds (5); the later phases know their totals: the 3 that share a size, and the 4 MiB of
        // the two whose starts agree.
        assert_eq!(phases[0].counts.last(), Some(&5));
        assert_eq!((phases[1].total, phases[1].counts.last().copied()), (Some(3), Some(3)));
        assert_eq!(
            (phases[2].total, phases[2].counts.last().copied()),
            (Some(4 * MB as u64), Some(4 * MB as u64))
        );
        assert_eq!(job.end_state(), JobState::Done);
    }

    #[test]
    fn the_walk_stops_at_each_limit_and_names_it() {
        let (_t, root) = fixture();
        let q = query(&root, FinderMode::Both);
        let few = Run::new()
            .job(|ctx| {
                run(
                    ctx,
                    &q,
                    "s",
                    Limits {
                        files: 2,
                        ..Limits::default()
                    },
                    w(4),
                    lg(),
                )
            })
            .unwrap();
        assert_eq!((few.limit.map(|l| l.limit), few.scanned), (Some(FinderLimit::Files), 2));
        // Depth 0: the chosen folder's own files are read, the folders below it are left out.
        let shallow = Run::new()
            .job(|ctx| {
                run(
                    ctx,
                    &q,
                    "s",
                    Limits {
                        depth: 0,
                        ..Limits::default()
                    },
                    w(4),
                    lg(),
                )
            })
            .unwrap();
        assert_eq!(shallow.limit.map(|l| l.limit), Some(FinderLimit::Depth));
        assert_eq!(shallow.large.len(), 1);
        // Comparing contents may read 1 MiB at most: the 4 MiB of candidates are over it.
        let small = Run::new()
            .job(|ctx| {
                run(
                    ctx,
                    &q,
                    "s",
                    Limits {
                        hash_bytes: MB as u64,
                        ..Limits::default()
                    },
                    w(4),
                    lg(),
                )
            })
            .unwrap();
        assert_eq!(small.limit.as_ref().map(|l| l.limit), Some(FinderLimit::HashBytes));
        assert!(small.limit.unwrap().message.contains("Some duplicates may be missing"));
        assert!(small.duplicates.is_empty());
        for l in [FinderLimit::Files, FinderLimit::Depth, FinderLimit::HashBytes] {
            let m = l.message(&Limits {
                files: 7,
                depth: 3,
                hash_bytes: 5 * 1024 * 1024 * 1024,
            });
            assert!(m.to_lowercase().contains("choose") && m.contains("Settings"), "{m}");
            assert!(
                m.contains(
                    &["7 files", "3 levels", "5 GiB"]
                        .iter()
                        .find(|x| m.contains(*x))
                        .copied()
                        .unwrap_or("?")
                        .to_string()
                ),
                "the numbers are the scan's own: {m}"
            );
        }
    }

    #[test]
    fn cancel_stops_a_parallel_walk_long_before_it_would_have_finished() {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap().join("many");
        const PER_DIR: usize = 2_000;
        for d in 0..8 {
            std::fs::create_dir_all(root.join(format!("d{d}"))).unwrap();
            for i in 0..PER_DIR {
                std::fs::write(root.join(format!("d{d}")).join(format!("f{i}")), b"x").unwrap();
            }
        }
        let job = Run::new().cancel_when("walk", 100);
        let out = job.job(|ctx| run(ctx, &query(&root, FinderMode::Both), "s", Limits::default(), w(4), lg()));
        assert!(matches!(out, Err(JobError::Cancelled(_))));
        let seen = job.phases()[0].counts.last().copied().unwrap();
        assert!(
            seen < (8 * PER_DIR / 2) as u64,
            "looked at {seen} of {} entries after the cancel",
            8 * PER_DIR
        );
        assert!(matches!(job.end_state(), JobState::Cancelled { .. }));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 8, "nothing was changed");
    }

    #[test]
    fn the_answer_does_not_depend_on_how_many_threads_read_the_folder() {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap().join("tree");
        for d in 0..6 {
            write(&root, &format!("d{d}/same-a.bin"), 3, 2 * MB);
            write(&root, &format!("d{d}/own-{d}.bin"), d as u8 + 10, MB + d * 1000);
            write(&root, &format!("d{d}/sub/same-b.bin"), 4, MB + 5);
        }
        let by = |n| {
            Run::new()
                .job(|ctx| run(ctx, &query(&root, FinderMode::Both), "s", Limits::default(), w(n), lg()))
                .unwrap()
        };
        let (one, many) = (by(1), by(8));
        assert_eq!(one, many, "same files, same order, same ids");
        assert_eq!(one.duplicates.len(), 2);
        assert!(one.duplicates.iter().all(|g| g.files.len() == 6));
    }

    #[test]
    fn a_file_over_the_comparison_budget_is_skipped_and_the_rest_are_still_compared() {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap().join("budget");
        // The biggest pair is first in the order of the groups and does not fit; the smaller pair after it does.
        write(&root, "big1.bin", 1, 3 * MB);
        write(&root, "big2.bin", 1, 3 * MB);
        write(&root, "small1.bin", 2, MB + 1);
        write(&root, "small2.bin", 2, MB + 1);
        let r = Run::new()
            .job(|ctx| {
                run(
                    ctx,
                    &query(&root, FinderMode::Duplicates),
                    "s",
                    Limits {
                        hash_bytes: 5 * MB as u64 / 2,
                        ..Limits::default()
                    },
                    w(4),
                    lg(),
                )
            })
            .unwrap();
        assert_eq!(r.limit.as_ref().map(|l| l.limit), Some(FinderLimit::HashBytes));
        assert_eq!(r.duplicates.len(), 1, "{:#?}", r.duplicates);
        assert_eq!(r.duplicates[0].bytes_each, MB as u64 + 1);
    }

    #[cfg(unix)]
    #[test]
    fn folders_that_could_not_be_read_are_counted_and_the_rest_is_still_listed() {
        use std::os::unix::fs::PermissionsExt;
        let (_t, root) = fixture();
        let locked = root.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let unreadable_here = std::fs::read_dir(&locked).is_err();
        let r = find(&root, FinderMode::Both).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Running as root can read anything; only assert the count when the folder really is closed.
        if unreadable_here {
            assert_eq!(r.unreadable, 1);
        }
        assert_eq!(r.large.len(), 4, "the rest is listed");
        assert_eq!(find(&root, FinderMode::Both).unwrap().unreadable, 0);
    }

    #[test]
    fn cancel_returns_at_once_while_a_call_is_stuck_and_its_threads_count_on_the_host_until_they_return() {
        let live = Arc::new(LiveWorkers::default());
        let door = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let entered = Arc::new(AtomicBool::new(false));
        let (d, e, l) = (door.clone(), entered.clone(), live.clone());
        let started = std::time::Instant::now();
        let out = Run::new().job(|ctx| {
            let token = ctx.cancel_token().clone();
            let watcher = e.clone();
            std::thread::spawn(move || {
                while !watcher.load(Ordering::SeqCst) {
                    std::thread::yield_now();
                }
                token.cancel();
            });
            abandonable(ctx, Linger::counting(l, 0, 4, 3), move |_ctx| {
                e.store(true, Ordering::SeqCst);
                // The OS call that does not come back until the share does.
                let mut open = d.0.lock().unwrap();
                while !*open {
                    open = d.1.wait(open).unwrap();
                }
                Ok(())
            })
        });
        assert!(matches!(out, Err(JobError::Cancelled(_))), "{out:?}");
        assert!(started.elapsed() < Duration::from_secs(5), "Cancel waited for the stuck call");
        assert_eq!(
            live.count(),
            3,
            "the search's walker threads are counted while they are inside the call"
        );
        *door.0.lock().unwrap() = true;
        door.1.notify_all();
        for _ in 0..500 {
            if live.count() == 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the threads never left the count");
    }

    #[test]
    fn a_search_that_finishes_normally_never_counts_and_one_that_ends_just_after_cancel_does_not_leak_a_count() {
        let live = Arc::new(LiveWorkers::default());
        let out = Run::new().job(|ctx| abandonable(ctx, Linger::counting(live.clone(), 0, 4, 4), |_ctx| Ok(1)));
        assert_eq!(out.unwrap(), 1);
        assert_eq!(live.count(), 0);
    }

    #[test]
    fn a_search_of_a_host_with_too_many_stuck_threads_is_refused_with_the_way_out() {
        let live = Arc::new(LiveWorkers::default());
        let out = Run::new().job(|ctx| abandonable(ctx, Linger::counting(live.clone(), 4, 4, 4), |_ctx| Ok(1)));
        let Err(JobError::Failed(m)) = out else { panic!("{out:?}") };
        assert!(
            m.contains("this computer still has 4 reads") && m.contains("Restarting Dyno clears it."),
            "{m}"
        );
        assert_eq!(
            Run::new()
                .job(|ctx| abandonable(ctx, Linger::counting(live.clone(), 3, 4, 4), |_ctx| Ok(1)))
                .unwrap(),
            1
        );
    }

    #[test]
    fn cancel_stops_comparing_within_a_chunk_and_leaves_the_files_alone() {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap().join("dups");
        let paths: Vec<_> = (0..3).map(|i| write(&root, &format!("same{i}.bin"), 9, 6 * MB)).collect();
        let job = Run::new().cancel_when("contents", 64 * 1024);
        let out = job.job(|ctx| run(ctx, &query(&root, FinderMode::Duplicates), "s", Limits::default(), w(4), lg()));
        assert!(matches!(out, Err(JobError::Cancelled(_))), "{out:?}");
        let read = job.phases().last().unwrap().counts.last().copied().unwrap();
        // Each thread reads at most one more chunk before it sees the cancel.
        assert!(
            read <= 64 * 1024 + 4 * paddock_jobs::CHUNK as u64,
            "read {read} bytes after the cancel"
        );
        assert!(paths.iter().all(|p| p.exists()));
        assert!(matches!(job.end_state(), JobState::Cancelled { .. }));
    }
}
