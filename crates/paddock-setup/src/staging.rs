//! `clean-staging`: what to delete from `$INSTDIR\updates\` (ADR 0053 section 4.2: the staged copy and `tmp\`; `setup.log`
//! stays). The deleting is the glue's.

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

/// The names to delete.
pub fn removals(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .filter(|e| e.is_dir || !KEPT.iter().any(|k| k.eq_ignore_ascii_case(&e.name)))
        .map(|e| e.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, is_dir: bool) -> Entry {
        Entry { name: name.into(), is_dir }
    }

    #[test]
    fn keeps_only_the_log() {
        let list = [e("setup.log", false), e("SETUP.LOG.1", false), e("update-1.2.0.exe", false), e("tmp", true), e("old.bin", false)];
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
}
