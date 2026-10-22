//! Cleanup safety (ADR 0005): what may be offered, what is pre-selected, and what a run may delete.
//!
//! The agent (or a plugin) proposes [`Candidate`]s; only those that pass [`admit`] reach the preview, and a run
//! deletes only items of the scan the user saw, re-checked by the agent before each delete.

use super::catalog::CleanupRisk;
use crate::time::{Timestamp, DAY, HOUR};
use serde::{Deserialize, Serialize};

/// Something a scan found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// Full path.
    pub path: String,
    /// Size (a folder's total).
    pub bytes: u64,
    /// Last modified (a folder's newest entry).
    pub modified: Timestamp,
    /// A folder.
    pub folder: bool,
    /// A symbolic link, junction or reparse point (never followed, never offered).
    pub link: bool,
    /// Why it is cleanable, when the scanner knows better than the category ("Rust build output, idle 42 days").
    pub reason: Option<String>,
}

/// One item of the preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupItem {
    /// Id (unique in its scan).
    pub id: String,
    /// Full path.
    pub path: String,
    /// Size.
    pub bytes: u64,
    /// Last modified.
    pub modified: Timestamp,
    /// A folder.
    pub folder: bool,
    /// Why it is offered.
    pub reason: String,
    /// Pre-selected (only for safe categories).
    pub selected: bool,
}

/// A category's part of a scan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryResult {
    /// Category id.
    pub category: String,
    /// Total bytes of all admitted items (also those beyond the list cap).
    pub bytes: u64,
    /// Number of admitted items.
    pub item_count: usize,
    /// The biggest items, at most [`MAX_ITEMS_PER_CATEGORY`].
    pub items: Vec<CleanupItem>,
    /// More items exist than listed (a run deletes only listed items).
    pub truncated: bool,
    /// Why the category could not be scanned fully.
    pub note: Option<String>,
}

/// A whole scan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupScan {
    /// Id.
    pub id: String,
    /// When.
    pub at: Timestamp,
    /// Per category.
    pub results: Vec<CategoryResult>,
    /// Sum.
    pub total_bytes: u64,
}

/// Where deleted items go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeleteMode {
    /// The OS's Recycle Bin / Trash.
    RecycleBin,
    /// Dyno's quarantine folder for 7 days, restorable from Activity.
    Quarantine,
}

/// An item a run could not delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedItem {
    /// Path.
    pub path: String,
    /// Why ("in use by Teams", "access denied: needs the helper").
    pub reason: String,
}

/// What a run did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOutcome {
    /// Bytes moved out of the scanned folders. The disk space is released when the quarantine expires (7 days) or the
    /// Recycle Bin is emptied.
    pub freed_bytes: u64,
    /// Items deleted.
    pub deleted: usize,
    /// Items that could not be deleted.
    pub failed: Vec<FailedItem>,
    /// The quarantine holding them, if that mode was used.
    pub quarantine_id: Option<String>,
}

/// A quarantine as the UI lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineEntry {
    /// Id.
    pub id: String,
    /// When.
    pub at: Timestamp,
    /// Bytes held.
    pub bytes: u64,
    /// Items held.
    pub item_count: usize,
    /// When it is deleted for good.
    pub expires: Timestamp,
}

/// A folder's size with its biggest children (the disk-space treemap).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeNode {
    /// Name.
    pub name: String,
    /// Full path.
    pub path: String,
    /// Bytes, including children.
    pub bytes: u64,
    /// Biggest children.
    pub children: Vec<SizeNode>,
}

/// The most items a category lists.
pub const MAX_ITEMS_PER_CATEGORY: usize = 1_000;
/// A scan older than this must be repeated before a run.
pub const SCAN_VALID_MS: i64 = HOUR;

/// Folders in the user's profile that hold personal files. Nothing under them is offered, except by a category whose
/// root is inside one (Downloads).
pub const PERSONAL_DIRS: &[&str] = &[
    "Documents",
    "Desktop",
    "Pictures",
    "Music",
    "Videos",
    "Movies",
    "OneDrive",
    "iCloud Drive",
    "Library/Mobile Documents",
    ".ssh",
    ".gnupg",
    "Favorites",
    "Contacts",
];

fn norm(p: &str) -> String {
    let mut s = p.replace('\\', "/").to_lowercase();
    while s.ends_with('/') && s.len() > 1 {
        s.pop();
    }
    s
}

/// Whether `path` has a `.` or `..` segment. `Path::components` reads `..` as a parent step only on ordinary paths: in a Windows
/// verbatim path (`\\?\C:\..`) it is a plain name, so this checks the names too and refuses both forms.
pub fn has_dot_segment(path: &std::path::Path) -> bool {
    use std::path::Component;
    path.components().any(|c| match c {
        Component::ParentDir | Component::CurDir => true,
        Component::Normal(n) => n == ".." || n == ".",
        _ => false,
    })
}

