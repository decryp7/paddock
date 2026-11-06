//! The Dyno app: tray icons, window and tray popover over the Dyno agent (ADR 0002), on the Paddock app shell (ADR 0012).
//! The shell plugin owns the frame around Dyno: windows, the tray popover window, the exit with its watchdog, dialogs,
//! notifications, service health, jobs, settings, About and updates. This crate adds Dyno's own commands ([`COMMANDS`]),
//! its service ([`health`]) and the link to the agent: [`link::AgentLink`] finds (or starts) the agent, forwards the
//! webview's requests and turns the agent's pushes into events, and [`tray`] draws the icons from them. It measures
//! nothing itself.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod actions;
pub mod breadcrumb;
pub mod commands;
pub mod crash_reports;
pub mod dialogs;
pub mod dto;
pub mod first_data;
pub mod health;
pub mod link;
pub mod logging;
pub mod login;
pub mod login_item;
pub mod mcp;
pub mod mcp_readings;
pub mod per_machine;
pub mod popover;
pub mod power_menu;
pub mod quit;
pub mod report;
pub mod shell;
pub mod supervise;
pub mod tray;
pub mod tray_away;
pub mod tray_host;
pub mod tray_identity;
pub mod tray_look;
pub mod tray_model;
pub mod tray_platform;
#[cfg(target_os = "macos")]
pub mod tray_position;
pub mod tray_shown;
pub mod tray_start;
#[cfg(target_os = "macos")]
pub mod tray_text;
pub mod wol_menu;

use link::{AgentLink, ProcessLauncher};
use paddock_shell::{AppIdentity, Builder};
use paddock_updater::tauri::UpdatesBuilder;
use paddock_window::tauri::{FixedPrefs, WindowConfig, Windows};
use paddock_window::{OnClose, OpenFilesIn};
use std::sync::Arc;
use tauri::{Manager, Runtime};

/// The app's id (the shell's `AppIdentity`, its folders and its log target).
pub const APP_ID: &str = dyno_agent::logging::APP_ID;
/// The product name (`tauri.conf.json` `productName`, the windows, the menus, About).
pub const APP_NAME: &str = dyno_agent::logging::APP_NAME;
/// The app's version: the workspace's (`tauri.conf.json` repeats it for the bundles).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Where the releases are (About's "Release notes", Updates' "Open the releases page").
pub const RELEASES_PAGE: &str = "https://dev.decryptology.net/decryp7/paddock/releases?q=dyno-v";
/// The feed the updater reads.
pub const UPDATE_FEED: &str = "https://dev.decryptology.net/decryp7/paddock/releases/download/dyno-latest/latest.json";
/// The manifest of the UI's contributions, compared by the tests.
pub const MANIFEST: &str = include_str!("../../paddock-manifest.json");

paddock_commands::command_table! {
    /// Dyno's own commands (the shell's are the plugin's, `plugin:paddock-shell|<command>`). Windows that are not `main`
    /// (the settings window, the tray popover) call only what is listed for them.
    pub static COMMANDS, macro handler;
    /// Every engine request, forwarded to the agent (`invoke('dyno', { request })`): parsed into a typed request here,
    /// refused when it is one the window may not ask. The agent re-validates.
    Mutation [MAIN | TRAY | DIALOG] => commands::dyno,
    /// "Export" on the Benchmarks page: the OS save dialog, then the agent writes the chosen runs as JSON or CSV.
    Mutation [MAIN] => commands::save_benchmarks,
    /// "Export" on the Reports page: the OS save dialog, then the agent writes the week's report.
    Mutation [MAIN] => commands::save_report,
    /// "Export for Debrief" on the Issues page.
    Mutation [MAIN] => commands::save_issues,
    /// "Export report..." on the Inventory tab.
    Mutation [MAIN] => commands::save_inventory_report,
    /// "Choose folder..." for the file finder: the OS folder dialog.
    Mutation [MAIN] => commands::open_taskbar_settings,
    /// Whether the desktop shows Dyno's tray icons: Settings › Menu bar shows the note when it does not.
    Query [MAIN | DIALOG] => commands::tray_availability,
    Mutation [MAIN] => commands::pick_folder,
    /// One surface's widget layout, with its catalogue, built-in presets and notes (family ADR 0035). The tray popup reads its
    /// own layout to draw its widgets.
    Query [MAIN | TRAY | DIALOG] => commands::widget_layout,
    /// Edits one surface's widget layout in the agent; answers the new layout, or why it was refused.
    Mutation [MAIN | DIALOG] => commands::edit_widget_layout,
    /// What an arrangement gesture on the dashboard would give, answered in this process from the last view the agent sent; applies nothing.
    Query [MAIN] => commands::preview_widget_layout,
    /// The tray popup's values now, for the popup that has just opened (the live ones arrive as `dyno://popover-values`); the Settings window's preview reads it too.
    Query [MAIN | TRAY | DIALOG] => commands::popover_values,
    /// A button of a widget: open a page, retry, restart the agent.
    Mutation [MAIN | TRAY] => commands::run_widget_action,
    /// The popup's values as known now, without opening it: the Settings preview.
    Query [MAIN | DIALOG] => commands::popover_snapshot,
    /// The popup's values for the Settings preview with the slow widgets asked of the agent; never marks the popup visible.
    Query [MAIN | DIALOG] => commands::popover_preview,
    /// Readings (value, text, scale and tone) of metrics from the newest live update: the dashboard and the previews.
    Query [MAIN | DIALOG] => commands::metric_readings,
    /// The kinds the choosers can add, each with a default widget and the metrics the engine accepts for it.
    Query [MAIN | DIALOG] => commands::widget_choices,
    /// The page or Settings section a widget asked the main window to open (once).
    Mutation [MAIN] => commands::take_navigation,
    /// Whether the agent keeps measuring after a plain Quit (a login service supervises it): the words of Quit.
    Query [MAIN | TRAY] => commands::quit_keeps_measuring,
    /// Quit and stop measuring: stops the agent (service job or the agent itself), checks it is gone, quits the app.
    Mutation [MAIN | TRAY] => commands::quit_and_stop_measuring,
}

