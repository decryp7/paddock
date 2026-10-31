use super::*;
use crate::mgmt::{EngineIssue, EngineSetupState, LiveUpdate, Overview};
use crate::widgets::facts::{Facts, UpdatesFact};
use crate::widgets::popover::PopoverCatalogue;
use paddock_widgets::{Edit, Stored};
use serde_json::{json, Value};

/// The test clock (milliseconds since the epoch).
const NOW: i64 = 1_800_000_000_000;
const GB: u64 = 1024 * 1024 * 1024;
const MIN: i64 = 60_000;
const HOUR: i64 = 3_600_000;

fn model(id: &str, name: &str, ram: u64, vram: u64, state: &str, active: u32, last_used: i64) -> Value {
    json!({
        "modelId": id, "name": name, "engine": "llama.cpp", "build": "vulkan", "state": state,
        "ramBytes": ram, "vramBytes": vram, "contextTokens": 4096, "pinned": false, "activeRequests": active,
        "loadedAt": 0, "lastUsedAt": last_used, "idleReleaseAt": null, "coldStartMs": 0, "capabilities": []
    })
}

fn server(serving: &str) -> Value {
    json!({
        "version": "1.1.0", "apiVersion": 1, "pid": 1, "port": 11435, "startedAt": 0, "serving": serving,
        "lan": {"enabled": false, "url": null, "certFingerprint": null}
    })
}

fn memory(total: u64, available: u64, pressure: &str, gpus: Value) -> Value {
    json!({
        "at": 0, "ramTotal": total, "ramAvailable": available, "commitUsed": null, "commitLimit": null, "swapUsed": 0,
        "pressure": pressure, "cpuPercent": 3.0, "gpus": gpus, "pitwallRss": GB, "foregroundBusy": false
    })
}

fn tile(availability: Value) -> Value {
    json!({"kind": "llama.cpp", "name": "llama.cpp", "availability": availability, "activeBuild": "vulkan", "processes": 1, "unhealthy": 0})
}

fn overview(serving: &str, models: Vec<Value>, mem: Value) -> Overview {
    serde_json::from_value(json!({
        "server": server(serving), "memory": mem, "models": models, "engines": [tile(json!({"state": "available"}))],
        "queue": {"running": [], "waiting": [], "depth": {"interactive": 0, "background": 0, "batch": 0}},
        "throughput": {"promptTps": 0.0, "genTps": 0.0, "ttftP50Ms": 0.0, "requestsPerMin": 0.0, "activeRequests": 0},
        "apps": [], "preset": "balanced", "pairings": [], "issues": []
    }))
    .unwrap()
}

fn calm_memory() -> Value {
    memory(16 * GB, 12 * GB, "normal", json!([]))
}

fn connected(serving: &str, models: Vec<Value>) -> Facts {
    let mut f = Facts::new();
    f.seed(&overview(serving, models, calm_memory()), NOW);
    f
}

fn download(id: &str, name: &str, state: &str, done: u64, total: u64, bps: u64) -> crate::mgmt::Download {
    serde_json::from_value(json!({
        "id": id, "kind": "model", "name": name, "host": "huggingface.co", "bytesDone": done, "bytesTotal": total,
        "speedBps": bps, "state": state, "error": null, "startedAt": 0
    }))
    .unwrap()
}

fn event(id: u64, severity: &str, at: i64, message: &str) -> crate::events::Event {
    serde_json::from_value(json!({
        "id": id, "at": at, "kind": "requestFailed", "severity": severity, "message": message,
        "modelId": null, "appId": null, "reason": null
    }))
    .unwrap()
}

fn issue(state: &str) -> EngineIssue {
    serde_json::from_value(json!({"modelId": "m", "state": state, "crashes": 3, "restartAt": null, "message": "It crashed."})).unwrap()
}

fn value_of(s: PopoverState) -> PopoverValue {
    match s {
        WidgetState::Ready { value, .. } => value,
        other => panic!("expected a ready value, got {other:?}"),
    }
}

/// An empty state's title, detail, next action and calm flag.
fn empty_of(s: PopoverState) -> (String, String, Option<WidgetActionId>, bool) {
    match s {
        WidgetState::Empty { title, detail, next, calm } => (title, detail, next.map(|n| n.action), calm),
        other => panic!("expected an empty state, got {other:?}"),
    }
}

// ---- the loaded models ----

