//! The cause-key notification centre (Paddock ADR 0010): things that happened and may need attention (a job finished,
//! a service stopped, an update, a crash recovered). One notification per cause, updated in place; a cause that clears
//! is resolved. Dedupe happens once, here, and every window receives the same list.
//!
//! - [`centre`]: the pure list (cause-key merge with counts, caps, read state).
//! - [`persist`]: the versioned records: a window's project messages travel with its project, the app's live in a
//!   file in the app's data folder.
//! - [`health`]: what a service-health change means for the centre (one message per outage, with Repair).
//! - Here: [`NotificationCentre`], the app's list and every window's project list, with the app list saved off the
//!   locks by a debounced writer.
//!
//! Windows are keyed by the label of the top-level window (`main`, `w-<n>`); the Tauri adapter maps a dialog or pop-out
//! to its host.

pub mod centre;
pub mod health;
pub mod persist;
mod writer;

use crate::clock::Clock;
use centre::{APP_CAP, Centre, NewNotice, Notice, NoticeId, NoticeScope, PROJECT_CAP, Posted};
use persist::NoticesRecord;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use writer::Debounced;

/// The event a window hears when its list changed; its payload is a [`NoticesEvent`].
pub const NOTIFICATIONS_EVENT: &str = "paddock://notifications";
/// The event telling a window to open a message (an OS notification of it was clicked); its payload is a [`NoticeId`].
pub const OPEN_EVENT: &str = "paddock://notifications-open";
/// An OS notification clicked (the app focused) within this time opens its message.
pub const OPEN_WITHIN_MS: i64 = 120_000;
/// How long changes to the app list wait before they are written (bursts are written once).
pub const SAVE_DELAY: Duration = Duration::from_millis(250);

/// A window's messages: its project's and the app's, newest first.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticesSnapshot {
    pub notices: Vec<Notice>,
    pub unread: usize,
}

/// What a window hears when its list changed.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticesEvent {
    #[serde(flatten)]
    pub snapshot: NoticesSnapshot,
    /// The message just posted or merged (the UI previews it as a toast); none for a quiet one or another change.
    pub posted: Option<Notice>,
}

/// A window's list after its project messages were restored, and why not all of them loaded.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoredNotices {
    pub snapshot: NoticesSnapshot,
    pub warning: Option<String>,
}

/// How many messages each list keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub app: usize,
    pub project: usize,
}

