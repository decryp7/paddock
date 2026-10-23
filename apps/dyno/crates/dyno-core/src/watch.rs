//! Watched folders with keep-for times (WP-10, ADR 0019). The user picks a folder and how long files may stay in it;
//! Dyno moves what has stayed longer to the 7-day quarantine (restorable from Activity), never anywhere else.
//!
//! Everything here is pure: what may be watched ([`validate_root`]), what a sweep would remove ([`plan`], admitted through
//! the same [`crate::cleanup::admit`] policy as cleanup, ADR 0005), the templates and the persisted list. The agent lists
//! the folder, calls these, re-checks each file and moves it.

use crate::cleanup::{admit, Candidate, CleanupRisk, Os};
use crate::time::{Timestamp, DAY, MINUTE};
use serde::{Deserialize, Serialize};

/// The shortest keep-for time (10 minutes), in seconds.
pub const MIN_KEEP_SECS: u32 = 10 * 60;
/// The longest keep-for time (10 days), in seconds.
pub const MAX_KEEP_SECS: u32 = 10 * 24 * 60 * 60;
/// The most folders watched at once.
pub const MAX_FOLDERS: usize = 20;
/// The most entries one sweep looks at in one folder.
pub const MAX_SCANNED: usize = 50_000;
/// The most files one sweep moves (the rest wait for the next sweep).
pub const MAX_PER_SWEEP: usize = 1_000;
/// The most files a preview lists.
pub const MAX_PREVIEW: usize = 200;
/// How deep a recursive folder is followed.
pub const MAX_DEPTH: usize = 8;
/// The persisted file's format version.
pub const FORMAT: u32 = 1;
/// The most characters in a filter word.
const MAX_WORD: usize = 64;
/// The most words in a filter list.
const MAX_WORDS: usize = 32;

/// Which files in a folder are subject to the keep-for time. Empty lists match every file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WatchFilter {
    /// File extensions without the dot, lower case (`dmg`, `png`).
    pub extensions: Vec<String>,
    /// File name beginnings, compared without regard to case (`Screenshot`).
    pub name_starts: Vec<String>,
}

impl WatchFilter {
    /// Whether a file with this name is subject to the keep-for time.
    pub fn matches(&self, name: &str) -> bool {
        if self.extensions.is_empty() && self.name_starts.is_empty() {
            return true;
        }
        let lower = name.to_lowercase();
        let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        self.extensions.iter().any(|e| !e.is_empty() && e == ext) || self.name_starts.iter().any(|p| lower.starts_with(&p.to_lowercase()))
    }

    /// The filter with every word trimmed, lower-cased where it should be, bounded, and duplicates removed.
    pub fn normalised(mut self) -> WatchFilter {
        let clean = |v: Vec<String>, lower: bool| -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            for w in v {
                let w = w.trim().trim_start_matches('.');
                let w = if lower { w.to_lowercase() } else { w.to_string() };
                let ok = !w.is_empty() && w.chars().count() <= MAX_WORD && !w.contains(['/', '\\']) && !w.chars().any(char::is_control);
                if ok && !out.contains(&w) && out.len() < MAX_WORDS {
                    out.push(w);
                }
            }
            out
        };
        self.extensions = clean(self.extensions, true);
        self.name_starts = clean(self.name_starts, false);
        self
    }

    /// A short description for the page ("dmg, pkg, exe" / "every file").
    pub fn describe(&self) -> String {
        let mut parts: Vec<String> = self.extensions.iter().map(|e| format!(".{e}")).collect();
        parts.extend(self.name_starts.iter().map(|p| format!("{p}…")));
        if parts.is_empty() {
            "every file".into()
        } else {
            parts.join(", ")
        }
    }
}

/// A ready-made starting point; the folder is still chosen by the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchTemplate {
    /// Stable id.
    pub id: &'static str,
    /// Name in the list.
    pub name: &'static str,
    /// What it does and which folder to pick.
    pub about: &'static str,
    /// Suggested keep-for time, seconds.
    pub keep_for_secs: u32,
    /// Whether subfolders are included.
    pub recursive: bool,
}

