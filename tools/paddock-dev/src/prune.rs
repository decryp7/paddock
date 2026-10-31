//! `dev host prune`: the pure plan (ADR 0022 R5). It replaces four scripts with one command:
//! `tools/prune-build-folders.sh`, `tools/prune-ci-build-folder.sh`, `tools/prune-build-folders.ps1` and Debrief's
//! `apps/debrief/scripts/clean-build-dirs.ps1` (the same listing as the `.ps1` with the app fixed to `debrief`). Dyno's
//! `ci-prune.sh`, named in the tooling inventory, no longer exists in the tree.
//!
//! This module holds no I/O: [`DirNode`] is a snapshot of a folder that [`crate::prune_fs`] reads, and every decision below
//! is a function of the snapshot, the numbers and the names, so it runs the same on every OS and a dry run predicts the
//! real run exactly.
//!
//! # The rules (identical to the scripts; each is pinned by `tests/golden-prune`)
//!
//! **`cap`** (`prune-build-folders.sh <folder> <max>`):
//! 1. A missing folder, a symlink, or anything that is not a folder is fine and left alone, silently.
//! 2. The size is the larger of the allocated size (blocks) and the apparent size (file lengths), each rounded up to KiB,
//!    counting every entry of the folder (files, symlinks themselves, folders) once per hard link, never following a symlink,
//!    divided (rounded down) by the unit: 1 GiB in KiB, or 1 KiB with the hidden `--unit kb` (tests).
//! 3. Size `>=` the cap prints `prune: <folder> is <size> (cap <max>); removing incremental caches` and removes every
//!    *folder* named exactly `incremental` below the root (case-sensitive, at any depth, not the root itself, not a file or
//!    symlink of that name, not looked into once matched).
//! 4. Measured again, size `>=` the cap prints `prune: still <size>; removing <folder>` and removes the whole folder.
//! 5. Only `<folder>` is touched. A failed removal is reported and does not stop the run (the script said `|| true`).
//!
//! **`cap` in the CI form** (`prune-ci-build-folder.sh`, selected by `--ci-home`): an empty folder prints
//! `prune: no build folder was set in this job; nothing to prune`. Anything that is not a folder, is a symlink, or whose
//! resolved path is not `<ci home>/target-<name>-<slot>` directly in the CI home (a non-empty `<name>`, this job's slot
//! only, so never a concurrent job's) prints `prune: <folder> is not a kept build folder of this job's slot (...); leaving
//! it alone` and removes nothing. Otherwise `dist/bundle` is removed first when `--drop-bundles` is given, then rule 2 to 5
//! on the resolved path. The CI form always ends well: a failure is data in the report, not an exit code.
//!
//! **`worktrees`** (`prune-build-folders.ps1 -App <app> [-Remove] [-Keep ...]`): the candidates are the folders directly in
//! the user's home named `.<app>-target*` and the folders directly in `<repo>/target` except `debug`, `release`,
//! `rust-analyzer`, `doc`, `tmp` and `flycheck*`; names compare ASCII case-insensitively, as PowerShell's `-like` does. A
//! candidate is kept when its name starts with `.<app>-target-<k>` or `agent-<k>` for a `-Keep` suffix `<k>`. Without
//! `--remove` every candidate is listed as `found`; with it the kept ones are `kept` and the others are removed
//! (`removing`). Each row is `{size,8:N1} GB  {action,-9} {path}`.
//!
//! # Differences from the scripts, on purpose
//!
//! - A candidate that is a symlink is listed as `skipped` and never removed or followed (the `.ps1` would have removed the
//!   link); `dist/bundle` behind a symlink is left alone (the shell script's `rm -rf` followed a symlinked `dist`).
//! - A keep suffix may not be empty or hold `*`, `?`, `[`, `]` or a path separator (in PowerShell they were wildcards).
//! - The `.ps1`'s closing `Free on <drive>` line is not printed (it needs a platform call; a later slice).
//! - Sizes of `worktrees` rows are the same sum of file lengths the `.ps1` took, but symlinks are not followed.
//! - `PADDOCK_PRUNE_UNIT=kb` is the hidden `--unit kb`.
//!
//! No rule depends on time, so no clock is injected.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// The most folders one scan reads (a Cargo build folder has tens of thousands).
pub const MAX_DIRS: usize = 2_000_000;
/// The most entries (files, links, folders) one scan counts.
pub const MAX_ENTRIES: u64 = 100_000_000;
/// The deepest folder nesting a scan follows.
pub const MAX_DEPTH: usize = 512;
/// The largest cap in whole units that is accepted (a typo guard, 1 PiB in GiB).
pub const MAX_CAP: u64 = 1 << 20;
/// The most `--keep` suffixes.
pub const MAX_KEEP: usize = 256;

