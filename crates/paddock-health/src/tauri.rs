//! The Tauri adapter of `paddock-health` (built with the `tauri` feature): service health and the notification
//! centre as managed state, their events to every window, and the commands with their table.
//!
//! - [`init`] manages the [`NotificationCentre`] and the app's [`Health`]: every health change is one
//!   `paddock://service-health` event to every window and one notification (posted on the outage, resolved on the
//!   recovery) in every window's list, with a `paddock://notifications` event each. A window that opens later asks
//!   with `health_snapshot` and `notifications_list`.
//! - [`post`] and [`resolve_cause`] are for the app's own producers (job outcomes, update offers, alerts).
//! - [`COMMANDS`] is the commands' table (`paddock-commands`) and [`invoke_handler`] their handler, so the shell's
//!   guard covers them: `Guard::new(&COMMANDS).wrap(invoke_handler())`.
//!
//! Windows are the shell's label scheme (`paddock_commands::labels`): a dialog or pop-out sees its host window's list.

use crate::error::HealthError;
use crate::health::Health;
use crate::model::{Change, SERVICE_HEALTH_EVENT, Snapshot, is_service_id};
use crate::notify::centre::{NewNotice, Notice, NoticeId, NoticeScope};
use crate::notify::health::{HealthNotice, for_change};
use crate::notify::os::OsNotifier;
use crate::notify::persist::NoticesRecord;
use crate::notify::window::{WindowNotice, WindowNoticeScope};
use crate::notify::{
    Caps, NOTIFICATIONS_EVENT, NoticesEvent, NoticesSnapshot, NotificationCentre, OPEN_EVENT,
    RestoredNotices,
};
use crate::registry::{ServiceSpec, SpecError};
use crate::{Clock, SystemClock};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager, Runtime, Webview};
use paddock_commands::labels::{class_of, resolve};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "os-notify")]
pub mod native;
mod os;

pub use os::{
    ActionHandler, ActionHandlers, Dispatched, dispatch, poll_digest, spawn_digest_ticker,
};

/// Shows an OS notification for a message (the app decides: its setting, focus, rate limit) and says whether it did.
/// Called on the main thread.
pub type OsNotify<R> = dyn Fn(&AppHandle<R>, &Notice) -> bool + Send + Sync;

/// What [`init`] sets up.
pub struct Setup<R: Runtime> {
    services: Vec<ServiceSpec>,
    file: Option<PathBuf>,
    clock: Arc<dyn Clock>,
    caps: Caps,
    os: Option<Arc<OsNotify<R>>>,
    notifier: Option<Arc<OsNotifier>>,
    handlers: ActionHandlers<R>,
    digest_every: Option<std::time::Duration>,
}

impl<R: Runtime> Setup<R> {
    /// The app's services (in the order the status bar lists them). Notifications stay in memory until
    /// [`Setup::notifications_file`] names their file.
    pub fn new(services: Vec<ServiceSpec>) -> Setup<R> {
        Setup {
            services,
            file: None,
            clock: Arc::new(SystemClock),
            caps: Caps::default(),
            os: None,
            notifier: None,
            handlers: ActionHandlers::default(),
            digest_every: None,
        }
    }

    /// The file of the app's messages (in the app's data folder).
    pub fn notifications_file(mut self, file: impl Into<PathBuf>) -> Setup<R> {
        self.file = Some(file.into());
        self
    }

    /// The clock of the messages (tests).
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Setup<R> {
        self.clock = clock;
        self
    }

    /// How many messages each list keeps.
    pub fn caps(mut self, caps: Caps) -> Setup<R> {
        self.caps = caps;
        self
    }

    /// Raises OS notifications through `notify` for posts that ask for one ([`post`] with `os`, and every outage).
    pub fn os_notifications(
        mut self,
        notify: impl Fn(&AppHandle<R>, &Notice) -> bool + Send + Sync + 'static,
    ) -> Setup<R> {
        self.os = Some(Arc::new(notify));
        self
    }

    /// Raises OS notifications through the shared policy ([`OsNotifier`]: level, seen-in-window, quiet hours, rate
    /// limits, buttons) and routes clicks on them: the toast opens its message, Repair runs in the shell, other buttons
    /// go to [`Setup::on_notice_action`] or to the window. The general hook is [`Setup::os_notifications`].
    pub fn os_notifier(mut self, notifier: Arc<OsNotifier>) -> Setup<R> {
        let n = notifier.clone();
        self.os = Some(Arc::new(move |app, notice| os::offer(app, &n, notice)));
        self.notifier = Some(notifier);
        self
    }