/// Whether `path` lies strictly inside `root`.
pub fn is_inside(path: &str, root: &str) -> bool {
    let (p, r) = (norm(path), norm(root));
    !r.is_empty() && p.len() > r.len() + 1 && p.starts_with(&r) && p.as_bytes()[r.len()] == b'/'
}

/// Checks a candidate against the category's resolved roots and the safety rules, and turns it into a preview item.
#[allow(clippy::too_many_arguments)]
pub fn admit(
    id: String,
    c: Candidate,
    roots: &[String],
    home: &str,
    risk: CleanupRisk,
    min_age_days: u32,
    now: Timestamp,
    default_reason: &str,
) -> Result<CleanupItem, String> {
    if c.link {
        return Err(format!("{} is a link; Dyno never follows or deletes links.", c.path));
    }
    if c.path.replace('\\', "/").split('/').any(|seg| seg == "..") {
        return Err(format!("{} is not a plain path.", c.path));
    }
    let Some(root) = roots.iter().find(|r| is_inside(&c.path, r)) else {
        return Err(format!("{} is outside the category's folders.", c.path));
    };
    let personal = PERSONAL_DIRS.iter().map(|d| format!("{}/{}", norm(home), d.to_lowercase()));
    for dir in personal {
        if is_inside(&c.path, &dir) && !(is_inside(root, &dir) || norm(root) == dir) {
            return Err(format!("{} is in a personal folder.", c.path));
        }
    }
    if min_age_days > 0 && now.since(c.modified) < i64::from(min_age_days) * DAY {
        return Err(format!("{} changed in the last {min_age_days} days.", c.path));
    }
    Ok(CleanupItem {
        id,
        path: c.path,
        bytes: c.bytes,
        modified: c.modified,
        folder: c.folder,
        reason: c.reason.unwrap_or_else(|| default_reason.to_string()),
        selected: risk == CleanupRisk::Safe,
    })
}

/// A category's result from its admitted items (biggest first, capped).
pub fn result(category: &str, mut items: Vec<CleanupItem>, note: Option<String>) -> CategoryResult {
    items.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.path.cmp(&b.path)));
    let bytes = items.iter().map(|i| i.bytes).sum();
    let item_count = items.len();
    let truncated = items.len() > MAX_ITEMS_PER_CATEGORY;
    items.truncate(MAX_ITEMS_PER_CATEGORY);
    CategoryResult {
        category: category.into(),
        bytes,
        item_count,
        items,
        truncated,
        note,
    }
}

/// The items of `scan` the user chose, checked: the scan must be recent and every id must be in it.
pub fn chosen<'a>(scan: &'a CleanupScan, ids: &[String], now: Timestamp) -> Result<Vec<(&'a str, &'a CleanupItem)>, String> {
    if now.since(scan.at) > SCAN_VALID_MS {
        return Err("This scan is more than an hour old; scan again before cleaning.".into());
    }
    if ids.is_empty() {
        return Err("Select something to clean.".into());
    }
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let hit = scan
            .results
            .iter()
            .find_map(|r| r.items.iter().find(|i| &i.id == id).map(|i| (r.category.as_str(), i)));
        match hit {
            Some(h) => out.push(h),
            None => return Err(format!("Item {id} is not part of this scan.")),
        }
    }
    Ok(out)
}

/// Whether a file name is an installer.
pub fn is_installer(name: &str) -> bool {
    let n = name.to_lowercase();
    [".exe", ".msi", ".msix", ".msixbundle", ".appx", ".dmg", ".pkg"]
        .iter()
        .any(|e| n.ends_with(e))
}

/// A project's build output folder, if it is old enough to offer. `markers` are the file names in the project
/// folder; `newest_source` is the newest modification of anything in it outside the artifact folder.
pub fn dev_artifact(artifact: &str, markers: &[String], newest_source: Timestamp, now: Timestamp, age_days: u32) -> Option<String> {
    let has = |m: &str| markers.iter().any(|x| x.eq_ignore_ascii_case(m));
    let kind = match artifact {
        "target" if has("Cargo.toml") => "Rust build output",
        "node_modules" if has("package.json") => "npm packages",
        _ => return None,
    };
    let idle = now.since(newest_source) / DAY;
    (idle >= i64::from(age_days)).then(|| format!("{kind}; project untouched for {idle} days"))
}

