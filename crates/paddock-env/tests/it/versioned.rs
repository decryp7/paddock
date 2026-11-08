//! The versioned state file against real temporary folders: missing, damaged, newer, migrations, backups, kept copies.

use paddock_env::{EnvError, Problem, VersionedFile};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    name: String,
    count: u32,
}

fn state(name: &str, count: u32) -> State {
    State {
        name: name.into(),
        count,
    }
}

fn file(dir: &Path) -> VersionedFile<State> {
    VersionedFile::new(dir.join("state.json"), 1).with_clock(|| 1000)
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn missing_gives_defaults_without_a_problem_and_save_round_trips() {
    let t = tempfile::tempdir().unwrap();
    let f = file(t.path());
    let l = f.load();
    assert_eq!(
        (l.value.clone(), l.problem, l.migrated_from),
        (State::default(), None, None)
    );
    f.save(&state("a", 3)).unwrap();
    assert_eq!(f.load().value, state("a", 3));
    let text = std::fs::read_to_string(f.path()).unwrap();
    assert!(text.contains("\"version\": 1") && text.ends_with('\n'));
    assert_eq!(
        names(t.path()),
        ["state.json"],
        "the first save has nothing to back up"
    );
}

#[test]
fn the_second_save_keeps_the_previous_content_as_bak() {
    let t = tempfile::tempdir().unwrap();
    let f = file(t.path());
    f.save(&state("a", 1)).unwrap();
    f.save(&state("b", 2)).unwrap();
    let bak: Value = serde_json::from_slice(&std::fs::read(f.backup_path()).unwrap()).unwrap();
    assert_eq!(bak, json!({"version": 1, "name": "a", "count": 1}));
    assert_eq!(names(t.path()), ["state.json", "state.json.bak"]);
}

#[test]
fn a_file_without_a_version_is_the_first_version() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("state.json"), r#"{"name":"old","count":9}"#).unwrap();
    let l = file(t.path()).load();
    assert_eq!(
        (l.value, l.problem, l.migrated_from),
        (state("old", 9), None, None)
    );
}