/// The folders under `<repo>/target` that belong to the editor and the build, never to a finished agent.
pub const PROTECTED_TARGETS: [&str; 5] = ["debug", "release", "rust-analyzer", "doc", "tmp"];

/// Why a prune request is refused or cannot run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PruneError {
    /// The cap is not a whole number of units from 0 to [`MAX_CAP`].
    BadCap(String),
    /// The app name is not `[a-z][a-z0-9-]*`.
    BadApp(String),
    /// A keep suffix is empty, holds a wildcard or a separator, or there are too many.
    BadKeep(String),
    /// The root is empty or relative.
    NotAbsolute(String),
    /// The root holds a `..` component.
    DotDot(String),
    /// The root is the top of a disk or one level below it.
    TooShallow(String),
    /// The root is the user's home folder or holds it.
    ContainsHome(String),
    /// The root is inside one of the user's document folders (ER-14).
    UserFolder(String),
    /// The folder tree is bigger than a scan accepts.
    TooLarge(&'static str),
    /// A file system call failed.
    Io(String),
}

impl fmt::Display for PruneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PruneError::BadCap(s) => write!(f, "the cap must be a whole number of units from 0 to {MAX_CAP}, got '{s}'"),
            PruneError::BadApp(s) => write!(
                f,
                "the app name must be lowercase letters, digits and '-', starting with a letter, got '{s}'"
            ),
            PruneError::BadKeep(s) => write!(
                f,
                "a keep suffix must be 1 or more letters, digits, '.', '_' or '-' (at most {MAX_KEEP} of them), got '{s}'"
            ),
            PruneError::NotAbsolute(s) => write!(f, "'{s}' is not an absolute path"),
            PruneError::DotDot(s) => write!(f, "'{s}' holds a '..' component"),
            PruneError::TooShallow(s) => write!(f, "'{s}' is the top of a disk or directly below it"),
            PruneError::ContainsHome(s) => write!(f, "'{s}' is the home folder or holds it"),
            PruneError::UserFolder(s) => write!(f, "'{s}' is inside the user's Documents, Desktop or Downloads"),
            PruneError::TooLarge(what) => write!(f, "the folder is too large to scan: {what}"),
            PruneError::Io(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for PruneError {}

/// The unit a cap is given in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Gigabytes (GiB), what the CI action passes.
    Gb,
    /// Kilobytes (KiB), the hidden test unit.
    Kb,
}

impl Unit {
    /// KiB in one unit.
    pub fn kib(self) -> u64 {
        match self {
            Unit::Gb => 1_048_576,
            Unit::Kb => 1,
        }
    }

    /// The unit's name in the report.
    pub fn name(self) -> &'static str {
        match self {
            Unit::Gb => "gb",
            Unit::Kb => "kb",
        }
    }
}

/// Parses a cap: digits only, so no sign, no fraction, no empty string; bounded by [`MAX_CAP`].
pub fn parse_cap(text: &str) -> Result<u64, PruneError> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(PruneError::BadCap(text.to_string()));
    }
    match text.parse::<u64>() {
        Ok(n) if n <= MAX_CAP => Ok(n),
        _ => Err(PruneError::BadCap(text.to_string())),
    }
}

/// A snapshot of one folder: its own entry and the files and links directly in it, and its sub-folders. Files are not kept
/// one by one (a build folder has millions); only their summed sizes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirNode {
    /// The folder's name (the root's is the last component of its path).
    pub name: String,
    /// The folder entry itself plus the files and symlinks directly in it: their lengths.
    pub apparent: u64,
    /// The same entries by blocks allocated (the length where the platform has no block count).
    pub allocated: u64,
    /// Device and inode, where the platform has them: the identity checked again before a removal.
    pub id: Option<FileId>,
    /// The sub-folders (never symlinks), sorted by name.
    pub dirs: Vec<DirNode>,
}

/// Device and inode of a folder (unix).
pub type FileId = (u64, u64);