/// The templates. Their filters are in [`template_filter`] (kept apart so the list serialises as plain data).
pub const TEMPLATES: [WatchTemplate; 3] = [
    WatchTemplate {
        id: "installers",
        name: "Downloads: installers",
        about: "Disk images, installers and packages you downloaded. Pick your Downloads folder.",
        keep_for_secs: 7 * 24 * 3600,
        recursive: false,
    },
    WatchTemplate {
        id: "screenshots",
        name: "Screenshots",
        about: "Files that start like a screenshot. Pick the folder your screenshots go to.",
        keep_for_secs: 3 * 24 * 3600,
        recursive: false,
    },
    WatchTemplate {
        id: "build-output",
        name: "Build output",
        about: "Every file in a build or scratch folder, subfolders included. Pick the folder yourself, never a source folder.",
        keep_for_secs: 2 * 24 * 3600,
        recursive: true,
    },
];

/// The filter of a template id (`None` for an unknown id).
pub fn template_filter(id: &str) -> Option<WatchFilter> {
    let words = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    Some(match id {
        "installers" => WatchFilter {
            extensions: words(&["dmg", "pkg", "exe", "msi", "deb", "rpm", "appimage", "iso"]),
            name_starts: vec![],
        },
        "screenshots" => WatchFilter {
            extensions: vec![],
            name_starts: words(&["Screenshot", "Screen Shot", "Screen Recording", "Capture"]),
        },
        "build-output" => WatchFilter::default(),
        _ => return None,
    })
}

/// What the last sweep of a folder did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SweepSummary {
    /// When.
    pub at: Timestamp,
    /// Files moved to the quarantine.
    pub moved: u32,
    /// Their size in bytes.
    pub bytes: u64,
    /// Files that were due but could not be moved (in use, changed, refused), with the first reason.
    pub failed: u32,
    /// Why the first one failed.
    pub first_failure: Option<String>,
    /// The quarantine holding them (undo from Activity), when anything moved.
    pub quarantine_id: Option<String>,
}

/// A watched folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WatchedFolder {
    /// Stable id (`w-…`).
    pub id: String,
    /// The folder the user chose, as a canonical absolute path.
    pub path: String,
    /// How long a file may stay, seconds ([`MIN_KEEP_SECS`] to [`MAX_KEEP_SECS`]).
    pub keep_for_secs: u32,
    /// Which files are subject to it.
    pub filter: WatchFilter,
    /// Include subfolders (symbolic links are never followed).
    pub recursive: bool,
    /// Sweeping on. A new folder starts off; it can only be turned on after its preview was shown.
    pub enabled: bool,
    /// The preview of the current settings was shown (reset by every change of folder, time, filter or subfolders).
    pub previewed: bool,
    /// The template it started from, if any.
    pub template: Option<String>,
    /// When it was added.
    pub added: Timestamp,
    /// The last sweep.
    pub last_sweep: Option<SweepSummary>,
}

impl Default for WatchedFolder {
    fn default() -> Self {
        WatchedFolder {
            id: String::new(),
            path: String::new(),
            keep_for_secs: 7 * 24 * 3600,
            filter: WatchFilter::default(),
            recursive: false,
            enabled: false,
            previewed: false,
            template: None,
            added: Timestamp(0),
            last_sweep: None,
        }
    }
}

impl WatchedFolder {
    /// Seconds between sweeps of this folder: a tenth of the keep-for time, at least a minute, at most a quarter hour.
    pub fn sweep_every_ms(&self) -> i64 {
        (i64::from(self.keep_for_secs) * 100).clamp(MINUTE, 15 * MINUTE)
    }

    /// Whether a sweep is due.
    pub fn due(&self, now: Timestamp) -> bool {
        self.enabled && self.last_sweep.as_ref().is_none_or(|s| now.since(s.at) >= self.sweep_every_ms())
    }
}

/// The persisted list (`watched-folders.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WatchedFolders {
    /// Format version.
    pub version: u32,
    /// The folders.
    pub folders: Vec<WatchedFolder>,
}

