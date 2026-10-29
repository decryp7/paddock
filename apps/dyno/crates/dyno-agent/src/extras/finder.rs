//! The large and duplicate file finder, as a job: walks a folder the user chose (bounded in files, depth and bytes
//! compared, never following links) and hashes only files that share a size. It reports a growing count while it walks,
//! a count of candidates while it compares starts, and bytes while it compares contents; Cancel is checked per entry and
//! per chunk. It lists and deletes nothing, and builds its answer on the side: a cancelled scan leaves nothing behind.

use crate::disk::walk::modified;
use dyno_core::extras::files::{
    duplicate_groups, largest, same_size_groups, FinderLimit, FinderMode, FinderQuery, FinderResult, LimitHit, Walked, MAX_DEPTH,
    MAX_HASH_BYTES, MAX_SCANNED,
};
use paddock_jobs::{copy_hashed, HashingWriter, JobContext, JobError, Phase};
use std::io::Read;
use std::path::{Path, PathBuf};

/// The first bytes hashed to rule out most non-duplicates cheaply.
const HEAD: u64 = 64 * 1024;

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

/// A file (by its index in the walk) and the digest of its content.
type Digest = (usize, [u8; 32]);

/// What the walk found.
struct Walk {
    files: Vec<Walked>,
    scanned: u32,
    limit: Option<FinderLimit>,
}

/// Walks `root` and collects files of at least `min_bytes`, counting every entry looked at. Stops at a limit and says
/// which; stops with an error when cancelled.
fn walk(ctx: &JobContext, root: &Path, min_bytes: u64, limits: Limits) -> Result<Walk, JobError> {
    ctx.phase(Phase::discovering("walk", "Listing files"));
    let mut out = Walk {
        files: Vec::new(),
        scanned: 0,
        limit: None,
    };
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            ctx.check()?;
            if (out.scanned as usize) >= limits.files {
                out.limit = Some(FinderLimit::Files);
                return Ok(out);
            }
            out.scanned += 1;
            ctx.add(1);
            let Ok(meta) = entry.path().symlink_metadata() else { continue };
            let ft = meta.file_type();
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if depth >= limits.depth {
                    out.limit.get_or_insert(FinderLimit::Depth);
                } else {
                    stack.push((entry.path(), depth + 1));
                }
            } else if ft.is_file() && meta.len() >= min_bytes {
                out.files.push(Walked {
                    path: entry.path().to_string_lossy().into_owned(),
                    bytes: meta.len(),
                    modified: modified(&meta),
                });
            }
        }
    }
    Ok(out)
}

/// The digest of the start of a file, or `None` when it cannot be read. Only a cancel is an error.
fn head_digest(ctx: &JobContext, path: &str) -> Result<Option<[u8; 32]>, JobError> {
    let Ok(f) = std::fs::File::open(path) else { return Ok(None) };
    hashed(ctx, f.take(HEAD), false)
}

