//! Sizes and the bounded worker pool: a folder's size on disk, measured with a time limit, and the folders measured in
//! parallel (ER-20: a few threads for an SSD, never one thread per folder).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// A measured size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// Bytes on disk (allocated blocks on Unix, file lengths elsewhere; a hard-linked file counts once).
    pub bytes: u64,
    /// The time limit ended the walk: the real size is larger.
    pub partial: bool,
}

#[cfg(unix)]
fn disk_bytes(m: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.blocks() * 512
}

#[cfg(not(unix))]
fn disk_bytes(m: &std::fs::Metadata) -> u64 {
    m.len()
}

#[cfg(unix)]
fn link_id(m: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    (m.nlink() > 1).then(|| (m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn link_id(_: &std::fs::Metadata) -> Option<(u64, u64)> {
    None
}

/// The size of `dir`, not following symlinks, giving up after `limit`.
pub fn folder_size(dir: &Path, limit: Duration) -> Size {
    let deadline = Instant::now() + limit;
    let mut total = 0u64;
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut stack = vec![dir.to_path_buf()];
    let mut n = 0u32;
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for entry in entries.flatten() {
            n = n.wrapping_add(1);
            if n.is_multiple_of(256) && Instant::now() >= deadline {
                return Size {
                    bytes: total,
                    partial: true,
                };
            }
            let Ok(m) = entry.path().symlink_metadata() else { continue };
            if m.is_dir() {
                stack.push(entry.path());
            }
            if let Some(id) = link_id(&m)
                && !seen.insert(id)
            {
                continue;
            }
            total += disk_bytes(&m);
        }
    }
    Size {
        bytes: total,
        partial: false,
    }
}

/// Runs `f` over `items` on `jobs` threads, results in the order of `items`.
pub fn par_map<T: Sync, R: Send>(items: &[T], jobs: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<R>>> = items.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..jobs.clamp(1, items.len().max(1)) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let r = f(item);
                    if let Ok(mut slot) = slots[i].lock() {
                        *slot = Some(r);
                    }
                }
            });
        }
    });
    slots.into_iter().filter_map(|m| m.into_inner().ok().flatten()).collect()
}

/// Every folder's size, on `jobs` threads, each limited to `limit`.
pub fn sizes(paths: &[PathBuf], jobs: usize, limit: Duration) -> Vec<Size> {
    par_map(paths, jobs, |p| folder_size(p, limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_the_sum_of_its_files_and_a_symlink_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let big = tempfile::tempdir().unwrap();
        std::fs::write(big.path().join("huge"), vec![1u8; 1 << 20]).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/a"), vec![1u8; 100_000]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(big.path(), dir.path().join("link")).unwrap();
        let s = folder_size(dir.path(), Duration::from_secs(30));
        assert!(!s.partial);
        assert!(s.bytes >= 100_000 && s.bytes < 400_000, "{}", s.bytes);
    }

    #[cfg(unix)]
    #[test]
    fn a_hard_linked_file_counts_once() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), vec![1u8; 200_000]).unwrap();
        std::fs::hard_link(dir.path().join("a"), dir.path().join("b")).unwrap();
        let s = folder_size(dir.path(), Duration::from_secs(30));
        assert!(s.bytes < 300_000, "{}", s.bytes);
    }

    #[test]
    fn a_walk_past_its_limit_stops_and_says_partial() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..600 {
            std::fs::write(dir.path().join(format!("f{n}")), b"x").unwrap();
        }
        let s = folder_size(dir.path(), Duration::ZERO);
        assert!(s.partial);
    }

    #[test]
    fn par_map_keeps_the_order_and_runs_everything_once() {
        let items: Vec<u32> = (0..50).collect();
        let out = par_map(&items, 4, |n| n * 2);
        assert_eq!(out, items.iter().map(|n| n * 2).collect::<Vec<_>>());
        assert!(par_map::<u32, u32>(&[], 4, |n| *n).is_empty());
    }
}
