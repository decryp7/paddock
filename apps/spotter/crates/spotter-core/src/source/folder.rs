//! Folder sources: a local folder, a mounted network drive (macOS `/Volumes/…`, a mapped drive) or a UNC share
//! (`\\server\share\…` on Windows, using the user's own sign-in). The adapter lists the folder; this decides which
//! entries are candidates.

use crate::pattern::{NamePattern, PatternSyntax};
use crate::release::Candidate;
use crate::time::Timestamp;
use spotter_plugin_api::Config;

/// Which entries a folder source looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Files only (the old app's behaviour).
    Files,
    /// Sub-folders only (builds dropped as one folder each).
    Folders,
    /// Both.
    Both,
}

/// A folder source.
#[derive(Debug, Clone, PartialEq)]
pub struct FolderSource {
    /// The folder, as entered.
    pub path: String,
    /// Which entries.
    pub entries: EntryKind,
    /// Which names.
    pub pattern: NamePattern,
}

/// One entry of a listed folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntry {
    /// File or folder name (the adapter skips names that are not valid Unicode, and says how many it skipped).
    pub name: String,
    /// Full path.
    pub path: String,
    /// Whether it is a folder.
    pub is_dir: bool,
    /// Size of a file.
    pub size: Option<u64>,
    /// Last change.
    pub modified: Option<Timestamp>,
}

impl FolderSource {
    /// From form values (`path`, `entries`, `pattern`, `syntax`).
    pub fn from_config(c: &Config) -> Result<FolderSource, String> {
        let entries = match c.get("entries").unwrap_or("files") {
            "folders" => EntryKind::Folders,
            "both" => EntryKind::Both,
            _ => EntryKind::Files,
        };
        let syntax = PatternSyntax::parse(c.get("syntax").unwrap_or("glob"))?;
        Ok(FolderSource {
            path: c.require("path")?.to_string(),
            entries,
            pattern: NamePattern::new(syntax, c.get("pattern").unwrap_or("*"))?,
        })
    }

    /// The candidates among a folder's entries.
    pub fn candidates(&self, entries: Vec<FolderEntry>) -> Vec<Candidate> {
        entries
            .into_iter()
            .filter(|e| match self.entries {
                EntryKind::Files => !e.is_dir,
                EntryKind::Folders => e.is_dir,
                EntryKind::Both => true,
            })
            .filter(|e| self.pattern.matches(&e.name))
            .map(|e| Candidate {
                version_text: e.name.clone(),
                name: e.name,
                location: e.path,
                published_at: e.modified,
                size: if e.is_dir { None } else { e.size },
                sha256: None,
                prerelease: false,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool) -> FolderEntry {
        FolderEntry {
            name: name.into(),
            path: format!("/drops/{name}"),
            is_dir,
            size: Some(10),
            modified: Some(Timestamp(1)),
        }
    }

    fn names(s: &FolderSource) -> Vec<String> {
        let all = vec![
            entry("R1.01.00T10.txt", false),
            entry("R1.02.00T01", true),
            entry("notes.md", false),
        ];
        s.candidates(all).into_iter().map(|c| c.name).collect()
    }

    #[test]
    fn entries_and_patterns_choose_candidates() {
        let mut s = FolderSource::from_config(
            &Config::new()
                .with("path", "/drops")
                .with("pattern", ".*.txt")
                .with("syntax", "regex"),
        )
        .unwrap();
        assert_eq!(names(&s), ["R1.01.00T10.txt"]);
        s.pattern = NamePattern::any();
        assert_eq!(names(&s), ["R1.01.00T10.txt", "notes.md"]);
        s.entries = EntryKind::Folders;
        assert_eq!(names(&s), ["R1.02.00T01"]);
        s.entries = EntryKind::Both;
        let c = s.candidates(vec![entry("d", true), entry("f", false)]);
        assert_eq!((c[0].size, c[1].size, c[1].location.as_str()), (None, Some(10), "/drops/f"));
    }

    #[test]
    fn config_values_parse() {
        let c = |e: &str| {
            FolderSource::from_config(&Config::new().with("path", "p").with("entries", e))
                .unwrap()
                .entries
        };
        assert_eq!(
            (c("folders"), c("both"), c("files"), c("")),
            (EntryKind::Folders, EntryKind::Both, EntryKind::Files, EntryKind::Files)
        );
        assert!(FolderSource::from_config(&Config::new()).is_err());
        assert!(FolderSource::from_config(&Config::new().with("path", "p").with("syntax", "x")).is_err());
        assert!(FolderSource::from_config(&Config::new().with("path", "p").with("syntax", "regex").with("pattern", "(")).is_err());
    }
}