#[test]
fn damaged_files_give_defaults_and_are_kept_aside() {
    let cases: [(&str, &[u8]); 6] = [
        ("not json", b"{ nope"),
        ("empty", b""),
        ("binary", &[0xff, 0xfe, 0x00, 0x01]),
        ("not an object", b"[1,2]"),
        ("wrong shape", br#"{"version":1,"name":5}"#),
        ("bad version", br#"{"version":"two","name":"x","count":1}"#),
    ];
    for (what, bytes) in cases {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("state.json"), bytes).unwrap();
        let f = file(t.path());
        let l = f.load();
        assert_eq!(l.value, State::default(), "{what}");
        let Some(Problem::Damaged {
            kept_as,
            recovered_from_backup,
            ..
        }) = l.problem.clone()
        else {
            panic!("{what}: {:?}", l.problem)
        };
        assert!(!recovered_from_backup, "{what}");
        let kept = kept_as.unwrap();
        assert_eq!(
            std::fs::read(&kept).unwrap(),
            bytes,
            "{what}: the damaged bytes are kept"
        );
        assert_eq!(kept.file_name().unwrap(), "state.json.corrupt-1000");
        assert!(!f.path().exists(), "{what}");
        assert!(
            l.problem
                .unwrap()
                .to_string()
                .contains("state.json.corrupt-1000")
        );
        // Saving afterwards works and the file loads again.
        f.save(&state("fresh", 1)).unwrap();
        assert_eq!(f.load().value, state("fresh", 1));
    }
}

#[test]
fn a_file_over_the_limit_is_damaged() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(
        t.path().join("state.json"),
        format!(r#"{{"name":"{}","count":1}}"#, "x".repeat(3000)),
    )
    .unwrap();
    let l = file(t.path()).max_bytes(1024).load();
    assert!(
        matches!(&l.problem, Some(Problem::Damaged { reason, .. }) if reason.contains("larger than 1 KB"))
    );
    assert_eq!(l.value, State::default());
}

#[test]
fn a_damaged_file_recovers_from_a_valid_backup() {
    let t = tempfile::tempdir().unwrap();
    let f = file(t.path());
    f.save(&state("good", 1)).unwrap();
    f.save(&state("newer", 2)).unwrap();
    std::fs::write(f.path(), "garbage").unwrap();
    let l = f.load();
    assert_eq!(
        l.value,
        state("good", 1),
        "the .bak holds the last good content"
    );
    assert!(matches!(
        &l.problem,
        Some(Problem::Damaged {
            recovered_from_backup: true,
            ..
        })
    ));
    assert!(l.problem.unwrap().to_string().contains("last good copy"));
    assert_eq!(f.load().value, state("good", 1), "and the file is restored");
    assert!(names(t.path()).contains(&"state.json.corrupt-1000".to_string()));
}

#[test]
fn a_damaged_backup_does_not_stop_the_defaults() {
    let t = tempfile::tempdir().unwrap();
    let f = file(t.path());
    std::fs::write(f.path(), "garbage").unwrap();
    std::fs::write(f.backup_path(), "also garbage").unwrap();
    let l = f.load();
    assert_eq!(l.value, State::default());
    assert!(matches!(
        l.problem,
        Some(Problem::Damaged {
            recovered_from_backup: false,
            ..
        })
    ));
}

#[test]
fn a_damaged_file_never_replaces_a_good_backup() {
    let t = tempfile::tempdir().unwrap();
    let f = file(t.path());
    f.save(&state("good", 1)).unwrap();
    f.save(&state("good2", 2)).unwrap();
    // The damaged file is still in place (it could not be moved aside), then the app saves.
    std::fs::write(f.path(), "garbage").unwrap();
    f.save(&state("after", 3)).unwrap();
    let bak: Value = serde_json::from_slice(&std::fs::read(f.backup_path()).unwrap()).unwrap();
    assert_eq!(
        bak["name"], "good",
        "the backup is still the last good file"
    );
    assert_eq!(f.load().value, state("after", 3));
}

#[test]
fn damaged_copies_do_not_collide_and_only_the_newest_are_kept() {
    let t = tempfile::tempdir().unwrap();
    let f = file(t.path()).keep_corrupt(3);
    for i in 0..5 {
        std::fs::write(f.path(), format!("bad {i}")).unwrap();
        assert!(f.load().problem.is_some());
    }
    // Same stamp for all five: -, -1, -2, -3, -4; the three newest stay.
    let kept: Vec<String> = names(t.path());
    assert_eq!(
        kept,
        [
            "state.json.corrupt-1000-2",
            "state.json.corrupt-1000-3",
            "state.json.corrupt-1000-4"
        ]
    );
    assert_eq!(
        std::fs::read_to_string(t.path().join("state.json.corrupt-1000-4")).unwrap(),
        "bad 4"
    );
}

#[test]
fn a_newer_version_is_refused_and_left_untouched() {
    let t = tempfile::tempdir().unwrap();
    let original = br#"{"version":7,"name":"future","count":1,"extra":true}"#;
    std::fs::write(t.path().join("state.json"), original).unwrap();
    let f = file(t.path());
    let l = f.load();
    assert_eq!(l.value, State::default());
    assert_eq!(
        l.problem,
        Some(Problem::Newer {
            found: 7,
            supported: 1
        })
    );
    assert!(l.problem.unwrap().to_string().contains("newer version"));
    let e = f.save(&state("x", 1)).unwrap_err();
    assert!(matches!(
        e,
        EnvError::NewerVersion {
            found: 7,
            supported: 1,
            ..
        }
    ));
    assert!(e.to_string().contains("format 7") && e.to_string().contains("update the app"));
    assert_eq!(
        std::fs::read(f.path()).unwrap(),
        original,
        "not modified, not moved"
    );
    assert_eq!(names(t.path()), ["state.json"]);
}

#[test]
fn a_reload_after_the_file_is_replaced_allows_saving_again() {
    let t = tempfile::tempdir().unwrap();
    let f = file(t.path());
    std::fs::write(f.path(), r#"{"version":9}"#).unwrap();
    f.load();
    assert!(f.save(&State::default()).is_err());
    std::fs::remove_file(f.path()).unwrap();
    f.load();
    f.save(&state("ok", 1)).unwrap();
}

fn add_label(mut v: Value) -> Result<Value, String> {
    v["label"] = json!(format!(
        "{}-{}",
        v["name"].as_str().unwrap_or(""),
        v["count"]
    ));
    Ok(v)
}
fn rename_count_to_n(mut v: Value) -> Result<Value, String> {
    let n = v
        .as_object_mut()
        .and_then(|o| o.remove("count"))
        .ok_or("no count")?;
    v["n"] = n;
    Ok(v)
}
fn to_wide(mut v: Value) -> Result<Value, String> {
    let n = v["n"].as_u64().ok_or("n is not a number")?;
    v["n"] = json!(n * 10);
    Ok(v)
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
struct StateV4 {
    name: String,
    label: String,
    n: u64,
}

fn v4(dir: &Path) -> VersionedFile<StateV4> {
    VersionedFile::new(dir.join("state.json"), 4)
        .with_migration(1, add_label)
        .with_migration(2, rename_count_to_n)
        .with_migration(3, to_wide)
}

#[test]
fn every_older_version_migrates_through_the_chain_once() {
    let files = [
        r#"{"name":"a","count":3}"#,             // unversioned = v1
        r#"{"version":1,"name":"a","count":3}"#, // v1
        r#"{"version":2,"name":"a","count":3,"label":"a-3"}"#,
        r#"{"version":3,"name":"a","label":"a-3","n":3}"#,
    ];
    let from = [1, 1, 2, 3];
    for (text, from) in files.into_iter().zip(from) {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("state.json"), text).unwrap();
        let f = v4(t.path());
        let l = f.load();
        assert_eq!(
            l.value,
            StateV4 {
                name: "a".into(),
                label: "a-3".into(),
                n: 30
            },
            "{text}"
        );
        assert_eq!((l.problem, l.migrated_from), (None, Some(from)));
        // Written back once as the current format; the old file is kept.
        let doc: Value = serde_json::from_slice(&std::fs::read(f.path()).unwrap()).unwrap();
        assert_eq!(doc["version"], 4);
        assert_eq!(
            std::fs::read_to_string(t.path().join(format!("state.json.v{from}.bak"))).unwrap(),
            text
        );
        let again = f.load();
        assert_eq!(
            (again.value, again.migrated_from),
            (l.value, None),
            "does not run twice"
        );
    }
}

#[test]
fn a_current_file_needs_no_migration_and_leaves_no_files() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(
        t.path().join("state.json"),
        r#"{"version":4,"name":"a","label":"l","n":1}"#,
    )
    .unwrap();
    let l = v4(t.path()).load();
    assert_eq!((l.value.n, l.migrated_from, l.problem), (1, None, None));
    assert_eq!(names(t.path()), ["state.json"]);
}

#[test]
fn a_failing_migration_step_makes_the_file_damaged_and_keeps_it() {
    let t = tempfile::tempdir().unwrap();
    let text = r#"{"version":3,"name":"a","label":"x","n":"many"}"#;
    std::fs::write(t.path().join("state.json"), text).unwrap();
    let l = v4(t.path()).load();
    assert_eq!(l.value, StateV4::default());
    let Some(Problem::Damaged {
        reason, kept_as, ..
    }) = l.problem
    else {
        panic!()
    };
    assert!(
        reason.contains("converting format 3 failed: n is not a number"),
        "{reason}"
    );
    assert_eq!(std::fs::read_to_string(kept_as.unwrap()).unwrap(), text);
}

fn not_an_object(_: Value) -> Result<Value, String> {
    Ok(json!([1]))
}

#[test]
fn a_step_that_returns_a_non_object_is_a_failure() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(
        t.path().join("state.json"),
        r#"{"version":1,"name":"a","count":1}"#,
    )
    .unwrap();
    let f: VersionedFile<State> =
        VersionedFile::new(t.path().join("state.json"), 2).with_migration(1, not_an_object);
    assert!(
        matches!(f.load().problem, Some(Problem::Damaged { reason, .. }) if reason.contains("did not give an object"))
    );
}