impl Default for WatchedFolders {
    fn default() -> Self {
        WatchedFolders {
            version: FORMAT,
            folders: Vec::new(),
        }
    }
}

impl WatchedFolders {
    /// Reads the persisted text: damage or a newer format falls back to an empty list (nothing is watched), with a note.
    pub fn parse(text: &str) -> (WatchedFolders, Option<String>) {
        match serde_json::from_str::<WatchedFolders>(text) {
            Ok(w) if w.version > FORMAT => (
                WatchedFolders::default(),
                Some("The watched folders were saved by a newer Dyno; none are watched until you add them again.".into()),
            ),
            Ok(mut w) => {
                w.folders.truncate(MAX_FOLDERS);
                for f in &mut w.folders {
                    f.keep_for_secs = f.keep_for_secs.clamp(MIN_KEEP_SECS, MAX_KEEP_SECS);
                    f.filter = std::mem::take(&mut f.filter).normalised();
                }
                // A list that names a folder twice or an unsafe id keeps nothing ambiguous: drop duplicates.
                let mut seen = std::collections::HashSet::new();
                w.folders
                    .retain(|f| plain_id(&f.id) && !f.path.is_empty() && seen.insert(f.id.clone()));
                (w, None)
            }
            Err(e) => (
                WatchedFolders::default(),
                Some(format!(
                    "The watched folders file is damaged ({e}); none are watched until you add them again."
                )),
            ),
        }
    }

    /// The folder with this id.
    pub fn get(&self, id: &str) -> Option<&WatchedFolder> {
        self.folders.iter().find(|f| f.id == id)
    }

    /// The folder with this id, mutable.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut WatchedFolder> {
        self.folders.iter_mut().find(|f| f.id == id)
    }
}

/// Whether `id` is made only of what a file name and a quarantine id may hold.
pub fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Checks a keep-for time.
pub fn check_keep(secs: u32) -> Result<u32, String> {
    if (MIN_KEEP_SECS..=MAX_KEEP_SECS).contains(&secs) {
        Ok(secs)
    } else {
        Err("Choose a keep-for time between 10 minutes and 10 days.".into())
    }
}

fn norm(p: &str) -> String {
    let mut s = p.replace('\\', "/").to_lowercase();
    while s.ends_with('/') && s.len() > 1 {
        s.pop();
    }
    s
}

fn same_or_inside(path: &str, root: &str) -> bool {
    let (p, r) = (norm(path), norm(root));
    p == r || crate::cleanup::is_inside(&p, &r)
}

/// Folders that are never watched, per OS (system and application folders).
fn system_roots(os: Os) -> &'static [&'static str] {
    match os {
        Os::Macos => &[
            "/",
            "/system",
            "/library",
            "/applications",
            "/usr",
            "/bin",
            "/sbin",
            "/etc",
            "/var",
            "/private",
            "/opt",
            "/volumes",
            "/users",
        ],
        Os::Linux => &[
            "/", "/boot", "/etc", "/usr", "/bin", "/sbin", "/lib", "/lib64", "/var", "/opt", "/proc", "/sys", "/dev", "/home", "/root",
        ],
        Os::Windows => &[
            "c:",
            "c:/windows",
            "c:/program files",
            "c:/program files (x86)",
            "c:/programdata",
            "c:/users",
        ],
    }
}

