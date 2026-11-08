//! The settings engine behind the app's "AI access (MCP)" page.

use paddock_jobs::FakeClock;
use paddock_log::Redactor;
use paddock_mcp::{
    AuditLog, Emit, ErrorCode, FileSettings, KeyStore, KeyVerifier, MAX_KEYS, McpControl, McpEvent,
    McpSettings, MemoryKeys, MemorySettings, Server, Session, SettingsStore, ToolDef, ToolOutput,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

struct Rig {
    control: McpControl,
    server: Arc<Server>,
    key_list: Arc<MemoryKeys>,
    store: Arc<MemorySettings>,
    events: Arc<Mutex<Vec<McpEvent>>>,
}

fn build(key_list: Arc<MemoryKeys>, store: Arc<MemorySettings>) -> Rig {
    let events: Arc<Mutex<Vec<McpEvent>>> = Arc::default();
    let sink_events = events.clone();
    let emit: Emit = Arc::new(move |e: &McpEvent| sink_events.lock().unwrap().push(e.clone()));
    let log = Arc::new(AuditLog::new(10));
    let sink = McpControl::announcing_sink(log.clone(), emit.clone());
    let server = Arc::new(Server::new(
        "Demo",
        "1",
        Arc::new(FakeClock::at_wall_ms(5)),
        sink,
        Arc::new(Redactor::new()),
    ));
    server
        .add_tool(ToolDef::read_only(
            "look",
            "Looks.",
            json!({"type":"object"}),
            |_, _| Ok(ToolOutput::text("seen")),
        ))
        .unwrap();
    server
        .add_tool(ToolDef::read_only(
            "peek",
            "Peeks.",
            json!({"type":"object"}),
            |_, _| Ok(ToolOutput::text("peeked")),
        ))
        .unwrap();
    server
        .add_tool(ToolDef::action(
            "change",
            "Changes.",
            json!({"type":"object"}),
            |_, _| Ok(ToolOutput::text("done")),
        ))
        .unwrap();
    let control = McpControl::new(
        server.clone(),
        log,
        Arc::new(KeyStore::new()),
        store.clone(),
        key_list.clone(),
        emit,
        Arc::new(|| 42),
    );
    Rig {
        control,
        server,
        key_list,
        store,
        events,
    }
}

fn rig() -> Rig {
    build(
        Arc::new(MemoryKeys::default()),
        Arc::new(MemorySettings::default()),
    )
}

fn settings(enabled: bool, off: &[&str], actions: &[&str]) -> McpSettings {
    McpSettings {
        enabled,
        resources: true,
        disabled_tools: off.iter().map(|s| s.to_string()).collect(),
        allowed_actions: actions.iter().map(|s| s.to_string()).collect(),
    }
}

fn exposed(r: &Rig) -> Vec<String> {
    r.control
        .get()
        .tools
        .into_iter()
        .filter(|t| t.exposed)
        .map(|t| t.name)
        .collect()
}

#[test]
fn it_starts_off_and_lists_every_tool() {
    let r = rig();
    let v = r.control.get();
    assert!(!v.settings.enabled && v.settings.resources);
    assert_eq!(
        v.tools
            .iter()
            .map(|t| (t.name.as_str(), t.action))
            .collect::<Vec<_>>(),
        [("change", true), ("look", false), ("peek", false)]
    );
    assert!(exposed(&r).is_empty(), "nothing is shown while it is off");
    assert_eq!(v.max_keys as usize, MAX_KEYS);
}

#[test]
fn saving_applies_to_the_running_server_and_actions_need_their_permission() {
    let r = rig();
    r.control.save(settings(true, &["peek"], &[])).unwrap();
    assert_eq!(exposed(&r), ["look"]);
    r.control.save(settings(true, &[], &["change"])).unwrap();
    assert_eq!(exposed(&r), ["change", "look", "peek"]);
    assert_eq!(
        r.store.load().allowed_actions,
        ["change"],
        "and it is stored"
    );
    r.control.save(settings(false, &[], &["change"])).unwrap();
    assert!(!r.server.policy().enabled);
    // The saved settings are in force after a restart.
    let again = build(r.key_list.clone(), r.store.clone());
    assert_eq!(again.control.get().settings, r.store.load());
}

#[test]
fn bad_settings_are_refused_with_what_to_fix_and_a_failed_save_changes_nothing() {
    let r = rig();
    let e = r.control.save(settings(true, &["ghost"], &[])).unwrap_err();
    assert_eq!((e.code, e.retryable), (ErrorCode::Invalid, false));
    assert!(e.message.contains("ghost"));
    let e = r.control.save(settings(true, &[], &["look"])).unwrap_err();
    assert!(e.message.contains("not an action tool"));
    let many: Vec<&str> = vec!["look"; 300];
    assert_eq!(
        r.control.save(settings(true, &many, &[])).unwrap_err().code,
        ErrorCode::Invalid
    );
    *r.store.fail.lock().unwrap() = Some("disk full".into());
    let e = r.control.save(settings(true, &[], &[])).unwrap_err();
    assert_eq!(
        (e.code, e.retryable, e.message.as_str()),
        (ErrorCode::StoreUnavailable, true, "disk full")
    );
    assert!(
        !r.server.policy().enabled,
        "the running policy did not change"
    );
}

#[test]
fn keys_are_issued_once_persisted_listed_without_the_key_and_revoked() {
    let r = rig();
    let issued = r.control.issue_key("  Pitwall ").unwrap();
    assert!(issued.key.starts_with("pmk_") && issued.client == "Pitwall");
    let view = r.control.get();
    assert_eq!(
        (
            view.keys.len(),
            view.keys[0].client.as_str(),
            view.keys[0].created_ms
        ),
        (1, "Pitwall", 42)
    );
    assert!(!serde_json::to_string(&view).unwrap().contains(&issued.key));
    assert_eq!(
        r.control.keys().verify(&issued.key).as_deref(),
        Some("Pitwall")
    );
    // A restart finds the key again, from the key list.
    let again = build(r.key_list.clone(), r.store.clone());
    assert_eq!(
        again.control.keys().verify(&issued.key).as_deref(),
        Some("Pitwall")
    );
    let view = r.control.revoke_key(issued.id).unwrap();
    assert!(view.keys[0].revoked);
    assert_eq!(
        build(r.key_list.clone(), r.store.clone())
            .control
            .keys()
            .verify(&issued.key),
        None,
        "the revocation was persisted"
    );
}

#[test]
fn bad_client_names_the_key_limit_and_unknown_ids_are_refused() {
    let r = rig();
    for bad in ["", "   ", "tab\there", &"x".repeat(65)] {
        assert_eq!(
            r.control.issue_key(bad).unwrap_err().code,
            ErrorCode::Invalid,
            "{bad:?}"
        );
    }
    for i in 0..MAX_KEYS {
        r.control.issue_key(&format!("c{i}")).unwrap();
    }
    let e = r.control.issue_key("one more").unwrap_err();
    assert!(e.message.contains("Revoke one"), "{}", e.message);
    r.control.revoke_key(1).unwrap();
    assert!(
        r.control.issue_key("fits again").is_ok(),
        "a revoked key frees a place"
    );
    assert_eq!(
        r.control.revoke_key(999).unwrap_err().code,
        ErrorCode::Invalid
    );
}

#[test]
fn an_unavailable_key_file_fails_the_key_commands_and_keeps_the_old_list() {
    let r = rig();
    let kept = r.control.issue_key("keeps").unwrap();
    *r.key_list.fail.lock().unwrap() = Some("read-only folder".into());
    let e = r.control.issue_key("lost").unwrap_err();
    assert_eq!((e.code, e.retryable), (ErrorCode::StoreUnavailable, true));
    assert!(e.message.contains("read-only folder") && e.message.contains("settings folder"));
    assert_eq!(
        r.control.get().keys.len(),
        1,
        "the key that could not be saved was not issued"
    );
    assert_eq!(
        r.control.revoke_key(kept.id).unwrap_err().code,
        ErrorCode::StoreUnavailable
    );
    assert!(
        !r.control.get().keys[0].revoked,
        "and the revoke was rolled back"
    );
    *r.key_list.fail.lock().unwrap() = None;
    assert!(r.control.revoke_key(kept.id).is_ok());
}

#[test]
fn calls_are_listed_newest_first_and_every_change_is_announced() {
    let r = rig();
    r.control.save(settings(true, &[], &[])).unwrap();
    let s = Session::new("Pitwall");
    r.server.handle(
        &s,
        r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}"#,
    );
    for tool in ["look", "peek"] {
        r.server.handle(
            &s,
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool}})
                .to_string(),
        );
    }
    let rows = r.control.audit();
    assert_eq!(
        rows.iter().map(|x| x.tool.as_str()).collect::<Vec<_>>(),
        ["peek", "look"]
    );
    assert_eq!(
        (rows[0].client.as_str(), rows[0].at_ms, rows[0].result_bytes),
        ("Pitwall", 5, 6)
    );
    assert_eq!(
        *r.events.lock().unwrap(),
        [McpEvent::Audit { count: 1 }, McpEvent::Audit { count: 2 }]
    );
    r.control.clear_audit();
    assert!(r.control.audit().is_empty());
    assert_eq!(
        r.events.lock().unwrap().last(),
        Some(&McpEvent::Audit { count: 0 })
    );
}

#[test]
fn the_file_store_round_trips_and_a_damaged_file_gives_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcp.json");
    let store = FileSettings::new(&path);
    assert_eq!(store.load(), McpSettings::default());
    store.save(&settings(true, &["peek"], &["change"])).unwrap();
    assert_eq!(
        FileSettings::new(&path).load(),
        settings(true, &["peek"], &["change"])
    );
    std::fs::write(&path, "{ broken").unwrap();
    assert_eq!(FileSettings::new(&path).load(), McpSettings::default());
    let blocked = FileSettings::new(dir.path().join("a-file").join("mcp.json"));
    std::fs::write(dir.path().join("a-file"), "x").unwrap();
    assert!(
        blocked
            .save(&McpSettings::default())
            .unwrap_err()
            .contains("could not be saved")
    );
}
