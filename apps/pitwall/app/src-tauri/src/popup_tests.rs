use super::*;
use pitwall_core::widgets::{PopoverStatus, ServerState};
use serde_json::json;

const NOW: i64 = 1_800_000_000_000;

fn kinds(p: &PopupState) -> Vec<PopoverWidget> {
    p.view().layout.placements.iter().map(|x| x.widget).collect()
}

fn id_of(p: &PopupState, kind: PopoverWidget) -> paddock_widgets::PlacementId {
    p.view().layout.placements.iter().find(|x| x.widget == kind).unwrap().id.clone()
}

fn overview() -> Overview {
    serde_json::from_value(json!({
        "server": {"version": "1.1.0", "apiVersion": 1, "pid": 1, "port": 11435, "startedAt": 0, "serving": "serving",
                   "lan": {"enabled": false, "url": null, "certFingerprint": null}},
        "memory": {"at": 0, "ramTotal": 100, "ramAvailable": 40, "commitUsed": null, "commitLimit": null, "swapUsed": 0,
                   "pressure": "normal", "cpuPercent": 1.0, "gpus": [], "pitwallRss": 1, "foregroundBusy": false},
        "models": [], "engines": [],
        "queue": {"running": [], "waiting": [], "depth": {"interactive": 0, "background": 0, "batch": 0}},
        "throughput": {"promptTps": 0.0, "genTps": 0.0, "ttftP50Ms": 0.0, "requestsPerMin": 0.0, "activeRequests": 0},
        "apps": [], "preset": "balanced", "pairings": [], "issues": []
    }))
    .unwrap()
}

#[test]
fn a_fresh_state_shows_the_default_popup() {
    let dir = tempfile::tempdir().unwrap();
    let p = PopupState::load(Some(dir.path().to_path_buf()));
    assert_eq!(
        kinds(&p),
        [
            PopoverWidget::Server,
            PopoverWidget::Activity,
            PopoverWidget::Memory,
            PopoverWidget::LoadedModels
        ]
    );
    let v = p.view();
    assert!(v.editable && !v.can_undo);
    assert_eq!(v.catalogue.kinds.len(), 7);
    assert!(!dir.path().join(FILE_NAME).exists(), "nothing is written until the first edit");
}

#[test]
fn an_edit_is_saved_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let p = PopupState::load(Some(dir.path().to_path_buf()));
    let view = p
        .edit(Edit::Remove {
            id: id_of(&p, PopoverWidget::Memory),
        })
        .unwrap();
    assert!(view.can_undo);
    assert_eq!(kinds(&p).len(), 3);
    drop(p);
    let again = PopupState::load(Some(dir.path().to_path_buf()));
    assert!(!kinds(&again).contains(&PopoverWidget::Memory), "the layout is in {FILE_NAME}");
    assert_eq!(kinds(&again).len(), 3);
}

#[test]
fn a_refused_edit_changes_nothing_in_memory_or_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let p = PopupState::load(Some(dir.path().to_path_buf()));
    let before = kinds(&p);
    let err = p
        .edit(Edit::Add {
            widget: PopoverWidget::Memory,
            span: None,
            at: None,
        })
        .unwrap_err();
    assert!(matches!(err, EditError::Refused(_)), "{err:?}");
    assert_eq!(kinds(&p), before);
    assert!(!dir.path().join(FILE_NAME).exists());
    assert_eq!(CommandError::from(err).code, ErrorCode::BadRequest);
}

#[test]
fn an_unwritable_folder_refuses_the_edit_and_keeps_the_layout() {
    let dir = tempfile::tempdir().unwrap();
    // The "folder" is a file: creating it fails.
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, "x").unwrap();
    let p = PopupState::load(Some(blocker));
    let before = kinds(&p);
    let err = p
        .edit(Edit::Remove {
            id: id_of(&p, PopoverWidget::Memory),
        })
        .unwrap_err();
    assert!(matches!(err, EditError::NotSaved(_)), "{err:?}");
    assert_eq!(kinds(&p), before);
    assert_eq!(CommandError::from(err).code, ErrorCode::Internal);
}

#[test]
fn a_damaged_or_foreign_file_gives_the_defaults() {
    for text in ["not json at all", "[1, 2, 3]", "{\"version\": 0}", ""] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), text).unwrap();
        let p = PopupState::load(Some(dir.path().to_path_buf()));
        assert_eq!(kinds(&p).len(), 4, "{text:?}");
    }
}

