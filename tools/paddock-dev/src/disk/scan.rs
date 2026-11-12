//! `disk scan`: find the folders, work out their states, measure their sizes and write the index.
//!
//! The folders are what sits directly under each root, and what sits under a container folder of a root (`wt`: the worktrees).
//! A folder with an owner record is tracked; any other is an orphan. Nothing here writes into a folder.

use super::config::Settings;
use super::index::{FolderRow, Index, SizeRow};
use super::measure::{par_map, sizes};
use super::record::{self, Record};
use super::state::{Context, State, inspect};
use crate::worktree::note::now_secs;
use crate::worktree::procs::process_folders;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

/// A folder found under a root.
#[derive(Debug, Clone)]
pub struct Found {
    /// The folder.
    pub path: PathBuf,
    /// Its record, if it has a readable one.
    pub record: Option<Record>,
    /// Why a record file there could not be read.
    pub record_error: Option<String>,
}

fn is_real_dir(p: &std::path::Path) -> bool {
    p.symlink_metadata().is_ok_and(|m| m.is_dir())
}

fn children(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| is_real_dir(p)).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// Every folder the tooling could have made, under every root that exists.
pub fn discover(s: &Settings) -> Vec<Found> {
    let mut out = Vec::new();
    for root in s.roots.iter().filter(|r| r.is_dir()) {
        for child in children(root) {
            let name = child.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            let rec = record::read(&child);
            if matches!(rec, Ok(None)) && s.containers.contains(&name) {
                for inner in children(&child) {
                    out.push(found(inner));
                }
            } else {
                out.push(found(child));
            }
        }
    }
    out
}

fn found(path: PathBuf) -> Found {
    match record::read(&path) {
        Ok(record) => Found {
            path,
            record,
            record_error: None,
        },
        Err(e) => Found {
            path,
            record: None,
            record_error: Some(e),
        },
    }
}

/// What a scan knows.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Every folder.
    pub folders: Vec<FolderRow>,
    /// The newest size of each, by path.
    pub sizes: HashMap<String, SizeRow>,
    /// Things worth telling (an index that was set aside, a record that could not be read).
    pub notes: Vec<String>,
}

/// The row of a found folder, with its state worked out.
pub fn row_of(f: &Found, ctx: &Context) -> FolderRow {
    let i = inspect(&f.path, f.record.as_ref(), ctx);
    let r = f.record.as_ref();
    FolderRow {
        path: f.path.to_string_lossy().to_string(),
        kind: r.map_or(String::new(), |r| r.kind.word().to_string()),
        owner: r.map_or(String::new(), |r| r.owner.clone()),
        branch: r.map_or(String::new(), |r| r.branch.clone()),
        head: r.map_or(String::new(), |r| r.head.clone()),
        created: r.map_or(0, |r| r.created),
        last_used: i.last_used,
        state: i.state.word().to_string(),
        repo: r.map_or(String::new(), |r| r.repo.clone()),
        link: r.map_or(String::new(), |r| r.link.clone()),
    }
}

/// The clock and the processes' folders for [`row_of`].
pub fn context(s: &Settings, now: u64) -> Context {
    Context {
        now,
        live_secs: s.live_secs,
        process_folders: process_folders().ok(),
    }
}

/// How a scan treats sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sizes {
    /// Not at all (a fast rebuild of the records and states).
    Skip,
    /// Measure folders whose cached size is older than the setting.
    Stale,
    /// Measure every folder.
    All,
}

/// Runs a scan. With an index the folders and sizes are written to it; without one they are only returned.
pub fn scan(s: &Settings, mode: Sizes, index: Option<&mut Index>, note: &mut dyn FnMut(&str)) -> Snapshot {
    let started = Instant::now();
    let now = now_secs();
    let mut snap = Snapshot::default();
    let found = discover(s);
    note(&format!("disk: {} folders under {} root(s)", found.len(), s.roots.len()));
    for f in &found {
        if let Some(e) = &f.record_error {
            snap.notes.push(format!("{}: {e}; counted as an orphan", f.path.display()));
        }
    }
    let ctx = context(s, now);
    snap.folders = par_map(&found, s.jobs, |f| row_of(f, &ctx));
    note(&format!("disk: states worked out in {:.1}s", started.elapsed().as_secs_f64()));
    let cached = index.as_ref().and_then(|i| i.latest_sizes().ok()).unwrap_or_default();
    let want: Vec<PathBuf> = match mode {
        Sizes::Skip => Vec::new(),
        Sizes::All => found.iter().map(|f| f.path.clone()).collect(),
        Sizes::Stale => found
            .iter()
            .filter(|f| {
                cached
                    .get(f.path.to_string_lossy().as_ref())
                    .is_none_or(|c| now.saturating_sub(c.measured_at) >= s.size_max_age_secs)
            })
            .map(|f| f.path.clone())
            .collect(),
    };
    let measured = if want.is_empty() {
        Vec::new()
    } else {
        note(&format!(
            "disk: measuring {} folder(s) on {} thread(s), {}s each at most",
            want.len(),
            s.jobs,
            s.folder_timeout.as_secs()
        ));
        sizes(&want, s.jobs, s.folder_timeout)
    };
    let new_rows: Vec<SizeRow> = want
        .iter()
        .zip(&measured)
        .map(|(p, m)| SizeRow {
            path: p.to_string_lossy().to_string(),
            bytes: m.bytes,
            partial: m.partial,
            measured_at: now,
        })
        .collect();
    for r in cached.values() {
        snap.sizes.insert(r.path.clone(), r.clone());
    }
    for r in &new_rows {
        snap.sizes.insert(r.path.clone(), r.clone());
    }
    let present: std::collections::HashSet<&str> = snap.folders.iter().map(|f| f.path.as_str()).collect();
    snap.sizes.retain(|p, _| present.contains(p.as_str()));
    if let Some(i) = index
        && let Err(e) = i.sync_folders(&snap.folders).and_then(|()| i.record_sizes(&new_rows, now))
    {
        snap.notes
            .push(format!("the index could not be written ({e}); this report is not saved"));
    }
    note(&format!("disk: scan finished in {:.1}s", started.elapsed().as_secs_f64()));
    snap
}