    /// Looks for the quiet-hours digest every `every` on a thread that ends with the app (quiet hours end by the clock,
    /// not by an event). Without it the digest also comes when a window gets the focus ([`poll_digest`]).
    pub fn os_digest_every(mut self, every: std::time::Duration) -> Setup<R> {
        self.digest_every = Some(every);
        self
    }

    /// The app's handler for a kind of action clicked on a toast (`viewJob`, `retry`...); see [`ActionHandler`].
    pub fn on_notice_action(
        mut self,
        kind: impl Into<String>,
        handler: impl Fn(&AppHandle<R>, &Notice, &crate::notify::centre::NoticeAction) -> bool
        + Send
        + Sync
        + 'static,
    ) -> Setup<R> {
        self.handlers.insert(kind, handler);
        self
    }
}

/// The OS notification hook, managed next to the centre.
struct OsHook<R: Runtime>(Option<Arc<OsNotify<R>>>);

/// Manages the notification centre and the app's health (announcing through every window and the centre). Call from
/// the app's `setup`; the health is returned for the supervisors (and managed as `Arc<Health>`).
pub fn init<R: Runtime>(app: &AppHandle<R>, setup: Setup<R>) -> Result<Arc<Health>, SpecError> {
    let Setup {
        services,
        file,
        clock,
        caps,
        os,
        notifier,
        handlers,
        digest_every,
    } = setup;
    let handle = app.clone();
    let health = Health::new(
        services,
        Box::new(move |snapshot, change| announce(&handle, snapshot, change)),
    )?;
    app.manage(Arc::new(NotificationCentre::with_caps(file, clock, caps)));
    app.manage(OsHook(os));
    app.manage(health.clone());
    if let Some(notifier) = notifier {
        os::install(app, &notifier, handlers);
        if let Some(every) = digest_every {
            os::spawn_digest_ticker(app, every);
        }
    }
    Ok(health)
}

/// A service changed: every window hears it, and the centre raises or resolves its one message.
fn announce<R: Runtime>(app: &AppHandle<R>, snapshot: &Snapshot, change: &Change) {
    warn_on_err(
        app.emit(SERVICE_HEALTH_EVENT, snapshot),
        "The service-health event could not be sent",
    );
    log::info!(
        target: "paddock_health",
        Service = change.service.as_str(),
        State = format!("{:?}", change.to).as_str();
        "Service health changed"
    );
    match for_change(snapshot, change) {
        HealthNotice::Post(n) => {
            post(app, None, *n, true);
        }
        HealthNotice::Resolve(cause) => resolve_cause(app, &cause),
        HealthNotice::Nothing => {}
    }
}

/// Logs a failed send (the app goes on: a window that missed an event asks again when it starts or retries).
fn warn_on_err(result: ::tauri::Result<()>, what: &str) {
    if let Err(e) = result {
        log::warn!(target: "paddock_health", "{what}: {e}");
    }
}

fn centre_of<R: Runtime>(app: &AppHandle<R>) -> Option<Arc<NotificationCentre>> {
    app.try_state::<Arc<NotificationCentre>>()
        .map(|c| c.inner().clone())
}

/// Every webview of the shell's label scheme, sorted (pages outside the scheme never hear notifications).
fn labels<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    let mut labels: Vec<String> = app
        .webview_windows()
        .into_keys()
        .filter(|l| class_of(l).is_some())
        .collect();
    labels.sort();
    labels
}

/// Sends `label` its list (the host window's, for a dialog or pop-out).
fn emit_list<R: Runtime>(
    app: &AppHandle<R>,
    centre: &NotificationCentre,
    label: &str,
    posted: Option<&Notice>,
) {
    let event = NoticesEvent {
        snapshot: centre.snapshot(resolve(label)),
        posted: posted.cloned(),
    };
    warn_on_err(
        app.emit_to(label, NOTIFICATIONS_EVENT, event),
        "The notifications event could not be sent",
    );
}

/// Tells the windows that show a change: every window for an app-wide one, else `window` and its dialogs and pop-outs.
fn emit_changed<R: Runtime>(
    app: &AppHandle<R>,
    centre: &NotificationCentre,
    window: Option<&str>,
    posted: Option<&Notice>,
) {
    for label in labels(app) {
        if window.is_none_or(|w| resolve(&label) == w) {
            emit_list(app, centre, &label, posted);
        }
    }
}

