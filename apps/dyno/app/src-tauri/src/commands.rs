//! The Tauri commands: `dyno` (every engine request, forwarded to the agent), `app_info`, and the tray window's two
//! buttons. Arguments are parsed into typed values here, at the edge; the webview is not trusted (it may not stop the
//! agent, and it only reaches the commands below).

use crate::dto::{AppInfo, CommandError};
use crate::link::AgentLink;
use crate::shell::Shell;
use crate::tray;
use crate::window;
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
        // A watched folder is added only through `add_watched_folder`, which asks the person in the OS folder dialog.
        Request::AddWatchedFolder { .. } => Err(CommandError::bad("A folder to watch is chosen in the folder dialog.")),
        // The report goes only where the OS save dialog said (`save_inventory_report`), never to a path the webview names.
        Request::SaveInventoryReport { .. } => Err(CommandError::bad("Use the Export button: it asks where to save the report.")),
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
    Ok(Value::String(picked.remember(&path.to_string_lossy())))
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
            path: path.to_string_lossy().into_owned(),
            format,
        })?;
        reply
            .into_json()
            .map_err(|e| CommandError::internal(format!("The agent's answer could not be read: {e}")))
    })
    .await
    .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))?
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
    // The file finder looks only in a folder the person picked in the OS dialog (`pick_folder`).
    if let Request::FindFiles { query } = &request {
        if !picked.allows(&query.folder) {
            return Err(CommandError::bad("Choose the folder with the Choose folder button first."));
        }
    }
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

/// `appInfo()`.
#[tauri::command]
pub async fn app_info(app: AppHandle) -> Result<AppInfo, CommandError> {
    Ok(crate::updater::app_info(&app))
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
        let path = path.to_string_lossy().into_owned();
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
        let path = path.to_string_lossy().into_owned();
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
        let path = path.to_string_lossy().into_owned();
        let reply = link.call(Request::SaveIssues { path, days })?;
        reply
            .into_json()
            .map_err(|e| CommandError::internal(format!("The agent's answer could not be read: {e}")))
    })
    .await
    .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))?
}

/// "Add folder" on the Watched folders page: the OS folder dialog (so the folder is one the person chose, never one the
/// webview named), then the agent adds it switched off. `null` when they cancel.
#[tauri::command]
pub async fn add_watched_folder(
    app: AppHandle,
    link: State<'_, Arc<AgentLink>>,
    template: Option<String>,
    keep_for_secs: u32,
    recursive: bool,
) -> Result<Value, CommandError> {
    let link = link.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        let Some(chosen) = app.dialog().file().set_title("Choose the folder to watch").blocking_pick_folder() else {
            return Ok(Value::Null);
        };
        let path = chosen
            .into_path()
            .map_err(|e| CommandError::bad(format!("That location cannot be used ({e}).")))?;
        let path = path.to_string_lossy().into_owned();
        let reply = link.call(Request::AddWatchedFolder {
            path,
            template,
            keep_for_secs,
            recursive,
        })?;
        reply
            .into_json()
            .map_err(|e| CommandError::internal(format!("The agent's answer could not be read: {e}")))
    })
    .await
    .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))?
}

/// The tray dropdown's "Open Dyno".
#[tauri::command]
pub async fn tray_open_main(app: AppHandle) -> Result<(), CommandError> {
    window::show_main(&app);
    tray::popover_blurred(&app, tray::DROPDOWN);
    Ok(())
}

/// The popover page reports the height of its content (logical pixels); the window follows, within limits. Returns the
/// height that was applied.
#[tauri::command]
pub async fn popover_resize(webview: Webview, app: AppHandle, height: f64) -> Result<f64, CommandError> {
    crate::popover::resize(&app, webview.label(), height).map_err(CommandError::bad)
}

/// Escape in a popover page (or its own close button).
#[tauri::command]
pub async fn popover_close(webview: Webview, app: AppHandle) -> Result<(), CommandError> {
    if !crate::popover::is_popover_label(webview.label()) {
        return Err(CommandError::bad("Only a popover can be closed this way."));
    }
    crate::popover::close(&app, webview.label());
    Ok(())
}

