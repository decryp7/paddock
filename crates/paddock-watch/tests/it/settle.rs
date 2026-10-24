use paddock_watch::*;
use std::path::{Path, PathBuf};

fn stamp(name: &str, size: u64, mtime: i64) -> FileStamp {
    FileStamp {
        path: PathBuf::from(format!("/w/{name}")),
        size,
        modified_ms: mtime,
    }
}

#[test]
fn ready_files_are_flushed_together_once_the_batch_stops_growing() {
    let mut b = Batcher::new(3_000, 20_000, 100);
    assert!(!b.flush(0, 0), "nothing ready");
    assert!(!b.flush(2, 1_000), "the first ready files wait for company");
    assert!(
        !b.flush(5, 3_500),
        "still growing: the quiet time starts again"
    );
    assert!(!b.flush(5, 6_000));
    assert!(b.flush(5, 6_500), "quiet for 3 s: one batch");
    assert!(!b.flush(1, 7_000), "the next batch starts afresh");
    assert!(b.flush(1, 10_000));
    // A batch that keeps growing is flushed after the longest wait, and a full one at once.
    let mut b = Batcher::new(3_000, 20_000, 100);
    assert!(!b.flush(1, 0));
    for (n, t) in [
        (2, 2_000),
        (3, 4_000),
        (4, 6_000),
        (5, 8_000),
        (6, 10_000),
        (7, 12_000),
        (8, 14_000),
        (9, 16_000),
        (10, 18_000),
    ] {
        assert!(!b.flush(n, t), "{n} at {t}");
    }
    assert!(b.flush(11, 20_000), "the longest wait");
    assert!(b.flush(100, 20_100), "a full batch goes at once");
}

#[test]
fn the_state_flushes_what_observe_found_ready_in_one_batch() {
    let mut s = state();
    let mut now = 0;
    let mut batches = Vec::new();
    for names in [
        vec!["a.log"],
        vec!["a.log", "b.log"],
        vec!["a.log", "b.log", "c.log"],
    ] {
        let scan: Vec<_> = names.iter().map(|n| stamp(n, 10, 1)).collect();
        now += 1_500;
        let ready = s.observe(&scan, now);
        if s.flush_due(&ready, now) {
            batches.push(ready.new.len());
        }
    }
    assert!(batches.is_empty(), "files kept settling");
    let scan: Vec<_> = ["a.log", "b.log", "c.log"]
        .iter()
        .map(|n| stamp(n, 10, 1))
        .collect();
    for _ in 0..4 {
        now += 1_500;
        let ready = s.observe(&scan, now);
        if s.flush_due(&ready, now) {
            batches.push(ready.new.len());
        }
    }
    assert_eq!(batches, [3], "one batch for all three");
}

fn state() -> Settle {
    Settle::new(SettleConfig {
        debounce_ms: 1_000,
        max_file_bytes: 1_000,
    })
}

#[test]
fn a_file_is_ready_only_after_it_stopped_changing() {
    let mut s = state();
    assert!(
        s.observe(&[stamp("a.log", 10, 1)], 0).is_empty(),
        "first sight starts the quiet time"
    );
    assert!(s.observe(&[stamp("a.log", 10, 1)], 900).is_empty());
    let r = s.observe(&[stamp("a.log", 10, 1)], 1_000);
    assert_eq!(r.new, [PathBuf::from("/w/a.log")]);
}

#[test]
fn a_file_still_being_written_keeps_waiting() {
    let mut s = state();
    s.observe(&[stamp("a.log", 10, 1)], 0);
    assert!(
        s.observe(&[stamp("a.log", 20, 2)], 800).is_empty(),
        "it grew: the quiet time starts again"
    );
    assert!(s.observe(&[stamp("a.log", 20, 2)], 1_700).is_empty());
    assert_eq!(s.observe(&[stamp("a.log", 20, 2)], 1_800).new.len(), 1);
}

#[test]
fn temporary_empty_and_hidden_files_are_ignored() {
    let mut s = state();
    let scan = [
        stamp("a.log.part", 5, 1),
        stamp("b.tmp", 5, 1),
        stamp(".hidden", 5, 1),
        stamp("c.log", 0, 1),
        stamp("~$d.docx", 5, 1),
        stamp("e.log~", 5, 1),
    ];
    s.observe(&scan, 0);
    assert!(s.observe(&scan, 5_000).is_empty());
    assert_eq!(s.pending_len(), 0);
    assert!(is_temporary_name("x.CRDOWNLOAD") && !is_temporary_name("app.log"));
}

#[test]
fn a_vanished_file_is_forgotten() {
    let mut s = state();
    s.observe(&[stamp("a.log", 10, 1)], 0);
    assert_eq!(s.pending_len(), 1);
    assert!(s.observe(&[], 500).is_empty());
    assert_eq!(s.pending_len(), 0);
    // It comes back: the quiet time starts from zero.
    assert!(s.observe(&[stamp("a.log", 10, 1)], 5_000).is_empty());
}

#[test]
fn imported_files_are_not_read_again_until_they_change() {
    let mut s = state();
    let a = stamp("a.log", 10, 1);
    s.observe(std::slice::from_ref(&a), 0);
    assert_eq!(s.observe(std::slice::from_ref(&a), 1_000).new.len(), 1);
    s.imported(&a);
    assert!(s.observe(std::slice::from_ref(&a), 9_000).is_empty());
    let grown = stamp("a.log", 30, 5);
    assert!(s.observe(std::slice::from_ref(&grown), 10_000).is_empty());
    let r = s.observe(std::slice::from_ref(&grown), 11_000);
    assert_eq!(
        (r.new.len(), r.changed.len()),
        (0, 1),
        "a known file that grew is refreshed, not opened"
    );
}

