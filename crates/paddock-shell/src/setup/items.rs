//! The shell's four steps, each over a small port so the tests never reach the OS (rule 7): the background service
//! (`paddock-service`), start at login (the `pd.login` setting), notification permission (the OS notifier's
//! `Toaster::permission`) and Pitwall pairing (`paddock-pitwall`'s [`Link`]). An app adds its own steps (Debrief's tour)
//! with its own [`SetupItem`]s.

use super::model::{Check, SetupItem};
use paddock_health::notify::os::Permission;
use paddock_pitwall::{AiState, Caps, Link, LinkView, NeedsAiView};
use paddock_service::LOGIN_SECTION;
use paddock_service::control::{Request, ServiceControl};
use paddock_service::login::LAUNCH_AT_LOGIN;
use paddock_service::status::{Condition, ServiceStatus};
use paddock_settings::Settings;
use paddock_siblings::{Handoff, Outcome, Siblings};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// The ids of the shell's steps.
pub mod ids {
    /// The background service.
    pub const SERVICE: &str = "service";
    /// Start at login.
    pub const LOGIN: &str = "login";
    /// Notification permission.
    pub const NOTIFICATIONS: &str = "notifications";
    /// Pitwall pairing.
    pub const PITWALL: &str = "pitwall";
}

// ---------------------------------------------------------------------------------------------------------------------
// The background service

/// What the service step needs of the background service.
pub trait ServicePort: Send + Sync {
    /// The service now; `None` when the app has none.
    fn status(&self) -> Option<ServiceStatus>;
    /// Registers it (returns at once; the outcome is read back).
    fn install(&self) -> Result<(), String>;
    /// Repairs it (returns at once).
    fn repair(&self) -> Result<(), String>;
    /// Opens the OS page of the user's login switch; false where there is none.
    fn open_login_items(&self) -> Result<bool, String>;
}

impl ServicePort for ServiceControl {
    fn status(&self) -> Option<ServiceStatus> {
        ServiceControl::status(self)
    }
    fn install(&self) -> Result<(), String> {
        self.run(Request::Install)
            .map(drop)
            .map_err(|e| e.to_string())
    }
    fn repair(&self) -> Result<(), String> {
        self.run(Request::Repair)
            .map(drop)
            .map_err(|e| e.to_string())
    }
    fn open_login_items(&self) -> Result<bool, String> {
        ServiceControl::open_login_items(self).map_err(|e| e.message)
    }
}

/// "Let the app keep a small helper running in the background" (the user's consent to the service, plan X4).
pub struct ServiceItem {
    app: String,
    port: Arc<dyn ServicePort>,
}

impl ServiceItem {
    /// The step for the app called `app`.
    pub fn new(app: &str, port: Arc<dyn ServicePort>) -> ServiceItem {
        ServiceItem {
            app: app.to_string(),
            port,
        }
    }

    fn status(&self) -> Result<ServiceStatus, String> {
        self.port
            .status()
            .ok_or_else(|| "The background service could not be asked how it is doing.".to_string())
    }
}

