//! The large and duplicate file finder: walks a folder the user chose (bounded in entries and time, never following
//! links), hashes only files that share a size, and keeps the result for an hour so a removal can refer to it. Finding
//! deletes nothing. Removal re-checks every file against the cleanup policy's admission (the file must still be a plain
//! file inside the chosen folder, not in a personal folder the user did not pick, unchanged since the scan, and not in
//! use) before it goes to the Trash or the quarantine.

use crate::cleanup::walk::{modified, Budget};
use dyno_core::cleanup::{admit, Candidate, CleanupRisk, FailedItem, SCAN_VALID_MS};
use dyno_core::extras::files::{
    duplicate_groups, find_file, largest, removes_every_copy, same_size_groups, FinderMode, FinderQuery, FinderResult, Walked, MAX_SCANNED,
};
use dyno_core::time::Timestamp;
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
/// The most files one removal may name.
pub const MAX_REMOVE: usize = 5_000;

/// A finder result kept for a removal.
pub struct Kept {
    /// The result the user saw.
    pub result: FinderResult,
    /// The folder as resolved (canonical).
    pub root: PathBuf,
    /// When it was made.
    pub at: Timestamp,
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
pub fn run(query: &FinderQuery, scan_id: &str) -> Result<(FinderResult, PathBuf), String> {
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
    Ok((
        FinderResult {
            scan_id: scan_id.into(),
            folder: root.to_string_lossy().into_owned(),
            large,
            duplicates,
            scanned,
            truncated,
            wasted_bytes,
        },
        root,
    ))
}

/// What a removal may do, decided from the kept result and the live file system. Files that fail a check are listed with
/// the reason; the rest are returned as `(real path, bytes)`.
#[derive(Debug)]
pub struct Admitted {
    /// Ready to move.
    pub ready: Vec<(PathBuf, u64)>,
    /// Refused, with the reason.
    pub failed: Vec<FailedItem>,
}

/// Checks the selected files of a kept result. Whole-request problems (a stale result, an unknown id, every copy of a
/// duplicate selected) are errors; single files that changed or are protected are `failed` entries.
pub fn admit_selection(
    kept: &Kept,
    ids: &[String],
    home: &Path,
    now: Timestamp,
    in_use: &mut dyn FnMut(&Path) -> Option<String>,
) -> Result<Admitted, String> {
    if now.since(kept.at) > SCAN_VALID_MS {
        return Err("This result is more than an hour old; look again before removing anything.".into());
    }
    if ids.is_empty() {
        return Err("Select something to remove.".into());
    }
    if ids.len() > MAX_REMOVE {
        return Err("That selection is too large.".into());
    }
    if removes_every_copy(&kept.result, ids) {
        return Err("That would remove every copy of a duplicate. Keep at least one copy of each.".into());
    }
    let root = kept.root.to_string_lossy().into_owned();
    let home = std::fs::canonicalize(home)
        .unwrap_or_else(|_| home.to_path_buf())
        .to_string_lossy()
        .into_owned();
    let mut seen = std::collections::HashSet::new();
    let mut out = Admitted {
        ready: Vec::new(),
        failed: Vec::new(),
    };
    for id in ids {
        let file = find_file(&kept.result, id).ok_or_else(|| format!("Item {id} is not part of this result."))?;
        if !seen.insert(file.path.clone()) {
            continue;
        }
        let mut refuse = |why: String| {
            out.failed.push(FailedItem {
                path: file.path.clone(),
                reason: why,
            })
        };
        let path = Path::new(&file.path);
        let meta = match path.symlink_metadata() {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                refuse("It is no longer there.".into());
                continue;
            }
            Err(e) => {
                refuse(format!("It cannot be read: {e}"));
                continue;
            }
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            refuse("It is no longer a plain file.".into());
            continue;
        }
        // A link in the middle of the path would send the move somewhere else: use the real parent.
        let Some(real) = path
            .parent()
            .and_then(|p| std::fs::canonicalize(p).ok())
            .and_then(|p| path.file_name().map(|n| p.join(n)))
        else {
            refuse("Its folder cannot be read.".into());
            continue;
        };
        if meta.len() != file.bytes || modified(&meta) > file.modified {
            refuse("It changed after the scan; look again to see it.".into());
            continue;
        }
        let admitted = admit(
            file.id.clone(),
            Candidate {
                path: real.to_string_lossy().into_owned(),
                bytes: meta.len(),
                modified: modified(&meta),
                folder: false,
                link: false,
                reason: None,
            },
            std::slice::from_ref(&root),
            &home,
            CleanupRisk::Personal,
            0,
            now,
            "Chosen in the file finder",
        );
        if let Err(why) = admitted {
            refuse(why);
            continue;
        }
        if let Some(who) = in_use(&real) {
            refuse(format!("In use by {who}. Close it and try again."));
            continue;
        }
        out.ready.push((real, meta.len()));
    }
    Ok(out)
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
        let (r, canon) = run(&query(&root, FinderMode::Both), "s1").unwrap();
        assert_eq!(canon, root);
        assert_eq!(r.large[0].path, root.join("big.bin").to_string_lossy());
        assert_eq!(r.large.len(), 4, "the tiny file is below the 1 MiB floor");
        assert_eq!(r.duplicates.len(), 1, "{:#?}", r.duplicates);
        assert_eq!(r.duplicates[0].files.len(), 2);
        assert_eq!(r.wasted_bytes, 2 * MB as u64);
        assert!(!r.truncated);
        let (only_large, _) = run(&query(&root, FinderMode::Large), "s2").unwrap();
        assert!(only_large.duplicates.is_empty() && !only_large.large.is_empty());
        let (only_dups, _) = run(&query(&root, FinderMode::Duplicates), "s3").unwrap();
        assert!(only_dups.large.is_empty() && only_dups.duplicates.len() == 1);
    }

    #[test]
    fn bad_folders_say_what_to_fix() {
        assert!(resolve_root("relative/path").unwrap_err().contains("full path"));
        assert!(resolve_root("/definitely/not/here/dyno").unwrap_err().contains("cannot be opened"));
        let (_t, root) = fixture();
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
        let (r, _) = run(&query(&root, FinderMode::Both), "s").unwrap();
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

    fn kept(root: &Path, r: FinderResult, at: i64) -> Kept {
        Kept {
            result: r,
            root: root.to_path_buf(),
            at: Timestamp(at),
        }
    }

    fn never_in_use() -> impl FnMut(&Path) -> Option<String> {
        |_| None
    }

    #[test]
    fn a_removal_is_refused_when_stale_empty_or_would_leave_no_copy() {
        let (_t, root) = fixture();
        let (r, canon) = run(&query(&root, FinderMode::Both), "s").unwrap();
        let ids: Vec<String> = r.duplicates[0].files.iter().map(|f| f.id.clone()).collect();
        let k = kept(&canon, r, 1_000);
        let home = root.parent().unwrap();
        let now = Timestamp(2_000);
        assert!(admit_selection(&k, &ids, home, now.plus(SCAN_VALID_MS), &mut never_in_use())
            .unwrap_err()
            .contains("more than an hour"));
        assert!(admit_selection(&k, &[], home, now, &mut never_in_use())
            .unwrap_err()
            .contains("Select"));
        assert!(admit_selection(&k, &ids, home, now, &mut never_in_use())
            .unwrap_err()
            .contains("every copy"));
        assert!(admit_selection(&k, &["nope".into()], home, now, &mut never_in_use())
            .unwrap_err()
            .contains("not part"));
        assert!(
            admit_selection(&k, &vec!["x".to_string(); MAX_REMOVE + 1], home, now, &mut never_in_use())
                .unwrap_err()
                .contains("too large")
        );
    }

    #[test]
    fn single_files_are_re_checked_against_the_disk() {
        let (_t, root) = fixture();
        let (r, canon) = run(&query(&root, FinderMode::Both), "s").unwrap();
        let keep = r.duplicates[0].files[0].clone();
        let drop = r.duplicates[0].files[1].clone();
        let big = r.large.iter().find(|f| f.path.ends_with("big.bin")).unwrap().clone();
        let k = kept(&canon, r, 1_000);
        let home = root.parent().unwrap();
        let now = Timestamp(2_000);

        let a = admit_selection(
            &k,
            &[drop.id.clone(), drop.id.clone(), big.id.clone()],
            home,
            now,
            &mut never_in_use(),
        )
        .unwrap();
        assert_eq!(a.ready.len(), 2, "a repeated id counts once");
        assert!(a.failed.is_empty());

        // In use: refused with the program's name.
        let a = admit_selection(&k, &[drop.id.clone()], home, now, &mut |_| Some("Player".into())).unwrap();
        assert!(a.failed[0].reason.contains("In use by Player"));

        // Changed since the scan.
        std::fs::write(&drop.path, vec![9u8; 3 * MB]).unwrap();
        let a = admit_selection(&k, &[drop.id.clone()], home, now, &mut never_in_use()).unwrap();
        assert!(a.failed[0].reason.contains("changed after the scan"));

        // Gone.
        std::fs::remove_file(&drop.path).unwrap();
        let a = admit_selection(&k, &[drop.id.clone()], home, now, &mut never_in_use()).unwrap();
        assert!(a.failed[0].reason.contains("no longer there"));
        let _ = keep;
    }

    #[cfg(unix)]
    #[test]
    fn a_file_swapped_for_a_link_or_moved_outside_the_folder_is_refused() {
        let (t, root) = fixture();
        let (r, canon) = run(&query(&root, FinderMode::Large), "s").unwrap();
        let big = r.large.iter().find(|f| f.path.ends_with("big.bin")).unwrap().clone();
        let k = kept(&canon, r, 1_000);
        let outside = std::fs::canonicalize(t.path()).unwrap().join("outside.bin");
        std::fs::write(&outside, vec![1u8; 5 * MB]).unwrap();
        std::fs::remove_file(&big.path).unwrap();
        std::os::unix::fs::symlink(&outside, &big.path).unwrap();
        let a = admit_selection(&k, &[big.id.clone()], root.parent().unwrap(), Timestamp(2_000), &mut never_in_use()).unwrap();
        assert!(a.failed[0].reason.contains("no longer a plain file"), "{:?}", a.failed);
        assert!(a.ready.is_empty());
        assert!(outside.exists());
    }

    #[test]
    fn personal_folders_are_refused_unless_the_user_picked_them() {
        let t = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(t.path()).unwrap();
        let docs = home.join("Documents");
        write(&docs, "movie.mov", 3, 2 * MB);
        // Picking the whole home folder does not make Documents fair game.
        let (r, canon) = run(&query(&home, FinderMode::Large), "s").unwrap();
        let id = r.large[0].id.clone();
        let k = kept(&canon, r, 1_000);
        let a = admit_selection(&k, &[id], &home, Timestamp(2_000), &mut never_in_use()).unwrap();
        assert!(a.failed[0].reason.contains("personal folder"), "{:?}", a.failed);
        // Picking Documents itself is the user's own choice.
        let (r, canon) = run(&query(&docs, FinderMode::Large), "s").unwrap();
        let id = r.large[0].id.clone();
        let k = kept(&canon, r, 1_000);
        let a = admit_selection(&k, &[id], &home, Timestamp(2_000), &mut never_in_use()).unwrap();
        assert_eq!(a.ready.len(), 1, "{:?}", a.failed);
    }
}
