//! Extraction end to end on archives built in memory, in temporary folders only.

use crate::common;

use common::*;
use paddock_archive::{
    ArchiveError, Control, Format, Layout, Limits, Links, Modes, NoControl, Options, extract,
};
use paddock_jobs::{CancelReason, Cancelled};
use std::cell::{Cell, RefCell};
use std::path::Path;

fn run(
    archive: &Path,
    dest: &Path,
    opts: &Options,
) -> Result<paddock_archive::Report, ArchiveError> {
    extract(archive, dest, opts, &NoControl)
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

/// The same tree as zip, tar, tar.gz and tar.xz.
fn all_formats(entries: &[E]) -> Vec<(&'static str, Vec<u8>)> {
    let t = tar_bytes(entries);
    vec![
        ("a.zip", zip_bytes(entries)),
        ("a.tar", t.clone()),
        ("a.tar.gz", gz(&t)),
        ("a.tar.xz", xz(&t)),
    ]
}

#[test]
fn every_format_unpacks_the_same_tree_with_modes_and_a_report() {
    let entries = [
        E::Dir("top"),
        E::File("top/readme.txt", b"hello"),
        E::Exec("top/bin/tool", b"#!x"),
        E::File("top/deep/er/f.bin", &[1, 2, 3]),
    ];
    for (name, bytes) in all_formats(&entries) {
        let d = tempfile::tempdir().unwrap();
        let a = write(d.path(), name, &bytes);
        let dest = d.path().join("out");
        let r = run(&a, &dest, &Options::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            tree(&dest),
            ["top/bin/tool", "top/deep/er/f.bin", "top/readme.txt"],
            "{name}"
        );
        assert_eq!(
            r.files,
            ["top/bin/tool", "top/deep/er/f.bin", "top/readme.txt"],
            "{name}"
        );
        assert_eq!((r.bytes, r.dirs), (11, 1), "{name}");
        assert_eq!(read(&dest.join("top/readme.txt")), "hello");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &str| {
                std::fs::metadata(dest.join(p))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777
            };
            assert_eq!(
                (mode("top/bin/tool"), mode("top/readme.txt")),
                (0o755, 0o644),
                "{name}"
            );
        }
        assert_eq!(
            others(d.path(), &[name, "out"]),
            Vec::<String>::new(),
            "{name}: no staging left"
        );
    }
}