/// Installed Rust toolchains that neither the default nor an override uses.
pub fn stale_toolchains(installed: &[String], default: Option<&str>, overrides: &[String]) -> Vec<String> {
    let used = |t: &str| {
        default.is_some_and(|d| t == d || t.starts_with(&format!("{d}-")))
            || overrides.iter().any(|o| t == o || t.starts_with(&format!("{o}-")))
    };
    installed.iter().filter(|t| !used(t)).cloned().collect()
}

/// Bounds a size tree for the UI: at most `per_level` children per node (the rest folded into "Other") and `depth`
/// levels.
pub fn prune(mut node: SizeNode, per_level: usize, depth: usize) -> SizeNode {
    node.children.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    if depth == 0 {
        node.children.clear();
        return node;
    }
    if node.children.len() > per_level {
        let rest: u64 = node.children[per_level..].iter().map(|c| c.bytes).sum();
        let count = node.children.len() - per_level;
        node.children.truncate(per_level);
        node.children.push(SizeNode {
            name: format!("{count} other items"),
            path: String::new(),
            bytes: rest,
            children: vec![],
        });
    }
    node.children = node.children.into_iter().map(|c| prune(c, per_level, depth - 1)).collect();
    node
}

#[cfg(test)]
mod tests {
    #[test]
    fn dot_segments_are_found_in_any_spelling() {
        use std::path::Path;
        assert!(has_dot_segment(Path::new("/a/../b")));
        assert!(!has_dot_segment(Path::new("/a/b..c/.hidden/..x")));
    }

    use super::*;

    fn cand(path: &str, days_old: i64) -> Candidate {
        Candidate {
            path: path.into(),
            bytes: 10,
            modified: Timestamp(100 * DAY - days_old * DAY),
            folder: false,
            link: false,
            reason: None,
        }
    }

    const NOW: Timestamp = Timestamp(100 * DAY);

    #[test]
    fn inside_means_strictly_inside() {
        assert!(is_inside(
            "C:\\Users\\me\\AppData\\Local\\Temp\\x.tmp",
            "c:/users/me/appdata/local/temp/"
        ));
        assert!(!is_inside("C:/Temp", "C:/Temp"));
        assert!(!is_inside("C:/Temp2/x", "C:/Temp"));
        assert!(!is_inside("C:/x", ""));
    }

    #[test]
    fn admission_rules() {
        let roots = vec!["C:/Users/me/AppData/Local/Temp".to_string()];
        let home = "C:/Users/me";
        let ok = admit(
            "1".into(),
            cand("C:/Users/me/AppData/Local/Temp/a.tmp", 5),
            &roots,
            home,
            CleanupRisk::Safe,
            2,
            NOW,
            "Temp",
        )
        .unwrap();
        assert!(ok.selected);
        assert_eq!(ok.reason, "Temp");
        let review = admit(
            "2".into(),
            cand("C:/Users/me/AppData/Local/Temp/b", 5),
            &roots,
            home,
            CleanupRisk::Review,
            0,
            NOW,
            "x",
        )
        .unwrap();
        assert!(!review.selected);
        let mut link = cand("C:/Users/me/AppData/Local/Temp/l", 5);
        link.link = true;
        assert!(admit("3".into(), link, &roots, home, CleanupRisk::Safe, 0, NOW, "")
            .unwrap_err()
            .contains("link"));
        assert!(admit(
            "4".into(),
            cand("C:/Users/me/AppData/Local/Temp/../../x", 5),
            &roots,
            home,
            CleanupRisk::Safe,
            0,
            NOW,
            ""
        )
        .is_err());
        assert!(admit(
            "5".into(),
            cand("C:/Windows/System32/x", 5),
            &roots,
            home,
            CleanupRisk::Safe,
            0,
            NOW,
            ""
        )
        .unwrap_err()
        .contains("outside"));
        assert!(admit(
            "6".into(),
            cand("C:/Users/me/AppData/Local/Temp/new", 1),
            &roots,
            home,
            CleanupRisk::Safe,
            2,
            NOW,
            ""
        )
        .unwrap_err()
        .contains("changed"));
        // A root that reaches into a personal folder can't offer what's in it…
        let wide = vec!["C:/Users/me".to_string()];
        assert!(admit(
            "7".into(),
            cand("C:/Users/me/Documents/cv.docx", 90),
            &wide,
            home,
            CleanupRisk::Safe,
            0,
            NOW,
            ""
        )
        .unwrap_err()
        .contains("personal"));
        // …but the Downloads category's root may be one.
        let dl = vec!["C:/Users/me/Downloads".to_string()];
        let mut c = cand("C:/Users/me/Downloads/setup.exe", 90);
        c.reason = Some("Installer".into());
        let item = admit("8".into(), c, &dl, home, CleanupRisk::Personal, 30, NOW, "").unwrap();
        assert!(!item.selected, "personal items are never pre-selected");
        assert_eq!(item.reason, "Installer");
        let docs = vec!["C:/Users/me/Documents".to_string()];
        assert!(admit(
            "9".into(),
            cand("C:/Users/me/Documents/a", 90),
            &docs,
            home,
            CleanupRisk::Personal,
            0,
            NOW,
            ""
        )
        .is_ok());
    }