/// The folders of a snapshot in a state.
pub fn count_state(snap: &Snapshot, state: State) -> usize {
    snap.folders.iter().filter(|f| f.state == state.word()).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::config::CommonArgs;
    use crate::disk::record::{Kind, Record};

    fn settings(root: &std::path::Path, index: &std::path::Path) -> Settings {
        let args = CommonArgs {
            roots: vec![root.to_path_buf()],
            index: Some(index.to_path_buf()),
            jobs: Some(2),
            ..CommonArgs::default()
        };
        Settings::resolve(&args, &|_| None)
    }

    fn tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("dev-targets");
        // a build folder with a record, a build folder without, a worktree container with one record-less folder.
        std::fs::create_dir_all(root.join("app-target")).unwrap();
        std::fs::write(root.join("app-target/blob"), vec![0u8; 50_000]).unwrap();
        record::write(&root.join("app-target"), &Record::new(Kind::Build, "implementer:x")).unwrap();
        std::fs::create_dir_all(root.join("mystery")).unwrap();
        std::fs::create_dir_all(root.join("wt/old-worktree")).unwrap();
        std::fs::write(root.join("a-file"), b"not a folder").unwrap();
        d
    }

    #[test]
    fn folders_under_a_root_and_its_containers_are_found_and_files_are_not() {
        let d = tree();
        let s = settings(&d.path().join("dev-targets"), &d.path().join("i.sqlite"));
        let names: Vec<String> = discover(&s)
            .iter()
            .map(|f| {
                f.path
                    .strip_prefix(d.path().join("dev-targets"))
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        assert_eq!(names, ["app-target", "mystery", "wt/old-worktree"]);
    }

    #[test]
    fn a_scan_writes_states_and_sizes_and_a_missing_index_is_rebuilt_from_the_records() {
        let d = tree();
        let idx = d.path().join("i.sqlite");
        let s = settings(&d.path().join("dev-targets"), &idx);
        let mut i = Index::open(&idx).unwrap();
        let snap = scan(&s, Sizes::All, Some(&mut i), &mut |_| {});
        assert_eq!(snap.folders.len(), 3);
        assert_eq!(count_state(&snap, State::Orphan), 2);
        let tracked = snap.folders.iter().find(|f| f.path.ends_with("app-target")).unwrap();
        assert_eq!((tracked.kind.as_str(), tracked.owner.as_str()), ("build", "implementer:x"));
        assert!(snap.sizes[&tracked.path].bytes >= 50_000);
        drop(i);
        // The index is lost: the records bring the tracked folders back; the sizes are measured again by the next scan.
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", idx.display()));
        }
        let mut again = Index::open(&idx).unwrap();
        assert!(again.folders().unwrap().is_empty());
        let rebuilt = scan(&s, Sizes::Skip, Some(&mut again), &mut |_| {});
        let rows = again.folders().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rebuilt.folders.iter().filter(|f| !f.kind.is_empty()).count(), 1);
        assert_eq!(rows.iter().find(|r| r.path.ends_with("app-target")).unwrap().owner, "implementer:x");
    }

    #[test]
    fn a_scan_does_not_measure_again_what_was_measured_recently() {
        let d = tree();
        let idx = d.path().join("i.sqlite");
        let s = settings(&d.path().join("dev-targets"), &idx);
        let mut i = Index::open(&idx).unwrap();
        scan(&s, Sizes::Stale, Some(&mut i), &mut |_| {});
        let before = i.history().unwrap().values().map(Vec::len).sum::<usize>();
        let mut said = Vec::new();
        scan(&s, Sizes::Stale, Some(&mut i), &mut |m| said.push(m.to_string()));
        assert_eq!(i.history().unwrap().values().map(Vec::len).sum::<usize>(), before);
        assert!(!said.iter().any(|m| m.contains("measuring")), "{said:?}");
    }

    #[test]
    fn an_unreadable_record_is_an_orphan_with_a_note() {
        let d = tree();
        std::fs::write(d.path().join("dev-targets/mystery").join(record::FILE), "garbage = = =").unwrap();
        let s = settings(&d.path().join("dev-targets"), &d.path().join("i.sqlite"));
        let snap = scan(&s, Sizes::Skip, None, &mut |_| {});
        assert!(
            snap.notes.iter().any(|n| n.contains("mystery") && n.contains("orphan")),
            "{:?}",
            snap.notes
        );
    }
}