/// Posts a message for the app's producers and tells the windows that show it (the message itself for their toast,
/// unless it is quiet). `window` is the window it is for (a project message lives in its list); `os` also raises an OS
/// notification through the app's hook. Never fails: without the centre nothing happens.
pub fn post<R: Runtime>(
    app: &AppHandle<R>,
    window: Option<&str>,
    new: NewNotice,
    os: bool,
) -> Option<Notice> {
    let centre = centre_of(app)?;
    let window = window.map(resolve);
    let quiet = new.quiet;
    let posted = centre.post(window, new);
    let notice = posted.notice;
    if posted.unchanged {
        return Some(notice);
    }
    let toast = (!quiet).then_some(&notice);
    match notice.scope {
        NoticeScope::App => emit_changed(app, &centre, None, toast),
        NoticeScope::Project => emit_changed(app, &centre, window, toast),
    }
    if os {
        show_os(app, &centre, &notice, window);
    }
    Some(notice)
}

/// Raises the OS notification on the main thread (focus and the like are window getters, which wait for it).
fn show_os<R: Runtime>(
    app: &AppHandle<R>,
    centre: &Arc<NotificationCentre>,
    notice: &Notice,
    window: Option<&str>,
) {
    let Some(hook) = app.try_state::<OsHook<R>>().and_then(|h| h.0.clone()) else {
        return;
    };
    let (handle, centre, shown, window) = (
        app.clone(),
        centre.clone(),
        notice.clone(),
        window.map(str::to_string),
    );
    let ran = app.run_on_main_thread(move || {
        if hook(&handle, &shown) {
            centre.note_shown(shown.id, window.as_deref());
        }
    });
    warn_on_err(ran, "The OS notification could not be raised");
}

/// The cause cleared: removes its message (if any) and tells every window. Never fails.
pub fn resolve_cause<R: Runtime>(app: &AppHandle<R>, cause: &str) {
    let Some(centre) = centre_of(app) else {
        return;
    };
    if centre.resolve(cause) {
        emit_changed(app, &centre, None, None);
    }
}

/// A window got the focus: if that was the click on an OS notification, the window it was for (or this one) opens it.
pub fn on_focus<R: Runtime>(app: &AppHandle<R>, label: &str) {
    let Some(centre) = centre_of(app) else {
        return;
    };
    let pending = centre.take_pending();
    // The user is looking now: a due digest is listed in the centre, without a toast.
    poll_digest(app);
    if let Some((id, window)) = pending {
        let target = window
            .filter(|w| app.get_webview_window(w).is_some())
            .unwrap_or_else(|| label.to_string());
        warn_on_err(
            app.emit_to(target.as_str(), OPEN_EVENT, id),
            "The notification could not be opened",
        );
    }
}

/// A window closed: its project messages go with it.
pub fn forget_window<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if let Some(c) = centre_of(app) {
        c.forget_window(label);
    }
}

/// Writes the app's messages now (the app is closing).
pub fn flush<R: Runtime>(app: &AppHandle<R>) -> bool {
    centre_of(app).is_none_or(|c| c.flush())
}

fn health_of<R: Runtime>(app: &AppHandle<R>) -> Result<Arc<Health>, HealthError> {
    app.try_state::<Arc<Health>>()
        .map(|h| h.inner().clone())
        .ok_or_else(|| HealthError::unavailable("Service health"))
}

fn centre<R: Runtime>(app: &AppHandle<R>) -> Result<Arc<NotificationCentre>, HealthError> {
    centre_of(app).ok_or_else(|| HealthError::unavailable("The notification centre"))
}

/// Every service's state now. A window asks when it starts: a new window must not assume everything is fine.
#[::tauri::command]
pub async fn health_snapshot<R: Runtime>(app: AppHandle<R>) -> Result<Snapshot, HealthError> {
    Ok(health_of(&app)?.snapshot())
}

/// Repairs `service` from the shell (never through the service itself). Returns at once with the state `repairing`;
/// the outcome arrives as the usual event.
#[::tauri::command]
pub async fn health_repair<R: Runtime>(
    app: AppHandle<R>,
    service: String,
) -> Result<Snapshot, HealthError> {
    if !is_service_id(&service) {
        return Err(HealthError::invalid("that is not a service id"));
    }
    let health = health_of(&app)?;
    let started = health.repair(&service)?;
    log::info!(target: "paddock_health", Service = service.as_str(); "Repair started");
    Ok(started)
}

/// The calling window's messages (its project's and the app's).
#[::tauri::command]
pub async fn notifications_list<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
) -> Result<NoticesSnapshot, HealthError> {
    Ok(centre(&app)?.snapshot(resolve(webview.label())))
}

/// Marks one message read; every window hears the new count.
#[::tauri::command]
pub async fn notifications_mark_read<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
    id: u64,
) -> Result<bool, HealthError> {
    let c = centre(&app)?;
    let done = c.mark_read(resolve(webview.label()), NoticeId(id));
    emit_changed(&app, &c, None, None);
    Ok(done)
}