    fn item(id: &str, bytes: u64) -> CleanupItem {
        CleanupItem {
            id: id.into(),
            path: format!("/t/{id}"),
            bytes,
            modified: Timestamp(0),
            folder: false,
            reason: String::new(),
            selected: true,
        }
    }

    #[test]
    fn results_are_sorted_and_capped() {
        let r = result("c", vec![item("a", 1), item("b", 5)], None);
        assert_eq!(r.items[0].id, "b");
        assert_eq!(r.bytes, 6);
        assert!(!r.truncated);
        let many = (0..MAX_ITEMS_PER_CATEGORY + 5).map(|i| item(&i.to_string(), 1)).collect();
        let r = result("c", many, Some("partial".into()));
        assert!(r.truncated);
        assert_eq!(r.item_count, MAX_ITEMS_PER_CATEGORY + 5);
        assert_eq!(r.items.len(), MAX_ITEMS_PER_CATEGORY);
        assert_eq!(r.bytes, (MAX_ITEMS_PER_CATEGORY + 5) as u64);
    }

    #[test]
    fn runs_take_only_items_of_a_recent_scan() {
        let scan = CleanupScan {
            id: "s".into(),
            at: Timestamp(0),
            results: vec![result("temp", vec![item("a", 1), item("b", 2)], None)],
            total_bytes: 3,
        };
        let got = chosen(&scan, &["b".into()], Timestamp(1)).unwrap();
        assert_eq!(got[0].0, "temp");
        assert_eq!(got[0].1.id, "b");
        assert!(chosen(&scan, &["zzz".into()], Timestamp(1)).unwrap_err().contains("not part"));
        assert!(chosen(&scan, &[], Timestamp(1)).is_err());
        assert!(chosen(&scan, &["a".into()], Timestamp(2 * HOUR)).unwrap_err().contains("old"));
    }

    #[test]
    fn developer_rules() {
        let markers = vec!["cargo.toml".to_string()];
        assert_eq!(
            dev_artifact("target", &markers, Timestamp(60 * DAY), NOW, 30).as_deref(),
            Some("Rust build output; project untouched for 40 days")
        );
        assert!(dev_artifact("target", &markers, Timestamp(90 * DAY), NOW, 30).is_none());
        assert!(dev_artifact("node_modules", &markers, Timestamp(0), NOW, 30).is_none());
        assert!(dev_artifact("node_modules", &["package.json".into()], Timestamp(0), NOW, 30)
            .unwrap()
            .starts_with("npm"));
        assert!(dev_artifact("bin", &markers, Timestamp(0), NOW, 30).is_none());
        let installed: Vec<String> = [
            "stable-x86_64-pc-windows-msvc",
            "nightly-2025-01-01-x86_64-pc-windows-msvc",
            "1.80.0-x86_64-pc-windows-msvc",
            "1.85.0-x86_64-pc-windows-msvc",
        ]
        .map(String::from)
        .to_vec();
        let stale = stale_toolchains(&installed, Some("stable-x86_64-pc-windows-msvc"), &["1.85.0".into()]);
        assert_eq!(stale, vec![installed[1].clone(), installed[2].clone()]);
        assert_eq!(stale_toolchains(&installed, Some("stable"), &[]).len(), 3);
        assert_eq!(stale_toolchains(&installed, None, &[]).len(), 4);
        assert!(is_installer("Setup.EXE") && is_installer("x.dmg") && !is_installer("photo.jpg"));
    }

    #[test]
    fn size_trees_are_bounded() {
        let leaf = |n: &str, b| SizeNode {
            name: n.into(),
            path: format!("/{n}"),
            bytes: b,
            children: vec![],
        };
        let mut deep = leaf("a", 100);
        deep.children = vec![leaf("x", 60), leaf("y", 30), leaf("z", 10)];
        let mut root = leaf("root", 200);
        root.children = vec![leaf("b", 50), deep, leaf("c", 40), leaf("d", 10)];
        let p = prune(root, 2, 1);
        assert_eq!(p.children.len(), 3);
        assert_eq!(p.children[0].name, "a");
        assert!(p.children[0].children.is_empty(), "depth 1");
        assert_eq!(p.children[2].name, "2 other items");
        assert_eq!(p.children[2].bytes, 50);
    }
}
