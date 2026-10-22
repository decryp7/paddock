//! The Tauri commands the UI calls (`app/src/app/core/tauri-backend.ts`). Each is a thin edge: it parses the
//! webview's arguments into typed values (never trusting the UI), asks the service through [`ServiceClient`] or does
//! an app-side job (credential store, Gitea API, diagnostics file, OS integration), off the UI thread.

use crate::dto::{
    ActivityReply, AppUpdateInfo, AppUpdateStatusDto, CommandError, CommandErrorCode, ConnectionState, DiskReply, MetricsReply,
    RegisterRequest, ServiceInstall, SettingsReply, StatusReply,
};
use crate::events::Latest;
use crate::service_client::{ClientError, ServiceClient};
use crate::{credentials, diagnostics, gitea, service_install, updater};
use pitcrew_core::config::{check_server_url, Config, RunnerConfig};
use pitcrew_core::distribution::RunnerKind;
use pitcrew_core::gitea_api::{job_days, RunDetails};
use pitcrew_core::names::RunnerName;
use pitcrew_core::protocol::{DiagnosticsSection, Request, Response, Secret};
use pitcrew_core::version::RunnerVersion;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

/// The service client, managed by Tauri.
pub type SharedClient = Arc<dyn ServiceClient>;

impl From<ClientError> for CommandError {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::Service(s) => s.into(),
            ClientError::Unavailable(_) => CommandError::new(CommandErrorCode::ServiceUnavailable, e.to_string()),
            ClientError::Mismatch(m) => CommandError::new(CommandErrorCode::ProtocolMismatch, m),
            ClientError::Protocol(_) => CommandError::internal(e.to_string()),
        }
    }
}

fn name(s: &str) -> Result<RunnerName, CommandError> {
    RunnerName::parse(s).map_err(|e| CommandError::bad(e.to_string()))
}

fn version(s: &str) -> Result<RunnerVersion, CommandError> {
    RunnerVersion::parse(s).map_err(|e| CommandError::bad(e.to_string()))
}

/// Sends a request and turns service errors into command errors.
pub fn ask(client: &dyn ServiceClient, req: Request) -> Result<Response, CommandError> {
    match client.request(req)? {
        Response::Error(e) => Err(e.into()),
        r => Ok(r),
    }
}

fn expect_ok(r: Response) -> Result<(), CommandError> {
    match r {
        Response::Ok => Ok(()),
        other => Err(unexpected(other)),
    }
}

fn unexpected(r: Response) -> CommandError {
    CommandError::internal(format!(
        "the service gave an unexpected answer ({})",
        serde_json::to_string(&r).unwrap_or_default().chars().take(80).collect::<String>()
    ))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, CommandError> + Send + 'static) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| CommandError::internal(format!("the request failed: {e}")))?
}

// ---- pure command logic (tested without Tauri) ----------------------------------------------------------------------

/// Every runner's status.
pub fn status(client: &dyn ServiceClient) -> Result<StatusReply, CommandError> {
    match ask(client, Request::Status)? {
        Response::Status { runners, service } => Ok(StatusReply { runners, service }),
        other => Err(unexpected(other)),
    }
}

/// A runner action by name.
pub fn action_request(runner: &str, action: &str) -> Result<Request, CommandError> {
    let runner = name(runner)?;
    Ok(match action {
        "start" => Request::Start { runner },
        "stop" => Request::Stop { runner },
        "restart" => Request::Restart { runner },
        "drain" => Request::Drain { runner },
        "resume" => Request::Resume { runner },
        "checkUpdate" => Request::CheckUpdate { runner },
        "applyUpdate" => Request::ApplyUpdate { runner },
        "cancelUpdate" => Request::CancelUpdate { runner },
        other => return Err(CommandError::bad(format!("unknown action {other:?}"))),
    })
}

