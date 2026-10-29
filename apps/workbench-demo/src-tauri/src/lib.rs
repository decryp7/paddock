//! Paddock Workbench Demo: the workbench demo's UI (`ui/projects/workbench-demo`) on the real `paddock-shell` and
//! `paddock-workbench` plugins (plan docs/plans/workbench.md §10, X50 W8):
//!
//! - [`shell`]: the shell plugin with the app's windows, the sample job and the app's own commands;
//! - [`workbench`]: the workbench plugin with the dock's layouts in the config folder, the pop-out panels (the drag ghost's
//!   page is served from the UI's public folder) and the assistant on Pitwall with one read-only sample tool;
//! - [`engine`]: the sample re-index job; [`assistant`]: the provider and the tool; [`commands`]: the app's own commands
//!   ([`COMMANDS`]): the four of the shared needs-AI component and the re-index.
//!
//! Without Pitwall the assistant pane shows its calm "Pitwall is not running" (or not installed) state with its fix; nothing
//! here needs a model to start. It is also the starter of new IDE-style apps (README.md, "Start a new app from the workbench demo").

pub mod assistant;
pub mod commands;
pub mod dialogs;
pub mod engine;

use paddock_env::{AppDirs, AppId, MapVars, Os};
use paddock_panels::tauri::Panels;
use paddock_pitwall::{AppIdentity as PitwallIdentity, Link, LinkConfig, Locator};
use paddock_secrets::MemorySecrets;
use paddock_shell::{AppIdentity, Builder};
use paddock_window::tauri::{FixedPrefs, WindowConfig, Windows};
use paddock_window::{OnClose, OpenFilesIn};
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Manager, Runtime};

/// The app's id (the shell's `AppIdentity`, its folders and its Pitwall key).
pub const APP_ID: &str = "workbench-demo";
/// The product name (`tauri.conf.json` `productName`, the windows, About).
pub const APP_NAME: &str = "Paddock Workbench Demo";
/// The app's version: Cargo.toml's (tauri.conf.json has none, so the bundles carry this one too).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

paddock_commands::command_table! {
    /// The demo's own commands (the shell's and the workbench's are the plugins').
    pub static COMMANDS, macro handler;
    /// The state of Pitwall for the needs-AI component (looks now).
    Query [MAIN | POPOUT] => commands::pitwall_state,
    /// The user chose to connect Pitwall: starts the pairing.
    Mutation [MAIN | POPOUT] => commands::pitwall_connect,
    /// Stops a waiting pairing.
    Mutation [MAIN | POPOUT] => commands::pitwall_cancel_pairing,
    /// "Use the built-in engine": the demo has none, so it is refused (and not offered).
    Mutation [MAIN | POPOUT] => commands::pitwall_use_builtin,
    /// Starts the sample re-index job and returns its id at once.
    Mutation [MAIN] => commands::demo_reindex,
}

/// Where the app runs: installed (the user's folders, real ticks, the real Pitwall folders) or in a test (nothing outside
/// `dir`, no Pitwall, instant ticks).
#[derive(Debug, Clone)]
pub enum Place {
    Installed,
    Test { dir: PathBuf },
}

impl Place {
    fn test_dir(&self) -> Option<&std::path::Path> {
        match self {
            Place::Installed => None,
            Place::Test { dir } => Some(dir),
        }
    }
}

type BoxError = Box<dyn Error + Send + Sync>;

