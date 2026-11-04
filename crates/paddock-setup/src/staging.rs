//! `clean-staging` and the clean-up before an update: what to delete from `$INSTDIR\updates\` (ADR 0053 section 4.2). The
//! log stays. Everything else goes **except what is in use**: the running installer's image and plug-in folder live in a run
//! folder, and an entry that cannot be removed because a program holds it is kept and logged, never a failure.
//! The folder and the removal are behind [`StagingFs`], so the decisions run over a fake.

use crate::checkdir::components;
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
/// The prefix of a run folder that could not be removed at once and was renamed for the next clean-up (it is an ordinary
/// entry, so `clean` removes it).
pub const TRASH_PREFIX: &str = "trash-";

/// The entry of `updates_dir` that contains `keep` (the running installer's own folder, `$EXEDIR`), if any. That entry is
/// never touched: a rename is no test for "in use" (Windows renames a folder with a running image in it), so what is in use is
/// named by the caller instead.
pub fn kept_child(updates_dir: &str, keep: &str) -> Option<String> {
    let base = components(updates_dir);
    let kept = components(keep);
    (!base.is_empty()
        && kept.len() > base.len()
        && base
            .iter()
            .zip(&kept)
            .all(|(a, b)| a.eq_ignore_ascii_case(b)))
    .then(|| kept[base.len()].clone())
}

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
/// `keep` is the name of an entry that is never touched (see [`kept_child`]).
pub fn clean(
    fs: &mut dyn StagingFs,
    keep: Option<&str>,
    log: &mut dyn Sink,
) -> Result<Cleaned, String> {
    let all = fs.list()?;
    if let Some(k) = keep
        && let Some(kept) = all.iter().find(|e| e.name.eq_ignore_ascii_case(k))
    {
        log.line(
            "clean-staging",
            &format!("kept {} (the running installer's folder)", kept.name),
        );
    }
    let entries: Vec<Entry> = all
        .into_iter()
        .filter(|e| keep.is_none_or(|k| !e.name.eq_ignore_ascii_case(k)))
        .collect();
    let wanted = removals(&entries);
    let mut out = Cleaned::default();
    for entry in entries.iter().filter(|e| wanted.contains(&e.name)) {
        match fs.remove(entry) {
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
    fn the_running_installers_folder_is_found_by_its_path() {
        let up = r"C:\Program Files\Dyno\updates";
        assert_eq!(
            kept_child(up, r"C:\Program Files\Dyno\updates\run-1-a").as_deref(),
            Some("run-1-a")
        );
        assert_eq!(
            kept_child(up, r"c:\program files\dyno\UPDATES\run-1-a\tmp\").as_deref(),
            Some("run-1-a")
        );
        assert_eq!(
            kept_child(up, r"\\?\C:\Program Files\Dyno\updates\x.exe").as_deref(),
            Some("x.exe")
        );
        for other in [
            up,
            r"C:\Program Files\Dyno",
            r"C:\Users\a\Temp\nsx1",
            r"C:\Program Files\Dyno\updates2\run-1",
            "",
        ] {
            assert_eq!(kept_child(up, other), None, "{other}");
        }
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
        list_error: Option<String>,
    }
    impl StagingFs for Fake {
        fn list(&mut self) -> Result<Vec<Entry>, String> {
            self.list_error
                .clone()
                .map_or(Ok(self.entries.clone()), Err)
        }
        fn remove(&mut self, entry: &Entry) -> Result<(), RemoveError> {
            if self.busy.contains(&entry.name.as_str()) {
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
            list_error: None,
        }
    }

    #[test]
    fn a_running_installers_folder_is_kept_and_is_not_a_failure() {
        let mut f = fake();
        f.busy = vec!["run-2-b"];
        let mut log = Lines::default();
        let done = clean(&mut f, None, &mut log).unwrap();
        assert_eq!(
            done,
            Cleaned {
                removed: 2,
                in_use: 1,
                failed: 0
            }
        );
        assert_eq!(f.removed, ["run-1-a", "stray.exe"]);
        assert!(
            log.0
                .iter()
                .any(|l| l == "clean-staging: kept run-2-b (in use)"),
            "{:?}",
            log.0
        );
    }

    #[test]
    fn the_kept_entry_is_never_listed_for_removal() {
        let mut f = fake();
        let mut log = Lines::default();
        let done = clean(&mut f, Some("RUN-2-B"), &mut log).unwrap();
        assert_eq!(done.removed, 2);
        assert!(
            log.0
                .iter()
                .any(|l| l == "clean-staging: kept run-2-b (the running installer's folder)"),
            "{:?}",
            log.0
        );
        assert_eq!(f.removed, ["run-1-a", "stray.exe"]);
    }

    #[test]
    fn another_failure_names_the_entry_and_the_cause() {
        let mut f = fake();
        f.broken = vec!["stray.exe"];
        let mut log = Lines::default();
        let done = clean(&mut f, None, &mut log).unwrap();
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
    fn an_unreadable_folder_is_an_error() {
        let mut f = fake();
        f.list_error = Some("access denied".into());
        assert_eq!(
            clean(&mut f, None, &mut Lines::default()),
            Err("access denied".to_string())
        );
    }
}