#[test]
fn loaded_models_list_the_busy_one_first_then_the_most_recent_and_cap_at_two_rows() {
    let f = connected(
        "serving",
        vec![
            model("a", "Old", GB, 0, "ready", 0, 10),
            model("b", "Recent", 2 * GB, 0, "ready", 0, 90),
            model("c", "Working", 3 * GB, GB, "busy", 1, 5),
            model("d", "Middle", GB, 0, "ready", 0, 50),
            model("e", "Oldest", GB, 0, "ready", 0, 1),
        ],
    );
    let PopoverValue::LoadedModels { total, rows, more, bytes } = value_of(state(PopoverWidget::LoadedModels, &f, NOW)) else {
        panic!("not models")
    };
    assert_eq!((total, rows.len(), more), (5, 2, 3));
    assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Working", "Recent"]);
    assert_eq!(rows[0].bytes, 4 * GB, "RAM and video memory together");
    assert_eq!(rows[0].state, ModelRowState::Busy);
    assert_eq!(bytes, 9 * GB, "all five models, not only the listed rows");
}

#[test]
fn no_loaded_model_is_an_empty_state_that_opens_models() {
    let f = connected("serving", vec![]);
    let (title, detail, next, calm) = empty_of(state(PopoverWidget::LoadedModels, &f, NOW));
    assert_eq!((title.as_str(), detail.as_str(), calm), ("See what is in memory", "No model is loaded.", false));
    assert_eq!(next, Some(WidgetActionId::OpenPage { page: Page::Models }));
}

#[test]
fn model_states_map_to_the_rows_word() {
    let states = |a: &str, b: &str| {
        let f = connected("serving", vec![model("a", "A", GB, 0, a, 0, 4), model("b", "B", GB, 0, b, 0, 3)]);
        let PopoverValue::LoadedModels { rows, .. } = value_of(state(PopoverWidget::LoadedModels, &f, NOW)) else { panic!() };
        rows.iter().map(|r| r.state).collect::<Vec<_>>()
    };
    assert_eq!(states("loading", "releasingContext"), [ModelRowState::Loading, ModelRowState::Releasing]);
    assert_eq!(states("unloading", "failed"), [ModelRowState::Releasing, ModelRowState::Failed]);
    assert_eq!(states("ready", "busy"), [ModelRowState::Busy, ModelRowState::Ready], "the busy one comes first");
}

// ---- memory ----

fn memory_percent(used: u64) -> (u8, MeterTone) {
    let mut f = Facts::new();
    f.seed(&overview("serving", vec![], memory(100, 100 - used, "normal", json!([]))), NOW);
    let PopoverValue::Memory { percent, tone, .. } = value_of(state(PopoverWidget::Memory, &f, NOW)) else { panic!() };
    (percent, tone)
}

#[test]
fn the_memory_meter_turns_warn_above_85_and_bad_above_95_percent() {
    assert_eq!(memory_percent(85), (85, MeterTone::Neutral));
    assert_eq!(memory_percent(86), (86, MeterTone::Warn));
    assert_eq!(memory_percent(95), (95, MeterTone::Warn));
    assert_eq!(memory_percent(96), (96, MeterTone::Bad));
    assert_eq!(memory_percent(0), (0, MeterTone::Neutral));
}

#[test]
fn the_first_gpu_that_reports_video_memory_is_shown_with_its_own_tone() {
    let gpus = json!([
        {"name": "Integrated", "vramTotal": null, "vramUsed": null, "utilisation": null, "unified": true},
        {"name": "Discrete", "vramTotal": 8 * GB, "vramUsed": 7 * GB, "utilisation": 41.6, "unified": false},
    ]);
    let mut f = Facts::new();
    f.seed(&overview("serving", vec![], memory(16 * GB, 8 * GB, "elevated", gpus)), NOW);
    let PopoverValue::Memory { used, total, percent, pressure, vram, .. } = value_of(state(PopoverWidget::Memory, &f, NOW)) else { panic!() };
    assert_eq!((used, total, percent, pressure), (8 * GB, 16 * GB, 50, MemoryLevel::Elevated));
    let vram = vram.expect("the discrete GPU reports its memory");
    assert_eq!((vram.percent, vram.busy_percent, vram.tone), (88, Some(42), MeterTone::Warn));
}

#[test]
fn no_video_memory_reading_means_no_vram_row() {
    let f = connected("serving", vec![]);
    let PopoverValue::Memory { vram, .. } = value_of(state(PopoverWidget::Memory, &f, NOW)) else { panic!() };
    assert_eq!(vram, None);
}

// ---- downloads ----

