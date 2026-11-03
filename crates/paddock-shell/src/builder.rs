//! [`Builder`]: the shell plugin with its parts. Everything is optional except the identity; a part the app leaves out
//! answers its commands with that crate's "not set up" error (degrade, don't fail), except the always-on ones: jobs,
//! the notification centre (window notices need it), settings with the shell's Appearance section, dialogs (an empty
//! contract file), the start-up watchdog with its defaults, and the menu.

use crate::setup::{
    LoginItem, NotificationAccess, NotificationsItem, PitwallItem, PitwallPort, ServiceItem,
    SetupItem,
};
use crate::{COMMANDS, OWN_COMMANDS, PLUGIN_NAME, commands, prefs};
use paddock_commands::Table;
use paddock_commands::tauri::{Guard, Routes};
use paddock_dialogs::Dialogs;
use paddock_health::ServiceSpec;
use paddock_health::tauri::Setup as HealthSetup;
use paddock_settings::{Configurable, Settings};
use paddock_startup::tauri::Startup;
use paddock_updater::tauri::UpdatesBuilder;
use paddock_window::WindowError;
use paddock_window::launch::LaunchArgs;
use paddock_window::tauri::{Tray, Windows};
use serde::Serialize;
use std::error::Error;
use std::sync::Arc;
use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, RunEvent, Runtime, Window, WindowEvent};

/// Which app this is: its id (`pitwall`), its name for the user and its version.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIdentity {
    pub id: String,
    pub name: String,
    pub version: String,
}

impl AppIdentity {
    pub fn new(id: &str, name: &str, version: &str) -> AppIdentity {
        AppIdentity {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
        }
    }
}

type BoxError = Box<dyn Error + Send + Sync>;
type Part<R, T> = Box<dyn FnOnce(&AppHandle<R>) -> T + Send>;
type DiagSetup<R> =
    Box<dyn FnOnce(&AppHandle<R>) -> Result<paddock_diag::tauri::Setup, BoxError> + Send>;
type FilesSetup<R> =
    Box<dyn FnOnce(&AppHandle<R>) -> Result<paddock_files::Files, BoxError> + Send>;
type PacksSetup<R> =
    Box<dyn FnOnce(&AppHandle<R>) -> Result<paddock_packs::tauri::Setup, BoxError> + Send>;
type ChannelsSetup<R> = Box<
    dyn FnOnce(
            &AppHandle<R>,
            paddock_channels::Emit,
        ) -> Result<Arc<paddock_channels::ChannelsControl>, BoxError>
        + Send,
>;
type StorageSetup<R> =
    Box<dyn FnOnce(&AppHandle<R>) -> Result<paddock_storage::Storage, BoxError> + Send>;
type McpSetup<R> = Box<
    dyn FnOnce(&AppHandle<R>, paddock_mcp::Emit) -> Result<Arc<paddock_mcp::McpControl>, BoxError>
        + Send,
>;
type ServiceSetup<R> = Box<dyn FnOnce(&AppHandle<R>) -> Result<ServicePart, BoxError> + Send>;
type AppSetup<R> = Box<dyn FnOnce(&AppHandle<R>) -> Result<(), BoxError> + Send>;
type MenuQuit<R> = dyn Fn(&AppHandle<R>) + Send + Sync;
type TraySetup<R> = Box<dyn FnOnce(&AppHandle<R>) -> Tray<R> + Send>;
type HealthAdjust<R> = Box<dyn FnOnce(&AppHandle<R>, HealthSetup<R>) -> HealthSetup<R> + Send>;

/// The settings stored at `path`, in memory when the app's folders are unknown.
fn settings_at(path: Option<std::path::PathBuf>) -> Settings {
    match path {
        Some(path) => Settings::open(path),
        None => Settings::in_memory(),
    }
}

/// The shell plugin with its parts (ADR 0012 §2: `paddock_shell::Builder::new(identity)...build()`).
pub struct Builder<R: Runtime> {
    identity: AppIdentity,
    app_commands: &'static Table,
    dialogs: Vec<&'static paddock_dialogs::DialogTable>,
    services: Vec<ServiceSpec>,
    health_setup: Option<HealthAdjust<R>>,
    settings: Option<Part<R, Settings>>,
    diagnostics: Option<DiagSetup<R>>,
    service: Option<ServiceSetup<R>>,
    first_run: Option<Part<R, FirstRun>>,
    files: Option<FilesSetup<R>>,
    storage: Option<StorageSetup<R>>,
    packs: Option<PacksSetup<R>>,
    channels: Option<ChannelsSetup<R>>,
    mcp: Option<McpSetup<R>>,
    sections: Vec<Arc<dyn Configurable>>,
    startup: Option<Startup<R>>,
    updater: Option<Part<R, UpdatesBuilder>>,
    windows: Option<Part<R, Windows<R>>>,
    downloader: Option<paddock_download::Downloader>,
    in_memory: bool,
    native_menu: bool,
    menu_quit: Option<Arc<MenuQuit<R>>>,
    setup: Vec<AppSetup<R>>,
    tray: Option<TraySetup<R>>,
    launch: Option<LaunchArgs>,
}

