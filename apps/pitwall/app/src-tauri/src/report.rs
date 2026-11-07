//! Pitwall's part of Report a problem (`paddock-diag` sections, plan B4): what a person needs to explain a Pitwall problem from
//! the report alone, read from a list of named files and the server's own answers, never from a walk of the data folder.
//!
//! - `pitwall/settings.toml`: the server's settings (redacted like every file).
//! - `pitwall/state/<file>`: the small state files that hold no user content (the discovery file, the engine pins, the
//!   benchmark results).
//! - `pitwall/inventory.json`: which engines and model files are installed, by name and size (no file is read).
//! - `pitwall/server.json`: what the server answers to `overview`, or why it did not answer.
//! - `pitwall/service.json`: how the background service is registered and whether it answers.
//!
//! The logs of all three processes (`pitwall.<day>.log`, `pitwall-server.<day>.log`, `pitwall-cli.<day>.log`) are the shell's
//! built-in Logs section: they share the `pitwall` prefix in the app's log folder, and each process gets its share of the window.
//!
//! Left out on purpose: chats and their attachments, presets (they hold system prompts), the app list and keys (`apps.json`,
//! `admin.key`), the response cache, the slot saves, partial downloads and the model files themselves. Keys that reach a log
//! or a settings file anyway (`pw_…`, `pmk_…`) are replaced by the shared redactor's prefixed-token rule.

use paddock_diag::{Content, Context, Section, SectionFile, SectionOutput};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The largest file a section reads.
pub const MAX_FILE: u64 = 512 * 1024;
/// At most this many engines or model files are listed.
pub const MAX_LISTED: usize = 200;
/// How long the server and the service state may take to answer for a report.
pub const ASK_TIMEOUT: Duration = Duration::from_secs(5);

/// The state files a report holds, by name in the data folder: none holds a prompt, a chat or a key.
pub const STATE_FILES: &[&str] = &[pitwall_core::discovery::FILE_NAME, "engine-pins.json", "benchmarks.json"];

/// What the live sections ask: the server and the service. The real one is the app's client; a test names its answers.
pub trait Ask: Send + Sync {
    /// The server's `overview`, or why it did not answer.
    fn overview(&self) -> Result<Value, String>;
    /// The background service's state as the Settings page shows it, or why it could not be read.
    fn service(&self) -> Result<Value, String>;
}

/// The sections of the installed app over its data folder; `app_logs` is the app's own log folder (the built-in Logs section
/// reads it), so a moved data folder's logs are added when they are elsewhere.
pub fn sections<R: tauri::Runtime>(app: &tauri::AppHandle<R>, data: PathBuf, app_logs: &Path) -> Vec<Arc<dyn Section>> {
    let mut out = pitwall_sections(data, Arc::new(Live(app.clone())));
    let home = std::env::var("PITWALL_HOME").ok();
    if let Some(dir) = moved_logs(home.as_deref(), app_logs) {
        out.push(Arc::new(MovedLogs { dir }));
    }
    out
}

/// Where the server and the command line write their logs when the data folder was moved (`PITWALL_HOME`, their own rule:
/// `<PITWALL_HOME>/logs`), when that is not the app's own log folder (in a release build the app's folders ignore the
/// variable, so the two differ).
pub fn moved_logs(pitwall_home: Option<&str>, app_logs: &Path) -> Option<PathBuf> {
    let dir = PathBuf::from(pitwall_home.filter(|h| !h.is_empty())?).join("logs");
    (dir != app_logs).then_some(dir)
}

/// The logs of a moved data folder: `logs/moved/<file>`, the newest part of each `pitwall*` process log, as the built-in Logs
/// section reads the app's own folder.
struct MovedLogs {
    dir: PathBuf,
}

impl Section for MovedLogs {
    fn id(&self) -> &str {
        "pitwall-moved-logs"
    }

    fn collect(&self, _cx: &Context<'_>) -> SectionOutput {
        let found = paddock_diag::window::log_window_report(&self.dir, "pitwall", paddock_diag::window::LOG_WINDOW);
        let mut out = SectionOutput::default();
        if let Some(why) = &found.folder_error {
            out.notes.push(format!("The moved data folder's logs cannot be read ({why})."));
        }
        for (name, why) in &found.skipped {
            out.notes.push(format!(
                "The log {name} in the moved data folder cannot be read ({why}) and was left out."
            ));
        }
        for (name, bytes, cut) in found.files {
            let part = if cut { "the recent part of" } else { "all of" };
            out.files.push(SectionFile {
                name: format!("logs/moved/{name}"),
                kind: paddock_diag::FileKind::Log,
                summary: format!("{part} {name}, from the moved data folder (PITWALL_HOME); file contents are never logged"),
                content: Content::Log(bytes),
            });
        }
        out
    }
}