#[test]
fn one_running_download_is_working_with_megabytes_speed_and_cancel() {
    let mut f = connected("serving", vec![]);
    f.set_downloads(&[download("d1", "Qwen3 8B", "downloading", 1_200 * MB, 4_800 * MB, 12 * MB)], NOW);
    let WidgetState::Working { title, phase, progress, cancel, .. } = state(PopoverWidget::Downloads, &f, NOW) else { panic!("working") };
    assert_eq!(title, "Downloading Qwen3 8B");
    assert_eq!(phase, "Downloading · 12 MB/s");
    assert_eq!(progress, Progress::Fraction { done: 1_200, total: 4_800, unit: "MB".into() });
    assert_eq!(cancel.map(|c| (c.action, c.label)), Some((WidgetActionId::CancelDownload { id: "d1".into() }, "Cancel download".into())));
}

#[test]
fn two_running_downloads_sum_up_and_have_no_single_cancel() {
    let mut f = connected("serving", vec![]);
    f.set_downloads(
        &[
            download("d1", "A", "downloading", 100 * MB, 400 * MB, 4 * MB),
            download("d2", "B", "verifying", 300 * MB, 600 * MB, 0),
        ],
        NOW,
    );
    let WidgetState::Working { title, phase, progress, cancel, .. } = state(PopoverWidget::Downloads, &f, NOW) else { panic!() };
    assert_eq!((title.as_str(), phase.as_str()), ("Downloading 2 files", "4 MB/s"));
    assert_eq!(progress, Progress::Fraction { done: 400, total: 1_000, unit: "MB".into() });
    assert!(cancel.is_none());
}

#[test]
fn a_download_of_unknown_size_has_an_indeterminate_bar_and_a_slow_one_says_so() {
    let mut f = connected("serving", vec![]);
    f.set_downloads(&[download("d1", "X", "downloading", 5 * MB, 0, 100)], NOW);
    let WidgetState::Working { progress, phase, .. } = state(PopoverWidget::Downloads, &f, NOW) else { panic!() };
    assert_eq!(progress, Progress::Unknown);
    assert_eq!(phase, "Downloading · under 1 MB/s");
}

#[test]
fn the_engines_own_install_download_counts_when_the_feed_has_not_listed_it() {
    let mut f = connected("serving", vec![]);
    f.engine_setup.state = EngineSetupState::Installing;
    f.engine_setup.download = Some(download("e1", "llama.cpp b4000 vulkan", "unpacking", 80 * MB, 80 * MB, 0));
    let WidgetState::Working { title, phase, .. } = state(PopoverWidget::Downloads, &f, NOW) else { panic!() };
    assert_eq!((title.as_str(), phase.as_str()), ("Downloading llama.cpp b4000 vulkan", "Unpacking"));
}

#[test]
fn no_running_download_lists_failed_then_paused_ones_or_is_calm() {
    let mut f = connected("serving", vec![]);
    let (title, _, next, calm) = empty_of(state(PopoverWidget::Downloads, &f, NOW));
    assert_eq!((title.as_str(), calm), ("No downloads running", true));
    assert_eq!(next, Some(WidgetActionId::OpenPage { page: Page::Models }));
    let mut bad = download("d3", "C", "failed", 0, 10, 0);
    bad.error = Some("The disk is full.".into());
    f.set_downloads(
        &[
            download("d1", "A", "paused", MB, 10 * MB, 0),
            download("d2", "B", "paused", MB, 10 * MB, 0),
            bad,
        ],
        NOW,
    );
    let PopoverValue::Downloads { rows, more } = value_of(state(PopoverWidget::Downloads, &f, NOW)) else { panic!() };
    assert_eq!((rows.len(), more), (2, 1));
    assert_eq!((rows[0].name.as_str(), rows[0].state, rows[0].message.as_deref()), ("C", DownloadRowState::Failed, Some("The disk is full.")));
    assert_eq!((rows[1].name.as_str(), rows[1].state), ("A", DownloadRowState::Paused));
}

#[test]
fn a_finished_or_cancelled_download_leaves_the_facts() {
    let mut f = connected("serving", vec![]);
    f.apply(&LiveUpdate::Download { download: download("d1", "A", "downloading", MB, 10 * MB, MB) }, NOW);
    assert!(matches!(state(PopoverWidget::Downloads, &f, NOW), WidgetState::Working { .. }));
    f.apply(&LiveUpdate::Download { download: download("d1", "A", "done", 10 * MB, 10 * MB, 0) }, NOW);
    assert!(matches!(state(PopoverWidget::Downloads, &f, NOW), WidgetState::Empty { calm: true, .. }));
    f.apply(&LiveUpdate::Download { download: download("d2", "B", "downloading", MB, 10 * MB, MB) }, NOW);
    f.apply(&LiveUpdate::Download { download: download("d2", "B", "cancelled", MB, 10 * MB, 0) }, NOW);
    assert!(matches!(state(PopoverWidget::Downloads, &f, NOW), WidgetState::Empty { calm: true, .. }));
}