impl<R: Runtime> Builder<R> {
    /// The shell of the app `identity`.
    pub fn new(identity: AppIdentity) -> Builder<R> {
        Builder {
            identity,
            app_commands: &Table::EMPTY,
            dialogs: Vec::new(),
            services: Vec::new(),
            health_setup: None,
            settings: None,
            diagnostics: None,
            service: None,
            first_run: None,
            files: None,
            storage: None,
            packs: None,
            channels: None,
            mcp: None,
            sections: Vec::new(),
            startup: None,
            updater: None,
            windows: None,
            downloader: None,
            in_memory: false,
            native_menu: true,
            menu_quit: None,
            setup: Vec::new(),
            tray: None,
            launch: None,
        }
    }

    /// The app's own command table (it stays the app's `invoke_handler`): the dialog contracts and the manifest are
    /// checked against it and the shell's table together ([`Builder::problems`]).
    pub fn app_commands(mut self, table: &'static Table) -> Self {
        self.app_commands = table;
        self
    }

    /// The dialog tables of the app's own dialogs and of the non-shell features it uses (the workbench's), ADR 0026 §3.
    /// The shell's own tables ([`crate::SHELL_DIALOGS`]) are always registered; two tables declaring one id fail the
    /// build of the plugin.
    pub fn dialogs(mut self, tables: &[&'static paddock_dialogs::DialogTable]) -> Self {
        self.dialogs.extend_from_slice(tables);
        self
    }

    /// Every dialog table: the shell's, then the app's.
    fn dialog_tables(&self) -> Vec<&'static paddock_dialogs::DialogTable> {
        crate::SHELL_DIALOGS
            .iter()
            .copied()
            .chain(self.dialogs.iter().copied())
            .collect()
    }

    /// The app's services, in the order the status bar lists them (their repairs are the shell's code, rule 19).
    pub fn health(mut self, services: Vec<ServiceSpec>) -> Self {
        self.services = services;
        self
    }

    /// Adjusts the health set-up (the OS notification hook, caps, clock) before it is installed.
    pub fn health_setup(
        mut self,
        f: impl FnOnce(&AppHandle<R>, HealthSetup<R>) -> HealthSetup<R> + Send + 'static,
    ) -> Self {
        self.health_setup = Some(Box::new(f));
        self
    }

    /// The settings (default: `shell-settings.json` in the app's config folder, never the app's own `settings.json`).
    pub fn settings(mut self, f: impl FnOnce(&AppHandle<R>) -> Settings + Send + 'static) -> Self {
        self.settings = Some(Box::new(f));
        self
    }

    /// Whether [`Builder::diagnostics`] was called (an app's test of its composition: a test place must not set it up).
    pub fn diagnostics_installed(&self) -> bool {
        self.diagnostics.is_some()
    }

    /// Report a problem and the log's Settings: the app's sections, redaction rules and log handle
    /// (`Ok(paddock_diag::tauri::Setup::new(app_info, dirs).log(handle))`; an error fails the app's start naming this part). The shell registers the `pd.logging` settings section
    /// on the log handle, and the Report a Problem dialog and the webview's log command work from then on. Where the user saves
    /// the report is asked natively with the `native-dialogs` feature.
    pub fn diagnostics(
        mut self,
        f: impl FnOnce(&AppHandle<R>) -> Result<paddock_diag::tauri::Setup, BoxError> + Send + 'static,
    ) -> Self {
        self.diagnostics = Some(Box::new(f));
        self
    }

    /// The app's background service (plan X4): its manager, health entry and the app's own login item. The shell manages
    /// it (`service_status`, `service_repair`, `service_install`, `open_login_items` and the `paddock://service` event),
    /// keeps the health entry current with its Repair, runs the start-up checks (rule 24) and registers the `pd.login`
    /// "Start at login" section, whose switch registers or removes the service and the login item together (only a
    /// change by the user acts; the value at start is never applied).
    pub fn service(
        mut self,
        f: impl FnOnce(&AppHandle<R>) -> Result<ServicePart, BoxError> + Send + 'static,
    ) -> Self {
        self.service = Some(Box::new(f));
        self
    }