/// The runner configuration the registration wizard asks for.
pub fn runner_config(req: &RegisterRequest, data_dir: &str) -> Result<RunnerConfig, CommandError> {
    let n = name(&req.runner)?;
    let dir = match req.dir.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(data_dir).join("runners").join(n.as_str()),
    };
    Ok(RunnerConfig {
        name: n,
        kind: RunnerKind::GiteaRunner,
        dir,
        config_file: None,
        labels: req.labels.iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
        session: req.session,
        logged_off: req.logged_off,
        server_url: Some(check_server_url(&req.server_url).map_err(CommandError::bad)?),
        enabled: true,
        binary_path: None,
        update: None,
        watchdog: None,
        cleanup: None,
    })
}

/// Registers a runner: adds it (unless it exists) and asks the service to register it with the token.
pub fn register(client: &dyn ServiceClient, req: RegisterRequest) -> Result<(), CommandError> {
    let token = Secret::new(&req.token).map_err(CommandError::bad)?;
    let st = status(client)?;
    let cfg = runner_config(&req, &st.service.data_dir)?;
    let runner = cfg.name.clone();
    if !st.runners.iter().any(|r| r.name == runner) {
        expect_ok(ask(client, Request::AddRunner { runner: cfg.clone() })?)?;
    }
    let display_name = req
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_string);
    expect_ok(ask(
        client,
        Request::Register {
            runner,
            server_url: req.server_url.clone(),
            token: token.clone(),
            display_name,
        },
    )?)?;
    if req.remember_token {
        credentials::set(credentials::REGISTRATION_SERVICE, &req.server_url, token.expose()).map_err(CommandError::internal)?;
    }
    Ok(())
}

// ---- Tauri commands -------------------------------------------------------------------------------------------------

/// `platform_name`.
#[tauri::command]
pub fn platform_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// `runner_status`.
#[tauri::command]
pub async fn runner_status(client: State<'_, SharedClient>) -> Result<StatusReply, CommandError> {
    let c = client.inner().clone();
    blocking(move || status(c.as_ref())).await
}

/// `connection_state`.
#[tauri::command]
pub fn connection_state(latest: State<'_, Latest>) -> ConnectionState {
    latest.connection()
}

/// `runner_action`.
#[tauri::command]
pub async fn runner_action(client: State<'_, SharedClient>, runner: String, action: String) -> Result<(), CommandError> {
    let req = action_request(&runner, &action)?;
    let c = client.inner().clone();
    blocking(move || expect_ok(ask(c.as_ref(), req)?)).await
}

/// `skip_version`.
#[tauri::command]
pub async fn skip_version(client: State<'_, SharedClient>, runner: String, version: String) -> Result<(), CommandError> {
    let req = Request::SkipVersion {
        runner: name(&runner)?,
        version: self::version(&version)?,
    };
    let c = client.inner().clone();
    blocking(move || expect_ok(ask(c.as_ref(), req)?)).await
}

/// `pin_version`.
#[tauri::command]
pub async fn pin_version(client: State<'_, SharedClient>, runner: String, version: Option<String>) -> Result<(), CommandError> {
    let req = Request::PinVersion {
        runner: name(&runner)?,
        version: version.as_deref().map(self::version).transpose()?,
    };
    let c = client.inner().clone();
    blocking(move || expect_ok(ask(c.as_ref(), req)?)).await
}

/// `runner_logs`.
#[tauri::command]
pub async fn runner_logs(
    client: State<'_, SharedClient>,
    runner: String,
    after: Option<u64>,
    limit: u32,
) -> Result<Vec<pitcrew_core::logs::LogLine>, CommandError> {
    let req = Request::Logs {
        runner: name(&runner)?,
        after,
        limit: limit.min(2000),
    };
    let c = client.inner().clone();
    blocking(move || match ask(c.as_ref(), req)? {
        Response::Logs { lines } => Ok(lines),
        other => Err(unexpected(other)),
    })
    .await
}