#[test]
fn a_missing_step_leaves_the_file_alone_and_refuses_saves() {
    let t = tempfile::tempdir().unwrap();
    let text = r#"{"version":1,"name":"a","count":1}"#;
    std::fs::write(t.path().join("state.json"), text).unwrap();
    let f: VersionedFile<State> = VersionedFile::new(t.path().join("state.json"), 2);
    let l = f.load();
    assert_eq!(l.problem, Some(Problem::MissingMigration { from: 1 }));
    assert!(l.problem.unwrap().to_string().contains("format 1"));
    assert!(matches!(
        f.save(&State::default()),
        Err(EnvError::ReadOnly { .. })
    ));
    assert_eq!(std::fs::read_to_string(f.path()).unwrap(), text);
}

#[test]
fn an_unreadable_path_is_left_alone_and_read_only() {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir(t.path().join("state.json")).unwrap();
    let f = file(t.path());
    let l = f.load();
    assert!(matches!(l.problem, Some(Problem::Unreadable { .. })));
    assert!(l.problem.unwrap().to_string().contains("not a file"));
    let e = f.save(&State::default()).unwrap_err();
    assert!(matches!(e, EnvError::ReadOnly { .. }) && e.to_string().contains("read-only"));
    assert!(t.path().join("state.json").is_dir());
}

#[test]
fn a_value_that_is_not_an_object_cannot_be_saved() {
    let t = tempfile::tempdir().unwrap();
    let f: VersionedFile<Vec<u8>> = VersionedFile::new(t.path().join("list.json"), 1);
    assert!(matches!(f.save(&vec![1]), Err(EnvError::NotAnObject)));
    assert!(!f.path().exists());
}

