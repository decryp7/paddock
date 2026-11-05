//! The client key list lives in an owner-only file in the config folder, never in the OS credential store (Dyno 1.1.0: the
//! Keychain prompt after an update blocked the app before its first window).

use paddock_jobs::FakeClock;
use paddock_log::Redactor;
use paddock_mcp::{
    AuditLog, ErrorCode, FileKeys, FileSettings, KeyEntry, KeyListStore, KeyStore, KeyVerifier,
    McpControl, MemorySettings, Server,
};
use std::path::Path;
use std::sync::Arc;

fn control(path: &Path) -> McpControl {
    let log = Arc::new(AuditLog::new(10));
    let server = Arc::new(Server::new(
        "Demo",
        "1",
        Arc::new(FakeClock::at_wall_ms(5)),
        McpControl::announcing_sink(log.clone(), Arc::new(|_| {})),
        Arc::new(Redactor::new()),
    ));
    McpControl::new(
        server,
        log,
        Arc::new(KeyStore::new()),
        Arc::new(MemorySettings::default()),
        Arc::new(FileKeys::new(path)),
        Arc::new(|_| {}),
        Arc::new(|| 42),
    )
}

fn keys_file() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcp-keys.json");
    (dir, path)
}

#[test]
fn issued_keys_survive_a_restart_from_the_file_and_a_revoked_key_stays_revoked() {
    let (_dir, path) = keys_file();
    let first = control(&path);
    let kept = first.issue_key("Pitwall").unwrap();
    let revoked = first.issue_key("old script").unwrap();
    first.revoke_key(revoked.id).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("Pitwall") && text.contains("sha256"),
        "{text}"
    );
    assert!(
        !text.contains(&kept.key) && !text.contains(&revoked.key),
        "only the hashes are on disk"
    );

    let second = control(&path);
    assert_eq!(second.keys().verify(&kept.key).as_deref(), Some("Pitwall"));
    assert_eq!(
        second.keys().verify(&revoked.key),
        None,
        "revoked stays revoked"
    );
    let view = second.get();
    assert_eq!(
        view.keys
            .iter()
            .map(|k| (k.id, k.revoked))
            .collect::<Vec<_>>(),
        [(kept.id, false), (revoked.id, true)]
    );
}

#[cfg(unix)]
#[test]
fn the_keys_file_and_its_backup_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, path) = keys_file();
    let c = control(&path);
    c.issue_key("one").unwrap();
    c.issue_key("two").unwrap();
    let mut backup = path.clone().into_os_string();
    backup.push(".bak");
    for file in [path.as_path(), Path::new(&backup)] {
        let mode = std::fs::metadata(file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{} is {mode:o}", file.display());
    }
}

#[test]
fn a_damaged_file_refuses_every_key_is_kept_aside_and_new_keys_can_be_issued() {
    let (dir, path) = keys_file();
    let issued = control(&path).issue_key("Pitwall").unwrap();
    std::fs::write(&path, "{not json").unwrap();
    let c = control(&path);
    assert!(c.get().keys.is_empty());
    assert_eq!(c.keys().verify(&issued.key), None, "no key is accepted");
    let kept_aside = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
    assert!(kept_aside, "the damaged file is kept for inspection");
    let fresh = c.issue_key("again").unwrap();
    assert_eq!(
        control(&path).keys().verify(&fresh.key).as_deref(),
        Some("again")
    );
}

#[test]
fn a_damaged_file_never_brings_back_a_key_from_the_backup() {
    let (_dir, path) = keys_file();
    let first = control(&path);
    let old = first.issue_key("old").unwrap();
    first.issue_key("newer").unwrap();
    first.revoke_key(old.id).unwrap();
    // The backup now holds the list from before the revoke: `old` is still active in it.
    let mut backup = path.clone().into_os_string();
    backup.push(".bak");
    assert!(
        std::fs::read_to_string(Path::new(&backup))
            .unwrap()
            .contains("\"revoked\": false")
    );
    std::fs::write(&path, "garbage").unwrap();
    let after = control(&path);
    assert_eq!(after.keys().verify(&old.key), None);
    assert!(!after.keys().has_active());
    assert!(
        !control(&path).keys().has_active(),
        "and the next start does not read the restored copy as good"
    );
}

#[test]
fn a_file_from_a_newer_version_is_left_alone_refuses_keys_and_says_what_to_do() {
    let (_dir, path) = keys_file();
    let text = r#"{"version": 99, "keys": []}"#;
    std::fs::write(&path, text).unwrap();
    let c = control(&path);
    let load = FileKeys::new(&path).load();
    assert!(load.entries.is_empty());
    assert!(load.problem.unwrap().contains("Update the app"));
    let e = c.issue_key("x").unwrap_err();
    assert_eq!((e.code, e.retryable), (ErrorCode::StoreUnavailable, true));
    assert!(
        e.message.contains("could not be saved") && e.message.contains("update the app"),
        "{}",
        e.message
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        text,
        "the file is untouched"
    );
}

#[test]
fn an_unreadable_file_does_not_take_the_app_down_and_refuses_keys() {
    let (_dir, path) = keys_file();
    std::fs::create_dir(&path).unwrap(); // a folder in the file's place
    let c = control(&path);
    assert!(c.get().keys.is_empty());
    assert_eq!(c.keys().verify("pmk_anything"), None);
    let load = FileKeys::new(&path).load();
    assert!(load.problem.unwrap().contains("No client can connect"));
    assert_eq!(
        c.issue_key("x").unwrap_err().code,
        ErrorCode::StoreUnavailable
    );
}

#[test]
fn the_list_holds_nothing_secret() {
    // The type that is written: an id, a name, a hash, a time and a flag (a new field would show up here).
    let entry = KeyEntry {
        id: 1,
        client: "c".into(),
        sha256: "h".into(),
        created_ms: 1,
        revoked: false,
    };
    let value = serde_json::to_value(&entry).unwrap();
    let mut names: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
    names.sort();
    assert_eq!(names, ["client", "created_ms", "id", "revoked", "sha256"]);
    let _ = FileSettings::new("unused"); // the settings file is a different file
}