/// `search_logs`.
#[tauri::command]
pub async fn search_logs(
    client: State<'_, SharedClient>,
    runner: String,
    query: String,
    limit: u32,
) -> Result<Vec<pitcrew_core::logs::LogLine>, CommandError> {
    if query.chars().count() > pitcrew_core::logs::MAX_QUERY_LEN {
        return Err(CommandError::bad("the search is too long"));
    }
    let req = Request::SearchLogs {
        runner: name(&runner)?,
        query,
        limit: limit.min(1000),
    };
    let c = client.inner().clone();
    blocking(move || match ask(c.as_ref(), req)? {
        Response::Logs { lines } => Ok(lines),
        other => Err(unexpected(other)),
    })
    .await
}

/// `runner_activity`: the history summary, job outcomes per day (Gitea API with the read token) and events.
#[tauri::command]
pub async fn runner_activity<R: Runtime>(
    app: AppHandle<R>,
    client: State<'_, SharedClient>,
    runner: String,
    days: u32,
    utc_offset_minutes: i32,
) -> Result<ActivityReply, CommandError> {
    let n = name(&runner)?;
    let c = client.inner().clone();
    let cache = app
        .path()
        .app_data_dir()
        .map(|d| d.join("job-outcomes.json"))
        .map_err(|e| CommandError::internal(e.to_string()))?;
    let server = app
        .state::<Latest>()
        .status()
        .and_then(|s| s.runners.into_iter().find(|r| r.name == n).and_then(|r| r.server_url));
    blocking(move || {
        let (summary, events) = match ask(
            c.as_ref(),
            Request::History {
                runner: n,
                days,
                utc_offset_minutes,
            },
        )? {
            Response::History { summary, events } => (summary, events),
            other => return Err(unexpected(other)),
        };
        let token = server.as_deref().and_then(|s| credentials::get(credentials::API_SERVICE, s));
        let details = gitea::outcomes(&cache, &summary.recent_jobs, token.as_deref());
        let job_days = job_days(&summary.recent_jobs, &details, summary.to, days, utc_offset_minutes);
        Ok(ActivityReply {
            summary,
            job_days,
            events,
            has_api_token: token.is_some(),
        })
    })
    .await
}

/// `machine_metrics`.
#[tauri::command]
pub async fn machine_metrics(
    client: State<'_, SharedClient>,
    runner: Option<String>,
    range_hours: u32,
) -> Result<MetricsReply, CommandError> {
    let req = Request::Metrics {
        runner: runner.as_deref().map(name).transpose()?,
        range_hours,
    };
    let c = client.inner().clone();
    blocking(move || match ask(c.as_ref(), req)? {
        Response::Metrics { machine, folder, usage } => Ok(MetricsReply { machine, folder, usage }),
        other => Err(unexpected(other)),
    })
    .await
}

/// `runner_disk`.
#[tauri::command]
pub async fn runner_disk(client: State<'_, SharedClient>, runner: String) -> Result<DiskReply, CommandError> {
    let req = Request::Disk { runner: name(&runner)? };
    let c = client.inner().clone();
    blocking(move || match ask(c.as_ref(), req)? {
        Response::Disk {
            scan,
            preview,
            last_cleanup,
            policy,
        } => Ok(DiskReply {
            scan,
            preview,
            last_cleanup,
            policy,
        }),
        other => Err(unexpected(other)),
    })
    .await
}

/// `clean_disk`.
#[tauri::command]
pub async fn clean_disk(
    client: State<'_, SharedClient>,
    runner: String,
) -> Result<pitcrew_core::housekeeping::CleanupReport, CommandError> {
    let req = Request::CleanDisk { runner: name(&runner)? };
    let c = client.inner().clone();
    blocking(move || match ask(c.as_ref(), req)? {
        Response::Cleaned { report } => Ok(report),
        other => Err(unexpected(other)),
    })
    .await
}