impl SetupItem for ServiceItem {
    fn id(&self) -> &str {
        ids::SERVICE
    }
    fn title(&self) -> String {
        "Background service".into()
    }
    fn action_label(&self) -> String {
        "Install".into()
    }
    fn detail(&self) -> String {
        format!(
            "A small helper that keeps {} working while its window is closed. You can switch it off at any time.",
            self.app
        )
    }
    fn check(&self) -> Check {
        let Some(s) = self.port.status() else {
            return Check::NotApplicable;
        };
        match s.condition {
            Condition::Running => Check::Done,
            Condition::Unavailable => Check::NotApplicable,
            Condition::NotInstalled | Condition::Repairing => Check::Todo,
            _ => Check::Problem(
                s.message
                    .unwrap_or_else(|| "The background service is not running.".into()),
            ),
        }
    }
    fn act(&self) -> Result<(), String> {
        let s = self.status()?;
        if s.can_install {
            self.port.install()
        } else if s.can_repair {
            self.port.repair()
        } else {
            self.repair()
        }
    }
    fn repair(&self) -> Result<(), String> {
        let s = self.status()?;
        if s.can_open_login_items {
            // The user's switch in Login Items stays the user's: open the page, never flip it.
            return match self.port.open_login_items()? {
                true => Ok(()),
                false => Err(
                    "This system has no Login Items page to open. Look for the app in its startup settings."
                        .into(),
                ),
            };
        }
        if s.can_repair {
            self.port.repair()
        } else if s.can_install {
            self.port.install()
        } else {
            Err(s
                .message
                .unwrap_or_else(|| "Nothing can be repaired from here.".into()))
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Start at login

/// "Open the app when I log in": the `pd.login` setting, which registers the login item and the service together.
pub struct LoginItem {
    app: String,
    settings: Arc<Settings>,
}

impl LoginItem {
    /// The step for the app called `app`, over its settings.
    pub fn new(app: &str, settings: Arc<Settings>) -> LoginItem {
        LoginItem {
            app: app.to_string(),
            settings,
        }
    }
}

impl SetupItem for LoginItem {
    fn id(&self) -> &str {
        ids::LOGIN
    }
    fn title(&self) -> String {
        "Start at login".into()
    }
    fn action_label(&self) -> String {
        "Turn on".into()
    }
    fn detail(&self) -> String {
        format!(
            "Open {} in the menu bar or tray when you log in to this computer.",
            self.app
        )
    }
    fn check(&self) -> Check {
        match self.settings.values(LOGIN_SECTION) {
            None => Check::NotApplicable,
            Some(v) if v.bool(LAUNCH_AT_LOGIN) == Some(true) => Check::Done,
            Some(_) => Check::Todo,
        }
    }
    fn act(&self) -> Result<(), String> {
        let result = self
            .settings
            .set(LOGIN_SECTION, &json!({ LAUNCH_AT_LOGIN: true }))
            .map_err(|e| e.to_string())?;
        if result.applied {
            Ok(())
        } else {
            Err(result
                .errors
                .into_iter()
                .map(|e| e.message)
                .collect::<Vec<_>>()
                .join(" "))
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Notification permission

/// What the notification step needs of the OS.
pub trait NotificationAccess: Send + Sync {
    /// What the OS allows now.
    fn permission(&self) -> Permission;
    /// Asks the OS (it may show its prompt once) and answers what it allows now.
    fn request(&self) -> Permission;
    /// Opens the system's notification settings, where a refusal is undone.
    fn open_system_settings(&self) -> Result<(), String>;
    /// Whether this system asks permission at all (macOS does; Windows and Linux notify without asking).
    fn asks(&self) -> bool {
        cfg!(target_os = "macos")
    }
}

/// [`NotificationAccess`] over the OS notifier's [`Toaster`](paddock_health::notify::os::Toaster): the permission, the
/// request (the plugin's `request_permission`) and opening the system's notification settings (macOS, Windows and GNOME)
/// are the toaster's own, so an app wires nothing. The closures replace one of them (an app with a platform page of its own).
pub struct ToasterAccess {
    toaster: Arc<dyn paddock_health::notify::os::Toaster>,
    request: Option<Box<dyn Fn() -> Permission + Send + Sync>>,
    open: Option<Box<dyn Fn() -> Result<(), String> + Send + Sync>>,
}

impl ToasterAccess {
    /// Access over `toaster`.
    pub fn new(toaster: Arc<dyn paddock_health::notify::os::Toaster>) -> ToasterAccess {
        ToasterAccess {
            toaster,
            request: None,
            open: None,
        }
    }

    /// Replaces how the app asks the OS for permission.
    #[must_use]
    pub fn request_with(mut self, f: impl Fn() -> Permission + Send + Sync + 'static) -> Self {
        self.request = Some(Box::new(f));
        self
    }

    /// Replaces how the app opens the system's notification settings.
    #[must_use]
    pub fn open_settings_with(
        mut self,
        f: impl Fn() -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.open = Some(Box::new(f));
        self
    }
}

impl NotificationAccess for ToasterAccess {
    fn permission(&self) -> Permission {
        self.toaster.permission()
    }
    fn request(&self) -> Permission {
        match &self.request {
            Some(f) => f(),
            None => self.toaster.request_permission(),
        }
    }
    fn open_system_settings(&self) -> Result<(), String> {
        match &self.open {
            Some(f) => f(),
            None => self.toaster.open_system_settings().map_err(|e| e.0),
        }
    }
}

/// "Allow notifications": policy, quiet hours and limits are the Notifications settings; this is only the OS's yes.
pub struct NotificationsItem {
    app: String,
    access: Arc<dyn NotificationAccess>,
    asked: AtomicBool,
}

impl NotificationsItem {
    /// The step for the app called `app`.
    pub fn new(app: &str, access: Arc<dyn NotificationAccess>) -> NotificationsItem {
        NotificationsItem {
            app: app.to_string(),
            access,
            asked: AtomicBool::new(false),
        }
    }
}

impl SetupItem for NotificationsItem {
    fn id(&self) -> &str {
        ids::NOTIFICATIONS
    }
    fn title(&self) -> String {
        "Notifications".into()
    }
    fn action_label(&self) -> String {
        "Allow".into()
    }
    fn detail(&self) -> String {
        format!(
            "Let {} tell you when something needs you while another window is in front. You choose which ones in Settings.",
            self.app
        )
    }
    fn check(&self) -> Check {
        if !self.access.asks() {
            return Check::NotApplicable;
        }
        match self.access.permission() {
            Permission::Granted => Check::Done,
            Permission::Denied => Check::Problem(
                "Notifications are switched off for this app in the system settings. Repair opens them; the app cannot switch them on for you."
                    .into(),
            ),
            Permission::Unknown if self.asked.load(Ordering::SeqCst) => Check::Done,
            Permission::Unknown => Check::Todo,
        }
    }
    fn act(&self) -> Result<(), String> {
        self.asked.store(true, Ordering::SeqCst);
        match self.access.request() {
            Permission::Denied => Err(
                "The system did not allow notifications. Open the system settings to change that."
                    .into(),
            ),
            _ => Ok(()),
        }
    }
    fn repair(&self) -> Result<(), String> {
        self.access.open_system_settings()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Pitwall

/// What the Pitwall step needs of the connection.
pub trait PitwallPort: Send + Sync {
    /// Looks at Pitwall now (bounded).
    fn probe(&self) -> LinkView;
    /// Starts the consented pairing; the code to compare is in the next look.
    fn connect(&self) -> Result<(), String>;
    /// Starts Pitwall (the app opens it; see `paddock-siblings`).
    fn start(&self) -> Result<(), String>;
}

/// [`PitwallPort`] over the app's [`Link`]. Starting Pitwall opens its app through `paddock-siblings` (a typed `Open`
/// hand-off), so an app wires nothing; "Pitwall is not installed" and "needs an update" come back as the step's words.
pub struct LinkPort {
    link: Arc<Link>,
    starter: Box<dyn Fn() -> Result<(), String> + Send + Sync>,
}

/// Opens Pitwall's app through `siblings`; its plain answers (not installed, needs an update) are the error text.
pub fn start_pitwall(siblings: &Siblings) -> Result<(), String> {
    match siblings.open("pitwall", &Handoff::Open) {
        Ok(Outcome::Opened { .. }) => Ok(()),
        Ok(Outcome::NotInstalled { message } | Outcome::NeedsUpdate { message }) => Err(message),
        Err(e) => Err(e.to_string()),
    }
}

impl LinkPort {
    /// The port over `link` for the app `app_id` (its id in the family: `debrief`, `spotter`…); Pitwall is started with
    /// the real machine's siblings. An app outside the family gets a step that says to open Pitwall by hand.
    pub fn new(link: Arc<Link>, app_id: &str) -> LinkPort {
        match Siblings::native(app_id) {
            Ok(s) => LinkPort::with_siblings(link, Arc::new(s)),
            Err(e) => {
                let why = e.to_string();
                LinkPort {
                    link,
                    starter: Box::new(move || {
                        Err(format!(
                            "{why} Open Pitwall to start it, then come back here."
                        ))
                    }),
                }
            }
        }
    }

    /// The port over `link` that starts Pitwall through `siblings`.
    pub fn with_siblings(link: Arc<Link>, siblings: Arc<Siblings>) -> LinkPort {
        LinkPort {
            link,
            starter: Box::new(move || start_pitwall(&siblings)),
        }
    }

    /// Replaces how the app starts Pitwall.
    #[must_use]
    pub fn starter(mut self, f: impl Fn() -> Result<(), String> + Send + Sync + 'static) -> Self {
        self.starter = Box::new(f);
        self
    }
}

impl PitwallPort for LinkPort {
    fn probe(&self) -> LinkView {
        self.link.probe()
    }
    fn connect(&self) -> Result<(), String> {
        self.link.connect(true).map(drop).map_err(|e| e.to_string())
    }
    fn start(&self) -> Result<(), String> {
        (self.starter)()
    }
}

/// "Connect to Pitwall": optional, so a missing Pitwall is a step to skip, never an error (ADR 0004).
pub struct PitwallItem {
    app: String,
    needs: Caps,
    port: Arc<dyn PitwallPort>,
    last: Mutex<LinkView>,
}

impl PitwallItem {
    /// The step for the app called `app`, which asks models for `needs`.
    pub fn new(app: &str, needs: Caps, port: Arc<dyn PitwallPort>) -> PitwallItem {
        PitwallItem {
            app: app.to_string(),
            needs,
            port,
            last: Mutex::new(LinkView::default()),
        }
    }

    fn look(&self) -> (LinkView, AiState) {
        let view = self.port.probe();
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) = view.clone();
        let state = AiState::from_view(&view, self.needs, false);
        (view, state)
    }

    fn last_state(&self) -> (LinkView, AiState) {
        let view = self
            .last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let state = AiState::from_view(&view, self.needs, false);
        (view, state)
    }
}

impl SetupItem for PitwallItem {
    fn id(&self) -> &str {
        ids::PITWALL
    }
    fn title(&self) -> String {
        "Connect to Pitwall".into()
    }
    fn action_label(&self) -> String {
        "Connect".into()
    }
    fn detail(&self) -> String {
        format!(
            "Pitwall is the local AI server of the family. Connect {} to use it for AI features; nothing leaves this computer. This step is optional.",
            self.app
        )
    }
    fn check(&self) -> Check {
        let (_, state) = self.look();
        match state {
            AiState::Ready { .. } => Check::Done,
            AiState::NotInstalled | AiState::NotRunning | AiState::NeedsPairing => Check::Todo,
            other => Check::Problem(other.message(&self.app)),
        }
    }
    fn act(&self) -> Result<(), String> {
        let (_, state) = self.look();
        match state {
            AiState::NeedsPairing => self.port.connect(),
            AiState::NotRunning | AiState::Paused { .. } => self.port.start(),
            AiState::Ready { .. } => Ok(()),
            other => Err(other.message(&self.app)),
        }
    }
    fn hint(&self) -> Option<String> {
        let (view, _) = self.last_state();
        view.pairing.map(|p| {
            format!(
                "Compare the code {} in Pitwall and approve {} there.",
                p.code, self.app
            )
        })
    }
    fn needs_ai(&self) -> Option<NeedsAiView> {
        let (_, state) = self.last_state();
        NeedsAiView::for_state("Local AI", &state, &self.app)
    }
}
