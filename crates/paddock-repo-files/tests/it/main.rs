//! The walk and the hashes on scratch folders.
use paddock_repo_files::{
    RepoFile, RepoFilesError, WalkLimits, hash_bytes, hash_file, hash_files, hash_listing, walk,
};
use std::path::Path;

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn paths(files: &[RepoFile]) -> Vec<&str> {
    files.iter().map(|f| f.path.as_str()).collect()
}

#[test]
fn walk_is_sorted_ignore_aware_and_skips_git() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "b/two.txt", "22");
    put(d.path(), "a.txt", "1");
    put(d.path(), ".hidden", "h");
    put(d.path(), ".git/config", "x");
    put(d.path(), ".gitignore", "/target/\n*.log\n");
    put(d.path(), "target/out.bin", "x");
    put(d.path(), "b/run.log", "x");
    let files = walk(d.path(), WalkLimits::default()).unwrap();
    assert_eq!(
        paths(&files),
        [".gitignore", ".hidden", "a.txt", "b/two.txt"]
    );
    assert_eq!(files[3].size, 2);
}

#[cfg(unix)]
#[test]
fn links_to_folders_are_not_followed() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "real/f.txt", "x");
    std::os::unix::fs::symlink(d.path().join("real"), d.path().join("link")).unwrap();
    assert_eq!(
        paths(&walk(d.path(), WalkLimits::default()).unwrap()),
        ["real/f.txt"]
    );
    // A dangling link has no file to list.
    std::os::unix::fs::symlink(d.path().join("missing"), d.path().join("dangling")).unwrap();
    assert_eq!(walk(d.path(), WalkLimits::default()).unwrap().len(), 1);
}

#[test]
fn walk_is_bounded_and_refuses_a_non_folder() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "a", "1");
    put(d.path(), "b", "1");
    put(d.path(), "x/y/z/deep", "1");
    let err = walk(
        d.path(),
        WalkLimits {
            max_files: 2,
            max_depth: 8,
        },
    )
    .unwrap_err();
    assert!(matches!(err, RepoFilesError::TooManyFiles(2)));
    assert!(err.to_string().contains(".gitignore"));
    let shallow = walk(
        d.path(),
        WalkLimits {
            max_files: 10,
            max_depth: 1,
        },
    )
    .unwrap();
    assert_eq!(paths(&shallow), ["a", "b"]);
    let err = walk(&d.path().join("a"), WalkLimits::default()).unwrap_err();
    assert!(matches!(err, RepoFilesError::NotAFolder(_)));
    assert!(err.to_string().contains("repository root"));
    assert!(walk(&d.path().join("nope"), WalkLimits::default()).is_err());
    assert_eq!(WalkLimits::default().max_depth, 64);
}

#[test]
fn hashes_are_sha256_hex_and_files_are_capped() {
    assert_eq!(
        hash_bytes(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "f", "abc");
    assert_eq!(
        hash_file(&d.path().join("f"), 3).unwrap(),
        hash_bytes(b"abc")
    );
    let err = hash_file(&d.path().join("f"), 2).unwrap_err();
    assert!(matches!(err, RepoFilesError::TooLarge { limit: 2, .. }));
    assert!(err.to_string().contains("raise the limit"));
    let err = hash_file(&d.path().join("missing"), 9).unwrap_err();
    assert!(std::error::Error::source(&err).is_some());
    assert!(std::error::Error::source(&RepoFilesError::TooManyFiles(1)).is_none());
}

#[test]
fn a_folder_is_not_a_hashable_file() {
    let d = tempfile::tempdir().unwrap();
    // Opening a folder succeeds on Unix and reading it fails; on Windows opening fails. Both are typed errors.
    let err = hash_file(d.path(), 10).unwrap_err();
    assert!(matches!(err, RepoFilesError::Io { .. }));
}

#[test]
fn hash_files_keeps_the_order_for_any_thread_count() {
    let d = tempfile::tempdir().unwrap();
    for i in 0..9 {
        put(d.path(), &format!("f{i}"), &format!("content {i}"));
    }
    let files = walk(d.path(), WalkLimits::default()).unwrap();
    let one = hash_files(d.path(), &files, 1, 1 << 20).unwrap();
    for threads in [0, 2, 4, 100] {
        assert_eq!(hash_files(d.path(), &files, threads, 1 << 20).unwrap(), one);
    }
    assert_eq!(one[3], hash_bytes(b"content 3"));
    assert!(hash_files(d.path(), &[], 4, 10).unwrap().is_empty());
    let bad = [
        files[0].clone(),
        RepoFile {
            path: "gone".into(),
            size: 0,
        },
    ];
    assert!(hash_files(d.path(), &bad, 2, 1 << 20).is_err());
}

#[test]
fn a_listing_hash_depends_on_paths_hashes_and_order() {
    let a = vec![
        ("a".to_string(), "1".to_string()),
        ("b".to_string(), "2".to_string()),
    ];
    let mut b = a.clone();
    b.reverse();
    let c = vec![
        ("a".to_string(), "1".to_string()),
        ("b".to_string(), "3".to_string()),
    ];
    assert_eq!(hash_listing(&a), hash_listing(&a));
    assert_ne!(hash_listing(&a), hash_listing(&b));
    assert_ne!(hash_listing(&a), hash_listing(&c));
}

#[cfg(unix)]
#[test]
fn an_unreadable_folder_is_a_typed_error() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "locked/f", "x");
    let locked = d.path().join("locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let got = walk(d.path(), WalkLimits::default());
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    // Running as root can read it anyway; every other user gets the typed error.
    if let Err(e) = got {
        assert!(matches!(e, RepoFilesError::Io { .. }));
        assert!(e.to_string().contains("readable") && std::error::Error::source(&e).is_some());
    }
}

#[test]
fn every_error_names_the_next_step() {
    let io = RepoFilesError::Io {
        path: "p".into(),
        source: std::io::Error::other("boom"),
    };
    assert!(io.to_string().contains("boom") && io.to_string().contains("readable"));
}