#[test]
fn a_stale_temporary_file_from_a_killed_write_is_ignored() {
    let t = tempfile::tempdir().unwrap();
    let f = file(t.path());
    f.save(&state("kept", 1)).unwrap();
    // A process killed mid-write leaves a half-written temporary file; the real file is still complete.
    std::fs::write(
        t.path().join(".state.json.99999.0.tmp"),
        r#"{"version":1,"na"#,
    )
    .unwrap();
    assert_eq!(f.load().value, state("kept", 1));
    f.save(&state("next", 2)).unwrap();
    assert_eq!(f.load().value, state("next", 2));
}

#[test]
fn a_private_file_is_owner_only() {
    let t = tempfile::tempdir().unwrap();
    let f = file(t.path()).private();
    f.save(&state("a", 1)).unwrap();
    f.save(&state("b", 2)).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in [f.path().to_path_buf(), f.backup_path()] {
            assert_eq!(
                std::fs::metadata(p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}

#[test]
fn a_version_field_in_the_value_does_not_override_the_files_version() {
    #[derive(Default, Serialize, Deserialize)]
    struct WithVersion {
        #[serde(default)]
        version: u32,
        x: u8,
    }
    let t = tempfile::tempdir().unwrap();
    let f: VersionedFile<WithVersion> =
        VersionedFile::new(t.path().join("v.json"), 2).with_migration(1, Ok);
    f.save(&WithVersion { version: 99, x: 1 }).unwrap();
    let doc: Value = serde_json::from_slice(&std::fs::read(f.path()).unwrap()).unwrap();
    assert_eq!(doc["version"], 2);
    // The value does not carry the file's version back: `version` is the file's, not the value's.
    assert_eq!(f.load().value.x, 1);
}

#[test]
fn version_zero_is_damaged_and_new_clamps_to_one() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(
        t.path().join("state.json"),
        r#"{"version":0,"name":"a","count":1}"#,
    )
    .unwrap();
    let f: VersionedFile<State> = VersionedFile::new(t.path().join("state.json"), 0);
    assert!(matches!(f.load().problem, Some(Problem::Damaged { .. })));
}

#[test]
fn every_error_says_what_to_fix_and_only_a_failed_operation_has_a_cause() {
    use std::error::Error;
    let io = EnvError::Io {
        op: "write",
        path: "/data/state.json".into(),
        source: std::io::Error::other("disk full"),
    };
    assert!(io.source().is_some());
    for (e, needle) in [
        (
            EnvError::InvalidApp("empty".into()),
            "invalid app name: empty",
        ),
        (EnvError::NoHome("home folder"), "home folder"),
        (io, "disk full"),
        (EnvError::NotAnObject, "JSON object"),
        (EnvError::Serialize("recursion".into()), "recursion"),
    ] {
        assert!(e.to_string().contains(needle), "{e}");
        if !matches!(e, EnvError::Io { .. }) {
            assert!(e.source().is_none(), "{e}");
        }
    }
}

#[test]
fn a_damaged_file_that_could_not_be_kept_aside_is_described_without_a_copy_name() {
    let none = Problem::Damaged {
        reason: "not JSON".into(),
        kept_as: None,
        recovered_from_backup: false,
    };
    let text = none.to_string();
    assert!(
        text.contains("not JSON") && text.contains("Defaults are in use."),
        "{text}"
    );
    assert!(!text.contains("kept as"), "{text}");
}

#[test]
fn a_migration_that_cannot_be_written_back_still_loads_and_is_retried_by_the_next_save() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("state.json"), r#"{"name":"a","count":3}"#).unwrap();
    // A folder where the old copy should go: the write-back fails, the value is still there.
    std::fs::create_dir(t.path().join("state.json.v1.bak")).unwrap();
    let f = v4(t.path());
    let l = f.load();
    assert_eq!(l.value.n, 30);
    assert_eq!((l.problem, l.migrated_from), (None, Some(1)));
    let on_disk = std::fs::read_to_string(f.path()).unwrap();
    assert!(
        on_disk.contains("\"count\":3"),
        "the old file is untouched: {on_disk}"
    );
}

#[test]
fn a_file_that_never_restores_ignores_a_valid_backup_and_leaves_nothing_in_its_place() {
    let t = tempfile::tempdir().unwrap();
    let f = file(t.path()).no_restore();
    f.save(&state("good", 1)).unwrap();
    f.save(&state("newer", 2)).unwrap();
    std::fs::write(f.path(), "garbage").unwrap();
    let l = f.load();
    assert_eq!(l.value, State::default(), "the older copy is not used");
    assert!(matches!(
        &l.problem,
        Some(Problem::Damaged {
            recovered_from_backup: false,
            ..
        })
    ));
    assert!(
        !f.path().exists(),
        "the damaged file was moved aside and nothing was written back"
    );
    assert!(names(t.path()).contains(&"state.json.corrupt-1000".to_string()));
    assert_eq!(
        f.load().problem,
        None,
        "the next start finds no file, not a restored copy"
    );
}