#[test]
fn a_file_over_the_limit_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(FILE_NAME), " ".repeat(64 * 1024 + 1)).unwrap();
    assert_eq!(kinds(&PopupState::load(Some(dir.path().to_path_buf()))).len(), 4);
}

#[test]
fn a_layout_from_a_newer_version_is_kept_and_cannot_be_edited() {
    let dir = tempfile::tempdir().unwrap();
    let newer = json!({"version": 99, "placements": [{"id": "memory", "span": 2, "widget": "memory"}], "presets": []});
    std::fs::write(dir.path().join(FILE_NAME), newer.to_string()).unwrap();
    let p = PopupState::load(Some(dir.path().to_path_buf()));
    assert!(!p.view().editable);
    assert_eq!(kinds(&p).len(), 4, "the defaults are shown");
    assert!(matches!(
        p.edit(Edit::Remove {
            id: id_of(&p, PopoverWidget::Memory)
        }),
        Err(EditError::Refused(_))
    ));
    assert_eq!(
        std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap(),
        newer.to_string(),
        "the file is untouched"
    );
}

#[test]
fn the_values_follow_the_facts_and_the_header_follows_the_server() {
    let p = PopupState::load(None);
    assert_eq!(p.header().status, PopoverStatus::Unknown);
    assert_eq!(p.values(NOW).placements.len(), 4);
    seed(&p, &overview());
    assert_eq!(p.header().status, PopoverStatus::Serving { port: 11435 });
    let memory = p.values(NOW).placements.into_iter().find(|x| x.id.as_str() == "memory").unwrap();
    assert!(matches!(
        memory.state,
        paddock_widgets::WidgetState::Ready {
            value: pitwall_core::widgets::PopoverValue::Memory { percent: 60, .. },
            ..
        }
    ));
    p.with_facts(|f| f.set_disconnected(NOW));
    assert_eq!(p.header().status, PopoverStatus::Unreachable);
    let server = p.values(NOW).placements.into_iter().find(|x| x.id.as_str() == "server").unwrap();
    assert!(matches!(
        server.state,
        paddock_widgets::WidgetState::Ready {
            value: pitwall_core::widgets::PopoverValue::Server {
                state: ServerState::Unreachable,
                ..
            },
            ..
        }
    ));
}

#[test]
fn the_windows_hear_only_what_changed() {
    let p = PopupState::load(None);
    assert!(p.changed(NOW).is_some(), "the first look is news");
    assert!(p.changed(NOW).is_none(), "nothing changed");
    assert!(p.changed(NOW + 5_000).is_none(), "time passing alone is not news");
    seed(&p, &overview());
    assert!(p.changed(NOW).is_some());
    p.edit(Edit::Remove {
        id: id_of(&p, PopoverWidget::Activity),
    })
    .unwrap();
    assert!(p.changed(NOW).is_some(), "a layout change is news");
    assert!(p.changed(NOW).is_none());
}

#[test]
fn only_a_model_update_event_asks_for_the_notices_again() {
    let event = |kind: &str| -> LiveUpdate {
        serde_json::from_value(json!({"kind": "event", "event": {
            "id": 1, "at": 0, "kind": kind, "severity": "info", "message": "m", "modelId": null, "appId": null, "reason": null}}))
        .unwrap()
    };
    assert!(touches_updates(&event("modelUpdate")));
    assert!(!touches_updates(&event("requestFailed")));
    assert!(!touches_updates(&LiveUpdate::Throughput {
        throughput: Default::default()
    }));
}

#[test]
fn the_push_has_the_three_keys_the_ui_reads() {
    let p = PopupState::load(None);
    let v = serde_json::to_value(p.snapshot(NOW)).unwrap();
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["header", "values", "view"]);
}

mod reads {
    use super::*;
    use crate::server_client::fake::{FakeServer, Reply};
    use paddock_widgets::WidgetState;
    use pitwall_core::widgets::PopoverValue;

    fn updates_state(p: &PopupState) -> PopoverState {
        p.values(NOW)
            .placements
            .into_iter()
            .find(|x| x.id.as_str() == "updates")
            .unwrap()
            .state
    }

    type PopoverState = paddock_widgets::WidgetState<PopoverValue, pitwall_core::widgets::WidgetActionId>;