/// The app's folders: the user's (installed) or `dir` (a test's, through the `WORKBENCH_DEMO_HOME` override).
pub fn dirs(test_dir: Option<&std::path::Path>) -> Result<AppDirs, BoxError> {
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

/// The app's Pitwall link: the real Pitwall folders when installed, none in a test. The key lives in memory for the run (a
/// demo does not touch the user's credential store).
pub fn link(place: &Place) -> Result<Arc<Link>, BoxError> {
    let locator = match place.test_dir() {
        None => Locator::system(),
        Some(dir) => Locator::in_dir(&dir.join("no-pitwall"), None, Arc::new(|_| false)),
    };
    let identity = PitwallIdentity::new(APP_ID, APP_NAME)?;
    Ok(Link::new(LinkConfig::new(
        identity,
        locator,
        Arc::new(MemorySecrets::new(APP_NAME)),
    )))
}

/// The shell plugin with the demo's parts: the identity, the window layer and the app's own state (the sample job, the
/// Pitwall link whose changes every window hears).
pub fn shell<R: Runtime>(place: &Place, link: Arc<Link>) -> Builder<R> {
    let installed = matches!(place, Place::Installed);
    let tick = if installed {
        engine::TICK
    } else {
        std::time::Duration::ZERO
    };
    let builder = Builder::new(AppIdentity::new(APP_ID, APP_NAME, VERSION))
        .app_commands(&COMMANDS)
        .dialogs(&[&dialogs::DIALOGS, &paddock_workbench::DIALOGS])
        .windows(move |app| windows(app, installed))
        .setup(move |app| {
            app.manage(engine::Engine::new(tick));
            let announced = app.clone();
            let watched = link.clone();
            link.set_listener(Arc::new(move |_| commands::announce(&announced, &watched)));
            app.manage(commands::Pitwall { link });
            Ok(())
        });
    match place {
        Place::Installed => builder,
        Place::Test { .. } => builder.in_memory().native_menu(false),
    }
}

/// The workbench plugin: layouts in the config folder, the pop-out panels (their places in the data folder) and the
/// assistant. Register it after the shell's.
pub fn workbench<R: Runtime>(
    place: &Place,
    link: Arc<Link>,
) -> Result<paddock_workbench::Builder<R>, BoxError> {
    let dirs = dirs(place.test_dir())?;
    let mut panels = Panels::builder().route("index.html#/popout");
    if !matches!(place, Place::Test { .. }) {
        panels = panels.bounds_file(dirs.data().join("popout-places.json"));
    }
    let assistant = assistant::assistant(link, dirs.data(), APP_NAME)?;
    Ok(paddock_workbench::Builder::new()
        .layouts(dirs.config())
        .panels(panels.build())
        .assistant(assistant))
}

/// The window layer: the main window from `tauri.conf.json` (`"create": false`), closing the last window quits, bounds
/// remembered in the data folder when installed.
fn windows<R: Runtime>(app: &tauri::AppHandle<R>, installed: bool) -> Windows<R> {
    let b = Windows::builder(
        WindowConfig::new(APP_NAME)
            .size(1280.0, 800.0)
            .min_size(900.0, 600.0),
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

/// The demo's own commands behind its guard: window classes, the per-dialog contracts and request limits.
pub fn invoke_handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static
{
    paddock_commands::tauri::Guard::new(&COMMANDS)
        .dialog_grants(paddock_dialogs::tauri::grant_of)
        .wrap(handler!())
}

/// The demo on `builder`: the shell plugin `shell` (it opens the main window for `launch` after every plugin is set
/// up), then the workbench plugin and the demo's guarded commands. The start `run` uses and the start test composes.
pub fn app<R: Runtime>(
    builder: tauri::Builder<R>,
    shell: Builder<R>,
    workbench: paddock_workbench::Builder<R>,
    launch: paddock_window::launch::LaunchArgs,
) -> tauri::Builder<R> {
    shell
        .launch(launch)
        .add_to(builder)
        .plugin(workbench.build())
        .invoke_handler(invoke_handler())
}

/// The app: the two plugins, the demo's guarded commands and the main window.
pub fn run() {
    let place = Place::Installed;
    let started = link(&place).and_then(|link| {
        let workbench = workbench::<tauri::Wry>(&place, link.clone())?;
        Ok((shell::<tauri::Wry>(&place, link), workbench))
    });
    let (shell, workbench) = match started {
        Ok(parts) => parts,
        Err(e) => {
            eprintln!("{APP_NAME} could not start: {e}");
            std::process::exit(1);
        }
    };
    let cwd = std::env::current_dir().ok();
    let launch = paddock_window::launch::parse_args(std::env::args().skip(1), cwd.as_deref(), &[]);
    let app =
        app(tauri::Builder::default(), shell, workbench, launch).build(tauri::generate_context!());
    match app {
        // A pop-out closing while the app quits docks nothing back (the layout keeps it for next time).
        Ok(app) => app.run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event
                && let Some(panels) = app.try_state::<Panels>()
            {
                panels.set_exiting(true);
            }
        }),
        Err(e) => {
            eprintln!("{APP_NAME} could not start: {e}");
            std::process::exit(1);
        }
    }
}