/// The sections over `data` and `ask` (tests pass a temporary folder and scripted answers).
pub fn pitwall_sections(data: PathBuf, ask: Arc<dyn Ask>) -> Vec<Arc<dyn Section>> {
    vec![
        Arc::new(Files { data: data.clone() }),
        Arc::new(Inventory { data }),
        Arc::new(Answers { ask }),
    ]
}

/// The settings file and the state files.
struct Files {
    data: PathBuf,
}

impl Section for Files {
    fn id(&self) -> &str {
        "pitwall-files"
    }

    fn collect(&self, _cx: &Context<'_>) -> SectionOutput {
        let mut out = SectionOutput::default();
        match read_bounded(&self.data.join("settings.toml")) {
            Ok(Some(text)) => out.files.push(SectionFile::app(
                "pitwall/settings.toml",
                "The server's settings; passwords, tokens and keys are removed",
                Content::Text(text),
            )),
            Ok(None) => out
                .notes
                .push("The server has no settings file yet (it runs on the defaults).".into()),
            Err(why) => out.notes.push(format!("The server's settings file {why}.")),
        }
        for name in STATE_FILES {
            match read_bounded(&self.data.join(name)) {
                Ok(Some(text)) => out.files.push(SectionFile::app(
                    &format!("pitwall/state/{name}"),
                    &format!("Pitwall's {name}: ports, engine choices or benchmark numbers, no chats"),
                    Content::Text(text),
                )),
                Ok(None) => {}
                Err(why) => out.notes.push(format!("{name} {why}.")),
            }
        }
        out
    }
}

/// Which engines and model files are installed: names and sizes, nothing read.
struct Inventory {
    data: PathBuf,
}

impl Section for Inventory {
    fn id(&self) -> &str {
        "pitwall-inventory"
    }

    fn collect(&self, _cx: &Context<'_>) -> SectionOutput {
        let engines = listing(&self.data.join("engines"));
        let models = model_files(&self.data.join("models"));
        SectionOutput::one(SectionFile::app(
            "pitwall/inventory.json",
            "The engines and model files installed, by path and size; the names are as downloaded or as you named the files (their contents are not in the report)",
            // As text: the JSON redactor turns a large nested value into one string, which a person cannot read.
            Content::Text(serde_json::to_string_pretty(&json!({ "engines": engines, "models": models })).unwrap_or_default()),
        ))
    }
}

/// The server's and the service's own answers.
struct Answers {
    ask: Arc<dyn Ask>,
}

impl Section for Answers {
    fn id(&self) -> &str {
        "pitwall-live"
    }

    fn collect(&self, _cx: &Context<'_>) -> SectionOutput {
        let mut out = SectionOutput::default();
        for (name, summary, answer) in [
            (
                "pitwall/server.json",
                "What the Pitwall server says about itself now",
                self.ask.overview(),
            ),
            (
                "pitwall/service.json",
                "How the background service is registered and whether it answers",
                self.ask.service(),
            ),
        ] {
            let value = match (name, answer) {
                ("pitwall/server.json", Ok(v)) => server_view(&v),
                (_, Ok(v)) => long_numbers_as_text(v),
                (_, Err(why)) => json!({ "answered": false, "why": why }),
            };
            // As text, like the inventory: the JSON field rules would cut a long nested value into one string and mask a
            // `*Tokens` count. What is in it is chosen by `server_view`, not by the redactor.
            out.files.push(SectionFile::app(
                name,
                summary,
                Content::Text(serde_json::to_string_pretty(&value).unwrap_or_default()),
            ));
        }
        out
    }
}