#[test]
fn modes_can_make_everything_executable() {
    let d = tempfile::tempdir().unwrap();
    let a = write(d.path(), "a.tar", &tar_bytes(&[E::File("x", b"1")]));
    let opts = Options {
        modes: Modes::AllExecutable,
        ..Options::default()
    };
    run(&a, &d.path().join("o"), &opts).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(d.path().join("o/x"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
}

#[test]
fn the_format_comes_from_the_content_not_the_name() {
    let d = tempfile::tempdir().unwrap();
    let a = write(
        d.path(),
        "download.bin",
        &zip_bytes(&[E::File("a.txt", b"x")]),
    );
    assert_eq!(Format::sniff(&a).unwrap(), Format::Zip);
    run(&a, &d.path().join("o"), &Options::default()).unwrap();
    assert_eq!(
        Format::sniff(&write(d.path(), "e.zip", &zip_bytes(&[]))).unwrap(),
        Format::Zip
    );
    for (name, bytes, fmt) in [
        ("t", tar_bytes(&[E::File("a", b"x")]), Format::Tar),
        ("g", gz(b"x"), Format::TarGz),
        ("x", xz(b"x"), Format::TarXz),
    ] {
        assert_eq!(Format::sniff(&write(d.path(), name, &bytes)).unwrap(), fmt);
    }
}

#[test]
fn not_an_archive_is_unsupported_and_a_missing_file_is_an_io_error() {
    let d = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        ("junk.zip", b"this is text, not an archive".to_vec()),
        ("empty.tar", vec![]),
        ("short", vec![0x50]),
    ] {
        let a = write(d.path(), name, &bytes);
        let e = run(&a, &d.path().join("o"), &Options::default()).unwrap_err();
        assert!(
            matches!(e, ArchiveError::Unsupported(ref m) if m.contains(name)),
            "{name}: {e}"
        );
    }
    assert!(matches!(
        run(
            &d.path().join("none.zip"),
            &d.path().join("o"),
            &Options::default()
        ),
        Err(ArchiveError::Io { .. })
    ));
    assert!(!d.path().join("o").exists());
}

#[test]
fn damaged_and_truncated_archives_are_corrupt_and_leave_nothing() {
    let d = tempfile::tempdir().unwrap();
    let t = tar_bytes(&[E::File("big", &vec![7u8; 4000])]);
    let z = zip_bytes(&[E::File("big", &vec![7u8; 4000])]);
    let mut bad_gz = gz(&t);
    bad_gz.truncate(bad_gz.len() / 2);
    let mut bad_xz = xz(&t);
    bad_xz.truncate(bad_xz.len() - 20);
    let mut bad_tar = t.clone();
    bad_tar.truncate(700);
    let mut bad_zip = z.clone();
    bad_zip.truncate(z.len() - 30);
    let mut bad_header = t.clone();
    bad_header[148] = b'z'; // a checksum that is not a number
    for (name, bytes) in [
        ("a.tar.gz", bad_gz),
        ("a.tar.xz", bad_xz),
        ("a.tar", bad_tar),
        ("a.zip", bad_zip),
        ("b.tar", bad_header),
    ] {
        let a = write(d.path(), name, &bytes);
        let e = run(&a, &d.path().join("o"), &Options::default()).unwrap_err();
        assert!(matches!(e, ArchiveError::Corrupt(_)), "{name}: {e:?}");
        assert!(e.to_string().contains("damaged"));
        assert!(!d.path().join("o").exists(), "{name}");
        assert_eq!(
            others(
                d.path(),
                &["a.tar.gz", "a.tar.xz", "a.tar", "a.zip", "b.tar"]
            ),
            Vec::<String>::new(),
            "{name}: no staging left"
        );
    }
}

#[test]
fn path_traversal_and_friends_are_refused_and_nothing_escapes() {
    for bad in [
        "../evil.txt",
        "a/../../evil.txt",
        "/abs/evil.txt",
        "C:evil.txt",
        "a\\..\\evil.txt",
        "CON",
        "x/aux.txt",
        "dots..",
    ] {
        for (fmt, bytes) in [
            (
                "tar",
                tar_bytes(&[E::File("ok.txt", b"1"), E::File(bad, b"x")]),
            ),
            (
                "zip",
                zip_bytes(&[E::File("ok.txt", b"1"), E::File(bad, b"x")]),
            ),
        ] {
            let d = tempfile::tempdir().unwrap();
            let sandbox = d.path().join("sandbox");
            std::fs::create_dir(&sandbox).unwrap();
            let a = write(d.path(), "a.bin", &bytes);
            let dest = sandbox.join("out");
            let e = run(&a, &dest, &Options::default()).unwrap_err();
            assert!(
                matches!(e, ArchiveError::Unsafe { .. }),
                "{fmt} {bad}: {e:?}"
            );
            assert!(e.to_string().contains("refused"));
            assert!(
                !dest.exists(),
                "{fmt} {bad}: the destination is not created"
            );
            assert_eq!(
                others(&sandbox, &[]),
                Vec::<String>::new(),
                "{fmt} {bad}: no staging"
            );
            assert_eq!(
                others(d.path(), &["a.bin", "sandbox"]),
                Vec::<String>::new(),
                "{fmt} {bad}: nothing outside"
            );
        }
    }
}

#[test]
fn a_failed_extract_leaves_an_existing_destination_untouched_and_a_good_one_replaces_it() {
    let d = tempfile::tempdir().unwrap();
    let dest = d.path().join("rt");
    std::fs::create_dir(&dest).unwrap();
    std::fs::write(dest.join("old.txt"), "old").unwrap();
    let bad = write(
        d.path(),
        "bad.tar",
        &tar_bytes(&[E::File("new.txt", b"n"), E::File("../x", b"x")]),
    );
    assert!(run(&bad, &dest, &Options::default()).is_err());
    assert_eq!(tree(&dest), ["old.txt"]);
    let good = write(
        d.path(),
        "good.tar",
        &tar_bytes(&[E::File("new.txt", b"n")]),
    );
    run(&good, &dest, &Options::default()).unwrap();
    assert_eq!(
        tree(&dest),
        ["new.txt"],
        "the old content is gone: the folder was swapped"
    );
    assert_eq!(
        others(d.path(), &["rt", "bad.tar", "good.tar"]),
        Vec::<String>::new(),
        "the moved-aside folder is removed"
    );
}

#[test]
fn a_destination_that_is_a_file_or_under_a_file_is_an_error_and_cleans_up() {
    let d = tempfile::tempdir().unwrap();
    let a = write(d.path(), "a.tar", &tar_bytes(&[E::File("x", b"1")]));
    let file = write(d.path(), "dest", b"i am a file");
    let e = run(&a, &file, &Options::default()).unwrap_err();
    assert!(matches!(e, ArchiveError::Io { op: "replace", .. }), "{e:?}");
    assert_eq!(read(&file), "i am a file");
    assert!(matches!(
        run(&a, &file.join("sub"), &Options::default()),
        Err(ArchiveError::Io { .. })
    ));
    assert_eq!(others(d.path(), &["a.tar", "dest"]), Vec::<String>::new());
}

#[test]
fn limits_are_enforced_while_writing() {
    let d = tempfile::tempdir().unwrap();
    let many: Vec<String> = (0..20).map(|i| format!("f{i}")).collect();
    let entries: Vec<E> = many.iter().map(|n| E::File(n, b"x")).collect();
    let opts = |l: Limits| Options {
        limits: l,
        ..Options::default()
    };
    for (name, bytes) in all_formats(&entries) {
        let a = write(d.path(), name, &bytes);
        let e = run(
            &a,
            &d.path().join("o"),
            &opts(Limits {
                max_entries: 5,
                ..Limits::default()
            }),
        )
        .unwrap_err();
        assert!(
            matches!(
                e,
                ArchiveError::Limit {
                    what: "entries",
                    limit: 5
                }
            ),
            "{name}: {e:?}"
        );
    }
    let big = [E::File("big", &vec![1u8; 5000])];
    for (name, bytes) in all_formats(&big) {
        let a = write(d.path(), name, &bytes);
        let e = run(
            &a,
            &d.path().join("o"),
            &opts(Limits {
                max_file_bytes: 1000,
                ..Limits::default()
            }),
        )
        .unwrap_err();
        assert!(
            matches!(
                e,
                ArchiveError::Limit {
                    what: "file size",
                    limit: 1000
                }
            ),
            "{name}: {e:?}"
        );
        let e = run(
            &a,
            &d.path().join("o"),
            &opts(Limits {
                max_total_bytes: 1000,
                ratio_floor_bytes: 1 << 30,
                ..Limits::default()
            }),
        )
        .unwrap_err();
        assert!(
            matches!(
                e,
                ArchiveError::Limit {
                    what: "total size",
                    limit: 1000
                }
            ),
            "{name}: {e:?}"
        );
    }
    assert!(!d.path().join("o").exists());
    let deep = write(
        d.path(),
        "deep.tar",
        &tar_bytes(&[E::File("a/b/c/d", b"x")]),
    );
    let e = run(
        &deep,
        &d.path().join("o"),
        &opts(Limits {
            max_depth: 2,
            ..Limits::default()
        }),
    )
    .unwrap_err();
    assert!(matches!(
        e,
        ArchiveError::Limit {
            what: "folder depth",
            ..
        }
    ));
}

#[test]
fn an_expansion_bomb_is_stopped_by_the_ratio_in_every_format() {
    let d = tempfile::tempdir().unwrap();
    let zeros = vec![0u8; 4 << 20];
    let entries = [E::File("zeros", &zeros)];
    let opts = Options {
        limits: Limits {
            max_ratio: 50,
            ratio_floor_bytes: 1024,
            ..Limits::default()
        },
        ..Options::default()
    };
    for (name, bytes) in all_formats(&entries)
        .into_iter()
        .filter(|(n, _)| *n == "a.zip" || *n == "a.tar.gz")
    {
        let a = write(d.path(), name, &bytes);
        let e = run(&a, &d.path().join("o"), &opts).expect_err(&format!("{name} {}", bytes.len()));
        assert!(
            matches!(
                e,
                ArchiveError::Limit {
                    what: "expansion ratio",
                    limit: 50
                }
            ),
            "{name}: {e:?}"
        );
        assert!(e.to_string().contains("expansion ratio"));
    }
    assert!(!d.path().join("o").exists());
    // The xz reader stops while expanding (it is stored here: the test encoder does not compress), and removes the expanded tar.
    let xz = write(d.path(), "b.tar.xz", &xz(&tar_bytes(&entries)));
    let tight = Options {
        limits: Limits {
            max_entries: 4,
            max_total_bytes: 1000,
            ratio_floor_bytes: 1 << 30,
            ..Limits::default()
        },
        ..Options::default()
    };
    let e = run(&xz, &d.path().join("o"), &tight).unwrap_err();
    assert!(
        matches!(
            e,
            ArchiveError::Limit {
                what: "total size",
                limit: 1000
            }
        ),
        "{e:?}"
    );
    assert_eq!(
        others(d.path(), &["a.zip", "a.tar.gz", "a.tar.xz", "b.tar.xz"]),
        Vec::<String>::new(),
        "the expanded tar is removed too"
    );
}

#[test]
fn duplicates_and_case_only_duplicates_are_refused() {
    let d = tempfile::tempdir().unwrap();
    for names in [["a.txt", "a.txt"], ["a.txt", "A.TXT"]] {
        let a = write(
            d.path(),
            "a.tar",
            &tar_bytes(&[E::File(names[0], b"1"), E::File(names[1], b"2")]),
        );
        assert!(
            matches!(
                run(&a, &d.path().join("o"), &Options::default()),
                Err(ArchiveError::Duplicate(_))
            ),
            "{names:?}"
        );
    }
    let a = write(
        d.path(),
        "b.tar",
        &tar_bytes(&[E::Dir("d"), E::Dir("d"), E::File("d/x", b"1")]),
    );
    assert!(
        run(&a, &d.path().join("o"), &Options::default()).is_ok(),
        "a folder named twice is fine"
    );
    let clash = write(
        d.path(),
        "c.tar",
        &tar_bytes(&[E::File("d", b"1"), E::File("d/x", b"2")]),
    );
    assert!(matches!(
        run(&clash, &d.path().join("o2"), &Options::default()),
        Err(ArchiveError::Io { .. })
    ));
}

fn with_links() -> Vec<E<'static>> {
    vec![
        E::File("lib/libx.so.1", b"LIB"),
        E::Sym("lib/libx.so", "libx.so.1"),
        E::Sym("lib/libx", "libx.so"),
        E::Hard("lib/hard.so", "lib/libx.so.1"),
        E::Sym("lib/missing", "not-in-the-archive"),
    ]
}

