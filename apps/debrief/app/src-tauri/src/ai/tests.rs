//! The AI command layer through the real IPC path (`tauri::test`), with the fakes of `debrief-ai`: a scripted
//! model, a fake engine runtime and a fake model host. Set-up, models, use cases, chat streaming,
//! background tasks, the settings section and saved chats.

use super::commands::{check_link, markdown_target, sample_of};
use super::core::{AiCore, AiDeps};
use super::AiState;
use crate::test_support::{samples, test_app_with_commands, window, TempDir};
use crate::tools::tests::ipc;
use debrief_ai::llm::{LlmProvider, Reply, ScriptedProvider};
use debrief_ai::AiError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::test::MockRuntime;
use tauri::{Listener, Manager};

struct Fixture {
    tmp: TempDir,
    safe_mode: bool,
}

impl Fixture {
    fn new(tag: &str) -> Fixture {
        Fixture {
            tmp: TempDir::new(&format!("ai-{tag}")),
            safe_mode: false,
        }
    }

    fn core(&self) -> Arc<AiCore> {
        Arc::new(AiCore::new(Some(self.tmp.0.join("ai")), self.safe_mode, AiDeps { pitwall: None }))
    }

    fn app(&self, core: Arc<AiCore>) -> tauri::App<MockRuntime> {
        let app = test_app_with_commands(Some(self.tmp.0.join("config/profiles")));
        super::manage(app.handle(), core);
        app
    }
}

fn scripted(replies: Vec<Reply>) -> Arc<dyn LlmProvider> {
    Arc::new(ScriptedProvider::new(replies))
}

fn text(s: &str) -> Reply {
    Reply::Text(s.to_string())
}

