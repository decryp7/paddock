//! The Tauri commands: `dyno` (every engine request, forwarded to the agent) and the commands that open an OS file dialog
//! (the shell owns the windows, the tray popover, About and updates). Arguments are parsed into typed values here, at the
//! edge; the webview is not trusted (it may not stop the agent, and it only reaches the commands in the table of lib.rs).

use crate::dto::CommandError;
use crate::link::AgentLink;
use crate::shell::Shell;
use dyno_core::protocol::{Reply, Request};
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, State, Webview};

/// Parses what the UI sent. Anything the UI is not allowed to ask is refused here.
pub fn parse_request(value: Value) -> Result<Request, CommandError> {
    let request: Request =
        serde_json::from_value(value).map_err(|e| CommandError::bad(format!("The request is not one Dyno understands: {e}")))?;
    match request {
        // Only the app's own shutdown path may stop the agent.
        Request::Shutdown => Err(CommandError::bad("The window may not stop the agent.")),
        // A file is written only where the OS save dialog said (the `save_*` commands), never to a path the webview names:
        // `dyno` is reachable from the tray and dialog windows too.
        Request::SaveInventoryReport { .. } | Request::SaveDiagnostics { .. } | Request::SaveReport { .. } | Request::SaveIssues { .. } => {
            Err(CommandError::bad("Use the Export or Save button: it asks where to save the file."))
        }
        // The widget layouts have their own commands (typed arguments, the popup's values are the app's own).
        Request::WidgetLayout { .. } | Request::EditWidgetLayout { .. } | Request::PopoverValues { .. } => {
            Err(CommandError::bad("Use the widget commands for layouts and the popup's values."))
        }
        r => Ok(r),
    }
}

/// Runs one request against the agent. A `Subscribe` is this window's wish for the live detail; everything else is
/// forwarded. Blocking (it waits for the agent): call it off the async runtime's threads.
pub fn handle(link: &AgentLink, shell: &Shell, window: &str, request: Request) -> Result<Value, CommandError> {
    if let Request::Subscribe { detail } = request {
        link.set_detail(window, detail)?;
        return Ok(Value::Null);
    }
    let reply = link.call(request)?;
    if let Reply::Settings(s) = &reply {
        // Saved settings drive the tray style and the login items; a failed registration is reported to the user.
        shell
            .settings_changed(s, link)
            .map_err(|m| CommandError::new(crate::dto::ErrorCode::AccessDenied, m))?;
    }
    reply
        .into_json()
        .map_err(|e| CommandError::internal(format!("The agent's answer could not be read: {e}")))
}

/// One surface's widget layout (the chooser, the dashboard): the agent's view of it.
#[tauri::command]
pub async fn widget_layout(link: State<'_, Arc<AgentLink>>, surface: dyno_core::widgets::DynoSurface) -> Result<Value, CommandError> {
    let link = link.inner().clone();
    tauri::async_runtime::spawn_blocking(move || to_json(link.call(Request::WidgetLayout { surface })?))
        .await
        .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))?
}

/// An edit of one surface's widget layout. The agent validates the whole result and saves it; a refused edit answers why.
#[tauri::command]
pub async fn edit_widget_layout(link: State<'_, Arc<AgentLink>>, edit: dyno_core::widgets::SurfaceEdit) -> Result<Value, CommandError> {
    let link = link.inner().clone();
    tauri::async_runtime::spawn_blocking(move || to_json(link.call(Request::EditWidgetLayout { edit })?))
        .await
        .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))?
}

/// The tray popup's values now: the first paint of a popup that has just opened (later ones are pushed).
#[tauri::command]
pub async fn popover_values(shell: State<'_, Arc<Shell>>) -> Result<Value, CommandError> {
    serde_json::to_value(shell.popover_snapshot()).map_err(|e| CommandError::internal(format!("The popup values could not be read: {e}")))
}