    /// The first-run set-up checklist (plan X19): the background service and Start at login when the app has them
    /// ([`Builder::service`]), then the steps `f` names (notification permission, Pitwall pairing, the app's own). The
    /// shell serves `setup_state`, `setup_act`, `setup_repair`, `setup_skip`, `setup_finish` and the `paddock://setup`
    /// event; the answers are kept in `setup.json` in the app's config folder. A step that cannot be built is a failed
    /// start naming it.
    pub fn first_run(mut self, f: impl FnOnce(&AppHandle<R>) -> FirstRun + Send + 'static) -> Self {
        self.first_run = Some(Box::new(f));
        self
    }

    /// The app's user files (plan X31): path grants, the pickers, recent lists, reveal and open. The shell manages the service
    /// (`paddock_files::Files::for_app(&dirs, &[recent::FILES, recent::PROJECTS])`), serves the `files_*` commands, grants files
    /// dropped on a window to it (`paddock://files-dropped`) and, with `native-dialogs`, shows the native dialogs when the
    /// service has no picker of its own. The app's own commands take `paddock_files::tauri::WindowGrants`.
    pub fn files(
        mut self,
        f: impl FnOnce(&AppHandle<R>) -> Result<paddock_files::Files, BoxError> + Send + 'static,
    ) -> Self {
        self.files = Some(Box::new(f));
        self
    }

    /// What the app keeps on disk (plan X32): its categories (`paddock_storage::Storage::new(source).within(&dirs)`). The shell manages
    /// the service, serves `storage_scan`, `storage_clear`, `storage_cancel`, `storage_last` and `storage_reveal` (jobs with
    /// progress and Cancel, the outcome on `paddock://storage`) and the Settings > Storage page reads them. Needs
    /// [`Builder::files`] for "Show in folder".
    pub fn storage(
        mut self,
        f: impl FnOnce(&AppHandle<R>) -> Result<paddock_storage::Storage, BoxError> + Send + 'static,
    ) -> Self {
        self.storage = Some(Box::new(f));
        self
    }

    /// The app's knowledge packs (plan X15): its pack kinds and, if it has one, the health entry that follows them. The
    /// shell manages them (`packs_status`, `packs_reload` and the `paddock://packs` event) and keeps the health entry
    /// current with its Repair (reload). Do the first load (`Reloader::reload_now`) before the closure returns, so the
    /// first look is real.
    pub fn packs(
        mut self,
        f: impl FnOnce(&AppHandle<R>) -> Result<paddock_packs::tauri::Setup, BoxError> + Send + 'static,
    ) -> Self {
        self.packs = Some(Box::new(f));
        self
    }

    /// The app's notification channels (plan X17): the closure builds the `ChannelsControl` (settings store, secrets, the
    /// `paddock-net` egress, clock) around the event sender it is given. The shell manages it (`channels_get`, `channels_save`,
    /// `channels_set_secret`, `channels_clear_secret`, `channels_test` and the `paddock://channels` event); only the secret
    /// dialog's window may store a secret.
    pub fn channels(
        mut self,
        f: impl FnOnce(
            &AppHandle<R>,
            paddock_channels::Emit,
        ) -> Result<Arc<paddock_channels::ChannelsControl>, BoxError>
        + Send
        + 'static,
    ) -> Self {
        self.channels = Some(Box::new(f));
        self
    }

    /// The app's AI access settings (plan X22, ADR 0007): the closure builds the `McpControl` (the MCP `Server` with its tools and
    /// the audit sink from `McpControl::announcing_sink`, the settings store, the secrets, the key store) around the event sender
    /// it is given. The shell manages it (`mcp_get`, `mcp_save`, `mcp_issue_key`, `mcp_revoke_key`, `mcp_audit`,
    /// `mcp_audit_clear` and the `paddock://mcp` event); only the key dialog's window may issue a key.
    pub fn mcp(
        mut self,
        f: impl FnOnce(
            &AppHandle<R>,
            paddock_mcp::Emit,
        ) -> Result<Arc<paddock_mcp::McpControl>, BoxError>
        + Send
        + 'static,
    ) -> Self {
        self.mcp = Some(Box::new(f));
        self
    }

    /// One of the app's settings sections (registered after the shell's own, in the order given).
    pub fn section(mut self, section: Arc<dyn Configurable>) -> Self {
        self.sections.push(section);
        self
    }

    /// The "Notifications" settings page (`pd.notifications`): the user's level, quiet hours and limits for OS
    /// notifications, applied to `settings`, which the app's `OsNotifier` reads. Wire the notifier in
    /// [`Builder::health_setup`] with `setup.os_notifier(...)` over the same `settings`.
    pub fn os_notifications(
        mut self,
        settings: paddock_health::notify::os::SharedOsSettings,
    ) -> Self {
        self.sections
            .push(crate::notifications::notifications_section(settings));
        self
    }

    /// The start-up contract and watchdog (default: `Startup::builder().build()`).
    pub fn startup(mut self, startup: Startup<R>) -> Self {
        self.startup = Some(startup);
        self
    }