/// Whether `path` may be watched. It must be an absolute, plain path to a folder that is not the user's home, an ancestor
/// of it, a system folder, the whole of a drive, or the folder Dyno keeps its own data in. `data` is Dyno's data folder.
pub fn validate_root(path: &str, home: &str, data: &str, os: Os) -> Result<(), String> {
    let p = norm(path);
    let absolute = p.starts_with('/') || (p.len() >= 2 && p.as_bytes()[1] == b':');
    if !absolute || p.split('/').any(|s| s == ".." || s == ".") {
        return Err("Choose a folder by its full path.".into());
    }
    if p == "/" || (p.len() == 2 && p.ends_with(':')) || (p.len() == 3 && p.ends_with(":/")) {
        return Err("A whole drive cannot be watched; choose a folder on it.".into());
    }
    let h = norm(home);
    if !h.is_empty() && (p == h || same_or_inside(&h, &p)) {
        return Err("Your home folder (or a folder that contains it) cannot be watched; choose a folder inside it.".into());
    }
    if system_roots(os).iter().any(|s| p == *s) {
        return Err("System folders cannot be watched.".into());
    }
    if os == Os::Windows && p.starts_with("c:/windows") {
        return Err("System folders cannot be watched.".into());
    }
    if os != Os::Windows
        && [
            "/system/",
            "/usr/",
            "/bin/",
            "/sbin/",
            "/etc/",
            "/library/",
            "/applications/",
            "/boot/",
            "/lib/",
        ]
        .iter()
        .any(|s| p.starts_with(s))
    {
        return Err("System folders cannot be watched.".into());
    }
    let d = norm(data);
    if !d.is_empty() && (same_or_inside(&p, &d) || same_or_inside(&d, &p)) {
        return Err("Dyno's own data folder cannot be watched.".into());
    }
    Ok(())
}

/// Whether two watched folders overlap (one is the other or inside it), which would sweep files twice.
pub fn overlaps(a: &str, b: &str) -> bool {
    same_or_inside(a, b) || same_or_inside(b, a)
}

/// One entry found in a watched folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundFile {
    /// Full path.
    pub path: String,
    /// File name.
    pub name: String,
    /// Size in bytes.
    pub bytes: u64,
    /// Last modified.
    pub modified: Timestamp,
    /// When it was created here, where the file system records it.
    pub created: Option<Timestamp>,
    /// A symbolic link, junction or reparse point.
    pub link: bool,
}

impl FoundFile {
    /// When the file arrived: the later of its creation and modification times, so a copy that kept an old modification
    /// time still gets its full keep-for time.
    pub fn arrived(&self) -> Timestamp {
        self.created.map_or(self.modified, |c| c.max(self.modified))
    }
}

/// A file a sweep would move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DueFile {
    /// Full path.
    pub path: String,
    /// Size in bytes.
    pub bytes: u64,
    /// When it arrived.
    pub arrived: Timestamp,
    /// When it was last modified (the re-check refuses a file that changed since).
    pub modified: Timestamp,
}

/// What a sweep of one folder would do right now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SweepPlan {
    /// The files that are due, oldest first, at most [`MAX_PER_SWEEP`].
    pub due: Vec<DueFile>,
    /// How many files are due in all (more than `due` when truncated).
    pub due_total: u32,
    /// Their total size.
    pub due_bytes: u64,
    /// Files the filter matched that have not yet stayed long enough.
    pub waiting: u32,
    /// Files the filter does not match.
    pub other: u32,
    /// Links that were passed over (never followed, never moved).
    pub links: u32,
    /// Files admitted by name but refused by the policy (outside the folder, in a personal folder).
    pub refused: u32,
    /// The folder had more entries than a sweep looks at.
    pub truncated: bool,
}

/// Names the OS and file managers keep in folders; they are never moved.
fn is_housekeeping(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        ".ds_store" | ".localized" | "desktop.ini" | "thumbs.db" | ".com.apple.timemachine.donotpresent"
    )
}

/// Names of downloads that are not finished; they are never moved, whatever their age.
fn is_partial(name: &str) -> bool {
    let l = name.to_lowercase();
    [".crdownload", ".part", ".download", ".partial", ".opdownload"]
        .iter()
        .any(|e| l.ends_with(e))
}