// ---- updates ----

#[test]
fn model_updates_are_a_count_or_calm_or_not_checked_yet() {
    let mut f = connected("serving", vec![]);
    let (title, detail, _, calm) = empty_of(state(PopoverWidget::Updates, &f, NOW));
    assert_eq!((title.as_str(), detail.as_str(), calm), ("Know when models have updates", "Not checked yet.", false));
    f.set_updates(UpdatesFact { count: 0, summary: None, last_checked_ms: None }, NOW);
    assert!(!empty_of(state(PopoverWidget::Updates, &f, NOW)).3, "a count of nothing without a check is not 'all good'");
    f.set_updates(UpdatesFact { count: 0, summary: None, last_checked_ms: Some(NOW - 3 * HOUR) }, NOW);
    let (title, detail, next, calm) = empty_of(state(PopoverWidget::Updates, &f, NOW));
    assert_eq!((title.as_str(), detail.as_str(), calm), ("Models are up to date", "Checked 3 h ago", true));
    assert_eq!(next, Some(WidgetActionId::OpenPage { page: Page::Models }));
    f.set_updates(UpdatesFact { count: 2, summary: Some("A better quantisation of Qwen3 is available.".into()), last_checked_ms: Some(NOW) }, NOW);
    assert_eq!(
        value_of(state(PopoverWidget::Updates, &f, NOW)),
        PopoverValue::Updates { count: 2, summary: Some("A better quantisation of Qwen3 is available.".into()) }
    );
}

#[test]
fn how_long_ago_is_worded_in_minutes_hours_and_days() {
    assert_eq!(ago(NOW, NOW), "just now");
    assert_eq!(ago(NOW, NOW + 5 * MIN), "just now", "a time of the future reads as just now");
    assert_eq!(ago(NOW, NOW - 59 * MIN), "59 min ago");
    assert_eq!(ago(NOW, NOW - 60 * MIN), "1 h ago");
    assert_eq!(ago(NOW, NOW - 24 * HOUR), "1 d ago");
}

// ---- recent errors ----

#[test]
fn recent_errors_cover_exactly_the_last_day_newest_first_and_cap_at_two_rows() {
    let mut f = connected("serving", vec![]);
    f.seed_events(
        &[
            event(1, "error", NOW - 24 * HOUR - 1, "Too old."),
            event(2, "error", NOW - 24 * HOUR, "Exactly a day."),
            event(3, "error", NOW - 3 * HOUR, "Three hours."),
            event(4, "error", NOW - 10 * MIN, "Ten minutes."),
            event(5, "warn", NOW - MIN, "Only a warning."),
            event(6, "info", NOW, "Only information."),
        ],
        NOW,
    );
    let PopoverValue::RecentErrors { rows, more } = value_of(state(PopoverWidget::RecentErrors, &f, NOW)) else { panic!() };
    assert_eq!(rows.iter().map(|r| r.message.as_str()).collect::<Vec<_>>(), ["Ten minutes.", "Three hours."]);
    assert_eq!(more, 1, "'Exactly a day.' is the one not listed; 'Too old.' is out of the window");
    assert_eq!(rows[0].id, "e4");
}

#[test]
fn no_error_in_the_last_day_is_calm() {
    let mut f = connected("serving", vec![]);
    f.seed_events(&[event(1, "error", NOW - 25 * HOUR, "Old.")], NOW);
    let (title, detail, next, calm) = empty_of(state(PopoverWidget::RecentErrors, &f, NOW));
    assert_eq!((title.as_str(), detail.as_str(), calm), ("No errors", "In the last day", true));
    assert_eq!(next, Some(WidgetActionId::OpenPage { page: Page::Requests }));
}

#[test]
fn a_pushed_error_event_is_added_once_and_the_log_keeps_the_newest_twenty() {
    let mut f = connected("serving", vec![]);
    let e = event(7, "error", NOW - MIN, "Failed.");
    f.apply(&LiveUpdate::Event { event: e.clone() }, NOW);
    f.apply(&LiveUpdate::Event { event: e }, NOW);
    f.apply(&LiveUpdate::Event { event: event(8, "info", NOW, "Fine.") }, NOW);
    assert_eq!(f.errors.len(), 1);
    for i in 100..130u64 {
        f.apply(&LiveUpdate::Event { event: event(i, "error", NOW - 1000 + i as i64, "Again.") }, NOW);
    }
    assert_eq!(f.errors.len(), 20);
    assert_eq!(f.errors[0].id, 129, "newest first");
}