/// Where the app runs: installed (the user's folders, real ticks) or in a test (nothing outside `dir`, no native menu
/// off the main thread, no update checks).
#[derive(Debug, Clone)]
pub enum Place {
    /// The installed app.
    Installed,
    /// A test: the updater and settings keep to `dir`.
    Test {
        /// The only folder the test touches.
        dir: std::path::PathBuf,
    },
}

/// The start-up watchdog with a visible question, for the installed app only (a test place has no real folders and no dialog: the shell's
/// default watchdog has no asker, so nothing is ever shown). The question offers Save a problem report… (written without the stuck window).
///
/// When it can appear (it used to be only logged, so this is new). Only for a window the user can see (a hidden one, started at login, is
/// never asked about). Two cases:
/// - A page that never became ready: reloaded after `Config::ready_timeout` (10 s), and still silent `Config::ask_timeout` (60 s) after the
///   reload, so never earlier than [`first_question_after`] (70 s) after it began loading; hidden time does not count.
/// - A page that was ready and stopped answering pings (a content process stopped by a limiter while the app keeps ticking): marked after
///   about 10 s for the dialog code, but asked about only after 60 s of silence, once at a time, and a question counts even when the page
///   comes back before it is answered.
///
/// Each question doubles the next wait (120, 240, 480 s) until the page has been healthy for ten minutes. While a report is being written
/// from the question it is not asked again. An app process that itself was paused is covered by `pause_gap` (clocks restart after a gap
/// of more than 3 s between two ticks), a stopped content process by the long waits.
fn watchdog<R: Runtime>(place: &Place) -> Option<Result<paddock_startup::tauri::Startup<R>, paddock_startup::ConfigError>> {
    matches!(place, Place::Installed).then(|| {
        paddock_startup::tauri::Startup::<R>::builder()
            .native_dialogs()
            .report(|app, _label| {
                let info = paddock_diag::AppInfo::new(APP_ID, APP_NAME, VERSION, option_env!("PADDOCK_BUILD_COMMIT").unwrap_or(""));
                report::save_report_from_question(app, &info)
            })
            .build()
    })
}

/// The earliest a question can follow the start of a page load: the reload after the first timeout, then the longer wait.
pub fn first_question_after(config: &paddock_startup::Config) -> std::time::Duration {
    config.ready_timeout + config.ask_timeout
}

