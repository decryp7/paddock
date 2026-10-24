//! Paddock Shell Demo: the shell demo's UI (`ui/projects/shell-demo`) on the real `paddock-shell` plugin, with a small
//! sample engine (plan docs/plans/workbench.md §10, S18):
//!
//! - [`engine`]: two health services (the demo engine, whose Restart works, and the search index, whose Reset fails)
//!   and two sample jobs (an import with three phases on their own scales and a step that cannot be cancelled, and a
//!   quiet job that reports nothing until it is cancelled);
//! - [`sections`]: the demo's settings sections (General and Storage: one field of every kind between them);
//! - [`commands`]: the demo's own commands ([`COMMANDS`]); the shell's are the plugin's;
//! - [`tray`]: the tray icon and its popover (`index.html#/tray`).
//!
//! [`shell`] is the plugin with all of it, for the app ([`run`]) and for the tests on Tauri's mock runtime ([`Place`]).
//! It is also the starter of new apps: `node scripts/new-app.mjs <id> "<Name>"` copies it (apps/shell-demo/README.md).

/// The sample background agent (plan X4): the `shell-demo-agent` crate.
pub use shell_demo_agent as agent;
pub mod commands;
pub mod engine;
pub mod mcp;
pub mod packs;
pub mod sections;
pub mod tray;

use paddock_diag::AppInfo;
use paddock_diag::tauri::Setup;
use paddock_env::{AppDirs, AppId, MapVars, Os};
use paddock_health::notify::os::{FakeToaster, OsNotifier, SharedOsSettings, Toaster};
use paddock_health::tauri::native::PluginToaster;
use paddock_log::{LogConfig, LogHandle, Logging};
use paddock_shell::{AppIdentity, Builder};
use paddock_updater::tauri::UpdatesBuilder;
use paddock_window::tauri::{FixedPrefs, WindowConfig, Windows};
use paddock_window::{OnClose, OpenFilesIn};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{Manager, Runtime};

/// The app's id (the shell's `AppIdentity`, its folders and its log target).
pub const APP_ID: &str = "shell-demo";
/// The product name (`tauri.conf.json` `productName`, the windows, the menus, About).
pub const APP_NAME: &str = "Paddock Shell Demo";
/// The app's version: Cargo.toml's (tauri.conf.json has none, so the bundles carry this one too).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Where the releases are (About's "Release notes", Updates' "Open the releases page").
pub const RELEASES_PAGE: &str = "https://dev.decryptology.net/decryp7/paddock/releases";
/// The demo's generated dialog contracts and manifest, written by the UI's specs from its registry.
pub const DIALOG_CONTRACTS: &str =
    include_str!("../../../../ui/projects/shell-demo/dialog-contracts.json");
pub const MANIFEST: &str = include_str!("../../../../ui/projects/shell-demo/paddock-manifest.json");

paddock_commands::command_table! {
    /// The demo's own commands (the shell's are the plugin's, `plugin:paddock-shell|<command>`).
    pub static COMMANDS, macro handler;
    /// Starts a sample job and returns its id at once (the tray popover starts one too).
    Mutation [MAIN | TRAY] => commands::demo_job_start,
    /// A service of the demo fails, as a supervisor reports it.
    Mutation [MAIN] => commands::demo_break,
    /// Forgets the greetings.
    Mutation [MAIN] => commands::demo_reset,
    /// What the engine inspector shows.
    Query [MAIN | DIALOG] => commands::demo_status,
    /// The tray popover's repair of a service (the shell's repair; `health_repair` is not open to the tray).
    Mutation [TRAY] => commands::demo_tray_repair,
    /// What the tray popover shows.
    Query [TRAY] => commands::demo_tray_state,
}

/// Where the app runs: installed (the user's folders, real ticks) or in a test (nothing outside `dir`, no background
/// threads that reach the network, no native menu off the main thread, instant ticks).
#[derive(Debug, Clone)]
pub enum Place {
    Installed,
    Test { dir: PathBuf },
}