/// A folder below the root as its chain of names.
pub type RelPath = Vec<String>;

/// The apparent and allocated bytes of a folder tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    /// Summed lengths.
    pub apparent: u64,
    /// Summed allocation.
    pub allocated: u64,
}

impl Size {
    /// The cap's figure: the larger of the two, each rounded up to KiB, in whole `unit`s (rounded down).
    pub fn units(self, unit: Unit) -> u64 {
        let kib = |bytes: u64| bytes.div_ceil(1024);
        kib(self.apparent).max(kib(self.allocated)) / unit.kib()
    }
}

fn skipped(skip: &BTreeSet<RelPath>, path: &[String]) -> bool {
    !skip.is_empty() && skip.contains(path)
}

/// The size of `node` without the sub-trees named in `skip` (already removed, or about to be in a dry run).
pub fn measure(node: &DirNode, skip: &BTreeSet<RelPath>) -> Size {
    fn walk(node: &DirNode, path: &mut Vec<String>, skip: &BTreeSet<RelPath>, acc: &mut Size) {
        acc.apparent = acc.apparent.saturating_add(node.apparent);
        acc.allocated = acc.allocated.saturating_add(node.allocated);
        for d in &node.dirs {
            path.push(d.name.clone());
            if !skipped(skip, path) {
                walk(d, path, skip, acc);
            }
            path.pop();
        }
    }
    let mut acc = Size::default();
    walk(node, &mut Vec::new(), skip, &mut acc);
    acc
}