/// `run_details`: the current job's run from the Gitea API (needs the read token).
#[tauri::command]
pub async fn run_details(latest: State<'_, Latest>, runner: String) -> Result<Option<RunDetails>, CommandError> {
    let n = name(&runner)?;
    let Some(r) = latest.status().and_then(|s| s.runners.into_iter().find(|r| r.name == n)) else {
        return Ok(None);
    };
    let Some(job) = r.current_job else { return Ok(None) };
    let server = job.info.server.clone().or(r.server_url);
    let Some(token) = server.as_deref().and_then(|s| credentials::get(credentials::API_SERVICE, s)) else {
        return Ok(None);
    };
    blocking(move || gitea::run_details(&job.info, &token).map_err(|e| CommandError::new(CommandErrorCode::ServiceUnavailable, e))).await
}

/// `get_settings`.
#[tauri::command]
pub async fn get_settings(client: State<'_, SharedClient>) -> Result<SettingsReply, CommandError> {
    let c = client.inner().clone();
    blocking(move || match ask(c.as_ref(), Request::GetSettings)? {
        Response::Settings {
            config,
            problems,
            read_only,
        } => Ok(SettingsReply {
            config,
            problems,
            read_only,
        }),
        other => Err(unexpected(other)),
    })
    .await
}

/// `save_settings` (the service validates it like a file it loads).
#[tauri::command]
pub async fn save_settings(client: State<'_, SharedClient>, config: Config) -> Result<(), CommandError> {
    let c = client.inner().clone();
    blocking(move || expect_ok(ask(c.as_ref(), Request::SaveSettings { config })?)).await
}

/// `add_runner`.
#[tauri::command]
pub async fn add_runner(client: State<'_, SharedClient>, runner: RunnerConfig) -> Result<(), CommandError> {
    let c = client.inner().clone();
    blocking(move || expect_ok(ask(c.as_ref(), Request::AddRunner { runner })?)).await
}

/// `remove_runner`.
#[tauri::command]
pub async fn remove_runner(client: State<'_, SharedClient>, runner: String) -> Result<(), CommandError> {
    let req = Request::RemoveRunner { runner: name(&runner)? };
    let c = client.inner().clone();
    blocking(move || expect_ok(ask(c.as_ref(), req)?)).await
}

/// `register_runner`.
#[tauri::command]
pub async fn register_runner(client: State<'_, SharedClient>, request: RegisterRequest) -> Result<(), CommandError> {
    let c = client.inner().clone();
    blocking(move || register(c.as_ref(), request)).await
}

/// `has_api_token`.
#[tauri::command]
pub async fn has_api_token(server_url: String) -> Result<bool, CommandError> {
    blocking(move || Ok(credentials::get(credentials::API_SERVICE, &server_url).is_some())).await
}

/// `set_api_token`.
#[tauri::command]
pub async fn set_api_token(server_url: String, token: String) -> Result<(), CommandError> {
    blocking(move || credentials::set(credentials::API_SERVICE, &server_url, &token).map_err(CommandError::bad)).await
}

/// `clear_api_token`.
#[tauri::command]
pub async fn clear_api_token(server_url: String) -> Result<(), CommandError> {
    blocking(move || credentials::clear(credentials::API_SERVICE, &server_url).map_err(CommandError::bad)).await
}

/// `service_install_state`.
#[tauri::command]
pub async fn service_install_state() -> Result<ServiceInstall, CommandError> {
    blocking(|| Ok(service_install::state())).await
}

/// `install_service` (UAC / Login Items approval).
#[tauri::command]
pub async fn install_service() -> Result<(), CommandError> {
    blocking(service_install::install).await
}

/// `uninstall_service`.
#[tauri::command]
pub async fn uninstall_service() -> Result<(), CommandError> {
    blocking(service_install::uninstall).await
}