impl Place {
    fn tick(&self) -> Duration {
        match self {
            Place::Installed => engine::TICK,
            Place::Test { .. } => Duration::ZERO,
        }
    }
}

/// The app's log, installed once at start by [`run`]; Report a Problem reports it and the Logging section tunes its level.
static LOG: OnceLock<LogHandle> = OnceLock::new();

/// The app's folders: the user's (installed) or `dir` (a test's, through the `SHELL_DEMO_HOME` override).
fn dirs(
    test_dir: Option<&std::path::Path>,
) -> Result<AppDirs, Box<dyn std::error::Error + Send + Sync>> {
    let id = AppId::new(APP_ID, APP_NAME)?;
    Ok(match test_dir {
        Some(dir) => AppDirs::resolve(
            &id,
            Os::current(),
            &MapVars::new().with(&id.home_var(), dir),
            None,
            true,
        )?,
        None => AppDirs::for_app(&id)?,
    })
}

/// Starts the app's log in the user's log folder at the level and limits saved in the settings (the level changes later
/// without a restart, from the Logging page). A log that cannot start is only noted: the app runs without one.
fn install_log() {
    let started = dirs(None).map_err(|e| e.to_string()).and_then(|dirs| {
        let id = AppId::new(APP_ID, APP_NAME).map_err(|e| e.to_string())?;
        let saved = paddock_diag::logging::stored(&dirs.config().join(paddock_settings::FILE_NAME));
        let config = LogConfig::new(&id, "app", VERSION)
            .with_dirs(&dirs)
            .with_settings(saved);
        Logging::new(config).install().map_err(|e| e.to_string())
    });
    match started {
        Ok(handle) => {
            let _ = LOG.set(handle);
        }
        Err(why) => eprintln!("{APP_NAME}: the log could not start ({why})"),
    }
}

/// The agent's manager: installed, the real OS next to this app's executable; in tests, the recording fake (rule 7).
fn agent_manager(
    test_dir: Option<&std::path::Path>,
) -> Result<paddock_service::Manager, Box<dyn std::error::Error + Send + Sync>> {
    let data = dirs(test_dir)?.data().to_path_buf();
    match test_dir {
        None => {
            let exe = std::env::current_exe()?;
            let program = exe
                .parent()
                .ok_or("the app's folder cannot be found")?
                .join(agent::spec().program_file(cfg!(windows)));
            Ok(agent::manager(
                &data,
                &program,
                paddock_service::backend::Side::App,
                VERSION,
            )?)
        }
        Some(_) => {
            use paddock_service::backend::{Target, WindowsRun};
            let sys = paddock_service::testing::FakeSystem::new();
            sys.put_program("C:\\Demo\\shell-demo-agent.exe");
            let backend = WindowsRun::new(
                agent::spec(),
                Target::exe("C:\\Demo\\shell-demo-agent.exe"),
                Box::new(sys),
            );
            let beat_dir = data.clone();
            Ok(paddock_service::Manager::new(
                APP_NAME,
                Box::new(backend),
                &data,
                agent::LABEL,
                VERSION,
                move || agent::answers(&beat_dir),
            ))
        }
    }
}

/// The shell plugin with every part of the demo.
pub fn shell<R: Runtime>(place: Place) -> Builder<R> {
    shell_with_log(place, LOG.get().cloned())
}