// ---- the server and the activity ----

#[test]
fn the_server_widget_says_serving_paused_stopped_or_unreachable_with_the_port() {
    let st = |serving: &str| value_of(state(PopoverWidget::Server, &connected(serving, vec![]), NOW));
    assert_eq!(st("serving"), PopoverValue::Server { state: ServerState::Serving, port: 11435, engine: Some(EngineHealth::Ready) });
    assert!(matches!(st("paused"), PopoverValue::Server { state: ServerState::Paused, .. }));
    assert!(matches!(st("stopped"), PopoverValue::Server { state: ServerState::Stopped, .. }));
}

#[test]
fn a_lost_feed_keeps_the_last_readings_but_marks_them_stale_from_the_moment_it_failed() {
    let mut f = connected("serving", vec![model("a", "A", GB, 0, "ready", 0, 1)]);
    f.set_disconnected(NOW + 5 * MIN);
    f.set_disconnected(NOW + 9 * MIN);
    match state(PopoverWidget::Server, &f, NOW + 10 * MIN) {
        WidgetState::Ready { value, stale_since, .. } => {
            assert_eq!(stale_since, Some(NOW + 5 * MIN), "the first failure, not the latest retry");
            assert!(matches!(value, PopoverValue::Server { state: ServerState::Unreachable, port: 11435, .. }));
        }
        other => panic!("{other:?}"),
    }
    let WidgetState::Ready { stale_since, .. } = state(PopoverWidget::LoadedModels, &f, NOW + 10 * MIN) else { panic!() };
    assert_eq!(stale_since, Some(NOW + 5 * MIN));
    // Back: fresh again.
    f.apply(&LiveUpdate::Throughput { throughput: Default::default() }, NOW + 11 * MIN);
    let WidgetState::Ready { stale_since, .. } = state(PopoverWidget::LoadedModels, &f, NOW + 11 * MIN) else { panic!() };
    assert_eq!(stale_since, None);
}

#[test]
fn engine_health_puts_the_most_serious_thing_first() {
    let health = |f: &Facts| match value_of(state(PopoverWidget::Server, f, NOW)) {
        PopoverValue::Server { engine, .. } => engine,
        _ => panic!(),
    };
    let mut f = connected("serving", vec![]);
    assert_eq!(health(&f), Some(EngineHealth::Ready));
    f.engines = vec![serde_json::from_value(tile(json!({"state": "notInstalled", "reason": "Download it."}))).unwrap()];
    assert_eq!(health(&f), Some(EngineHealth::Missing));
    f.issues = vec![issue("restarting"), issue("restarting")];
    assert_eq!(health(&f), Some(EngineHealth::Restarting { count: 2 }));
    f.issues.push(issue("failed"));
    assert_eq!(health(&f), Some(EngineHealth::Failed { count: 1 }));
    f.engine_setup.state = EngineSetupState::Installing;
    assert_eq!(health(&f), Some(EngineHealth::Installing));
}

#[test]
fn activity_shows_the_queue_and_rounds_the_speed_and_is_zero_while_paused() {
    let mut f = connected("serving", vec![]);
    f.running = 2;
    f.waiting = 3;
    f.throughput.gen_tps = 38.5;
    let tps = |f: &Facts| match value_of(state(PopoverWidget::Activity, f, NOW)) {
        PopoverValue::Activity { running, waiting, tokens_per_sec } => (running, waiting, tokens_per_sec),
        _ => panic!(),
    };
    assert_eq!(tps(&f), (2, 3, 39));
    f.throughput.gen_tps = 0.4;
    assert_eq!(tps(&f).2, 0);
    f.throughput.gen_tps = 50.0;
    let mut paused = connected("paused", vec![]);
    paused.throughput.gen_tps = 50.0;
    assert_eq!(tps(&paused).2, 0, "a paused server shows no speed");
    f.set_disconnected(NOW);
    assert_eq!(tps(&f).2, 0, "no speed from a lost feed");
}

// ---- the facts take every pushed update ----

