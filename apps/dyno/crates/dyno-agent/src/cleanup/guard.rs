//! The re-check before every delete (ADR 0005 item 4): the item must still exist, still be a plain file or folder,
//! lie inside its category's folders after resolving the real path, not be in a personal folder, and not have changed
//! since the scan. Nothing is trusted from the scan except which item the user saw.

use super::walk::{measure, Budget};
use dyno_core::cleanup::{admit, Candidate, CategorySpec, CleanupItem};
use dyno_core::time::Timestamp;
use std::path::{Path, PathBuf};

/// The most entries measured when re-checking one item.
const RECHECK_BUDGET: usize = 200_000;

fn canonical_string(p: &Path) -> String {
    std::fs::canonicalize(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Checks `item` against the live file system and returns the real path to remove.
///
/// `roots` are the category's folders as resolved now; `home` is the user's home.
pub fn recheck(item: &CleanupItem, spec: &CategorySpec, roots: &[PathBuf], home: &Path, now: Timestamp) -> Result<PathBuf, String> {
    let path = Path::new(&item.path);
    if !path.is_absolute() || dyno_core::cleanup::has_dot_segment(path) {
        return Err(format!("{} is not a plain full path.", item.path));
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(format!("{} has no parent folder.", item.path));
    };
    let meta = match path.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err("It is no longer there.".into()),
        Err(e) => return Err(format!("It cannot be read: {e}")),
    };
    if meta.file_type().is_symlink() {
        return Err("It is now a link; Dyno never follows or deletes links.".into());
    }
    if meta.is_dir() != item.folder {
        return Err("It changed from a file to a folder (or back) since the scan.".into());
    }
    // A link in the middle of the path would send the delete somewhere else: use the real parent.
    let real_parent = std::fs::canonicalize(parent).map_err(|e| format!("Its folder cannot be read: {e}"))?;
    let real = real_parent.join(name);
    let real_roots: Vec<String> = roots.iter().map(|r| canonical_string(r)).collect();
    let mut budget = Budget::new(RECHECK_BUDGET, 20);
    let live = measure(&real, &meta, None, &mut budget);
    if !live.complete {
        return Err("It is too large to check again safely; try a smaller selection.".into());
    }
    if live.newest > item.modified {
        return Err("It changed after the scan; scan again to see it.".into());
    }
    admit(
        item.id.clone(),
        Candidate {
            path: real.to_string_lossy().into_owned(),
            bytes: live.bytes,
            modified: live.newest,
            folder: meta.is_dir(),
            link: false,
            reason: None,
        },
        &real_roots,
        &canonical_string(home),
        spec.risk,
        spec.min_age_days,
        now,
        "",
    )?;
    Ok(real)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{backdate, now};
    use super::super::walk::modified;
    use super::*;
    use dyno_core::cleanup::pack::{parse, Trust};

    fn spec(min_age: u32) -> CategorySpec {
        parse(
            &format!("format = 1\n[[category]]\nid = \"t\"\nname = \"T\"\ndescription = \"d\"\ngroup = \"developer\"\nrisk = \"review\"\npaths = [\"{{home}}/cachey\"]\nmin_age_days = {min_age}\n"),
            Trust::User,
        )
        .unwrap()
        .remove(0)
    }

    fn item(path: &Path, folder: bool, modified: Timestamp) -> CleanupItem {
        CleanupItem {
            id: "i".into(),
            path: path.to_string_lossy().into_owned(),
            bytes: 1,
            modified,
            folder,
            reason: String::new(),
            selected: false,
        }
    }

    struct Fx {
        home: tempfile::TempDir,
        root: PathBuf,
    }

    fn fx() -> Fx {
        let home = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(home.path()).unwrap().join("cachey");
        std::fs::create_dir_all(&root).unwrap();
        Fx { home, root }
    }

    fn seen(path: &Path) -> Timestamp {
        modified(&path.symlink_metadata().unwrap())
    }

    #[test]
    fn an_untouched_item_inside_its_root_passes() {
        let f = fx();
        let file = f.root.join("a.bin");
        std::fs::write(&file, b"x").unwrap();
        backdate(&file, 10);
        let got = recheck(
            &item(&file, false, seen(&file)),
            &spec(5),
            std::slice::from_ref(&f.root),
            f.home.path(),
            now(),
        )
        .unwrap();
        assert_eq!(got, file);
    }

    #[test]
    fn dotdot_is_refused() {
        let f = fx();
        let sneaky = f.root.join("..").join("cachey").join("a.bin");
        std::fs::write(f.root.join("a.bin"), b"x").unwrap();
        let err = recheck(
            &item(&sneaky, false, Timestamp(0)),
            &spec(0),
            std::slice::from_ref(&f.root),
            f.home.path(),
            now(),
        )
        .unwrap_err();
        assert!(err.contains("plain full path"), "{err}");
        let rel = recheck(
            &item(Path::new("cachey/a.bin"), false, Timestamp(0)),
            &spec(0),
            std::slice::from_ref(&f.root),
            f.home.path(),
            now(),
        );
        assert!(rel.is_err());
    }

    #[test]
    fn outside_the_root_is_refused() {
        let f = fx();
        let elsewhere = f.home.path().join("other.txt");
        std::fs::write(&elsewhere, b"x").unwrap();
        backdate(&elsewhere, 10);
        let err = recheck(
            &item(&elsewhere, false, seen(&elsewhere)),
            &spec(0),
            std::slice::from_ref(&f.root),
            f.home.path(),
            now(),
        )
        .unwrap_err();
        assert!(err.contains("outside"), "{err}");
        // The root itself is not an item.
        assert!(recheck(
            &item(&f.root, true, seen(&f.root)),
            &spec(0),
            std::slice::from_ref(&f.root),
            f.home.path(),
            now()
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_refused_even_when_the_scan_saw_a_file() {
        let f = fx();
        let target = f.home.path().join("precious.txt");
        std::fs::write(&target, b"keep").unwrap();
        let link = f.root.join("looks-like-cache");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let err = recheck(
            &item(&link, false, Timestamp(i64::MAX)),
            &spec(0),
            std::slice::from_ref(&f.root),
            f.home.path(),
            now(),
        )
        .unwrap_err();
        assert!(err.contains("link"), "{err}");
        assert!(target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_middle_of_the_path_is_resolved_and_refused() {
        let f = fx();
        let outside = f.home.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret"), b"x").unwrap();
        backdate(&outside.join("secret"), 10);
        std::os::unix::fs::symlink(&outside, f.root.join("dir-link")).unwrap();
        let via = f.root.join("dir-link").join("secret");
        let err = recheck(
            &item(&via, false, Timestamp(i64::MAX)),
            &spec(0),
            std::slice::from_ref(&f.root),
            f.home.path(),
            now(),
        )
        .unwrap_err();
        assert!(err.contains("outside"), "{err}");
    }

    #[test]
    fn personal_folders_are_refused_unless_they_are_the_root() {
        let f = fx();
        let docs = std::fs::canonicalize(f.home.path()).unwrap().join("Documents");
        std::fs::create_dir_all(&docs).unwrap();
        let file = docs.join("cv.docx");
        std::fs::write(&file, b"x").unwrap();
        backdate(&file, 10);
        let wide = std::fs::canonicalize(f.home.path()).unwrap();
        let err = recheck(&item(&file, false, seen(&file)), &spec(0), &[wide], f.home.path(), now()).unwrap_err();
        assert!(err.contains("personal"), "{err}");
    }

    #[test]
    fn a_change_since_the_scan_is_refused() {
        let f = fx();
        let file = f.root.join("a.bin");
        std::fs::write(&file, b"x").unwrap();
        backdate(&file, 10);
        let scanned = seen(&file);
        std::fs::write(&file, b"changed").unwrap();
        let err = recheck(
            &item(&file, false, scanned),
            &spec(0),
            std::slice::from_ref(&f.root),
            f.home.path(),
            now(),
        )
        .unwrap_err();
        assert!(err.contains("changed after the scan"), "{err}");
        // A file that was replaced by a folder.
        let err = recheck(
            &item(&file, true, Timestamp(i64::MAX)),
            &spec(0),
            std::slice::from_ref(&f.root),
            f.home.path(),
            now(),
        )
        .unwrap_err();
        assert!(err.contains("file to a folder"), "{err}");
    }

    #[test]
    fn the_age_rule_applies_again() {
        let f = fx();
        let file = f.root.join("young.bin");
        std::fs::write(&file, b"x").unwrap();
        let err = recheck(
            &item(&file, false, seen(&file)),
            &spec(5),
            std::slice::from_ref(&f.root),
            f.home.path(),
            now(),
        )
        .unwrap_err();
        assert!(err.contains("changed in the last"), "{err}");
    }

    #[test]
    fn a_missing_item_is_reported() {
        let f = fx();
        let err = recheck(
            &item(&f.root.join("gone"), false, Timestamp(0)),
            &spec(0),
            std::slice::from_ref(&f.root),
            f.home.path(),
            now(),
        )
        .unwrap_err();
        assert!(err.contains("no longer there"));
        assert!(recheck(
            &item(Path::new("/"), true, Timestamp(0)),
            &spec(0),
            std::slice::from_ref(&f.root),
            f.home.path(),
            now()
        )
        .is_err());
    }
}