/// `app_update_status`: whether this build can update itself.
#[tauri::command]
pub fn app_update_status<R: Runtime>(app: AppHandle<R>) -> AppUpdateStatusDto {
    updater::status(&app)
}

/// `check_app_update`.
#[tauri::command]
pub async fn check_app_update<R: Runtime>(app: AppHandle<R>) -> Result<Option<AppUpdateInfo>, CommandError> {
    updater::check(&app).await
}

/// `install_app_update`.
#[tauri::command]
pub async fn install_app_update<R: Runtime>(app: AppHandle<R>) -> Result<(), CommandError> {
    updater::install(&app).await
}

/// `get_autostart`.
#[tauri::command]
pub fn get_autostart<R: Runtime>(app: AppHandle<R>) -> Result<bool, CommandError> {
    app.autolaunch().is_enabled().map_err(|e| CommandError::internal(e.to_string()))
}

/// `set_autostart`.
#[tauri::command]
pub fn set_autostart<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Result<(), CommandError> {
    let a = app.autolaunch();
    if enabled { a.enable() } else { a.disable() }.map_err(|e| CommandError::internal(e.to_string()))
}

/// `export_diagnostics`: writes the redacted bundle to the Downloads folder and returns its path.
#[tauri::command]
pub async fn export_diagnostics<R: Runtime>(app: AppHandle<R>, client: State<'_, SharedClient>) -> Result<String, CommandError> {
    let dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|e| CommandError::internal(e.to_string()))?;
    let version = app.package_info().version.to_string();
    let connection = format!("{:?}", app.state::<Latest>().connection());
    let c = client.inner().clone();
    blocking(move || {
        let mut sections = match c.request(Request::Diagnostics) {
            Ok(Response::Diagnostics { sections }) => sections,
            Ok(other) => vec![DiagnosticsSection {
                name: "service-error.txt".into(),
                text: format!("{other:?}"),
            }],
            Err(e) => vec![DiagnosticsSection {
                name: "service-error.txt".into(),
                text: e.to_string(),
            }],
        };
        sections.push(DiagnosticsSection {
            name: "app.txt".into(),
            text: format!("Pitcrew app {version}\nplatform: {}\nconnection: {connection}\n", platform_name()),
        });
        let stamp = pitcrew_core::time::format_rfc3339(pitcrew_core::time::Timestamp(
            u64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis(),
            )
            .unwrap_or(0),
        ))
        .replace([':', '.'], "-");
        diagnostics::write_bundle(&dir, &stamp, &sections)
            .map(|p| p.display().to_string())
            .map_err(CommandError::internal)
    })
    .await
}

/// `open_url`: http(s) links only (e.g. a run's page).
#[tauri::command]
pub fn open_url<R: Runtime>(app: AppHandle<R>, url: String) -> Result<(), CommandError> {
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.len() > 2048 || url.chars().any(char::is_control) {
        return Err(CommandError::bad("only http(s) links can be opened"));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| CommandError::internal(e.to_string()))
}

