//! The tool cache (`$home/bin`, `$home/bin-source`) holds every cached tool: paddock-dev's entries (`<hash>/paddock-dev-<host>`)
//! and those paddock-dev builds itself (`heavy-<hash>/heavy`, `tool_cache.rs`). One policy for all of them:
//!  - each tool keeps its five newest entries (a tool is the name of the file an entry holds, so heavy's never count as ours);
//!  - an entry used within the last day is never removed: a job on the same mount (several runners share a host's tools
//!    folder) runs it again at its end, when its temp folder is already gone and a rebuild cannot work (run 7773), and every
//!    hit touches its entry;
//!  - an entry a run holds is never removed: a hold is a hard link under `$home/held/` (`tool_cache::hold`), so a file with
//!    more than one link is in use (2026-11-14: heavy vanished in the middle of a train-10 `gen check`).
//!
//! Only paddock-dev's own entries take their legacy build folders (`target-<hash>`) with them.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const DAY: Duration = Duration::from_secs(24 * 3600);

fn age(path: &Path) -> Option<Duration> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(SystemTime::now().duration_since(modified).unwrap_or_default())
}

/// The tool an entry holds: the first file whose name does not start with a dot (a temp copy sorts after it); empty for none.
fn cache_tool(entry: &Path) -> String {
    let mut names: Vec<String> = std::fs::read_dir(entry)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names.into_iter().next().unwrap_or_default()
}

/// Whether a file under `dir` has more than one hard link: a run holds it.
fn held(dir: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let Ok(meta) = e.metadata() else { continue };
                if meta.is_dir() {
                    stack.push(e.path());
                } else if meta.is_file() && meta.nlink() > 1 {
                    return true;
                }
            }
        }
        false
    }
    // Windows cannot remove a running program's link, and the hold is Unix only.
    #[cfg(not(unix))]
    {
        let _ = dir;
        false
    }
}

/// Keeps the five newest entries of each tool under `root`; older ones go when a day old and held by no run. `own_name` is
/// paddock-dev's file name (its entries take their legacy build folders with them); `home` is the cache home.
pub fn prune(root: &Path, own_name: &str, home: &Path) {
    let Ok(read) = std::fs::read_dir(root) else { return };
    let mut entries: Vec<(SystemTime, String)> = read
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| {
            (
                e.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH),
                e.file_name().to_string_lossy().into_owned(),
            )
        })
        .collect();
    // `ls -t`: newest first.
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (_, entry) in entries {
        let dir = root.join(&entry);
        let tool = cache_tool(&dir);
        let n = seen.entry(tool.clone()).or_insert(0);
        *n += 1;
        if *n <= 5 {
            continue;
        }
        if age(&dir).is_none_or(|a| a <= DAY) {
            continue;
        }
        if held(&dir) {
            eprintln!(
                "dev: kept the cached tool {} although it is past the five newest: a run holds it (a hard link under {}).",
                dir.display(),
                home.join("held").display()
            );
            continue;
        }
        let _ = std::fs::remove_dir_all(&dir);
        if tool.starts_with(own_name) && !home.join("bin").join(&entry).is_dir() && !home.join("bin-source").join(&entry).is_dir() {
            let _ = std::fs::remove_dir_all(home.join(format!("target-{entry}")));
            let prefix = format!("target-{entry}.");
            for e in std::fs::read_dir(home).into_iter().flatten().flatten() {
                if e.file_name().to_string_lossy().starts_with(&prefix) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
}

/// Subfolders of `dir` whose name starts with `prefix`.
fn subfolders(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(prefix) && e.path().is_dir())
        .map(|e| e.path())
        .collect()
}

/// Build folders of compilers no longer installed go a week after their last build (each build touches its own folder). A build
/// is never judged by the age of a folder: the folder of the compiler in use (`rustc-<ckey>`) is never removed. Legacy per-hash
/// folders (`target-<hash>.*`, an earlier launcher) go when a day old and their hash was never published; they have no owner
/// record, so nothing younger is touched.
pub fn sweep_build_folders(home: &Path, target_base: &Path, ckey: &str) {
    for base in [home, target_base] {
        for t in subfolders(base, "rustc-") {
            if t.file_name().is_some_and(|n| n.to_string_lossy() == format!("rustc-{ckey}")) {
                continue;
            }
            // `find -mtime +7`: more than seven whole days.
            if age(&t).is_some_and(|a| a >= DAY * 8) {
                let _ = std::fs::remove_dir_all(&t);
            }
        }
    }
    for base in [home, target_base] {
        for t in subfolders(base, "target-") {
            let name = t.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let hash = name.trim_start_matches("target-").split('.').next().unwrap_or_default().to_string();
            if home.join("bin").join(&hash).is_dir() || home.join("bin-source").join(&hash).is_dir() {
                continue;
            }
            if age(&t).is_some_and(|a| a > DAY) {
                let _ = std::fs::remove_dir_all(&t);
            }
        }
    }
}

/// A hit counts as use: the prune keeps the newest-used tools, so main's hash outlives a stream of branch hashes.
pub fn touch(dir: &Path) {
    let by_handle = std::fs::File::open(dir).and_then(|f| f.set_modified(SystemTime::now()));
    if by_handle.is_err() {
        // A folder cannot be opened for writing everywhere (Windows): making and removing a file in it also moves its time.
        let probe = dir.join(".touch");
        let _ = std::fs::write(&probe, b"");
        let _ = std::fs::remove_file(&probe);
    }
}
