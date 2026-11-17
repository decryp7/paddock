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

/// The macOS drift (`/var` is a link to `/private/var`) on any Unix: the temp folder is named through a link.
#[cfg(unix)]
#[test]
fn a_temp_folder_reached_through_a_link_is_resolved() {
    use paddock_testkit::scratch::try_canonical_tempdir_in;
    use std::fs;
    let real = canonical_tempdir();
    fs::create_dir(real.join("real")).expect("real");
    let link = real.join("link");
    std::os::unix::fs::symlink(real.join("real"), &link).expect("symlink");
    // The temp folder named through the link, as a `TMPDIR` that is a link names it.
    let plain = tempfile::Builder::new()
        .tempdir_in(&link)
        .expect("plain temp folder");
    assert!(
        plain.path().starts_with(&link),
        "the plain folder keeps the link: {:?}",
        plain.path()
    );
    let canon = try_canonical_tempdir_in(&link).expect("canonical temp folder");
    assert!(
        canon.path().starts_with(real.join("real")),
        "{:?}",
        canon.path()
    );
    assert!(!canon.path().starts_with(&link));
}
