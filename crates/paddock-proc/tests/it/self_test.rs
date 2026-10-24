use paddock_proc::self_test::{Check, FLAG, handle, requested, run};

fn report(checks: &[Check]) -> (i32, String) {
    let mut out = Vec::new();
    let code = run("worker", "1.2.3", checks, &mut out);
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn the_flag_is_the_first_argument() {
    assert!(requested(&[FLAG]));
    assert!(requested(&["--self-test".to_string(), "x".to_string()]));
    assert!(!requested::<&str>(&[]));
    assert!(!requested(&["run", FLAG]));
    assert!(!requested(&["--self-testing"]));
}

#[test]
fn a_sidecar_with_no_checks_of_its_own_still_proves_it_starts() {
    let (code, text) = report(&[]);
    assert_eq!(code, 0);
    assert_eq!(text, "worker 1.2.3 self-test\nok executable\n");
}

#[test]
fn every_check_is_reported_and_one_failure_fails_the_run() {
    let (code, text) = report(&[
        Check::new("engine", || Ok(())),
        Check::new("library", || Err("libduckdb not found".into())),
        Check::new("table", || Ok(())),
    ]);
    assert_eq!(code, 1);
    assert_eq!(
        text,
        "worker 1.2.3 self-test\nok executable\nok engine\nFAIL library: libduckdb not found\nok table\n"
    );
}

#[test]
fn a_panicking_check_fails_with_its_message_and_the_report_goes_on() {
    let (code, text) = report(&[
        Check::new("static", || panic!("boom")),
        Check::new("formatted", || panic!("{} {}", "bad", 7)),
        Check::new("other", || std::panic::panic_any(5u8)),
        Check::new("after", || Ok(())),
    ]);
    assert_eq!(code, 1);
    assert!(text.contains("FAIL static: panicked: boom\n"), "{text}");
    assert!(text.contains("FAIL formatted: panicked: bad 7\n"), "{text}");
    assert!(text.contains("FAIL other: panicked: panicked\n"), "{text}");
    assert!(text.ends_with("ok after\n"), "{text}");
}

#[test]
fn handle_does_nothing_without_the_flag() {
    // The test binary is started without `--self-test`.
    assert_eq!(handle("worker", "1", &[]), None);
}