#[test]
fn every_pushed_update_lands_in_the_facts() {
    let mut f = Facts::new();
    f.seed(&overview("serving", vec![], calm_memory()), NOW);
    let m: crate::mgmt::LoadedModel = serde_json::from_value(model("a", "A", GB, 0, "ready", 0, 1)).unwrap();
    f.apply(&LiveUpdate::Models { models: vec![m] }, NOW + 1);
    f.apply(&LiveUpdate::Memory { sample: serde_json::from_value(memory(100, 10, "critical", json!([]))).unwrap() }, NOW + 2);
    let queue: crate::mgmt::QueueSnapshot = serde_json::from_value(json!({
        "running": [{"id": "1", "appId": "debrief", "modelId": "a", "priority": "interactive", "kind": "chat", "state": "running", "submittedAt": 0, "startedAt": 0, "progress": null, "tokens": 0}],
        "waiting": [], "depth": {"interactive": 0, "background": 0, "batch": 0}
    }))
    .unwrap();
    f.apply(&LiveUpdate::Queue { queue }, NOW + 3);
    f.apply(&LiveUpdate::Issues { issues: vec![issue("failed")] }, NOW + 4);
    f.apply(&LiveUpdate::EngineSetup { setup: crate::mgmt::EngineSetup { state: EngineSetupState::Failed, error: Some("x".into()), download: None } }, NOW + 5);
    f.apply(&LiveUpdate::Pairing { pairings: vec![] }, NOW + 6);
    f.apply(&LiveUpdate::Server { server: serde_json::from_value(server("paused")).unwrap() }, NOW + 7);
    assert_eq!((f.models.len(), f.running, f.waiting, f.issues.len(), f.pairings), (1, 1, 0, 1, 0));
    assert_eq!(f.engine_setup.state, EngineSetupState::Failed);
    assert!(f.serving_off());
    assert_eq!(f.changed_ms, NOW + 7, "the readings are as of the last change");
    let PopoverValue::Memory { percent, .. } = value_of(state(PopoverWidget::Memory, &f, NOW)) else { panic!() };
    assert_eq!(percent, 90);
    let WidgetState::Ready { as_of, .. } = state(PopoverWidget::Activity, &f, NOW) else { panic!() };
    assert_eq!(as_of, NOW + 7);
}

#[test]
fn a_waiting_or_testing_download_says_what_it_is_doing_and_a_failed_install_means_no_engine() {
    let mut f = connected("serving", vec![]);
    for (state_word, phase) in [("queued", "Waiting in line"), ("selfTest", "Testing the build"), ("verifying", "Checking the file"), ("unpacking", "Unpacking")] {
        f.set_downloads(&[download("d1", "Engine", state_word, 0, 10 * MB, 0)], NOW);
        let WidgetState::Working { phase: p, .. } = state(PopoverWidget::Downloads, &f, NOW) else { panic!() };
        assert_eq!(p, phase, "{state_word}");
    }
    f.engine_setup.state = EngineSetupState::Failed;
    let PopoverValue::Server { engine, .. } = value_of(state(PopoverWidget::Server, &f, NOW)) else { panic!() };
    assert_eq!(engine, Some(EngineHealth::Missing));
}

#[test]
fn a_share_of_nothing_is_zero_and_a_missing_reading_is_said() {
    assert_eq!(percent(5, 0), 0);
    assert_eq!(percent(150, 100), 100, "never above a hundred");
    let mut f = connected("serving", vec![]);
    f.memory = None;
    let (title, detail, next, calm) = empty_of(state(PopoverWidget::Memory, &f, NOW));
    assert_eq!((title.as_str(), detail.as_str(), next, calm), ("See memory in use", "No reading yet.", None, false));
}

// ---- before the first snapshot ----

#[test]
fn before_the_first_snapshot_widgets_say_connecting_and_after_a_failure_not_running() {
    let mut f = Facts::new();
    let (_, detail, next, calm) = empty_of(state(PopoverWidget::Memory, &f, NOW));
    assert_eq!((detail.as_str(), next, calm), ("Connecting to the server.", None, false));
    assert!(matches!(state(PopoverWidget::Server, &f, NOW), WidgetState::Empty { .. }));
    assert_eq!(popover_header(&f).status, PopoverStatus::Unknown);
    assert!(notice(&f).is_none());

    for kind in [PopoverWidget::Activity, PopoverWidget::LoadedModels, PopoverWidget::Downloads, PopoverWidget::Updates, PopoverWidget::RecentErrors] {
        let (title, detail, _, calm) = empty_of(state(kind, &f, NOW));
        assert!(!title.is_empty() && detail == "Connecting to the server." && !calm, "{kind:?}");
    }
    assert_eq!(empty_of(state(PopoverWidget::Server, &f, NOW)).0, "See whether the server is serving");

    f.set_disconnected(NOW);
    let (_, detail, next, _) = empty_of(state(PopoverWidget::Memory, &f, NOW));
    assert_eq!(detail, "The server is not running.");
    assert_eq!(next, Some(WidgetActionId::OpenPage { page: Page::Dashboard }));
    assert!(matches!(
        value_of(state(PopoverWidget::Server, &f, NOW)),
        PopoverValue::Server { state: ServerState::Unreachable, port: 0, engine: None }
    ));
    assert_eq!(popover_header(&f).status, PopoverStatus::Unreachable);
}

