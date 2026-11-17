use paddock_testkit::{Scratch, ScratchError};
use std::fs;

#[test]
fn has_work_home_and_tmp_and_is_removed_on_drop() {
    let s = Scratch::new();
    let root = s
        .work()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    assert!(s.work().is_dir() && s.home().is_dir() && s.tmp().is_dir());
    assert!(root.exists());
    s.finish().expect("tmp is empty");
    assert!(!root.exists(), "the folder is deleted");
}

#[test]
fn write_read_and_listing_are_sorted_and_relative() {
    let s = Scratch::new();
    s.write("b/two.txt", "2").expect("write");
    s.write("a.txt", b"1").expect("write");
    assert_eq!(s.read("b/two.txt").expect("read"), "2");
    assert_eq!(s.listing().expect("list"), ["a.txt", "b/", "b/two.txt"]);
}

#[test]
fn paths_cannot_leave_the_work_folder() {
    let s = Scratch::new();
    for bad in ["", "/etc/passwd", "../x", "a/../../x"] {
        assert!(
            matches!(s.try_path(bad), Err(ScratchError::Escapes(_))),
            "{bad:?}"
        );
        assert!(matches!(s.write(bad, "x"), Err(ScratchError::Escapes(_))));
    }
    assert!(s.try_path("./ok/file").is_ok());
    assert_eq!(
        s.read("missing")
            .map_err(|e| matches!(e, ScratchError::Io(_))),
        Err(true)
    );
}

#[test]
fn a_leftover_in_tmp_is_reported_by_finish() {
    let s = Scratch::new();
    fs::create_dir(s.tmp().join("job-1")).expect("mkdir");
    fs::write(s.tmp().join("job-1/log"), "x").expect("write");
    fs::write(s.tmp().join("stray.tmp"), "x").expect("write");
    let left = s.finish().expect_err("leftovers");
    assert_eq!(left.0, ["job-1/", "job-1/log", "stray.tmp"]);
    assert!(
        left.to_string()
            .contains("3 item(s) left in the temp folder")
    );
}

#[test]
fn dropping_with_a_leftover_fails_the_test() {
    let result = std::panic::catch_unwind(|| {
        let s = Scratch::new();
        fs::write(s.tmp().join("stray.tmp"), "x").expect("write");
    });
    let message = match result {
        Err(e) => e.downcast_ref::<String>().cloned().unwrap_or_default(),
        Ok(()) => String::new(),
    };
    assert!(message.contains("stray.tmp"), "{message}");
}

#[test]
fn a_panicking_test_is_not_hidden_by_the_leftover_check() {
    // The original failure must be the one reported: the drop stays quiet while panicking.
    let result = std::panic::catch_unwind(|| {
        let s = Scratch::new();
        fs::write(s.tmp().join("stray.tmp"), "x").expect("write");
        panic!("the real failure");
    });
    let message = result
        .err()
        .and_then(|e| e.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default();
    assert_eq!(message, "the real failure");
}

#[test]
fn an_unreadable_tmp_counts_as_a_leftover() {
    let s = Scratch::new();
    fs::remove_dir(s.tmp()).expect("remove tmp");
    let left = s.finish().expect_err("the check never passes by accident");
    assert!(left.0[0].starts_with("(unreadable"));
}

#[test]
fn a_failed_test_keeps_its_folder_only_when_asked() {
    let mut env = paddock_testkit::env_lock();
    let root_of = |_keep: bool| {
        let root = std::sync::Mutex::new(std::path::PathBuf::new());
        let _ = std::panic::catch_unwind(|| {
            let s = Scratch::new();
            *root.lock().unwrap_or_else(|e| e.into_inner()) = s.work();
            panic!("failing test");
        });
        root.into_inner().unwrap_or_default()
    };
    env.remove(paddock_testkit::scratch::KEEP_VAR);
    assert!(!root_of(false).exists(), "deleted by default");
    env.set(paddock_testkit::scratch::KEEP_VAR, "1");
    let kept = root_of(true);
    assert!(kept.exists(), "kept for inspection");
    let _ = fs::remove_dir_all(kept.parent().unwrap_or(&kept));
}

#[test]
fn errors_read_as_sentences_and_path_panics_on_a_mistake() {
    let s = Scratch::default();
    let escapes = s
        .try_path("../x")
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(escapes.contains("not a plain relative path"), "{escapes}");
    let io = s
        .read("missing")
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(io.starts_with("scratch folder: "), "{io}");
    let message = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.path("/abs")))
        .err()
        .and_then(|e| e.downcast_ref::<String>().cloned());
    assert!(message.is_some_and(|m| m.contains("not a plain relative path")));
    let from_io: ScratchError = std::io::Error::other("boom").into();
    assert!(matches!(from_io, ScratchError::Io(_)));
}

/// A group other than `egid` the caller may give a folder to: a supplementary group, or any other id for root. `None` when the
/// caller has no other group (then the condition below cannot be made and the test only checks the plain case).
#[cfg(unix)]
fn another_group(egid: u32) -> Option<u32> {
    // SAFETY: getgroups with a null list returns the count; the second call fills a list of that size.
    let n = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    let mut groups = vec![0 as libc::gid_t; n.max(0) as usize];
    let n = unsafe { libc::getgroups(n, groups.as_mut_ptr()) };
    groups.truncate(n.max(0) as usize);
    groups
        .into_iter()
        .find(|g| *g != egid)
        .or_else(|| (unsafe { libc::geteuid() } == 0).then_some(egid + 1))
}

#[cfg(unix)]
#[test]
fn folders_are_in_the_callers_group_even_when_the_parent_is_in_another() {
    use std::os::unix::fs::{MetadataExt, chown};
    // On macOS a new folder takes its parent's group: a temp folder on a disk whose folders are group `admin` made Dyno's
    // "folder is in a shared group" check fail in tests that passed elsewhere (twice).
    let egid = unsafe { libc::getegid() };
    let parent = tempfile::tempdir().unwrap();
    if let Some(other) = another_group(egid) {
        chown(parent.path(), None, Some(other)).unwrap();
        assert_eq!(
            fs::metadata(parent.path()).unwrap().gid(),
            other,
            "the parent is in the other group"
        );
    }
    let s = Scratch::try_new_in(parent.path()).unwrap();
    for dir in [
        s.work(),
        s.home(),
        s.tmp(),
        s.work().parent().unwrap().to_path_buf(),
    ] {
        assert_eq!(
            fs::metadata(&dir).unwrap().gid(),
            egid,
            "{} is in the caller's group",
            dir.display()
        );
    }
    // A file written into it takes the folder's group too.
    let file = s.write("a/b.txt", "x").unwrap();
    assert_eq!(fs::metadata(&file).unwrap().gid(), egid);
    assert_eq!(fs::metadata(file.parent().unwrap()).unwrap().gid(), egid);
}
