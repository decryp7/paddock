//! The Tauri commands: `dyno` (every engine request, forwarded to the agent) and the commands that open an OS file dialog
//! (the shell owns the windows, the tray popover, About and updates). Arguments are parsed into typed values here, at the
//! edge; the webview is not trusted (it may not stop the agent, and it only reaches the commands in the table of lib.rs).

use crate::dto::CommandError;
use crate::link::AgentLink;
use crate::shell::Shell;
use dyno_core::protocol::{Reply, Request};
use paddock_widgets::{PreviewRequest, Previews};
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
        Request::SaveInventoryReport { .. } | Request::SaveReport { .. } | Request::SaveIssues { .. } => {
            Err(CommandError::bad("Use the Export or Save button: it asks where to save the file."))
        }
        // The agent's report files are unredacted: only Report a problem reads them (Rust to Rust), redacts and previews them.
        Request::DiagnosticsFiles => Err(CommandError::bad(
            "Use Report a problem: it redacts the files and shows them before saving.",
        )),
        // The widget layouts have their own commands (typed arguments, the popup's values are the app's own).
        Request::WidgetLayout { .. } | Request::EditWidgetLayout { .. } | Request::PopoverValues { .. } => {
            Err(CommandError::bad("Use the widget commands for layouts and the popup's values."))
        }
        r => Ok(r),
    }
}

/// The id of the next logged action (ties its line to the request).
static NEXT_ACTION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Runs one request against the agent. A `Subscribe` is this window's wish for the live detail; everything else is
/// forwarded. Blocking (it waits for the agent): call it off the async runtime's threads.
pub fn handle(link: &AgentLink, shell: &Shell, window: &str, request: Request) -> Result<Value, CommandError> {
    if let Request::Subscribe { detail } = request {
        link.set_detail(window, detail)?;
        return Ok(Value::Null);
    }
    let action = crate::breadcrumb::describe(&request);
    let (id, started) = (
        NEXT_ACTION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::Instant::now(),
    );
    let saved_keys = match (&action, &request) {
        (Some(_), Request::SaveSettings { settings }) => {
            crate::breadcrumb::settings_saved(serde_json::to_value(settings).unwrap_or_default())
        }
        _ => Vec::new(),
    };
    let reply = match link.call(request) {
        Ok(reply) => reply,
        Err(e) => {
            if let Some(a) = &action {
                crate::breadcrumb::log_end(a, id, started.elapsed().as_millis(), Err(&format!("{:?}", e.code)), &[]);
            }
            return Err(e);
        }
    };
    if let Some(a) = &action {
        crate::breadcrumb::log_end(a, id, started.elapsed().as_millis(), Ok(()), &saved_keys);
    }
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

/// The previews' copy of a dashboard view the agent sent, so a drag is answered here and never by the agent (ADR 0044 §6).
fn record_view(previews: &Previews, reply: &Reply) {
    if let Reply::WidgetLayout(view) = reply {
        if let dyno_core::widgets::SurfaceView::Dashboard(view) = view.as_ref() {
            if !previews.record::<dyno_core::widgets::DashboardCatalogue>(view) {
                log::warn!(
                    "The dashboard view could not be recorded for previews (revision {}).",
                    view.revision
                );
            }
        }
    }
}

/// One surface's widget layout (the chooser, the dashboard): the agent's view of it.
#[tauri::command]
pub async fn widget_layout(
    link: State<'_, Arc<AgentLink>>,
    previews: State<'_, Arc<Previews>>,
    surface: dyno_core::widgets::DynoSurface,
) -> Result<Value, CommandError> {
    let (link, previews) = (link.inner().clone(), previews.inner().clone());
    tauri::async_runtime::spawn_blocking(move || {
        let reply = link.call(Request::WidgetLayout { surface })?;
        record_view(&previews, &reply);
        to_json(reply)
    })
    .await
    .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))?
}

/// An edit of one surface's widget layout. The agent validates the whole result and saves it; a refused edit answers why.
/// `base_revision` is the revision of the view the edit was computed on: a layout edited meanwhile refuses it.
#[tauri::command]
pub async fn edit_widget_layout(
    link: State<'_, Arc<AgentLink>>,
    previews: State<'_, Arc<Previews>>,
    edit: dyno_core::widgets::SurfaceEdit,
    base_revision: Option<u64>,
) -> Result<Value, CommandError> {
    let (link, previews) = (link.inner().clone(), previews.inner().clone());
    tauri::async_runtime::spawn_blocking(move || {
        let reply = link.call(Request::EditWidgetLayout { edit, base_revision })?;
        record_view(&previews, &reply);
        to_json(reply)
    })
    .await
    .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))?
}

