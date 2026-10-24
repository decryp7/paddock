use super::*;
use serde_json::json;
use std::path::PathBuf;

fn store(dir: &Path) -> Arc<SessionStore> {
    Arc::new(SessionStore::new(dir))
}

fn files(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join("current.json"), dir.join("recovered.json"))
}

fn one(label: &str, ui: Value) -> Vec<(String, Option<Value>)> {
    vec![(label.to_string(), Some(ui))]
}

#[test]
fn a_run_starts_empty_and_the_previous_runs_session_becomes_the_recovered_one() {
    let t = tempfile::tempdir().unwrap();
    let first = store(t.path());
    first.rotate();
    assert_eq!(first.previous_info(), SessionInfo::default());
    first.put("main", &json!({"query": "error"})).unwrap();
    first.put("w-2", &json!({"query": "timeout"})).unwrap();
    // The process is killed here: no end-of-run work happens.
    drop(first);

    let second = store(t.path());
    second.rotate();
    let info = second.previous_info();
    assert_eq!(
        (info.available, info.windows, info.problem),
        (true, 2, None)
    );
    assert_eq!(second.previous().windows["main"], json!({"query": "error"}));
    assert!(second.current().windows.is_empty(), "this run starts empty");
    // An autosave of the new run does not touch what a restore would offer.
    second.put("main", &json!({"query": "new"})).unwrap();
    assert_eq!(second.previous().windows["main"], json!({"query": "error"}));
}

#[test]
fn an_empty_run_in_between_does_not_wipe_the_session_worth_restoring() {
    let t = tempfile::tempdir().unwrap();
    let a = store(t.path());
    a.rotate();
    a.put("main", &json!({"n": 1})).unwrap();
    let b = store(t.path());
    b.rotate(); // the run that crashed saved n = 1; this one saves nothing and dies
    let c = store(t.path());
    c.rotate();
    assert_eq!(c.previous().windows["main"], json!({"n": 1}));
    assert!(c.previous_info().available);
}

#[test]
fn a_kill_in_the_middle_of_a_checkpoint_leaves_the_last_whole_one() {
    let t = tempfile::tempdir().unwrap();
    let run = store(t.path());
    run.rotate();
    run.put("main", &json!({"n": 1})).unwrap();
    run.put("main", &json!({"n": 2})).unwrap();
    drop(run);
    // What a kill leaves behind: a half-written temporary file next to the intact session file.
    let (current, _) = files(t.path());
    std::fs::write(
        t.path().join(".current.json.4242.7.tmp"),
        r#"{"version":1,"windows":{"main":{"n":"#,
    )
    .unwrap();
    assert!(current.exists());

    let next = store(t.path());
    next.rotate();
    assert_eq!(next.previous().windows["main"], json!({"n": 2}));
    assert_eq!(next.previous_info().problem, None);
}

#[test]
fn a_torn_session_file_falls_back_to_the_last_good_copy_with_a_notice() {
    let t = tempfile::tempdir().unwrap();
    let run = store(t.path());
    run.rotate();
    run.put("main", &json!({"n": 1})).unwrap();
    run.put("main", &json!({"n": 2})).unwrap();
    drop(run);
    let (current, _) = files(t.path());
    // A write that was not atomic (another tool, a full disk): the file is cut in half.
    let bytes = std::fs::read(&current).unwrap();
    std::fs::write(&current, &bytes[..bytes.len() / 2]).unwrap();

    let next = store(t.path());
    next.rotate();
    let info = next.previous_info();
    assert!(info.available, "the backup holds the previous checkpoint");
    assert_eq!(next.previous().windows["main"], json!({"n": 1}));
    let problem = info.problem.expect("a notice");
    assert!(problem.contains("last good copy"), "{problem}");
    let kept: Vec<_> = std::fs::read_dir(t.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
        .collect();
    assert_eq!(kept.len(), 1, "the damaged file is kept for the report");
}

#[test]
fn a_corrupt_session_file_without_a_backup_starts_clean_with_a_notice() {
    let t = tempfile::tempdir().unwrap();
    let (current, _) = files(t.path());
    std::fs::write(&current, "{ not json at all").unwrap();
    let run = store(t.path());
    run.rotate();
    let info = run.previous_info();
    assert!(!info.available);
    assert!(info.problem.unwrap().contains("could not be read"));
    // The run is usable: its autosave works and the next start sees it.
    run.put("main", &json!({"ok": true})).unwrap();
    let next = store(t.path());
    next.rotate();
    assert_eq!(next.previous().windows["main"], json!({"ok": true}));
}

#[test]
fn a_damaged_current_file_does_not_offer_an_older_recovered_session() {
    let t = tempfile::tempdir().unwrap();
    let a = store(t.path());
    a.rotate();
    a.put("main", &json!({"old": true})).unwrap();
    let b = store(t.path());
    b.rotate(); // recovered.json = old; current empty
    let (current, _) = files(t.path());
    std::fs::write(&current, "garbage").unwrap();
    std::fs::remove_file(t.path().join("current.json.bak")).unwrap();
    let c = store(t.path());
    c.rotate();
    assert!(
        !c.previous_info().available,
        "two runs back is not the last session"
    );
}

#[test]
fn a_session_from_a_newer_version_is_left_alone() {
    let t = tempfile::tempdir().unwrap();
    let (current, _) = files(t.path());
    let newer = r#"{"version": 99, "windows": {"main": {"x": 1}}}"#;
    std::fs::write(&current, newer).unwrap();
    let run = store(t.path());
    run.rotate();
    let info = run.previous_info();
    assert!(!info.available);
    assert!(info.problem.unwrap().contains("newer version"));
    assert!(
        run.put("main", &json!({"a": 1})).is_err(),
        "never overwrites it"
    );
    assert_eq!(std::fs::read_to_string(&current).unwrap(), newer);
}

#[test]
fn a_damaged_recovered_file_is_reported_not_offered() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("recovered.json"), "[1,2").unwrap();
    let run = store(t.path());
    run.rotate();
    let info = run.previous_info();
    assert!(!info.available && info.problem.is_some());
}