// ---- the header and the notice ----

#[test]
fn the_header_status_and_toggles_follow_the_server() {
    let h = |f: &Facts| popover_header(f);
    let serving = connected("serving", vec![model("a", "A", GB, 0, "ready", 0, 1)]);
    assert_eq!(h(&serving).status, PopoverStatus::Serving { port: 11435 });
    assert_eq!((h(&serving).serving, h(&serving).can_release_memory), (ServingToggle::Pause, true));

    let paused = connected("paused", vec![]);
    assert_eq!(h(&paused).status, PopoverStatus::Paused);
    assert_eq!((h(&paused).serving, h(&paused).can_release_memory), (ServingToggle::Resume, false));

    let stopped = connected("stopped", vec![]);
    assert_eq!(h(&stopped).status, PopoverStatus::Stopped);
    assert_eq!(h(&stopped).serving, ServingToggle::Resume);

    let mut lost = serving.clone();
    lost.set_disconnected(NOW);
    assert_eq!(h(&lost).status, PopoverStatus::Unreachable);
    assert_eq!((h(&lost).serving, h(&lost).can_release_memory), (ServingToggle::Off, false));
}

#[test]
fn engine_problems_outrank_paused_in_the_header() {
    let mut f = connected("paused", vec![]);
    f.issues = vec![issue("failed"), issue("restarting")];
    assert_eq!(popover_header(&f).status, PopoverStatus::Problems { count: 2 });
}

#[test]
fn the_notice_is_a_lost_server_first_then_apps_waiting_to_pair() {
    let mut f = connected("serving", vec![]);
    assert!(notice(&f).is_none());
    f.pairings = 1;
    let n = notice(&f).unwrap();
    assert_eq!((n.tone, n.title.as_str()), (Tone::Info, "An app wants to pair"));
    f.pairings = 3;
    assert_eq!(notice(&f).unwrap().title, "3 apps want to pair");
    assert_eq!(notice(&f).unwrap().action.unwrap().label, "Review");
    f.set_disconnected(NOW);
    let n = notice(&f).unwrap();
    assert_eq!((n.tone, n.title.as_str()), (Tone::Danger, "The server is not responding"));
    let mut never = Facts::new();
    never.set_disconnected(NOW);
    assert_eq!(notice(&never).unwrap().title, "The server is not running");
}

// ---- the whole surface ----

#[test]
fn values_come_in_layout_order_one_per_placement() {
    let f = connected("serving", vec![]);
    let stored = Stored::<PopoverCatalogue>::default();
    let values = popover_values(stored.layout(), &f, NOW);
    let ids: Vec<&str> = values.placements.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["server", "activity", "memory", "loaded-models"]);
    let mut edited = stored.clone();
    edited
        .edit(Edit::Remove {
            id: stored.layout().placements[0].id.clone(),
        })
        .unwrap();
    assert_eq!(popover_values(edited.layout(), &f, NOW).placements.len(), 3);
}

// ---- fixtures for the UI's mock ----

