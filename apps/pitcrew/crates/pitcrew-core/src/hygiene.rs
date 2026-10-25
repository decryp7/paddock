//! Container engine hygiene and the Actions cache (ADR 0016): what a runner machine's Docker or Podman holds, what a
//! prune would remove, and the same for the runner's cache folder.
//!
//! This is the pure part: parsing the engine's text output, sizes, previews and the cache cleanup plan. The service runs
//! the engine's commands (with timeouts) and measures folders. Nothing is removed without an explicit request that
//! carries a selection, and a preview (a query) always comes first in the UI.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// Most rows of examples kept in a preview.
pub const MAX_EXAMPLES: usize = 8;
/// Longest age filter accepted, in days.
pub const MAX_PRUNE_DAYS: u32 = 3650;
/// Most lines of engine output parsed.
pub const MAX_LINES: usize = 20_000;

/// Which engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineKind {
    /// Docker (or anything with its CLI).
    Docker,
    /// Podman.
    Podman,
}

impl EngineKind {
    /// The program name.
    pub fn program(self) -> &'static str {
        match self {
            EngineKind::Docker => "docker",
            EngineKind::Podman => "podman",
        }
    }
}

/// What can be pruned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PruneKind {
    /// Stopped containers.
    StoppedContainers,
    /// Images without a name (left over from rebuilt tags).
    DanglingImages,
    /// Images no container uses, older than the age filter.
    UnusedImages,
    /// The builder's cache, older than the age filter.
    BuildCache,
    /// Volumes no container uses.
    Volumes,
}

impl PruneKind {
    /// Every kind, in the order the UI lists them.
    pub const ALL: [PruneKind; 5] = [
        PruneKind::StoppedContainers,
        PruneKind::DanglingImages,
        PruneKind::UnusedImages,
        PruneKind::BuildCache,
        PruneKind::Volumes,
    ];
}

/// What the owner chose to prune.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneSelection {
    /// The kinds (each once).
    pub kinds: Vec<PruneKind>,
    /// Only things older than this many days (0: any age). Does not apply to dangling images and volumes.
    pub older_than_days: u32,
}

impl PruneSelection {
    /// Checks the selection (not empty, no duplicates, a sane age).
    pub fn validate(&self) -> Result<(), String> {
        if self.kinds.is_empty() {
            return Err("choose what to prune".into());
        }
        let mut seen = self.kinds.clone();
        seen.sort();
        seen.dedup();
        if seen.len() != self.kinds.len() {
            return Err("each kind may be chosen once".into());
        }
        if self.older_than_days > MAX_PRUNE_DAYS {
            return Err(format!("the age filter is at most {MAX_PRUNE_DAYS} days"));
        }
        Ok(())
    }

    /// The `--filter until=` value, `None` for any age.
    pub fn until(&self) -> Option<String> {
        (self.older_than_days > 0).then(|| format!("{}h", u64::from(self.older_than_days) * 24))
    }
}

/// One category's size in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineUse {
    /// What (`Images`, `Containers`, `Local Volumes`, `Build Cache`).
    pub what: String,
    /// How many.
    pub total: u32,
    /// How many are in use.
    pub active: u32,
    /// Disk used.
    pub bytes: u64,
    /// What a prune could give back.
    pub reclaimable_bytes: u64,
}

/// A folder's size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderUse {
    /// What it is for (`Action checkouts (act)`).
    pub label: String,
    /// Its path.
    pub path: String,
    /// Whether it exists.
    pub exists: bool,
    /// Its size (lower bound when `truncated`).
    pub bytes: u64,
    /// Entries directly inside it.
    pub entries: u32,
    /// The measuring stopped at a limit.
    pub truncated: bool,
}

/// The report: the engine, its disk use and the act cache folders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineReport {
    /// The engine found, if any.
    pub engine: Option<EngineKind>,
    /// Why there is no engine data (not installed, daemon not running), with a next step.
    pub problem: Option<String>,
    /// The engine's disk use.
    pub usage: Vec<EngineUse>,
    /// The act cache folders.
    pub folders: Vec<FolderUse>,
    /// When it was measured.
    pub at: Timestamp,
}