#[test]
fn start_fresh_discards_the_previous_session_for_good() {
    let t = tempfile::tempdir().unwrap();
    let a = store(t.path());
    a.rotate();
    a.put("main", &json!({"n": 1})).unwrap();
    let b = store(t.path());
    b.rotate();
    assert!(b.previous_info().available);
    b.discard_previous();
    assert_eq!(b.previous_info(), SessionInfo::default());
    drop(b);
    let c = store(t.path());
    c.rotate();
    assert!(!c.previous_info().available);
}

#[test]
fn labels_sizes_and_counts_are_bounded() {
    let t = tempfile::tempdir().unwrap();
    let s = store(t.path());
    s.rotate();
    for bad in ["", "../x", "a b", &"x".repeat(MAX_LABEL_LEN + 1)] {
        assert_eq!(
            s.put(bad, &json!({})),
            Err(SessionError::InvalidLabel),
            "{bad:?}"
        );
    }
    assert_eq!(s.put("main", &json!([1])), Err(SessionError::NotAnObject));
    let big = json!({"blob": "x".repeat(MAX_WINDOW_BYTES)});
    assert!(matches!(
        s.put("main", &big),
        Err(SessionError::TooLarge { .. })
    ));
    for i in 0..MAX_WINDOWS {
        s.put(&format!("w-{i}"), &json!({})).unwrap();
    }
    assert_eq!(
        s.put("w-extra", &json!({})),
        Err(SessionError::TooManyWindows)
    );
    s.put("w-0", &json!({"again": 1})).unwrap();
    assert_eq!(s.current().windows.len(), MAX_WINDOWS);
    s.forget("w-0").unwrap();
    s.put("w-extra", &json!({})).unwrap();
    assert!(!SessionError::TooManyWindows.to_string().is_empty());
    assert!(SessionError::Write("x".into()).to_string().contains('x'));
}

#[test]
fn a_batch_applies_the_valid_entries_and_reports_the_first_error() {
    let t = tempfile::tempdir().unwrap();
    let s = store(t.path());
    s.rotate();
    let mut batch = one("main", json!({"a": 1}));
    batch.push(("bad label".into(), Some(json!({}))));
    batch.push(("w-2".into(), Some(json!({"b": 2}))));
    assert_eq!(s.apply(&batch), Err(SessionError::InvalidLabel));
    assert_eq!(s.current().windows.len(), 2);
}

#[test]
fn an_unwritable_session_folder_is_an_error_not_a_panic() {
    let t = tempfile::tempdir().unwrap();
    // A file where the session folder should be.
    let blocker = t.path().join("session");
    std::fs::write(&blocker, "x").unwrap();
    let s = store(&blocker);
    s.rotate();
    assert!(matches!(
        s.put("main", &json!({})),
        Err(SessionError::Write(_))
    ));
}

#[test]
fn the_autosaver_writes_in_the_background_and_flush_waits_for_it() {
    let t = tempfile::tempdir().unwrap();
    let s = store(t.path());
    s.rotate();
    let saver = Autosaver::start(Arc::clone(&s));
    saver.submit("main", json!({"n": 1})).unwrap();
    saver.submit("main", json!({"n": 2})).unwrap();
    saver.submit("w-2", json!({"m": 1})).unwrap();
    saver.flush();
    assert_eq!(s.current().windows["main"], json!({"n": 2}));
    assert!(saver.batches() >= 1);
    saver.forget("w-2");
    saver.flush();
    assert!(!s.current().windows.contains_key("w-2"));
    // What flush saw is on disk.
    let next = store(t.path());
    next.rotate();
    assert_eq!(next.previous().windows.len(), 1);
}

#[test]
fn the_autosaver_refuses_what_can_never_be_kept_and_survives_write_failures() {
    let t = tempfile::tempdir().unwrap();
    let blocker = t.path().join("session");
    std::fs::write(&blocker, "x").unwrap();
    let s = store(&blocker);
    s.rotate();
    let saver = Autosaver::start(Arc::clone(&s));
    assert_eq!(
        saver.submit("bad label", json!({})),
        Err(SessionError::InvalidLabel)
    );
    assert_eq!(
        saver.submit("main", json!(3)),
        Err(SessionError::NotAnObject)
    );
    saver.forget("not a label");
    // The writes fail (the folder is a file); the writer keeps going and flush returns.
    saver.submit("main", json!({"n": 1})).unwrap();
    saver.flush();
    saver.submit("main", json!({"n": 2})).unwrap();
    saver.flush();
    assert!(saver.batches() >= 2);
    saver.stop();
    // After stop nothing is accepted and nothing hangs.
    saver.submit("main", json!({"n": 3})).unwrap();
    saver.flush();
    saver.stop();
}

#[test]
fn dropping_the_autosaver_writes_what_was_queued() {
    let t = tempfile::tempdir().unwrap();
    let s = store(t.path());
    s.rotate();
    {
        let saver = Autosaver::start(Arc::clone(&s));
        saver.submit("main", json!({"last": true})).unwrap();
    }
    assert_eq!(s.current().windows["main"], json!({"last": true}));
}