/// Every folder named exactly `incremental` below the root (not the root), pruned once matched, minus those under `skip`.
pub fn find_incremental(root: &DirNode, skip: &BTreeSet<RelPath>) -> Vec<RelPath> {
    fn walk(node: &DirNode, path: &mut Vec<String>, skip: &BTreeSet<RelPath>, out: &mut Vec<RelPath>) {
        for d in &node.dirs {
            path.push(d.name.clone());
            if !skipped(skip, path) {
                if d.name == "incremental" {
                    out.push(path.clone());
                } else {
                    walk(d, path, skip, out);
                }
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(root, &mut Vec::new(), skip, &mut out);
    out
}

/// Whether the folder at `path` below `root` is in the snapshot.
pub fn contains(root: &DirNode, path: &[String]) -> bool {
    let mut node = root;
    for name in path {
        match node.dirs.iter().find(|d| &d.name == name) {
            Some(d) => node = d,
            None => return false,
        }
    }
    true
}

/// `dist/bundle`: the installers already uploaded as artifacts.
pub fn bundle_path() -> RelPath {
    vec!["dist".to_string(), "bundle".to_string()]
}

/// What a `cap` run does to a folder, decided from one snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapPlan {
    /// The cap's figure before anything is removed.
    pub size_before: u64,
    /// The figure was at or over the cap, so the incremental caches go.
    pub over: bool,
    /// The folders to remove first (empty when not over).
    pub incremental: Vec<RelPath>,
    /// The figure after they are gone (the same as `size_before` when not over).
    pub size_after: u64,
    /// The folder itself goes as well.
    pub remove_folder: bool,
}

/// Rules 3 and 4 on a snapshot; `skip` are folders already gone (or about to be, in a dry run).
pub fn plan_cap(tree: &DirNode, skip: &BTreeSet<RelPath>, max: u64, unit: Unit) -> CapPlan {
    let size_before = measure(tree, skip).units(unit);
    let over = size_before >= max;
    if !over {
        return CapPlan {
            size_before,
            over,
            incremental: Vec::new(),
            size_after: size_before,
            remove_folder: false,
        };
    }
    let incremental = find_incremental(tree, skip);
    let mut after = skip.clone();
    after.extend(incremental.iter().cloned());
    let size_after = measure(tree, &after).units(unit);
    CapPlan {
        size_before,
        over,
        incremental,
        size_after,
        remove_folder: size_after >= max,
    }
}

/// The notice before the incremental caches go.
pub fn over_message(folder: &str, size: u64, max: u64) -> String {
    format!("prune: {folder} is {size} (cap {max}); removing incremental caches")
}

/// The notice before the whole folder goes.
pub fn still_message(folder: &str, size: u64) -> String {
    format!("prune: still {size}; removing {folder}")
}

/// The CI form's notice for an empty folder.
pub const NO_FOLDER_MESSAGE: &str = "prune: no build folder was set in this job; nothing to prune";

/// The CI form's notice for a folder that is not this job's kept build folder.
pub fn not_owned_message(folder: &str, slot: Option<&str>) -> String {
    format!(
        "prune: {folder} is not a kept build folder of this job's slot (target-<name>-{} in the CI home); leaving it alone",
        slot.filter(|s| !s.is_empty()).unwrap_or("<slot>")
    )
}

/// A slot name as `job-isolation.sh` accepts it: `s` and a number from 1, no leading zero.
pub fn valid_slot(slot: &str) -> bool {
    let digits = slot.strip_prefix('s').unwrap_or("");
    !digits.is_empty() && !digits.starts_with('0') && digits.bytes().all(|b| b.is_ascii_digit())
}

/// `job-isolation.sh owns <folder> target`: the resolved `folder` is `target-<name>-<slot>` directly in the resolved CI home.
pub fn owns(ci_home: &Path, folder: &Path, slot: Option<&str>) -> bool {
    let Some(slot) = slot.filter(|s| valid_slot(s)) else {
        return false;
    };
    if folder.parent() != Some(ci_home) {
        return false;
    }
    let name = folder.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let tail = format!("-{slot}");
    name.strip_prefix("target-")
        .and_then(|rest| rest.strip_suffix(tail.as_str()))
        .is_some_and(|middle| !middle.is_empty())
}

fn names_ci(component: &std::ffi::OsStr, wanted: &str) -> bool {
    component.to_str().is_some_and(|c| c.eq_ignore_ascii_case(wanted))
}

/// ER-14 and ER-24: the roots a prune never touches. `root` is absolute and, where it exists, already resolved.
///
/// Refused: a relative or empty root, a `..`, the top of a disk or one level below it, the user's home folder or a folder
/// holding it, and anything inside the home's `Documents`, `Desktop` or `Downloads`. The CI home (`~/.paddock-ci`) and
/// `~/.<app>-target-*` are inside the home on purpose and are allowed.
pub fn check_root(root: &Path, home: Option<&Path>) -> Result<(), PruneError> {
    let shown = root.display().to_string();
    if shown.is_empty() || !root.is_absolute() {
        return Err(PruneError::NotAbsolute(shown));
    }
    if root.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(PruneError::DotDot(shown));
    }
    let depth = root.components().filter(|c| matches!(c, Component::Normal(_))).count();
    if depth < 2 {
        return Err(PruneError::TooShallow(shown));
    }
    if let Some(home) = home {
        if home.starts_with(root) {
            return Err(PruneError::ContainsHome(shown));
        }
        if let Ok(rest) = root.strip_prefix(home)
            && let Some(first) = rest.components().next()
            && ["Documents", "Desktop", "Downloads"].iter().any(|n| names_ci(first.as_os_str(), n))
        {
            return Err(PruneError::UserFolder(shown));
        }
    }
    Ok(())
}

/// `[a-z][a-z0-9-]*`, the app name of the `.ps1`'s `ValidatePattern`.
pub fn valid_app(app: &str) -> bool {
    let mut bytes = app.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase()) && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Validates the keep suffixes (bounded in number and shape).
pub fn check_keep(keep: &[String]) -> Result<(), PruneError> {
    if keep.len() > MAX_KEEP {
        return Err(PruneError::BadKeep(format!("{} suffixes", keep.len())));
    }
    for k in keep {
        if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
            return Err(PruneError::BadKeep(k.clone()));
        }
    }
    Ok(())
}

fn starts_with_ci(name: &str, prefix: &str) -> bool {
    name.len() >= prefix.len() && name.is_char_boundary(prefix.len()) && name[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// A folder directly in the home is a candidate when it is named `.<app>-target*`.
pub fn home_candidate(app: &str, name: &str) -> bool {
    starts_with_ci(name, &format!(".{app}-target"))
}

/// A folder directly in `<repo>/target` is a candidate unless it is protected or an editor's `flycheck*`.
pub fn target_candidate(name: &str) -> bool {
    !PROTECTED_TARGETS.iter().any(|p| name.eq_ignore_ascii_case(p)) && !starts_with_ci(name, "flycheck")
}

/// Whether a candidate is named by a keep suffix: `.<app>-target-<k>*` or `agent-<k>*`.
pub fn is_kept(app: &str, keep: &[String], name: &str) -> bool {
    keep.iter()
        .any(|k| starts_with_ci(name, &format!(".{app}-target-{k}")) || starts_with_ci(name, &format!("agent-{k}")))
}

/// A listed build folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The folder.
    pub path: PathBuf,
    /// Its name.
    pub name: String,
    /// Summed file lengths.
    pub bytes: u64,
    /// It is a symlink: listed, never followed or removed.
    pub symlink: bool,
}

/// What happens to a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Listed only (no `--remove`).
    Found,
    /// Named by `--keep`.
    Kept,
    /// Removed.
    Removing,
    /// Would be removed (`--remove --dry-run`).
    WouldRemove,
    /// A symlink: left alone.
    Skipped,
}

