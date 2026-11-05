//! The client key list lives in an owner-only file in the config folder, never in the OS credential store (Dyno 1.1.0: the
//! Keychain prompt after an update blocked the app before its first window).

use paddock_jobs::FakeClock;
use paddock_log::Redactor;
use paddock_mcp::{
    AuditLog, ErrorCode, FileKeys, KeyEntry, KeyListStore, KeyStore, KeyVerifier, McpControl,
    MemorySettings, Server,
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
}

#[test]
fn the_view_says_why_the_saved_keys_are_not_in_use_until_a_new_key_is_saved() {
    let (_dir, path) = keys_file();
    assert_eq!(
        control(&path).get().keys_problem,
        None,
        "no file is not a problem"
    );
    std::fs::write(&path, "{not json").unwrap();
    let c = control(&path);
    let why = c.get().keys_problem.expect("a damaged list is explained");
    assert!(
        why.contains("not valid JSON")
            && why.contains("kept as")
            && why.contains("No client can connect"),
        "{why}"
    );
    assert!(
        !why.contains("last good copy"),
        "the damaged list's older copy is not used, and the text must not say it is: {why}"
    );
    c.issue_key("again").unwrap();
    assert_eq!(
        c.get().keys_problem,
        None,
        "a saved list is the list in use"
    );
}

#[test]
fn each_problem_is_worded_for_what_happened_and_what_to_do() {
    let (_dir, path) = keys_file();
    std::fs::write(&path, r#"{"version": 99, "keys": []}"#).unwrap();
    let newer = FileKeys::new(&path).load().problem.unwrap();
    assert!(
        newer.contains("newer version")
            && newer.contains("Update the app")
            && !newer.contains("Defaults"),
        "{newer}"
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let unreadable = FileKeys::new(&path).load().problem.unwrap();
    assert!(
        unreadable.contains("could not be opened")
            && unreadable.contains("move it away")
            && unreadable.contains("restart"),
        "{unreadable}"
    );
}

#[cfg(unix)]
#[test]
fn a_list_that_others_can_write_is_not_trusted() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, path) = keys_file();
    let first = control(&path);
    let key = first.issue_key("Pitwall").unwrap();
    for mode in [0o666, 0o662, 0o620, 0o602] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        let c = control(&path);
        assert_eq!(c.keys().verify(&key.key), None, "mode {mode:o} is refused");
        let why = c.get().keys_problem.unwrap_or_default();
        assert!(
            why.contains("other users") && why.contains("chmod 600") && why.contains("restart"),
            "{why}"
        );
    }
    for mode in [0o600, 0o644, 0o640] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            control(&path).keys().verify(&key.key).as_deref(),
            Some("Pitwall"),
            "mode {mode:o} is accepted"
        );
    }
}