/// As [`shell`] with the log to report and tune (a test passes a scoped one).
pub fn shell_with_log<R: Runtime>(place: Place, log: Option<LogHandle>) -> Builder<R> {
    let tick = place.tick();
    let installed = matches!(place, Place::Installed);
    let os_settings = SharedOsSettings::default();
    let mut builder = Builder::new(AppIdentity::new(APP_ID, APP_NAME, VERSION))
        .app_commands(&COMMANDS)
        .dialog_contracts(DIALOG_CONTRACTS)
        .health({
            let mut services = engine::services(tick);
            services.push(paddock_health::ServiceSpec::new(
                agent::HEALTH_ID,
                "the demo agent",
            ));
            services
        })
        .os_notifications(os_settings.clone())
        .health_setup(move |app, setup| {
            // Installed: the real notification plugin. Tests: a toaster that shows nothing (rule 7).
            let toaster: Arc<dyn Toaster> = if installed {
                Arc::new(PluginToaster::new(app.clone()))
            } else {
                Arc::new(FakeToaster::default())
            };
            let setup = setup.os_notifier(Arc::new(OsNotifier::new(os_settings, toaster)));
            if installed {
                setup.os_digest_every(Duration::from_secs(30))
            } else {
                setup
            }
        })
        .section(sections::general())
        .section(sections::storage())
        .setup(move |app| {
            app.manage(engine::Engine::new(tick));
            Ok(())
        });
    let test_dir = match &place {
        Place::Installed => None,
        Place::Test { dir } => Some(dir.clone()),
    };
    let updates_dir = test_dir.clone();
    let agent_dir = test_dir.clone();
    // The sample background agent (plan X4): the Background service page, its health entry and Start at login.
    builder = builder.service(move |_| {
        let setup = paddock_service::tauri::Setup::new(agent_manager(agent_dir.as_deref())?)
            .health(agent::HEALTH_ID);
        Ok(paddock_shell::ServicePart::new(match agent_dir {
            None => setup,
            Some(_) => setup.checks(Duration::ZERO, Duration::ZERO, 0),
        }))
    });
    // The first-run set-up checklist (plan X19): the service and Start at login steps of the agent above.
    builder = builder.first_run(|_| paddock_shell::FirstRun::new());
    let files_dir = test_dir.clone();
    // User files (plan X31): path grants, the native pickers, the recent lists in the config folder, reveal and open.
    builder = builder.files(move |_| {
        let dirs = dirs(files_dir.as_deref())?;
        Ok(paddock_files::Files::for_app(
            &dirs,
            &[
                paddock_files::recent::FILES,
                paddock_files::recent::PROJECTS,
            ],
        ))
    });
    let storage_dir = test_dir.clone();
    // What the demo keeps (plan X32): its cache and logs, cleared only inside the app's own folders, as jobs with a confirmation.
    builder = builder.storage(move |_| {
        use paddock_storage::{Category, Kind, Storage};
        let dirs = dirs(storage_dir.as_deref())?;
        let started = std::time::SystemTime::now();
        let categories = vec![
            Category::new("cache", "Cache", Kind::Cache)
                .folder(dirs.cache())
                .entries(|_| None)
                .note("The demo rebuilds it when it needs it."),
            Category::new("logs", "Logs", Kind::Data)
                .folder(dirs.logs())
                .entries(move |p| paddock_storage::from_this_run(p, Some(started), "written"))
                .note("Logs written in this session are kept."),
        ];
        Ok(Storage::new(std::sync::Arc::new(categories)).within(&dirs))
    });
    let packs_dir = test_dir.clone();
    // The sample knowledge packs (plan X15/X23): the Knowledge packs page and the hot reload of the user's packs folder.
    builder = builder.packs(move |app| Ok(packs::setup(app, &dirs(packs_dir.as_deref())?)));
    let mcp_dir = test_dir.clone();
    // The AI access settings (plan X22): the MCP server's tools, the clients' keys and the activity log behind Settings > AI access.
    builder = builder.mcp(move |_, emit| {
        let dirs = dirs(mcp_dir.as_deref())?;
        Ok(mcp::control(
            &dirs,
            APP_NAME,
            VERSION,
            mcp_dir.is_none(),
            emit,
        ))
    });
    let report_dir = test_dir.clone();
    // Report a problem (plan X2) and the Logging section: the app's folders and log, the shell's built-in sections.
    builder = builder.diagnostics(move |_| {
        let setup = Setup::new(
            AppInfo::new(
                APP_ID,
                APP_NAME,
                VERSION,
                option_env!("SHELL_DEMO_COMMIT").unwrap_or(""),
            ),
            dirs(report_dir.as_deref())?,
        );
        Ok(match log {
            Some(handle) => setup.log(handle),
            None => setup,
        })
    });
    builder = builder.updater(move |_| updater(updates_dir));
    builder = builder.windows(move |app| windows(app, test_dir.is_none()));
    match place {
        Place::Installed => builder,
        Place::Test { .. } => builder.in_memory().native_menu(false),
    }
}