    fn with_updates_widget() -> PopupState {
        let p = PopupState::load(None);
        seed(&p, &overview());
        p.edit(Edit::Add {
            widget: PopoverWidget::Updates,
            span: None,
            at: None,
        })
        .unwrap();
        p
    }

    fn notice() -> serde_json::Value {
        json!({"id": "m#betterQuant", "modelId": "m", "repo": "a/b", "kind": "betterQuant", "file": "m-Q6.gguf", "quant": "Q6_K", "sizeBytes": 5,
               "installedFile": "m-Q4.gguf", "installedQuant": "Q4_K_M", "summary": "A better quantisation of m is available.", "foundAt": 1, "targetSha256": null})
    }

    fn status(last_checked: Option<i64>) -> serde_json::Value {
        json!({"settings": {"enabled": true, "schedule": "weekly"}, "lastCheckedAt": last_checked, "lastAttemptAt": null, "lastMessage": null,
               "nextCheckAt": null, "running": false, "notices": 1, "host": "huggingface.co", "maxRepos": 50})
    }

    #[tokio::test]
    async fn the_update_notices_and_the_time_of_the_last_check_reach_the_widget() {
        let server = FakeServer::new();
        server.reply("update_notices", Reply::Ok(json!([notice()])));
        server.reply("update_check_status", Reply::Ok(status(Some(NOW))));
        let p = with_updates_widget();
        refresh_updates(&p, &server).await;
        let WidgetState::Ready { value, .. } = updates_state(&p) else {
            panic!("ready")
        };
        assert_eq!(
            value,
            PopoverValue::Updates {
                count: 1,
                summary: Some("A better quantisation of m is available.".into())
            }
        );

        server.reply("update_notices", Reply::Ok(json!([])));
        refresh_updates(&p, &server).await;
        assert!(
            matches!(updates_state(&p), WidgetState::Empty { calm: true, .. }),
            "checked, nothing on offer"
        );
    }

    #[tokio::test]
    async fn an_unreadable_answer_leaves_the_widget_saying_not_checked_yet() {
        let p = with_updates_widget();
        let server = FakeServer::new();
        // The server is not scripted: it answers null, which is neither a list nor a status.
        refresh_updates(&p, &server).await;
        assert!(matches!(updates_state(&p), WidgetState::Empty { calm: false, .. }));
        server.reply("update_notices", Reply::Err(crate::server_client::unavailable()));
        server.reply("update_check_status", Reply::Ok(status(None)));
        refresh_updates(&p, &server).await;
        assert!(
            matches!(updates_state(&p), WidgetState::Empty { calm: false, .. }),
            "a failed read does not guess"
        );
    }

    fn state_of(p: &PopupState, id: &str) -> PopoverState {
        p.values(NOW).placements.into_iter().find(|x| x.id.as_str() == id).unwrap().state
    }

    fn with_downloads_and_errors() -> PopupState {
        let p = PopupState::load(None);
        seed(&p, &overview());
        p.edit(Edit::Add {
            widget: PopoverWidget::Downloads,
            span: None,
            at: None,
        })
        .unwrap();
        p.edit(Edit::Add {
            widget: PopoverWidget::RecentErrors,
            span: None,
            at: None,
        })
        .unwrap();
        p
    }

    #[tokio::test]
    async fn failed_reads_of_downloads_and_events_never_become_a_calm_nothing() {
        let p = with_downloads_and_errors();
        let server = FakeServer::new();
        server.reply("downloads", Reply::Err(crate::server_client::unavailable()));
        server.reply("events", Reply::Ok(json!("not a list")));
        read_extras(&p, &server).await;
        for id in ["downloads", "recent-errors"] {
            let WidgetState::Empty { calm, detail, .. } = state_of(&p, id) else {
                panic!("{id} is empty")
            };
            assert!(!calm, "{id}: a failed read is not 'all good'");
            assert_eq!(detail, "Not checked yet.", "{id}");
        }
    }

    #[tokio::test]
    async fn successful_reads_make_nothing_to_report_calm() {
        let p = with_downloads_and_errors();
        let server = FakeServer::new();
        server.reply("downloads", Reply::Ok(json!([])));
        server.reply("events", Reply::Ok(json!([])));
        read_extras(&p, &server).await;
        for id in ["downloads", "recent-errors"] {
            assert!(matches!(state_of(&p, id), WidgetState::Empty { calm: true, .. }), "{id}");
        }
    }
}
