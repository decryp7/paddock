//! Folder sources: a local folder, a mounted network drive (macOS `/Volumes/…`, a mapped drive) or a UNC share
//! (`\\server\share\…` on Windows, using the user's own sign-in). The adapter walks the folder; everything that
//! decides lives here as pure functions: which entries are candidates (judged on their names only, before anything
//! is read from the share), what the previous look remembered, and when a re-listing can be skipped.

use crate::extract::Ignored;
use crate::pattern::{NamePattern, PatternSyntax};
use crate::release::{Candidate, Skip};
use crate::schema::Config;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

/// The text the version, id and channel are read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatchAgainst {
    /// The file (or folder) name.
    #[default]
    Name,
    /// The name of the folder the file is in (`R3.10.00T32` for `drops\R3.10.00T32\setup.exe`).
    Folder,
    /// The path relative to the watched folder, with `/` separators.
    Path,
}

/// Deepest folder level that can be walked.
pub const MAX_DEPTH: u32 = 8;
/// Default cap on entries looked at in one check.
pub const DEFAULT_MAX_ENTRIES: u32 = 200_000;
/// Smallest and largest cap that can be set.
pub const MAX_ENTRIES_RANGE: (u32, u32) = (100, 5_000_000);
/// Default time one listing may take (seconds), and its range.
pub const DEFAULT_SCAN_SECS: u32 = 60;
/// Default time a new or changed file must stay unchanged before it is announced (seconds).
pub const DEFAULT_SETTLE_SECS: u32 = 5;
/// Default number of checks between full re-listings.
pub const DEFAULT_RELIST_EVERY: u32 = 12;
/// Most extensions in the filter.
pub const MAX_EXTENSIONS: usize = 32;

/// The bounds and timing of one scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanLimits {
    /// Entries looked at before the scan gives up ("too many files").
    pub max_entries: u32,
    /// Seconds one listing may take.
    pub timeout_secs: u32,
    /// Seconds a new or changed file must stay unchanged before it counts (0: no wait).
    pub settle_secs: u32,
    /// A full re-listing at least every this many checks, even when the folders look unchanged.
    pub relist_every: u32,
    /// Files up to this many MiB are hashed when they change, to tell a real change from a touch (0: never).
    pub hash_below_mib: u32,
}

impl Default for ScanLimits {
    fn default() -> ScanLimits {
        ScanLimits {
            max_entries: DEFAULT_MAX_ENTRIES,
            timeout_secs: DEFAULT_SCAN_SECS,
            settle_secs: DEFAULT_SETTLE_SECS,
            relist_every: DEFAULT_RELIST_EVERY,
            hash_below_mib: 0,
        }
    }
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
    /// What the version is read from.
    pub against: MatchAgainst,
    /// How many folder levels below `path` are walked (0: only `path`).
    pub depth: u32,
    /// Only files with these extensions (lower case, no dot); empty for all.
    pub extensions: Vec<String>,
    /// Bounds and timing.
    pub limits: ScanLimits,
}

/// One entry of a listed folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntry {
    /// File or folder name (the adapter skips names that are not valid Unicode, and says how many it skipped).
    pub name: String,
    /// Full path.
    pub path: String,
    /// The folder it is in, relative to the watched folder, with `/` separators (empty at the top).
    pub rel_dir: String,
    /// Whether it is a folder.
    pub is_dir: bool,
    /// Size of a file.
    pub size: Option<u64>,
    /// Last change.
    pub modified: Option<Timestamp>,
    /// Creation time, where the share reports it.
    pub created: Option<Timestamp>,
}

/// The verdict on one name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Not the kind of entry the source looks at (a folder when it wants files); not worth mentioning.
    NotThisKind,
    /// Left out, and why.
    Skip(Skip),
    /// A candidate: the text the version is read from.
    Keep(String),
}