/// A widget's button. The action is the closed `WidgetActionId`; what it does is `WidgetActionId::effect`.
#[tauri::command]
pub async fn run_widget_action(
    app: AppHandle,
    link: State<'_, Arc<AgentLink>>,
    shell: State<'_, Arc<Shell>>,
    action: dyno_core::widgets::popover_values::WidgetActionId,
) -> Result<Value, CommandError> {
    use dyno_core::widgets::popover_values::Effect;
    use tauri::Emitter;
    let (link, shell) = (link.inner().clone(), shell.inner().clone());
    match action.effect() {
        Effect::Navigate(to) => {
            paddock_window::tauri::reopen(&app);
            app.emit(crate::shell::events::NAVIGATE, to)
                .map_err(|e| CommandError::internal(format!("Dyno could not open that page: {e}")))?;
        }
        Effect::Refresh => shell.popover_changed(),
        // Restart goes to the app's own supervisor, never through the agent that may be the thing that is stuck.
        Effect::RestartAgent => link.retry(),
        Effect::StartBackupCheck => {
            tauri::async_runtime::spawn_blocking(move || {
                link.call(Request::StartJob {
                    spec: dyno_core::jobs::JobSpec::RefreshBackup,
                })
                .map(|_| ())
            })
            .await
            .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))??;
        }
    }
    Ok(Value::Null)
}

fn to_json(reply: Reply) -> Result<Value, CommandError> {
    reply
        .into_json()
        .map_err(|e| CommandError::internal(format!("The agent's answer could not be read: {e}")))
}

/// The path as text for the agent. A path that is not valid Unicode is refused rather than altered: the agent would
/// write to a different file than the one the person chose.
pub fn utf8_path(path: &std::path::Path) -> Result<String, CommandError> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| CommandError::bad("That location has characters Dyno cannot use in a file name. Choose another folder."))
}

/// The folders the person picked in the OS folder dialog this session; the file finder may look only in these.
#[derive(Default)]
pub struct PickedFolders(std::sync::Mutex<Vec<String>>);

/// The most folders remembered.
const MAX_PICKED: usize = 16;

impl PickedFolders {
    /// Remembers a picked folder (its canonical form when it can be resolved).
    pub fn remember(&self, folder: &str) -> String {
        let canon = std::fs::canonicalize(folder)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| folder.to_string());
        let mut list = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        list.retain(|f| f != &canon);
        list.push(canon.clone());
        if list.len() > MAX_PICKED {
            list.remove(0);
        }
        canon
    }

    /// Whether `folder` is one the person picked (compared in canonical form).
    pub fn allows(&self, folder: &str) -> bool {
        let canon = std::fs::canonicalize(folder)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| folder.to_string());
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(&canon)
    }
}

/// "Choose folder..." for the file finder: the OS folder dialog; the answer is the folder (`null` when they cancel) and is
/// the only kind of folder the finder accepts afterwards.
#[tauri::command]
pub async fn pick_folder(app: AppHandle, picked: State<'_, PickedFolders>) -> Result<Value, CommandError> {
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        app.dialog().file().set_title("Choose a folder to look in").blocking_pick_folder()
    })
    .await
    .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))?;
    let Some(chosen) = chosen else { return Ok(Value::Null) };
    let path = chosen
        .into_path()
        .map_err(|e| CommandError::bad(format!("That location cannot be used ({e}).")))?;
    Ok(Value::String(picked.remember(&utf8_path(&path)?)))
}

/// "Export report..." on the Inventory tab: the OS save dialog, then the agent writes the hardware and software report
/// there. `null` when they cancel.
#[tauri::command]
pub async fn save_inventory_report(
    app: AppHandle,
    link: State<'_, Arc<AgentLink>>,
    format: dyno_core::extras::software::InventoryFormat,
) -> Result<Value, CommandError> {
    let link = link.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        let ext = format.extension();
        let Some(chosen) = app
            .dialog()
            .file()
            .set_title("Save the inventory report")
            .set_file_name(format!("dyno-inventory.{ext}"))
            .add_filter("Report", &[ext])
            .blocking_save_file()
        else {
            return Ok(Value::Null);
        };
        let path = chosen
            .into_path()
            .map_err(|e| CommandError::bad(format!("That location cannot be used ({e}).")))?;
        let reply = link.call(Request::SaveInventoryReport {
            path: utf8_path(&path)?,
            format,
        })?;
        reply
            .into_json()
            .map_err(|e| CommandError::internal(format!("The agent's answer could not be read: {e}")))
    })
    .await
    .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))?
}

