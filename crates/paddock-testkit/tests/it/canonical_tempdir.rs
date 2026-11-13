use paddock_testkit::canonical_tempdir;

#[test]
fn the_path_is_canonical_and_the_folder_is_removed_on_drop() {
    let d = canonical_tempdir();
    let path = d.path().to_path_buf();
    assert_eq!(
        paddock_env::winpath::real(&path).expect("canonicalize"),
        path
    );
    assert!(path.is_dir());
    drop(d);
    assert!(!path.exists());
}

/// The macOS drift (`/var` is a link to `/private/var`) on any Unix: `TMPDIR` names a link.
#[cfg(unix)]
#[test]
fn a_temp_folder_reached_through_a_link_is_resolved() {
    use paddock_testkit::{env_lock, try_canonical_tempdir};
    use std::fs;
    let real = canonical_tempdir();
    fs::create_dir(real.join("real")).expect("real");
    let link = real.join("link");
    std::os::unix::fs::symlink(real.join("real"), &link).expect("symlink");
    let mut env = env_lock();
    env.set("TMPDIR", &link);
    let plain = tempfile::tempdir().expect("plain temp folder");
    assert!(
        plain.path().starts_with(&link),
        "the plain folder keeps the link: {:?}",
        plain.path()
    );
    let canon = try_canonical_tempdir().expect("canonical temp folder");
    assert!(
        canon.path().starts_with(real.join("real")),
        "{:?}",
        canon.path()
    );
    assert!(!canon.path().starts_with(&link));
}
