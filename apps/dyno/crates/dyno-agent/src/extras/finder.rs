//! The large and duplicate file finder: walks a folder the user chose (bounded in entries and time, never following
//! links), hashes only files that share a size, and keeps the result for an hour so a removal can refer to it. Finding
//! deletes nothing. Removal re-checks every file against the cleanup policy's admission (the file must still be a plain
//! file inside the chosen folder, not in a personal folder the user did not pick, unchanged since the scan, and not in
//! use) before it goes to the Trash or the quarantine.

use crate::disk::walk::{modified, Budget};
use dyno_core::extras::files::{duplicate_groups, largest, same_size_groups, FinderMode, FinderQuery, FinderResult, Walked, MAX_SCANNED};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// How long the walk may take.
pub const WALK_SECS: u64 = 12;
/// How long hashing may take (on top of the walk).
pub const HASH_SECS: u64 = 12;
/// The first bytes hashed to rule out most non-duplicates cheaply.
const HEAD: usize = 64 * 1024;

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

/// Walks `root` and collects files of at least `min_bytes`. Returns them and whether the walk stopped early.
pub fn walk(root: &Path, min_bytes: u64, mut budget: Budget) -> (Vec<Walked>, u32, bool) {
    let mut files = Vec::new();
    let mut scanned = 0u32;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            if !budget.spend() || (scanned as usize) >= MAX_SCANNED {
                return (files, scanned, true);
            }
            scanned += 1;
            let Ok(meta) = entry.path().symlink_metadata() else { continue };
            let ft = meta.file_type();
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() && meta.len() >= min_bytes {
                files.push(Walked {
                    path: entry.path().to_string_lossy().into_owned(),
                    bytes: meta.len(),
                    modified: modified(&meta),
                });
            }
        }
    }
    (files, scanned, false)
}

fn hash_file(path: &str, head_only: bool, deadline: Instant) -> Option<[u8; 32]> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; if head_only { HEAD } else { 1024 * 1024 }];
    loop {
        if Instant::now() >= deadline {
            return None;
        }
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        if head_only {
            break;
        }
    }
    Some(h.finalize().into())
}

/// Hashes the files that share a size: first their heads, then the whole of those whose heads agree.
pub fn find_duplicates(files: &[Walked], min_bytes: u64, deadline: Instant) -> (Vec<(usize, [u8; 32])>, bool) {
    let mut hashed = Vec::new();
    let mut truncated = false;
    for group in same_size_groups(files, min_bytes) {
        let mut by_head: std::collections::BTreeMap<[u8; 32], Vec<usize>> = Default::default();
        for i in group {
            match hash_file(&files[i].path, true, deadline) {
                Some(h) => by_head.entry(h).or_default().push(i),
                None if Instant::now() >= deadline => {
                    truncated = true;
                }
                None => {}
            }
        }
        for same_head in by_head.into_values().filter(|g| g.len() > 1) {
            for i in same_head {
                match hash_file(&files[i].path, false, deadline) {
                    Some(h) => hashed.push((i, h)),
                    None if Instant::now() >= deadline => truncated = true,
                    None => {}
                }
            }
        }
        if truncated {
            break;
        }
    }
    (hashed, truncated)
}

/// Runs the finder. `scan_id` names the result.
pub fn run(query: &FinderQuery, scan_id: &str) -> Result<FinderResult, String> {
    let query = query.clone().validated()?;
    let root = resolve_root(&query.folder)?;
    let (files, scanned, mut truncated) = walk(&root, query.min_bytes, Budget::new(MAX_SCANNED, WALK_SECS));
    let large = if query.mode == FinderMode::Duplicates {
        Vec::new()
    } else {
        largest(&files, query.min_bytes, scan_id)
    };
    let duplicates = if query.mode == FinderMode::Large {
        Vec::new()
    } else {
        let (hashed, cut) = find_duplicates(&files, query.min_bytes, Instant::now() + std::time::Duration::from_secs(HASH_SECS));
        truncated |= cut;
        duplicate_groups(&files, &hashed, scan_id)
    };
    let wasted_bytes = duplicates.iter().fold(0u64, |a, g| a.saturating_add(g.wasted_bytes));
    Ok(FinderResult {
        scan_id: scan_id.into(),
        folder: root.to_string_lossy().into_owned(),
        large,
        duplicates,
        scanned,
        truncated,
        wasted_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::extras::files::FinderMode;

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

    #[test]
    fn finds_the_biggest_files_and_only_real_duplicates() {
        let (_t, root) = fixture();
        let r = run(&query(&root, FinderMode::Both), "s1").unwrap();
        assert_eq!(r.large[0].path, root.join("big.bin").to_string_lossy());
        assert_eq!(r.large.len(), 4, "the tiny file is below the 1 MiB floor");
        assert_eq!(r.duplicates.len(), 1, "{:#?}", r.duplicates);
        assert_eq!(r.duplicates[0].files.len(), 2);
        assert_eq!(r.wasted_bytes, 2 * MB as u64);
        assert!(!r.truncated);
        let only_large = run(&query(&root, FinderMode::Large), "s2").unwrap();
        assert!(only_large.duplicates.is_empty() && !only_large.large.is_empty());
        let only_dups = run(&query(&root, FinderMode::Duplicates), "s3").unwrap();
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
        assert!(run(
            &FinderQuery {
                folder: " ".into(),
                mode: FinderMode::Both,
                min_bytes: 0
            },
            "s"
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn links_are_never_followed() {
        let (_t, root) = fixture();
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
        std::os::unix::fs::symlink(root.join("big.bin"), root.join("link.bin")).unwrap();
        let r = run(&query(&root, FinderMode::Both), "s").unwrap();
        assert!(r.large.iter().all(|f| !f.path.contains("loop") && !f.path.ends_with("link.bin")));
    }

    #[test]
    fn the_walk_stops_at_its_budget() {
        let (_t, root) = fixture();
        let (_, scanned, truncated) = walk(&root, 0, Budget::new(2, 30));
        assert!(truncated && scanned <= 2);
        let (_, _, expired) = walk(
            &root,
            0,
            Budget {
                entries: 100,
                deadline: Instant::now(),
            },
        );
        assert!(expired);
    }

    #[test]
    fn hashing_stops_at_its_deadline() {
        let (_t, root) = fixture();
        let (files, _, _) = walk(&root, MB as u64, Budget::new(1000, 30));
        let (hashed, cut) = find_duplicates(&files, MB as u64, Instant::now());
        assert!(cut && hashed.is_empty());
    }
}
