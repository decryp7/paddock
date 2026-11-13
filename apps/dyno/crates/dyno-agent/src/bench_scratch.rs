//! The disk test's scratch folder (ADR 0030 section 1): made by the agent inside a folder Dyno owns (the temp folder for the
//! system drive, `<mount>/.dyno-bench-<run>` for another drive), recorded with its identity before the child starts, and removed
//! when the kind ends, whatever the child did. If the agent itself dies mid-run, the next start removes a folder only when the
//! record's identity still matches what is on disk (rule 53: never by name alone, a name can be reused by someone else).
//! The scratch file inside is the runner's business and cannot outlive the child ([`dyno_bench::disk::scratch`]).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Prefix of every folder this module makes, so cleanup refuses anything else.
pub const FOLDER_PREFIX: &str = ".dyno-bench-";

/// A folder the agent made and has not yet removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchRecord {
    /// The folder.
    pub path: String,
    /// Its identity when it was made (device and inode; volume serial and file index on Windows).
    pub identity: String,
}

/// The records file next to the history.
pub fn records_file(history: &Path) -> PathBuf {
    history.with_file_name("bench-scratch.json")
}

/// A folder's identity, `None` when it cannot be read.
pub fn identity_of(path: &Path) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(path).ok()?;
        Some(format!("{}:{}", m.dev(), m.ino()))
    }
    #[cfg(windows)]
    {
        let f = dyno_platform::fileid::file_facts(path).ok()?;
        Some(format!("{}:{}", f.volume, f.index))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        None
    }
}