/// `reveal_logs`: shows the runner's log file in the file manager.
#[tauri::command]
pub fn reveal_logs<R: Runtime>(app: AppHandle<R>, runner: String) -> Result<(), CommandError> {
    let n = name(&runner)?;
    let r = app
        .state::<Latest>()
        .status()
        .and_then(|s| s.runners.into_iter().find(|r| r.name == n))
        .ok_or_else(|| CommandError::new(CommandErrorCode::UnknownRunner, format!("no runner is named {n}")))?;
    let log = PathBuf::from(r.dir).join("logs").join(pitcrew_core::layout::LOG_FILE);
    app.opener()
        .reveal_item_in_dir(log)
        .map_err(|e| CommandError::internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitcrew_core::protocol::{ErrorCode, ServiceError, ServiceInfo};
    use std::sync::Mutex;

    type Answer = Box<dyn Fn(&Request) -> Result<Response, ClientError> + Send + Sync>;

    struct Fake {
        sent: Mutex<Vec<Request>>,
        answer: Answer,
    }

    impl ServiceClient for Fake {
        fn request(&self, r: Request) -> Result<Response, ClientError> {
            let a = (self.answer)(&r);
            self.sent.lock().unwrap().push(r);
            a
        }
    }

    fn info() -> ServiceInfo {
        ServiceInfo {
            version: "0.1.0".into(),
            started_at: pitcrew_core::time::Timestamp(0),
            data_dir: "/data".into(),
            config_problems: vec![],
            read_only: false,
            machine: None,
            own_cpu_permille: 0,
            own_mem_bytes: 0,
        }
    }

    fn req(token: &str, remember: bool) -> RegisterRequest {
        serde_json::from_value(serde_json::json!({
            "runner": "mac-mini", "serverUrl": "https://git.example.com", "token": token, "displayName": " ",
            "labels": ["macos-latest", " "], "dir": null, "session": "interactive", "loggedOff": "wait", "rememberToken": remember
        }))
        .unwrap()
    }

    #[test]
    fn actions_are_parsed_and_unknown_ones_refused() {
        assert!(matches!(action_request("a", "drain"), Ok(Request::Drain { .. })));
        for a in ["start", "stop", "restart", "resume", "checkUpdate", "applyUpdate", "cancelUpdate"] {
            assert!(action_request("a", a).is_ok());
        }
        assert_eq!(action_request("a", "rm -rf").unwrap_err().code, CommandErrorCode::BadRequest);
        assert_eq!(action_request("../x", "start").unwrap_err().code, CommandErrorCode::BadRequest);
    }

    #[test]
    fn registration_adds_then_registers_without_storing_the_token() {
        let fake = Fake {
            sent: Mutex::new(vec![]),
            answer: Box::new(|r| {
                Ok(match r {
                    Request::Status => Response::Status {
                        runners: vec![],
                        service: info(),
                    },
                    _ => Response::Ok,
                })
            }),
        };
        register(&fake, req("tok123", false)).unwrap();
        let sent = fake.sent.lock().unwrap();
        let Request::AddRunner { runner } = &sent[1] else {
            panic!("{sent:?}")
        };
        assert_eq!(runner.dir, PathBuf::from("/data").join("runners").join("mac-mini"));
        assert_eq!(runner.labels, vec!["macos-latest".to_string()]);
        let Request::Register { display_name, token, .. } = &sent[2] else {
            panic!()
        };
        assert_eq!(display_name, &None);
        assert_eq!(token.expose(), "tok123");
        assert!(register(&fake, req("bad token", false)).is_err());
    }

    #[test]
    fn errors_are_typed_for_the_ui() {
        let unavailable = Fake {
            sent: Mutex::new(vec![]),
            answer: Box::new(|_| Err(ClientError::Unavailable("no pipe".into()))),
        };
        assert_eq!(status(&unavailable).unwrap_err().code, CommandErrorCode::ServiceUnavailable);
        let refused = Fake {
            sent: Mutex::new(vec![]),
            answer: Box::new(|_| Ok(Response::Error(ServiceError::new(ErrorCode::Busy, "later")))),
        };
        assert_eq!(status(&refused).unwrap_err().code, CommandErrorCode::Busy);
        let odd = Fake {
            sent: Mutex::new(vec![]),
            answer: Box::new(|_| Ok(Response::Ok)),
        };
        assert_eq!(status(&odd).unwrap_err().code, CommandErrorCode::Internal);
        assert_eq!(
            CommandError::from(ClientError::Mismatch("m".into())).code,
            CommandErrorCode::ProtocolMismatch
        );
        assert_eq!(
            CommandError::from(ClientError::Protocol("p".into())).code,
            CommandErrorCode::Internal
        );
        assert!(matches!(platform_name(), "windows" | "macos" | "linux"));
        assert!(version("x").is_err());
    }
}