/// The shell plugin with Dyno's parts.
pub fn shell<R: Runtime>(place: Place, hub: &health::Hub<R>, pixels: Arc<dyn shell::TrayPixels<R>>) -> Builder<R> {
    let attach = hub.clone();
    let agent_hub = hub.clone();
    let installed = matches!(place, Place::Installed);
    let dirs = match &place {
        Place::Installed => dyno_agent::paths::Dirs::current(),
        Place::Test { dir } => dyno_agent::paths::Dirs::at(dir.join("data")),
    };
    let mut builder = Builder::new(AppIdentity::new(APP_ID, APP_NAME, VERSION))
        // "Quit and stop measuring" from the macOS menu bar with no window open does its work (the shell would only reopen the window).
        .menu_action_without_window(quit::QUIT_STOP_COMMAND, |app| quit::quit_and_stop(app))
        .app_commands(&COMMANDS)
        .dialogs(&[&dialogs::DIALOGS])
        .health(hub.services())
        .setup(move |app| {
            attach.attach(app);
            Ok(())
        })
        // The agent link: state only, no window (the shell opens the tray and the window after every plugin, ADR 0029).
        .setup(move |app| {
            agent_setup(app, &agent_hub, dirs, pixels, installed)?;
            Ok(())
        });
    let test_dir = match &place {
        Place::Installed => None,
        Place::Test { dir } => Some(dir.clone()),
    };
    let updates_dir = test_dir.clone();
    builder = builder.updater(move |app| updater(updates_dir, app));
    // Report a problem with the logs of both processes, working with the agent down (the shell registers the Logging settings
    // section on the log handle, so a support case can raise the level to Debug).
    // Installed only: a test never resolves the real folders (ER-14).
    if let Some(startup) = watchdog::<R>(&place) {
        match startup {
            Ok(startup) => builder = builder.startup(startup),
            Err(e) => log::error!("The start-up watchdog could not be configured: {e}"),
        }
    }
    if diagnostics_wanted(&place) {
        builder = builder.diagnostics(|app| diagnostics_setup(app));
    }
    // AI access (ADR 0007, 0032 D7): Dyno's four tools, off until the user allows them in Settings > AI access. The tools ask the agent
    // through the shell's link; the settings and the key list live in the config folder (a test place has its own, ER-14).
    let mcp_dir = test_dir.clone();
    builder = builder.mcp(move |app, emit| {
        let id = paddock_env::AppId::new(APP_ID, APP_NAME)?;
        let dirs = match &mcp_dir {
            Some(dir) => paddock_env::AppDirs::resolve(
                &id,
                paddock_env::Os::current(),
                &paddock_env::MapVars::new().with(&id.home_var(), dir),
                None,
                true,
            )?,
            None => paddock_env::AppDirs::for_app(&id)?,
        };
        let calls = Arc::new(mcp::ShellCalls(app.clone()));
        Ok(mcp::control(&dirs, APP_NAME, VERSION, emit, calls))
    });
    builder = builder.windows(move |app| windows(app, test_dir.is_none()));
    match place {
        Place::Installed => builder,
        Place::Test { .. } => builder.in_memory().native_menu(false),
    }
}

/// Whether Report a problem is set up: only for the installed app. A test place never resolves the real folders (ER-14).
fn diagnostics_wanted(place: &Place) -> bool {
    matches!(place, Place::Installed)
}

/// Report a problem's setup: paddock-diag's built-in sections (system facts, the shell's settings, the crash markers, the dated logs
/// of both processes, which work with the agent down because they are read from disk); the log handle when the installed app has one.
fn diagnostics_setup<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<paddock_diag::tauri::Setup, Box<dyn std::error::Error + Send + Sync>> {
    let id = paddock_env::AppId::new(APP_ID, APP_NAME)?;
    let dirs = paddock_env::AppDirs::for_app(&id)?;
    let info = paddock_diag::AppInfo::new(APP_ID, APP_NAME, VERSION, option_env!("PADDOCK_BUILD_COMMIT").unwrap_or(""));
    let mut setup = paddock_diag::tauri::Setup::new(info, dirs)
        .settings_file(paddock_shell::logging::SHELL_SETTINGS_FILE)
        .section(Arc::new(crash_reports::CrashReports::new(crash_reports::default_folders())));
    for section in report::sections(app) {
        setup = setup.section(section);
    }
    if let Some(handle) = logging::handle() {
        setup = setup.log(handle);
    }
    Ok(setup)
}

/// The legal documents About lists, compiled into the binary: the third-party notices (generated by `scripts/notices.sh dyno`
/// from the Rust crates and npm packages Dyno ships; CI fails when they are stale) and Dyno's own two licences.
const LEGAL: [(&str, &str, &str); 3] = [
    (
        "notices",
        "Third-party notices",
        include_str!("../resources/THIRD_PARTY_NOTICES.md"),
    ),
    ("licence-mit", "MIT licence", include_str!("../../../LICENSE-MIT")),
    ("licence-apache", "Apache licence 2.0", include_str!("../../../LICENSE-APACHE")),
];

/// Updates, About and What's New. Without the CI's updater key the build cannot update itself and says so ("Updates
/// are not available in this build").
///
/// The installer is the per-machine hand-off on Windows and `tauri-plugin-updater` (registered in [`run`], with the same key
/// the updater verifies with) elsewhere: without one an update is found but never installed.
fn updater<R: Runtime>(test_dir: Option<std::path::PathBuf>, app: &tauri::AppHandle<R>) -> UpdatesBuilder {
    let key = option_env!("TAURI_UPDATER_PUBKEY");
    let b = LEGAL
        .iter()
        .fold(UpdatesBuilder::new(APP_NAME), |b, (id, title, text)| b.legal(id, title, *text));
    let b = b
        .channel("stable", "Stable", UPDATE_FEED)
        .public_key(key)
        .releases_page(RELEASES_PAGE)
        .homepage("https://dev.decryptology.net/decryp7/paddock")
        .changelog(include_str!("../../../CHANGELOG.md"))
        .build_info(option_env!("PADDOCK_BUILD_COMMIT"), option_env!("PADDOCK_BUILD_DATE"));
    let b = install_with(b, app, key.unwrap_or_default());
    match test_dir {
        None => b,
        Some(dir) => b.dirs(dir.join("config"), dir.join("cache")).background(false),
    }
}

