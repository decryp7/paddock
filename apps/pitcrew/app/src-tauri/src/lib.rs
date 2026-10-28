//! The Pitcrew app: tray icon and window over the Pitcrew service (ADR 0001). It holds no runner state of its own:
//! everything comes from the service through [`service_client::ServiceClient`], pushed by `events`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod app_state;
pub mod applog;
pub mod channels;
pub mod commands;
pub mod credentials;
pub mod diagnostics;
pub mod dto;
pub mod events;
pub mod gitea;
pub mod handoff;
pub mod health;
pub mod jobs;
pub mod ops;
pub mod service_client;
pub mod service_install;
pub mod service_jobs;
pub mod service_repair;
pub mod tray;

use commands::SharedClient;
use paddock_shell::{AppIdentity, Builder};
use paddock_updater::tauri::UpdatesBuilder;
use paddock_window::tauri::{FixedPrefs, WindowConfig, Windows};
use paddock_window::{OnClose, OpenFilesIn};
use std::sync::Arc;
use tauri::{Manager, Runtime};

/// The app's id (the shell's `AppIdentity`, its folders and its log target).
pub const APP_ID: &str = "pitcrew";
/// The product name.
pub const APP_NAME: &str = "Pitcrew";
/// The app's version: the workspace's (Cargo is the single source).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Where the releases are (About's "Release notes", Updates' "Open the releases page"): the family's page filtered to
/// Pitcrew's tags.
pub const RELEASES_PAGE: &str = "https://dev.decryptology.net/decryp7/paddock/releases?q=pitcrew-v";
/// The update feed: the moving release that pitcrew-release.yml publishes `latest.json` on (`tests/shell.rs` ties it to the
/// workflow's `latest-tag`; `tauri.conf.json` names the same address for the updater plugin).
pub const UPDATE_FEED: &str = "https://dev.decryptology.net/decryp7/paddock/releases/download/pitcrew-latest/latest.json";
/// The dialog contracts and the manifest, written by the UI's specs from its registry (`app/src/app/shell/`).
pub const DIALOG_CONTRACTS: &str = include_str!("../dialog-contracts.json");
/// The UI's `paddock-manifest.json` (checked against the plugin in `tests/shell.rs`).
pub const MANIFEST: &str = include_str!("../paddock-manifest.json");

paddock_commands::command_table! {
    /// Pitcrew's own commands behind the shell's guard. Windows and dialogs may call them; each dialog's contract
    /// (`dialog-contracts.json`) narrows a dialog to the few it needs.
    pub static COMMANDS, macro handler;
    Query [MAIN | DIALOG] => commands::platform_name,
    Query [MAIN] => commands::take_pending_navigation,
    Query [MAIN | DIALOG] => commands::runner_status,
    Query [MAIN | DIALOG] => commands::connection_state,
    Mutation [MAIN] => commands::runner_action,
    Mutation [MAIN] => commands::skip_version,
    Mutation [MAIN] => commands::pin_version,
    Quiet [MAIN] => commands::runner_logs,
    Query [MAIN] => commands::search_logs,
    Query [MAIN] => commands::runner_activity,
    Quiet [MAIN] => commands::machine_metrics,
    Query [MAIN] => commands::runner_disk,
    Mutation [MAIN] => commands::clean_disk,
    Query [MAIN] => commands::runner_config_get,
    Query [MAIN] => commands::runner_config_preview,
    Mutation [MAIN] => commands::runner_config_save,
    Query [MAIN] => commands::runner_processes,
    Mutation [MAIN] => commands::runner_process_stop,
    Mutation [MAIN] => commands::runner_process_keep,
    Query [MAIN] => commands::run_details,
    Query [MAIN | DIALOG] => commands::get_settings,
    Mutation [MAIN | DIALOG] => commands::save_settings,
    Mutation [MAIN] => commands::add_runner,
    Mutation [MAIN] => commands::remove_runner,
    Mutation [MAIN | DIALOG] => commands::register_runner,
    Query [MAIN | DIALOG] => commands::has_api_token,
    Mutation [MAIN | DIALOG] => commands::set_api_token,
    Mutation [MAIN | DIALOG] => commands::clear_api_token,
    Query [MAIN | DIALOG] => commands::service_install_state,
    Mutation [MAIN | DIALOG] => commands::install_service,
    Mutation [MAIN | DIALOG] => commands::uninstall_service,
    Mutation [MAIN | DIALOG] => commands::repair_service,
    Mutation [MAIN | DIALOG] => commands::pitcrew_open_login_items,
    Query [MAIN | DIALOG] => commands::get_autostart,
    Mutation [MAIN | DIALOG] => commands::set_autostart,
    Mutation [MAIN | DIALOG] => commands::export_diagnostics,
    Mutation [MAIN | DIALOG] => commands::open_url,
    Mutation [MAIN] => commands::reveal_logs,
    Query [MAIN | DIALOG] => jobs::job_result,
    Query [MAIN] => ops::fleet_insights,
    Query [MAIN] => ops::engine_report,
    Query [MAIN] => ops::engine_prune_preview,
    Mutation [MAIN] => ops::engine_prune,
    Query [MAIN] => ops::cache_report,
    Query [MAIN] => ops::cache_preview,
    Mutation [MAIN] => ops::cache_clean,
    Query [MAIN] => ops::debrief_status,
    Mutation [MAIN] => ops::open_in_debrief,
    Query [MAIN | DIALOG] => ops::notification_channels_get,
    Mutation [MAIN | DIALOG] => ops::notification_channels_save,
    Mutation [MAIN | DIALOG] => ops::channel_secret_set,
    Mutation [MAIN | DIALOG] => ops::channel_secret_clear,
    Query [MAIN | DIALOG] => ops::channel_secret_present,
    Mutation [MAIN | DIALOG] => ops::channel_test,
}

