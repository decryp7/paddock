//! Dyno's part of Report a problem (`paddock-diag` sections): the agent's data files, the administrator helper (so a broken helper
//! can be explained from the report alone), how Dyno is registered to start at login, and the install's own logs and crash reports.
//! Every section reads through a small port (the agent link, a tool runner, a folder) so the tests run on temporary folders and
//! scripted tools (ER-14); the OS commands run as the standard user, with a timeout, and their output is redacted with the rest.

use dyno_core::diagnostics::DiagnosticsFiles;
use dyno_core::helper::HelperHost;
use dyno_core::helper_paths::{LINUX_UNIT, MAC_LABEL, WINDOWS_SERVICE};
use dyno_core::helper_wire::HelperStatus;
use dyno_platform::runner::{tool, Tool, ToolRunner};
use paddock_diag::{Content, Context, Section, SectionFile, SectionOutput};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// How long one OS command may take (they only read state).
const OS_TIMEOUT: Duration = Duration::from_secs(5);
/// The most of a command's output kept.
const MAX_OUTPUT: usize = 8 * 1024;
/// The Event Log window and cap for the helper's entries.
const EVENT_HOURS: u32 = 24 * 7;
/// The longest one OS command may take in the tests' scripted runner (the Event Log read scales with its window).
#[cfg(test)]
const LONGEST_TOOL: Duration = Duration::from_secs(120);
const EVENT_MAX: u32 = 50;

/// What the sections ask of the agent. The real one is the agent link; a test names its answers.
pub trait AgentPort: Send + Sync {
    /// The agent's data files, or why it did not answer.
    fn files(&self) -> Result<DiagnosticsFiles, String>;
    /// The helper's status as the agent sees it, or why it did not answer.
    fn helper_status(&self) -> Result<HelperStatus, String>;
}

impl AgentPort for crate::link::AgentLink {
    fn files(&self) -> Result<DiagnosticsFiles, String> {
        match self.call(dyno_core::protocol::Request::DiagnosticsFiles) {
            Ok(dyno_core::protocol::Reply::DiagnosticsFiles(f)) => Ok(*f),
            Ok(_) => Err("the agent answered with the wrong kind of reply".into()),
            Err(e) => Err(e.message),
        }
    }

    fn helper_status(&self) -> Result<HelperStatus, String> {
        match self.call(dyno_core::protocol::Request::HelperStatus) {
            Ok(dyno_core::protocol::Reply::HelperStatus(s)) => Ok(*s),
            Ok(_) => Err("the agent answered with the wrong kind of reply".into()),
            Err(e) => Err(e.message),
        }
    }
}

/// The agent link, looked up when a report is made (the link is created after the report's setup, and may be absent in a test place).
pub struct LazyAgent<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> LazyAgent<R> {
    fn link(&self) -> Result<Arc<crate::link::AgentLink>, String> {
        use tauri::Manager;
        self.0
            .try_state::<Arc<crate::link::AgentLink>>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| "the agent link is not running".to_string())
    }
}

impl<R: tauri::Runtime> AgentPort for LazyAgent<R> {
    fn files(&self) -> Result<DiagnosticsFiles, String> {
        self.link()?.files()
    }

    fn helper_status(&self) -> Result<HelperStatus, String> {
        self.link()?.helper_status()
    }
}

/// The sections of this machine, for the installed app: the agent's files, the helper, the start at login and the install.
pub fn sections<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Vec<Arc<dyn Section>> {
    disk_sections(Arc::new(LazyAgent(app.clone())))
}

fn sections_of<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Vec<Arc<dyn Section>> {
    sections(app)
}

/// The numeric id of the account running Dyno (macOS `gui/<uid>` domains), from the owner of the home folder; 0 when unknown.
fn owner_id(home: Option<&Path>) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        home.and_then(|h| std::fs::metadata(h).ok()).map_or(0, |m| m.uid())
    }
    #[cfg(not(unix))]
    {
        let _ = home;
        0
    }
}

/// The agent is not asked by a report written without a window (its files are the data of a running agent; the logs, settings,
/// crash markers, helper and startup facts are read from disk and the OS).
pub struct NoAgent;

impl AgentPort for NoAgent {
    fn files(&self) -> Result<DiagnosticsFiles, String> {
        Err("a report written without the window does not ask it".into())
    }

    fn helper_status(&self) -> Result<HelperStatus, String> {
        Err("a report written without the window does not ask the agent".into())
    }
}