/// Windows installs per machine (ADR 0053): Install hands the verified download to `paddock-setup.exe apply-update` in the
/// install folder, elevated through one Windows prompt; nothing installs when Dyno quits. At start the updater waits, bounded,
/// for another user's installer. Other systems install through `tauri-plugin-updater`.
#[cfg(windows)]
fn install_with<R: Runtime>(b: UpdatesBuilder, app: &tauri::AppHandle<R>, _key: &str) -> UpdatesBuilder {
    use paddock_updater::{ElevatedHandoff, InstallScope};
    let dir = per_machine::install_dir();
    // No hand-off (and so no way to install from the app) unless the folder passes the Program Files rule: the elevated program is
    // never found through a relative name or a folder an ordinary user can change.
    let handoff = dir
        .clone()
        .ok_or_else(|| "the folder of the running program is unknown".to_string())
        .and_then(|d| {
            ElevatedHandoff::new(
                d,
                Arc::new(paddock_updater::tauri::ShellElevator),
                Arc::new(per_machine::SetupFacts),
            )
        });
    let b = match handoff {
        Ok(h) => b.installer(Arc::new(h)),
        Err(why) => {
            logging::info(&format!("updates cannot be installed from this copy: {why}"));
            b
        }
    };
    b.install_scope(InstallScope::PerMachine, dir, Some(per_machine::administrator()))
        .installer_lock(Arc::new(per_machine::SetupLock::new(&app.config().identifier)))
}

#[cfg(not(windows))]
fn install_with<R: Runtime>(b: UpdatesBuilder, app: &tauri::AppHandle<R>, key: &str) -> UpdatesBuilder {
    b.installer(Arc::new(paddock_updater::tauri::PluginInstaller::new(app.clone(), key)))
}

/// The window layer: the main window from `tauri.conf.json` (`"create": false`), closing the last window keeps Dyno in the
/// tray (the shell quits when there is no tray), bounds remembered in the data folder when installed, and the exit's
/// teardown: the agent this app started is stopped unless a service keeps it (`link::stop_agent_on_quit`), bounded by the
/// exit watchdog. A closed window no longer wants any live detail from the agent.
fn windows<R: Runtime>(app: &tauri::AppHandle<R>, installed: bool) -> Windows<R> {
    let b = Windows::builder(WindowConfig::new(APP_NAME).size(1280.0, 800.0).min_size(900.0, 600.0))
        .prefs(Arc::new(FixedPrefs::new(OnClose::Tray, OpenFilesIn::NewWindow)))
        .on_destroyed(|app, label| {
            if let Some(link) = app.try_state::<Arc<AgentLink>>() {
                link.drop_window(label);
            }
        })
        .teardown("agent", |app| {
            if let Some(link) = app.try_state::<Arc<AgentLink>>() {
                link.quit();
            }
        });
    // A test cannot exit the mock runtime: Quit begins the quit (`Windows::quitting`) and the exit is left out.
    let b = if installed { b } else { b.exit_app(|_, _| ()) };
    match app.path().app_data_dir() {
        Ok(dir) if installed => b.bounds_file(dir.join("window-state.json")).build(),
        _ => b.build(),
    }
}

/// Dyno's commands behind the shell's guard: window classes, the per-dialog contracts and request limits.
pub fn invoke_handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    paddock_commands::tauri::Guard::new(&COMMANDS)
        .dialog_grants(paddock_dialogs::tauri::grant_of)
        .wrap(handler!())
}

/// Dyno on `builder` (after the plugins already on it): the shell plugin `shell`, the tray drawn by `tray_backend`, the
/// main window for `launch` (none for `--hidden` with a tray). The start `run` uses (it adds Dyno's guarded commands)
/// and the start test composes.
pub fn app<R: Runtime>(
    builder: tauri::Builder<R>,
    shell: Builder<R>,
    launch: paddock_window::launch::LaunchArgs,
    tray_backend: impl paddock_window::tauri::TrayBackend<R>,
) -> tauri::Builder<R> {
    // The shell opens both after every plugin is set up (ADR 0029: a window built inside a plugin hook deadlocks).
    // The popover a tray click opens is registered with the tray (the shell finds it by name when the icon is clicked).
    shell
        .tray(move |app| {
            tauri::Manager::manage(app, tray::popovers());
            tray::tray(tray_backend).on_action(|app, id| {
                if id == quit::QUIT_STOP_ID {
                    quit::quit_and_stop(app);
                    return;
                }
                if let Some(shell) = tauri::Manager::try_state::<Arc<shell::ShellOf<R>>>(app) {
                    shell.power_menu_chosen(id);
                    shell.wol_menu_chosen(id);
                }
            })
        })
        .launch(launch)
        .add_to(builder)
}