    /// Updates, About and What's New. The update preferences become the shell's `pd.updates` settings section.
    pub fn updater(
        mut self,
        f: impl FnOnce(&AppHandle<R>) -> UpdatesBuilder + Send + 'static,
    ) -> Self {
        self.updater = Some(Box::new(f));
        self
    }

    /// The window layer (top-level windows, close setting, bounds, quit). Dialog windows then get its feel.
    pub fn windows(mut self, f: impl FnOnce(&AppHandle<R>) -> Windows<R> + Send + 'static) -> Self {
        self.windows = Some(Box::new(f));
        self
    }

    /// The app's one downloader (its host caps hold across every download, the updater's too).
    pub fn downloader(mut self, downloader: paddock_download::Downloader) -> Self {
        self.downloader = Some(downloader);
        self
    }

    /// Keeps settings and notifications in memory (tests and demos: nothing is written to the user's folders).
    pub fn in_memory(mut self) -> Self {
        self.in_memory = true;
        self
    }

    /// Whether the shell shows the macOS native menu (`menu_set`; default true). Tests turn it off: macOS builds menus
    /// on the main thread only, and a test's thread is not it.
    pub fn native_menu(mut self, on: bool) -> Self {
        self.native_menu = on;
        self
    }

    /// The app's own set-up, run after the shell's state is managed (in the order given). An error fails the app's
    /// start with that error. It runs inside the plugin's set-up, where Tauri holds its plugin store: it opens no window
    /// (one built here fails the start; building its webview would wait for the store forever). The shell opens the tray
    /// ([`Builder::tray`]) and the main window ([`Builder::launch`]) itself, after every plugin is set up.
    pub fn setup(
        mut self,
        f: impl FnOnce(&AppHandle<R>) -> Result<(), BoxError> + Send + 'static,
    ) -> Self {
        self.setup.push(Box::new(f));
        self
    }

    /// What a click on Quit in the menu bar does when no window is open (the last one was closed to the tray; the menu
    /// bar still shows its menus). Default: the shell's quit, the same path as the `app_quit` command (the window layer's
    /// exit with its teardown, or Tauri's exit without a window layer). An app with its own quit path gives it here. Any
    /// other item clicked with no window open brings the main window back, as a Dock click does.
    pub fn menu_quit_without_window(
        mut self,
        f: impl Fn(&AppHandle<R>) + Send + Sync + 'static,
    ) -> Self {
        self.menu_quit = Some(Arc::new(f));
        self
    }

    /// The app's tray: `f` builds it (backend, model, menu actions) at start, after every plugin is set up and before the
    /// main window, so `--hidden` can stay in it. Without a tray (or on a desktop without one) closing the last window
    /// quits.
    pub fn tray(mut self, f: impl FnOnce(&AppHandle<R>) -> Tray<R> + Send + 'static) -> Self {
        self.tray = Some(Box::new(f));
        self
    }

    /// The start's arguments: the shell opens the main window for them at start (none for `--hidden` with a tray), with
    /// the launch's files and links waiting in it. Needs [`Builder::windows`] and [`Builder::add_to`].
    pub fn launch(mut self, launch: LaunchArgs) -> Self {
        self.launch = Some(launch);
        self
    }

    /// Adds the shell plugin to `builder` (after the plugins already on it) and the start: the tray, then the main window,
    /// from Tauri's own `setup`, which runs after every plugin is set up (a window built inside a plugin hook deadlocks).
    /// A start that cannot open its window is logged and ends the app with code 1. This takes `builder`'s `setup`.
    pub fn add_to(mut self, builder: tauri::Builder<R>) -> tauri::Builder<R> {
        let (tray, launch) = (self.tray.take(), self.launch.take());
        builder.plugin(self.build()).setup(move |app| {
            started(
                app.handle(),
                open_at_start(app.handle(), tray, launch.as_ref()),
            );
            Ok(())
        })
    }

    /// The ids of the app's health services.
    pub fn service_ids(&self) -> Vec<&str> {
        self.services.iter().map(ServiceSpec::id).collect()
    }

    /// The ids of the app's own settings sections (not the shell's).
    pub fn section_ids(&self) -> Vec<String> {
        self.sections.iter().map(|s| s.section().id).collect()
    }