/// The file finder and a folder scan of the disk map look only in a folder the person picked in the OS dialog
/// (`pick_folder`); a volume scan names a volume the agent itself listed.
fn check_picked(picked: &PickedFolders, request: &Request) -> Result<(), CommandError> {
    let folder = match request {
        Request::StartJob {
            spec: dyno_core::jobs::JobSpec::FindFiles { query },
        } => &query.folder,
        Request::StartDiskScan {
            target: dyno_core::disk::scan::DiskTarget::Folder { path },
        } => path,
        _ => return Ok(()),
    };
    if picked.allows(folder) {
        Ok(())
    } else {
        Err(CommandError::bad("Choose the folder with the Choose folder button first."))
    }
}

/// The one command of the engine contract: `invoke('dyno', { request })`.
#[tauri::command]
pub async fn dyno(
    app: AppHandle,
    webview: Webview,
    link: State<'_, Arc<AgentLink>>,
    shell: State<'_, Arc<Shell>>,
    picked: State<'_, PickedFolders>,
    request: Value,
) -> Result<Value, CommandError> {
    let request = parse_request(request)?;
    let opens_browser = matches!(
        request,
        Request::LookupIp { .. } | Request::WebSearch { .. } | Request::VirusTotalUrl { .. }
    );
    check_picked(&picked, &request)?;
    let (link, shell, label) = (link.inner().clone(), shell.inner().clone(), webview.label().to_string());
    let value = tauri::async_runtime::spawn_blocking(move || handle(&link, &shell, &label, request))
        .await
        .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))??;
    if opens_browser {
        // "Look up IP", "Search the web" and "Search this hash on VirusTotal": the agent built the address from validated input;
        // the default browser opens it on the person's click, Dyno never fetches it and uploads nothing.
        let url = value
            .as_str()
            .ok_or_else(|| CommandError::internal("The agent did not return a web address."))?;
        open_in_browser(&app, url)?;
        return Ok(Value::Null);
    }
    Ok(value)
}

/// The only schemes the shell hands to the OS to open: https (checked again here, whatever the agent built).
pub fn https_only(url: &str) -> bool {
    url.starts_with("https://") && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

fn open_in_browser(app: &AppHandle, url: &str) -> Result<(), CommandError> {
    use tauri_plugin_opener::OpenerExt;
    if !https_only(url) {
        return Err(CommandError::bad("Only https addresses are opened."));
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| {
        CommandError::new(
            crate::dto::ErrorCode::Unsupported,
            format!("Dyno could not open the browser ({e}). Copy the address instead."),
        )
    })
}

/// "Save bundle...": the OS save dialog (so the path is one the person chose, never one the webview supplied), then the
/// agent writes the redacted zip there. `null` when they cancel.
#[tauri::command]
pub async fn save_diagnostics(app: AppHandle, link: State<'_, Arc<AgentLink>>) -> Result<Value, CommandError> {
    let link = link.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        let Some(chosen) = app
            .dialog()
            .file()
            .set_title("Save the diagnostics bundle")
            .set_file_name("dyno-diagnostics.zip")
            .add_filter("Zip archive", &["zip"])
            .blocking_save_file()
        else {
            return Ok(Value::Null);
        };
        let path = chosen
            .into_path()
            .map_err(|e| CommandError::bad(format!("That location cannot be used ({e}).")))?;
        let path = utf8_path(&path)?;
        let reply = link.call(Request::SaveDiagnostics { path })?;
        reply
            .into_json()
            .map_err(|e| CommandError::internal(format!("The agent's answer could not be read: {e}")))
    })
    .await
    .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))?
}

/// "Export" on the Reports page: the OS save dialog (so the path is one the person chose, never one the webview supplied),
/// then the agent writes the week's report there as Markdown or HTML. `null` when they cancel.
#[tauri::command]
pub async fn save_report(
    app: AppHandle,
    link: State<'_, Arc<AgentLink>>,
    format: dyno_core::report::ReportFormat,
    weeks_ago: u32,
) -> Result<Value, CommandError> {
    let link = link.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        use dyno_core::report::ReportFormat;
        use tauri_plugin_dialog::DialogExt;
        let (name, filter) = match format {
            ReportFormat::Markdown => ("dyno-weekly-report.md", ("Markdown", "md")),
            ReportFormat::Html => ("dyno-weekly-report.html", ("Web page", "html")),
        };
        let Some(chosen) = app
            .dialog()
            .file()
            .set_title("Save the weekly report")
            .set_file_name(name)
            .add_filter(filter.0, &[filter.1])
            .blocking_save_file()
        else {
            return Ok(Value::Null);
        };
        let path = chosen
            .into_path()
            .map_err(|e| CommandError::bad(format!("That location cannot be used ({e}).")))?;
        let path = utf8_path(&path)?;
        let reply = link.call(Request::SaveReport { path, format, weeks_ago })?;
        reply
            .into_json()
            .map_err(|e| CommandError::internal(format!("The agent's answer could not be read: {e}")))
    })
    .await
    .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))?
}

