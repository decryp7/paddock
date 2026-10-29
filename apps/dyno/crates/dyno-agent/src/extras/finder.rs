//! The large and duplicate file finder, as a job: walks a folder the user chose in parallel (bounded in files, depth and bytes
//! compared, never following links), then hashes only files that share a size, the heads first and the contents of those whose
//! heads agree, also in parallel. Every phase reports a growing count on its own scale and checks Cancel per entry and per
//! chunk; results come back in input order whatever the scheduling, so the answer does not depend on timing. It lists and
//! deletes nothing and builds its answer on the side: a cancelled scan leaves nothing behind.
//!
//! A folder on a dead network share can hold a `read_dir` or a `stat` inside the kernel for as long as the server is gone. The
//! work therefore runs on a thread of its own and the job waits for it with Cancel able to wake the wait: Cancel returns at once,
//! the thread is left to finish when the OS lets it, and a few such abandoned threads at a time are the most that may pile up
//! (the disk map's approach, `volumes::within`).

use crate::disk::walk::modified;
use dyno_core::extras::files::{
    duplicate_groups, largest, same_size_groups, FinderLimit, FinderMode, FinderQuery, FinderResult, LimitHit, Walked, MAX_DEPTH,
    MAX_HASH_BYTES, MAX_SCANNED,
};
use paddock_jobs::{
    copy_hashed, par_map_ordered, par_walk, CancelReason, Decision, EntryKind, HashingWriter, JobContext, JobError, Phase, SymlinkPolicy,
    WalkOptions, Workers,
};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// The first bytes hashed to rule out most non-duplicates cheaply.
const HEAD: u64 = 64 * 1024;
/// How long the first look at the chosen folder may take before the search is refused (a dead share must not hold the job).
pub const STAT_LIMIT: Duration = Duration::from_secs(5);
/// Scans that were cancelled while the OS still held a call of theirs; a new search is refused while this many linger.
pub const MAX_ABANDONED: usize = 4;

static ABANDONED: AtomicUsize = AtomicUsize::new(0);

/// What one scan may spend. The defaults are the product limits; tests pass small ones.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Most files and folders looked at.
    pub files: usize,
    /// Deepest folder level entered.
    pub depth: u32,
    /// Most bytes read comparing contents.
    pub hash_bytes: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            files: MAX_SCANNED,
            depth: MAX_DEPTH,
            hash_bytes: MAX_HASH_BYTES,
        }
    }
}

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

/// Runs `work` on a thread of its own and waits for it; Cancel wakes the wait at once and abandons the thread (it ends when
/// the call it is stuck in returns, and it stops at its next cancel check). At most [`MAX_ABANDONED`] may linger.
fn abandonable<T: Send + 'static>(
    ctx: &JobContext,
    work: impl FnOnce(&JobContext) -> Result<T, JobError> + Send + 'static,
) -> Result<T, JobError> {
    abandonable_counting(ctx, &ABANDONED, MAX_ABANDONED, work)
}

/// [`abandonable`] over its own count of abandoned threads and its own maximum (tests give theirs).
fn abandonable_counting<T: Send + 'static>(
    ctx: &JobContext,
    lingering: &'static AtomicUsize,
    most: usize,
    work: impl FnOnce(&JobContext) -> Result<T, JobError> + Send + 'static,
) -> Result<T, JobError> {
    if lingering.load(Ordering::SeqCst) >= most {
        return Err(JobError::Failed(
            "Earlier searches are still waiting on a folder that does not answer. Wait for them, or check that the drive or share is reachable.".into(),
        ));
    }
    enum Wake<T> {
        Done(Result<T, JobError>),
        Cancelled,
    }
    let (tx, rx) = mpsc::channel::<Wake<T>>();
    let finished = Arc::new(AtomicBool::new(false));
    let abandoned = Arc::new(AtomicBool::new(false));
    let thread = {
        let (ctx, tx, finished, abandoned) = (ctx.clone(), tx.clone(), finished.clone(), abandoned.clone());
        std::thread::Builder::new().name("dyno-finder".into()).spawn(move || {
            let result = work(&ctx);
            finished.store(true, Ordering::SeqCst);
            if abandoned.swap(false, Ordering::SeqCst) {
                lingering.fetch_sub(1, Ordering::SeqCst);
            }
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
            // Counted as lingering unless the worker has just ended (it takes itself off the count when it ends after this).
            abandoned.store(true, Ordering::SeqCst);
            lingering.fetch_add(1, Ordering::SeqCst);
            if finished.load(Ordering::SeqCst) && abandoned.swap(false, Ordering::SeqCst) {
                lingering.fetch_sub(1, Ordering::SeqCst);
            }
            Err(JobError::Cancelled(ctx.cancel_token().reason().unwrap_or(CancelReason::User)))
        }
    }
}

/// What the walk found.
struct Walk {
    files: Vec<(PathBuf, u64)>,
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
        issue_cap: 0,
        ..WalkOptions::default()
    }
    .with_filter(move |e| {
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
        .filter_map(|e| e.size.filter(|b| *b >= min_bytes).map(|b| (e.path, b)))
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
        full_digest(ctx, Path::new(&files[i].path)).map(|h| (i, h))
    })?;
    Ok((digests.into_iter().filter_map(|(i, h)| h.map(|h| (i, h))).collect(), cut))
}

