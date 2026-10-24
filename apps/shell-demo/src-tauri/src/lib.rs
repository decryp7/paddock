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

pub mod commands;
pub mod engine;
pub mod sections;
pub mod tray;

use paddock_shell::{AppIdentity, Builder};
use paddock_updater::tauri::UpdatesBuilder;
use paddock_window::tauri::{FixedPrefs, WindowConfig, Windows};
use paddock_window::{OnClose, OpenFilesIn};
use std::path::PathBuf;
use std::sync::Arc;
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

/// The shell plugin with every part of the demo.
pub fn shell<R: Runtime>(place: Place) -> Builder<R> {
    let tick = place.tick();
    let mut builder = Builder::new(AppIdentity::new(APP_ID, APP_NAME, VERSION))
        .app_commands(&COMMANDS)
        .dialog_contracts(DIALOG_CONTRACTS)
        .health(engine::services(tick))
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
            option_env!("SHELL_DEMO_COMMIT"),
            option_env!("SHELL_DEMO_BUILT"),
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
    let cwd = std::env::current_dir().ok();
    let launch = paddock_window::launch::parse_args(std::env::args().skip(1), cwd.as_deref(), &[]);
    let shell = shell::<tauri::Wry>(Place::Installed).setup(move |app| {
        tray::create(app);
        paddock_window::tauri::start(app, &launch)?;
        Ok(())
    });
    let result = tauri::Builder::default()
        .plugin(shell.build())
        .invoke_handler(invoke_handler())
        .on_web_content_process_terminate(paddock_startup::tauri::web_content_terminated)
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("{APP_NAME} could not start: {e}");
        std::process::exit(1);
    }
}