    /// Every problem of the app's parts against the shell: the dialog contracts against both tables, and the app's
    /// table against the shell's (a command name the shell already uses). Run it in a test of the app; empty when
    /// they agree. With `manifest` (the app's `paddock-manifest.json`), also [`crate::manifest::check`].
    pub fn problems(&self, manifest: Option<&str>) -> Vec<String> {
        let mut out = crate::manifest::table_problems(self.app_commands);
        match paddock_dialogs::Contracts::from_tables(&self.dialog_tables()) {
            Ok(c) => {
                out.extend(crate::manifest::contract_problems(&c, self.app_commands));
                if let Some(json) = manifest {
                    let own: Vec<&str> = self
                        .dialogs
                        .iter()
                        .flat_map(|t| t.entries().iter().map(|e| e.id))
                        .collect();
                    out.extend(crate::manifest::contract_drift(json, &c, &own));
                }
            }
            Err(e) => out.push(format!("the dialog tables do not fit together: {e}")),
        }
        if let Some(json) = manifest {
            let sections = self.section_ids();
            let expect = crate::manifest::Expect {
                app_commands: self.app_commands,
                services: &self.service_ids(),
                sections: &sections.iter().map(String::as_str).collect::<Vec<_>>(),
            };
            out.extend(crate::manifest::check(json, &expect));
        }
        out
    }

    /// The plugin. Its set-up manages the shell's state; a part that fails to set up fails the app's start with a
    /// message naming it.
    pub fn build(self) -> TauriPlugin<R> {
        let routes = Routes::new()
            .route(&OWN_COMMANDS, commands::invoke_handler())
            .route(
                &paddock_startup::COMMANDS,
                paddock_startup::tauri::invoke_handler(),
            )
            .route(
                &paddock_health::tauri::COMMANDS,
                paddock_health::tauri::invoke_handler(),
            )
            .route(
                &paddock_dialogs::COMMANDS,
                paddock_dialogs::tauri::invoke_handler(),
            )
            .route(
                &paddock_diag::COMMANDS,
                paddock_diag::tauri::invoke_handler(),
            )
            .route(
                &paddock_files::COMMANDS,
                paddock_files::tauri::invoke_handler(),
            )
            .route(
                &paddock_storage::COMMANDS,
                paddock_storage::tauri::invoke_handler(),
            )
            .route(
                &paddock_service::COMMANDS,
                paddock_service::tauri::invoke_handler(),
            )
            .route(
                &crate::setup::COMMANDS,
                crate::setup::tauri::invoke_handler(),
            )
            .route(
                &paddock_packs::COMMANDS,
                paddock_packs::tauri::invoke_handler(),
            )
            .route(
                &paddock_channels::COMMANDS,
                paddock_channels::tauri::invoke_handler(),
            )
            .route(&paddock_mcp::COMMANDS, paddock_mcp::tauri::invoke_handler())
            .route(
                &paddock_jobs::tauri::COMMANDS,
                paddock_jobs::tauri::invoke_handler(),
            )
            .route(
                &paddock_settings::tauri::COMMANDS,
                paddock_settings::tauri::invoke_handler(),
            )
            .route(
                &paddock_updater::COMMANDS,
                paddock_updater::tauri::invoke_handler(),
            )
            .route(
                &paddock_window::COMMANDS,
                paddock_window::tauri::invoke_handler(),
            )
            .route(
                &paddock_menu::COMMANDS,
                paddock_menu::tauri::invoke_handler(),
            );
        let handler = Guard::new(&COMMANDS)
            .dialog_grants(paddock_dialogs::tauri::grant_of)
            .wrap(routes.handler());
        // The tray and the launch open only through `add_to`: a shell built here with them would start without its window.
        let unopened = self.tray.is_some() || self.launch.is_some();
        let mut parts = Some(self);
        tauri::plugin::Builder::<R>::new(PLUGIN_NAME)
            .invoke_handler(handler)
            .setup(move |app, _api| match parts.take() {
                Some(_) if unopened => Err(
                    "tray/launch need Builder::add_to (the shell opens them from Tauri's own setup)".into(),
                ),
                Some(parts) => {
                    // No window may be built while Tauri holds its plugin store (it would deadlock).
                    let _scope = paddock_window::tauri::plugin_hook_scope();
                    parts.install(app).map_err(|e| e as Box<dyn Error>)
                }
                None => Ok(()),
            })
            // Tauri runs these hooks with its plugin store held: no window may be built in them (it would deadlock).
            .on_page_load(|webview, payload| {
                let _scope = paddock_window::tauri::plugin_hook_scope();
                paddock_startup::tauri::on_page_load(webview, payload)
            })
            .on_window_ready(|window| {
                let _scope = paddock_window::tauri::plugin_hook_scope();
                let w = window.clone();
                window.on_window_event(move |event| on_window_event(&w, event));
            })
            .on_event(on_run_event)
            .build()
    }