impl Action {
    /// The word in the listing and the JSON.
    pub fn word(self) -> &'static str {
        match self {
            Action::Found => "found",
            Action::Kept => "kept",
            Action::Removing => "removing",
            Action::WouldRemove => "would-remove",
            Action::Skipped => "skipped",
        }
    }
}

/// One row of the listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The folder.
    pub path: PathBuf,
    /// Summed file lengths.
    pub bytes: u64,
    /// What happens to it.
    pub action: Action,
}

/// Decides every candidate's action.
pub fn plan_rows(app: &str, keep: &[String], remove: bool, dry_run: bool, candidates: &[Candidate]) -> Vec<Row> {
    candidates
        .iter()
        .map(|c| {
            let action = if c.symlink {
                Action::Skipped
            } else if !remove {
                Action::Found
            } else if is_kept(app, keep, &c.name) {
                Action::Kept
            } else if dry_run {
                Action::WouldRemove
            } else {
                Action::Removing
            };
            Row {
                path: c.path.clone(),
                bytes: c.bytes,
                action,
            }
        })
        .collect()
}

/// Gigabytes to one decimal with banker's rounding, as `[math]::Round(x, 1)` gives, grouped as `N1` prints.
pub fn format_gb(bytes: u64) -> String {
    let tenths = (bytes as f64 / 1_073_741_824.0 * 10.0).round_ties_even() as u64;
    let int = (tenths / 10).to_string();
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{grouped}.{}", tenths % 10)
}

