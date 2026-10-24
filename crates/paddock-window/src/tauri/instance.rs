//! Files and links from outside: a second launch (the single-instance callback), macOS "Open with" and URL schemes
//! (`RunEvent::Opened`), the Dock icon (`RunEvent::Reopen`) and the tray. They follow [`plan`]: an empty window is
//! reused, links go to the window in front, and the `openFilesIn` setting decides the rest.

use super::manager::{self, state};
use crate::launch::{OpenPlan, OpenRequest, Target, existing, names, parse_args, parse_link, plan};
use crate::wire::{OPEN_ASK_EVENT, OPEN_EVENT, OpenAsk, WindowError};
use ::tauri::{AppHandle, Emitter, Runtime};
use paddock_commands::labels::MAIN_LABEL;
use std::path::PathBuf;

fn schemes<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    state(app)
        .map(|w| w.config.schemes.clone())
        .unwrap_or_default()
}

/// The single-instance callback: another launch passed `argv` (program name first) from `cwd`. Use it as
/// `tauri_plugin_single_instance::init(paddock_window::tauri::second_instance)`. A second log-in start (`--hidden`
/// with nothing to open) changes nothing.
pub fn second_instance<R: Runtime>(app: &AppHandle<R>, argv: Vec<String>, cwd: String) {
    let cwd = (!cwd.is_empty()).then(|| PathBuf::from(&cwd));
    let schemes = schemes(app);
    let schemes: Vec<&str> = schemes.iter().map(String::as_str).collect();
    let args = parse_args(argv.into_iter().skip(1), cwd.as_deref(), &schemes);
    if args.hidden && args.paths.is_empty() && args.links.is_empty() {
        log::info!("A second log-in start changes nothing");
        return;
    }
    let request = OpenRequest {
        paths: existing(&args.paths),
        links: args.links,
    };
    // Counts only: paths and links stay out of the logs.
    log::info!(Paths = request.paths.len() as u64, Requested = args.paths.len() as u64, Links = request.links.len() as u64, NewWindow = args.new_window; "Second launch forwarded to the running instance");
    open_from_outside(app, request, args.new_window);
}

/// Files and links the OS opened the app with (macOS `RunEvent::Opened`): `file:` URLs are paths, the app's schemes
/// are deep links, anything else is ignored.
pub fn opened<R: Runtime>(app: &AppHandle<R>, urls: &[url::Url]) {
    let schemes = schemes(app);
    let schemes: Vec<&str> = schemes.iter().map(String::as_str).collect();
    let paths: Vec<PathBuf> = urls.iter().filter_map(|u| u.to_file_path().ok()).collect();
    let links = urls
        .iter()
        .filter(|u| u.scheme() != "file")
        .filter_map(|u| parse_link(u.as_str(), &schemes))
        .collect();
    open_from_outside(
        app,
        OpenRequest {
            paths: existing(&paths),
            links,
        },
        false,
    );
}

/// The Dock icon was clicked while no window is visible (macOS `RunEvent::Reopen`): show the window in front, or
/// rebuild the main window.
pub fn reopen<R: Runtime>(app: &AppHandle<R>) {
    open_from_outside(app, OpenRequest::default(), false);
}

/// Gives `request` to window `label`: it waits there until the UI takes it (`window_take_pending`) after
/// `paddock://window-open`.
pub fn queue_open<R: Runtime>(app: &AppHandle<R>, label: &str, request: OpenRequest) {
    if request.is_empty() {
        return;
    }
    if let Ok(w) = state(app) {
        w.queue(label, request);
        let _ = app.emit_to(label, OPEN_EVENT, ());
    }
}

/// Asks window `label` whether `paths` open there or in a new window.
fn ask_open<R: Runtime>(app: &AppHandle<R>, label: &str, paths: Vec<String>) {
    if let Ok(w) = state(app) {
        let names = names(&paths);
        w.set_ask(label, paths);
        let _ = app.emit_to(
            label,
            OPEN_ASK_EVENT,
            OpenAsk {
                count: names.len(),
                names,
            },
        );
    }
}

fn apply<R: Runtime>(app: &AppHandle<R>, plan: OpenPlan) -> Result<(), WindowError> {
    match plan {
        OpenPlan::Reveal(label) => manager::reveal(app, &label),
        // No window is left (a tray app), or a single-window app: the main window, rebuilt when needed.
        OpenPlan::NewWindow(request)
            if manager::target(app).is_none() || !state(app)?.config.multi_window =>
        {
            manager::create_main(app)?;
            queue_open(app, MAIN_LABEL, request);
            manager::reveal(app, MAIN_LABEL)
        }
        OpenPlan::NewWindow(request) => manager::open_new(app, request).map(|_| ()),
        OpenPlan::OpenIn(label, request) => {
            queue_open(app, &label, request);
            manager::reveal(app, &label)
        }
        OpenPlan::Ask {
            label,
            paths,
            links,
        } => {
            let shown = manager::reveal(app, &label);
            queue_open(
                app,
                &label,
                OpenRequest {
                    paths: Vec::new(),
                    links,
                },
            );
            ask_open(app, &label, paths);
            shown
        }
    }
}

/// Opens files and links from outside (a second launch, the OS, the tray) per the plan; failures are logged.
pub fn open_from_outside<R: Runtime>(
    app: &AppHandle<R>,
    request: OpenRequest,
    force_new_window: bool,
) {
    let Ok(windows) = state(app) else { return };
    let target = manager::target(app).map(|label| Target {
        pristine: windows.is_pristine_for(app, &label),
        label,
    });
    let plan = plan(
        request,
        force_new_window,
        windows.prefs.open_files_in(),
        target,
        windows.config.multi_window,
    );
    log::info!(Plan = plan.kind(); "Opening from another app");
    if let Err(e) = apply(app, plan) {
        log::warn!(Error = e.message.as_str(); "Could not open what another app passed");
    }
}

/// The calling window's answer to "open here or in a new window?". The files are taken either way, so a later answer
/// opens nothing.
pub(crate) fn resolve_open<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    choice: crate::OpenChoice,
) -> Result<(), WindowError> {
    let paths = state(app)?.take_ask(label);
    if paths.is_empty() {
        return Ok(());
    }
    let request = OpenRequest {
        paths,
        links: Vec::new(),
    };
    match choice {
        crate::OpenChoice::Here => queue_open(app, label, request),
        crate::OpenChoice::NewWindow => {
            manager::open_new(app, request)?;
        }
        crate::OpenChoice::Cancel => {
            log::info!(Files = request.paths.len() as u64; "Opening files from another app cancelled")
        }
    }
    Ok(())
}