/// No agent program in a test: the link reports it as not running (nothing is started, rule 7).
struct NoAgent;

impl link::Launcher for NoAgent {
    fn launch(&self) -> Result<(), String> {
        Err("no agent in a test".into())
    }
    fn child(&self) -> link::Child {
        link::Child::None
    }
    fn finish(&self, _: std::time::Duration) {}
}

/// The launcher of the app's own agent ([`link::launch_route`]): a launchd job on macOS, the app's child elsewhere.
fn app_launcher(dirs: &dyno_agent::paths::Dirs) -> Box<dyn link::Launcher> {
    let route = link::launch_route(std::env::consts::OS);
    logging::info(&format!(
        "the app starts its own agent {}",
        match route {
            link::LaunchRoute::Launchd => "as a launchd job",
            link::LaunchRoute::Child => "as its child process",
        }
    ));
    match route {
        #[cfg(target_os = "macos")]
        link::LaunchRoute::Launchd => Box::new(link::LaunchdLauncher::new(
            login::agent_program,
            link::launchd::Setup {
                plist: dirs.root().join(link::launchd::PLIST_FILE),
                uid: paddock_service::native::current_uid(),
                grace: link::driver::END_GRACE,
                env: link::launchd::agent_env(|k| std::env::var(k).ok()),
                sys: Box::new(paddock_service::system::RealSystem::new().with_timeout(std::time::Duration::from_secs(5))),
                clock: std::sync::Arc::new(paddock_jobs::SystemClock),
            },
        )),
        _ => {
            let _ = dirs;
            Box::new(ProcessLauncher::new(login::agent_program))
        }
    }
}

/// Dyno's own log, the shell glue and the agent link in `dirs`; the installed app also starts the agent process, keeps
/// late windows told about it and repairs its service registration.
fn agent_setup<R: Runtime>(
    app: &tauri::AppHandle<R>,
    hub: &health::Hub<R>,
    dirs: dyno_agent::paths::Dirs,
    pixels: Arc<dyn shell::TrayPixels<R>>,
    installed: bool,
) -> std::io::Result<()> {
    logging::info(&format!(
        "Dyno {VERSION} starting (OS {}, {}; UptimeMs {})",
        dyno_agent::logging::os_version(),
        dyno_agent::logging::arch(),
        logging::uptime_ms()
    ));
    let events = shell::ShellOf::new(
        app.clone(),
        hub.clone(),
        pixels,
        shell::saved_items(&dirs.settings()),
        Arc::new(paddock_jobs::SystemClock),
    );
    let launcher: Box<dyn link::Launcher> = if installed { app_launcher(&dirs) } else { Box::new(NoAgent) };
    let link = AgentLink::new(dirs, VERSION, events.clone(), launcher);
    #[cfg(windows)]
    if installed {
        if let Some(dir) = per_machine::install_dir() {
            health::check_setup_version(app, &dir);
        }
    }
    if installed {
        // The first run's registration comes first, on a worker the link waits for (it may open System Settings or wait on
        // systemctl: ER-17), so the link finds a supervising service and starts no agent of its own.
        let (first_app, first_link) = (app.clone(), link.clone());
        link.before_first_launch(Box::new(move || {
            // An agent stopped on request ("Quit and stop measuring") comes back now: the log says it was a request, and the service
            // starts again when nothing supervises it, so the link finds it and starts no second agent.
            quit::resume_at_start(
                first_link.dirs.root(),
                &*login::service_probe(),
                login::current_location(),
                login::platform_service().as_mut(),
            );
            login::run_first_run(&first_app, &first_link, false);
        }));
        // Asked before the link starts an agent of its own: none beside one the login service supervises.
        link.set_service_probe(login::service_probe());
    }
    app.manage(events.clone());
    app.manage(link.clone());
    app.manage(commands::PickedFolders::default());
    app.manage(Arc::new(
        paddock_widgets::Previews::new().grid::<dyno_core::widgets::DashboardCatalogue>(),
    ));
    link.spawn()?;
    if !installed {
        return Ok(());
    }
    // The words of Quit follow the service: "Quit Dyno (keeps measuring)" while a service supervises the agent. Asked of the OS on
    // its own thread (the probe can take seconds), at the start and then every minute; the tray menus change only when it did.
    #[cfg(not(test))]
    {
        let words = events.clone();
        std::thread::spawn(move || loop {
            words.set_keeps_measuring(quit::keeps_measuring(login::service_probe()()));
            std::thread::sleep(std::time::Duration::from_secs(60));
        });
    }
    let beat = events.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(3));
        beat.replay_status();
    });
    // After an update launchd may refuse the registered agent (a rebuilt app has a new code identity on macOS) or
    // keep the old job: replace the registration once per version and whenever launchd says it is stale
    // (`paddock_service::Manager::on_start`), in the background, so measuring resumes without the user's help.
    #[cfg(not(windows))]
    let repair = link.clone();
    // The old route (`tauri-plugin-autostart`) left `~/Library/LaunchAgents/Dyno.plist`; `SMAppService.mainApp` replaces it.
    #[cfg(target_os = "macos")]
    std::thread::spawn(login::migrate_legacy_login_at_start);
    // Windows: no repair here (it rewrote the app's logon entry in 1.0.0); the stale entry of 1.0.0 is repaired once, with the
    // switch, when the settings first arrive (`shell::settings_changed`).
    #[cfg(not(windows))]
    let repair_done = login::track_repair();
    #[cfg(not(windows))]
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(5));
        if login::service_work_cancelled() {
            repair_done.send(()).ok();
            return;
        }
        match login::platform_service().repair(std::sync::Arc::new(move || repair.connected())) {
            Ok(true) => logging::warn("registered the agent service again (a new version, or launchd refused the old registration)"),
            Ok(false) => {}
            Err(e) => logging::warn(&format!("the agent service could not be registered again: {e}")),
        }
        repair_done.send(()).ok();
    });
    Ok(())
}