/// One kind in a preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewItem {
    /// The kind.
    pub kind: PruneKind,
    /// How many things would go.
    pub count: u32,
    /// How much space they hold (0 when the engine does not say).
    pub bytes: u64,
    /// Some of their names.
    pub examples: Vec<String>,
}

/// What a prune would remove (a dry run).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrunePreview {
    /// Per kind.
    pub items: Vec<PreviewItem>,
    /// Sum of the sizes known.
    pub total_bytes: u64,
}

/// What a prune did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneResult {
    /// Per kind: bytes the engine reported as reclaimed.
    pub freed: Vec<(PruneKind, u64)>,
    /// Kinds that failed, with the engine's message.
    pub failures: Vec<(PruneKind, String)>,
    /// When.
    pub at: Timestamp,
}

/// Parses `1.5GB`, `512MB`, `12kB`, `0B`, `1.2 GiB` (decimal for `kB/MB/GB/TB`, binary for `KiB..`); `None` when it
/// is not a size. The engines print rounded decimal sizes, so this is approximate by nature.
pub fn parse_size(text: &str) -> Option<u64> {
    let t = text.trim();
    let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let value: f64 = num.parse().ok()?;
    let mult: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        "tib" => 1024.0_f64.powi(4),
        _ => return None,
    };
    let bytes = value * mult;
    (bytes.is_finite() && (0.0..9e18).contains(&bytes)).then_some(bytes as u64)
}

/// Parses `docker system df --format '{{.Type}}\t{{.TotalCount}}\t{{.Active}}\t{{.Size}}\t{{.Reclaimable}}'`. A
/// reclaimable column looks like `1.2GB (45%)`. Lines that do not fit are skipped.
pub fn parse_df(output: &str) -> Vec<EngineUse> {
    output
        .lines()
        .take(MAX_LINES)
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            if c.len() < 5 {
                return None;
            }
            let reclaim = c[4].split(" (").next().unwrap_or("");
            Some(EngineUse {
                what: c[0].trim().to_string(),
                total: c[1].trim().parse().ok()?,
                active: c[2].trim().parse().ok()?,
                bytes: parse_size(c[3])?,
                reclaimable_bytes: parse_size(reclaim).unwrap_or(0),
            })
        })
        .collect()
}

/// An image from `images --format '{{.ID}}\t{{.Repository}}:{{.Tag}}\t{{.Size}}'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRow {
    /// Its id (short).
    pub id: String,
    /// `repository:tag` (`<none>:<none>` when unnamed).
    pub name: String,
    /// Its size.
    pub bytes: u64,
}

/// Parses the image list.
pub fn parse_images(output: &str) -> Vec<ImageRow> {
    output
        .lines()
        .take(MAX_LINES)
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            (c.len() >= 3 && !c[0].trim().is_empty()).then(|| ImageRow {
                id: c[0].trim().to_string(),
                name: c[1].trim().to_string(),
                bytes: parse_size(c[2]).unwrap_or(0),
            })
        })
        .collect()
}

/// Images that no container uses: not referenced by a container's image name or id (either may be a prefix of the
/// other, the engines print short ids).
pub fn unused_images(images: &[ImageRow], container_images: &[String]) -> Vec<ImageRow> {
    images
        .iter()
        .filter(|i| {
            !container_images
                .iter()
                .any(|c| c == &i.name || (!c.is_empty() && (i.id.starts_with(c.as_str()) || c.starts_with(i.id.as_str()))))
        })
        .cloned()
        .collect()
}

/// Whether an image row is unnamed (dangling).
pub fn is_dangling(i: &ImageRow) -> bool {
    i.name.contains("<none>")
}