impl Default for Caps {
    fn default() -> Caps {
        Caps {
            app: APP_CAP,
            project: PROJECT_CAP,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PendingOpen {
    id: NoticeId,
    window: Option<String>,
    at: i64,
}

/// The app's messages and each window's project messages.
pub struct NotificationCentre {
    app: Mutex<Centre>,
    projects: Mutex<HashMap<String, Centre>>,
    caps: Caps,
    next: AtomicU64,
    /// Writes the app list to its file, debounced, off the centre's locks (none: in memory only).
    writer: Option<Debounced>,
    clock: Arc<dyn Clock>,
    pending: Mutex<Option<PendingOpen>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl NotificationCentre {
    /// A centre whose app messages are saved in `file` (and loaded from it: missing or damaged gives none); `None`
    /// keeps them in memory only.
    pub fn new(file: Option<PathBuf>, clock: Arc<dyn Clock>) -> NotificationCentre {
        Self::with_caps(file, clock, Caps::default())
    }

    /// [`NotificationCentre::new`] with other caps.
    pub fn with_caps(
        file: Option<PathBuf>,
        clock: Arc<dyn Clock>,
        caps: Caps,
    ) -> NotificationCentre {
        let saved = file.as_deref().map(persist::load_file).unwrap_or_default();
        let app = Centre::with(caps.app, saved);
        NotificationCentre {
            next: AtomicU64::new(app.max_id() + 1),
            app: Mutex::new(app),
            projects: Mutex::new(HashMap::new()),
            caps,
            writer: file.map(|path| {
                Debounced::new(SAVE_DELAY, move |notices: Vec<Notice>| {
                    if let Err(e) = persist::save_file(&path, &notices) {
                        log::warn!(target: "paddock_health", "The notifications could not be saved: {e}");
                    }
                })
            }),
            clock,
            pending: Mutex::new(None),
        }
    }

    fn id(&self) -> NoticeId {
        NoticeId(self.next.fetch_add(1, Ordering::SeqCst))
    }

    /// Queues the app list for its file (written by the debounced writer, never under the centre's locks).
    fn save_app(&self, app: &Centre) {
        if let Some(w) = &self.writer {
            w.schedule(app.notices().to_vec());
        }
    }

    /// Writes what is pending now (the app is closing; tests). True when everything is written.
    pub fn flush(&self) -> bool {
        self.writer
            .as_ref()
            .is_none_or(|w| w.flush(Duration::from_secs(2)))
    }

    /// Posts a message: app-scoped ones to the app list, project-scoped ones to `window`'s (without a window they go to
    /// the app list too).
    pub fn post(&self, window: Option<&str>, mut new: NewNotice) -> Posted {
        let now = self.clock.now_ms();
        let id = self.id();
        if new.link.window.is_none() {
            new.link.window = window.map(str::to_string);
        }
        let posted = match (new.scope, window) {
            (NoticeScope::Project, Some(w)) => lock(&self.projects)
                .entry(w.to_string())
                .or_insert_with(|| Centre::new(self.caps.project))
                .post(new, id, now),
            _ => {
                new.scope = NoticeScope::App;
                let mut app = lock(&self.app);
                let p = app.post(new, id, now);
                if !p.unchanged {
                    self.save_app(&app);
                }
                p
            }
        };
        if !posted.unchanged {
            let n = &posted.notice;
            log::info!(
                target: "paddock_health",
                Source = n.source.as_str(),
                Severity = format!("{:?}", n.severity).as_str(),
                Count = n.count;
                "{}", if posted.merged { "Notification merged" } else { "Notification posted" }
            );
        }
        posted
    }

    /// Runs `f` on the list holding the message (the window's project list, then the app list).
    fn with_list<T>(&self, window: &str, f: impl Fn(&mut Centre) -> Option<T>) -> Option<T> {
        if let Some(t) = lock(&self.projects).get_mut(window).and_then(&f) {
            return Some(t);
        }
        let mut app = lock(&self.app);
        let t = f(&mut app);
        if t.is_some() {
            self.save_app(&app);
        }
        t
    }

    /// The window's messages: its project's and the app's, newest first.
    pub fn snapshot(&self, window: &str) -> NoticesSnapshot {
        let mut notices: Vec<Notice> = lock(&self.app).notices().to_vec();
        if let Some(p) = lock(&self.projects).get(window) {
            notices.extend(p.notices().iter().cloned());
        }
        notices.sort_by(|a, b| b.at.cmp(&a.at).then(b.id.cmp(&a.id)));
        let unread = notices.iter().filter(|n| !n.read).count();
        NoticesSnapshot { notices, unread }
    }

    /// Marks one message read. False when the window cannot see it.
    pub fn mark_read(&self, window: &str, id: NoticeId) -> bool {
        self.with_list(window, |c| c.mark_read(id).then_some(()))
            .is_some()
    }

    /// The cause cleared (the service is back): removes its message from the app list and every window's list. True
    /// when one was removed.
    pub fn resolve(&self, cause: &str) -> bool {
        let mut any = false;
        for c in lock(&self.projects).values_mut() {
            any |= c.resolve_cause(cause).is_some();
        }
        let mut app = lock(&self.app);
        if app.resolve_cause(cause).is_some() {
            self.save_app(&app);
            any = true;
        }
        any
    }

    /// Removes one message. False when the window cannot see it.
    pub fn dismiss(&self, window: &str, id: NoticeId) -> bool {
        self.with_list(window, |c| c.dismiss(id).then_some(()))
            .is_some()
    }

    /// Marks the window's messages (its project's and the app's) read; returns how many were unread.
    pub fn mark_all_read(&self, window: &str) -> usize {
        let project = lock(&self.projects)
            .get_mut(window)
            .map_or(0, Centre::mark_all_read);
        let mut app = lock(&self.app);
        let n = app.mark_all_read();
        if n > 0 {
            self.save_app(&app);
        }
        project + n
    }

    /// The window's project messages, as saved with its project.
    pub fn record(&self, window: &str) -> NoticesRecord {
        NoticesRecord::new(
            lock(&self.projects)
                .get(window)
                .map(|c| c.notices().to_vec())
                .unwrap_or_default(),
        )
    }

    /// Replaces the window's project messages with saved ones (renumbered); returns why not all loaded.
    pub fn restore(&self, window: &str, value: Option<&Value>) -> Option<String> {
        let (saved, warning) = NoticesRecord::from_value(value);
        let renumbered = saved
            .into_iter()
            .map(|mut n| {
                n.id = self.id();
                n.scope = NoticeScope::Project;
                n.link.window = Some(window.to_string());
                n
            })
            .collect();
        lock(&self.projects).insert(
            window.to_string(),
            Centre::with(self.caps.project, renumbered),
        );
        warning
    }

    /// A window closed: its project messages go with it.
    pub fn forget_window(&self, window: &str) {
        lock(&self.projects).remove(window);
    }

    /// An OS notification for `id` was shown: focusing the app soon after opens it.
    pub fn note_shown(&self, id: NoticeId, window: Option<&str>) {
        *lock(&self.pending) = Some(PendingOpen {
            id,
            window: window.map(str::to_string),
            at: self.clock.now_ms(),
        });
    }

    /// The message to open now that the app got the focus (once), if an OS notification was shown recently, and the
    /// window it was posted for.
    pub fn take_pending(&self) -> Option<(NoticeId, Option<String>)> {
        let p = lock(&self.pending).take()?;
        (self.clock.now_ms() - p.at <= OPEN_WITHIN_MS).then_some((p.id, p.window))
    }
}

#[cfg(test)]
mod tests;