/// The digest of a whole file, or `None` when it cannot be read. Progress counts the bytes read.
fn full_digest(ctx: &JobContext, path: &str) -> Result<Option<[u8; 32]>, JobError> {
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

/// Hashes the files that share a size: first their heads, then the whole of those whose heads agree. Returns the
/// digests and the limit that cut the comparison short, if one did.
fn find_duplicates(
    ctx: &JobContext,
    files: &[Walked],
    min_bytes: u64,
    limits: Limits,
) -> Result<(Vec<Digest>, Option<FinderLimit>), JobError> {
    let groups = same_size_groups(files, min_bytes);
    let candidates: u64 = groups.iter().map(|g| g.len() as u64).sum();
    ctx.phase(Phase::items("heads", "Comparing the start of files", candidates));
    let mut same_heads: Vec<Vec<usize>> = Vec::new();
    for group in groups {
        let mut by_head: std::collections::BTreeMap<[u8; 32], Vec<usize>> = Default::default();
        for i in group {
            ctx.check()?;
            if let Some(h) = head_digest(ctx, &files[i].path)? {
                by_head.entry(h).or_default().push(i);
            }
            ctx.add(1);
        }
        same_heads.extend(by_head.into_values().filter(|g| g.len() > 1));
    }
    let to_read: u64 = same_heads.iter().flatten().map(|i| files[*i].bytes).sum();
    ctx.phase(Phase::bytes("contents", "Comparing contents", Some(to_read)));
    let mut hashed = Vec::new();
    let mut read = 0u64;
    for group in same_heads {
        for i in group {
            ctx.check()?;
            if read.saturating_add(files[i].bytes) > limits.hash_bytes {
                return Ok((hashed, Some(FinderLimit::HashBytes)));
            }
            read += files[i].bytes;
            if let Some(h) = full_digest(ctx, &files[i].path)? {
                hashed.push((i, h));
            }
        }
    }
    Ok((hashed, None))
}

/// Runs the finder as the job `ctx`. `scan_id` names the result. A bad folder is a failed job with what to fix.
pub fn run(ctx: &JobContext, query: &FinderQuery, scan_id: &str, limits: Limits) -> Result<FinderResult, JobError> {
    let query = query.clone().validated().map_err(JobError::Failed)?;
    let root = resolve_root(&query.folder).map_err(JobError::Failed)?;
    let walked = walk(ctx, &root, query.min_bytes, limits)?;
    let mut limit = walked.limit;
    let large = if query.mode == FinderMode::Duplicates {
        Vec::new()
    } else {
        largest(&walked.files, query.min_bytes, scan_id)
    };
    let duplicates = if query.mode == FinderMode::Large {
        Vec::new()
    } else {
        let (hashed, cut) = find_duplicates(ctx, &walked.files, query.min_bytes, limits)?;
        limit = limit.or(cut);
        duplicate_groups(&walked.files, &hashed, scan_id)
    };
    let wasted_bytes = duplicates.iter().fold(0u64, |a, g| a.saturating_add(g.wasted_bytes));
    Ok(FinderResult {
        scan_id: scan_id.into(),
        folder: root.to_string_lossy().into_owned(),
        large,
        duplicates,
        scanned: walked.scanned,
        limit: limit.map(LimitHit::from),
        wasted_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::testing::Run;
    use paddock_jobs::JobState;

    const MB: usize = 1024 * 1024;

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
        Run::new().job(|ctx| run(ctx, &query(root, mode), "s", Limits::default()))
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
        let out = Run::new().job(|ctx| run(ctx, &blank, "s", Limits::default()));
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
        job.job(|ctx| run(ctx, &query(&root, FinderMode::Both), "s", Limits::default()))
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
        // The walk counts every entry looked at (5 files, 3 folders); the later phases know their totals.
        assert_eq!(phases[0].counts.last(), Some(&8));
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
    fn cancel_stops_the_walk_within_an_entry() {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap().join("many");
        for i in 0..400 {
            write(&root, &format!("d{}/f{i}", i % 8), 1, 1);
        }
        let job = Run::new().cancel_when("walk", 50);
        let out = job.job(|ctx| run(ctx, &query(&root, FinderMode::Both), "s", Limits::default()));
        assert!(matches!(out, Err(JobError::Cancelled(_))));
        let seen = job.phases()[0].counts.last().copied().unwrap();
        assert!((50..=51).contains(&seen), "looked at {seen} entries after the cancel");
        assert!(matches!(job.end_state(), JobState::Cancelled { .. }));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 8, "nothing was changed");
    }

    #[test]
    fn cancel_stops_comparing_within_a_chunk_and_leaves_the_files_alone() {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap().join("dups");
        let paths: Vec<_> = (0..3).map(|i| write(&root, &format!("same{i}.bin"), 9, 6 * MB)).collect();
        let job = Run::new().cancel_when("contents", 64 * 1024);
        let out = job.job(|ctx| run(ctx, &query(&root, FinderMode::Duplicates), "s", Limits::default()));
        assert!(matches!(out, Err(JobError::Cancelled(_))), "{out:?}");
        let read = job.phases().last().unwrap().counts.last().copied().unwrap();
        assert!(read <= 64 * 1024 + paddock_jobs::CHUNK as u64, "read {read} bytes after the cancel");
        assert!(paths.iter().all(|p| p.exists()));
        assert!(matches!(job.end_state(), JobState::Cancelled { .. }));
    }
}