/// Where the app runs: installed (the user's folders, the real service) or in a test (nothing outside `dir`, no
/// background threads that reach the service, no native menu off the main thread).
#[derive(Debug, Clone)]
pub enum Place {
    /// The installed app.
    Installed,
    /// A test on Tauri's mock runtime.
    Test {
        /// The folder everything is kept in.
        dir: std::path::PathBuf,
    },
}

/// The shell plugin with Pitcrew's parts: its service as a health service with a Repair, the updater with About and
/// What's New, the window layer (closing the window keeps Pitcrew in the tray), and its managed state.
pub fn shell<R: Runtime>(client: SharedClient, place: Place) -> Builder<R> {
    let test_dir = match &place {
        Place::Installed => None,
        Place::Test { dir } => Some(dir.clone()),
    };
    let installed = test_dir.is_none();
    let updates_dir = test_dir.clone();
    let mut builder = Builder::new(AppIdentity::new(APP_ID, APP_NAME, VERSION))
        .app_commands(&COMMANDS)
        .dialog_contracts(DIALOG_CONTRACTS)
        .health(vec![health::spec()])
        .updater(move |_| updater(updates_dir))
        .windows(move |app| windows(app, installed))
        .setup(move |app| {
            app.manage(client);
            app.manage(events::PendingNav::default());
            app.manage(events::Latest::default());
            app.manage(events::Actions::new(app.state::<SharedClient>().inner().clone()));
            app.manage(jobs::Work::default());
            app.manage(service_jobs::Mirror::default());
            if let Ok(dir) = app.path().app_data_dir() {
                health::set_data_dir(dir);
            }
            Ok(())
        });
    if !installed {
        builder = builder.in_memory().native_menu(false);
    }
    builder
}

/// Updates, About and What's New with Pitcrew's changelog. A build without the CI's updater key cannot update itself and
/// says so.
fn updater(test_dir: Option<std::path::PathBuf>) -> UpdatesBuilder {
    let b = UpdatesBuilder::new(APP_NAME)
        .channel("stable", "Stable", UPDATE_FEED)
        .public_key(option_env!("TAURI_UPDATER_PUBKEY"))
        .releases_page(RELEASES_PAGE)
        .homepage("https://dev.decryptology.net/decryp7/paddock")
        .changelog(include_str!("../../../CHANGELOG.md"))
        .legal("licence-mit", "MIT licence", include_str!("../../../LICENSE-MIT"))
        .legal("licence-apache", "Apache licence 2.0", include_str!("../../../LICENSE-APACHE"))
        .build_info(option_env!("PITCREW_COMMIT"), option_env!("PITCREW_BUILT"));
    match test_dir {
        None => b,
        Some(dir) => b.dirs(dir.join("config"), dir.join("cache")).background(false),
    }
}

/// The window layer: the main window from `tauri.conf.json` (`"create": false`); closing it keeps Pitcrew in the tray
/// (without a tray the window layer quits); bounds are remembered in the data folder when installed.
fn windows<R: Runtime>(app: &tauri::AppHandle<R>, installed: bool) -> Windows<R> {
    let b = Windows::builder(WindowConfig::new(APP_NAME).size(1200.0, 780.0).min_size(760.0, 520.0))
        .prefs(Arc::new(FixedPrefs::new(OnClose::Tray, OpenFilesIn::NewWindow)));
    match app.path().app_data_dir() {
        Ok(dir) if installed => b.bounds_file(dir.join("window-state.json")).build(),
        _ => b.build(),
    }
}

/// Pitcrew's own commands behind the guard: window classes, per-dialog contracts and request limits.
pub fn invoke_handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    paddock_commands::tauri::Guard::new(&COMMANDS)
        .dialog_grants(paddock_dialogs::tauri::grant_of)
        .wrap(handler!())
}

/// Starts the app.
pub fn run() {
    let client: SharedClient = Arc::new(service_client::LocalServiceClient);
    let cwd = std::env::current_dir().ok();
    let launch = paddock_window::launch::parse_args(std::env::args().skip(1), cwd.as_deref(), &[]);
    let shell = shell::<tauri::Wry>(client, Place::Installed).setup(move |app| {
        // After an update launchd may refuse the registered service (a rebuilt app has a new code identity on macOS):
        // register it again in the background, so the runners come back without the user's help (rule 24).
        #[cfg(target_os = "macos")]
        {
            let repair_dir = app.path().app_data_dir().ok();
            std::thread::spawn(move || {
                let Some(dir) = repair_dir else { return };
                // The service may only fail later (it restarts itself onto a new binary when idle), so keep checking while
                // it does not answer, a bounded number of times per run of the app.
                for attempt in 0..service_install::REPAIR_ATTEMPTS {
                    std::thread::sleep(service_install::repair_delay(attempt));
                    match service_install::repair_registration(&dir) {
                        Some(service_repair::RepairOutcome::Recovered | service_repair::RepairOutcome::NeedsApproval) => break,
                        // Failed: the next check tries again (bounded by REPAIR_ATTEMPTS); nothing to repair: keep watching.
                        Some(service_repair::RepairOutcome::Failed(_)) | None => {}
                    }
                }
            });
        }
        tray::create(app);
        events::start(app.clone());
        paddock_window::tauri::start(app, &launch)?;
        Ok(())
    });
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(shell.build())
        .invoke_handler(invoke_handler());
    // Tauri has this event on macOS and iOS only (not WebView2 or WebKitGTK).
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(paddock_startup::tauri::web_content_terminated);
    let result = builder.run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("{APP_NAME} could not start: {e}");
        std::process::exit(1);
    }
}