/// Reads the records; damage means none.
pub fn read_records(file: &Path) -> Vec<ScratchRecord> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_records(file: &Path, records: &[ScratchRecord]) -> std::io::Result<()> {
    if records.is_empty() {
        return match std::fs::remove_file(file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    std::fs::write(file, serde_json::to_string(records).map_err(std::io::Error::other)?)
}

/// Where the scratch folder of a drive goes: the system drive uses `scratch_root` (the temp folder: the volume the system
/// itself writes to; a Mac's root is read-only), any other drive its own hidden folder at its top.
pub fn folder_path(mount: &Path, is_system: bool, scratch_root: &Path, run_id: &str) -> PathBuf {
    let name = format!("{FOLDER_PREFIX}{run_id}");
    if is_system {
        scratch_root.join(name)
    } else {
        mount.join(name)
    }
}

/// A scratch folder that is removed, and forgotten, when it is dropped.
pub struct ScratchFolder {
    path: PathBuf,
    records: PathBuf,
}

impl ScratchFolder {
    /// Makes the folder (it must not exist) and records its identity before anything is written into it.
    pub fn create(path: &Path, records: &Path) -> std::io::Result<ScratchFolder> {
        std::fs::create_dir(path)?;
        let folder = ScratchFolder {
            path: path.to_path_buf(),
            records: records.to_path_buf(),
        };
        let Some(identity) = identity_of(path) else {
            // Cannot be recorded, so a crash could not be cleaned up by identity: do not start (the drop removes the folder).
            return Err(std::io::Error::other("the new scratch folder's identity cannot be read"));
        };
        let mut all = read_records(records);
        all.retain(|r| r.path != path.to_string_lossy());
        all.push(ScratchRecord {
            path: path.to_string_lossy().into_owned(),
            identity,
        });
        write_records(records, &all)?;
        Ok(folder)
    }

    /// Whether Dyno can create and remove a file here: asked before the run, so a permission prompt (macOS removable and network
    /// volumes) appears while the person is at the picker, never in the middle of a run.
    pub fn probe(&self) -> std::io::Result<()> {
        let f = self.path.join(".dyno-probe");
        std::fs::File::create_new(&f)?;
        std::fs::remove_file(&f)
    }

    /// The folder.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchFolder {
    fn drop(&mut self) {
        let removed = std::fs::remove_dir_all(&self.path);
        log::info!("dyno-agent: scratch folder removed: {}", removed.is_ok());
        if removed.is_ok() || !self.path.exists() {
            let mut all = read_records(&self.records);
            let before = all.len();
            all.retain(|r| r.path != self.path.to_string_lossy());
            if all.len() != before {
                let _ = write_records(&self.records, &all);
            }
        }
    }
}

/// At the agent's start: removes the folders an earlier agent left (it died mid-run) whose identity still matches their record,
/// and forgets every record. Returns what it removed, for the log.
pub fn cleanup_leftovers(records: &Path) -> Vec<String> {
    let all = read_records(records);
    let mut removed = Vec::new();
    for r in &all {
        let p = Path::new(&r.path);
        let ours = p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(FOLDER_PREFIX))
            && identity_of(p).as_deref() == Some(r.identity.as_str());
        if ours && std::fs::remove_dir_all(p).is_ok() {
            removed.push(r.path.clone());
        } else if p.exists() {
            log::info!("dyno-agent: a scratch folder record does not match what is on disk; the folder is left alone");
        }
    }
    let _ = write_records(records, &[]);
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_drive_uses_the_scratch_root_and_another_drive_its_own_top_folder() {
        let root = Path::new("/tmp");
        assert_eq!(folder_path(Path::new("/"), true, root, "7"), Path::new("/tmp/.dyno-bench-7"));
        assert_eq!(
            folder_path(Path::new("/Volumes/USB"), false, root, "7"),
            Path::new("/Volumes/USB/.dyno-bench-7")
        );
    }

    #[test]
    fn a_folder_is_recorded_when_made_and_removed_and_forgotten_when_dropped() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let rec = d.path().join("rec.json");
        let p = d.path().join(".dyno-bench-1");
        let f = ScratchFolder::create(&p, &rec).unwrap();
        assert!(p.is_dir());
        assert_eq!(read_records(&rec).len(), 1);
        f.probe().unwrap();
        assert_eq!(std::fs::read_dir(&p).unwrap().count(), 0, "the probe removes its file");
        drop(f);
        assert!(!p.exists());
        assert!(!rec.exists(), "no records left");
    }

    #[test]
    fn a_folder_that_exists_is_never_taken_over() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let p = d.path().join(".dyno-bench-1");
        std::fs::create_dir(&p).unwrap();
        std::fs::write(p.join("theirs"), b"x").unwrap();
        assert!(ScratchFolder::create(&p, &d.path().join("rec.json")).is_err());
        assert!(p.join("theirs").exists());
    }

    #[test]
    fn restart_cleanup_removes_a_folder_whose_identity_matches() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let rec = d.path().join("rec.json");
        let p = d.path().join(".dyno-bench-2");
        // The agent died: the folder and its record stay (forget = no drop).
        std::mem::forget(ScratchFolder::create(&p, &rec).unwrap());
        std::fs::write(p.join("leftover"), b"x").unwrap();
        assert_eq!(cleanup_leftovers(&rec), vec![p.to_string_lossy().into_owned()]);
        assert!(!p.exists());
        assert!(!rec.exists());
    }

    #[test]
    fn restart_cleanup_leaves_a_folder_whose_identity_does_not_match() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let rec = d.path().join("rec.json");
        let p = d.path().join(".dyno-bench-3");
        std::mem::forget(ScratchFolder::create(&p, &rec).unwrap());
        // Someone removed it and made another folder under the same name: not ours any more.
        std::fs::remove_dir(&p).unwrap();
        std::fs::create_dir(&p).unwrap();
        std::fs::write(p.join("theirs"), b"x").unwrap();
        // The identity of a recreated directory can repeat on some file systems (the inode is reused): make the record unmistakably stale.
        let mut all = read_records(&rec);
        all[0].identity = "0:0".into();
        write_records(&rec, &all).unwrap();
        assert!(cleanup_leftovers(&rec).is_empty());
        assert!(p.join("theirs").exists());
        assert!(!rec.exists(), "the record is dropped either way");
    }

    #[test]
    fn restart_cleanup_refuses_a_name_that_is_not_ours_even_with_a_matching_identity() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let rec = d.path().join("rec.json");
        let p = d.path().join("Documents");
        std::fs::create_dir(&p).unwrap();
        write_records(
            &rec,
            &[ScratchRecord {
                path: p.to_string_lossy().into_owned(),
                identity: identity_of(&p).unwrap(),
            }],
        )
        .unwrap();
        assert!(cleanup_leftovers(&rec).is_empty());
        assert!(p.is_dir());
    }

    #[test]
    fn damaged_records_mean_none() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let rec = d.path().join("rec.json");
        std::fs::write(&rec, b"not json").unwrap();
        assert!(read_records(&rec).is_empty());
        assert!(cleanup_leftovers(&rec).is_empty());
    }
}