/// Splits a comma, semicolon or space separated list of extensions (`iso, .EXE`) into lower-case names.
pub fn parse_extensions(list: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for part in list
        .split([',', ';', ' '])
        .map(|p| p.trim().trim_start_matches(['.', '*']).to_ascii_lowercase())
    {
        if part.is_empty() {
            continue;
        }
        if part.len() > 12 || !part.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') {
            return Err(format!(
                "\"{part}\" is not a file extension (use letters and digits, such as iso, exe or tar.gz)."
            ));
        }
        if !out.contains(&part) {
            out.push(part);
        }
    }
    if out.len() > MAX_EXTENSIONS {
        return Err(format!("At most {MAX_EXTENSIONS} extensions can be listed."));
    }
    Ok(out)
}

fn ranged(c: &Config, key: &str, default: u32, min: u32, max: u32, what: &str) -> Result<u32, String> {
    let n = c.integer(key, i64::from(default))?;
    u32::try_from(n)
        .ok()
        .filter(|n| (min..=max).contains(n))
        .ok_or_else(|| format!("{what} must be a whole number from {min} to {max}."))
}

impl FolderSource {
    /// From form values (`path`, `entries`, `pattern`, `syntax`, `matchAgainst`, `depth`, `extensions` and the scan
    /// limits). Every key but `path` is optional.
    pub fn from_config(c: &Config) -> Result<FolderSource, String> {
        let entries = match c.get("entries").unwrap_or("files") {
            "folders" => EntryKind::Folders,
            "both" => EntryKind::Both,
            _ => EntryKind::Files,
        };
        let against = match c.get("matchAgainst").unwrap_or("name") {
            "name" => MatchAgainst::Name,
            "folder" => MatchAgainst::Folder,
            "path" => MatchAgainst::Path,
            other => return Err(format!("\"{other}\" is not something to match against (name, folder or path).")),
        };
        let syntax = PatternSyntax::parse(c.get("syntax").unwrap_or("glob"))?;
        Ok(FolderSource {
            path: c.require("path")?.to_string(),
            entries,
            pattern: NamePattern::new(syntax, c.get("pattern").unwrap_or("*"))?,
            against,
            depth: ranged(c, "depth", 0, 0, MAX_DEPTH, "The folder depth")?,
            extensions: parse_extensions(c.get("extensions").unwrap_or(""))?,
            limits: ScanLimits {
                max_entries: ranged(
                    c,
                    "maxEntries",
                    DEFAULT_MAX_ENTRIES,
                    MAX_ENTRIES_RANGE.0,
                    MAX_ENTRIES_RANGE.1,
                    "The entry limit",
                )?,
                timeout_secs: ranged(c, "scanSeconds", DEFAULT_SCAN_SECS, 5, 1800, "The scan time limit")?,
                settle_secs: ranged(c, "settleSeconds", DEFAULT_SETTLE_SECS, 0, 300, "The settle time")?,
                relist_every: ranged(c, "relistEvery", DEFAULT_RELIST_EVERY, 1, 1000, "The re-listing interval")?,
                hash_below_mib: ranged(c, "hashBelowMib", 0, 0, 256, "The hash size limit")?,
            },
        })
    }

    /// The text the version is read from for an entry called `name` in `rel_dir`.
    pub fn subject(&self, name: &str, rel_dir: &str) -> String {
        match self.against {
            MatchAgainst::Name => name.to_string(),
            MatchAgainst::Folder => rel_dir.rsplit('/').next().filter(|f| !f.is_empty()).unwrap_or(name).to_string(),
            MatchAgainst::Path if rel_dir.is_empty() => name.to_string(),
            MatchAgainst::Path => format!("{rel_dir}/{name}"),
        }
    }