    /// The plugin's set-up: the shell's state in dependency order, then the app's own.
    fn install(self, app: &AppHandle<R>) -> Result<(), BoxError> {
        app.manage(self.identity.clone());
        paddock_jobs::tauri::manage(app);
        if let Some(d) = self.downloader {
            paddock_download::tauri::manage(app, d);
        }
        let mut health = HealthSetup::new(self.services);
        if !self.in_memory
            && let Ok(dir) = app.path().app_data_dir()
        {
            health = health.notifications_file(dir.join("notifications.json"));
        }
        if let Some(f) = self.health_setup {
            health = f(app, health);
        }
        paddock_health::tauri::init(app, health).map_err(|e| named("service health", e))?;

        let settings = match self.settings {
            Some(f) => f(app),
            None if self.in_memory => Settings::in_memory(),
            None => settings_at(crate::logging::shell_settings_path_for(&self.identity)),
        };
        let settings = paddock_settings::tauri::manage(app, settings);
        settings
            .register(Arc::new(paddock_settings::shell::appearance()))
            .map_err(|e| named("the Appearance settings", e))?;

        if let Some(f) = self.diagnostics {
            let native = native_picker(app);
            let setup = f(app).map_err(|e| named("Report a problem", e))?;
            let diagnostics = paddock_diag::tauri::install(app, setup, native);
            if let Some(handle) = diagnostics.log_handle() {
                settings
                    .register(paddock_diag::logging::logging_section(handle))
                    .map_err(|e| named("the Logging settings", e))?;
            }
        }

        if let Some(f) = self.files {
            let files = f(app).map_err(|e| named("user files", e))?;
            let files = if files.can_pick() {
                files
            } else {
                match native_files_picker(app) {
                    Some(picker) => files.picker(picker),
                    None => files,
                }
            };
            paddock_files::tauri::install(app, files);
        } else {
            // The shell serves the `files_*` commands to every app (About's releases link is `files_open_url`), so an app that
            // set up no user files gets the web-address-only service: no drops granted, no folders, no recent lists.
            log::info!("No user files set up by the app; the shell serves files_open_url only (web addresses)");
            paddock_files::tauri::install_open_url_only(app);
        }

        if let Some(f) = self.storage {
            let storage = f(app).map_err(|e| named("storage", e))?;
            paddock_storage::tauri::install(app, storage);
        }

        if let Some(f) = self.service {
            let part = f(app).map_err(|e| named("the background service", e))?;
            let service = paddock_service::tauri::install(app, part.setup);
            let control = service.control().clone();
            let autostart = Arc::new(std::sync::Mutex::new(part.autostart));
            let switch = paddock_service::login::LoginSwitch::default();
            let section =
                paddock_service::login::launch_at_login_section(&self.identity.name, true);
            settings
                .register(paddock_settings::section(section, move |values| {
                    let on = values
                        .bool(paddock_service::login::LAUNCH_AT_LOGIN)
                        .unwrap_or(false);
                    if switch.changed(on) {
                        // Registering is slow: never on the settings call's thread (rule 10).
                        control
                            .spawn_launch_at_login(on, autostart.clone())
                            .map_err(|e| -> paddock_settings::ApplyError {
                                format!("The change could not start: {e}").into()
                            })?;
                    }
                    Ok(())
                }))
                .map_err(|e| named("the Start at login settings", e))?;
        }

        if let Some(f) = self.packs {
            let setup = f(app).map_err(|e| named("the knowledge packs", e))?;
            paddock_packs::tauri::install(app, setup);
        }

        if let Some(f) = self.channels {
            paddock_channels::tauri::install(app, |emit| f(app, emit))
                .map_err(|e| named("the notification channels", e))?;
        }

        if let Some(f) = self.mcp {
            paddock_mcp::tauri::install(app, |emit| f(app, emit))
                .map_err(|e| named("the AI access settings", e))?;
        }

        let tables: Vec<_> = crate::SHELL_DIALOGS
            .iter()
            .copied()
            .chain(self.dialogs.iter().copied())
            .collect();
        let startup = match self.startup {
            Some(s) => s,
            None => Startup::builder()
                .build()
                .map_err(|e| named("the start-up watchdog", e))?,
        };
        // A dialog window that never sends its ready handshake is shown at the start-up config's deadline (ADR 0026 §4.3).
        let deadline = startup.with(|dog| dog.config().dialog_show_deadline);
        let dialogs = Dialogs::from_tables(&tables)
            .map_err(|e| named("the dialog contracts", e))?
            .with_show_deadline(deadline);
        app.manage(dialogs);
        app.manage(startup);

        if let Some(f) = self.windows {
            app.manage(f(app));
            app.manage(Windows::dialog_hooks(app));
        }
        if self.native_menu {
            paddock_menu::tauri::install(app);
        }
        if let Some(f) = self.updater {
            let updates =
                paddock_updater::tauri::install(app, f(app)).map_err(|e| named("updates", e))?;
            if let Some(section) = prefs::section(&updates.updater) {
                settings
                    .register(section)
                    .map_err(|e| named("the Updates settings", e))?;
                updates
                    .updater
                    .set_preference_store(Arc::new(prefs::SettingsPreferences::new(
                        settings.clone(),
                    )));
            }
        }
        for section in self.sections {
            let id = section.section().id;
            settings
                .register(section)
                .map_err(|e| named(&format!("the settings section {id}"), e))?;
        }
        if let Some(f) = self.first_run {
            let part = f(app);
            let mut items: Vec<Arc<dyn SetupItem>> = Vec::new();
            if let Some(service) = app.try_state::<Arc<paddock_service::tauri::Service>>() {
                items.push(Arc::new(ServiceItem::new(
                    &self.identity.name,
                    service.control().clone(),
                )));
                items.push(Arc::new(LoginItem::new(
                    &self.identity.name,
                    settings.clone(),
                )));
            }
            if let Some(access) = part.notifications {
                items.push(Arc::new(NotificationsItem::new(
                    &self.identity.name,
                    access,
                )));
            }
            if let Some((port, needs)) = part.pitwall {
                items.push(Arc::new(PitwallItem::new(&self.identity.name, needs, port)));
            }
            items.extend(part.extra);
            let dir = if self.in_memory {
                None
            } else {
                app.path().app_config_dir().ok()
            };
            crate::setup::tauri::install(app, items, dir)
                .map_err(|e| named("the set-up checklist", e))?;
        }
        for f in self.setup {
            f(app)?;
        }
        // After the app's own set-up, which may manage the menu state: clicks with no window open. The shell's policy
        // never replaces a handler the app set itself.
        if let Some(menu) = app.try_state::<paddock_menu::tauri::NativeMenu<R>>() {
            let quit = self.menu_quit;
            menu.on_no_window_by_default(move |app, id| {
                menu_without_window(app, id, quit.as_deref())
            });
        }
        Ok(())
    }
}