/// A preview item from rows.
pub fn item(kind: PruneKind, rows: &[ImageRow]) -> PreviewItem {
    PreviewItem {
        kind,
        count: u32::try_from(rows.len()).unwrap_or(u32::MAX),
        bytes: rows.iter().map(|r| r.bytes).sum(),
        examples: rows.iter().take(MAX_EXAMPLES).map(|r| format!("{} ({})", r.name, r.id)).collect(),
    }
}

/// A preview from items; the total is their sizes.
pub fn preview(items: Vec<PreviewItem>) -> PrunePreview {
    let total_bytes = items.iter().map(|i| i.bytes).sum();
    PrunePreview { items, total_bytes }
}

/// The bytes after `Total reclaimed space:` in a prune's output (`0B` or missing: 0).
pub fn parse_reclaimed(output: &str) -> u64 {
    output
        .lines()
        .find_map(|l| l.split_once("reclaimed space:").map(|(_, v)| v.trim()))
        .and_then(parse_size)
        .unwrap_or(0)
}

/// The arguments of the engine command that prunes `kind` for `selection`, without the program name.
pub fn prune_args(kind: PruneKind, selection: &PruneSelection) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(ToString::to_string).collect::<Vec<_>>();
    let mut args = match kind {
        PruneKind::StoppedContainers => s(&["container", "prune", "-f"]),
        PruneKind::DanglingImages => return s(&["image", "prune", "-f"]),
        PruneKind::UnusedImages => s(&["image", "prune", "-a", "-f"]),
        PruneKind::BuildCache => s(&["builder", "prune", "-f"]),
        PruneKind::Volumes => return s(&["volume", "prune", "-f"]),
    };
    if let Some(until) = selection.until() {
        args.push("--filter".into());
        args.push(format!("until={until}"));
    }
    args
}

// ---- the Actions cache folder ----------------------------------------------------------------------------------

/// One entry of the cache folder (a file or folder directly inside it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    /// Its name inside the folder.
    pub name: String,
    /// Its size.
    pub bytes: u64,
    /// When it last changed.
    pub modified: Timestamp,
}

/// How to trim the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheLimits {
    /// Keep the cache at most this big, in MB (0: no size limit).
    pub max_mb: u64,
    /// Delete entries not changed for this many days (0: no age limit).
    pub max_age_days: u32,
}

impl CacheLimits {
    /// Checks the limits (at least one, sane values).
    pub fn validate(&self) -> Result<(), String> {
        if self.max_mb == 0 && self.max_age_days == 0 {
            return Err("set a size limit, an age limit, or both".into());
        }
        if self.max_age_days > MAX_PRUNE_DAYS {
            return Err(format!("the age limit is at most {MAX_PRUNE_DAYS} days"));
        }
        Ok(())
    }
}

/// Why an entry is deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheReason {
    /// Older than the age limit.
    Old,
    /// The cache is over its size limit; oldest first.
    OverLimit,
}

/// The entries to delete for `limits`: first those older than the age limit, then the oldest of the rest until the
/// size limit holds. Never more than needed for the size limit.
pub fn plan_cache(entries: &[CacheEntry], limits: &CacheLimits, now: Timestamp) -> Vec<(CacheEntry, CacheReason)> {
    let mut sorted: Vec<&CacheEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| (e.modified, e.name.clone()));
    let age_ms = u64::from(limits.max_age_days) * 86_400_000;
    let mut out = Vec::new();
    let mut total: u64 = entries.iter().map(|e| e.bytes).sum();
    let mut rest = Vec::new();
    for e in sorted {
        if limits.max_age_days > 0 && now.0.saturating_sub(e.modified.0) >= age_ms {
            total = total.saturating_sub(e.bytes);
            out.push((e.clone(), CacheReason::Old));
        } else {
            rest.push(e);
        }
    }
    let cap = limits.max_mb.saturating_mul(1024 * 1024);
    if limits.max_mb > 0 {
        for e in rest {
            if total <= cap {
                break;
            }
            total = total.saturating_sub(e.bytes);
            out.push((e.clone(), CacheReason::OverLimit));
        }
    }
    out
}

