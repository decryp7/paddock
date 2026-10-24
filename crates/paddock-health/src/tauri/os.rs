//! The Tauri side of the OS notification bridge (plan X8): which windows the user sees, the hook that offers a posted
//! message to the [`OsNotifier`], and what a click on a toast does: open the message, repair a service from the shell,
//! run the app's handler for an action, or hand the action to the window that shows the message.

use super::{centre_of, emit_changed, emit_list, labels, warn_on_err};
use crate::health::Health;
use crate::notify::centre::{NewNotice, NoticeScope, NoticeSeverity};
use crate::notify::centre::{Notice, NoticeAction, NoticeId};
use crate::notify::os::{Click, DIGEST_CAUSE, OsNotifier, Route, WindowPresence, route, user_sees};
use crate::notify::{ACTION_EVENT, NoticeActionEvent, OPEN_EVENT};
use ::tauri::{AppHandle, Emitter, Manager, Runtime};
use paddock_commands::labels::resolve;
use std::collections::HashMap;
use std::sync::Arc;

/// The app's own handler for a kind of action (`viewJob`, `retryDownload`...): returns whether it handled it. Called on
/// the OS's callback thread, never the main thread, so it may call the app's commands; it must not block for long.
pub type ActionHandler<R> = dyn Fn(&AppHandle<R>, &Notice, &NoticeAction) -> bool + Send + Sync;

/// The handlers by action kind.
pub struct ActionHandlers<R: Runtime>(HashMap<String, Arc<ActionHandler<R>>>);

impl<R: Runtime> Default for ActionHandlers<R> {
    fn default() -> Self {
        ActionHandlers(HashMap::new())
    }
}

impl<R: Runtime> ActionHandlers<R> {
    /// Runs `handler` for actions of `kind` clicked on a toast.
    pub fn insert(
        &mut self,
        kind: impl Into<String>,
        handler: impl Fn(&AppHandle<R>, &Notice, &NoticeAction) -> bool + Send + Sync + 'static,
    ) {
        self.0.insert(kind.into(), Arc::new(handler));
    }

    fn get(&self, kind: &str) -> Option<Arc<ActionHandler<R>>> {
        self.0.get(kind).cloned()
    }
}

/// What a click ended up doing (tests assert it; the OS callback ignores it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dispatched {
    /// The message was opened in its window.
    Opened(NoticeId),
    /// A repair of the service started in the shell.
    Repairing(String),
    /// The app's handler did it.
    Handled(String),
    /// The window got the action to run.
    Forwarded(NoticeId),
    /// The repair was refused or the message is gone: the window was brought forward, nothing else.
    Nothing,
}

/// The windows as the user sees them now (a dialog or pop-out counts for its host). Windows outside the shell's label
/// scheme are not part of the app's UI. Call on the main thread: window getters wait for it.
pub(super) fn presence<R: Runtime>(app: &AppHandle<R>) -> Vec<WindowPresence> {
    let windows = app.webview_windows();
    labels(app)
        .into_iter()
        .filter_map(|label| {
            let w = windows.get(&label)?;
            Some(WindowPresence {
                host: resolve(&label).to_string(),
                focused: w.is_focused().unwrap_or(false),
                visible: w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false),
            })
        })
        .collect()
}

/// The hook of [`Setup::os_notifier`](super::Setup::os_notifier): offers the message to the notifier, with whether the
/// user already sees it.
pub(super) fn offer<R: Runtime>(
    app: &AppHandle<R>,
    notifier: &OsNotifier,
    notice: &Notice,
) -> bool {
    let seen = user_sees(notice.link.window.as_deref().map(resolve), &presence(app));
    notifier.offer(notice, seen).shown()
}

/// Brings the window forward (`label`, else the main window, else any).
fn focus<R: Runtime>(app: &AppHandle<R>, label: Option<&str>) -> Option<String> {
    let windows = app.webview_windows();
    let target = label
        .filter(|l| windows.contains_key(*l))
        .map(str::to_string)
        .or_else(|| windows.contains_key("main").then(|| "main".to_string()))
        .or_else(|| labels(app).into_iter().next())?;
    let w = windows.get(&target)?;
    warn_on_err(w.show(), "The window could not be shown");
    warn_on_err(w.unminimize(), "The window could not be restored");
    warn_on_err(w.set_focus(), "The window could not be focused");
    Some(target)
}