/// macOS: `SMAppService.mainApp` starts the app at login without `--hidden`; macOS says it was a login start in the
/// open-application Apple Event instead ([`paddock_service::apple_event::open_event`]), which is readable only while that event is
/// being handled, i.e. inside `applicationDidFinishLaunching`. Plugin setups run earlier (in `Builder::build`), so the
/// decision is made in the run loop's `Ready` event, which tao emits from `applicationDidFinishLaunching` right after the
/// App's setup. The shell was told to start in the tray (`launch.hidden`); unless the flag or the login event asked for
/// that, the main window is opened here (the tray exists by then). A missed detection shows the window, never hides one.
#[cfg(target_os = "macos")]
fn on_ready(app: &tauri::AppHandle, asked_hidden: bool) {
    let event = paddock_service::apple_event::open_event();
    let by = paddock_service::launch::launch_reason(asked_hidden, event);
    let hidden = by.is_some();
    logging::info(&format!(
        "start signal: {} (UptimeMs {})",
        event.describe(asked_hidden),
        logging::uptime_ms()
    ));
    logging::info(&match by {
        Some(why) => format!("started hidden ({})", why.describe()),
        None => "started with a window".to_string(),
    });
    if !hidden && app.get_webview_window(paddock_commands::labels::MAIN_LABEL).is_none() {
        if let Err(e) = paddock_window::tauri::create_main(app) {
            log::error!(Error = e.message.as_str(); "The main window could not be opened");
        }
    }
}

/// This run's clean-exit marker (written at Ready, removed at Exit).
static RUN_MARKER: std::sync::OnceLock<paddock_startup::run_marker::RunMarker> = std::sync::OnceLock::new();

