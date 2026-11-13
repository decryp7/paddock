//! The scratch file with real files, in the target directory only (ER-14): it cannot be left behind and never replaces a file.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dyno_bench::disk::driver::{AlignedBuf, BlockIo, ALIGN};
use dyno_bench::disk::scratch::{Scratch, SCRATCH_NAME};

fn dir() -> paddock_testkit::PrivateTemp {
    paddock_testkit::PrivateTemp::new_in(std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))).unwrap()
}

#[test]
fn the_file_reads_back_what_was_written_at_aligned_offsets() {
    let d = dir();
    let f = Scratch::create(d.path()).unwrap();
    let mut w = AlignedBuf::new(8192, ALIGN);
    w.bytes().iter_mut().enumerate().for_each(|(i, b)| *b = (i % 251) as u8);
    f.write_at(w.bytes(), 4096).unwrap();
    let mut r = AlignedBuf::new(8192, ALIGN);
    f.read_at(r.bytes(), 4096).unwrap();
    assert_eq!(r.bytes(), w.bytes());
}

#[test]
#[cfg(unix)]
fn on_unix_the_name_is_gone_while_the_file_is_open() {
    let d = dir();
    let f = Scratch::create(d.path()).unwrap();
    assert_eq!(
        std::fs::read_dir(d.path()).unwrap().count(),
        0,
        "nothing for a crash to leave behind"
    );
    drop(f);
}

#[test]
fn the_file_is_gone_after_it_is_dropped() {
    let d = dir();
    drop(Scratch::create(d.path()).unwrap());
    assert!(!d.path().join(SCRATCH_NAME).exists());
}

#[test]
fn a_file_that_was_already_there_is_refused_and_left_alone() {
    let d = dir();
    let theirs = d.path().join(SCRATCH_NAME);
    std::fs::write(&theirs, b"mine").unwrap();
    assert!(Scratch::create(d.path()).is_err());
    assert_eq!(std::fs::read(&theirs).unwrap(), b"mine");
}

#[test]
fn a_missing_folder_is_an_error_and_makes_nothing() {
    let d = dir();
    let missing = d.path().join("nope");
    assert!(Scratch::create(&missing).is_err());
    assert!(!missing.exists());
}
