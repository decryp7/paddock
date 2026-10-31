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
//! - Sizes of `worktrees` rows sum file lengths plus the folder entries themselves (a few KiB more than the `.ps1`'s files-only
//!   sum), and symlinks are not followed.
//! - `PADDOCK_PRUNE_UNIT=kb` is the hidden `--unit kb`.
//!
//! - The CI form exits 0 on every outcome, as the script did: a refused root, an unusable home, a failed scan, an empty
//!   `--max-gb` (`prune: no cap given; nothing to prune`) or a bad cap each print a notice and remove nothing. The plain form
//!   exits 2 for a bad command line and 4 for a refused root. A bad cap in the CI form is noticed before `--drop-bundles`
//!   removes anything (the script removed the bundles first).
//! - The home folder comes from `--home`, else `HOME` / `USERPROFILE`, is resolved, and the command fails closed (refused,
//!   exit 4) when none exists. The root guard compares by file identity where the platform has one (see [`crate::prune_root`]).
//!   The scripts had no such guard; `PADDOCK_CI_HOME` and `PADDOCK_CI_SLOT` are not read: pass `--ci-home` and `--slot`.
//! - A root that does not exist is still checked and refused when it would be inside the user's Documents, Desktop or
//!   Downloads; the scripts did nothing silently for any missing folder.
//! - `worktrees` looks for `<repo>/target` in `--repo` (default: the current folder), whereas Debrief's copy used its own
//!   `apps/debrief`: run it with `--repo apps/debrief --app debrief` to reproduce that one. A candidate that cannot be
//!   scanned is reported in `errors` and the listing goes on, as the `.ps1` did.
//!
//! Whole-folder removal does not demand that the folder looks like a Cargo build folder (CACHEDIR.TAG): the recorded golden
//! cases remove a plain fixture folder, as the real script does.
//!
//! No rule depends on time, so no clock is injected. The `.ps1` listing has no golden recorded from the real script (no
//! PowerShell here); its rules are ported by reading and pinned by unit and integration tests only.

use std::collections::BTreeSet;
use std::fmt;

/// The most folders one scan reads (a Cargo build folder has tens of thousands).
pub const MAX_DIRS: usize = 2_000_000;
/// The most entries (files, links, folders) one scan counts.
pub const MAX_ENTRIES: u64 = 100_000_000;
/// The deepest folder nesting a scan follows.
pub const MAX_DEPTH: usize = 512;
/// The largest cap in whole units that is accepted (a typo guard, 1 PiB in GiB).
pub const MAX_CAP: u64 = 1 << 20;
/// Why a prune request is refused or cannot run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PruneError {
    /// The cap is not a whole number of units from 0 to [`MAX_CAP`].
    BadCap(String),
    /// The app name is not `[a-z][a-z0-9-]*`.
    BadApp(String),
    /// A keep suffix is empty, holds a wildcard or a separator, or there are too many.
    BadKeep(String),
    /// No usable home folder: HOME / USERPROFILE is unset or does not exist and `--home` was not given (fails closed).
    NoHome(String),
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
            PruneError::NoHome(s) => write!(f, "no home folder could be found ({s}); pass an existing one with --home"),
            PruneError::BadKeep(s) => write!(
                f,
                "a keep suffix must be 1 or more letters, digits, '.', '_' or '-' (at most 256 of them), got '{s}'"
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

/// The CI form's notice for an empty cap.
pub const NO_CAP_MESSAGE: &str = "prune: no cap given; nothing to prune";

/// The CI form's notice for an empty folder.
pub const NO_FOLDER_MESSAGE: &str = "prune: no build folder was set in this job; nothing to prune";

/// The CI form's notice for a folder that is not this job's kept build folder.
pub fn not_owned_message(folder: &str, slot: Option<&str>) -> String {
    format!(
        "prune: {folder} is not a kept build folder of this job's slot (target-<name>-{} in the CI home); leaving it alone",
        slot.filter(|s| !s.is_empty()).unwrap_or("<slot>")
    )
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
    fn every_error_says_what_to_fix() {
        let all = [
            PruneError::BadCap("x".into()),
            PruneError::BadApp("X".into()),
            PruneError::BadKeep("".into()),
            PruneError::NoHome("HOME is not set".into()),
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