/// The named scenarios of the UI's mock (`demo`, `empty`, `worst`, `down`, `paused`): the facts that make them.
fn scenario(name: &str) -> Facts {
    match name {
        "empty" => {
            let mut f = connected("serving", vec![]);
            f.set_updates(UpdatesFact { count: 0, summary: None, last_checked_ms: Some(NOW - 2 * HOUR) }, NOW);
            f
        }
        "demo" => {
            let mut f = Facts::new();
            let mem = memory(32 * GB, 13 * GB, "normal", json!([{"name": "Discrete", "vramTotal": 12 * GB, "vramUsed": 7 * GB, "utilisation": 38.0, "unified": false}]));
            f.seed(
                &overview(
                    "serving",
                    vec![
                        model("qwen3-8b", "Qwen3 8B Q4_K_M", 3 * GB, 2 * GB, "busy", 1, NOW - MIN),
                        model("gemma3-4b", "Gemma 3 4B Q5_K_M", 3 * GB, 0, "ready", 0, NOW - 12 * MIN),
                    ],
                    mem,
                ),
                NOW,
            );
            f.running = 1;
            f.waiting = 0;
            f.throughput.gen_tps = 38.4;
            f.set_downloads(&[download("d1", "Llama 3.2 3B Q4_K_M", "downloading", 900 * MB, 2_000 * MB, 12 * MB)], NOW);
            f.set_updates(UpdatesFact { count: 2, summary: Some("A better quantisation of Qwen3 8B is available.".into()), last_checked_ms: Some(NOW - 3 * HOUR) }, NOW);
            f.seed_events(&[event(1, "error", NOW - 12 * MIN, "A chat request to Qwen3 8B failed: the context was full.")], NOW);
            f
        }
        "worst" => {
            // The worst the default popup meets: a notice, a paused server whose engine keeps failing, memory nearly full with a
            // full GPU, the most models and errors the rows show, a download running.
            let mut f = Facts::new();
            let mem = memory(16 * GB, GB / 2, "critical", json!([{"name": "Discrete", "vramTotal": 8 * GB, "vramUsed": 8 * GB - 100 * MB, "utilisation": 97.0, "unified": false}]));
            f.seed(
                &overview(
                    "paused",
                    vec![
                        model("a", "Qwen3 Coder 30B A3B Instruct Q4_K_M", 9 * GB, 4 * GB, "busy", 2, NOW - MIN),
                        model("b", "Gemma 3 27B Instruct QAT", 6 * GB, 3 * GB, "ready", 0, NOW - 5 * MIN),
                        model("c", "Llama 3.2 3B", 2 * GB, 0, "releasingContext", 0, NOW - 9 * MIN),
                        model("d", "Phi 4 Mini", 2 * GB, 0, "ready", 0, NOW - 20 * MIN),
                        model("e", "Mistral Small 24B", 8 * GB, 0, "ready", 0, NOW - 40 * MIN),
                    ],
                    mem,
                ),
                NOW,
            );
            f.running = 3;
            f.waiting = 12;
            f.pairings = 2;
            f.issues = vec![issue("failed")];
            f.set_downloads(&[download("d1", "Qwen3 Coder 30B A3B Instruct Q4_K_M", "downloading", 9_000 * MB, 18_000 * MB, 25 * MB)], NOW);
            f.set_updates(UpdatesFact { count: 12, summary: Some("A newer revision of Qwen3 Coder 30B A3B Instruct is available.".into()), last_checked_ms: Some(NOW - MIN) }, NOW);
            f.seed_events(
                &[
                    event(1, "error", NOW - MIN, "The engine for Qwen3 Coder 30B crashed three times and was given up on."),
                    event(2, "error", NOW - 12 * MIN, "A chat request from Debrief failed: the model did not fit in memory."),
                    event(3, "error", NOW - 50 * MIN, "A download from huggingface.co failed: the disk is full."),
                ],
                NOW,
            );
            f
        }
        "connecting" => Facts::new(),
        "paused" => {
            let mut f = scenario("demo");
            f.server.as_mut().unwrap().serving = crate::mgmt::ServingState::Paused;
            f
        }
        "down" => {
            let mut f = scenario("demo");
            f.set_disconnected(NOW);
            f
        }
        other => panic!("no scenario {other}"),
    }
}

const MB: u64 = 1024 * 1024;

/// The UI's mock serves these files' contents instead of re-deriving the engine's rules in TypeScript: for every scenario the
/// state of every widget kind (as if all were placed), the header and the surface notice, besides the catalogue, the default
/// layout and the loading shape. `PADDOCK_WRITE_TS=1 cargo test -p pitwall-core --lib widgets::build::tests::the_mock_fixtures`
/// rewrites it.
#[test]
fn the_mock_fixtures_are_the_engines_output() {
    use paddock_commands::testing::{self, WriteMode};
    let mut all = Stored::<PopoverCatalogue>::default();
    for widget in [PopoverWidget::Downloads, PopoverWidget::Updates, PopoverWidget::RecentErrors] {
        all.edit(Edit::Add { widget, span: None, at: None }).unwrap();
    }
    let mut scenarios = serde_json::Map::new();
    for name in ["demo", "empty", "worst", "down", "paused", "connecting"] {
        let f = scenario(name);
        let values = popover_values(all.layout(), &f, NOW);
        let by_kind: serde_json::Map<String, Value> = all
            .layout()
            .placements
            .iter()
            .zip(&values.placements)
            .map(|(p, st)| (serde_json::to_value(p.widget).unwrap().as_str().unwrap().to_string(), serde_json::to_value(&st.state).unwrap()))
            .collect();
        scenarios.insert(
            name.into(),
            json!({ "byKind": by_kind, "notice": values.notice, "header": popover_header(&f) }),
        );
    }
    let defaults = Stored::<PopoverCatalogue>::default();
    let view: crate::widgets::PopoverView = paddock_widgets::view(&defaults);
    let text = serde_json::to_string_pretty(&json!({ "nowMs": NOW, "view": view, "shape": crate::widgets::loading_shape(), "scenarios": scenarios })).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/mock/popover-fixtures.json");
    testing::sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}