/// What a sweep of `folder` would move at `now`, given the files found in it. `home` is the user's home folder.
pub fn plan(folder: &WatchedFolder, found: &[FoundFile], home: &str, now: Timestamp) -> SweepPlan {
    let keep = i64::from(folder.keep_for_secs) * 1000;
    let roots = [folder.path.clone()];
    let mut out = SweepPlan {
        truncated: found.len() > MAX_SCANNED,
        ..SweepPlan::default()
    };
    let mut due: Vec<DueFile> = Vec::new();
    for f in found.iter().take(MAX_SCANNED) {
        if f.link {
            out.links += 1;
            continue;
        }
        if is_housekeeping(&f.name) || is_partial(&f.name) || !folder.filter.matches(&f.name) {
            out.other += 1;
            continue;
        }
        if now.since(f.arrived()) < keep {
            out.waiting += 1;
            continue;
        }
        let candidate = Candidate {
            path: f.path.clone(),
            bytes: f.bytes,
            modified: f.modified,
            folder: false,
            link: false,
            reason: None,
        };
        // The same admission as cleanup: inside the chosen folder, not a link, no `..`, and not in a personal folder unless
        // the chosen folder is itself in (or is) one. The keep-for time is checked above, so the policy's own age is 0.
        if admit(f.path.clone(), candidate, &roots, home, CleanupRisk::Review, 0, now, "").is_err() {
            out.refused += 1;
            continue;
        }
        due.push(DueFile {
            path: f.path.clone(),
            bytes: f.bytes,
            arrived: f.arrived(),
            modified: f.modified,
        });
    }
    due.sort_by(|a, b| a.arrived.cmp(&b.arrived).then_with(|| a.path.cmp(&b.path)));
    out.due_total = due.len() as u32;
    out.due_bytes = due.iter().map(|d| d.bytes).sum();
    due.truncate(MAX_PER_SWEEP);
    out.due = due;
    out
}

/// A folder as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolderView {
    /// The folder's settings and last sweep.
    pub folder: WatchedFolder,
    /// The folder exists now (a missing one is not swept).
    pub exists: bool,
    /// What the filter means in words.
    pub filter_text: String,
    /// Next sweep, when the folder is on.
    pub next_sweep: Option<Timestamp>,
}

/// The Watched folders page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchOverview {
    /// The folders.
    pub folders: Vec<WatchedFolderView>,
    /// The templates to start from.
    pub templates: Vec<WatchTemplateInfo>,
}

/// A template as the UI gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchTemplateInfo {
    /// Id.
    pub id: String,
    /// Name.
    pub name: String,
    /// About.
    pub about: String,
    /// Suggested keep-for time, seconds.
    pub keep_for_secs: u32,
    /// Includes subfolders.
    pub recursive: bool,
    /// What it matches, in words.
    pub filter_text: String,
}

/// The templates for the page.
pub fn template_infos() -> Vec<WatchTemplateInfo> {
    TEMPLATES
        .iter()
        .map(|t| WatchTemplateInfo {
            id: t.id.into(),
            name: t.name.into(),
            about: t.about.into(),
            keep_for_secs: t.keep_for_secs,
            recursive: t.recursive,
            filter_text: template_filter(t.id).unwrap_or_default().describe(),
        })
        .collect()
}

/// The preview a user sees before turning a folder on, and the answer to "Clean now".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchPreview {
    /// The folder.
    pub id: String,
    /// What a sweep would do now (the list is capped at [`MAX_PREVIEW`]; the totals are complete).
    pub plan: SweepPlan,
}

impl WatchPreview {
    /// A preview of `plan`, listing at most [`MAX_PREVIEW`] files.
    pub fn of(id: &str, mut plan: SweepPlan) -> WatchPreview {
        plan.due.truncate(MAX_PREVIEW);
        WatchPreview { id: id.into(), plan }
    }
}

