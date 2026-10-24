use paddock_testkit::golden::UPDATE_VAR;
use paddock_testkit::{Golden, GoldenError, GoldenMode, Scratch, check_golden};

#[test]
fn matching_and_crlf_files_pass() {
    let s = Scratch::new();
    let p = s.write("g/out.txt", "a\r\nb\r\n").expect("write");
    assert!(matches!(check_golden(&p, "a\nb\n", GoldenMode::Check), Ok(Golden::Matched)));
    assert!(matches!(check_golden(&p, "a\r\nb\r\n", GoldenMode::Check), Ok(Golden::Matched)));
}

#[test]
fn a_difference_names_the_first_differing_line() {
    let s = Scratch::new();
    let p = s.write("out.txt", "a\nb\nc\n").expect("write");
    match check_golden(&p, "a\nB\nc\n", GoldenMode::Check) {
        Err(GoldenError::Differs {
            line, expected, actual, ..
        }) => assert_eq!((line, expected.as_str(), actual.as_str()), (2, "b", "B")),
        other => panic!("{other:?}"),
    }
    match check_golden(&p, "a\nb\n", GoldenMode::Check) {
        Err(GoldenError::Differs {
            line, expected, actual, ..
        }) => assert_eq!((line, expected.as_str(), actual.as_str()), (3, "c", "")),
        other => panic!("{other:?}"),
    }
    let message = check_golden(&p, "a\nb\n", GoldenMode::Check)
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(message.contains(UPDATE_VAR) && message.contains("line 3"), "{message}");
}

#[test]
fn a_missing_final_newline_is_a_difference() {
    let s = Scratch::new();
    let p = s.write("out.txt", "a\n").expect("write");
    match check_golden(&p, "a", GoldenMode::Check) {
        Err(GoldenError::Differs { expected, actual, .. }) => assert_eq!(
            (expected.as_str(), actual.as_str()),
            ("(ends with a newline)", "(no final newline)")
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn missing_file_fails_in_check_and_is_written_in_update() {
    let s = Scratch::new();
    let p = s.path("new/dir/out.txt");
    assert!(matches!(
        check_golden(&p, "x\n", GoldenMode::Check),
        Err(GoldenError::Missing { .. })
    ));
    assert!(!p.exists(), "checking never writes");
    assert!(matches!(check_golden(&p, "x\r\n", GoldenMode::Update), Ok(Golden::Updated)));
    assert_eq!(s.read("new/dir/out.txt").expect("read"), "x\n");
    assert!(matches!(check_golden(&p, "y\n", GoldenMode::Update), Ok(Golden::Updated)));
    assert!(matches!(check_golden(&p, "y\n", GoldenMode::Update), Ok(Golden::Matched)));
}

#[test]
fn unreadable_and_unwritable_files_are_io_errors() {
    let s = Scratch::new();
    // A folder where the file should be: reading fails with something other than "not found".
    let dir = s.path("is-a-folder");
    std::fs::create_dir(&dir).expect("mkdir");
    assert!(matches!(check_golden(&dir, "x", GoldenMode::Check), Err(GoldenError::Io { .. })));
    assert!(matches!(check_golden(&dir, "x", GoldenMode::Update), Err(GoldenError::Io { .. })));
    // A file where a parent folder should be.
    let file = s.write("f", "x").expect("write");
    assert!(matches!(
        check_golden(&file.join("child.txt"), "x", GoldenMode::Update),
        Err(GoldenError::Io { .. })
    ));
}

#[test]
fn the_mode_comes_from_the_variable() {
    assert_eq!(GoldenMode::from_var(None), GoldenMode::Check);
    assert_eq!(GoldenMode::from_var(Some("")), GoldenMode::Check);
    assert_eq!(GoldenMode::from_var(Some("0")), GoldenMode::Check);
    assert_eq!(GoldenMode::from_var(Some("1")), GoldenMode::Update);
    let mut env = paddock_testkit::env_lock();
    env.remove(UPDATE_VAR);
    assert_eq!(GoldenMode::from_env(), GoldenMode::Check);
    env.set(UPDATE_VAR, "1");
    assert_eq!(GoldenMode::from_env(), GoldenMode::Update);
}

#[test]
fn assert_golden_panics_on_a_difference_and_passes_on_a_match() {
    let s = Scratch::new();
    let p = s.write("out.txt", "a\n").expect("write");
    let mut env = paddock_testkit::env_lock();
    env.remove(UPDATE_VAR);
    paddock_testkit::assert_golden(&p, "a\n");
    let failed = std::panic::catch_unwind(|| paddock_testkit::assert_golden(&p, "b\n"));
    assert!(failed.is_err());
}

#[test]
fn every_error_reads_as_a_sentence_naming_the_file() {
    let s = Scratch::new();
    let missing = check_golden(&s.path("a.txt"), "x", GoldenMode::Check)
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(missing.contains("a.txt is missing") && missing.contains(UPDATE_VAR), "{missing}");
    let dir = s.path("d");
    std::fs::create_dir(&dir).expect("mkdir");
    let io = check_golden(&dir, "x", GoldenMode::Check)
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(io.starts_with("golden file ") && io.contains("d: "), "{io}");
    let mut env = paddock_testkit::env_lock();
    env.set(UPDATE_VAR, "1");
    let p = s.write("x.txt", "a\n").expect("write");
    paddock_testkit::assert_golden(&p, "b\n");
    assert_eq!(s.read("x.txt").expect("read"), "b\n", "assert_golden rewrites in update mode");
}