/// The tray dropdown's "Quit".
#[tauri::command]
pub async fn tray_quit(app: AppHandle) -> Result<(), CommandError> {
    tray::quit(&app);
    Ok(())
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
        // The finder's request parses, but `dyno` refuses a folder nobody picked.
        let r = parse_request(json!({"findFiles": {"query": {"folder": "/tmp", "minBytes": 1}}})).unwrap();
        assert!(matches!(r, Request::FindFiles { .. }));
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
    fn the_extras_requests_have_the_shape_the_ui_sends() {
        use serde_json::json;
        for name in [
            "sleepBlockers",
            "bootAnalysis",
            "appUsage",
            "gpuProcesses",
            "softwareInventory",
            "maintenance",
            "backup",
            "refreshBackup",
        ] {
            assert!(parse_request(json!(name)).is_ok(), "{name}");
        }
        assert!(matches!(
            parse_request(json!({"changes": {"query": {"hours": 24}}})).unwrap(),
            Request::Changes { query } if query.hours == 24
        ));
        assert!(matches!(
            parse_request(json!({"removeFoundFiles": {"scanId": "s", "ids": ["a"], "mode": "quarantine"}})).unwrap(),
            Request::RemoveFoundFiles { .. }
        ));
        assert!(parse_request(json!({"removeFoundFiles": {"scanId": "s", "ids": ["a"], "mode": "shred"}})).is_err());
    }

    #[test]
    fn the_issue_and_unusual_requests_have_the_shape_the_ui_sends() {
        assert!(matches!(
            parse_request(json!({"issues": {"query": {"days": 14}}})).unwrap(),
            Request::Issues { query } if query.days == 14
        ));
        assert!(matches!(
            parse_request(json!({"rescanIssues": {"query": {"days": 7}}})).unwrap(),
            Request::RescanIssues { .. }
        ));
        assert_eq!(
            parse_request(json!({"unusual": {"includeIgnored": true}})).unwrap(),
            Request::Unusual { include_ignored: true }
        );
        assert!(matches!(
            parse_request(json!({"unusualAct": {"key": "/a/b", "action": "protect"}})).unwrap(),
            Request::UnusualAct {
                action: dyno_core::unusual::baseline::UnusualAction::Protect,
                ..
            }
        ));
        assert_eq!(parse_request(json!("forgetBaseline")).unwrap(), Request::ForgetBaseline);
        // A forged action is refused at the edge.
        assert!(parse_request(json!({"unusualAct": {"key": "k", "action": "delete"}})).is_err());
    }

    #[test]
    fn a_watched_folder_cannot_be_added_by_the_webview_but_the_other_watch_requests_parse() {
        let add = json!({"addWatchedFolder": {"path": "/x", "template": null, "keepForSecs": 600, "recursive": false}});
        assert!(parse_request(add).unwrap_err().message.contains("folder dialog"));
        assert!(matches!(parse_request(json!("watchedFolders")).unwrap(), Request::WatchedFolders));
        assert!(matches!(
            parse_request(json!({"setWatchedFolder": {"id": "w-1", "keepForSecs": 600, "enabled": true, "recursive": null}})).unwrap(),
            Request::SetWatchedFolder { enabled: Some(true), .. }
        ));
        assert!(matches!(
            parse_request(json!({"previewWatchedFolder": {"id": "w-1"}})).unwrap(),
            Request::PreviewWatchedFolder { .. }
        ));
        assert!(matches!(
            parse_request(json!({"webSearch": {"query": "q"}})).unwrap(),
            Request::WebSearch { .. }
        ));
        assert!(matches!(
            parse_request(json!({"fileHash": {"source": {"path": "/a"}}})).unwrap(),
            Request::FileHash { .. }
        ));
        assert!(parse_request(json!({"fileHash": {"source": {"pid": 1}}})).is_err());
        assert!(matches!(
            parse_request(json!({"startupOverview": {"filter": {"hideOs": true, "hideSigned": false}}})).unwrap(),
            Request::StartupOverview { .. }
        ));
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