/// `'{0,8:N1} GB  {1,-9} {2}'`.
pub fn format_row(row: &Row) -> String {
    format!("{:>8} GB  {:<9} {}", format_gb(row.bytes), row.action.word(), row.path.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str, apparent: u64, allocated: u64, dirs: Vec<DirNode>) -> DirNode {
        DirNode {
            name: name.into(),
            apparent,
            allocated,
            id: None,
            dirs,
        }
    }
    fn rel(parts: &[&str]) -> RelPath {
        parts.iter().map(|s| s.to_string()).collect()
    }
    fn set(paths: &[&[&str]]) -> BTreeSet<RelPath> {
        paths.iter().map(|p| rel(p)).collect()
    }
    fn tree() -> DirNode {
        // root 1k; debug 1k with incremental 2k and deps 1k; release/deep/incremental 4k (with an inner incremental);
        // a folder `incrementals` is not a match.
        dir(
            "root",
            1024,
            1024,
            vec![
                dir(
                    "debug",
                    1024,
                    1024,
                    vec![
                        dir("deps", 1024, 1024, vec![]),
                        dir("incremental", 2048, 2048, vec![dir("incremental", 512, 512, vec![])]),
                    ],
                ),
                dir("incrementals", 1024, 1024, vec![]),
                dir(
                    "release",
                    0,
                    0,
                    vec![dir("deep", 0, 0, vec![dir("incremental", 4096, 4096, vec![])])],
                ),
            ],
        )
    }

    #[test]
    fn cap_is_digits_only_and_bounded() {
        assert_eq!(parse_cap("0"), Ok(0));
        assert_eq!(parse_cap("20"), Ok(20));
        assert_eq!(parse_cap(&MAX_CAP.to_string()), Ok(MAX_CAP));
        for bad in [
            "",
            "many",
            "-1",
            "1.5",
            " 1",
            "+1",
            &(MAX_CAP + 1).to_string(),
            "99999999999999999999999",
        ] {
            assert_eq!(parse_cap(bad), Err(PruneError::BadCap(bad.to_string())), "{bad:?}");
        }
    }

    #[test]
    fn units_take_the_larger_figure_rounded_up_then_down_by_the_unit() {
        let s = Size {
            apparent: 1025,
            allocated: 0,
        };
        assert_eq!(s.units(Unit::Kb), 2, "1025 bytes round up to 2 KiB");
        let s = Size {
            apparent: 10,
            allocated: 4096 * 3,
        };
        assert_eq!(s.units(Unit::Kb), 12, "allocation wins when larger");
        let gib = 1u64 << 30;
        let s = Size {
            apparent: 3 * gib - 2048,
            allocated: 0,
        };
        assert_eq!(s.units(Unit::Gb), 2, "whole gigabytes round down");
        assert_eq!(Unit::Gb.kib(), 1_048_576);
        assert_eq!((Unit::Gb.name(), Unit::Kb.name()), ("gb", "kb"));
    }

    #[test]
    fn measure_sums_everything_except_the_skipped_subtrees() {
        let t = tree();
        let all = measure(&t, &BTreeSet::new());
        assert_eq!(all.apparent, 1024 + 1024 + 1024 + 2048 + 512 + 1024 + 4096);
        let without = measure(&t, &set(&[&["debug", "incremental"], &["release", "deep", "incremental"]]));
        assert_eq!(without.apparent, 1024 * 4);
    }

    #[test]
    fn incremental_folders_match_by_exact_name_and_are_not_looked_into() {
        let t = tree();
        assert_eq!(
            find_incremental(&t, &BTreeSet::new()),
            vec![rel(&["debug", "incremental"]), rel(&["release", "deep", "incremental"])]
        );
        assert_eq!(
            find_incremental(&t, &set(&[&["debug"]])),
            vec![rel(&["release", "deep", "incremental"])],
            "a skipped (gone) folder is not looked into"
        );
        let named_root = dir("incremental", 1, 1, vec![]);
        assert!(
            find_incremental(&named_root, &BTreeSet::new()).is_empty(),
            "the root is never matched"
        );
    }

    #[test]
    fn contains_follows_a_chain_of_names() {
        let t = tree();
        assert!(contains(&t, &[]) && contains(&t, &rel(&["debug", "deps"])));
        assert!(!contains(&t, &rel(&["debug", "nope"])) && !contains(&t, &rel(&["dist", "bundle"])));
        assert_eq!(bundle_path(), rel(&["dist", "bundle"]));
    }

    #[test]
    fn under_the_cap_nothing_is_planned() {
        let p = plan_cap(&tree(), &BTreeSet::new(), 100, Unit::Kb);
        assert_eq!((p.size_before, p.over, p.size_after, p.remove_folder), (11, false, 11, false));
        assert!(p.incremental.is_empty());
    }

    #[test]
    fn over_the_cap_the_incremental_caches_go_first_and_the_folder_only_if_still_over() {
        let t = tree();
        let p = plan_cap(&t, &BTreeSet::new(), 8, Unit::Kb);
        assert_eq!((p.size_before, p.size_after), (11, 4));
        assert_eq!(p.incremental.len(), 2);
        assert!(!p.remove_folder);
        let p = plan_cap(&t, &BTreeSet::new(), 4, Unit::Kb);
        assert!(p.remove_folder, "equal to the cap counts as over");
        let p = plan_cap(&t, &set(&[&["release"]]), 7, Unit::Kb);
        assert_eq!((p.size_before, p.over), (7, true), "skipped folders are not counted");
        let p = plan_cap(&dir("r", 5 * 1024, 5 * 1024, vec![]), &BTreeSet::new(), 3, Unit::Kb);
        assert!(
            p.over && p.incremental.is_empty() && p.remove_folder,
            "no caches, still over: the folder goes"
        );
        let p = plan_cap(&dir("r", 0, 0, vec![]), &BTreeSet::new(), 0, Unit::Kb);
        assert!(p.remove_folder, "a zero cap removes everything");
    }

    #[test]
    fn messages_match_the_scripts() {
        assert_eq!(over_message("/b", 9, 3), "prune: /b is 9 (cap 3); removing incremental caches");
        assert_eq!(still_message("/b", 9), "prune: still 9; removing /b");
        assert!(not_owned_message("/b", Some("s2")).contains("target-<name>-s2 in the CI home); leaving it alone"));
        assert!(not_owned_message("/b", None).contains("target-<name>-<slot>"));
        assert!(
            not_owned_message("/b", Some("")).contains("target-<name>-<slot>"),
            "an empty slot is an unset one"
        );
        assert!(NO_FOLDER_MESSAGE.ends_with("nothing to prune"));
    }

    #[test]
    fn slots_are_s_and_a_number_from_one() {
        for ok in ["s1", "s9", "s10", "s123"] {
            assert!(valid_slot(ok), "{ok}");
        }
        for bad in ["", "s", "s0", "s01", "1", "S1", "s1a", "s-1", " s1"] {
            assert!(!valid_slot(bad), "{bad:?}");
        }
    }

    #[test]
    fn ownership_is_target_name_slot_directly_in_the_ci_home() {
        let home = Path::new("/ci");
        assert!(owns(home, Path::new("/ci/target-app-s1"), Some("s1")));
        assert!(owns(home, Path::new("/ci/target-a-b-s1"), Some("s1")));
        assert!(!owns(home, Path::new("/ci/target-app-s2"), Some("s1")), "another job's slot");
        assert!(!owns(home, Path::new("/ci/target-app-s1"), None) && !owns(home, Path::new("/ci/target-app-s1"), Some("1")));
        assert!(!owns(home, Path::new("/ci/target--s1"), Some("s1")), "the name may not be empty");
        assert!(!owns(home, Path::new("/ci/target-s1"), Some("s1")));
        assert!(!owns(home, Path::new("/ci/notes"), Some("s1")) && !owns(home, Path::new("/ci"), Some("s1")));
        assert!(!owns(home, Path::new("/ci/sub/target-app-s1"), Some("s1")), "not nested");
        assert!(
            !owns(home, Path::new("/other/target-app-s1"), Some("s1")),
            "not outside the CI home"
        );
        assert!(!owns(home, Path::new("/"), Some("s1")), "no file name");
    }

    #[test]
    fn roots_in_the_users_documents_or_holding_the_home_are_refused() {
        let home = Path::new("/Users/me");
        let ok = |p: &str| check_root(Path::new(p), Some(home));
        assert_eq!(ok("/Users/me/.paddock-ci/target-a-s1"), Ok(()));
        assert_eq!(ok("/Volumes/work/build"), Ok(()));
        assert_eq!(
            check_root(Path::new("/Users/me/Documents/x"), None),
            Ok(()),
            "no home known: only the shape is checked"
        );
        assert!(matches!(ok("/Users/me/Documents/build"), Err(PruneError::UserFolder(_))));
        assert!(
            matches!(ok("/Users/me/downloads"), Err(PruneError::UserFolder(_))),
            "case-insensitive"
        );
        assert!(matches!(ok("/Users/me/Desktop"), Err(PruneError::UserFolder(_))));
        assert!(matches!(ok("/Users/me"), Err(PruneError::ContainsHome(_))));
        assert!(matches!(ok("/Users"), Err(PruneError::TooShallow(_))), "one level below the top");
        assert!(matches!(ok("/"), Err(PruneError::TooShallow(_))));
        assert!(matches!(ok("relative/dir"), Err(PruneError::NotAbsolute(_))));
        assert!(matches!(ok(""), Err(PruneError::NotAbsolute(_))));
        assert!(matches!(ok("/Users/me/x/../Documents"), Err(PruneError::DotDot(_))));
        assert!(matches!(
            check_root(Path::new("/a/b/c"), Some(Path::new("/a/b/c/d"))),
            Err(PruneError::ContainsHome(_))
        ));
    }

    #[test]
    fn app_names_and_keep_suffixes_are_checked() {
        for ok in ["debrief", "a", "pit-wall2"] {
            assert!(valid_app(ok), "{ok}");
        }
        for bad in ["", "Debrief", "2fast", "-a", "a b", "a_b", "a*"] {
            assert!(!valid_app(bad), "{bad:?}");
        }
        assert_eq!(check_keep(&["6c".into(), "sdk-1.2_x".into()]), Ok(()));
        for bad in ["", "a*", "a?", "[a]", "a/b", "a\\b"] {
            assert_eq!(check_keep(&[bad.to_string()]), Err(PruneError::BadKeep(bad.to_string())), "{bad:?}");
        }
        assert!(check_keep(&vec!["a".to_string(); MAX_KEEP + 1]).is_err());
    }

    #[test]
    fn candidates_are_chosen_by_name_without_regard_to_case() {
        assert!(home_candidate("debrief", ".debrief-target") && home_candidate("debrief", ".Debrief-Target-6c"));
        assert!(
            !home_candidate("debrief", ".dyno-target-x")
                && !home_candidate("debrief", "debrief-target")
                && !home_candidate("debrief", ".deb")
        );
        for protected in ["debug", "Release", "rust-analyzer", "DOC", "tmp", "flycheck0", "Flycheck-x"] {
            assert!(!target_candidate(protected), "{protected}");
        }
        for ok in ["agent-6c", "agent-x", "other", "debugger", "tmp2"] {
            assert!(target_candidate(ok), "{ok}");
        }
        assert!(
            !starts_with_ci("é", "ab") && !starts_with_ci("aé", "a\u{e9}x"),
            "a prefix inside a character is not a match"
        );
        assert!(!starts_with_ci("a", "ab"));
    }

    #[test]
    fn keep_suffixes_name_both_kinds_of_folder() {
        let keep = vec!["6c".to_string(), "sdk".to_string()];
        assert!(is_kept("debrief", &keep, ".debrief-target-6c1") && is_kept("debrief", &keep, "agent-SDK"));
        assert!(
            !is_kept("debrief", &keep, ".debrief-target")
                && !is_kept("debrief", &keep, "agent-7")
                && !is_kept("debrief", &keep, ".dyno-target-6c")
        );
        assert!(!is_kept("debrief", &[], "agent-6c"));
    }

    fn cands() -> Vec<Candidate> {
        let c = |name: &str, bytes: u64, symlink: bool| Candidate {
            path: PathBuf::from("/h").join(name),
            name: name.into(),
            bytes,
            symlink,
        };
        vec![
            c(".d-target-6c", 1 << 30, false),
            c("agent-9", 3 << 30, false),
            c("agent-link", 0, true),
        ]
    }

    #[test]
    fn rows_list_remove_keep_and_skip_links() {
        let keep = vec!["6c".to_string()];
        let words = |remove, dry| {
            plan_rows("d", &keep, remove, dry, &cands())
                .iter()
                .map(|r| r.action.word())
                .collect::<Vec<_>>()
        };
        assert_eq!(words(false, false), ["found", "found", "skipped"]);
        assert_eq!(words(true, false), ["kept", "removing", "skipped"]);
        assert_eq!(words(true, true), ["kept", "would-remove", "skipped"]);
        assert_eq!(
            words(false, true),
            ["found", "found", "skipped"],
            "dry run without --remove only lists"
        );
    }

    #[test]
    fn rows_print_like_the_powershell_format() {
        assert_eq!(format_gb(0), "0.0");
        assert_eq!(format_gb(1 << 30), "1.0");
        assert_eq!(format_gb(1 << 28), "0.2", "0.25 GB is 2.5 tenths: a tie rounds to the even 2");
        assert_eq!(format_gb(3 << 28), "0.8", "0.75 GB is 7.5 tenths: a tie rounds to the even 8");
        assert_eq!(format_gb(1_536 * (1 << 20)), "1.5");
        assert_eq!(format_gb(1234 * (1 << 30)), "1,234.0");
        assert_eq!(format_gb(1_234_567 * (1 << 30)), "1,234,567.0");
        let row = Row {
            path: PathBuf::from("/h/agent-9"),
            bytes: 3 << 30,
            action: Action::Removing,
        };
        assert_eq!(format_row(&row), "     3.0 GB  removing  /h/agent-9");
        assert_eq!(Action::Found.word(), "found");
        assert_eq!(Action::Kept.word(), "kept");
    }

    #[test]
    fn every_error_says_what_to_fix() {
        let all = [
            PruneError::BadCap("x".into()),
            PruneError::BadApp("X".into()),
            PruneError::BadKeep("".into()),
            PruneError::NotAbsolute("r".into()),
            PruneError::DotDot("a/..".into()),
            PruneError::TooShallow("/".into()),
            PruneError::ContainsHome("/h".into()),
            PruneError::UserFolder("/h/Documents".into()),
            PruneError::TooLarge("too many folders"),
            PruneError::Io("boom".into()),
        ];
        let texts: BTreeSet<String> = all.iter().map(|e| e.to_string()).collect();
        assert_eq!(texts.len(), all.len());
        assert!(texts.iter().all(|t| !t.is_empty()));
    }
}
