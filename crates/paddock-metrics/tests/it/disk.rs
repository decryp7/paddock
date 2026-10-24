//! Volumes: the longest mount wins, free never exceeds total, missing folders use their ancestor.

use paddock_metrics::{Mount, Space, existing_ancestor, free_bytes, volume_free, volume_of};
use paddock_testkit::Scratch;
use std::path::{Path, PathBuf};

fn mount(path: &str, free: u64, total: u64) -> Mount {
    Mount {
        path: PathBuf::from(path),
        free,
        total,
    }
}

#[test]
fn the_longest_matching_mount_point_wins() {
    let mounts = [
        mount("/", 10, 100),
        mount("/data", 20, 200),
        mount("/data/big", 30, 300),
        mount("/dat", 40, 400),
    ];
    assert_eq!(
        volume_free(Path::new("/data/big/x/y"), &mounts),
        Some(Space {
            free: 30,
            total: 300
        })
    );
    assert_eq!(
        volume_free(Path::new("/data/other"), &mounts),
        Some(Space {
            free: 20,
            total: 200
        })
    );
    assert_eq!(
        volume_free(Path::new("/home"), &mounts),
        Some(Space {
            free: 10,
            total: 100
        })
    );
    assert_eq!(
        volume_of(Path::new("/data"), &mounts).unwrap().path,
        PathBuf::from("/data")
    );
}

#[test]
fn no_mount_means_unknown_and_free_is_capped_by_total() {
    assert_eq!(volume_free(Path::new("/x"), &[]), None);
    assert_eq!(
        volume_free(Path::new("/x"), &[mount("/", 500, 100)]),
        Some(Space {
            free: 100,
            total: 100
        })
    );
}

#[test]
fn a_folder_that_does_not_exist_yet_uses_its_nearest_ancestor() {
    let scratch = Scratch::new();
    let missing = scratch.work().join("a").join("b");
    assert_eq!(
        existing_ancestor(&missing),
        Some(std::fs::canonicalize(scratch.work()).unwrap())
    );
    assert_eq!(existing_ancestor(Path::new("")), None);
}

#[test]
fn the_real_volume_of_a_scratch_folder_is_found() {
    let scratch = Scratch::new();
    let space =
        free_bytes(&scratch.work().join("later")).expect("a volume holds the scratch folder");
    assert!(space.total > 0 && space.free <= space.total);
    assert_eq!(free_bytes(Path::new("")), None);
}

#[cfg(windows)]
#[test]
fn verbatim_windows_paths_match_plain_mount_points() {
    let mounts = [mount(r"C:\", 1, 2), mount(r"\\srv\share", 3, 4)];
    assert_eq!(
        volume_free(Path::new(r"\\?\C:\Users\x"), &mounts)
            .unwrap()
            .total,
        2
    );
    assert_eq!(
        volume_free(Path::new(r"\\?\UNC\srv\share\dir"), &mounts)
            .unwrap()
            .total,
        4
    );
    assert_eq!(volume_free(Path::new(r"\\.\COM1"), &mounts), None);
}