#[test]
fn a_fixed_cause_works_without_a_restart() {
    // Unreadable (a folder in the file's place): refused, then the folder is removed and the same control issues a key.
    let (_dir, path) = keys_file();
    std::fs::create_dir(&path).unwrap();
    let c = control(&path);
    assert_eq!(
        c.issue_key("x").unwrap_err().code,
        ErrorCode::StoreUnavailable
    );
    std::fs::remove_dir(&path).unwrap();
    let issued = c
        .issue_key("x")
        .expect("the retry works once the cause is gone");
    assert_eq!(
        control(&path).keys().verify(&issued.key).as_deref(),
        Some("x")
    );
    assert_eq!(c.get().keys_problem, None);

    // Newer version: refused, then the file is moved away to start fresh.
    let (_dir2, path2) = keys_file();
    std::fs::write(&path2, r#"{"version": 99, "keys": []}"#).unwrap();
    let c = control(&path2);
    assert!(c.issue_key("x").is_err());
    std::fs::rename(&path2, path2.with_extension("old")).unwrap();
    assert!(
        c.issue_key("x").is_ok(),
        "moving the file away starts fresh"
    );
}

/// Three clicks on "Add client": every one refuses with the restart advice, and the file stays exactly as the user left it.
fn three_clicks_refuse(c: &McpControl, path: &Path, left: &[u8]) {
    for click in 1..=3 {
        let e = c.issue_key("x").unwrap_err();
        assert!(
            e.message.contains("Restart the app"),
            "click {click}: {}",
            e.message
        );
        assert!(!e.message.contains(".."), "click {click}: {}", e.message);
        assert_eq!(
            std::fs::read(path).unwrap(),
            left,
            "click {click} changed the file"
        );
    }
    let mut backup = path.to_path_buf().into_os_string();
    backup.push(".bak");
    assert!(
        !Path::new(&backup).exists() || std::fs::read(&backup).unwrap() != b"",
        "the backup is not the user's list pushed out"
    );
}

fn good_list() -> Vec<u8> {
    let (_d, p) = keys_file();
    control(&p).issue_key("Pitwall").unwrap();
    std::fs::read(&p).unwrap()
}

#[test]
fn a_list_restored_by_hand_after_a_refusal_is_never_overwritten() {
    let good = good_list();
    let (_dir, path) = keys_file();
    std::fs::write(&path, r#"{"version": 99, "keys": []}"#).unwrap();
    let c = control(&path);
    assert!(c.issue_key("x").is_err());
    std::fs::write(&path, &good).unwrap();
    three_clicks_refuse(&c, &path, &good);
}

#[test]
fn a_list_restored_by_hand_after_a_damaged_file_at_start_is_never_overwritten() {
    let good = good_list();
    let (_dir, path) = keys_file();
    std::fs::write(&path, "{not json").unwrap();
    let c = control(&path);
    std::fs::write(&path, &good).unwrap();
    three_clicks_refuse(&c, &path, &good);
}

#[cfg(unix)]
#[test]
fn a_refused_writable_list_fixed_with_chmod_is_never_overwritten_before_a_restart() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, path) = keys_file();
    let key = control(&path).issue_key("Pitwall").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    let c = control(&path);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let left = std::fs::read(&path).unwrap();
    three_clicks_refuse(&c, &path, &left);
    assert_eq!(
        control(&path).keys().verify(&key.key).as_deref(),
        Some("Pitwall"),
        "a restart reads it"
    );
}

#[cfg(unix)]
#[test]
fn a_plain_write_failure_says_what_failed_not_that_the_file_changed() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("config");
    std::fs::create_dir(&folder).unwrap();
    let path = folder.join("mcp-keys.json");
    let c = control(&path);
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o500)).unwrap();
    let e = c.issue_key("x").unwrap_err();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        !e.message.contains("Restart the app") && !e.message.contains(".."),
        "{}",
        e.message
    );
    assert!(
        e.message.to_lowercase().contains("permission denied"),
        "{}",
        e.message
    );
}

#[cfg(unix)]
#[test]
fn a_folder_left_group_writable_by_a_umask_of_002_is_made_private_and_the_keys_survive_a_restart() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
    let (dir, path) = keys_file();
    // What `create_dir_all` makes under umask 002 (Ubuntu and Fedora desktops): 0775.
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o775)).unwrap();
    let key = control(&path).issue_key("Pitwall").unwrap();
    assert_eq!(
        mode(dir.path()),
        0o700,
        "the folder is private after the first save"
    );
    assert_eq!(
        control(&path).keys().verify(&key.key).as_deref(),
        Some("Pitwall"),
        "the key survives a restart"
    );
    // A folder left 0775 by an older run is fixed when it is read, so the list is not refused.
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o775)).unwrap();
    let again = control(&path);
    assert_eq!(again.keys().verify(&key.key).as_deref(), Some("Pitwall"));
    assert_eq!(again.get().keys_problem, None);
    assert_eq!(mode(dir.path()), 0o700);
    // A folder that has to be made is private whatever the umask.
    let nested = dir.path().join("a").join("b").join("mcp-keys.json");
    control(&nested).issue_key("x").unwrap();
    assert_eq!(mode(nested.parent().unwrap()), 0o700);
}

#[cfg(unix)]
#[test]
fn a_sticky_shared_folder_is_left_alone() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, path) = keys_file();
    let key = control(&path).issue_key("Pitwall").unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o1777)).unwrap();
    assert_eq!(
        control(&path).keys().verify(&key.key).as_deref(),
        Some("Pitwall")
    );
    assert_eq!(
        std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o7777,
        0o1777,
        "a /tmp-like folder is not chmodded"
    );
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
}