/// Updates, About and What's New with this app's changelog. Without the CI's updater key the build cannot update
/// itself and says so ("Updates are not available in this build"); the demo is released unsigned for now.
fn updater(test_dir: Option<PathBuf>) -> UpdatesBuilder {
    let b = UpdatesBuilder::new(APP_NAME)
        .public_key(option_env!("TAURI_UPDATER_PUBKEY"))
        .releases_page(RELEASES_PAGE)
        .homepage("https://dev.decryptology.net/decryp7/paddock")
        .changelog(include_str!("../../CHANGELOG.md"))
        .legal(
            "licence-mit",
            "MIT licence",
            include_str!("../../../../LICENSE-MIT"),
        )
        .legal(
            "licence-apache",
            "Apache licence 2.0",
            include_str!("../../../../LICENSE-APACHE"),
        )
        .build_info(
            option_env!("PADDOCK_BUILD_COMMIT"),
            option_env!("PADDOCK_BUILD_DATE"),
        );
    match test_dir {
        None => b,
        Some(dir) => b
            .dirs(dir.join("config"), dir.join("cache"))
            .background(false),
    }
}

/// The window layer: the main window from `tauri.conf.json` (`"create": false`), closing the last window quits (the
/// tray only opens the popover), bounds remembered in the data folder when installed.
fn windows<R: Runtime>(app: &tauri::AppHandle<R>, installed: bool) -> Windows<R> {
    let b = Windows::builder(
        WindowConfig::new(APP_NAME)
            .size(1200.0, 780.0)
            .min_size(860.0, 560.0),
    )
    .prefs(Arc::new(FixedPrefs::new(
        OnClose::Quit,
        OpenFilesIn::NewWindow,
    )));
    match app.path().app_data_dir() {
        Ok(dir) if installed => b.bounds_file(dir.join("window-state.json")).build(),
        _ => b.build(),
    }
}

/// The demo's own commands behind its guard: window classes, the per-dialog contracts (the inspector may call
/// `demo_status`, ADR 0009) and request limits.
pub fn invoke_handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static
{
    paddock_commands::tauri::Guard::new(&COMMANDS)
        .dialog_grants(paddock_dialogs::tauri::grant_of)
        .wrap(handler!())
}

/// The app: the shell plugin, the demo's guarded commands, the tray and the main window.
pub fn run() {
    install_log();
    let cwd = std::env::current_dir().ok();
    let launch = paddock_window::launch::parse_args(std::env::args().skip(1), cwd.as_deref(), &[]);
    let mut shell = shell::<tauri::Wry>(Place::Installed);
    // `app_relaunch` (a language change in Settings): the session is written, then the whole app starts again.
    match paddock_startup::tauri::Startup::builder()
        .relaunch(|app| app.restart())
        .build()
    {
        Ok(startup) => shell = shell.startup(startup),
        Err(e) => log::error!("The relaunch hook is not installed: {e}"),
    }
    let shell = shell.setup(move |app| {
        tray::create(app);
        paddock_window::tauri::start(app, &launch)?;
        Ok(())
    });
    let builder = tauri::Builder::default()
        // The native save dialog of Report a Problem (the shell's `native-dialogs`).
        .plugin(tauri_plugin_dialog::init())
        // The OS notification plugin behind the shell's `PluginToaster`.
        .plugin(tauri_plugin_notification::init())
        .plugin(shell.build())
        .invoke_handler(invoke_handler());
    // A web content process termination hook exists on macOS and iOS only (Tauri 2 has none for WebView2 or WebKitGTK).
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let builder =
        builder.on_web_content_process_terminate(paddock_startup::tauri::web_content_terminated);
    let result = builder.run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("{APP_NAME} could not start: {e}");
        std::process::exit(1);
    }
}