/// Marks the calling window's messages read; returns how many were unread.
#[::tauri::command]
pub async fn notifications_mark_all_read<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
) -> Result<usize, HealthError> {
    let c = centre(&app)?;
    let n = c.mark_all_read(resolve(webview.label()));
    emit_changed(&app, &c, None, None);
    Ok(n)
}

/// Removes one message.
#[::tauri::command]
pub async fn notifications_dismiss<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
    id: u64,
) -> Result<bool, HealthError> {
    let c = centre(&app)?;
    let done = c.dismiss(resolve(webview.label()), NoticeId(id));
    emit_changed(&app, &c, None, None);
    Ok(done)
}

/// The most ids one `notifications_dismiss_many` call takes (the centre's caps are far below this).
const MAX_DISMISS_IDS: usize = 1000;

/// Removes the listed messages (those the calling window cannot see are ignored); returns how many were removed. Logs
/// counts only.
#[::tauri::command]
pub async fn notifications_dismiss_many<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
    ids: Vec<u64>,
) -> Result<usize, HealthError> {
    if ids.len() > MAX_DISMISS_IDS {
        return Err(HealthError::invalid("too many messages to remove at once"));
    }
    let c = centre(&app)?;
    let ids: Vec<NoticeId> = ids.into_iter().map(NoticeId).collect();
    let n = c.dismiss_many(resolve(webview.label()), &ids);
    emit_changed(&app, &c, None, None);
    log::info!(target: "paddock_health", Requested = ids.len(), Removed = n; "Notifications removed");
    Ok(n)
}

/// Removes every message of the calling window (its project's and the app's); returns how many were removed. Logs the
/// count only.
#[::tauri::command]
pub async fn notifications_dismiss_all<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
) -> Result<usize, HealthError> {
    let c = centre(&app)?;
    let n = c.dismiss_all(resolve(webview.label()));
    emit_changed(&app, &c, None, None);
    log::info!(target: "paddock_health", Removed = n; "All notifications removed");
    Ok(n)
}

/// Posts a notice of the calling window (`SHELL_PORTS.notify`) to the centre: validated and bounded, keyed by the
/// engine's cause (`ui:<kind>:<title>`), so the same notice again counts up. An app-scope notice reaches every window,
/// a window-scope one the caller's window and its dialogs and pop-outs. Returns the message as listed.
#[::tauri::command]
pub async fn notifications_post<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
    notice: WindowNotice,
) -> Result<Notice, HealthError> {
    let window =
        (notice.scope() == WindowNoticeScope::Window).then(|| resolve(webview.label()).to_string());
    let new = notice.into_new()?;
    centre(&app)?;
    post(&app, window.as_deref(), new, false)
        .ok_or_else(|| HealthError::unavailable("The notification centre"))
}

/// The calling window's project messages for saving with its project (`NoticesRecord` v1).
#[::tauri::command]
pub async fn notifications_record<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
) -> Result<NoticesRecord, HealthError> {
    Ok(centre(&app)?.record(resolve(webview.label())))
}

/// Replaces the calling window's project messages with saved ones (a damaged or newer record gives none, with a
/// warning).
#[::tauri::command]
pub async fn notifications_restore<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
    record: Option<Value>,
) -> Result<RestoredNotices, HealthError> {
    let c = centre(&app)?;
    let window = resolve(webview.label());
    let warning = c.restore(window, record.as_ref());
    emit_changed(&app, &c, Some(window), None);
    Ok(RestoredNotices {
        snapshot: c.snapshot(window),
        warning,
    })
}

paddock_commands::command_table! {
    /// The health and notification commands: every window may ask and repair; a restored record may be large.
    pub static COMMANDS, macro handler;
    Query [WINDOW] => health_snapshot,
    Mutation [WINDOW] => health_repair,
    Query [WINDOW] => notifications_list,
    Mutation [WINDOW] => notifications_mark_read,
    Mutation [WINDOW] => notifications_mark_all_read,
    Mutation [WINDOW] => notifications_dismiss,
    Mutation [WINDOW] => notifications_dismiss_many,
    Mutation [WINDOW] => notifications_dismiss_all,
    Mutation [WINDOW] limit 16 * paddock_commands::KIB => notifications_post,
    Quiet [WINDOW] => notifications_record,
    Mutation [WINDOW] limit 4 * paddock_commands::MIB => notifications_restore,
}

/// The invoke handler of every command of [`COMMANDS`] (wrap it in the shell's guard).
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    handler!()
}

#[cfg(test)]
mod os_tests;
#[cfg(test)]
mod tests;