/// "Export for Debrief" on the Issues page: the OS save dialog (so the path is one the person chose, never one the webview
/// supplied), then the agent writes the issues of the last `days` days there as JSON lines. `null` when they cancel.
#[tauri::command]
pub async fn save_issues(app: AppHandle, link: State<'_, Arc<AgentLink>>, days: u32) -> Result<Value, CommandError> {
    let link = link.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        let Some(chosen) = app
            .dialog()
            .file()
            .set_title("Save the issues for Debrief")
            .set_file_name("dyno-issues.jsonl")
            .add_filter("JSON lines", &["jsonl"])
            .blocking_save_file()
        else {
            return Ok(Value::Null);
        };
        let path = chosen
            .into_path()
            .map_err(|e| CommandError::bad(format!("That location cannot be used ({e}).")))?;
        let path = utf8_path(&path)?;
        let reply = link.call(Request::SaveIssues { path, days })?;
        reply
            .into_json()
            .map_err(|e| CommandError::internal(format!("The agent's answer could not be read: {e}")))
    })
    .await
    .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_uis_requests_parse_into_typed_values() {
        assert_eq!(parse_request(json!("whySlow")).unwrap(), Request::WhySlow);
        assert!(matches!(
            parse_request(json!({"subscribe": {"detail": "processes"}})).unwrap(),
            Request::Subscribe { .. }
        ));
        assert!(matches!(
            parse_request(json!({"history": {"range": "day", "end": null}})),
            Ok(Request::History { .. }) | Err(_)
        ));
        assert!(matches!(
            parse_request(json!({"activity": {"limit": 5}})).unwrap(),
            Request::Activity { limit: 5 }
        ));
        assert_eq!(parse_request(json!("throttling")).unwrap(), Request::Throttling);
        assert_eq!(parse_request(json!("lifespan")).unwrap(), Request::Lifespan);
        assert_eq!(parse_request(json!("baselines")).unwrap(), Request::Baselines);
    }

    #[test]
    fn the_report_path_and_the_finder_folder_cannot_come_from_the_webview() {
        use serde_json::json;
        // The report is saved only through the dialog command.
        assert!(parse_request(json!({"saveInventoryReport": {"path": "/tmp/x.md", "format": "markdown"}})).is_err());
        // So are the bundle, the weekly report and the issues: `dyno` is reachable from the tray and dialog windows too.
        assert!(parse_request(json!({"saveDiagnostics": {"path": "/tmp/x.zip"}})).is_err());
        assert!(parse_request(json!({"saveReport": {"path": "/tmp/x.md", "format": "markdown", "weeksAgo": 0}})).is_err());
        assert!(parse_request(json!({"saveIssues": {"path": "/tmp/x.jsonl", "days": 7}})).is_err());
        // The finder's request parses, but `dyno` refuses a folder nobody picked.
        let r = parse_request(json!({"startJob": {"spec": {"findFiles": {"query": {"folder": "/tmp", "minBytes": 1}}}}})).unwrap();
        assert!(matches!(
            r,
            Request::StartJob {
                spec: dyno_core::jobs::JobSpec::FindFiles { .. }
            }
        ));
        let picked = PickedFolders::default();
        let dir = std::env::temp_dir();
        let dir = dir.to_string_lossy();
        assert!(!picked.allows(&dir));
        let canon = picked.remember(&dir);
        assert!(picked.allows(&dir) && picked.allows(&canon));
        assert!(!picked.allows("/definitely/not/picked"));
        // Only the most recent folders are remembered.
        for i in 0..(MAX_PICKED + 3) {
            picked.remember(&format!("/picked/{i}"));
        }
        assert!(!picked.allows(&dir));
        assert!(picked.allows(&format!("/picked/{}", MAX_PICKED + 2)));
    }

    #[test]
    fn a_disk_scan_of_a_folder_needs_a_picked_folder_but_a_volume_does_not() {
        use serde_json::json;
        let picked = PickedFolders::default();
        let dir = std::env::temp_dir().to_string_lossy().into_owned();
        let folder = parse_request(json!({"startDiskScan": {"target": {"folder": {"path": dir}}}})).unwrap();
        assert!(check_picked(&picked, &folder).is_err(), "nobody picked it");
        picked.remember(&dir);
        assert!(check_picked(&picked, &folder).is_ok());
        let volume = parse_request(json!({"startDiskScan": {"target": {"volume": {"id": "/"}}}})).unwrap();
        assert!(check_picked(&PickedFolders::default(), &volume).is_ok());
        assert!(check_picked(&PickedFolders::default(), &Request::WhySlow).is_ok());
        let finder = parse_request(json!({"startJob": {"spec": {"findFiles": {"query": {"folder": dir, "minBytes": 1}}}}})).unwrap();
        assert!(check_picked(&PickedFolders::default(), &finder).is_err());
    }

    #[test]
    fn the_extras_requests_have_the_shape_the_ui_sends() {
        use serde_json::json;
        for name in ["gpuProcesses", "maintenance", "backup", "jobs"] {
            assert!(parse_request(json!(name)).is_ok(), "{name}");
        }
        assert!(matches!(
            parse_request(json!({"changes": {"query": {"hours": 24}}})).unwrap(),
            Request::Changes { query } if query.hours == 24
        ));
    }

    #[test]
    fn the_issue_and_unusual_requests_have_the_shape_the_ui_sends() {
        assert!(matches!(
            parse_request(json!({"issues": {"query": {"days": 14}}})).unwrap(),
            Request::Issues { query } if query.days == 14
        ));
        assert!(matches!(
            parse_request(json!({"startJob": {"spec": {"rescanIssues": {"query": {"days": 7}}}}})).unwrap(),
            Request::StartJob {
                spec: dyno_core::jobs::JobSpec::RescanIssues { .. }
            }
        ));
        assert_eq!(parse_request(json!("unusual")).unwrap(), Request::Unusual);
        assert_eq!(parse_request(json!("forgetBaseline")).unwrap(), Request::ForgetBaseline);
        // The ignore and protect actions are gone.
        assert!(parse_request(json!({"unusualAct": {"key": "k", "action": "protect"}})).is_err());
        assert!(matches!(
            parse_request(json!({"startJob": {"spec": {"fileHash": {"source": {"path": "/a"}}}}})).unwrap(),
            Request::StartJob {
                spec: dyno_core::jobs::JobSpec::FileHash { .. }
            }
        ));
        assert!(parse_request(json!({"startJob": {"spec": {"fileHash": {"source": {"pid": 1}}}}})).is_err());
        assert_eq!(
            parse_request(json!({"cancelJob": {"id": 3}})).unwrap(),
            Request::CancelJob { id: 3 }
        );
        assert_eq!(
            parse_request(json!({"jobResult": {"id": 3}})).unwrap(),
            Request::JobResult { id: 3 }
        );
    }

    #[test]
    fn a_save_path_that_is_not_unicode_is_refused_not_altered() {
        assert_eq!(utf8_path(std::path::Path::new("/tmp/report.md")).unwrap(), "/tmp/report.md");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let odd = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/tmp/caf\xe9.md"));
            assert_eq!(utf8_path(odd).unwrap_err().code, crate::dto::ErrorCode::BadRequest);
        }
    }

    #[test]
    fn only_clean_https_addresses_are_opened() {
        assert!(https_only("https://ipinfo.io/8.8.8.8"));
        for bad in [
            "http://x/",
            "file:///etc/passwd",
            "javascript:1",
            "https://x/ y",
            "https://x/\n",
            "ftp://x",
            "",
        ] {
            assert!(!https_only(bad), "{bad:?}");
        }
    }

    #[test]
    fn nonsense_and_forbidden_requests_are_refused_at_the_edge() {
        for bad in [
            json!("nope"),
            json!(42),
            json!({"act": {}}),
            json!({"activity": {"limit": "many"}}),
            json!(null),
        ] {
            assert_eq!(parse_request(bad).unwrap_err().code, crate::dto::ErrorCode::BadRequest);
        }
        let e = parse_request(json!("shutdown")).unwrap_err();
        assert_eq!(e.code, crate::dto::ErrorCode::BadRequest);
        assert!(e.message.contains("may not stop"));
    }
}