/// The modification time of the files that survived the size filter, on `workers` threads, in input order.
fn with_times(ctx: &JobContext, found: Vec<(PathBuf, u64)>, workers: Workers) -> Result<Vec<Walked>, JobError> {
    ctx.phase(Phase::items("details", "Reading file details", found.len() as u64));
    let read = par_map_ordered(found, workers, ctx.cancel_token(), ctx, |(path, bytes)| {
        Ok(std::fs::symlink_metadata(&path).ok().map(|m| Walked {
            path: path.to_string_lossy().into_owned(),
            bytes,
            modified: modified(&m),
        }))
    })?;
    Ok(read.into_iter().flatten().collect())
}

/// Runs the finder as the job `ctx` with `workers` threads reading the folder. `scan_id` names the result. A bad folder is a failed
/// job with what to fix; a folder that does not answer within [`STAT_LIMIT`] is refused with a sentence.
pub fn run(ctx: &JobContext, query: &FinderQuery, scan_id: &str, limits: Limits, workers: Workers) -> Result<FinderResult, JobError> {
    let query = query.clone().validated().map_err(JobError::Failed)?;
    let root = resolve_within(&query.folder, STAT_LIMIT).map_err(JobError::Failed)?;
    let scan_id = scan_id.to_string();
    abandonable(ctx, move |ctx| {
        let walked = walk(ctx, &root, query.min_bytes, limits, workers)?;
        let mut limit = walked.limit;
        let files = with_times(ctx, walked.files, workers)?;
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
            limit: limit.map(LimitHit::from),
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
        Run::new().job(|ctx| run(ctx, &query(root, mode), "s", Limits::default(), w(4)))
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
        let out = Run::new().job(|ctx| run(ctx, &blank, "s", Limits::default(), w(4)));
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
        job.job(|ctx| run(ctx, &query(&root, FinderMode::Both), "s", Limits::default(), w(4)))
            .unwrap();
        let phases = job.phases();
        assert_eq!(
            phases.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["walk", "details", "heads", "contents"]
        );
        // Queued, started, then the walk at once: a count with no total (a growing count, never a static 0).
        assert!(
            matches!(job.snapshots()[2].state, JobState::Running { phase: Some(ref p) } if p.phase.id == "walk" && p.phase.total.is_none())
        );
        for p in &phases {
            assert!(p.counts.windows(2).all(|w| w[0] <= w[1]), "{} went backwards: {:?}", p.id, p.counts);
        }
        // The walk counts every file it finds (5); the later phases know their totals: the 4 files over the floor, the 3 that
        // share a size, and the 4 MiB of the two whose starts agree.
        assert_eq!(phases[0].counts.last(), Some(&5));
        assert_eq!((phases[1].total, phases[1].counts.last().copied()), (Some(4), Some(4)));
        assert_eq!((phases[2].total, phases[2].counts.last().copied()), (Some(3), Some(3)));
        assert_eq!(
            (phases[3].total, phases[3].counts.last().copied()),
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
                )
            })
            .unwrap();
        assert_eq!(small.limit.as_ref().map(|l| l.limit), Some(FinderLimit::HashBytes));
        assert!(small.limit.unwrap().message.contains("Some duplicates may be missing"));
        assert!(small.duplicates.is_empty());
        for l in [FinderLimit::Files, FinderLimit::Depth, FinderLimit::HashBytes] {
            assert!(l.message().to_lowercase().contains("choose"), "{}", l.message());
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
        let out = job.job(|ctx| run(ctx, &query(&root, FinderMode::Both), "s", Limits::default(), w(4)));
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
                .job(|ctx| run(ctx, &query(&root, FinderMode::Both), "s", Limits::default(), w(n)))
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

    fn lingering() -> &'static AtomicUsize {
        Box::leak(Box::new(AtomicUsize::new(0)))
    }

    #[test]
    fn cancel_returns_at_once_while_a_call_is_stuck_and_the_thread_is_given_up_on_until_it_ends() {
        let stuck = lingering();
        let door = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let entered = Arc::new(AtomicBool::new(false));
        let (d, e) = (door.clone(), entered.clone());
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
            abandonable_counting(ctx, stuck, 4, move |_ctx| {
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
        assert_eq!(stuck.load(Ordering::SeqCst), 1, "the thread is counted while it lingers");
        *door.0.lock().unwrap() = true;
        door.1.notify_all();
        for _ in 0..500 {
            if stuck.load(Ordering::SeqCst) == 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the thread never left the count");
    }

    #[test]
    fn a_search_is_refused_while_too_many_cancelled_ones_still_wait_on_a_dead_folder() {
        let stuck = lingering();
        stuck.store(4, Ordering::SeqCst);
        let out = Run::new().job(|ctx| abandonable_counting(ctx, stuck, 4, |_ctx| Ok(1)));
        assert!(matches!(out, Err(JobError::Failed(m)) if m.contains("still waiting")));
        stuck.store(3, Ordering::SeqCst);
        assert_eq!(Run::new().job(|ctx| abandonable_counting(ctx, stuck, 4, |_ctx| Ok(1))).unwrap(), 1);
    }

    #[test]
    fn cancel_stops_comparing_within_a_chunk_and_leaves_the_files_alone() {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap().join("dups");
        let paths: Vec<_> = (0..3).map(|i| write(&root, &format!("same{i}.bin"), 9, 6 * MB)).collect();
        let job = Run::new().cancel_when("contents", 64 * 1024);
        let out = job.job(|ctx| run(ctx, &query(&root, FinderMode::Duplicates), "s", Limits::default(), w(4)));
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