#[test]
fn links_are_refused_by_default() {
    let d = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        ("a.tar", tar_bytes(&with_links())),
        ("a.zip", zip_bytes(&[E::File("f", b"1"), E::Sym("l", "f")])),
    ] {
        let a = write(d.path(), name, &bytes);
        let e = run(&a, &d.path().join("o"), &Options::default()).unwrap_err();
        assert!(
            matches!(e, ArchiveError::Unsafe { ref reason, .. } if reason.contains("links are not allowed")),
            "{name}: {e:?}"
        );
    }
}

#[test]
fn skip_ignores_links_and_copy_turns_them_into_files() {
    let d = tempfile::tempdir().unwrap();
    let a = write(d.path(), "a.tar", &tar_bytes(&with_links()));
    let skip = Options {
        links: Links::Skip,
        ..Options::default()
    };
    let r = run(&a, &d.path().join("s"), &skip).unwrap();
    assert_eq!(
        (r.files.clone(), r.skipped),
        (vec!["lib/libx.so.1".to_string()], 4)
    );
    let copy = Options {
        links: Links::Copy,
        ..Options::default()
    };
    let r = run(&a, &d.path().join("c"), &copy).unwrap();
    assert_eq!(
        r.files,
        ["lib/hard.so", "lib/libx", "lib/libx.so", "lib/libx.so.1"]
    );
    assert_eq!(r.dropped_links, ["lib/missing"]);
    for f in &r.files {
        assert_eq!(read(&d.path().join("c").join(f)), "LIB", "{f}");
        #[cfg(unix)]
        assert!(
            !std::fs::symlink_metadata(d.path().join("c").join(f))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
    assert_eq!(r.bytes, 12);
    let z = write(
        d.path(),
        "a.zip",
        &zip_bytes(&[E::File("f", b"1"), E::Sym("l", "f")]),
    );
    assert_eq!(
        run(&z, &d.path().join("z"), &copy).unwrap().files,
        ["f", "l"]
    );
}

#[test]
fn a_copied_link_counts_against_the_size_cap() {
    let d = tempfile::tempdir().unwrap();
    let a = write(
        d.path(),
        "a.tar",
        &tar_bytes(&[
            E::File("f", &[9u8; 600]),
            E::Sym("l1", "f"),
            E::Sym("l2", "f"),
        ]),
    );
    let opts = Options {
        links: Links::Copy,
        limits: Limits {
            max_total_bytes: 1000,
            ratio_floor_bytes: 1 << 30,
            ..Limits::default()
        },
        ..Options::default()
    };
    assert!(matches!(
        run(&a, &d.path().join("o"), &opts),
        Err(ArchiveError::Limit {
            what: "total size",
            ..
        })
    ));
}

#[test]
fn links_that_point_out_of_the_tree_are_refused_whatever_the_policy() {
    let d = tempfile::tempdir().unwrap();
    for target in [
        "/etc/passwd",
        "../../etc/passwd",
        "../../outside",
        "C:\\x",
        "",
    ] {
        for links in [Links::Copy, Links::Keep] {
            let a = write(
                d.path(),
                "a.tar",
                &tar_bytes(&[E::File("a/f", b"1"), E::Sym("a/l", target)]),
            );
            let e = run(
                &a,
                &d.path().join("o"),
                &Options {
                    links,
                    ..Options::default()
                },
            )
            .unwrap_err();
            assert!(
                matches!(e, ArchiveError::Unsafe { .. }),
                "{target:?} {links:?}: {e:?}"
            );
        }
    }
    // A hard link must name a safe archive path.
    let a = write(d.path(), "h.tar", &tar_bytes(&[E::Hard("l", "../x")]));
    assert!(matches!(
        run(
            &a,
            &d.path().join("o"),
            &Options {
                links: Links::Copy,
                ..Options::default()
            }
        ),
        Err(ArchiveError::Unsafe { .. })
    ));
    assert!(!d.path().join("o").exists());
}

#[test]
fn copy_allows_dotdot_inside_the_tree_but_keep_does_not() {
    let d = tempfile::tempdir().unwrap();
    let a = write(
        d.path(),
        "a.tar",
        &tar_bytes(&[E::File("bin/x", b"X"), E::Sym("lib/l", "../bin/x")]),
    );
    let r = run(
        &a,
        &d.path().join("c"),
        &Options {
            links: Links::Copy,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(read(&d.path().join("c/lib/l")), "X");
    assert_eq!(r.files, ["bin/x", "lib/l"]);
    assert!(matches!(
        run(
            &a,
            &d.path().join("k"),
            &Options {
                links: Links::Keep,
                ..Options::default()
            }
        ),
        Err(ArchiveError::Unsafe { .. })
    ));
}

#[test]
fn keep_creates_relative_symbolic_links_that_stay_inside() {
    let d = tempfile::tempdir().unwrap();
    let a = write(
        d.path(),
        "a.tar",
        &tar_bytes(&[
            E::File("lib/libx.so.1", b"LIB"),
            E::Sym("lib/libx.so", "libx.so.1"),
            E::Hard("lib/hard", "lib/libx.so.1"),
        ]),
    );
    let r = run(
        &a,
        &d.path().join("o"),
        &Options {
            links: Links::Keep,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(r.files, ["lib/hard", "lib/libx.so", "lib/libx.so.1"]);
    assert_eq!(
        read(&d.path().join("o/lib/hard")),
        "LIB",
        "a hard link becomes a copy"
    );
    assert_eq!(read(&d.path().join("o/lib/libx.so")), "LIB");
    #[cfg(unix)]
    assert_eq!(
        std::fs::read_link(d.path().join("o/lib/libx.so")).unwrap(),
        Path::new("libx.so.1")
    );
}

#[test]
fn devices_are_never_created_but_their_names_are_still_checked() {
    let d = tempfile::tempdir().unwrap();
    let a = write(
        d.path(),
        "a.tar",
        &tar_bytes(&[E::File("f", b"1"), E::Dev("dev/null")]),
    );
    let r = run(&a, &d.path().join("o"), &Options::default()).unwrap();
    assert_eq!((r.files, r.skipped), (vec!["f".to_string()], 1));
    let a = write(d.path(), "b.tar", &tar_bytes(&[E::Dev("../dev")]));
    assert!(matches!(
        run(&a, &d.path().join("o"), &Options::default()),
        Err(ArchiveError::Unsafe { .. })
    ));
}

#[test]
fn layouts_and_filters_pick_and_place_entries() {
    let d = tempfile::tempdir().unwrap();
    let entries = [
        E::Dir("pkg-1"),
        E::File("pkg-1/bin/tool", b"T"),
        E::File("pkg-1/lib/libz.so", b"Z"),
        E::File("pkg-1/docs/readme.md", b"R"),
        E::File("pkg-1/a/b/c/deep.dll", b"D"),
        E::File("top.txt", b"t"),
    ];
    for (name, bytes) in all_formats(&entries) {
        let a = write(d.path(), name, &bytes);
        let o = |layout, filter: Option<fn(&str) -> bool>| {
            let mut opts = Options {
                layout,
                ..Options::default()
            };
            if let Some(f) = filter {
                opts = opts.filter(f);
            }
            opts
        };
        let strip = d.path().join(format!("strip-{name}"));
        let r = run(&a, &strip, &o(Layout::Strip(1), None)).unwrap();
        assert_eq!(
            r.files,
            [
                "a/b/c/deep.dll",
                "bin/tool",
                "docs/readme.md",
                "lib/libz.so"
            ],
            "{name}: top.txt has too few components"
        );
        let flat = d.path().join(format!("flat-{name}"));
        let r = run(
            &a,
            &flat,
            &o(
                Layout::Flat { max_depth: 3 },
                Some(|n: &str| n.ends_with(".so") || n == "tool"),
            ),
        )
        .unwrap();
        assert_eq!(r.files, ["libz.so", "tool"], "{name}");
        assert!(r.skipped >= 3);
    }
}

/// Records what the extraction told its Control, and can cancel after a number of checks.
struct Probe {
    phases: RefCell<Vec<(String, Option<u64>)>>,
    progress: RefCell<Vec<u64>>,
    checks: Cell<u32>,
    cancel_after: u32,
}

impl Probe {
    fn new(cancel_after: u32) -> Probe {
        Probe {
            phases: RefCell::new(vec![]),
            progress: RefCell::new(vec![]),
            checks: Cell::new(0),
            cancel_after,
        }
    }
}

impl Control for Probe {
    fn phase(&self, id: &str, _: &str, total: Option<u64>) {
        self.phases.borrow_mut().push((id.into(), total));
    }
    fn progress(&self, done: u64) {
        self.progress.borrow_mut().push(done);
    }
    fn check(&self) -> Result<(), Cancelled> {
        self.checks.set(self.checks.get() + 1);
        if self.checks.get() > self.cancel_after {
            Err(Cancelled(CancelReason::User))
        } else {
            Ok(())
        }
    }
}

#[test]
fn progress_has_labelled_phases_on_their_own_scale_and_never_goes_backwards() {
    let d = tempfile::tempdir().unwrap();
    let entries = [
        E::File("a", &vec![1u8; 70_000]),
        E::File("b", &vec![2u8; 70_000]),
        E::File("c", b"x"),
    ];
    for (name, bytes) in all_formats(&entries) {
        let a = write(d.path(), name, &bytes);
        let p = Probe::new(u32::MAX);
        extract(
            &a,
            &d.path().join(format!("o-{name}")),
            &Options::default(),
            &p,
        )
        .unwrap();
        let phases = p.phases.borrow();
        let ids: Vec<&str> = phases.iter().map(|(i, _)| i.as_str()).collect();
        assert_eq!(
            ids,
            if name.ends_with("xz") {
                vec!["expand", "unpack"]
            } else {
                vec!["unpack"]
            },
            "{name}"
        );
        let total = phases.last().unwrap().1.expect("the unpack total is known");
        let prog = p.progress.borrow();
        assert!(
            prog.windows(2).all(|w| w[0] < w[1]),
            "{name}: strictly increasing {prog:?}"
        );
        assert_eq!(*prog.last().unwrap(), total, "{name}: ends at the total");
    }
}

#[test]
fn cancel_at_any_point_leaves_no_trace_and_the_old_destination() {
    let d = tempfile::tempdir().unwrap();
    let entries = [
        E::File("a", &vec![1u8; 200_000]),
        E::File("b", &vec![2u8; 200_000]),
        E::File("c", &vec![3u8; 200_000]),
    ];
    for (name, bytes) in all_formats(&entries) {
        let a = write(d.path(), name, &bytes);
        let dest = d.path().join("rt");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("keep"), "k").unwrap();
        // Find how many checks a full run makes, then cancel at every earlier point.
        let full = Probe::new(u32::MAX);
        extract(&a, &d.path().join("full"), &Options::default(), &full).unwrap();
        std::fs::remove_dir_all(d.path().join("full")).unwrap();
        let n = full.checks.get();
        assert!(n > 3, "{name}: {n}");
        for at in (0..n).step_by(((n / 6).max(1)) as usize) {
            let e = extract(&a, &dest, &Options::default(), &Probe::new(at)).unwrap_err();
            assert!(
                matches!(e, ArchiveError::Cancelled(CancelReason::User)),
                "{name} at {at}: {e:?}"
            );
            assert_eq!(tree(&dest), ["keep"], "{name} at {at}");
            assert_eq!(
                others(
                    d.path(),
                    &[name, "rt", "a.zip", "a.tar", "a.tar.gz", "a.tar.xz"]
                ),
                Vec::<String>::new(),
                "{name} at {at}: no staging"
            );
        }
        assert!(e_display(&ArchiveError::Cancelled(CancelReason::Deadline)).contains("cancelled"));
    }
}

fn e_display(e: &ArchiveError) -> String {
    e.to_string()
}

#[test]
fn error_messages_say_what_to_do() {
    let io = ArchiveError::Io {
        op: "write",
        path: "/x".into(),
        source: std::io::Error::other("full"),
    };
    assert!(io.to_string().contains("disk space"));
    assert!(
        ArchiveError::Duplicate("a".into())
            .to_string()
            .contains("twice")
    );
    assert!(
        ArchiveError::Limit {
            what: "entries",
            limit: 3
        }
        .to_string()
        .contains("entries")
    );
    let j: paddock_jobs::JobError = ArchiveError::Cancelled(CancelReason::Deadline).into();
    assert!(matches!(
        j,
        paddock_jobs::JobError::Cancelled(CancelReason::Deadline)
    ));
    let j: paddock_jobs::JobError = ArchiveError::Corrupt("x".into()).into();
    assert!(matches!(j, paddock_jobs::JobError::Failed(m) if m.contains("damaged")));
}

#[test]
fn sweep_removes_only_old_leftovers_of_that_destination() {
    let d = tempfile::tempdir().unwrap();
    let dest = d.path().join("rt");
    let old = [
        "rt.paddock-staging-1-0",
        "rt.paddock-old-1-1",
        "rt.paddock-expand-1-2",
    ];
    for n in old {
        let p = d.path().join(format!(".{n}"));
        std::fs::create_dir(&p).unwrap();
        std::fs::write(p.join("f"), "x").ok();
    }
    std::fs::write(d.path().join(".rt.paddock-expand-1-3"), "x").unwrap();
    std::fs::create_dir(d.path().join(".rt.paddock-staging-9-9")).unwrap();
    std::fs::create_dir(d.path().join("other")).unwrap();
    std::fs::create_dir(d.path().join(".other.paddock-staging-1-0")).unwrap();
    // Nothing is old enough yet.
    assert_eq!(
        paddock_archive::sweep_stale(&dest, std::time::Duration::from_secs(3600)),
        0
    );
    let ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
    for n in old.iter().chain(&["rt.paddock-expand-1-3"]) {
        let p = d.path().join(format!(".{n}"));
        std::fs::File::open(&p).unwrap().set_modified(ago).unwrap();
    }
    assert_eq!(
        paddock_archive::sweep_stale(&dest, std::time::Duration::from_secs(3600)),
        4
    );
    assert_eq!(
        others(d.path(), &[]),
        [
            ".other.paddock-staging-1-0",
            ".rt.paddock-staging-9-9",
            "other"
        ]
    );
    assert_eq!(
        paddock_archive::sweep_stale(&d.path().join("missing/dir"), std::time::Duration::ZERO),
        0
    );
}