/// A file's text when it exists and is at most [`MAX_FILE`]; `Ok(None)` when it is not there; otherwise why not, in words.
fn read_bounded(path: &Path) -> Result<Option<String>, String> {
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot be read ({:?})", e.kind())),
        Ok(m) if !m.is_file() => Err("is not a file".into()),
        Ok(m) if m.len() > MAX_FILE => Err(format!("is larger than {} KB and was left out", MAX_FILE / 1024)),
        Ok(_) => std::fs::read(path)
            .map(|b| Some(String::from_utf8_lossy(&b).into_owned()))
            .map_err(|e| format!("cannot be read ({:?})", e.kind())),
    }
}

/// The most directory entries one listing visits: a folder with millions of entries costs a bounded walk.
pub const MAX_VISITED: usize = 5000;
/// How deep the model folder is walked: the server's own scan depth (`library.rs` `MAX_SCAN_DEPTH`).
pub const MODEL_DEPTH: usize = 4;

/// What `overview` says, reduced to what explains a problem and holds nothing of apps.json (review P1-1): the server, memory,
/// engines, the hot models (id, state, sizes, context, slots), issues, the engine set-up, the queue's size and throughput; the
/// apps and pending pairings only as counts (their names, keys, permissions, usage and the live pairing code stay out).
pub fn server_view(overview: &Value) -> Value {
    let count = |v: &Value| v.as_array().map_or(0, Vec::len);
    let pick = |v: &Value, keys: &[&str]| -> Value {
        Value::Object(
            keys.iter()
                .filter_map(|k| v.get(*k).map(|x| ((*k).to_string(), x.clone())))
                .collect(),
        )
    };
    let apps = overview["apps"].as_array().map(Vec::as_slice).unwrap_or_default();
    long_numbers_as_text(json!({
        "server": pick(&overview["server"], &["version", "apiVersion", "pid", "port", "startedAt", "serving", "lan"]),
        "memory": overview["memory"].clone(),
        "engines": overview["engines"].as_array().map(|e| e.iter().map(|x| pick(x, &["kind", "name", "availability", "activeBuild", "processes", "unhealthy"])).collect::<Vec<_>>()),
        "models": overview["models"].as_array().map(|m| m.iter().map(|x| pick(x, &["modelId", "engine", "build", "state", "ramBytes", "vramBytes", "contextTokens", "slots", "pinned"])).collect::<Vec<_>>()),
        "queue": { "running": count(&overview["queue"]["running"]), "waiting": count(&overview["queue"]["waiting"]) },
        "throughput": overview["throughput"].clone(),
        "preset": overview["preset"].clone(),
        "issues": overview["issues"].as_array().map(|i| i.iter().map(|x| pick(x, &["modelId", "state", "message", "attempts", "since"])).collect::<Vec<_>>()),
        "engineSetup": overview["engineSetup"].clone(),
        "apps": { "total": apps.len(), "connected": apps.iter().filter(|a| a["connected"] == true).count() },
        "pairings": { "pending": count(&overview["pairings"]) },
    }))
}

/// Numbers of 13 digits or more (a start time in ms, a terabyte count) as strings: the text redactor replaces a digit run that
/// could be a card number, which inside a string keeps the file valid JSON and outside one would not.
fn long_numbers_as_text(v: Value) -> Value {
    match v {
        Value::Number(n) if n.as_u64().is_some_and(|x| x >= 1_000_000_000_000) || n.as_i64().is_some_and(|x| x <= -1_000_000_000_000) => {
            Value::String(n.to_string())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(long_numbers_as_text).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, x)| (k, long_numbers_as_text(x))).collect()),
        other => other,
    }
}

/// The model files below `dir`: every `*.gguf` (projector files marked) by its path relative to `dir` and its size, down to
/// [`MODEL_DEPTH`] folders, sorted, at most [`MAX_LISTED`]; no link is followed, nothing is opened, and the walk stops after
/// [`MAX_VISITED`] entries.
fn model_files(dir: &Path) -> Value {
    if !dir.is_dir() {
        return json!({ "listed": false });
    }
    let mut found: Vec<(String, u64)> = Vec::new();
    let mut visited = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = vec![(dir.to_path_buf(), 0)];
    'walk: while let Some((folder, depth)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&folder) else { continue };
        for entry in read.flatten() {
            visited += 1;
            if visited > MAX_VISITED {
                break 'walk;
            }
            // `file_type` of a directory entry does not follow a link: a link is neither a folder nor a file here.
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() && depth < MODEL_DEPTH {
                stack.push((path, depth + 1));
            } else if kind.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gguf")) {
                let rel = path
                    .strip_prefix(dir)
                    .map(|r| {
                        r.components()
                            .map(|c| c.as_os_str().to_string_lossy())
                            .collect::<Vec<_>>()
                            .join("/")
                    })
                    .unwrap_or_default();
                found.push((rel, entry.metadata().map_or(0, |m| m.len())));
            }
        }
    }
    found.sort();
    let count = found.len();
    json!({
        "listed": true,
        "count": count,
        "complete": visited <= MAX_VISITED,
        "entries": found
            .into_iter()
            .take(MAX_LISTED)
            .map(|(path, bytes)| {
                let mmproj = path.rsplit('/').next().is_some_and(|n| n.to_ascii_lowercase().contains("mmproj"));
                json!({ "path": path, "bytes": bytes, "mmproj": mmproj })
            })
            .collect::<Vec<_>>(),
    })
}