/// A click on a menu item with no window open (the last one was closed to the tray): Quit quits (`quit`, else the
/// shell's), any other item brings the main window back as a Dock click does. The item itself is not forwarded to the
/// new window: it is not listening yet, so the click would be lost; the person chooses again.
fn menu_without_window<R: Runtime>(app: &AppHandle<R>, id: &str, quit: Option<&MenuQuit<R>>) {
    menu_without_window_using(app, id, quit, |app| app.exit(0));
}

/// [`menu_without_window`] with the exit of an app without a window layer given (the mock runtime cannot exit).
fn menu_without_window_using<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    quit: Option<&MenuQuit<R>>,
    exit: impl FnOnce(&AppHandle<R>),
) {
    if id == paddock_menu::model::QUIT_COMMAND {
        match quit {
            Some(f) => f(app),
            None => quit_or(app, exit),
        }
    } else if app.try_state::<Windows<R>>().is_some() {
        reopen_after_hook(app);
    }
}

/// The shell's quit: the window layer's (every window closes, the exit watchdog is armed), as the `app_quit` command;
/// the given exit when there is no window layer.
fn quit_or<R: Runtime>(app: &AppHandle<R>, exit: impl FnOnce(&AppHandle<R>)) {
    if app.try_state::<Windows<R>>().is_some() {
        paddock_window::tauri::quit(app);
    } else {
        exit(app);
    }
}

/// A Dock click or a menu click with no window open (macOS): the window comes back after the hook, not inside it.
fn reopen_after_hook<R: Runtime>(app: &AppHandle<R>) {
    paddock_window::tauri::after_plugin_hook(app, paddock_window::tauri::reopen);
}

/// The tray, then the main window for `launch` (none for `--hidden` with a tray). Runs in Tauri's `setup`.
fn open_at_start<R: Runtime>(
    app: &AppHandle<R>,
    tray: Option<TraySetup<R>>,
    launch: Option<&LaunchArgs>,
) -> Result<(), WindowError> {
    if let Some(f) = tray {
        let tray = f(app);
        if !paddock_window::tauri::create_tray(app, tray) {
            log::info!("No tray icon; closing the last window quits");
        }
    }
    match launch {
        Some(launch) => paddock_window::tauri::start(app, launch),
        None => Ok(()),
    }
}

/// A start that could not open its window is logged and ends the app (code 1) instead of a panic in the event loop.
fn started<R: Runtime>(app: &AppHandle<R>, result: Result<(), WindowError>) {
    if let Err(e) = result {
        log::error!(Error = e.message.as_str(); "The app could not open its window and stops");
        paddock_window::tauri::exit_with(app, 1);
    }
}

/// What [`Builder::first_run`] adds to the shell's own steps.
#[derive(Default)]
pub struct FirstRun {
    notifications: Option<Arc<dyn NotificationAccess>>,
    pitwall: Option<(Arc<dyn PitwallPort>, paddock_pitwall::Caps)>,
    extra: Vec<Arc<dyn SetupItem>>,
}