/// Starts the app.
pub fn run() {
    let _ = logging::uptime_ms();
    // A window that failed to start can still be reported: `--export-diagnostics` writes the report and exits, with no window.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a.starts_with(paddock_diag::export::FLAG)) {
        let _log = paddock_shell::logging::install(&AppIdentity::new(APP_ID, APP_NAME, VERSION));
        let info = paddock_diag::AppInfo::new(APP_ID, APP_NAME, VERSION, option_env!("PADDOCK_BUILD_COMMIT").unwrap_or(""));
        if let Some(code) = report::export_cli(&args, &info) {
            std::process::exit(code);
        }
    }
    // First: the shared crates' log lines (the start's error among them) reach Dyno's daily log file (paddock-log); Dyno's
    // own lines stay in its agent-side log.
    let _log = paddock_shell::logging::install_with_commit(
        &AppIdentity::new(APP_ID, APP_NAME, VERSION),
        option_env!("PADDOCK_BUILD_COMMIT").unwrap_or(""),
    );
    if let Some(handle) = &_log {
        logging::set_handle(handle.clone());
    }
    crash_reports::log_at_start_in_background(
        crash_reports::default_folders(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
    );
    let cwd = std::env::current_dir().ok();
    #[allow(unused_mut)]
    let mut launch = paddock_window::launch::parse_args(std::env::args().skip(1), cwd.as_deref(), &[]);
    #[cfg(target_os = "macos")]
    let asked_hidden = std::mem::replace(&mut launch.hidden, true);
    let hub = health::Hub::<tauri::Wry>::default();
    let shell = shell::<tauri::Wry>(Place::Installed, &hub, Arc::new(shell::NativePixels));
    let builder = tauri::Builder::default()
        // First: a second launch shows the running app's window instead of starting another app.
        .plugin(tauri_plugin_single_instance::init(paddock_window::tauri::second_instance))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build());
    // The icons start as the saved layout (read from the agent's settings file), not the defaults: a custom layout must not flash.
    let items = shell::saved_items(&dyno_agent::paths::Dirs::current().settings());
    let builder = app(builder, shell, launch, tray::DynoTray::new(items)).invoke_handler(invoke_handler());
    // A web content process termination hook exists on macOS and iOS only (Tauri 2 has none for WebView2 or WebKitGTK).
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let builder = builder.on_web_content_process_terminate(paddock_startup::tauri::web_content_terminated);
    // `Builder::run` is `build` then `run` with no callback; the callback is only for macOS's login-start decision.
    match builder.build(tauri::generate_context!()) {
        Ok(app) => app.run(move |_handle, _event| {
            // The end of a run (ER-27): why it ends and how long it ran; the last repeated lines are written before the process goes.
            match &_event {
                // After the single-instance check (a second launch exits before Ready): say whether the last run died, leave a marker.
                tauri::RunEvent::Ready => {
                    let marker = paddock_env::AppId::new(APP_ID, APP_NAME)
                        .and_then(|id| paddock_env::AppDirs::for_app(&id))
                        .ok()
                        .map(|dirs| {
                            paddock_startup::run_marker::begin(
                                dirs.data(),
                                "app",
                                std::process::id(),
                                &paddock_startup::lifecycle::os_probe,
                            )
                            .0
                        });
                    if let Some(marker) = marker {
                        let _ = RUN_MARKER.set(marker);
                    }
                }
                tauri::RunEvent::ExitRequested { code, .. } => {
                    log::info!(Code = code.unwrap_or(0), UptimeMs = logging::uptime_ms(); "Exit requested");
                }
                tauri::RunEvent::Exit => {
                    if let Some(marker) = RUN_MARKER.get() {
                        marker.end();
                    }
                    log::info!(UptimeMs = logging::uptime_ms(); "Exit");
                    if let Some(handle) = logging::handle() {
                        handle.flush_repeats();
                    }
                }
                _ => {}
            }
            #[cfg(target_os = "macos")]
            if matches!(_event, tauri::RunEvent::Ready) {
                on_ready(_handle, asked_hidden);
            }
            // The icons are removed on the way out (AppKit may drop their positions then): keep a copy while they are still there.
            #[cfg(target_os = "macos")]
            if matches!(_event, tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit) {
                tray::save_positions(_handle);
            }
        }),
        Err(e) => {
            // The log first (it is what a bug report holds), then standard error for a person at a terminal.
            log::error!(Error = e.to_string().as_str(), UptimeMs = logging::uptime_ms(); "Dyno could not start; check the webview runtime and the app's folders, then start it again");
            if let Some(handle) = logging::handle() {
                handle.flush_repeats();
            }
            eprintln!("Dyno could not start: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod diagnostics_tests {
    use super::*;

    #[test]
    fn the_watchdog_question_exists_for_the_installed_app_only_and_never_before_seventy_seconds() {
        let test = Place::Test {
            dir: std::path::PathBuf::from("/nonexistent/dyno-test"),
        };
        assert!(
            watchdog::<tauri::test::MockRuntime>(&test).is_none(),
            "a test place shows no question"
        );
        assert!(watchdog::<tauri::test::MockRuntime>(&Place::Installed).is_some_and(|s| s.is_ok()));
        let config = paddock_startup::Config::default();
        assert!(
            first_question_after(&config) >= std::time::Duration::from_secs(70),
            "a healthy slow start that is ready within 70 s is never asked"
        );
        assert!(
            config.pause_gap >= std::time::Duration::from_secs(3),
            "a paused app restarts the clocks instead of asking"
        );
    }

    #[test]
    fn report_a_problem_is_set_up_for_the_installed_app_only() {
        assert!(diagnostics_wanted(&Place::Installed));
        let test = Place::Test {
            dir: std::path::PathBuf::from("/nonexistent/dyno-test"),
        };
        assert!(!diagnostics_wanted(&test), "a test place never resolves the real folders (ER-14)");
    }
}

#[cfg(test)]
mod updater_tests {
    use super::*;

    /// About lists the third-party notices first and they name what Dyno ships: Rust crates of the app, npm packages of the UI and
    /// the licence text each is under (the attribution Apache-2.0 and MIT require).
    #[test]
    fn about_lists_the_third_party_notices_with_known_dependencies() {
        let (id, title, text) = LEGAL[0];
        assert_eq!((id, title), ("notices", "Third-party notices"));
        for known in ["- tauri ", "- serde ", "- echarts ", "- rxjs "] {
            assert!(text.contains(known), "the notices do not list `{}`", known.trim());
        }
        assert!(
            text.contains("## Licence texts") && text.contains("Permission is hereby granted"),
            "the licence texts are included"
        );
        let ids: Vec<_> = LEGAL.iter().map(|d| d.0).collect();
        assert_eq!(
            ids,
            ["notices", "licence-mit", "licence-apache"],
            "ids are unique and the notices stay listed"
        );
    }

    #[test]
    fn the_built_app_has_an_installer_so_a_found_update_can_be_installed() {
        let app = tauri::test::mock_app();
        let has = updater(None, app.handle()).has_installer();
        // Windows hands the download to `paddock-setup.exe` only from a folder that passes the Program Files rule; the test
        // program runs from the build folder, so there it finds updates and says it cannot install them (logged at start).
        #[cfg(windows)]
        assert!(!has, "a copy outside Program Files has no hand-off");
        #[cfg(not(windows))]
        assert!(has);
    }
}

#[cfg(test)]
mod capability_tests {
    fn caps(src: &str) -> serde_json::Value {
        serde_json::from_str(src).unwrap()
    }

    fn granted(c: &serde_json::Value) -> Vec<&str> {
        c["permissions"].as_array().unwrap().iter().filter_map(|p| p.as_str()).collect()
    }

    /// The window has an overlay title bar on macOS: the page's drag regions only move it when the webview may start
    /// a window drag. Without the permission dragging fails silently.
    #[test]
    fn the_main_window_can_be_dragged_by_its_drag_regions() {
        let c = caps(include_str!("../capabilities/shell.json"));
        let granted = granted(&c);
        assert!(granted.contains(&"core:window:allow-start-dragging"), "{granted:?}");
        assert!(granted.contains(&"core:window:allow-internal-toggle-maximize"), "{granted:?}");
        assert!(c["windows"].as_array().unwrap().iter().any(|w| w == "main"));
    }

    /// The webview reaches the OS only through Dyno's own commands and the shell's sets: no other plugin permission, no
    /// wildcard, no remote content.
    #[test]
    fn the_capabilities_are_least_privilege() {
        for src in [
            include_str!("../capabilities/shell.json"),
            include_str!("../capabilities/dialog.json"),
            include_str!("../capabilities/tray.json"),
        ] {
            let c = caps(src);
            for p in granted(&c) {
                assert!(
                    p.starts_with("core:window:allow-") || p == "core:event:default" || p.starts_with("paddock-shell:"),
                    "unexpected permission {p}"
                );
                assert!(!p.contains('*'), "{p}");
            }
            assert!(c.get("remote").is_none(), "no remote content may use IPC");
        }
    }

    /// Each window class gets its own shell set; the popover has no dialog or main commands.
    #[test]
    fn each_window_class_has_its_shell_set() {
        let want = [
            (include_str!("../capabilities/shell.json"), "paddock-shell:main"),
            (include_str!("../capabilities/dialog.json"), "paddock-shell:dialog"),
            (include_str!("../capabilities/tray.json"), "paddock-shell:tray"),
        ];
        for (src, set) in want {
            assert!(granted(&caps(src)).contains(&set), "{set}");
        }
    }

    /// No `unsafe-eval`, no remote origin, nothing but the app's own scheme and IPC.
    #[test]
    fn the_csp_has_no_unsafe_eval_and_no_remote_origin() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = conf["app"]["security"]["csp"].as_str().unwrap();
        assert!(!csp.contains("unsafe-eval"), "{csp}");
        assert!(!csp.contains("http://") || csp.contains("http://ipc.localhost"), "{csp}");
        assert!(!csp.contains("https:"), "{csp}");
        assert!(!csp.contains('*'), "{csp}");
        assert!(csp.contains("object-src 'none'") && csp.contains("frame-ancestors 'none'"));
    }

    /// The main window is built by the shell (`create: false`), so its remembered bounds and start in the tray apply.
    #[test]
    fn the_shell_builds_the_main_window() {
        for src in [
            include_str!("../tauri.conf.json"),
            include_str!("../tauri.macos.conf.json"),
            include_str!("../tauri.windows.conf.json"),
        ] {
            let conf: serde_json::Value = serde_json::from_str(src).unwrap();
            let main = conf["app"]["windows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|w| w["label"] == "main")
                .unwrap();
            assert_eq!(main["create"], false);
        }
    }

    /// The bundle step checks the built index.html first (a CSP nonce would break Angular's styles).
    #[test]
    fn the_build_checks_the_index_before_bundling() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert!(conf["build"]["beforeBuildCommand"].as_str().unwrap().contains("check-index.mjs"));
    }

    /// The autostart registration and the login item the agent installs both start Dyno hidden in the tray, with the
    /// flag the shell's launch parser knows.
    #[test]
    fn the_login_start_uses_the_shells_hidden_flag() {
        assert_eq!(paddock_window::launch::HIDDEN_FLAG, "--hidden");
    }
}