/// Does what a click on a toast means. The message is read from the centre again; the OS's payload only names it.
pub fn dispatch<R: Runtime>(
    app: &AppHandle<R>,
    handlers: &ActionHandlers<R>,
    click: Click,
) -> Dispatched {
    let Some(centre) = centre_of(app) else {
        return Dispatched::Nothing;
    };
    // The click is the answer; the "focused soon after a toast" fallback must not open the message a second time.
    centre.take_pending();
    let Some(notice) = centre.find(click.notice) else {
        focus(app, None);
        return Dispatched::Nothing;
    };
    let window = notice.link.window.clone();
    let host = window.as_deref().map(resolve).map(str::to_string);
    let read = |app: &AppHandle<R>| {
        if let Some(h) = host.as_deref() {
            centre.mark_read(h, notice.id);
        } else {
            centre.mark_read("main", notice.id);
        }
        emit_changed(app, &centre, None, None);
    };
    match route(&notice, click.target) {
        Route::Open(id) => {
            if let Some(target) = focus(app, host.as_deref()) {
                read(app);
                warn_on_err(
                    app.emit_to(target.as_str(), OPEN_EVENT, id),
                    "The notification could not be opened",
                );
            }
            Dispatched::Opened(id)
        }
        Route::Repair(service) => {
            let Some(health) = app.try_state::<Arc<Health>>().map(|h| h.inner().clone()) else {
                return Dispatched::Nothing;
            };
            match health.repair(&service) {
                Ok(_) => {
                    log::info!(target: "paddock_health", Service = service.as_str(); "Repair started from a notification");
                    read(app);
                    Dispatched::Repairing(service)
                }
                Err(e) => {
                    log::warn!(target: "paddock_health", "A repair from a notification was refused: {e}");
                    focus(app, host.as_deref());
                    Dispatched::Nothing
                }
            }
        }
        Route::Forward(action) => {
            if let Some(handler) = handlers.get(&action.kind)
                && handler(app, &notice, &action)
            {
                read(app);
                return Dispatched::Handled(action.kind);
            }
            let Some(target) = focus(app, host.as_deref()) else {
                return Dispatched::Nothing;
            };
            let event = NoticeActionEvent {
                id: notice.id,
                action,
            };
            warn_on_err(
                app.emit_to(target.as_str(), ACTION_EVENT, event),
                "The notification action could not be sent",
            );
            emit_list(app, &centre, &target, None);
            Dispatched::Forwarded(notice.id)
        }
        Route::Nothing => {
            focus(app, host.as_deref());
            Dispatched::Nothing
        }
    }
}

/// Starts listening for clicks on this app's toasts.
pub(super) fn install<R: Runtime>(
    app: &AppHandle<R>,
    notifier: &Arc<OsNotifier>,
    handlers: ActionHandlers<R>,
) {
    app.manage(Managed(notifier.clone()));
    let (handle, handlers) = (app.clone(), Arc::new(handlers));
    notifier.toaster().subscribe(Arc::new(move |click| {
        dispatch(&handle, &handlers, click);
    }));
}

/// The notifier, managed so the digest can be polled from anywhere in the shell.
pub(super) struct Managed(pub(super) Arc<OsNotifier>);

/// Posts and shows the quiet-hours digest when quiet hours are over and notifications were held ("N notifications held
/// during quiet hours"), at most as often as the limits allow. Called when a window gets the focus and by [`spawn_digest_ticker`]. Returns whether a toast was shown. Never fails.
pub fn poll_digest<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(notifier) = app.try_state::<Managed>().map(|m| m.0.clone()) else {
        return false;
    };
    let Some(count) = notifier.pending_digest() else {
        return false;
    };
    let title = if count == 1 {
        "1 notification held during quiet hours".to_string()
    } else {
        format!("{count} notifications held during quiet hours")
    };
    let new = NewNotice::new(
        DIGEST_CAUSE,
        "app",
        NoticeSeverity::Info,
        title,
        NoticeScope::App,
    )
    .body("They are in the notification centre.")
    .quiet();
    let Some(notice) = super::post(app, None, new, false) else {
        return false;
    };
    let seen = user_sees(None, &presence(app));
    let shown = notifier.send_digest(&notice, seen).shown();
    if shown && let Some(centre) = centre_of(app) {
        centre.note_shown(notice.id, None);
    }
    shown
}

/// Starts a thread that polls the digest every `every` while the app lives (it ends with the notifier). Quiet hours end
/// by the clock, not by an event, so something has to look.
pub fn spawn_digest_ticker<R: Runtime>(app: &AppHandle<R>, every: std::time::Duration) {
    let (app, weak) = (
        app.clone(),
        app.try_state::<Managed>().map(|m| Arc::downgrade(&m.0)),
    );
    let Some(weak) = weak else { return };
    let spawned = std::thread::Builder::new()
        .name("os-digest".into())
        .spawn(move || {
            loop {
                std::thread::park_timeout(every);
                if weak.upgrade().is_none() {
                    return;
                }
                poll_digest(&app);
            }
        });
    if let Err(e) = spawned {
        log::warn!(target: "paddock_health", "The quiet-hours digest cannot be polled: {e}");
    }
}