    /// Judges an entry by its name alone, cheapest test first; nothing is read from the share. `readable` says
    /// whether the version scheme can read the text (a pure name check supplied by the caller), so names that could
    /// never become a release are dropped before any file is looked at.
    pub fn judge(&self, name: &str, rel_dir: &str, is_dir: bool, readable: Option<&dyn Fn(&str) -> bool>) -> Verdict {
        let wanted = match self.entries {
            EntryKind::Files => !is_dir,
            EntryKind::Folders => is_dir,
            EntryKind::Both => true,
        };
        if !wanted {
            return Verdict::NotThisKind;
        }
        if !is_dir && !self.extensions.is_empty() && !self.has_extension(name) {
            return Verdict::Skip(Skip::Extension);
        }
        if !self.pattern.matches(name) {
            return Verdict::Skip(Skip::Name);
        }
        let subject = self.subject(name, rel_dir);
        if readable.is_some_and(|ok| !ok(&subject)) {
            return Verdict::Skip(Skip::Unread(Ignored::NoMatch));
        }
        Verdict::Keep(subject)
    }

    fn has_extension(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        // Checksum files travel with their file whatever the filter says.
        crate::release::checksum_target(&lower).is_some()
            || self
                .extensions
                .iter()
                .any(|e| lower.len() > e.len() && lower.ends_with(e.as_str()) && lower.as_bytes()[lower.len() - e.len() - 1] == b'.')
    }

    /// The candidate for a kept entry.
    pub fn candidate(&self, e: FolderEntry, subject: String) -> Candidate {
        Candidate {
            version_text: subject,
            name: e.name,
            location: e.path,
            published_at: e.modified,
            size: if e.is_dir { None } else { e.size },
            sha256: None,
            prerelease: false,
            created_at: e.created,
        }
    }

    /// The candidates among a folder's entries (no scheme prefilter).
    pub fn candidates(&self, entries: Vec<FolderEntry>) -> Vec<Candidate> {
        entries
            .into_iter()
            .filter_map(|e| match self.judge(&e.name, &e.rel_dir, e.is_dir, None) {
                Verdict::Keep(subject) => Some(self.candidate(e, subject)),
                _ => None,
            })
            .collect()
    }
}

/// How a scan went, for the wizard ("Scanned 48 213 entries in 2.4 s, 37 matched") and the watch's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ScanStats {
    /// Entries looked at (0 when the previous listing was reused).
    pub scanned: u64,
    /// Entries kept as candidates.
    pub matched: u64,
    /// Folders walked.
    pub folders: u64,
    /// How long it took, in milliseconds.
    pub took_ms: u64,
    /// The previous listing was reused because no folder changed.
    pub reused: bool,
    /// New or changed files left out until they stop changing.
    pub settling: u64,
}

/// One remembered match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapEntry {
    /// Name.
    pub name: String,
    /// Folder relative to the watched one.
    pub rel_dir: String,
    /// Whether it is a folder.
    pub is_dir: bool,
    /// Size.
    pub size: Option<u64>,
    /// Last change, in milliseconds.
    pub modified: Option<i64>,
    /// SHA-256 of a small file (hex), when hashing is on.
    #[serde(default)]
    pub hash: Option<String>,
}

impl SnapEntry {
    /// The key of the entry.
    pub fn key(&self) -> String {
        if self.rel_dir.is_empty() {
            self.name.clone()
        } else {
            format!("{}/{}", self.rel_dir, self.name)
        }
    }
}

/// What the last look at a folder remembers: only the matches (name, size, mtime) and the modified times of the
/// folders walked. Versioned like the rest of the state; a missing or damaged one just means a full listing.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    /// Snapshot format.
    pub schema: u32,
    /// Identifies the settings it was made under; a snapshot made under other settings is not trusted.
    pub fingerprint: String,
    /// Folders walked (relative, `""` for the top) with their modified times (milliseconds); `None` when the share
    /// did not report one.
    pub dirs: BTreeMap<String, Option<i64>>,
    /// The matches.
    pub entries: Vec<SnapEntry>,
    /// Checks since the last full listing.
    pub since_full: u32,
}

/// The snapshot format written now.
pub const SNAPSHOT_SCHEMA: u32 = 1;

impl Snapshot {
    /// Reads a stored snapshot; anything unreadable or from another format is no snapshot.
    pub fn parse(text: &str) -> Option<Snapshot> {
        serde_json::from_str::<Snapshot>(text).ok().filter(|s| s.schema == SNAPSHOT_SCHEMA)
    }