/// What an arrangement gesture on the dashboard would give (a drag or a resize in edit mode): the view of the result,
/// computed here, in the app's process, from the last view the agent sent. Nothing is applied or saved. A revision that
/// is not the recorded one is refused as stale and the webview reloads the view.
#[tauri::command]
pub async fn preview_widget_layout(previews: State<'_, Arc<Previews>>, request: PreviewRequest) -> Result<Value, CommandError> {
    let reply = previews.preview(&request).map_err(|e| CommandError::bad(e.to_string()))?;
    serde_json::to_value(reply).map_err(|e| CommandError::internal(format!("The preview could not be written: {e}")))
}

/// The tray popup has opened and asks for its first paint: it counts as visible from now and its slow widgets are asked for
/// at once; later values are pushed (`dyno://popover-values`) while it stays visible.
#[tauri::command]
pub async fn popover_values(shell: State<'_, Arc<Shell>>) -> Result<Value, CommandError> {
    let shell = shell.inner().clone();
    serde_json::to_value(shell.popover_opened()).map_err(|e| CommandError::internal(format!("The popup values could not be read: {e}")))
}

/// The popup's values as known now (the body of `popover_snapshot`): no visibility change, no ask, no broadcast.
pub fn snapshot_value<R: tauri::Runtime>(shell: &crate::shell::ShellOf<R>) -> Result<Value, CommandError> {
    serde_json::to_value(shell.popover_snapshot()).map_err(|e| CommandError::internal(format!("The popup values could not be read: {e}")))
}

/// The Settings preview's values (the body of `popover_preview`): the slow widgets are asked of the agent, the popup is
/// not marked visible. Blocking.
pub fn preview_value<R: tauri::Runtime>(shell: &crate::shell::ShellOf<R>) -> Result<Value, CommandError> {
    serde_json::to_value(shell.popover_preview()).map_err(|e| CommandError::internal(format!("The popup values could not be read: {e}")))
}

/// The popup's values as known now, for a view that does not open the popup.
#[tauri::command]
pub async fn popover_snapshot(shell: State<'_, Arc<Shell>>) -> Result<Value, CommandError> {
    snapshot_value(shell.inner())
}

/// The popup's values for the Settings preview, with the slow widgets asked of the agent now. It does not mark the popup
/// visible (only `popover_values`, the popup's own first paint, does).
#[tauri::command]
pub async fn popover_preview(shell: State<'_, Arc<Shell>>) -> Result<Value, CommandError> {
    let shell = shell.inner().clone();
    tauri::async_runtime::spawn_blocking(move || preview_value(&shell))
        .await
        .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))?
}

/// Readings of metrics from the newest live update, with the tone of each against its thresholds: the dashboard's widgets
/// and the previews of the menu-bar and popup choosers. The answer is in the order of the queries.
#[tauri::command]
pub async fn metric_readings(shell: State<'_, Arc<Shell>>, queries: Vec<dyno_core::widgets::ReadingQuery>) -> Result<Value, CommandError> {
    serde_json::to_value(shell.metric_readings(&queries))
        .map_err(|e| CommandError::internal(format!("The readings could not be read: {e}")))
}

/// Whether the agent keeps measuring after a plain Quit: the login service is installed and supervises it, asked of the OS now
/// (it can take a moment, so off the UI thread). The menus show "Quit Dyno (keeps measuring)" for it.
#[tauri::command]
pub async fn quit_keeps_measuring() -> Result<bool, CommandError> {
    tauri::async_runtime::spawn_blocking(|| crate::quit::keeps_measuring(crate::login::service_probe()()))
        .await
        .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))
}

/// "Quit and stop measuring": returns at once; a worker stops the agent, checks with the system that it is gone and quits.
#[tauri::command]
pub async fn quit_and_stop_measuring(app: AppHandle) -> Result<(), CommandError> {
    crate::quit::quit_and_stop(&app);
    Ok(())
}

/// For each kind the chooser can add: a ready default widget and the metrics the engine accepts for it.
#[tauri::command]
pub async fn widget_choices() -> Result<Value, CommandError> {
    serde_json::to_value(dyno_core::widgets::widget_choices(
        dyno_core::widgets::TrayPlatform::current(),
        crate::tray::native_icon_px(),
    ))
    .map_err(|e| CommandError::internal(format!("The choices could not be read: {e}")))
}