fn open_samples(w: &tauri::WebviewWindow<MockRuntime>) {
    let dir = samples().join("serilog-clef");
    ipc(w, "open_paths", json!({ "paths": [dir.to_string_lossy()] })).unwrap();
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        // Generous: a loaded runner stalls a test for many seconds; the bound only turns a hang into a message.
        assert!(start.elapsed() < Duration::from_secs(180), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn code(e: Value) -> String {
    e["code"].as_str().unwrap_or_default().to_string()
}

/// Waits for the chat's turn to end and returns its events.
fn finish_turn(w: &tauri::WebviewWindow<MockRuntime>, sid: &str) -> Vec<Value> {
    wait_until("the turn to end", || {
        ipc(w, "ai_chat_sessions", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["running"] == false)
    });
    ipc(w, "ai_chat_events_since", json!({"sessionId": sid, "seq": 0}))
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

fn kinds(events: &[Value]) -> Vec<String> {
    events.iter().map(|e| e["type"].as_str().unwrap().to_string()).collect()
}

#[test]
fn use_cases_route_through_ipc_to_the_model_and_the_engine() {
    let f = Fixture::new("usecases");
    let core = f.core();
    core.set_fixed_provider(scripted(vec![
        text(r#"{"ql": "level >= error", "explanation": "All errors and worse."}"#),
        text("The most likely root cause: not found in the logs."),
    ]));
    let app = f.app(core);
    let main = window(&app, "main");
    open_samples(&main);
    let status = ipc(&main, "ai_status", json!({})).unwrap();
    assert_eq!(
        (status["available"].as_bool(), status["state"].as_str()),
        (Some(true), Some("ready"))
    );
    // A query proposal: the job starts at once; a dialog that waits gets the whole answer.
    let id = start(&main, json!({"op": "query", "question": "show me the errors"}));
    let outcome = ipc(&main, "jobs_wait", json!({"id": id})).unwrap();
    assert_eq!(outcome["state"], "done");
    let q = outcome["result"]["proposal"].clone();
    assert_eq!(q["ql"], "level >= error");
    assert!(q["estimatedMatches"].as_u64().unwrap() > 0);
    // Root causes runs in the background: the command returns the job at once; the answer is its result.
    let id = start(&main, json!({"op": "rootCauses"}));
    let job = ended(&app, id).expect("an answer is kept: it needs the user's attention");
    assert_eq!(job.state, crate::jobs::JobState::Done);
    let n = ipc(&main, "jobs_result", json!({"id": id})).unwrap();
    assert_eq!(n["type"], "narrative");
    assert!(n["narrative"]["text"].as_str().unwrap().contains("not found in the logs"));
    let notices = ipc(&main, "notifications_list", json!({})).unwrap();
    let ready = notices["notices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["title"] == "Root causes ready")
        .expect("a message")
        .clone();
    assert_eq!(ready["primary"], json!({"type": "viewJob", "jobId": id}));
    // Chat turns and unknown requests do not start this way.
    assert_eq!(
        code(ipc(&main, "ai_start", json!({"request": {"op": "chat", "sessionId": "c", "text": "x"}})).unwrap_err()),
        "invalid"
    );
    assert_eq!(
        code(ipc(&main, "ai_start", json!({"request": {"op": "nope"}})).unwrap_err()),
        "invalid"
    );
    // Arguments are validated before anything runs.
    let invalid = |r: Value| code(ipc(&main, "ai_start", json!({ "request": r })).unwrap_err());
    assert_eq!(invalid(json!({"op": "query", "question": "  "})), "invalid");
    assert_eq!(invalid(json!({"op": "regex", "lines": []})), "invalid");
    assert_eq!(
        invalid(json!({"op": "pipeline", "goal": "drop health checks"})),
        "invalid",
        "no sample and no event"
    );
    // The scripted model has no more replies: the job fails with the engine's reason, and offers Retry.
    let id = start(&main, json!({"op": "summarize", "from": 0, "to": i64::MAX}));
    let failed = ended(&app, id).expect("failures are kept");
    assert_eq!(failed.state, crate::jobs::JobState::Failed);
    assert!(failed.retryable);
    assert_eq!(ipc(&main, "ai_cancel", json!({})).unwrap(), 0);
}

#[test]
fn proposal_samples_come_from_the_ui_or_the_selected_event() {
    let mut ws = debrief_core::Workspace::new(debrief_core::Profile::default_profile()).unwrap();
    ws.open(&[samples().join("serilog-text")]).unwrap();
    let ws = debrief_core::workspace::WorkspaceStore::new(ws);
    let direct: &dyn debrief_core::ports::Store = &ws;
    let store: debrief_ai::StoreRef = direct.into();
    let (s, f) = sample_of(store, None, Some("a\nb".into()), Some("x.log".into())).unwrap();
    assert_eq!((s.as_str(), f.as_str()), ("a\nb", "x.log"));
    assert_eq!(sample_of(store, None, Some("a".into()), None).unwrap().1, "sample.log");
    assert!(sample_of(store, None, Some("a".repeat(70_000)), None).is_err());
    let (s, f) = sample_of(store, Some(0), None, None).unwrap();
    assert!(!s.is_empty() && !f.is_empty(), "{f}");
    assert_eq!(
        sample_of(store, Some(u32::MAX), None, None).unwrap_err().code,
        super::error::AiErrorCode::NotFound
    );
    assert_eq!(
        sample_of(store, None, Some(" ".into()), None).unwrap_err().code,
        super::error::AiErrorCode::Invalid
    );
}

#[test]
fn applying_unchanged_settings_keeps_the_keys_of_a_1_1_file() {
    let f = Fixture::new("keeps-old-keys");
    let dir = f.tmp.0.join("ai");
    std::fs::create_dir_all(&dir).unwrap();
    let old = r#"{"version":1,"context_length":8192,"max_tokens":1024,"temperature":0.2,"models_dir":"/chosen/models","llama_server_path":"/chosen/llama-server"}"#;
    std::fs::write(dir.join("ai.json"), old).unwrap();
    let core = f.core();
    // What Settings does at start: it applies the section's values, here the very same ones.
    core.apply_settings(core.settings()).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("ai.json")).unwrap(),
        old,
        "AI-4 still finds the chosen folders"
    );
    // A real change writes the file (and drops the keys of the removed engine).
    let mut next = core.settings();
    next.max_tokens = 512;
    core.apply_settings(next).unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("ai.json")).unwrap()).unwrap();
    assert_eq!(saved["max_tokens"], 512);
    assert!(saved.get("models_dir").is_none());
}

#[test]
fn safe_mode_and_a_missing_pitwall_are_typed_errors() {
    let f = Fixture::new("unavailable");
    // Safe mode: AI is off, every command says so.
    let safe = Fixture {
        safe_mode: true,
        ..Fixture::new("safe")
    };
    let app = safe.app(safe.core());
    let main = window(&app, "main");
    let status = ipc(&main, "ai_status", json!({})).unwrap();
    assert_eq!(
        (status["available"].clone(), status["state"].clone()),
        (json!(false), json!("unavailable"))
    );
    assert_eq!(
        code(ipc(&main, "ai_start", json!({"request": {"op": "rootCauses"}})).unwrap_err()),
        "unavailable"
    );
    // No Pitwall at all (tests pass none): the assistant has no model.
    let app = f.app(f.core());
    let main = window(&app, "main");
    let status = ipc(&main, "ai_status", json!({})).unwrap();
    assert_eq!(
        (status["available"].clone(), status["state"].clone()),
        (json!(true), json!("noModel"))
    );
    assert_eq!(status["pitwall"]["found"], "absent");
    open_samples(&main);
    assert_eq!(
        code(ipc(&main, "ai_start", json!({"request": {"op": "rootCauses"}})).unwrap_err()),
        "noModel"
    );
}

#[test]
fn chat_streams_to_its_window_and_history_round_trips_through_the_project() {
    let f = Fixture::new("chat");
    let core = f.core();
    core.set_fixed_provider(scripted(vec![
        Reply::Tools(String::new(), vec![("run_query".into(), json!({"ql": "level = fatal"}))]),
        text("The service crashed; see `level = fatal`."),
    ]));
    let app = f.app(core);
    let main = window(&app, "main");
    open_samples(&main);
    let chat = ipc(&main, "ai_chat_new", json!({})).unwrap();
    let sid = chat["id"].as_str().unwrap().to_string();
    ipc(
        &main,
        "ai_chat_send",
        json!({"sessionId": sid, "text": "What crashed?", "context": {"openView": "grid", "selectedIds": [999999]}}),
    )
    .unwrap();
    let ev = finish_turn(&main, &sid);
    let k = kinds(&ev);
    assert_eq!(&k[..2], ["toolStarted", "toolFinished"], "{k:?}");
    assert_eq!(k.last().unwrap(), "done");
    let done = ev.last().unwrap();
    assert!(done["citations"].as_array().unwrap().iter().any(|c| c["kind"] == "query"));
    assert!(done["notes"][0].as_str().unwrap().contains("do not exist"));
    assert!(ev.iter().all(|e| e["sessionId"].is_null()), "replayed envelopes are the library's");
    let since = ipc(&main, "ai_chat_events_since", json!({"sessionId": sid, "seq": 2})).unwrap();
    assert_eq!(since.as_array().unwrap()[0]["seq"], 3);
    let sessions = ipc(&main, "ai_chat_sessions", json!({})).unwrap();
    assert_eq!(
        (sessions[0]["title"].as_str(), sessions[0]["turns"].as_u64()),
        (Some("What crashed?"), Some(1))
    );
    let rec = ipc(&main, "ai_chat_get", json!({"sessionId": sid})).unwrap();
    assert!(rec["turns"][0]["assistant"].as_str().unwrap().contains("crashed"));
    assert_eq!(
        code(ipc(&main, "ai_chat_get", json!({"sessionId": "nope"})).unwrap_err()),
        "notFound"
    );
    assert_eq!(code(ipc(&main, "ai_chat_get", json!({"sessionId": ""})).unwrap_err()), "invalid");
    assert_eq!(
        code(ipc(&main, "ai_chat_send", json!({"sessionId": "nope", "text": "x"})).unwrap_err()),
        "notFound"
    );
    // The model has no more replies: the turn ends with an error event, and the chat is free again.
    ipc(&main, "ai_chat_send", json!({"sessionId": sid, "text": "And then?"})).unwrap();
    assert_eq!(kinds(&finish_turn(&main, &sid)).last().unwrap(), "error");

    // Export to Markdown, with the overwrite guard.
    let md = f.tmp.0.join("chat.md");
    let path = md.to_string_lossy().to_string();
    // Only to a file the user chose in the save dialog.
    assert_eq!(
        code(ipc(&main, "ai_chat_export_markdown", json!({"sessionId": sid, "path": path})).unwrap_err()),
        "notGranted"
    );
    assert!(!md.exists());
    crate::paths::grant(
        app.handle(),
        "main",
        &md,
        crate::paths::Access::Write,
        crate::paths::GrantSource::Dialog,
    )
    .unwrap();
    ipc(&main, "ai_chat_export_markdown", json!({"sessionId": sid, "path": path})).unwrap();
    assert!(std::fs::read_to_string(&md).unwrap().contains("**You:** What crashed?"));
    assert_eq!(
        code(ipc(&main, "ai_chat_export_markdown", json!({"sessionId": sid, "path": path})).unwrap_err()),
        "fileExists"
    );
    ipc(
        &main,
        "ai_chat_export_markdown",
        json!({"sessionId": sid, "path": path, "overwrite": true}),
    )
    .unwrap();

    // Saved in the project's UI state under `chats`, reopened in another window.
    let history = ipc(&main, "ai_chat_history", json!({})).unwrap();
    assert_eq!(history["version"], 1);
    let project = f.tmp.0.join("p.debriefproj");
    let file = debrief_core::project::ProjectFile {
        version: debrief_core::project::PROJECT_VERSION,
        name: "p".into(),
        sources: vec![],
        removed: vec![],
        profile: String::new(),
        ui: json!({ "version": 1, "chats": history }),
        clock: Default::default(),
        store: None,
        reports: vec![],
    };
    file.save(&project).unwrap();
    let loaded = debrief_core::project::ProjectFile::load(&project).unwrap();
    let other = window(&app, "w-2");
    let r = ipc(&other, "ai_chat_restore", json!({"record": loaded.ui["chats"]})).unwrap();
    assert!(r["warning"].is_null());
    assert_eq!(ipc(&other, "ai_chat_history", json!({})).unwrap(), history);
    // Old projects (no chats), damaged and newer records.
    assert_eq!(ipc(&other, "ai_chat_restore", json!({})).unwrap()["sessions"], json!([]));
    assert!(
        ipc(&other, "ai_chat_restore", json!({"record": {"version": "x"}})).unwrap()["warning"]
            .as_str()
            .unwrap()
            .contains("damaged")
    );
    assert!(
        ipc(&other, "ai_chat_restore", json!({"record": {"version": 9}})).unwrap()["warning"]
            .as_str()
            .unwrap()
            .contains("newer")
    );
    assert_eq!(ipc(&main, "ai_chat_delete", json!({"sessionId": sid})).unwrap(), true);
    assert!(ipc(&main, "ai_suggested_prompts", json!({})).unwrap().as_array().unwrap().len() >= 3);
}

#[test]
fn the_ai_settings_section_is_registered_and_applies() {
    let f = Fixture::new("settings");
    let core = f.core();
    let app = f.app(core.clone());
    let main = window(&app, "main");
    let overview = ipc(&main, "settings_sections", json!({})).unwrap();
    let ai = overview["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "ai")
        .unwrap()
        .clone();
    assert_eq!((ai["group"].as_str(), ai["status"].as_str()), (Some("ai"), Some("active")));
    let keys: Vec<&str> = ai["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        ["context_length", "max_tokens", "temperature"],
        "the model and its runtime are Pitwall's"
    );
    let r = ipc(
        &main,
        "set_settings",
        json!({"section": "ai", "values": {"context_length": 4096, "max_tokens": 512}}),
    )
    .unwrap();
    assert_eq!(r["applied"], true, "{r}");
    assert_eq!((core.settings().context_length, core.settings().max_tokens), (4096, 512));
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(f.tmp.0.join("ai/ai.json")).unwrap()).unwrap();
    assert_eq!(
        (saved["context_length"].as_u64(), saved["max_tokens"].as_u64()),
        (Some(4096), Some(512))
    );
    assert!(app.try_state::<AiState>().is_some());
}

#[test]
fn chat_exports_go_only_to_markdown_files_in_existing_folders() {
    let tmp = TempDir::new("ai-md");
    let ok = tmp.0.join("a.md");
    assert!(markdown_target(&ok.to_string_lossy(), false).is_ok());
    std::fs::write(&ok, "x").unwrap();
    assert_eq!(
        markdown_target(&ok.to_string_lossy(), false).unwrap_err().code,
        super::error::AiErrorCode::FileExists
    );
    assert!(markdown_target(&ok.to_string_lossy(), true).is_ok());
    for bad in ["", "rel.md", "/x/\0.md"] {
        assert_eq!(
            markdown_target(bad, false).unwrap_err().code,
            super::error::AiErrorCode::Invalid,
            "{bad}"
        );
    }
    assert!(markdown_target(&tmp.0.join("a.txt").to_string_lossy(), false).is_err());
    assert!(markdown_target(&tmp.0.join("nope/a.md").to_string_lossy(), false).is_err());
    let dir = tmp.0.join("d.md");
    std::fs::create_dir(&dir).unwrap();
    assert!(markdown_target(&dir.to_string_lossy(), true).is_err());
}

#[test]
fn only_public_https_links_from_answers_are_opened() {
    for ok in ["https://learn.example.com/a?b=c#d", "https://example.com:8443/x"] {
        assert!(check_link(ok).is_ok(), "{ok}");
    }
    for bad in [
        "http://example.com",
        "file:///etc/passwd",
        "https://localhost/x",
        "https://127.0.0.1/",
        "https://[::1]/",
        "https://10.0.0.5/",
        "https://user@example.com/",
        "https:///x",
        "https://exa mple.com",
        "javascript:alert(1)",
    ] {
        assert!(check_link(bad).is_err(), "{bad}");
    }
    assert!(check_link(&format!("https://example.com/{}", "a".repeat(3000))).is_err());
    // A public link that no answer showed is refused before anything is opened.
    let f = Fixture::new("links");
    let app = f.app(f.core());
    let main = window(&app, "main");
    let e = ipc(&main, "ai_open_link", json!({"url": "https://example.com/"})).unwrap_err();
    assert_eq!(code(e), "refused");
    assert!(!main
        .state::<crate::windows::Windows>()
        .get("main")
        .unwrap()
        .ai
        .chats
        .knows_url("https://example.com/"));
}

/// A model that, from its second answer on, keeps "thinking" until the test opens the gate (at most 20 s), to
/// show the workspace is free while it generates.
struct SlowModel {
    inner: ScriptedProvider,
    calls: std::sync::atomic::AtomicUsize,
    gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
}

impl LlmProvider for SlowModel {
    fn id(&self) -> &str {
        "slow"
    }
    fn display_name(&self) -> String {
        "Slow scripted model".into()
    }
    fn context_length(&self) -> usize {
        self.inner.context_length()
    }
    fn chat(
        &self,
        req: &debrief_ai::llm::ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> debrief_ai::Result<debrief_ai::llm::ChatResponse> {
        if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
            let (open, cv) = &*self.gate;
            let guard = open.lock().unwrap();
            let _unused = cv.wait_timeout_while(guard, Duration::from_secs(180), |o| !*o).unwrap();
        }
        self.inner.chat(req, on_delta)
    }
}

#[test]
fn imports_do_not_wait_for_a_slow_turn_and_changed_citations_are_flagged() {
    let f = Fixture::new("slow");
    let core = f.core();
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    core.set_fixed_provider(Arc::new(SlowModel {
        inner: ScriptedProvider::new(vec![
            Reply::Tools(String::new(), vec![("get_events".into(), json!({"ids": [10]}))]),
            text("The failure is event 10."),
        ]),
        calls: Default::default(),
        gate: gate.clone(),
    }));
    let app = f.app(core);
    let main = window(&app, "main");
    open_samples(&main);
    let sid = ipc(&main, "ai_chat_new", json!({})).unwrap()["id"].as_str().unwrap().to_string();
    ipc(&main, "ai_chat_send", json!({"sessionId": sid, "text": "What failed?"})).unwrap();
    // The model read event 10 and is now writing its answer (until the gate opens).
    wait_until("the tool result", || {
        ipc(&main, "ai_chat_events_since", json!({"sessionId": sid, "seq": 0}))
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "toolFinished")
    });
    // An import into the same window (a write, like a live tail append) completes while the turn still runs,
    // and its files interleave with the open ones, so the ids are re-sorted.
    let start = Instant::now();
    let more = samples().join("serilog-text");
    ipc(&main, "open_paths", json!({ "paths": [more.to_string_lossy()] })).unwrap();
    let import_took = start.elapsed();
    let running = ipc(&main, "ai_chat_sessions", json!({})).unwrap()[0]["running"] == true;
    assert!(running, "the import finished in {import_took:?}, before the answer");
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    let ev = finish_turn(&main, &sid);
    let done = ev.iter().find(|e| e["type"] == "done").unwrap();
    let cited = done["citations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "record")
        .unwrap();
    // Event ids are stable in the store (they are never renumbered when files interleave), so the citation
    // still names the same event and stays verified.
    assert_eq!(
        (cited["id"].as_u64(), cited["verified"].as_bool()),
        (Some(10), Some(true)),
        "{done}"
    );
}

/// What the gated model does with its next call.
enum Step {
    Answer,
    /// The worker died mid-answer.
    Crash,
}

/// A model that answers (from a script) only when the test says so: requests visibly wait in the AI queue.
struct GatedProvider {
    inner: ScriptedProvider,
    steps: Mutex<std::sync::mpsc::Receiver<Step>>,
    /// How many requests reached the model (and wait for their step).
    entered: Arc<std::sync::atomic::AtomicUsize>,
}

impl LlmProvider for GatedProvider {
    fn id(&self) -> &str {
        "gated"
    }
    fn display_name(&self) -> String {
        "Gated test model".into()
    }
    fn context_length(&self) -> usize {
        self.inner.context_length()
    }
    fn chat(
        &self,
        req: &debrief_ai::llm::ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> debrief_ai::Result<debrief_ai::llm::ChatResponse> {
        self.entered.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match self.steps.lock().unwrap().recv() {
            Ok(Step::Answer) => self.inner.chat(req, on_delta),
            _ => Err(AiError::Engine("the AI worker crashed (exit code 3)".into())),
        }
    }
}

fn gated(replies: Vec<Reply>) -> (Arc<dyn LlmProvider>, std::sync::mpsc::Sender<Step>) {
    let (p, tx, _) = gated_counted(replies);
    (p, tx)
}

/// Like [`gated`], with the count of requests that reached the model.
fn gated_counted(
    replies: Vec<Reply>,
) -> (
    Arc<dyn LlmProvider>,
    std::sync::mpsc::Sender<Step>,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    let (tx, rx) = std::sync::mpsc::channel();
    let entered = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let p = GatedProvider {
        inner: ScriptedProvider::new(replies),
        steps: Mutex::new(rx),
        entered: entered.clone(),
    };
    (Arc::new(p), tx, entered)
}

/// Starts a background AI answer (explain, summarise, root causes) and returns its job.
fn start(main: &tauri::WebviewWindow<MockRuntime>, request: Value) -> u64 {
    ipc(main, "ai_start", json!({ "request": request })).unwrap().as_u64().unwrap()
}

/// Waits until job `id` ended; its info (none when it ended unseen and was not kept).
fn ended(app: &tauri::App<MockRuntime>, id: u64) -> Option<crate::jobs::queue::JobInfo> {
    let jobs = jobs_of(app);
    let id = crate::jobs::JobId(id);
    wait_until("the job to end", || jobs.state_of(id).is_none_or(|s| s.is_finished()));
    jobs.info(id)
}

fn jobs_of(app: &tauri::App<MockRuntime>) -> crate::jobs::Jobs {
    app.state::<crate::jobs::Jobs>().inner().clone()
}

fn active_ai(app: &tauri::App<MockRuntime>) -> Vec<crate::jobs::JobId> {
    jobs_of(app).find_active(|j| j.kind == crate::jobs::JobKind::Ai)
}

#[test]
fn ai_requests_are_jobs_run_one_at_a_time_and_a_crash_fails_only_the_running_one() {
    let f = Fixture::new("ai-jobs");
    let core = f.core();
    let (model, steps) = gated(vec![text("Chat answer: nothing unusual.")]);
    core.set_fixed_provider(model);
    let app = f.app(core);
    let main = window(&app, "main");
    open_samples(&main);
    // An explain request runs (waiting for the model), a chat turn waits behind it.
    let explain = start(&main, json!({"op": "explain", "recordId": 1}));
    wait_until("the explain job to run", || {
        let ids = active_ai(&app);
        ids.len() == 1 && jobs_of(&app).state_of(ids[0]) == Some(crate::jobs::JobState::Running)
    });
    let sid = ipc(&main, "ai_chat_new", json!({})).unwrap()["id"].as_str().unwrap().to_string();
    let chat_job = ipc(&main, "ai_chat_send", json!({"sessionId": sid, "text": "anything unusual?"}))
        .unwrap()
        .as_u64()
        .unwrap();
    assert_eq!(
        code(ipc(&main, "ai_chat_send", json!({"sessionId": sid, "text": "again"})).unwrap_err()),
        "busy",
        "one turn per chat, also while it waits"
    );
    // Shown in the pane after 2 s: the turn waits at position 1, and may be moved.
    app.state::<crate::test_support::TestClock>().0.advance(3_000);
    let list = ipc(&main, "jobs_list", json!({})).unwrap();
    // The window's name-table rebuild (a `JobKind::Names` job, shown like any job after 2 s) is not what this test is about: only AI jobs are.
    let jobs: Vec<&Value> = list["jobs"].as_array().unwrap().iter().filter(|j| j["kind"] == "ai").collect();
    assert_eq!(jobs.len(), 2);
    assert_eq!(
        (jobs[0]["state"].as_str(), jobs[0]["title"].as_str()),
        (Some("running"), Some("Explain event 1"))
    );
    assert_eq!(jobs[1]["id"], chat_job);
    assert_eq!(
        (
            jobs[1]["state"].as_str(),
            jobs[1]["position"].as_u64(),
            jobs[1]["movable"].as_bool()
        ),
        (Some("queued"), Some(1), Some(true))
    );
    assert_eq!(jobs[1]["source"]["link"], json!({"type": "chat", "sessionId": sid}));
    assert_eq!(jobs[0]["source"]["link"], json!({"type": "event", "recordId": 1}));
    // The worker crashes during the explain: only it fails, with the reason; the turn runs next.
    steps.send(Step::Crash).unwrap();
    let crashed = ended(&app, explain).expect("the failure is kept");
    assert_eq!(crashed.state, crate::jobs::JobState::Failed);
    assert!(crashed.error.as_deref().unwrap_or_default().contains("crashed"));
    steps.send(Step::Answer).unwrap();
    let events = finish_turn(&main, &sid);
    assert!(kinds(&events).contains(&"done".to_string()), "{events:?}");
    wait_until("both jobs to end", || active_ai(&app).is_empty());
    let list = ipc(&main, "jobs_list", json!({})).unwrap();
    let failed = list["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["state"] == "failed")
        .expect("the crashed request is listed")
        .clone();
    assert!(failed["error"].as_str().unwrap().contains("crashed"));
    assert_eq!(
        (failed["retryable"].as_bool(), failed["unseen"].as_bool()),
        (Some(true), Some(true))
    );
    // Its failure is a message in the notification centre, with Retry.
    let notices = ipc(&main, "notifications_list", json!({})).unwrap();
    let n = notices["notices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["source"] == "job")
        .expect("a job message")
        .clone();
    assert_eq!(n["primary"], json!({"type": "retryJob", "jobId": failed["id"]}));
    // The finished turn's View opens its chat.
    assert_eq!(
        ipc(&main, "jobs_result", json!({"id": chat_job})).unwrap(),
        json!({"type": "chat", "sessionId": sid})
    );
    // A failed request runs again with Retry, as a job of its own linked to the failed one.
    steps.send(Step::Crash).unwrap();
    let retried = ipc(&main, "jobs_retry", json!({"id": failed["id"]})).unwrap().as_u64().unwrap();
    assert_eq!(
        jobs_of(&app).info(crate::jobs::JobId(retried)).and_then(|i| i.retry_of),
        Some(crate::jobs::JobId(failed["id"].as_u64().unwrap()))
    );
    wait_until("the retry to end", || active_ai(&app).is_empty());
}

#[test]
fn a_waiting_request_can_be_cancelled_and_a_queued_turn_frees_its_chat() {
    let f = Fixture::new("ai-cancel-queued");
    let core = f.core();
    let (model, steps) = gated(vec![text("Explained.")]);
    core.set_fixed_provider(model);
    let app = f.app(core);
    let main = window(&app, "main");
    open_samples(&main);
    let first = start(&main, json!({"op": "explain", "recordId": 1}));
    wait_until("running", || active_ai(&app).len() == 1);
    let second = start(&main, json!({"op": "rootCauses"}));
    // The second request is queued before the chat adds its turn (on a busy machine its thread can start late).
    wait_until("the second request queued", || active_ai(&app).len() == 2);
    let sid = ipc(&main, "ai_chat_new", json!({})).unwrap()["id"].as_str().unwrap().to_string();
    ipc(&main, "ai_chat_send", json!({"sessionId": sid, "text": "hello"})).unwrap();
    wait_until("three AI jobs", || active_ai(&app).len() == 3);
    // Stop in the chat drops its waiting turn: the chat is free again and told.
    assert_eq!(ipc(&main, "ai_chat_cancel", json!({"sessionId": sid})).unwrap(), true);
    wait_until("the chat to be free", || {
        ipc(&main, "ai_chat_sessions", json!({})).unwrap()[0]["running"] == false
    });
    assert_eq!(active_ai(&app).len(), 2);
    // The dialog's Cancel (`ai_cancel`) stops the running use case and drops the waiting one.
    assert_eq!(ipc(&main, "ai_cancel", json!({})).unwrap(), 2);
    steps.send(Step::Answer).unwrap();
    for id in [first, second] {
        let state = ended(&app, id).map(|j| j.state);
        assert!(state.is_none_or(|s| s == crate::jobs::JobState::Cancelled), "{state:?}");
    }
    wait_until("no AI jobs", || active_ai(&app).is_empty());
}

#[test]
fn clicking_summarize_twice_gives_one_job() {
    let f = Fixture::new("summarize-twice");
    let core = f.core();
    let (model, steps, entered) = gated_counted(vec![text("A summary.")]);
    core.set_fixed_provider(model);
    let app = f.app(core);
    let main = window(&app, "main");
    open_samples(&main);
    let range = json!({"op": "summarize", "from": 0, "to": 1000});
    let first = start(&main, range.clone());
    // The same ask from the menu or the palette while it runs (and the button again) joins the running job.
    wait_until("the model to be asked", || entered.load(Ordering::SeqCst) == 1);
    assert_eq!(start(&main, range.clone()), first, "a second click while it runs");
    assert_eq!(active_ai(&app).len(), 1);
    // A different range is a different job, queued behind it.
    let other = start(&main, json!({"op": "summarize", "from": 0, "to": 2000}));
    assert_ne!(other, first);
    assert_eq!(active_ai(&app).len(), 2);
    steps.send(Step::Answer).unwrap();
    assert_eq!(ended(&app, first).map(|j| j.state), Some(crate::jobs::JobState::Done));
    // A finished job frees the key: asking again summarises again.
    wait_until("the other job to run", || entered.load(Ordering::SeqCst) == 2);
    assert_eq!(ipc(&main, "ai_cancel", json!({})).unwrap(), 1);
    steps.send(Step::Answer).unwrap();
    wait_until("no AI jobs", || active_ai(&app).is_empty());
    assert_ne!(start(&main, range), first);
}

#[test]
fn research_with_workspace_data_in_the_search_asks_to_confirm_first() {
    let f = Fixture::new("research-confirm");
    let core = f.core();
    core.set_fixed_provider(scripted(vec![]));
    let app = f.app(core);
    let main = window(&app, "main");
    open_samples(&main);
    // The samples' MachineName is WEB01: a name the logs mark as private.
    let r = ipc(
        &main,
        "ai_research",
        json!({"args": {"question": "why does WEB01 time out on checkout?"}}),
    )
    .unwrap();
    assert_eq!(r["status"], "confirm", "{r}");
    let segments = r["checked"]["segments"].as_array().unwrap();
    assert!(segments.iter().any(|s| s["text"] == "WEB01" && !s["leak"].is_null()), "{r}");
    assert!(
        jobs_of(&app).find_active(|j| j.kind == crate::jobs::JobKind::Research).is_empty(),
        "nothing searched yet"
    );
    // Bad arguments are refused before anything runs.
    assert_eq!(code(ipc(&main, "ai_research", json!({"args": {}})).unwrap_err()), "invalid");
    assert_eq!(
        code(ipc(&main, "ai_research", json!({"args": {"question": "  "}})).unwrap_err()),
        "invalid"
    );
    assert_eq!(
        code(ipc(&main, "ai_research", json!({"args": {"question": "q", "sessionId": "nope"}})).unwrap_err()),
        "notFound"
    );
    assert_eq!(
        code(ipc(&main, "ai_research", json!({"args": {"recordId": u32::MAX}})).unwrap_err()),
        "notFound"
    );
}

#[test]
fn the_model_drafts_the_search_text_and_the_leak_guard_still_checks_it() {
    let f = Fixture::new("research-draft");
    let core = f.core();
    core.set_fixed_provider(scripted(vec![
        // "Research this" on an event: the model kept a host name despite the instructions.
        text("\"System.TimeoutException The operation has timed out WEB01\""),
        // "Search the web for this error": the flagged part is taken out.
        text("TimeoutException WEB01 operation timed out"),
        // A chatty reply is not a query: the engine's text is used.
        text("Sure! Here is a query:\nTimeoutException"),
        // A question without an event.
        text("Query: E_ACCESSDENIED IIS app pool"),
    ]));
    let app = f.app(core);
    let main = window(&app, "main");
    open_samples(&main);
    let page = ipc(
        &main,
        "query_page",
        json!({"request": {"query": {"text": "timed out"}, "offset": 0, "limit": 1, "descending": false}}),
    )
    .unwrap();
    let id = page["rows"][0]["id"].as_u64().unwrap();
    let d = ipc(&main, "ai_research_draft", json!({"args": {"recordId": id}})).unwrap();
    assert_eq!(d["source"], "model", "{d}");
    assert_eq!(d["checked"]["query"], "System.TimeoutException The operation has timed out WEB01");
    assert_eq!(d["checked"]["needsConfirm"], true, "WEB01 is a name from the logs: {d}");
    assert!(d["question"].as_str().unwrap().starts_with("What does this error mean"), "{d}");
    assert!(
        jobs_of(&app).find_active(|j| j.kind == crate::jobs::JobKind::Research).is_empty(),
        "drafting never searches"
    );
    let q = ipc(&main, "browser_event_query", json!({"recordId": id})).unwrap();
    assert_eq!(
        (q["source"].as_str(), q["query"].as_str()),
        (Some("model"), Some("TimeoutException operation timed out")),
        "{q}"
    );
    assert_eq!(q["removed"], json!(["1 value from your logs"]));
    let q = ipc(&main, "browser_event_query", json!({"recordId": id})).unwrap();
    assert_eq!(q["source"], "engine", "{q}");
    assert!(!q["query"].as_str().unwrap().contains("WEB0"), "{q}");
    let d = ipc(
        &main,
        "ai_research_draft",
        json!({"args": {"question": "what does E_ACCESSDENIED mean for IIS?"}}),
    )
    .unwrap();
    assert_eq!(
        (d["source"].as_str(), d["checked"]["query"].as_str()),
        (Some("model"), Some("E_ACCESSDENIED IIS app pool"))
    );
    // The model has no more replies: the question is the search text, as before drafting.
    let d = ipc(
        &main,
        "ai_research_draft",
        json!({"args": {"question": "what is HRESULT 0x80070005?"}}),
    )
    .unwrap();
    assert_eq!(
        (d["source"].as_str(), d["checked"]["query"].as_str()),
        (Some("engine"), Some("what is HRESULT 0x80070005?"))
    );
    assert_eq!(code(ipc(&main, "ai_research_draft", json!({"args": {}})).unwrap_err()), "invalid");
    assert_eq!(
        code(ipc(&main, "ai_research_draft", json!({"args": {"recordId": u32::MAX}})).unwrap_err()),
        "notFound"
    );
    let long = "x".repeat(2000);
    assert_eq!(
        code(ipc(&main, "ai_research_draft", json!({"args": {"question": long}})).unwrap_err()),
        "invalid"
    );
}

mod knowledge;
mod pitwall;
mod vision;

#[test]
fn finding_values_builds_the_names_and_never_asks_the_model() {
    let f = Fixture::new("names");
    let core = f.core();
    let provider = Arc::new(ScriptedProvider::new(vec![text(r#"{"items":[]}"#); 40]));
    core.set_fixed_provider(provider.clone());
    let app = f.app(core.clone());
    let main = window(&app, "main");
    open_samples(&main);
    let win = app.state::<crate::windows::Windows>().get("main").unwrap();
    wait_until("the name table", || {
        ipc(&main, "find_values", json!({"request": {"text": "gateway"}})).unwrap();
        win.names.names().is_built()
    });
    // Rule 46: no background job over many names, so nothing can ask the model.
    let reply = ipc(&main, "find_values", json!({"request": {"text": "gateway"}})).unwrap();
    assert!(reply["values"].is_array(), "{reply}");
    assert!(provider.requests().is_empty(), "no name is sent to the model unless the user asks");
}

/// A rebuild that is running: a job under the window's rebuild key that ends when the returned sender is dropped.
fn hold_rebuild(app: &tauri::App<MockRuntime>, label: &str) -> std::sync::mpsc::Sender<()> {
    let jobs = app.state::<crate::jobs::Jobs>().inner().clone();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    jobs.submit_work(crate::names::rebuild_spec(label), crate::jobs::Controls::default(), move |_| {
        let _ = rx.recv();
        crate::jobs::queue::JobEnd::Done(None)
    })
    .unwrap();
    tx
}

#[test]
fn a_search_answers_from_the_last_table_at_once_and_queues_at_most_one_rebuild() {
    let f = Fixture::new("names-once");
    let app = f.app(f.core());
    let main = window(&app, "main");
    let win = app.state::<crate::windows::Windows>().get("main").unwrap();
    // A rebuild is running (its job key is held): no request, and no import, may rebuild or queue another.
    let held = hold_rebuild(&app, "main");
    open_samples(&main);
    for _ in 0..20 {
        let reply = ipc(&main, "find_values", json!({"request": {"text": "gateway"}})).unwrap();
        assert!(reply["values"].is_array(), "{reply}");
    }
    assert!(!win.names.names().is_built(), "no request rebuilt the table inline");
    assert_eq!(win.names.rebuild_runs(), 0, "every request joined the one that is running");
    // The rebuild ends: the next search queues exactly one job, which builds the table.
    drop(held);
    wait_until("the name table", || {
        ipc(&main, "find_values", json!({"request": {"text": "gateway"}})).unwrap();
        win.names.names().is_built()
    });
    let jobs = app.state::<crate::jobs::Jobs>().inner().clone();
    wait_until("the rebuild job to end", || {
        jobs.find_active(|j| j.kind == crate::jobs::JobKind::Names).is_empty()
    });
    // The table is current: more searches queue nothing.
    assert_eq!(win.names.rebuild_runs(), 1, "exactly one rebuild job");
    for _ in 0..10 {
        ipc(&main, "find_values", json!({"request": {"text": "gateway"}})).unwrap();
    }
    assert_eq!(win.names.rebuild_runs(), 1);
    // Cancel stops a rebuild and leaves the table as it was.
    let ctx = crate::jobs::JobCtx::detached();
    ctx.cancel_flag().store(true, Ordering::SeqCst);
    let (id, store) = win.engine.tagged_store().unwrap();
    assert_eq!(
        win.names.run_rebuild(id, store.as_ref(), &ctx),
        crate::names::RebuildOutcome::Cancelled
    );
    assert!(win.names.names().is_built());
}

#[test]
fn queue_if_stale_queues_a_rebuild_only_for_a_table_that_is_out_of_date_and_says_so() {
    let f = Fixture::new("names-stale");
    let app = f.app(f.core());
    let main = window(&app, "main");
    let win = app.state::<crate::windows::Windows>().get("main").unwrap();
    let jobs = app.state::<crate::jobs::Jobs>().inner().clone();
    // A rebuild is running (its job key is held): the table does not exist, so it is stale and one more is queued.
    let held = hold_rebuild(&app, "main");
    open_samples(&main);
    assert!(
        crate::names::queue_if_stale(app.handle(), &win),
        "no table yet: stale, a rebuild was queued"
    );
    drop(held);
    wait_until("the name table", || win.names.names().is_built());
    wait_until("the rebuild jobs to end", || {
        jobs.find_active(|j| j.kind == crate::jobs::JobKind::Names).is_empty()
    });
    // The table is current: nothing is queued.
    let runs = win.names.rebuild_runs();
    assert!(!crate::names::queue_if_stale(app.handle(), &win), "a current table queues nothing");
    assert!(jobs.find_active(|j| j.kind == crate::jobs::JobKind::Names).is_empty());
    assert_eq!(win.names.rebuild_runs(), runs);
}

/// ADR 0024 section 3: each reader of the name table has a test with a stale table. The chat turn is one: it uses the
/// last table, rebuilds nothing itself (0 rebuilds inline, 0 queued while one runs) and queues exactly one rebuild job.
#[test]
fn a_chat_turn_reads_the_last_name_table_and_starts_no_rebuild_of_its_own() {
    let f = Fixture::new("names-chat");
    let core = f.core();
    core.set_fixed_provider(scripted(vec![text("Nothing unusual."), text("Still nothing.")]));
    let app = f.app(core);
    let main = window(&app, "main");
    let win = app.state::<crate::windows::Windows>().get("main").unwrap();
    // A rebuild is running (its job key is held) while the workspace opens and the chat asks.
    let held = hold_rebuild(&app, "main");
    open_samples(&main);
    let sid = ipc(&main, "ai_chat_new", json!({})).unwrap()["id"].as_str().unwrap().to_string();
    ipc(&main, "ai_chat_send", json!({"sessionId": sid, "text": "Anything unusual?"})).unwrap();
    finish_turn(&main, &sid);
    assert!(!win.names.names().is_built(), "the turn did not rebuild the table inline");
    assert_eq!(win.names.rebuild_runs(), 0, "and queued nothing while a rebuild was running");
    // The rebuild ends: the next turn answers from the (empty) last table and queues exactly one job.
    drop(held);
    ipc(&main, "ai_chat_send", json!({"sessionId": sid, "text": "And now?"})).unwrap();
    finish_turn(&main, &sid);
    wait_until("the name table", || win.names.names().is_built());
    assert_eq!(win.names.rebuild_runs(), 1, "exactly one rebuild job");
}

#[test]
fn the_name_commands_validate_what_the_webview_sends() {
    let f = Fixture::new("names-cmd");
    let app = f.app(f.core());
    let main = window(&app, "main");
    for (cmd, args) in [
        ("names_accept", json!({"request": {"text": "gfx", "group": "executable|nothing"}})),
        ("names_reject", json!({"request": {"text": "  ", "group": "x"}})),
        ("names_remove_alias", json!({"request": {"group": "", "alias": "a"}})),
    ] {
        assert_eq!(code(ipc(&main, cmd, args).unwrap_err()), "invalid", "{cmd}");
    }
    assert_eq!(ipc(&main, "names_undo", json!({})).unwrap()["undone"], Value::Null);
}
