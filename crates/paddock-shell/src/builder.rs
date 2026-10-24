//! [`Builder`]: the shell plugin with its parts. Everything is optional except the identity; a part the app leaves out
//! answers its commands with that crate's "not set up" error (degrade, don't fail), except the always-on ones: jobs,
//! the notification centre (window notices need it), settings with the shell's Appearance section, dialogs (an empty
//! contract file), the start-up watchdog with its defaults, and the menu.

use crate::{COMMANDS, OWN_COMMANDS, PLUGIN_NAME, commands, prefs};
use paddock_commands::Table;
use paddock_commands::tauri::{Guard, Routes};
use paddock_dialogs::Dialogs;
use paddock_health::ServiceSpec;
use paddock_health::tauri::Setup as HealthSetup;
use paddock_settings::{Configurable, Settings};
use paddock_startup::tauri::Startup;
use paddock_updater::tauri::UpdatesBuilder;
use paddock_window::tauri::Windows;
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

/// An empty contract file: no dialog of the app's own (the shell's dialogs still need the app's contract file).
const NO_CONTRACTS: &str = r#"{"version":1,"base":[],"dialogs":{}}"#;

type BoxError = Box<dyn Error + Send + Sync>;
type Part<R, T> = Box<dyn FnOnce(&AppHandle<R>) -> T + Send>;
type AppSetup<R> = Box<dyn FnOnce(&AppHandle<R>) -> Result<(), BoxError> + Send>;
type HealthAdjust<R> = Box<dyn FnOnce(&AppHandle<R>, HealthSetup<R>) -> HealthSetup<R> + Send>;

/// The shell plugin with its parts (ADR 0012 §2: `paddock_shell::Builder::new(identity)...build()`).
pub struct Builder<R: Runtime> {
    identity: AppIdentity,
    app_commands: &'static Table,
    contracts: Option<String>,
    services: Vec<ServiceSpec>,
    health_setup: Option<HealthAdjust<R>>,
    settings: Option<Part<R, Settings>>,
    diagnostics: Option<Part<R, paddock_diag::tauri::Setup>>,
    sections: Vec<Arc<dyn Configurable>>,
    startup: Option<Startup<R>>,
    updater: Option<Part<R, UpdatesBuilder>>,
    windows: Option<Part<R, Windows<R>>>,
    downloader: Option<paddock_download::Downloader>,
    in_memory: bool,
    native_menu: bool,
    setup: Vec<AppSetup<R>>,
}

impl<R: Runtime> Builder<R> {
    /// The shell of the app `identity`.
    pub fn new(identity: AppIdentity) -> Builder<R> {
        Builder {
            identity,
            app_commands: &Table::EMPTY,
            contracts: None,
            services: Vec::new(),
            health_setup: None,
            settings: None,
            diagnostics: None,
            sections: Vec::new(),
            startup: None,
            updater: None,
            windows: None,
            downloader: None,
            in_memory: false,
            native_menu: true,
            setup: Vec::new(),
        }
    }

    /// The app's own command table (it stays the app's `invoke_handler`): the dialog contracts and the manifest are
    /// checked against it and the shell's table together ([`Builder::problems`]).
    pub fn app_commands(mut self, table: &'static Table) -> Self {
        self.app_commands = table;
        self
    }

    /// The app's generated dialog contract file (`dialog-contracts.json`, ADR 0009). Without one only the shell's base
    /// commands reach dialogs.
    pub fn dialog_contracts(mut self, json: impl Into<String>) -> Self {
        self.contracts = Some(json.into());
        self
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

    /// The settings (default: `settings.json` in the app's config folder).
    pub fn settings(mut self, f: impl FnOnce(&AppHandle<R>) -> Settings + Send + 'static) -> Self {
        self.settings = Some(Box::new(f));
        self
    }

    /// Report a problem and the log's Settings: the app's sections, redaction rules and log handle
    /// (`paddock_diag::tauri::Setup::new(app_info, dirs).log(handle)`). The shell registers the `pd.logging` settings section
    /// on the log handle, and the Report a Problem dialog and the webview's log command work from then on. Where the user saves
    /// the report is asked natively with the `native-dialogs` feature.
    pub fn diagnostics(
        mut self,
        f: impl FnOnce(&AppHandle<R>) -> paddock_diag::tauri::Setup + Send + 'static,
    ) -> Self {
        self.diagnostics = Some(Box::new(f));
        self
    }

    /// One of the app's settings sections (registered after the shell's own, in the order given).
    pub fn section(mut self, section: Arc<dyn Configurable>) -> Self {
        self.sections.push(section);
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
    /// start with that error.
    pub fn setup(
        mut self,
        f: impl FnOnce(&AppHandle<R>) -> Result<(), BoxError> + Send + 'static,
    ) -> Self {
        self.setup.push(Box::new(f));
        self
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
        match paddock_dialogs::Contracts::parse(self.contracts.as_deref().unwrap_or(NO_CONTRACTS)) {
            Ok(c) => {
                out.extend(crate::manifest::contract_problems(&c, self.app_commands));
                if let Some(json) = manifest {
                    out.extend(crate::manifest::contract_drift(json, &c));
                }
            }
            Err(e) => out.push(format!("the dialog contract file is not valid: {e}")),
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
        let mut parts = Some(self);
        tauri::plugin::Builder::<R>::new(PLUGIN_NAME)
            .invoke_handler(handler)
            .setup(move |app, _api| match parts.take() {
                Some(parts) => parts.install(app).map_err(|e| e as Box<dyn Error>),
                None => Ok(()),
            })
            .on_page_load(|webview, payload| paddock_startup::tauri::on_page_load(webview, payload))
            .on_window_ready(|window| {
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
            None => match app.path().app_config_dir() {
                Ok(dir) => Settings::open(dir.join(paddock_settings::FILE_NAME)),
                Err(_) => Settings::in_memory(),
            },
        };
        let settings = paddock_settings::tauri::manage(app, settings);
        settings
            .register(Arc::new(paddock_settings::shell::appearance()))
            .map_err(|e| named("the Appearance settings", e))?;

        if let Some(f) = self.diagnostics {
            let native = native_picker(app);
            let diagnostics = paddock_diag::tauri::install(app, f(app), native);
            if let Some(handle) = diagnostics.log_handle() {
                settings
                    .register(paddock_diag::logging::logging_section(handle))
                    .map_err(|e| named("the Logging settings", e))?;
            }
        }

        let dialogs = Dialogs::from_json(self.contracts.as_deref().unwrap_or(NO_CONTRACTS))
            .map_err(|e| named("the dialog contracts", e))?;
        app.manage(dialogs);

        let startup = match self.startup {
            Some(s) => s,
            None => Startup::builder()
                .build()
                .map_err(|e| named("the start-up watchdog", e))?,
        };
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
        for f in self.setup {
            f(app)?;
        }
        Ok(())
    }
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
    let app = window.app_handle();
    match event {
        WindowEvent::Focused(true) => paddock_health::tauri::on_focus(app, window.label()),
        WindowEvent::Destroyed => paddock_health::tauri::forget_window(app, window.label()),
        _ => {}
    }
}

/// The app's run events: the window layer's exit (bounded teardown), and at exit the pending update and the messages.
fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    if app.try_state::<Windows<R>>().is_some() {
        paddock_window::tauri::on_run_event(app, event);
    }
    if let RunEvent::Exit = event {
        paddock_updater::tauri::on_exit(app);
        paddock_health::tauri::flush(app);
    }
}
