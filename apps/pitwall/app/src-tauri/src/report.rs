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

/// The sections of the installed app over its data folder.
pub fn sections<R: tauri::Runtime>(app: &tauri::AppHandle<R>, data: PathBuf) -> Vec<Arc<dyn Section>> {
    pitwall_sections(data, Arc::new(Live(app.clone())))
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
                "The server's settings; passwords, tokens and keys are replaced by ***",
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
        let models = listing(&self.data.join("models"));
        SectionOutput::one(SectionFile::app(
            "pitwall/inventory.json",
            "The engines and model files installed, by name and size (their contents are not in the report)",
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
            let value = match answer {
                Ok(v) => v,
                Err(why) => json!({ "answered": false, "why": why }),
            };
            out.files.push(SectionFile::app(name, summary, Content::Json(value)));
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

/// The entries of `dir` (one level, sorted, at most [`MAX_LISTED`]) with their sizes; nothing is opened.
fn listing(dir: &Path) -> Value {
    let Ok(read) = std::fs::read_dir(dir) else {
        return json!({ "listed": false });
    };
    let mut entries: Vec<(String, Option<u64>, bool)> = read
        .flatten()
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
        let client = self.client()?;
        bounded(ASK_TIMEOUT, move || {
            tauri::async_runtime::block_on(crate::commands::forward(client.as_ref(), "overview", Value::Null)).map_err(|e| e.message)
        })
    }

    fn service(&self) -> Result<Value, String> {
        let client = self.client()?;
        bounded(ASK_TIMEOUT, move || {
            let state = tauri::async_runtime::block_on(crate::app_commands::compose_state(&client));
            serde_json::to_value(state).map_err(|e| format!("the service state could not be written: {e}"))
        })
    }
}

/// Runs `f` on a thread of its own and waits at most `timeout` ([`ASK_TIMEOUT`]: the report is never held by a server that
/// hangs).
fn bounded(timeout: Duration, f: impl FnOnce() -> Result<Value, String> + Send + 'static) -> Result<Value, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("pitwall-report-ask".into())
        .spawn(move || {
            let _ = tx.send(f());
        })
        .map_err(|e| format!("the question could not be asked: {e}"))?;
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

    impl Ask for Scripted {
        fn overview(&self) -> Result<Value, String> {
            Ok(json!({ "server": { "version": "1.1.0", "serving": "serving" }, "token": APP_KEY }))
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
        let (_hold, wait) = std::sync::mpsc::channel::<()>();
        // The question never answers (it waits on a channel nobody sends on): the wait ends at its bound.
        let why = bounded(Duration::from_millis(20), move || {
            let _ = wait.recv();
            Ok(Value::Null)
        })
        .unwrap_err();
        assert!(why.contains("no answer within 20 ms"), "{why}");
        assert_eq!(bounded(ASK_TIMEOUT, || Ok(json!(1))), Ok(json!(1)));
    }
}
