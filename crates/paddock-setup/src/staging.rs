//! `clean-staging` and the clean-up before an update: what to delete from `$INSTDIR\updates\` (ADR 0053 section 4.2). The
//! log stays. Everything else goes **except what is in use**: the running installer's image and plug-in folder live in a run
//! folder, and an entry that cannot be removed because a program holds it is kept and logged, never a failure.
//! The folder and the removal are behind [`StagingFs`], so the decisions run over a fake.

use crate::log::Sink;

/// An entry of `$INSTDIR\updates\`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The file name.
    pub name: String,
    /// Whether it is a folder.
    pub is_dir: bool,
}

/// The files kept: the log and its one rotation.
pub const KEPT: [&str; 2] = ["setup.log", "setup.log.1"];
/// The prefix of a folder that was renamed for removal.
pub const TRASH_PREFIX: &str = "trash-";

/// Why an entry was not removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveError {
    /// A program still holds something in it (`ERROR_ACCESS_DENIED` on a mapped image, `ERROR_SHARING_VIOLATION`).
    InUse,
    /// Anything else, with the Windows error text and code.
    Other(String),
}

/// The folder behind `clean`.
pub trait StagingFs {
    /// The entries of `updates\` (no recursion).
    fn list(&mut self) -> Result<Vec<Entry>, String>;
    /// Renames a folder next to itself to `trash-<name>`; this fails (`InUse`) when anything inside is open, which is how a
    /// running installer's folder is found without deleting part of it. Returns the renamed entry.
    fn trash(&mut self, entry: &Entry) -> Result<Entry, RemoveError>;
    /// Removes an entry (a folder with everything in it, a link as a link).
    fn remove(&mut self, entry: &Entry) -> Result<(), RemoveError>;
}

/// The names to delete.
pub fn removals(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .filter(|e| e.is_dir || !KEPT.iter().any(|k| k.eq_ignore_ascii_case(&e.name)))
        .map(|e| e.name.clone())
        .collect()
}

/// What `clean` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cleaned {
    /// Entries removed.
    pub removed: u32,
    /// Entries kept because something in them is in use.
    pub in_use: u32,
    /// Entries that could not be removed for another reason (each named in the log).
    pub failed: u32,
}