impl FirstRun {
    /// No steps beyond the service and Start at login.
    pub fn new() -> FirstRun {
        FirstRun::default()
    }

    /// The notification-permission step over the OS (`paddock_health::tauri::native` has the real one).
    #[must_use]
    pub fn notifications(mut self, access: Arc<dyn NotificationAccess>) -> FirstRun {
        self.notifications = Some(access);
        self
    }

    /// The Pitwall step; `needs` is what the app asks of models.
    #[must_use]
    pub fn pitwall(mut self, port: Arc<dyn PitwallPort>, needs: paddock_pitwall::Caps) -> FirstRun {
        self.pitwall = Some((port, needs));
        self
    }

    /// One of the app's own steps (Debrief's tour), after the shell's.
    #[must_use]
    pub fn item(mut self, item: Arc<dyn SetupItem>) -> FirstRun {
        self.extra.push(item);
        self
    }
}

/// What [`Builder::service`] sets up.
pub struct ServicePart {
    setup: paddock_service::tauri::Setup,
    autostart: Box<dyn paddock_service::login::Autostart>,
}

impl ServicePart {
    /// The service (`paddock_service::tauri::Setup::new(manager).health("agent")`), with no login item of the app's own.
    pub fn new(setup: paddock_service::tauri::Setup) -> ServicePart {
        ServicePart {
            setup,
            autostart: Box::new(paddock_service::login::NoAutostart),
        }
    }

    /// The app's own login item (e.g. over `tauri-plugin-autostart`), switched together with the service.
    pub fn autostart(
        mut self,
        autostart: Box<dyn paddock_service::login::Autostart>,
    ) -> ServicePart {
        self.autostart = autostart;
        self
    }
}

/// The native open and save dialogs for user files (`native-dialogs`); without the feature the app's files name a picker.
#[cfg(feature = "native-dialogs")]
fn native_files_picker<R: Runtime>(app: &AppHandle<R>) -> Option<Arc<dyn paddock_files::Picker>> {
    Some(Arc::new(paddock_files::tauri::NativePicker::new(
        app.clone(),
    )))
}

#[cfg(not(feature = "native-dialogs"))]
fn native_files_picker<R: Runtime>(_: &AppHandle<R>) -> Option<Arc<dyn paddock_files::Picker>> {
    None
}

/// The native save dialog for reports (`native-dialogs`); without the feature the app's setup must name a picker.
#[cfg(feature = "native-dialogs")]
fn native_picker<R: Runtime>(
    app: &AppHandle<R>,
) -> Option<Arc<dyn paddock_diag::tauri::SavePicker>> {
    Some(Arc::new(paddock_diag::tauri::NativePicker::new(
        app.clone(),
    )))
}

#[cfg(not(feature = "native-dialogs"))]
fn native_picker<R: Runtime>(_: &AppHandle<R>) -> Option<Arc<dyn paddock_diag::tauri::SavePicker>> {
    None
}

/// A set-up error naming the part that failed.
fn named(part: &str, e: impl std::fmt::Display) -> BoxError {
    format!("The shell could not set up {part}: {e}").into()
}

/// Every window's events, for each part that follows them.
fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    paddock_dialogs::tauri::on_window_event(window, event);
    if window.try_state::<Windows<R>>().is_some() {
        paddock_window::tauri::on_window_event(window, event);
    }
    paddock_menu::tauri::on_window_event(window, event);
    paddock_files::tauri::on_window_event(window, event);
    let app = window.app_handle();
    match event {
        WindowEvent::Focused(true) => paddock_health::tauri::on_focus(app, window.label()),
        WindowEvent::Destroyed => paddock_health::tauri::forget_window(app, window.label()),
        _ => {}
    }
}

/// The app's run events: the window layer's exit (bounded teardown), and at exit the pending update and the messages.
fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    // Tauri calls this with its plugin store held: no window may be built here (it would deadlock).
    let _scope = paddock_window::tauri::plugin_hook_scope();
    if app.try_state::<Windows<R>>().is_some() {
        match event {
            // A file opened from Finder or a Dock click with no window open: these build windows, after the hook.
            #[cfg(target_os = "macos")]
            RunEvent::Opened { urls } => {
                let urls = urls.clone();
                paddock_window::tauri::after_plugin_hook(app, move |app| {
                    paddock_window::tauri::opened(app, &urls)
                });
            }
            #[cfg(target_os = "macos")]
            RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => reopen_after_hook(app),
            _ => paddock_window::tauri::on_run_event(app, event),
        }
    }
    if let RunEvent::Exit = event {
        // First: the last checkpoint and the removal of the running marker are what makes this a clean exit.
        paddock_startup::tauri::end_run(app);
        paddock_updater::tauri::on_exit(app);
        paddock_health::tauri::flush(app);
    }
}

#[cfg(test)]
mod tests;