    /// The stored form.
    pub fn render(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// The remembered entry for a key.
    pub fn entry(&self, key: &str) -> Option<&SnapEntry> {
        self.entries.iter().find(|e| e.key() == key)
    }

    /// Whether the listing can be reused: every folder reports a modified time, none changed, and a full listing
    /// was done within the last `relist_every` checks.
    pub fn can_reuse(&self, now_dirs: &BTreeMap<String, Option<i64>>, relist_every: u32) -> bool {
        self.schema == SNAPSHOT_SCHEMA
            && !self.dirs.is_empty()
            && self.since_full + 1 < relist_every
            && self.dirs.len() == now_dirs.len()
            && self
                .dirs
                .iter()
                .all(|(dir, then)| matches!((then, now_dirs.get(dir)), (Some(a), Some(Some(b))) if *a > 0 && a == b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool) -> FolderEntry {
        FolderEntry {
            name: name.into(),
            path: format!("/drops/{name}"),
            rel_dir: String::new(),
            is_dir,
            size: Some(10),
            modified: Some(Timestamp(1)),
            created: None,
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

    fn src(extra: &[(&str, &str)]) -> FolderSource {
        let mut c = Config::new().with("path", "/drops");
        for (k, v) in extra {
            c = c.with(k, v);
        }
        FolderSource::from_config(&c).unwrap()
    }

    #[test]
    fn scale_options_have_defaults_and_ranges() {
        let d = src(&[]);
        assert_eq!((d.depth, d.against, d.limits), (0, MatchAgainst::Name, ScanLimits::default()));
        assert!(d.extensions.is_empty());
        let s = src(&[
            ("depth", "3"),
            ("matchAgainst", "folder"),
            ("maxEntries", "1000"),
            ("settleSeconds", "0"),
            ("extensions", "ISO, .exe;tar.gz iso"),
        ]);
        assert_eq!(
            (s.depth, s.against, s.limits.max_entries, s.limits.settle_secs),
            (3, MatchAgainst::Folder, 1000, 0)
        );
        assert_eq!(s.extensions, ["iso", "exe", "tar.gz"]);
        for (k, v, msg) in [
            ("depth", "9", "folder depth"),
            ("maxEntries", "5", "entry limit"),
            ("scanSeconds", "1", "time limit"),
            ("settleSeconds", "301", "settle time"),
            ("relistEvery", "0", "re-listing"),
            ("hashBelowMib", "999", "hash size"),
            ("matchAgainst", "x", "match against"),
            ("extensions", "a/b", "not a file extension"),
        ] {
            let e = FolderSource::from_config(&Config::new().with("path", "p").with(k, v)).unwrap_err();
            assert!(e.contains(msg), "{k}: {e}");
        }
        let many = (0..40).map(|i| format!("e{i}")).collect::<Vec<_>>().join(",");
        assert!(parse_extensions(&many).unwrap_err().contains("At most"));
    }

    #[test]
    fn names_are_judged_before_anything_is_read() {
        let s = src(&[("extensions", "iso,exe"), ("pattern", "Acme*"), ("syntax", "glob")]);
        let readable = |t: &str| t.contains("R3");
        let j = |n: &str| s.judge(n, "", false, Some(&readable));
        assert_eq!(j("Acme-R3.iso"), Verdict::Keep("Acme-R3.iso".into()));
        assert_eq!(j("Acme-R3.zip"), Verdict::Skip(Skip::Extension));
        assert_eq!(j("Other-R3.iso"), Verdict::Skip(Skip::Name));
        assert_eq!(j("Acme-R4.iso"), Verdict::Skip(Skip::Unread(Ignored::NoMatch)));
        assert_eq!(
            j("Acme-R3.iso.sha256"),
            Verdict::Keep("Acme-R3.iso.sha256".into()),
            "checksums travel with their file"
        );
        assert_eq!(s.judge("Acme-R3", "", true, None), Verdict::NotThisKind);
        let folders = FolderSource {
            entries: EntryKind::Both,
            ..src(&[("extensions", "iso")])
        };
        assert_eq!(
            folders.judge("R3", "", true, None),
            Verdict::Keep("R3".into()),
            "folders have no extension"
        );
    }

    #[test]
    fn the_version_is_read_from_the_name_the_folder_or_the_path() {
        let path = |a: &str| src(&[("matchAgainst", a)]);
        assert_eq!(path("name").subject("setup.exe", "R3.10.00T32"), "setup.exe");
        assert_eq!(path("folder").subject("setup.exe", "drops/R3.10.00T32"), "R3.10.00T32");
        assert_eq!(
            path("folder").subject("setup.exe", ""),
            "setup.exe",
            "at the top the name is all there is"
        );
        assert_eq!(path("path").subject("setup.exe", "a/R3"), "a/R3/setup.exe");
        assert_eq!(path("path").subject("setup.exe", ""), "setup.exe");
        let e = FolderEntry {
            rel_dir: "R3.10.00T32".into(),
            ..entry("setup.exe", false)
        };
        let c = path("folder").candidates(vec![e]);
        assert_eq!((c[0].version_text.as_str(), c[0].name.as_str()), ("R3.10.00T32", "setup.exe"));
    }

    fn snap(dirs: &[(&str, Option<i64>)], since_full: u32) -> Snapshot {
        Snapshot {
            schema: SNAPSHOT_SCHEMA,
            fingerprint: String::new(),
            dirs: dirs.iter().map(|(d, t)| (d.to_string(), *t)).collect(),
            entries: vec![SnapEntry {
                name: "a.zip".into(),
                rel_dir: "x".into(),
                is_dir: false,
                size: Some(1),
                modified: Some(5),
                hash: None,
            }],
            since_full,
        }
    }

    #[test]
    fn a_listing_is_reused_only_when_it_is_safe() {
        let before = snap(&[("", Some(10)), ("x", Some(20))], 0);
        let same: BTreeMap<_, _> = [("".to_string(), Some(10)), ("x".to_string(), Some(20))].into();
        assert!(before.can_reuse(&same, 12));
        assert!(!before.can_reuse(&same, 1), "a full listing every check");
        assert!(
            !snap(&[("", Some(10)), ("x", Some(20))], 11).can_reuse(&same, 12),
            "a full listing at least every N checks"
        );
        let changed: BTreeMap<_, _> = [("".to_string(), Some(10)), ("x".to_string(), Some(21))].into();
        assert!(!before.can_reuse(&changed, 12));
        let fewer: BTreeMap<_, _> = [("".to_string(), Some(10))].into();
        assert!(!before.can_reuse(&fewer, 12), "a folder appeared or vanished");
        let unreported: BTreeMap<_, _> = [("".to_string(), None), ("x".to_string(), Some(20))].into();
        assert!(!before.can_reuse(&unreported, 12), "the share does not report folder times");
        assert!(
            !snap(&[("", Some(0))], 0).can_reuse(&[("".to_string(), Some(0))].into(), 12),
            "zero is not a time"
        );
        assert!(!Snapshot::default().can_reuse(&BTreeMap::new(), 12));
    }

    #[test]
    fn snapshots_round_trip_and_damage_means_no_snapshot() {
        let s = snap(&[("", Some(1))], 3);
        assert_eq!(Snapshot::parse(&s.render()), Some(s.clone()));
        assert_eq!(s.entry("x/a.zip").map(|e| e.size), Some(Some(1)));
        assert!(s.entry("nope").is_none());
        assert_eq!(Snapshot::parse("garbage"), None);
        assert_eq!(Snapshot::parse(r#"{"schema":99}"#), None);
        assert_eq!(Snapshot::parse("{}"), None, "an unversioned snapshot is not trusted");
        let top = SnapEntry {
            rel_dir: String::new(),
            ..s.entries[0].clone()
        };
        assert_eq!(top.key(), "a.zip");
    }
}