/// Removes what [`removals`] names; in-use entries are kept and logged, other failures are logged with the entry and the cause.
pub fn clean(fs: &mut dyn StagingFs, log: &mut dyn Sink) -> Result<Cleaned, String> {
    let entries = fs.list()?;
    let wanted = removals(&entries);
    let mut out = Cleaned::default();
    for entry in entries.iter().filter(|e| wanted.contains(&e.name)) {
        // A folder is renamed first: the rename fails when anything in it is open, so nothing of a folder that a running
        // installer uses is ever half deleted. Only what was renamed is deleted. Leftover `trash-` folders go directly.
        let target = if entry.is_dir && !entry.name.starts_with(TRASH_PREFIX) {
            match fs.trash(entry) {
                Ok(renamed) => renamed,
                Err(RemoveError::InUse) => {
                    out.in_use += 1;
                    log.line("clean-staging", &format!("kept {} (in use)", entry.name));
                    continue;
                }
                Err(RemoveError::Other(why)) => {
                    out.failed += 1;
                    log.line(
                        "clean-staging",
                        &format!("could not rename {}: {why}", entry.name),
                    );
                    continue;
                }
            }
        } else {
            entry.clone()
        };
        match fs.remove(&target) {
            Ok(()) => out.removed += 1,
            Err(RemoveError::InUse) => {
                out.in_use += 1;
                log.line("clean-staging", &format!("kept {} (in use)", entry.name));
            }
            Err(RemoveError::Other(why)) => {
                out.failed += 1;
                log.line(
                    "clean-staging",
                    &format!("could not remove {}: {why}", entry.name),
                );
            }
        }
    }
    log.line(
        "clean-staging",
        &format!(
            "removed {}, kept in use {}, failed {}",
            out.removed, out.in_use, out.failed
        ),
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, is_dir: bool) -> Entry {
        Entry {
            name: name.into(),
            is_dir,
        }
    }

    #[test]
    fn keeps_only_the_log() {
        let list = [
            e("setup.log", false),
            e("SETUP.LOG.1", false),
            e("update-1.2.0.exe", false),
            e("tmp", true),
            e("old.bin", false),
        ];
        assert_eq!(removals(&list), vec!["update-1.2.0.exe", "tmp", "old.bin"]);
    }

    #[test]
    fn a_folder_named_like_the_log_is_not_kept() {
        assert_eq!(removals(&[e("setup.log", true)]), vec!["setup.log"]);
    }

    #[test]
    fn empty() {
        assert!(removals(&[]).is_empty());
    }

    struct Fake {
        entries: Vec<Entry>,
        busy: Vec<&'static str>,
        broken: Vec<&'static str>,
        removed: Vec<String>,
        trashed: Vec<String>,
        stuck: Vec<&'static str>,
        list_error: Option<String>,
    }
    impl StagingFs for Fake {
        fn list(&mut self) -> Result<Vec<Entry>, String> {
            self.list_error
                .clone()
                .map_or(Ok(self.entries.clone()), Err)
        }
        fn trash(&mut self, entry: &Entry) -> Result<Entry, RemoveError> {
            if self.busy.contains(&entry.name.as_str()) {
                return Err(RemoveError::InUse);
            }
            if self.broken.contains(&entry.name.as_str()) {
                return Err(RemoveError::Other("error 5".into()));
            }
            self.trashed.push(entry.name.clone());
            Ok(Entry {
                name: format!("{TRASH_PREFIX}{}", entry.name),
                is_dir: true,
            })
        }
        fn remove(&mut self, entry: &Entry) -> Result<(), RemoveError> {
            if self.stuck.contains(&entry.name.as_str()) {
                return Err(RemoveError::InUse);
            }
            if self.broken.contains(&entry.name.as_str()) {
                return Err(RemoveError::Other("error 1920".into()));
            }
            self.removed.push(entry.name.clone());
            Ok(())
        }
    }
    #[derive(Default)]
    struct Lines(Vec<String>);
    impl Sink for Lines {
        fn line(&mut self, phase: &str, message: &str) {
            self.0.push(format!("{phase}: {message}"));
        }
    }
    fn fake() -> Fake {
        Fake {
            entries: vec![
                e("setup.log", false),
                e("run-1-a", true),
                e("run-2-b", true),
                e("stray.exe", false),
            ],
            busy: vec![],
            broken: vec![],
            removed: vec![],
            trashed: vec![],
            stuck: vec![],
            list_error: None,
        }
    }

    #[test]
    fn a_running_installers_folder_is_kept_and_is_not_a_failure() {
        let mut f = fake();
        f.busy = vec!["run-2-b"];
        let mut log = Lines::default();
        let done = clean(&mut f, &mut log).unwrap();
        assert_eq!(
            done,
            Cleaned {
                removed: 2,
                in_use: 1,
                failed: 0
            }
        );
        // The busy folder was neither renamed nor deleted: no part of a running installer's folder goes.
        assert_eq!(f.trashed, ["run-1-a"]);
        assert_eq!(f.removed, ["trash-run-1-a", "stray.exe"]);
        assert!(
            log.0
                .iter()
                .any(|l| l == "clean-staging: kept run-2-b (in use)"),
            "{:?}",
            log.0
        );
    }

    #[test]
    fn another_failure_names_the_entry_and_the_cause() {
        let mut f = fake();
        f.broken = vec!["stray.exe"];
        let mut log = Lines::default();
        let done = clean(&mut f, &mut log).unwrap();
        assert_eq!(done.failed, 1);
        assert!(
            log.0
                .iter()
                .any(|l| l == "clean-staging: could not remove stray.exe: error 1920"),
            "{:?}",
            log.0
        );
        assert!(
            log.0
                .last()
                .unwrap()
                .ends_with("removed 2, kept in use 0, failed 1")
        );
    }

    #[test]
    fn a_rename_that_fails_for_another_reason_and_a_delete_that_finds_a_file_in_use_are_reported() {
        let mut f = fake();
        f.broken = vec!["run-1-a"];
        f.stuck = vec!["stray.exe"];
        let mut log = Lines::default();
        let done = clean(&mut f, &mut log).unwrap();
        assert_eq!(
            done,
            Cleaned {
                removed: 1,
                in_use: 1,
                failed: 1
            }
        );
        assert!(
            log.0
                .iter()
                .any(|l| l == "clean-staging: could not rename run-1-a: error 5"),
            "{:?}",
            log.0
        );
        assert!(
            log.0
                .iter()
                .any(|l| l == "clean-staging: kept stray.exe (in use)"),
            "{:?}",
            log.0
        );
    }

    #[test]
    fn a_leftover_trash_folder_is_removed_directly() {
        let mut f = fake();
        f.entries = vec![e("trash-run-9-z", true)];
        let done = clean(&mut f, &mut Lines::default()).unwrap();
        assert_eq!(done.removed, 1);
        assert!(f.trashed.is_empty());
        assert_eq!(f.removed, ["trash-run-9-z"]);
    }

    #[test]
    fn an_unreadable_folder_is_an_error() {
        let mut f = fake();
        f.list_error = Some("access denied".into());
        assert_eq!(
            clean(&mut f, &mut Lines::default()),
            Err("access denied".to_string())
        );
    }
}
