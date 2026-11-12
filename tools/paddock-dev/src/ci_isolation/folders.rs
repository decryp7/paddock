//! The kept folders of a slot: which exist, which of them a sweep removes now (`due`), and `owns`.

use super::home::{ci_home, slot};
use super::table::{Kept, expand_dirs, glob_match, glob_of};
use super::{Ctx, R, Stop};
use paddock_env::winpath;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Every kept folder of slot `n` (all names and sources), found through [`glob_of`], one per folder.
pub fn slot_folders(kept: &Kept, home: &Path, n: u32) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for l in kept.slot_layouts() {
        found.extend(expand_dirs(home, &glob_of(&l.layout, &format!("s{n}"), false)));
    }
    found
}

/// Kept folders of the old unslotted layout (a slot layout without its `-{slot}`; names that end in a slot are the new layout's).
pub fn legacy_folders(kept: &Kept, home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for l in kept.slot_layouts() {
        let unslotted = l.layout.replace("-{slot}", "");
        for f in expand_dirs(home, &glob_of(&unslotted, "", false)) {
            let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if ends_in_slot(name) {
                continue;
            }
            found.push(f);
        }
    }
    found
}

/// `...-s<digits>`: the new layout's names.
fn ends_in_slot(name: &str) -> bool {
    name.rsplit_once("-s")
        .is_some_and(|(_, d)| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

/// Folders of a retired layout (amendment §1) that no current layout owns, for slot `n`.
pub fn retired_folders(kept: &Kept, home: &Path, n: u32) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for r in kept.retired.iter().filter(|r| r.contains("{slot}")) {
        for f in expand_dirs(home, &glob_of(r, &format!("s{n}"), false)) {
            let rel = f
                .strip_prefix(home)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if !rel.is_empty() && kept.is_retired(&rel, n) && !found.contains(&f) {
                found.push(f);
            }
        }
    }
    found
}

/// The most slots one host hands out (`PADDOCK_CI_SLOTS`, default 4).
pub fn max_slots(ctx: &Ctx) -> R<u32> {
    let max = ctx.get("PADDOCK_CI_SLOTS").unwrap_or("4");
    match max.parse::<u32>() {
        Ok(n) if n > 0 && max.bytes().all(|b| b.is_ascii_digit()) => Ok(n),
        _ => Err(Stop::die(format!("PADDOCK_CI_SLOTS must be a whole number above 0 (got '{max}')"))),
    }
}

/// How many slots keep their folders (`PADDOCK_CI_KEEP_SLOTS`, default the maximum).
pub fn keep_slots(ctx: &Ctx) -> R<u32> {
    match ctx.get("PADDOCK_CI_KEEP_SLOTS") {
        Some(k) if k.bytes().all(|b| b.is_ascii_digit()) => Ok(k.parse().unwrap_or(0)),
        _ => max_slots(ctx),
    }
}

/// Is the folder's time stamp older than `days` days (what `find -mtime +days` asks: whole days of age above `days`)?
pub fn old(folder: &Path, days: u64, now: SystemTime) -> bool {
    let Ok(modified) = std::fs::metadata(folder).and_then(|m| m.modified()) else {
        return false;
    };
    let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
    age.as_secs() / 86_400 > days
}

/// What a sweep removes now from slot `n`: `(folder, why)`. With `all`, every folder (the disk is below its floor); for a slot above
/// the kept slots, every folder; else the folders no job used for `PADDOCK_CI_ORPHAN_DAYS` (default 14). Folders of a retired
/// layout are due at once, whatever their age (amendment §1). The old unslotted layout's folders belong to slot 1.
pub fn due(ctx: &Ctx, kept: &Kept, home: &Path, n: u32, all: bool, now: SystemTime) -> R<Vec<(PathBuf, String)>> {
    let keep = keep_slots(ctx)?;
    let days = ctx.digits("PADDOCK_CI_ORPHAN_DAYS").unwrap_or(14);
    let mut folders = slot_folders(kept, home, n);
    if n == 1 {
        folders.extend(legacy_folders(kept, home));
    }
    let retired = retired_folders(kept, home, n);
    for r in &retired {
        if !folders.contains(r) {
            folders.push(r.clone());
        }
    }
    let mut out = Vec::new();
    for f in folders {
        let why = if retired.contains(&f) {
            "retired layout: no current layout names it".to_string()
        } else if all {
            "the disk is below its free floor".to_string()
        } else if n > keep {
            format!("slot s{n} is above the {keep} kept slots")
        } else if old(&f, days, now) {
            format!("no job used it for {days} days")
        } else {
            continue;
        };
        out.push((f, why));
    }
    Ok(out)
}

/// `real` (a canonical folder, `/` separators) relative to `home`, or `None` when it is not under it. On Windows the drive letter's
/// case is not compared (`C:/ci` and `c:/ci` are one folder).
pub fn rel_under_home(home: &str, real: &str, windows: bool) -> Option<String> {
    let (home, real) = if windows {
        (super::home::normalize_root(home, true), super::home::normalize_root(real, true))
    } else {
        (home.to_string(), real.to_string())
    };
    real.strip_prefix(&format!("{home}/")).map(str::to_string)
}

/// Is `rel` a folder of a slot layout (of kind `only`, when given) of slot `slot`? Only a path with as many `/` as the layout: `*` in a
/// layout's glob would otherwise match across a `/`, and a folder inside a checkout would be taken for a kept folder.
pub fn owns_rel(kept: &Kept, rel: &str, slot: &str, only: &str) -> bool {
    kept.slot_layouts().any(|l| {
        (only.is_empty() || l.kind == only)
            && rel.matches('/').count() == l.layout.matches('/').count()
            && glob_match(&glob_of(&l.layout, slot, true), rel)
    })
}

/// `owns <folder> [<kind>]`: exit 0 when the folder is a kept folder (of that kind) of this job's slot; 1 otherwise, with no output.
pub fn cmd_owns(ctx: &Ctx, args: &[String]) -> R<()> {
    let refuse = || Stop::silent(1);
    let dir = super::home::from_git_bash(args.first().map(String::as_str).unwrap_or(""), ctx.windows);
    let only = args.get(1).map(String::as_str).unwrap_or("");
    let meta = std::fs::symlink_metadata(&dir).map_err(|_| refuse())?;
    if dir.is_empty() || !meta.is_dir() {
        return Err(refuse());
    }
    let s = slot(ctx).map_err(|_| refuse())?;
    let home = ci_home(ctx)?;
    let path = Path::new(&dir);
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let real_parent = winpath::real_for_tools(parent).map_err(|_| refuse())?;
    let name = path.file_name().ok_or_else(refuse)?;
    let real = format!("{}/{}", real_parent.trim_end_matches('/'), name.to_string_lossy());
    let rel = rel_under_home(&home, &real, ctx.windows).ok_or_else(refuse)?;
    if owns_rel(&ctx.kept()?, &rel, &s, only) {
        Ok(())
    } else {
        Err(refuse())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_isolation::table::parse_kept;
    use crate::ci_isolation::testkit::ctx;

    fn kept() -> Kept {
        parse_kept(
            "target target-{name}-{src}-{slot} slot\nsrc src/{name}-{slot} slot\nsccache sccache-{slot} slot\ntools tools shared\n\
             # retired: target-{name}-{slot} 2026-11-10\n",
        )
        .unwrap()
    }

    #[test]
    fn owns_is_by_layout_slot_and_depth_in_every_spelling_of_the_home() {
        let k = kept();
        assert!(owns_rel(&k, "target-x-0123456789-s1", "s1", "target"));
        assert!(!owns_rel(&k, "target-x-0123456789-s2", "s1", "target"), "another slot");
        assert!(!owns_rel(&k, "target-x-0123456789-s1", "s1", "src"), "another kind");
        // Nested inside a kept folder or a checkout: `?*` would match across the `/`, the depth guard does not.
        assert!(!owns_rel(&k, "target-x-0123456789-s1/sub-0123456789-s1", "s1", ""));
        assert!(!owns_rel(&k, "src/x-s1/target-y-0123456789-s1", "s1", ""));
        assert!(owns_rel(&k, "src/x-s1", "s1", "src"));
        // One answer for `/c/ci`, `C:\ci` and `c:/ci` as the home and as the folder.
        let real = "C:/ci/src/x-s1";
        for home in ["/c/ci", "C:\\ci", "c:/ci", "C:/ci"] {
            let rel = rel_under_home(home, real, true).unwrap_or_else(|| panic!("{home}"));
            assert!(owns_rel(&k, &rel, "s1", "src"), "{home}");
        }
        assert_eq!(rel_under_home("c:/ci", "/c/ci/src/x-s1", true).as_deref(), Some("src/x-s1"));
        assert_eq!(rel_under_home("/ci", "/other/src/x-s1", false), None);
    }

    fn make(home: &Path, names: &[&str]) {
        for n in names {
            std::fs::create_dir_all(home.join(n)).unwrap();
        }
    }

    fn aged(path: &Path, days: u64) {
        let t = SystemTime::now() - Duration::from_secs(days * 86_400 + 60);
        open_folder(path).set_modified(t).unwrap();
    }

    /// A handle that may set the folder's modification time: Windows opens a folder only with `FILE_FLAG_BACKUP_SEMANTICS`, and
    /// `File::open` without it is "Access is denied" (run 7904).
    fn open_folder(path: &Path) -> std::fs::File {
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS)
                .open(path)
                .unwrap()
        }
        #[cfg(not(windows))]
        {
            std::fs::File::open(path).unwrap()
        }
    }

    /// `p` below `home` as text with `/` between the parts, whatever the platform joined it with.
    fn rel(p: &Path, home: &Path) -> String {
        p.strip_prefix(home).unwrap().to_string_lossy().replace('\\', "/")
    }

    fn names(v: &[(PathBuf, String)], home: &Path) -> Vec<(String, String)> {
        v.iter().map(|(f, w)| (rel(f, home), w.clone())).collect()
    }

    #[test]
    fn slot_folders_read_src_as_a_star_and_see_every_name() {
        let d = tempfile::tempdir().unwrap();
        make(
            d.path(),
            &[
                "target-a-0123456789-s1",
                "target-a-b-0123456789-s1",
                "target-a-0123456789-s2",
                "src/x-s1",
                "sccache-s1",
                "tools",
            ],
        );
        let k = kept();
        let got: Vec<String> = slot_folders(&k, d.path(), 1).iter().map(|p| rel(p, d.path())).collect();
        assert_eq!(
            got,
            ["target-a-0123456789-s1", "target-a-b-0123456789-s1", "src/x-s1", "sccache-s1"]
        );
    }

    #[test]
    fn an_old_layout_folder_is_due_as_retired_at_once_and_a_new_one_never() {
        let d = tempfile::tempdir().unwrap();
        make(
            d.path(),
            &[
                "target-pitwall-app-s1",
                "target-x-s1",
                "target-pitwall-app-0123456789-s1",
                "target-a-b-c-ffffffffff-s1",
            ],
        );
        let c = ctx(&[]);
        let due = due(&c, &kept(), d.path(), 1, false, SystemTime::now()).unwrap();
        let n = names(&due, d.path());
        let mut got: Vec<&str> = n.iter().map(|(f, _)| f.as_str()).collect();
        got.sort_unstable();
        assert_eq!(
            got,
            ["target-pitwall-app-s1", "target-x-s1"],
            "old names, with and without a hyphen: {n:?}"
        );
        assert!(n.iter().all(|(_, why)| why.starts_with("retired")), "{n:?}");
    }

    #[test]
    fn a_current_folder_is_due_by_age_or_slot_not_as_retired_also_with_all() {
        let d = tempfile::tempdir().unwrap();
        make(d.path(), &["target-a-b-0123456789-s1", "target-x-0123456789-s2"]);
        aged(&d.path().join("target-a-b-0123456789-s1"), 20);
        let c = ctx(&[("PADDOCK_CI_KEEP_SLOTS", "1")]);
        let now = SystemTime::now();
        let one = names(&due(&c, &kept(), d.path(), 1, false, now).unwrap(), d.path());
        assert_eq!(
            one,
            [("target-a-b-0123456789-s1".to_string(), "no job used it for 14 days".to_string())]
        );
        let two = names(&due(&c, &kept(), d.path(), 2, false, now).unwrap(), d.path());
        assert_eq!(two[0].1, "slot s2 is above the 1 kept slots");
        let all = names(&due(&c, &kept(), d.path(), 2, true, now).unwrap(), d.path());
        assert_eq!(all[0].1, "the disk is below its free floor");
        // A fresh folder of a kept slot is not due.
        assert!(due(&ctx(&[]), &kept(), d.path(), 2, false, now).unwrap().is_empty());
    }

    #[test]
    fn legacy_folders_are_slot_one_only_and_names_ending_in_a_slot_are_not_legacy() {
        let d = tempfile::tempdir().unwrap();
        make(d.path(), &["sccache", "src/x", "src/x-s3"]);
        let k = kept();
        let l: Vec<String> = legacy_folders(&k, d.path()).iter().map(|p| rel(p, d.path())).collect();
        assert!(
            l.contains(&"sccache".to_string()) && l.contains(&"src/x".to_string()) && !l.iter().any(|n| n.ends_with("s3")),
            "{l:?}"
        );
    }

    #[test]
    fn age_counts_whole_days_like_find_mtime() {
        let d = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        aged(d.path(), 14);
        assert!(old(d.path(), 13, now), "14 days and a minute old is above 13 whole days");
        assert!(!old(d.path(), 14, now), "...but not above 14: find -mtime +14 wants 15 whole days");
        aged(d.path(), 15);
        assert!(old(d.path(), 14, now));
        aged(d.path(), 2);
        assert!(!old(d.path(), 14, now));
    }

    #[test]
    fn limits_follow_the_environment() {
        assert_eq!(max_slots(&ctx(&[])).unwrap(), 4);
        assert_eq!(max_slots(&ctx(&[("PADDOCK_CI_SLOTS", "3")])).unwrap(), 3);
        assert!(max_slots(&ctx(&[("PADDOCK_CI_SLOTS", "many")])).is_err());
        assert!(max_slots(&ctx(&[("PADDOCK_CI_SLOTS", "0")])).is_err());
        assert_eq!(keep_slots(&ctx(&[("PADDOCK_CI_SLOTS", "3")])).unwrap(), 3);
        assert_eq!(
            keep_slots(&ctx(&[("PADDOCK_CI_SLOTS", "3"), ("PADDOCK_CI_KEEP_SLOTS", "1")])).unwrap(),
            1
        );
        assert_eq!(
            keep_slots(&ctx(&[("PADDOCK_CI_SLOTS", "3"), ("PADDOCK_CI_KEEP_SLOTS", "x")])).unwrap(),
            3
        );
    }
}