/// A keep-for time in words ("10 minutes", "2 days").
pub fn keep_text(secs: u32) -> String {
    let (n, unit) = if secs >= 24 * 3600 && secs.is_multiple_of(24 * 3600) {
        (secs / (24 * 3600), "day")
    } else if secs >= 3600 && secs.is_multiple_of(3600) {
        (secs / 3600, "hour")
    } else {
        (secs.div_ceil(60), "minute")
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// Milliseconds in a day, re-exported for the agent's age maths.
pub const ONE_DAY_MS: i64 = DAY;

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/Users/me";

    fn folder(path: &str, keep: u32) -> WatchedFolder {
        WatchedFolder {
            id: "w-1".into(),
            path: path.into(),
            keep_for_secs: keep,
            ..Default::default()
        }
    }

    fn file(path: &str, age_ms: i64, now: Timestamp) -> FoundFile {
        let name = path.rsplit('/').next().unwrap().to_string();
        FoundFile {
            path: path.into(),
            name,
            bytes: 100,
            modified: now.minus(age_ms),
            created: None,
            link: false,
        }
    }

    #[test]
    fn keep_times_are_bounded() {
        assert!(check_keep(MIN_KEEP_SECS).is_ok());
        assert!(check_keep(MAX_KEEP_SECS).is_ok());
        assert!(check_keep(MIN_KEEP_SECS - 1).is_err());
        assert!(check_keep(MAX_KEEP_SECS + 1).is_err());
        assert_eq!(keep_text(600), "10 minutes");
        assert_eq!(keep_text(3600), "1 hour");
        assert_eq!(keep_text(2 * 86400), "2 days");
        assert_eq!(keep_text(90 * 60), "90 minutes");
    }

    #[test]
    fn filters_match_by_extension_or_name_start() {
        let f = WatchFilter {
            extensions: vec!["dmg".into()],
            name_starts: vec!["Screenshot".into()],
        };
        assert!(f.matches("App.DMG"));
        assert!(f.matches("screenshot 2026.png"));
        assert!(!f.matches("notes.txt"));
        assert!(!f.matches("dmg"));
        assert!(WatchFilter::default().matches("anything"));
        assert_eq!(f.describe(), ".dmg, Screenshot…");
        assert_eq!(WatchFilter::default().describe(), "every file");
    }

    #[test]
    fn filters_are_cleaned() {
        let f = WatchFilter {
            extensions: vec![" .PNG ".into(), "png".into(), "".into(), "a/b".into(), "x".repeat(65)],
            name_starts: vec!["Shot".into(), "Shot".into(), "bad\u{7}".into()],
        }
        .normalised();
        assert_eq!(f.extensions, vec!["png"]);
        assert_eq!(f.name_starts, vec!["Shot"]);
        let many = WatchFilter {
            extensions: (0..100).map(|i| format!("e{i}")).collect(),
            name_starts: vec![],
        }
        .normalised();
        assert_eq!(many.extensions.len(), MAX_WORDS);
    }

    #[test]
    fn every_template_has_a_filter_and_a_valid_time() {
        for t in &TEMPLATES {
            assert!(template_filter(t.id).is_some(), "{}", t.id);
            assert!(check_keep(t.keep_for_secs).is_ok(), "{}", t.id);
        }
        assert!(template_filter("nope").is_none());
        assert_eq!(template_infos().len(), TEMPLATES.len());
        assert_eq!(template_infos()[2].filter_text, "every file");
    }

    #[test]
    fn roots_that_are_unsafe_are_refused() {
        let ok = |p: &str| validate_root(p, HOME, "/Users/me/Library/Application Support/Dyno", Os::Macos);
        assert!(ok("/Users/me/Downloads").is_ok());
        assert!(ok("/Users/me/Library/Application Support/Other").is_ok());
        for bad in [
            "/",
            "/Users/me",
            "/Users/me/",
            "/Users",
            "/System/Library",
            "/usr/local",
            "/Applications",
            "relative/folder",
            "/Users/me/Downloads/../Documents",
            "/Users/me/Library/Application Support/Dyno",
            "/Users/me/Library/Application Support/Dyno/quarantine",
            "/Users/me/Library/Application Support",
        ] {
            assert!(ok(bad).is_err(), "{bad}");
        }
        let win = |p: &str| validate_root(p, "C:\\Users\\me", "C:\\Users\\me\\AppData\\Local\\Dyno", Os::Windows);
        assert!(win("C:\\Users\\me\\Downloads").is_ok());
        for bad in [
            "C:\\",
            "C:",
            "C:\\Windows",
            "C:\\Windows\\Temp",
            "C:\\Users\\me",
            "C:\\Users",
            "C:\\Program Files",
            "D:",
        ] {
            assert!(win(bad).is_err(), "{bad}");
        }
        assert!(validate_root("/home/me/dl", "/home/me", "/home/me/.local/share/dyno", Os::Linux).is_ok());
        assert!(validate_root("/home/me", "/home/me", "", Os::Linux).is_err());
        assert!(validate_root("/etc/cron.d", "/home/me", "", Os::Linux).is_err());
        assert!(validate_root("/home", "/home/me", "", Os::Linux).is_err());
    }

    #[test]
    fn overlapping_folders_are_detected() {
        assert!(overlaps("/a/b", "/a/b"));
        assert!(overlaps("/a/b", "/a/b/c"));
        assert!(overlaps("/a/b/c", "/a/b"));
        assert!(overlaps("/A/B", "/a/b/"));
        assert!(!overlaps("/a/b", "/a/bc"));
    }

    #[test]
    fn only_files_that_stayed_long_enough_are_due() {
        let now = Timestamp(1_000_000_000_000);
        let f = folder("/Users/me/Downloads", 3600);
        let files = vec![
            file("/Users/me/Downloads/old.dmg", 2 * 3_600_000, now),
            file("/Users/me/Downloads/new.dmg", 600_000, now),
            file("/Users/me/Downloads/older.dmg", 5 * 3_600_000, now),
            file("/Users/me/Downloads/.DS_Store", 9 * 3_600_000, now),
            file("/Users/me/Downloads/movie.mp4.crdownload", 9 * 3_600_000, now),
        ];
        let p = plan(&f, &files, HOME, now);
        assert_eq!(
            p.due.iter().map(|d| d.path.as_str()).collect::<Vec<_>>(),
            ["/Users/me/Downloads/older.dmg", "/Users/me/Downloads/old.dmg"]
        );
        assert_eq!(
            (p.due_total, p.due_bytes, p.waiting, p.other, p.links, p.refused),
            (2, 200, 1, 2, 0, 0)
        );
    }

    #[test]
    fn the_exact_boundary_is_due_and_a_copy_keeps_its_full_time() {
        let now = Timestamp(1_000_000_000_000);
        let f = folder("/Users/me/Downloads", 600);
        let at_boundary = file("/Users/me/Downloads/a.bin", 600_000, now);
        assert_eq!(plan(&f, &[at_boundary], HOME, now).due_total, 1);
        let just_under = file("/Users/me/Downloads/a.bin", 599_999, now);
        assert_eq!(plan(&f, &[just_under], HOME, now).due_total, 0);
        // Old modification time but created a minute ago: it arrived a minute ago.
        let mut copy = file("/Users/me/Downloads/c.bin", 30 * 86_400_000, now);
        copy.created = Some(now.minus(60_000));
        assert_eq!(copy.arrived(), now.minus(60_000));
        assert_eq!(plan(&f, &[copy], HOME, now).waiting, 1);
    }

    #[test]
    fn links_are_never_planned() {
        let now = Timestamp(1_000_000_000_000);
        let mut l = file("/Users/me/Downloads/link", 9 * 86_400_000, now);
        l.link = true;
        let p = plan(&folder("/Users/me/Downloads", 600), &[l], HOME, now);
        assert_eq!((p.due_total, p.links), (0, 1));
    }

    #[test]
    fn nothing_outside_the_chosen_folder_is_planned() {
        let now = Timestamp(1_000_000_000_000);
        let f = folder("/Users/me/Downloads", 600);
        let files = vec![
            file("/Users/me/Documents/secret.pdf", 9 * 86_400_000, now),
            file("/Users/me/Downloads-old/x.bin", 9 * 86_400_000, now),
            file("/Users/me/Downloads/../Documents/y.bin", 9 * 86_400_000, now),
            file("/Users/me/Downloads/ok.bin", 9 * 86_400_000, now),
        ];
        let p = plan(&f, &files, HOME, now);
        assert_eq!(p.due.len(), 1);
        assert_eq!(p.refused, 3);
    }

    #[test]
    fn a_personal_folder_below_the_chosen_one_is_refused_but_a_chosen_personal_folder_works() {
        let now = Timestamp(1_000_000_000_000);
        // The user chose a folder that contains Documents (not allowed as a root, but the policy still protects it).
        let outer = folder("/Users/me/work", 600);
        let inner = file("/Users/me/work/x.bin", 9 * 86_400_000, now);
        assert_eq!(plan(&outer, &[inner], HOME, now).due_total, 1);
        // The user chose a folder inside Documents on purpose.
        let chosen = folder("/Users/me/Documents/Scans", 600);
        let f = file("/Users/me/Documents/Scans/a.pdf", 9 * 86_400_000, now);
        assert_eq!(plan(&chosen, &[f], HOME, now).due_total, 1);
    }

    #[test]
    fn a_sweep_is_capped_and_says_so() {
        let now = Timestamp(1_000_000_000_000);
        let f = folder("/Users/me/Downloads", 600);
        let files: Vec<FoundFile> = (0..MAX_PER_SWEEP + 50)
            .map(|i| file(&format!("/Users/me/Downloads/f{i:05}.bin"), 9 * 86_400_000 + i as i64, now))
            .collect();
        let p = plan(&f, &files, HOME, now);
        assert_eq!(p.due.len(), MAX_PER_SWEEP);
        assert_eq!(p.due_total as usize, MAX_PER_SWEEP + 50);
        let prev = WatchPreview::of("w-1", p);
        assert_eq!(prev.plan.due.len(), MAX_PREVIEW);
        assert_eq!(prev.plan.due_total as usize, MAX_PER_SWEEP + 50);
    }

    #[test]
    fn the_schedule_follows_the_keep_time() {
        let mut f = folder("/a/b", MIN_KEEP_SECS);
        assert_eq!(f.sweep_every_ms(), MINUTE);
        f.keep_for_secs = MAX_KEEP_SECS;
        assert_eq!(f.sweep_every_ms(), 15 * MINUTE);
        let now = Timestamp(10_000_000);
        assert!(!f.due(now), "off folders are never due");
        f.enabled = true;
        assert!(f.due(now));
        f.last_sweep = Some(SweepSummary {
            at: now.minus(MINUTE),
            moved: 0,
            bytes: 0,
            failed: 0,
            first_failure: None,
            quarantine_id: None,
        });
        assert!(!f.due(now));
        assert!(f.due(now.plus(15 * MINUTE)));
    }

    #[test]
    fn the_list_survives_damage_and_clamps_old_values() {
        let (w, note) = WatchedFolders::parse("{nope");
        assert!(w.folders.is_empty() && note.unwrap().contains("damaged"));
        let (w, note) = WatchedFolders::parse(r#"{"version":99,"folders":[]}"#);
        assert!(w.folders.is_empty() && note.unwrap().contains("newer"));
        let (w, note) = WatchedFolders::parse(
            r#"{"version":1,"folders":[
              {"id":"w-1","path":"/a/b","keepForSecs":5,"filter":{"extensions":["PNG"]}},
              {"id":"w-1","path":"/a/c"},
              {"id":"../x","path":"/a/d"},
              {"id":"w-2","path":""}
            ]}"#,
        );
        assert!(note.is_none());
        assert_eq!(w.folders.len(), 1);
        assert_eq!(w.folders[0].keep_for_secs, MIN_KEEP_SECS);
        assert_eq!(w.folders[0].filter.extensions, vec!["png"]);
        assert!(!w.folders[0].enabled, "a folder is off unless the file says otherwise");
        assert!(w.get("w-1").is_some() && w.get("w-9").is_none());
        let json = serde_json::to_string(&w).unwrap();
        assert_eq!(WatchedFolders::parse(&json).0, w);
    }

    #[test]
    fn ids_are_plain() {
        assert!(plain_id("w-123_a"));
        for bad in ["", "a/b", "..", "a b", &"x".repeat(65)] {
            assert!(!plain_id(bad), "{bad}");
        }
    }
}