#[test]
fn a_restart_does_not_read_imported_files_again() {
    let a = stamp("a.log", 10, 1);
    let mut s = state().with_imported([a.clone()]);
    s.observe(std::slice::from_ref(&a), 0);
    assert!(s.observe(std::slice::from_ref(&a), 10_000).is_empty());
}

#[test]
fn huge_files_are_reported_once() {
    let mut s = state();
    let big = stamp("big.log", 5_000, 1);
    let r = s.observe(std::slice::from_ref(&big), 0);
    assert_eq!(
        r.skipped,
        [(big.path.clone(), Skipped::TooLarge { size: 5_000 })]
    );
    assert!(
        s.observe(std::slice::from_ref(&big), 10_000).is_empty(),
        "reported once"
    );
    assert_eq!(s.pending_len(), 0);
}

#[test]
fn a_locked_file_is_retried_with_backoff_and_then_given_up() {
    let mut s = state();
    let a = stamp("a.log", 10, 1);
    s.observe(std::slice::from_ref(&a), 0);
    assert_eq!(s.observe(std::slice::from_ref(&a), 1_000).new.len(), 1);
    assert_eq!(s.failed(&a.path, "locked", 1_000), None);
    assert!(
        s.observe(std::slice::from_ref(&a), 2_500).is_empty(),
        "waiting for the retry time"
    );
    assert_eq!(s.observe(std::slice::from_ref(&a), 3_000).new.len(), 1);
    assert_eq!(s.failed(&a.path, "locked", 3_000), None);
    assert_eq!(s.failed(&a.path, "locked", 8_000), None);
    let gave_up = s.failed(&a.path, "locked", 20_000);
    assert_eq!(
        gave_up,
        Some(Skipped::GaveUp {
            reason: "locked".into()
        })
    );
    assert!(
        s.observe(std::slice::from_ref(&a), 99_000).is_empty(),
        "given up on this very file"
    );
    // Changing the file is a new chance.
    let fixed = stamp("a.log", 12, 9);
    s.observe(std::slice::from_ref(&fixed), 100_000);
    assert_eq!(
        s.observe(std::slice::from_ref(&fixed), 101_000).new.len(),
        1
    );
    assert_eq!(s.failed(Path::new("/w/unknown"), "x", 0), None);
}

#[test]
fn the_pending_list_is_bounded() {
    let mut s = state();
    let scan: Vec<FileStamp> = (0..MAX_PENDING + 50)
        .map(|i| stamp(&format!("f{i}.log"), 10, 1))
        .collect();
    s.observe(&scan, 0);
    assert_eq!(s.pending_len(), MAX_PENDING);
}

#[test]
fn scanning_filters_skips_links_and_reports_a_missing_folder_as_empty() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("a.log"), "x").unwrap();
    std::fs::write(dir.join("sub/b.log"), "y").unwrap();
    std::fs::write(dir.join("c.bin"), "z").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.join("sub"), dir.join("link")).unwrap();
    let scan = scan_folder(dir, &|rel| rel.extension().is_some_and(|e| e == "log"));
    assert!(!scan.truncated);
    let names: Vec<_> = scan
        .files
        .iter()
        .map(|f| {
            f.path
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert_eq!(names, ["a.log", "sub/b.log"]);
    assert!(scan.files.iter().all(|f| f.size == 1 && f.modified_ms > 0));
    assert_eq!(
        scan_folder(&dir.join("missing"), &|_| true),
        Scan::default()
    );
}

#[test]
fn a_scan_stops_at_the_entry_limit_and_the_depth_limit() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    for i in 0..10 {
        std::fs::write(dir.join(format!("{i}.log")), b"").unwrap();
    }
    let scan = scan_folder_limited(dir, &|_| true, 5, 4);
    assert!(scan.truncated && scan.files.len() <= 5);
    std::fs::create_dir_all(dir.join("d1/d2/d3")).unwrap();
    std::fs::write(dir.join("d1/d2/d3/deep.log"), b"x").unwrap();
    std::fs::write(dir.join("d1/near.log"), b"x").unwrap();
    let scan = scan_folder_limited(
        dir,
        &|rel| {
            rel.to_string_lossy().ends_with("log")
                && !rel
                    .to_string_lossy()
                    .starts_with(|c: char| c.is_ascii_digit())
        },
        100,
        1,
    );
    let names: Vec<_> = scan
        .files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["near.log"],
        "the folder three levels down is not entered"
    );
    assert_eq!(scan_folder(dir, &|_| false).files.len(), 0);
}

#[test]
fn defaults_are_the_documented_ones() {
    let c = SettleConfig::default();
    assert_eq!(
        (c.debounce_ms, c.max_file_bytes),
        (DEFAULT_DEBOUNCE_MS, DEFAULT_MAX_FILE_BYTES)
    );
    let b = Batcher::default();
    assert_eq!(
        (b.quiet_ms, b.max_wait_ms, b.max_files),
        (
            DEFAULT_BATCH_QUIET_MS,
            DEFAULT_BATCH_MAX_WAIT_MS,
            DEFAULT_BATCH_MAX_FILES
        )
    );
    assert!(Ready::default().is_empty());
}