/// The sections that need no running app: the OS, the disk and (when `agent` answers) the agent.
#[allow(
    clippy::disallowed_methods,
    reason = "the report describes this machine, so the build OS is the host"
)]
fn disk_sections(agent: Arc<dyn AgentPort>) -> Vec<Arc<dyn Section>> {
    let runner: Arc<dyn ToolRunner> = Arc::new(dyno_platform::runner::SystemRunner);
    let host = dyno_agent::helper_os::host();
    let exe = std::env::current_exe().unwrap_or_default();
    let home = home_dir();
    vec![
        Arc::new(AgentFiles::new(agent.clone())),
        Arc::new(HelperReport::new(
            agent,
            runner.clone(),
            host,
            dyno_agent::helper_os::helper_paths().lifecycle,
        )),
        Arc::new(StartupReport::new(runner, host, owner_id(home.as_deref()), exe.clone())),
        Arc::new(InstallReport::new(exe.parent().map(Path::to_path_buf))),
    ]
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// The folders a headless report is written to when none is named, in order.
fn default_dirs() -> Vec<PathBuf> {
    home_dir()
        .into_iter()
        .flat_map(|h| [h.join("Documents"), h.join("Downloads"), h])
        .collect()
}

/// `dyno-app --export-diagnostics [<path.zip>] [--force]`: the report without a window, for a window that failed to start. `None` when
/// the arguments do not ask for it; else the exit code (0, or 1 with the reason printed and logged).
pub fn export_cli(args: &[String], app: &paddock_diag::AppInfo) -> Option<i32> {
    let parsed = paddock_diag::export::parse(args)?;
    // A release build on Windows has no console: started from a terminal it would print nothing (shared with `dyno-agent`).
    paddock_proc::console::attach_parent_console();
    let outcome = parsed.and_then(|a| {
        let id = paddock_env::AppId::new(&app.id, &app.name).map_err(|e| e.to_string())?;
        let dirs = paddock_env::AppDirs::for_app(&id).map_err(|e| e.to_string())?;
        let now = chrono::Utc::now();
        let cwd = std::env::current_dir().unwrap_or_default();
        let dest = paddock_diag::export::resolve(&a, app, &cwd, &default_dirs(), now)?;
        let mut sections = paddock_diag::builtin::all();
        sections.extend(disk_sections(Arc::new(NoAgent)));
        // A report the user reads and saves on this machine: private network addresses stay (paddock_diag::Redactor).
        let redactor = paddock_diag::Redactor::for_machine(&paddock_env::ProcessVars).keep_local_addresses();
        paddock_diag::export::write(app, &dirs, &sections, &redactor, &dest, now)
    });
    Some(match outcome {
        Ok(saved) => {
            log::info!(Files = saved.files; "Problem report written without the window");
            println!(
                "Wrote {} ({} files, {} bytes). Look through it before you share it: issue reports are public.",
                saved.path, saved.files, saved.bytes
            );
            0
        }
        Err(why) => {
            log::error!("Problem report without the window failed: {why}");
            eprintln!("{} could not write the report: {why}", app.name);
            1
        }
    })
}

/// Only one report from the question is written at a time, and the question is not answered again while it is: the save dialog is
/// modal to nobody, so a second click would open a second dialog.
static SAVING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Holds the one save slot; dropping it (also while unwinding from a panic in the report hook) frees the slot.
struct SavingSlot;

impl Drop for SavingSlot {
    fn drop(&mut self) {
        SAVING.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl SavingSlot {
    /// The slot, or `None` when a save is under way.
    fn take() -> Option<SavingSlot> {
        if SAVING.swap(true, std::sync::atomic::Ordering::SeqCst) {
            None
        } else {
            Some(SavingSlot)
        }
    }
}

/// Tells the person in a native message that the report could not be saved (the question's own flow has no other place to say it).
fn tell_failed<R: tauri::Runtime>(app: &tauri::AppHandle<R>, why: &str) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    app.dialog()
        .message(format!(
            "The problem report could not be saved.\n\n{why}\n\nChoose a different folder, or try again."
        ))
        .title("Dyno could not save the report")
        .kind(MessageDialogKind::Error)
        .blocking_show();
}

/// The watchdog question's Save a problem report…: a native save dialog (its own thread, never the stuck window), then the report
/// as `--export-diagnostics` writes it, plus the agent's files when the agent answers.
pub fn save_report_from_question<R: tauri::Runtime>(app: &tauri::AppHandle<R>, app_info: &paddock_diag::AppInfo) -> Result<(), String> {
    let Some(slot) = SavingSlot::take() else {
        log::info!("A problem report is already being saved; the second request was ignored");
        return Ok(());
    };
    let outcome = save_report_inner(app, app_info);
    drop(slot);
    if let Err(why) = &outcome {
        tell_failed(app, why);
    }
    outcome
}

fn save_report_inner<R: tauri::Runtime>(app: &tauri::AppHandle<R>, app_info: &paddock_diag::AppInfo) -> Result<(), String> {
    use tauri_plugin_dialog::DialogExt;
    let now = chrono::Utc::now();
    let Some(chosen) = app
        .dialog()
        .file()
        .set_title("Save a problem report")
        .set_file_name(paddock_diag::suggested_name(app_info, now))
        .add_filter("Zip archive", &["zip"])
        .blocking_save_file()
    else {
        return Ok(());
    };
    let dest = chosen.into_path().map_err(|e| format!("that location cannot be used ({e})"))?;
    let id = paddock_env::AppId::new(&app_info.id, &app_info.name).map_err(|e| e.to_string())?;
    let dirs = paddock_env::AppDirs::for_app(&id).map_err(|e| e.to_string())?;
    let mut sections = paddock_diag::builtin::all();
    sections.extend(sections_of(app));
    // A report the user reads and saves on this machine: private network addresses stay (paddock_diag::Redactor).
    let redactor = paddock_diag::Redactor::for_machine(&paddock_env::ProcessVars).keep_local_addresses();
    paddock_diag::export::write(app_info, &dirs, &sections, &redactor, &dest, now).map(|_| ())
}

/// `text` as a report file: JSON when it parses (so the field rules apply key by key), else text.
fn content_of(name: &str, text: String) -> Content {
    if name.ends_with(".json") {
        if let Ok(v) = serde_json::from_str(&text) {
            return Content::Json(v);
        }
    }
    Content::Text(text)
}

/// The agent's data files (inventory, alarms, sockets, activity, alerts, the recorder, the helper's log). When the agent is down the
/// report says so, and the rest of the report (logs, settings, crash markers) is still there because it is read from disk.
pub struct AgentFiles {
    agent: Arc<dyn AgentPort>,
}

impl AgentFiles {
    /// The section over `agent`.
    pub fn new(agent: Arc<dyn AgentPort>) -> Self {
        AgentFiles { agent }
    }
}

impl Section for AgentFiles {
    fn id(&self) -> &str {
        "agent"
    }

    fn collect(&self, _cx: &Context<'_>) -> SectionOutput {
        match self.agent.files() {
            Ok(f) => SectionOutput {
                files: f
                    .files
                    .into_iter()
                    .map(|file| {
                        let content = content_of(&file.name, file.text);
                        SectionFile::app(&file.name, &file.summary, content)
                    })
                    .collect(),
                notes: f.notes,
            },
            Err(why) => SectionOutput::note(format!(
                "Dyno's agent did not answer ({why}), so its inventory, alarms, connections, activity and recorder are not in this report. \
                 The logs, settings and crash markers are."
            )),
        }
    }
}

/// The OS tools that show the helper's registration as a standard user can see it.
pub fn helper_os_tools(host: HelperHost) -> Vec<(&'static str, Tool)> {
    match host {
        HelperHost::Macos => vec![("launchd", tool("/bin/launchctl", &["print", &format!("system/{MAC_LABEL}")]))],
        HelperHost::Windows => vec![("service", tool("sc.exe", &["queryex", WINDOWS_SERVICE]))],
        HelperHost::Linux => vec![(
            "systemd",
            tool(
                "systemctl",
                &[
                    "show",
                    LINUX_UNIT.trim_end_matches(".service"),
                    "--no-pager",
                    "--property=LoadState,ActiveState,SubState,Result,ExecMainStatus,ExecMainCode,NRestarts",
                ],
            ),
        )],
    }
}

/// One command and what it answered, as report text: the command line, the outcome and the output, bounded.
fn run_report(runner: &dyn ToolRunner, label: &str, t: &Tool) -> String {
    let line = format!("{} {}", t.program, t.args.join(" "));
    let body = match runner.run_tool(t, OS_TIMEOUT) {
        Ok(out) => {
            let mut text = out.stdout;
            text.truncate(text.char_indices().nth(MAX_OUTPUT).map_or(text.len(), |(i, _)| i));
            let status = if out.ok {
                "exit 0".to_string()
            } else {
                format!("failed: {}", out.error)
            };
            format!("{status}\n{text}")
        }
        Err(e) => format!("could not run: {e}\n"),
    };
    format!("== {label} ==\n$ {line}\n{body}\n")
}

/// The newest `max` bytes of a text file, cut to whole lines; the error kind when it cannot be read.
fn bounded_text(path: &Path, max: u64) -> Result<String, std::io::ErrorKind> {
    paddock_diag::window::try_tail(path, max)
        .map(|(bytes, _)| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|e| e.kind())
}

/// The administrator helper, explained without the helper: the status the agent sees (state, message, detail, the last audit
/// entries), the OS's own view of the registration, the lifecycle file the helper writes for exactly this, and (Windows) the
/// Event Log entries of the service. Each part that cannot be read says why in the report.
pub struct HelperReport {
    agent: Arc<dyn AgentPort>,
    runner: Arc<dyn ToolRunner>,
    host: HelperHost,
    lifecycle: Option<PathBuf>,
}

impl HelperReport {
    /// The section for `host`, reading the lifecycle file at `lifecycle`.
    pub fn new(agent: Arc<dyn AgentPort>, runner: Arc<dyn ToolRunner>, host: HelperHost, lifecycle: Option<PathBuf>) -> Self {
        HelperReport {
            agent,
            runner,
            host,
            lifecycle,
        }
    }
}

impl Section for HelperReport {
    fn id(&self) -> &str {
        "helper"
    }

    fn collect(&self, _cx: &Context<'_>) -> SectionOutput {
        let mut out = SectionOutput::default();
        match self.agent.helper_status() {
            Ok(status) => out.files.push(SectionFile::app(
                "helper/status.json",
                "The administrator helper's state as the agent sees it: state, message, detail, the last requests it served",
                Content::Json(serde_json::to_value(&status).unwrap_or_default()),
            )),
            Err(why) => out
                .notes
                .push(format!("The helper's status could not be asked of the agent ({why}).")),
        }
        let state: String = helper_os_tools(self.host)
            .iter()
            .map(|(label, t)| run_report(self.runner.as_ref(), label, t))
            .collect();
        out.files.push(SectionFile::app(
            "helper/os-state.txt",
            "What the operating system says about the helper's registration (read as you, no administrator rights)",
            Content::Text(state),
        ));
        match &self.lifecycle {
            // Windows has no lifecycle file (a standard user can make the folder a SYSTEM service would write in): the Event Log
            // entries below say when the service started, stopped and failed.
            None => out.notes.push("There is no lifecycle file on this system; the Windows Event Log entries show when the helper started and stopped.".into()),
            Some(path) => match bounded_text(path, 32 * 1024) {
                Ok(text) => out.files.push(SectionFile::app(
                    "helper/lifecycle.txt",
                    "When the helper started, served and stopped, and why it refused to start (t= is seconds since 1970)",
                    Content::Text(text),
                )),
                Err(kind) => out.notes.push(format!(
                    "The helper's lifecycle file is not readable ({kind:?}). If the folder it lives in does not exist, the helper has never started on this computer; otherwise it was removed or its rights were changed."
                )),
            },
        }
        if self.host == HelperHost::Windows {
            let lines = dyno_platform::issues::windows::source_events(self.runner.as_ref(), WINDOWS_SERVICE, EVENT_HOURS, EVENT_MAX);
            out.files.push(SectionFile::app(
                "helper/event-log.txt",
                "Windows Event Log entries of the Dyno helper service from the last week",
                Content::Text(lines.join("\n") + "\n"),
            ));
        }
        out
    }
}

/// How the folder a program runs from affects starting it at login: an installed app, a downloaded or mounted copy, a portable one.
pub fn path_kind(exe: &Path) -> &'static str {
    let p = exe.to_string_lossy().replace('\\', "/").to_lowercase();
    if p.contains("/apptranslocation/") {
        "translocated (macOS runs it from a temporary place until it is moved to Applications)"
    } else if p.starts_with("/volumes/") {
        "on a mounted volume (a disk image or another disk)"
    } else if p.contains("/downloads/") {
        "in the Downloads folder"
    } else if p.starts_with("/applications/") || p.contains("/program files") || p.starts_with("/usr/") || p.starts_with("/opt/") {
        "in a system application folder"
    } else if p.contains("/appdata/local/programs/") || p.contains("/.local/") || p.contains("/applications/") {
        "in a per-user application folder"
    } else {
        "somewhere else"
    }
}

/// The OS tools that show how Dyno is registered to start at login, read as the user.
pub fn startup_tools(host: HelperHost, uid: u32) -> Vec<(&'static str, Tool)> {
    match host {
        HelperHost::Windows => vec![
            (
                "Run values",
                tool("reg.exe", &["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run"]),
            ),
            (
                "startup approval",
                tool(
                    "reg.exe",
                    &[
                        "query",
                        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
                    ],
                ),
            ),
        ],
        HelperHost::Macos => vec![
            (
                "launchd disabled list",
                tool("/bin/launchctl", &["print-disabled", &format!("gui/{uid}")]),
            ),
            (
                "login items (agent job)",
                tool("/bin/launchctl", &["print", &format!("gui/{uid}/net.decryptology.dyno.agent")]),
            ),
        ],
        HelperHost::Linux => vec![("systemd user unit", tool("systemctl", &["--user", "is-enabled", "dyno-agent"]))],
    }
}

/// Only Dyno's lines of a registry listing or launchd list (other programs' startup entries are not this report's business), and the
/// outcome of the command.
pub fn dyno_lines(text: &str) -> String {
    let outcome = |l: &str| l.starts_with("exit ") || l.starts_with("failed:") || l.starts_with("could not run:");
    let kept: Vec<&str> = text.lines().filter(|l| outcome(l) || l.to_lowercase().contains("dyno")).collect();
    if kept.is_empty() {
        "(no Dyno entry)\n".into()
    } else {
        kept.join("\n") + "\n"
    }
}

/// Why a start at login might not have happened: the OS registration, the install type and the kind of place it runs from. The
/// start reason and signals are logged by the app itself at every start (see the log), not repeated here.
pub struct StartupReport {
    runner: Arc<dyn ToolRunner>,
    host: HelperHost,
    uid: u32,
    exe: PathBuf,
}

impl StartupReport {
    /// The section for `host`, for the user `uid` (macOS), of the app at `exe`.
    pub fn new(runner: Arc<dyn ToolRunner>, host: HelperHost, uid: u32, exe: PathBuf) -> Self {
        StartupReport { runner, host, uid, exe }
    }
}

impl Section for StartupReport {
    fn id(&self) -> &str {
        "startup"
    }

    fn collect(&self, cx: &Context<'_>) -> SectionOutput {
        let mut text = format!(
            "install type: {}\nplace: {}\nversion: {}\n\n",
            if cx.dirs.source() == paddock_env::Source::Portable {
                "portable"
            } else {
                "installed"
            },
            path_kind(&self.exe),
            cx.app.version
        );
        for (label, t) in startup_tools(self.host, self.uid) {
            let report = run_report(self.runner.as_ref(), label, &t);
            // The raw listings hold other programs' entries; keep Dyno's lines and the command outcome.
            let (head, body) = report.split_once("\n$ ").map_or((report.as_str(), ""), |(h, b)| (h, b));
            text.push_str(&format!("{head}\n$ {}\n", dyno_lines(body)));
        }
        SectionOutput::one(SectionFile::app(
            "startup/registration.txt",
            "How Dyno is registered to start at login, as the OS reports it, and where Dyno runs from",
            Content::Text(text),
        ))
    }
}

/// The install's own logs: the installer's `updates/setup.log` (and the rotated one). The OS's crash report names are the
/// section the tray login work adds (one copy, not two).
pub struct InstallReport {
    install_dir: Option<PathBuf>,
}

impl InstallReport {
    /// The section: `install_dir` is the folder the app is installed in (holds `updates/`).
    pub fn new(install_dir: Option<PathBuf>) -> Self {
        InstallReport { install_dir }
    }
}

impl Section for InstallReport {
    fn id(&self) -> &str {
        "install"
    }

    fn collect(&self, _cx: &Context<'_>) -> SectionOutput {
        let mut out = SectionOutput::default();
        if let Some(dir) = &self.install_dir {
            for name in ["setup.log", "setup.log.1"] {
                let path = dir.join("updates").join(name);
                match paddock_diag::window::try_tail(&path, 256 * 1024) {
                    Ok((bytes, cut)) if !bytes.is_empty() => out.files.push(SectionFile::app(
                        &format!("install/{name}"),
                        if cut {
                            "The recent part of the installer's log"
                        } else {
                            "The installer's log"
                        },
                        Content::Log(bytes),
                    )),
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => out
                        .notes
                        .push(format!("The installer's log {name} cannot be read ({:?}).", e.kind())),
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::diagnostics::DiagnosticsFile;
    use dyno_core::helper_wire::HelperState;
    use paddock_diag::{AppInfo, Collected, Redactor};
    use paddock_env::{AppDirs, AppId, MapVars, Os};
    use std::sync::Mutex;

    struct Agent {
        files: Result<DiagnosticsFiles, String>,
        status: Result<HelperStatus, String>,
    }

    impl AgentPort for Agent {
        fn files(&self) -> Result<DiagnosticsFiles, String> {
            self.files.clone()
        }
        fn helper_status(&self) -> Result<HelperStatus, String> {
            self.status.clone()
        }
    }

    /// Scripted tools: the answer for a program, and the calls made.
    struct Tools {
        calls: Mutex<Vec<String>>,
        answer: fn(&Tool) -> Result<dyno_platform::runner::ToolOutput, dyno_core::ports::PlatformError>,
    }

    impl ToolRunner for Tools {
        fn run_tool(&self, t: &Tool, timeout: Duration) -> Result<dyno_platform::runner::ToolOutput, dyno_core::ports::PlatformError> {
            assert!(timeout <= LONGEST_TOOL, "every OS command has a timeout");
            self.calls.lock().unwrap().push(format!("{} {}", t.program, t.args.join(" ")));
            (self.answer)(t)
        }
    }

    fn ok(stdout: &str) -> Result<dyno_platform::runner::ToolOutput, dyno_core::ports::PlatformError> {
        Ok(dyno_platform::runner::ToolOutput {
            ok: true,
            stdout: stdout.into(),
            error: String::new(),
        })
    }

    fn status() -> HelperStatus {
        HelperStatus {
            state: HelperState::NotRunning,
            message: "The helper is installed but not answering.".into(),
            version: Some("1.1.0".into()),
            prompt: String::new(),
            recent: vec![],
            readings: vec![],
            detail: Some("the endpoint did not answer: connection refused".into()),
            error: None,
        }
    }

    fn dirs(root: &Path) -> AppDirs {
        let id = AppId::new("dyno", "Dyno").unwrap();
        AppDirs::resolve(&id, Os::MacOs, &MapVars::new().with("DYNO_HOME", root), None, true).unwrap()
    }

    fn collect(sections: Vec<Arc<dyn Section>>, root: &Path) -> Collected {
        let info = AppInfo::new("dyno", "Dyno", "1.1.0", "abc1234");
        let now = chrono::DateTime::from_timestamp(1_762_333_200, 0).unwrap();
        Collected::collect(&info, &dirs(root), &sections, &Redactor::new(), now)
    }

    fn file_text(c: &Collected, name: &str) -> Option<String> {
        c.files()
            .iter()
            .find(|f| f.name == name)
            .map(|f| String::from_utf8_lossy(&f.data).into_owned())
    }

    #[test]
    fn a_broken_helper_is_explained_by_the_report_alone() {
        let root = paddock_testkit::private_tempdir().unwrap();
        let lifecycle = root.path().join("helper-lifecycle.log");
        std::fs::write(
            &lifecycle,
            "t=1 starting pid=4 version=1.1.0\nt=2 refused:NotProtected pid=4 version=1.1.0\n",
        )
        .unwrap();
        let agent = Arc::new(Agent {
            files: Ok(DiagnosticsFiles::default()),
            status: Ok(status()),
        });
        let tools = Arc::new(Tools {
            calls: Mutex::new(vec![]),
            answer: |t| {
                if t.program == "sc.exe" {
                    ok("STATE : 1 STOPPED\nWIN32_EXIT_CODE : 1066")
                } else {
                    ok("(2 entries)")
                }
            },
        });
        let section = HelperReport::new(agent, tools.clone(), HelperHost::Windows, Some(lifecycle));
        let c = collect(vec![Arc::new(section)], root.path());
        let st = file_text(&c, "helper/status.json").expect("status.json");
        assert!(st.contains("notRunning") && st.contains("connection refused"), "{st}");
        let os = file_text(&c, "helper/os-state.txt").unwrap();
        assert!(os.contains("$ sc.exe queryex DynoHelper") && os.contains("STOPPED"), "{os}");
        assert!(file_text(&c, "helper/lifecycle.txt").unwrap().contains("refused:NotProtected"));
        let ev = file_text(&c, "helper/event-log.txt").unwrap();
        assert!(ev.contains("Application") && ev.contains("System"), "{ev}");
        assert!(tools.calls.lock().unwrap().iter().any(|l| l.starts_with("wevtutil.exe")));
    }

    #[test]
    fn a_panic_in_the_save_does_not_leave_the_slot_taken() {
        let first = SavingSlot::take().expect("free at first");
        assert!(SavingSlot::take().is_none(), "one save at a time");
        drop(first);
        let panicked = std::panic::catch_unwind(|| {
            let _slot = SavingSlot::take().expect("free again");
            panic!("the report hook panicked");
        });
        assert!(panicked.is_err());
        assert!(SavingSlot::take().is_some(), "the slot is free after the panic");
    }

    #[test]
    fn windows_has_no_lifecycle_file_and_says_where_to_look_instead() {
        let root = paddock_testkit::private_tempdir().unwrap();
        let agent = Arc::new(Agent {
            files: Ok(DiagnosticsFiles::default()),
            status: Ok(status()),
        });
        let tools = Arc::new(Tools {
            calls: Mutex::new(vec![]),
            answer: |_| ok(""),
        });
        let c = collect(
            vec![Arc::new(HelperReport::new(agent, tools, HelperHost::Windows, None))],
            root.path(),
        );
        assert!(file_text(&c, "helper/lifecycle.txt").is_none());
        assert!(
            c.notes().iter().any(|n| n.contains("no lifecycle file") && n.contains("Event Log")),
            "{:?}",
            c.notes()
        );
        assert!(
            file_text(&c, "helper/event-log.txt").is_some(),
            "the Event Log entries carry the history instead"
        );
    }

    #[test]
    fn what_cannot_be_read_becomes_a_note_not_a_missing_section() {
        let root = paddock_testkit::private_tempdir().unwrap();
        let agent = Arc::new(Agent {
            files: Err("no agent".into()),
            status: Err("no agent".into()),
        });
        let tools = Arc::new(Tools {
            calls: Mutex::new(vec![]),
            answer: |_| Err(dyno_core::ports::PlatformError::Failed("timed out".into())),
        });
        let c = collect(
            vec![
                Arc::new(HelperReport::new(
                    agent.clone(),
                    tools,
                    HelperHost::Linux,
                    Some(root.path().join("missing")),
                )),
                Arc::new(AgentFiles::new(agent)),
            ],
            root.path(),
        );
        let notes = c.notes().join(" | ");
        assert!(
            notes.contains("status could not be asked") && notes.contains("lifecycle file is not readable"),
            "{notes}"
        );
        assert!(notes.contains("agent did not answer"), "{notes}");
        let os = file_text(&c, "helper/os-state.txt").unwrap();
        assert!(os.contains("could not run: ") && os.contains("systemctl show dyno-helper"), "{os}");
    }

    #[test]
    fn the_agents_files_become_report_files_with_json_kept_structured() {
        let root = paddock_testkit::private_tempdir().unwrap();
        let files = DiagnosticsFiles {
            files: vec![
                DiagnosticsFile::new("alarms/open.json", "Alarms", r#"{"apiKey":"sk-invented"}"#.into()),
                DiagnosticsFile::new("helper/helper.log", "The helper's log", "line\n".into()),
            ],
            notes: vec!["the recorder is empty".into()],
        };
        let agent = Arc::new(Agent {
            files: Ok(files),
            status: Err(String::new()),
        });
        let c = collect(vec![Arc::new(AgentFiles::new(agent))], root.path());
        let alarms = file_text(&c, "alarms/open.json").unwrap();
        assert!(
            !alarms.contains("sk-invented"),
            "the field rules apply to the agent's JSON: {alarms}"
        );
        assert!(file_text(&c, "helper/helper.log").is_some());
        assert!(c.notes().iter().any(|n| n.contains("recorder is empty")));
    }

    #[test]
    fn the_startup_report_keeps_only_dynos_entries_and_says_where_it_runs_from() {
        let root = paddock_testkit::private_tempdir().unwrap();
        let tools = Arc::new(Tools {
            calls: Mutex::new(vec![]),
            answer: |_| {
                ok("    Dyno    REG_SZ    \"C:\\Users\\Alex\\AppData\\Local\\Programs\\Dyno\\dyno.exe\" --hidden\n    Spotify    REG_SZ    C:\\x\\spotify.exe\n")
            },
        });
        let exe = PathBuf::from(r"C:\Users\Alex\AppData\Local\Programs\Dyno\dyno.exe");
        let c = collect(vec![Arc::new(StartupReport::new(tools, HelperHost::Windows, 0, exe))], root.path());
        let t = file_text(&c, "startup/registration.txt").unwrap();
        assert!(t.contains("Dyno    REG_SZ") && !t.contains("Spotify"), "{t}");
        assert!(t.contains("per-user application folder") && t.contains("installed"), "{t}");
        assert_eq!(
            path_kind(Path::new("/private/var/folders/x/AppTranslocation/y/Dyno.app/x"))
                .split(' ')
                .next(),
            Some("translocated")
        );
        assert!(path_kind(Path::new("/Volumes/Dyno 1.1/Dyno.app")).starts_with("on a mounted volume"));
    }

    #[test]
    fn the_installers_log_and_the_rotated_one_are_in_the_report() {
        let root = paddock_testkit::private_tempdir().unwrap();
        let install = root.path().join("Dyno");
        std::fs::create_dir_all(install.join("updates")).unwrap();
        std::fs::write(install.join("updates").join("setup.log"), "step 3 failed\n").unwrap();
        std::fs::write(install.join("updates").join("setup.log.1"), "older run\n").unwrap();
        let c = collect(vec![Arc::new(InstallReport::new(Some(install)))], root.path());
        assert!(file_text(&c, "install/setup.log").unwrap().contains("step 3 failed"));
        assert!(file_text(&c, "install/setup.log.1").unwrap().contains("older run"));
        let none = collect(vec![Arc::new(InstallReport::new(Some(root.path().join("absent"))))], root.path());
        assert!(
            file_text(&none, "install/setup.log").is_none() && none.notes().is_empty(),
            "no installer log is not a problem"
        );
    }

    /// The composition: Dyno's sections with the built-in ones over temporary folders (ER-14: nothing of this machine), written as the
    /// real zip and opened again. Prints the listing so a run shows what a bundle holds (`--nocapture`).
    #[test]
    fn a_bundle_of_a_broken_helper_holds_what_explains_it_and_nothing_hostile() {
        let root = paddock_testkit::private_tempdir().unwrap();
        let d = dirs(root.path());
        for p in [d.logs(), d.config(), d.data()] {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(
            d.logs().join("dyno.2026-11-05.log"),
            "{\"@t\":\"2026-11-05T09:00:00Z\",\"@m\":\"Helper state\",\"State\":\"NotRunning\",\"Why\":\"opened /Users/sam/x token=hunter2hunter2\"}\n",
        )
        .unwrap();
        std::fs::write(d.data().join("crash-agent.json"), r#"{"version":1,"process":"agent","pid":9,"time":"2026-11-05T08:00:00Z","thread":"poller","message":"boom","location":"src/y.rs:2"}"#).unwrap();
        let lifecycle = root.path().join("helper-lifecycle.log");
        std::fs::write(
            &lifecycle,
            "t=1 starting pid=4 version=1.1.0\nt=2 refused:NotProtected pid=4 version=1.1.0\n",
        )
        .unwrap();
        let agent: Arc<dyn AgentPort> = Arc::new(Agent {
            files: Ok(DiagnosticsFiles {
                files: vec![DiagnosticsFile::new(
                    "system/inventory.json",
                    "Operating system and disks",
                    r#"{"host":"sams-mac","owner":"/Users/sam"}"#.into(),
                )],
                notes: vec![],
            }),
            status: Ok(status()),
        });
        let tools: Arc<dyn ToolRunner> = Arc::new(Tools {
            calls: Mutex::new(vec![]),
            answer: |_| ok("state = not running"),
        });
        let mut sections = paddock_diag::builtin::all();
        sections.push(Arc::new(AgentFiles::new(agent.clone())));
        sections.push(Arc::new(HelperReport::new(
            agent,
            tools.clone(),
            HelperHost::Macos,
            Some(lifecycle),
        )));
        sections.push(Arc::new(StartupReport::new(
            tools,
            HelperHost::Macos,
            501,
            root.path().join("Applications/Dyno.app/Contents/MacOS/dyno"),
        )));
        sections.push(Arc::new(InstallReport::new(Some(root.path().join("Applications/Dyno.app")))));
        let redactor = Redactor::for_machine_named(
            &MapVars::new().with("USER", "sam").with("HOME", "/Users/sam"),
            Some("sams-mac.local"),
        );
        let info = AppInfo::new("dyno", "Dyno", "1.1.0", "abc1234");
        let dest = root.path().join("bundle.zip");
        let now = chrono::DateTime::from_timestamp(1_762_333_200, 0).unwrap();
        let saved = paddock_diag::export::write(&info, &d, &sections, &redactor, &dest, now).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().unwrap().to_string())
            .collect();
        eprintln!("{} files in the bundle:\n{}", saved.files, names.join("\n"));
        for want in [
            "manifest.json",
            "system-info.json",
            "logs/dyno.2026-11-05.log",
            "crash-agent.json",
            "system/inventory.json",
            "helper/status.json",
            "helper/os-state.txt",
            "helper/lifecycle.txt",
            "startup/registration.txt",
        ] {
            assert!(names.iter().any(|n| n == want), "{want} missing from {names:?}");
        }
        let mut all = String::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).unwrap();
            let mut t = String::new();
            let _ = std::io::Read::read_to_string(&mut f, &mut t);
            all.push_str(&t);
        }
        for hostile in ["Users/sam", "hunter2", "sams-mac"] {
            assert!(!all.contains(hostile), "{hostile} survived in the bundle");
        }
    }

    #[test]
    fn tools_run_with_timeouts_and_their_output_is_bounded() {
        let big = "x".repeat(50_000);
        let t = tool("/bin/launchctl", &["print", "system/x"]);
        let runner = Tools {
            calls: Mutex::new(vec![]),
            answer: |_| ok(&"x".repeat(50_000)),
        };
        let text = run_report(&runner, "launchd", &t);
        assert!(text.len() < big.len() / 4 && text.contains("== launchd =="), "{}", text.len());
    }
}