/// Whether `path` is a folder Pitcrew may clean as a cache: absolute, not a root or a home folder itself, and at least
/// two components deep.
pub fn safe_cache_dir(path: &std::path::Path, home: Option<&std::path::Path>) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("the cache folder must be an absolute path".into());
    }
    // At least two real folder names below the root or drive (a prefix and a root do not count).
    let named = path.components().filter(|c| matches!(c, std::path::Component::Normal(_))).count();
    if named < 2 || home.is_some_and(|h| h == path) {
        return Err("the cache folder is too broad to clean; choose a folder of its own".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse_decimal_and_binary() {
        assert_eq!(parse_size("1.5GB"), Some(1_500_000_000));
        assert_eq!(parse_size("512MB"), Some(512_000_000));
        assert_eq!(parse_size("12kB"), Some(12_000));
        assert_eq!(parse_size("0B"), Some(0));
        assert_eq!(parse_size("1 GiB"), Some(1 << 30));
        assert_eq!(parse_size("7"), Some(7));
        assert_eq!(parse_size("lots"), None);
        assert_eq!(parse_size("1.2parsecs"), None);
        assert_eq!(parse_size(""), None);
    }

    #[test]
    fn the_df_table_is_parsed_and_bad_lines_skipped() {
        let out = "Images\t12\t3\t4.2GB\t3.1GB (73%)\nContainers\t5\t1\t120MB\t100MB (83%)\nbroken\nLocal Volumes\tx\t0\t0B\t0B\nBuild Cache\t40\t0\t9.5GB\t9.5GB\n";
        let u = parse_df(out);
        assert_eq!(u.len(), 3);
        assert_eq!(
            (u[0].what.as_str(), u[0].total, u[0].active, u[0].bytes, u[0].reclaimable_bytes),
            ("Images", 12, 3, 4_200_000_000, 3_100_000_000)
        );
        assert_eq!(u[2].reclaimable_bytes, 9_500_000_000);
    }

    #[test]
    fn unused_and_dangling_images_are_told_apart() {
        let imgs = parse_images("aaa111\tnode:20\t1.1GB\nbbb222\t<none>:<none>\t300MB\nccc333\tpostgres:16\t400MB\n\nshort\n");
        assert_eq!(imgs.len(), 3);
        assert!(is_dangling(&imgs[1]) && !is_dangling(&imgs[0]));
        let unused = unused_images(&imgs, &["postgres:16".to_string(), "aaa".to_string()]);
        assert_eq!(unused.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec!["bbb222"]);
        let it = item(PruneKind::DanglingImages, &unused);
        assert_eq!((it.count, it.bytes), (1, 300_000_000));
        assert_eq!(
            preview(vec![it, item(PruneKind::UnusedImages, &imgs)]).total_bytes,
            300_000_000 + 1_800_000_000
        );
        let many: Vec<ImageRow> = (0..20)
            .map(|i| ImageRow {
                id: format!("{i}"),
                name: "x".into(),
                bytes: 1,
            })
            .collect();
        assert_eq!(item(PruneKind::UnusedImages, &many).examples.len(), MAX_EXAMPLES);
    }

    #[test]
    fn prune_commands_and_results() {
        let sel = PruneSelection {
            kinds: vec![PruneKind::UnusedImages, PruneKind::Volumes],
            older_than_days: 7,
        };
        assert!(sel.validate().is_ok());
        assert_eq!(
            prune_args(PruneKind::UnusedImages, &sel),
            ["image", "prune", "-a", "-f", "--filter", "until=168h"]
        );
        assert_eq!(
            prune_args(PruneKind::Volumes, &sel),
            ["volume", "prune", "-f"],
            "no age filter on volumes"
        );
        assert_eq!(prune_args(PruneKind::DanglingImages, &sel), ["image", "prune", "-f"]);
        let any = PruneSelection {
            kinds: vec![PruneKind::BuildCache],
            older_than_days: 0,
        };
        assert_eq!(prune_args(PruneKind::BuildCache, &any), ["builder", "prune", "-f"]);
        assert_eq!(
            prune_args(PruneKind::StoppedContainers, &sel),
            ["container", "prune", "-f", "--filter", "until=168h"]
        );
        assert_eq!(
            parse_reclaimed("Deleted Images:\nx\n\nTotal reclaimed space: 1.2GB\n"),
            1_200_000_000
        );
        assert_eq!(parse_reclaimed("nothing"), 0);
        assert!(PruneSelection {
            kinds: vec![],
            older_than_days: 0
        }
        .validate()
        .is_err());
        assert!(PruneSelection {
            kinds: vec![PruneKind::Volumes, PruneKind::Volumes],
            older_than_days: 0
        }
        .validate()
        .is_err());
        assert!(PruneSelection {
            kinds: vec![PruneKind::Volumes],
            older_than_days: 99_999
        }
        .validate()
        .is_err());
        assert_eq!(EngineKind::Podman.program(), "podman");
        assert_eq!(PruneKind::ALL.len(), 5);
    }

    fn entry(name: &str, mb: u64, age_days: u64) -> CacheEntry {
        CacheEntry {
            name: name.into(),
            bytes: mb * 1024 * 1024,
            modified: Timestamp(100 * 86_400_000 - age_days * 86_400_000),
        }
    }

    #[test]
    fn the_cache_plan_deletes_old_entries_then_the_oldest_over_the_size_limit() {
        let now = Timestamp(100 * 86_400_000);
        let e = vec![entry("a", 500, 40), entry("b", 400, 10), entry("c", 300, 5), entry("d", 200, 1)];
        let by_age = plan_cache(
            &e,
            &CacheLimits {
                max_mb: 0,
                max_age_days: 30,
            },
            now,
        );
        assert_eq!(
            by_age.iter().map(|(e, r)| (e.name.as_str(), *r)).collect::<Vec<_>>(),
            vec![("a", CacheReason::Old)]
        );
        // 1400 MB total, limit 700: a (old, none), then b and c are the oldest of the rest.
        let by_size = plan_cache(
            &e,
            &CacheLimits {
                max_mb: 700,
                max_age_days: 0,
            },
            now,
        );
        assert_eq!(by_size.iter().map(|(e, _)| e.name.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        let both = plan_cache(
            &e,
            &CacheLimits {
                max_mb: 600,
                max_age_days: 30,
            },
            now,
        );
        assert_eq!(
            both.iter().map(|(e, r)| (e.name.as_str(), *r)).collect::<Vec<_>>(),
            vec![("a", CacheReason::Old), ("b", CacheReason::OverLimit)]
        );
        assert!(plan_cache(
            &e,
            &CacheLimits {
                max_mb: 5000,
                max_age_days: 0
            },
            now
        )
        .is_empty());
        assert!(CacheLimits {
            max_mb: 0,
            max_age_days: 0
        }
        .validate()
        .is_err());
        assert!(CacheLimits {
            max_mb: 1,
            max_age_days: 99_999
        }
        .validate()
        .is_err());
        assert!(CacheLimits {
            max_mb: 1,
            max_age_days: 0
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn only_specific_folders_may_be_cleaned_as_a_cache() {
        // Absolute on this OS: a drive on Windows, the root elsewhere.
        let root = if cfg!(windows) {
            std::path::PathBuf::from("C:\\")
        } else {
            std::path::PathBuf::from("/")
        };
        let users = root.join("Users");
        let home = users.join("me");
        assert!(safe_cache_dir(&home.join(".cache").join("actcache"), Some(&home)).is_ok());
        assert!(safe_cache_dir(std::path::Path::new("relative"), Some(&home)).is_err());
        assert!(safe_cache_dir(&root, Some(&home)).is_err());
        assert!(safe_cache_dir(&users, Some(&home)).is_err());
        assert!(safe_cache_dir(&home, Some(&home)).is_err());
    }
}
