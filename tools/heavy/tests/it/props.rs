//! Parsing never panics, on any input; masking never leaks a following secret.

use heavy::mask::{is_secret_word, mask_cmd};
use heavy::record::{OwnerRead, format_owner, marker_pid, parse_marker_epoch, parse_owner, parse_ticket, sanitize_name};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn owner_parsing_never_panics_and_only_empty_means_empty(bytes in proptest::collection::vec(any::<u8>(), 0..2048), mtime in proptest::option::of(any::<u64>())) {
        let r = parse_owner(&bytes, mtime);
        prop_assert_eq!(bytes.is_empty(), r == OwnerRead::Empty);
    }

    #[test]
    fn other_records_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048), s in ".{0,60}") {
        let _ = parse_ticket(&bytes);
        let _ = parse_marker_epoch(&bytes);
        let _ = marker_pid(&s);
        let _ = sanitize_name(&s);
        let _ = is_secret_word(&s);
    }

    #[test]
    fn a_written_record_reads_back(pid in 1u32.., epoch in 0u64..4_000_000_000, name in "[ -~]{0,40}", cmd in "[ -~]{0,100}") {
        let o = heavy::record::Owner { pid, start_text: "t".into(), name: name.clone(), epoch: None, cwd: "/w".into(), cmd };
        let OwnerRead::Ok(back) = parse_owner(format_owner(&o, epoch).as_bytes(), None) else { return Err(TestCaseError::fail("not ok")) };
        prop_assert_eq!((back.pid, back.epoch), (pid, Some(epoch)));
        prop_assert_eq!(back.name, name);
    }

    #[test]
    fn masking_never_panics_stays_on_one_line_and_is_bounded(args in proptest::collection::vec(".{0,80}", 0..12)) {
        let m = mask_cmd(&args);
        prop_assert!(!m.contains('\n') && !m.contains('\r') && !m.contains('\t'));
        prop_assert!(m.chars().count() <= 500);
    }

    #[test]
    fn the_value_after_a_secret_flag_is_always_hidden(v in "[a-zA-Z0-9]{6,20}") {
        let m = mask_cmd(&["tool".into(), "--token".into(), v.clone()]);
        prop_assert!(!m.contains(&v));
        let m = mask_cmd(&["tool".into(), format!("password={v}")]);
        prop_assert!(!m.contains(&v));
    }

    #[test]
    fn record_epoch_comparisons_never_panic(pid in any::<u32>(), start in any::<u64>(), epoch in proptest::option::of(any::<u64>())) {
        let procs = heavy::procs::FakeProcs::new();
        procs.run(pid, start / 1_000_000);
        let _ = heavy::procs::pid_alive(&procs, pid, epoch);
        let _ = heavy::procs::evidence(&procs, pid, epoch);
    }

    #[test]
    fn the_real_process_table_never_panics_on_any_pid(pid in any::<u32>()) {
        let _ = heavy::procs::Procs::probe(&heavy::procs::OsProcs, pid);
    }

    #[test]
    fn guard_paths_never_panic(dir in ".{0,80}") {
        let cwd = std::path::Path::new("/");
        let _ = heavy::guard::violation(std::slice::from_ref(&dir), Some(std::path::Path::new("/home/x")), cwd);
        let _ = heavy::guard::build_dirs(Some(&dir), &[dir.clone(), "--target-dir".into(), dir.clone()]);
    }
}

#[test]
fn pid_zero_and_huge_pids_are_gone_never_a_group_probe() {
    for pid in [0u32, u32::MAX, i32::MAX as u32 + 1] {
        assert_eq!(
            heavy::procs::Procs::probe(&heavy::procs::OsProcs, pid),
            heavy::procs::Presence::Gone,
            "pid {pid}"
        );
    }
}

#[cfg(unix)]
#[test]
fn the_signal_probe_never_aims_at_a_group() {
    // kill(0, 0) would probe this process's own group and succeed; kill(-1, 0) every process we may signal
    assert!(!heavy::probe::exists(0));
    assert!(!heavy::probe::exists(u32::MAX));
    assert!(heavy::probe::exists(std::process::id()));
}