/// The page or Settings section a widget asked the main window to open, once: a window that was still being created when
/// the button was pressed asks for it as it starts.
#[tauri::command]
pub async fn take_navigation(shell: State<'_, Arc<Shell>>) -> Result<Value, CommandError> {
    serde_json::to_value(shell.take_navigation()).map_err(|e| CommandError::internal(format!("The navigation could not be read: {e}")))
}

/// The app's real ports for a widget's button.
struct LivePorts {
    app: AppHandle,
    link: Arc<AgentLink>,
    shell: Arc<Shell>,
}

impl crate::actions::ActionPorts for LivePorts {
    fn navigate(&self, to: dyno_core::widgets::popover_values::Navigate) -> Result<(), CommandError> {
        // The main window is shown (or created) first; the request waits in the shell until the window takes it.
        self.shell.navigate(to);
        paddock_window::tauri::reopen(&self.app);
        Ok(())
    }
    fn refresh(&self) {
        self.shell.popover_changed();
    }
    fn start_backup_check(&self) -> Result<(), CommandError> {
        self.link
            .call(Request::StartJob {
                spec: dyno_core::jobs::JobSpec::RefreshBackup,
            })
            .map(|_| ())
    }
    fn restart_agent(&self) {
        self.link.retry();
    }
}

/// A widget's button. The action is the closed `WidgetActionId`; what it does is `WidgetActionId::effect`, run through
/// [`crate::actions::run`].
#[tauri::command]
pub async fn run_widget_action(
    app: AppHandle,
    link: State<'_, Arc<AgentLink>>,
    shell: State<'_, Arc<Shell>>,
    action: dyno_core::widgets::popover_values::WidgetActionId,
) -> Result<Value, CommandError> {
    let ports = LivePorts {
        app,
        link: link.inner().clone(),
        shell: shell.inner().clone(),
    };
    tauri::async_runtime::spawn_blocking(move || crate::actions::run(action.effect(), &ports))
        .await
        .map_err(|e| CommandError::internal(format!("The request was interrupted: {e}")))??;
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

/// The taskbar settings of Windows (`ms-settings:taskbar`), where "Other system tray icons" switches Dyno's icons on: the hint
/// on the first start and for each new menu-bar icon links here. The target is fixed, never taken from the page; other
/// systems have no such page and refuse.
#[tauri::command]
pub async fn open_taskbar_settings(app: AppHandle) -> Result<Value, CommandError> {
    use tauri_plugin_opener::OpenerExt;
    if !cfg!(windows) {
        return Err(CommandError::bad("The taskbar settings page exists on Windows only."));
    }
    app.opener().open_url(TASKBAR_SETTINGS_URL, None::<&str>).map_err(|e| {
        CommandError::internal(format!(
            "The Windows taskbar settings could not be opened ({e}). Open Settings > Personalization > Taskbar."
        ))
    })?;
    Ok(Value::Null)
}

/// Whether the desktop shows Dyno's tray icons (the engine's `TrayAvailability`; the changes arrive as `dyno://tray`). Settings ›
/// Menu bar shows the engine's note when it does not.
#[tauri::command]
pub async fn tray_availability(app: AppHandle) -> Result<Value, CommandError> {
    serde_json::to_value(crate::tray_host::current(&app))
        .map_err(|e| CommandError::internal(format!("The tray state could not be read: {e}")))
}

/// What [`open_taskbar_settings`] opens.
pub const TASKBAR_SETTINGS_URL: &str = "ms-settings:taskbar";

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
            ..
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

/// "Export" on the Benchmarks page: the OS save dialog (so the path is one the person chose, never one the webview supplied), then
/// the agent writes the chosen runs there as JSON or CSV. `null` when they cancel.
#[tauri::command]
pub async fn save_benchmarks(
    app: AppHandle,
    link: State<'_, Arc<AgentLink>>,
    format: dyno_core::bench::export::ExportFormat,
    ids: Vec<String>,
) -> Result<Value, CommandError> {
    let link = link.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        use dyno_core::bench::export::ExportFormat;
        use tauri_plugin_dialog::DialogExt;
        let (name, filter) = match format {
            ExportFormat::Json => ("dyno-benchmarks.json", ("JSON", "json")),
            ExportFormat::Csv => ("dyno-benchmarks.csv", ("Spreadsheet (CSV)", "csv")),
        };
        let Some(chosen) = app
            .dialog()
            .file()
            .set_title("Save the benchmark results")
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
        let reply = link.call(Request::SaveBenchmarks { path, format, ids })?;
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
        assert!(
            parse_request(json!("diagnosticsFiles")).is_err(),
            "the unredacted files are for Report a problem only"
        );
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