/// The entries of `dir` (one level, sorted, at most [`MAX_LISTED`]) with their sizes, from at most [`MAX_VISITED`] entries
/// read; nothing is opened.
fn listing(dir: &Path) -> Value {
    let Ok(read) = std::fs::read_dir(dir) else {
        return json!({ "listed": false });
    };
    let mut entries: Vec<(String, Option<u64>, bool)> = read
        .flatten()
        .take(MAX_VISITED)
        .map(|e| {
            let meta = e.metadata().ok();
            let is_dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
            (
                e.file_name().to_string_lossy().into_owned(),
                meta.filter(|m| m.is_file()).map(|m| m.len()),
                is_dir,
            )
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let total = entries.len();
    entries.truncate(MAX_LISTED);
    json!({
        "listed": true,
        "count": total,
        "entries": entries
            .into_iter()
            .map(|(name, bytes, is_dir)| json!({ "name": name, "bytes": bytes, "folder": is_dir }))
            .collect::<Vec<_>>(),
    })
}

/// The app's own client and service state, looked up when a report is made.
struct Live<R: tauri::Runtime>(tauri::AppHandle<R>);

impl<R: tauri::Runtime> Live<R> {
    fn client(&self) -> Result<crate::commands::SharedClient, String> {
        use tauri::Manager;
        self.0
            .try_state::<crate::commands::SharedClient>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| "the app's server client is not set up".to_string())
    }
}

impl<R: tauri::Runtime> Ask for Live<R> {
    fn overview(&self) -> Result<Value, String> {
        static BUSY: AtomicBool = AtomicBool::new(false);
        let client = self.client()?;
        bounded(&BUSY, ASK_TIMEOUT, move || {
            // Bounded here too, so the thread itself ends at the bound (not only the report's wait for it).
            tauri::async_runtime::block_on(tokio::time::timeout(
                ASK_TIMEOUT,
                crate::commands::forward(client.as_ref(), "overview", Value::Null),
            ))
            .map_err(|_| "the server did not answer in time".to_string())?
            .map_err(|e| e.message)
        })
    }

    fn service(&self) -> Result<Value, String> {
        static BUSY: AtomicBool = AtomicBool::new(false);
        let client = self.client()?;
        bounded(&BUSY, ASK_TIMEOUT, move || {
            let state = tauri::async_runtime::block_on(tokio::time::timeout(ASK_TIMEOUT, crate::app_commands::compose_state(&client)))
                .map_err(|_| "the service state was not read in time".to_string())?;
            serde_json::to_value(state).map_err(|e| format!("the service state could not be written: {e}"))
        })
    }
}

/// Runs `f` on a thread of its own and waits at most `timeout` ([`ASK_TIMEOUT`]: the report is never held by a server that
/// hangs).
/// One question at a time per `busy` flag: while an earlier one still runs (it outlived its bound), another is not started, so
/// threads cannot pile up behind a server that hangs.
fn bounded(
    busy: &'static AtomicBool,
    timeout: Duration,
    f: impl FnOnce() -> Result<Value, String> + Send + 'static,
) -> Result<Value, String> {
    if busy.swap(true, Ordering::SeqCst) {
        return Err("the last question is still waiting for its answer".into());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("pitwall-report-ask".into()).spawn(move || {
        let answer = f();
        // Free the next question before handing the answer over.
        busy.store(false, Ordering::SeqCst);
        let _ = tx.send(answer);
    });
    if let Err(e) = spawned {
        busy.store(false, Ordering::SeqCst);
        return Err(format!("the question could not be asked: {e}"));
    }
    rx.recv_timeout(timeout)
        .unwrap_or_else(|_| Err(format!("no answer within {} ms", timeout.as_millis())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_diag::{AppInfo, Collected, Redactor};
    use paddock_env::{AppDirs, AppId, MapVars, Os};

    /// Text no report may hold: it stands for a chat, an attachment, a prompt or a key file.
    const MARKER: &str = "PLANTED-PRIVATE-MARKER-7f3a";
    /// A Pitwall app key and an MCP key, as they would leak into a log line or a settings file.
    const APP_KEY: &str = "pw_Zk9Qx2LmZk9Qx2LmZk9Qx2LmZk9Qx2LmZk9Qx2LmZk9";
    const MCP_KEY: &str = "pmk_Q2xpZW50S2V5Rm9yVGVzdHM0MjQyNDI0MjQyNDI";

    struct Scripted;

    /// The live pairing code a pairing request shows (six digits): never in a report.
    const PAIRING_CODE: &str = "482913";

    /// A hot model as `overview` lists it.
    fn hot(id: &str) -> Value {
        json!({ "modelId": id, "name": id, "engine": "llama", "build": "b5000", "state": "ready", "ramBytes": 5_100_000_000u64,
                "vramBytes": 4_800_000_000u64, "contextTokens": 8192, "slots": 4, "pinned": false })
    }

    /// What `overview` answers on a used Pitwall: the server, memory, engines, three hot models, the queue, and the apps and a
    /// pairing request whose names hold the marker (apps.json's content, which a report must not hold).
    fn overview_fixture() -> Value {
        let long_name = "x".repeat(1500);
        json!({
            "server": { "version": "1.1.0", "apiVersion": 3, "pid": 4242, "port": 11500, "startedAt": 1_762_590_000_000u64, "serving": "serving",
                        "lan": { "enabled": false, "url": null, "certFingerprint": null } },
            "memory": { "ramTotal": 34_359_738_368u64, "ramUsed": 20_000_000_000u64, "vramTotal": 25_769_803_776u64, "vramUsed": 14_000_000_000u64 },
            "engines": [{ "kind": "llama", "name": "llama.cpp", "availability": "ready", "activeBuild": "b5000", "processes": 3, "unhealthy": 0 }],
            "models": [hot("qwen3-8b"), hot("gemma-3-12b"), hot(&format!("long-{long_name}"))],
            "queue": { "running": [{ "app": format!("{MARKER}-app"), "model": "qwen3-8b" }], "waiting": [], "depth": { "interactive": 1, "background": 0 } },
            "throughput": { "promptTps": 120.5, "genTps": 31.2, "requestsPerMin": 4.0, "activeRequests": 1 },
            "apps": [
                { "id": "debrief", "name": format!("Debrief {MARKER}"), "suite": true, "connected": true, "keys": [{ "id": "k1", "prefix": "pw_ab12" }] },
                { "id": "notes", "name": format!("My notes {MARKER}"), "suite": false, "connected": false, "keys": [] },
            ],
            "preset": "balanced",
            "pairings": [{ "id": "p1", "appId": "editor", "appName": format!("Editor {MARKER}"), "code": PAIRING_CODE, "requestedAt": 1u64 }],
            "issues": [{ "modelId": "gemma-3-12b", "state": "restarting", "message": "the engine exited with code 3" }],
            "engineSetup": { "state": "idle" },
            "token": APP_KEY,
        })
    }

    impl Ask for Scripted {
        fn overview(&self) -> Result<Value, String> {
            Ok(overview_fixture())
        }
        fn service(&self) -> Result<Value, String> {
            Err("the service manager cannot be asked here".into())
        }
    }

    fn dirs(root: &Path) -> AppDirs {
        let id = AppId::new("pitwall", "Pitwall").unwrap();
        AppDirs::resolve(&id, Os::MacOs, &MapVars::new().with("PITWALL_HOME", root), None, true).unwrap()
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A data folder as a used Pitwall has it, with the marker in everything a report must leave out.
    fn planted(root: &Path) -> AppDirs {
        let d = dirs(root);
        let data = d.data().to_path_buf();
        for private in [
            "chats/2026-11-08-chat.json",
            "chats/attachments/photo.txt",
            "attachments/notes.txt",
            "presets.json",
            "apps.json",
            "admin.key",
            "cache.sqlite",
            "slots/model-a.bin",
            "partial/model-b.gguf.part",
            "models/model-c.gguf",
            "usage.json",
            "metrics.json",
        ] {
            write(&data.join(private), &format!("{{\"text\": \"{MARKER}\"}}"));
        }
        std::fs::create_dir_all(data.join("engines/llama-b5000")).unwrap();
        write(
            &data.join("settings.toml"),
            &format!("[api]\nport = 11500\nadmin = \"{APP_KEY}\"\n"),
        );
        write(&data.join("engine-pins.json"), "{\"pins\": []}");
        let logs = d.logs().to_path_buf();
        let line = |process: &str, extra: &str| {
            format!("{{\"@t\":\"2026-11-08T10:00:00Z\",\"@l\":\"Information\",\"@m\":\"{process} started {extra}\"}}\n")
        };
        write(&logs.join("pitwall.2026-11-08.log"), &line("app", &format!("key {APP_KEY}")));
        write(
            &logs.join("pitwall-server.2026-11-08.log"),
            &line("server", &format!("mcp {MCP_KEY}")),
        );
        write(&logs.join("pitwall-cli.2026-11-08.log"), &line("cli", ""));
        d
    }

    fn collect(d: &AppDirs) -> Collected {
        let mut sections = paddock_diag::builtin::all();
        sections.extend(pitwall_sections(d.data().to_path_buf(), Arc::new(Scripted)));
        let info = AppInfo::new("pitwall", "Pitwall", "1.1.0", "abc1234");
        let now = chrono::DateTime::from_timestamp(1_762_596_000, 0).unwrap();
        Collected::collect(&info, d, &sections, &Redactor::new(), now)
    }

    #[test]
    fn a_report_leaves_chats_attachments_prompts_and_keys_out_and_redacts_leaked_keys() {
        let root = tempfile::tempdir().unwrap();
        let d = planted(root.path());
        let c = collect(&d);
        let names: Vec<&str> = c.files().iter().map(|f| f.name.as_str()).collect();
        for f in c.files() {
            let text = String::from_utf8_lossy(&f.data);
            assert!(!text.contains(MARKER), "{} holds planted private text", f.name);
            assert!(!text.contains(APP_KEY) && !text.contains(MCP_KEY), "{} holds a key: {text}", f.name);
        }
        for want in [
            "pitwall/settings.toml",
            "pitwall/state/engine-pins.json",
            "pitwall/inventory.json",
            "pitwall/server.json",
            "pitwall/service.json",
        ] {
            assert!(names.contains(&want), "{want} missing from {names:?}");
        }
        let settings = c.files().iter().find(|f| f.name == "pitwall/settings.toml").unwrap();
        assert!(
            String::from_utf8_lossy(&settings.data).contains("port = 11500"),
            "the rest of the settings stays"
        );
        // The inventory names the model file without reading it.
        let inventory = c.files().iter().find(|f| f.name == "pitwall/inventory.json").unwrap();
        let inventory = String::from_utf8_lossy(&inventory.data);
        assert!(
            inventory.contains("model-c.gguf") && inventory.contains("llama-b5000"),
            "{inventory}"
        );
        // A service that could not be asked says so instead of leaving a gap.
        let service = c.files().iter().find(|f| f.name == "pitwall/service.json").unwrap();
        assert!(String::from_utf8_lossy(&service.data).contains("cannot be asked here"));
    }

    fn file_text(c: &Collected, name: &str) -> String {
        String::from_utf8_lossy(
            &c.files()
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{name} missing"))
                .data,
        )
        .into_owned()
    }

    /// Review P1-1 and P1-2 of B4: `server.json` held what apps.json holds (app names, key prefixes, the live pairing code), and
    /// the JSON field rules cut a long nested value into one string and masked `contextTokens`.
    #[test]
    fn the_server_view_keeps_the_server_and_counts_apps_and_pairings() {
        let root = tempfile::tempdir().unwrap();
        let c = collect(&planted(root.path()));
        let text = file_text(&c, "pitwall/server.json");
        assert!(
            !text.contains(MARKER) && !text.contains(PAIRING_CODE) && !text.contains("pw_ab12"),
            "{text}"
        );
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("server.json parses back: {e}\n{text}"));
        assert_eq!(v["server"]["version"], "1.1.0");
        // A 13-digit number could read as a card number to the text redactor: it is a string, so the file stays valid JSON.
        assert!(v["server"]["startedAt"].is_string(), "{text}");
        assert_eq!(v["models"].as_array().map(Vec::len), Some(3), "{text}");
        for m in v["models"].as_array().unwrap() {
            assert_eq!(m["contextTokens"], 8192, "{m}");
            assert_eq!(m["slots"], 4);
            assert!(m["ramBytes"].is_u64() && m["state"] == "ready");
        }
        assert_eq!(v["apps"], json!({ "total": 2, "connected": 1 }));
        assert_eq!(v["pairings"], json!({ "pending": 1 }));
        assert_eq!(v["queue"], json!({ "running": 1, "waiting": 0 }));
        assert_eq!(v["engines"][0]["activeBuild"], "b5000");
        assert_eq!(v["issues"][0]["modelId"], "gemma-3-12b");
        assert_eq!(v["engineSetup"]["state"], "idle");
        assert!(v.get("token").is_none() && v.get("throughput").is_some());
        // The service's state is pretty text too, and parses back.
        let service: Value = serde_json::from_str(&file_text(&c, "pitwall/service.json")).unwrap();
        assert_eq!(service["answered"], false);
    }

    /// Review P1-3 of B4: downloads are `models/<owner>/<repo>/<file>.gguf`, so the inventory lists those files by relative
    /// path and size, the projector files too, and nothing else; no link is followed and the walk stops at the scan depth.
    #[test]
    fn the_inventory_lists_the_model_files_where_downloads_put_them() {
        let root = tempfile::tempdir().unwrap();
        let d = dirs(root.path());
        let models = d.data().join("models");
        write(&models.join("bartowski/Qwen3-8B-GGUF/Qwen3-8B-Q4_K_M.gguf"), "1234");
        write(&models.join("bartowski/Qwen3-8B-GGUF/mmproj-Qwen3-8B-F16.gguf"), "12");
        write(&models.join("bartowski/Qwen3-8B-GGUF/README.md"), "not a model");
        write(&models.join("a/b/c/d/deep.gguf"), "1");
        write(&models.join("a/b/c/d/e/too-deep.gguf"), "1");
        write(&models.join("mine.gguf"), "123");
        #[cfg(unix)]
        {
            write(&root.path().join("elsewhere/linked.gguf"), "1");
            std::os::unix::fs::symlink(root.path().join("elsewhere"), models.join("link")).unwrap();
        }
        let c = collect(&d);
        let v: Value = serde_json::from_str(&file_text(&c, "pitwall/inventory.json")).unwrap();
        let entries = v["models"]["entries"].as_array().unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e["path"].as_str().unwrap()).collect();
        assert_eq!(
            paths,
            [
                "a/b/c/d/deep.gguf",
                "bartowski/Qwen3-8B-GGUF/Qwen3-8B-Q4_K_M.gguf",
                "bartowski/Qwen3-8B-GGUF/mmproj-Qwen3-8B-F16.gguf",
                "mine.gguf"
            ]
        );
        let q = entries
            .iter()
            .find(|e| e["path"].as_str().unwrap().ends_with("Q4_K_M.gguf"))
            .unwrap();
        assert_eq!((q["bytes"].as_u64(), q["mmproj"].as_bool()), (Some(4), Some(false)));
        assert_eq!(entries[2]["mmproj"], true);
        assert_eq!(v["models"]["count"], 4);
    }

    /// B4 review P2-5: with PITWALL_HOME set in a release build, the server and the command line write to
    /// `<PITWALL_HOME>/logs` (their own rule) while the app's folders ignore the variable; the report reads that folder too.
    #[test]
    fn logs_written_to_a_moved_data_folder_are_in_the_report() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("moved");
        let line = "{\"@t\":\"2026-11-08T10:00:00Z\",\"@l\":\"Information\",\"@m\":\"server started\"}\n";
        write(&home.join("logs/pitwall-server.2026-11-08.log"), line);
        write(&home.join("logs/pitwall-cli.2026-11-08.log"), line);
        write(&home.join("logs/other-app.2026-11-08.log"), line);
        let d = dirs(&root.path().join("app"));
        let moved = moved_logs(Some(home.to_str().unwrap()), d.logs()).expect("a moved folder that is not the app's own");
        assert_eq!(moved, home.join("logs"));
        let info = AppInfo::new("pitwall", "Pitwall", "1.1.0", "abc1234");
        let now = chrono::DateTime::from_timestamp(1_762_596_000, 0).unwrap();
        let sections: Vec<Arc<dyn Section>> = vec![Arc::new(MovedLogs { dir: moved })];
        let c = Collected::collect(&info, &d, &sections, &Redactor::new(), now);
        let names: Vec<&str> = c.files().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            ["logs/moved/pitwall-cli.2026-11-08.log", "logs/moved/pitwall-server.2026-11-08.log"],
            "{names:?}"
        );
        // No variable, an empty one, or a folder that is the app's own: nothing more to read.
        assert_eq!(moved_logs(None, d.logs()), None);
        assert_eq!(moved_logs(Some(""), d.logs()), None);
        assert_eq!(moved_logs(Some(d.data().to_str().unwrap()), &d.data().join("logs")), None);
    }

    #[test]
    fn the_logs_of_the_app_the_server_and_the_command_line_are_all_in_the_report() {
        let root = tempfile::tempdir().unwrap();
        let c = collect(&planted(root.path()));
        let names: Vec<&str> = c.files().iter().map(|f| f.name.as_str()).collect();
        for want in [
            "logs/pitwall.2026-11-08.log",
            "logs/pitwall-server.2026-11-08.log",
            "logs/pitwall-cli.2026-11-08.log",
        ] {
            assert!(names.contains(&want), "{want} missing from {names:?}");
        }
    }

    #[test]
    fn an_empty_folder_gives_notes_and_an_empty_inventory_not_errors() {
        let root = tempfile::tempdir().unwrap();
        let d = dirs(root.path());
        let c = collect(&d);
        assert!(c.notes().iter().any(|n| n.contains("no settings file yet")), "{:?}", c.notes());
        let inventory = c.files().iter().find(|f| f.name == "pitwall/inventory.json").unwrap();
        assert!(String::from_utf8_lossy(&inventory.data).contains("\"listed\": false"));
    }

    #[test]
    fn a_large_settings_file_is_left_out_with_a_note_and_a_long_listing_is_cut() {
        let root = tempfile::tempdir().unwrap();
        let d = dirs(root.path());
        let data = d.data().to_path_buf();
        write(&data.join("settings.toml"), &"x".repeat(MAX_FILE as usize + 1));
        for i in 0..(MAX_LISTED + 5) {
            write(&data.join(format!("models/m{i:04}.gguf")), "");
        }
        let c = collect(&d);
        assert!(c.notes().iter().any(|n| n.contains("larger than")), "{:?}", c.notes());
        let v: Value = serde_json::from_slice(&c.files().iter().find(|f| f.name == "pitwall/inventory.json").unwrap().data).unwrap();
        assert_eq!(v["models"]["count"], MAX_LISTED + 5);
        assert_eq!(v["models"]["entries"].as_array().unwrap().len(), MAX_LISTED);
    }

    #[test]
    fn a_question_that_hangs_ends_at_the_timeout_and_says_so() {
        static BUSY: AtomicBool = AtomicBool::new(false);
        let (_hold, wait) = std::sync::mpsc::channel::<()>();
        // The question never answers (it waits on a channel nobody sends on): the wait ends at its bound.
        let why = bounded(&BUSY, Duration::from_millis(20), move || {
            let _ = wait.recv();
            Ok(Value::Null)
        })
        .unwrap_err();
        assert!(why.contains("no answer within 20 ms"), "{why}");
        // B4 review P2-5: while that question still hangs, another is not started (threads cannot pile up).
        let again = bounded(&BUSY, ASK_TIMEOUT, || unreachable!("not asked while the last one waits")).unwrap_err();
        assert!(again.contains("still waiting"), "{again}");
        // A question that ends frees the next one before its answer is handed over.
        static FREE: AtomicBool = AtomicBool::new(false);
        assert_eq!(bounded(&FREE, ASK_TIMEOUT, || Ok(json!(1))), Ok(json!(1)));
        assert!(!FREE.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(bounded(&FREE, ASK_TIMEOUT, || Ok(json!(2))), Ok(json!(2)));
    }
}
