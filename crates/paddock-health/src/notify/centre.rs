//! The notification centre's messages and the pure list that keeps them: the cause key merges a repeat into one
//! message with a count, caps (read messages are trimmed first), read state and dismissal. No threads, no clock (`now`
//! is passed in) and no I/O, so every rule is unit-tested.
//!
//! Wire contract: `ui/projects/paddock-ui/shell/generated/health.ts` (generated from these types).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Window-scoped messages kept per window (and saved with its project).
pub const PROJECT_CAP: usize = 200;
/// App-scoped messages kept (in the app's data folder).
pub const APP_CAP: usize = 100;
/// The longest title, in characters.
pub const MAX_TITLE: usize = 160;
/// The longest body, in characters.
pub const MAX_BODY: usize = 1000;
/// The longest cause key, source, code or action type, in characters.
pub const MAX_KEY: usize = 200;
/// The most arguments an action keeps.
pub const MAX_ACTION_ARGS: usize = 8;

/// A message's id, unique in the running app.
#[cfg_attr(test, derive(ts_rs::TS), ts(type = "number"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NoticeId(pub u64);

/// What raised the message (the view groups by it): an id the app defines (`job`, `update`, `liveAlert`); the shell's
/// own is [`NoticeSource::HEALTH`].
#[cfg_attr(test, derive(ts_rs::TS), ts(type = "string"))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NoticeSource(pub String);

impl NoticeSource {
    /// A service stopped (service health).
    pub const HEALTH: &'static str = "health";

    /// The source `id`.
    pub fn new(id: impl Into<String>) -> NoticeSource {
        NoticeSource(id.into())
    }

    /// The service-health source.
    pub fn health() -> NoticeSource {
        NoticeSource::new(NoticeSource::HEALTH)
    }

    /// The id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for NoticeSource {
    fn from(id: &str) -> NoticeSource {
        NoticeSource::new(id)
    }
}

/// How bad it is.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoticeSeverity {
    Info,
    Success,
    Warning,
    Error,
    /// A failure that stops the work and stays until it is resolved or dismissed.
    Critical,
}

/// Where a message lives: with the window's project (saved with it), or with the app (update, crash recovery,
/// service health).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoticeScope {
    App,
    Project,
}

/// What a message's button does: a `type` the UI maps to a feature (an app's notification kinds name their actions)
/// and its arguments, flat on the wire: `{"type": "viewJob", "jobId": 3}`. The UI never runs anything the engine did not
/// offer here. The shell's own are [`NoticeAction::repair_service`] and [`NoticeAction::open_settings`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticeAction {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub args: Map<String, Value>,
}

impl NoticeAction {
    /// Repairs a service from the shell (`health_repair`).
    pub const REPAIR_SERVICE: &'static str = "repairService";
    /// Opens Settings at a section.
    pub const OPEN_SETTINGS: &'static str = "openSettings";

    /// An action of `kind` without arguments.
    pub fn new(kind: impl Into<String>) -> NoticeAction {
        NoticeAction {
            kind: kind.into(),
            args: Map::new(),
        }
    }

    /// Adds an argument (camelCase name). `type` is the action's own key and is ignored as an argument.
    pub fn arg(mut self, name: impl Into<String>, value: impl Into<Value>) -> NoticeAction {
        let name = name.into();
        if name != "type" {
            self.args.insert(name, value.into());
        }
        self
    }

    /// Repairs `service` from the shell: `{"type": "repairService", "service": <id>}`.
    pub fn repair_service(service: impl Into<String>) -> NoticeAction {
        NoticeAction::new(NoticeAction::REPAIR_SERVICE).arg("service", service.into())
    }

    /// Opens Settings at `section`: `{"type": "openSettings", "section": <id>}`.
    pub fn open_settings(section: impl Into<String>) -> NoticeAction {
        NoticeAction::new(NoticeAction::OPEN_SETTINGS).arg("section", section.into())
    }

    /// Bounded: the type cleaned, at most [`MAX_ACTION_ARGS`] arguments.
    fn bounded(mut self) -> NoticeAction {
        self.kind = clean(&self.kind, MAX_KEY);
        if self.args.len() > MAX_ACTION_ARGS {
            self.args = self.args.into_iter().take(MAX_ACTION_ARGS).collect();
        }
        self
    }
}

/// What a message refers to.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticeLink {
    /// The window it was posted for.
    pub window: Option<String>,
    /// The job it is about (`paddock-jobs`).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number | null"))]
    pub job_id: Option<u64>,
    /// The file or folder it is about.
    #[serde(default)]
    pub path: Option<String>,
}

/// One message.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub id: NoticeId,
    pub source: NoticeSource,
    pub severity: NoticeSeverity,
    pub title: String,
    pub body: String,
    /// A stable id of the wording, for translation later.
    #[serde(default)]
    pub code: Option<String>,
    /// Epoch ms of the latest occurrence.
    #[cfg_attr(test, ts(type = "number"))]
    pub at: i64,
    #[cfg_attr(test, ts(type = "number"))]
    pub first_at: i64,
    /// How many times it happened (a repeat of the cause merges).
    pub count: u32,
    pub read: bool,
    #[cfg_attr(test, ts(type = "NoticeAction | null"))]
    pub primary: Option<NoticeAction>,
    #[cfg_attr(test, ts(type = "NoticeAction | null"))]
    pub secondary: Option<NoticeAction>,
    /// A third button (e.g. "Skip this version" next to "Update" and "Later").
    #[serde(default)]
    #[cfg_attr(test, ts(type = "NoticeAction | null"))]
    pub tertiary: Option<NoticeAction>,
    #[serde(default)]
    pub link: NoticeLink,
    /// The cause key (`health:store`, `job:12`, `update:1.2`): a second post with the same cause updates this message in
    /// place; a cause that clears resolves (removes) it.
    pub cause: String,
    pub scope: NoticeScope,
}

/// What a producer posts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNotice {
    pub source: NoticeSource,
    pub severity: NoticeSeverity,
    pub title: String,
    pub body: String,
    pub code: Option<String>,
    pub primary: Option<NoticeAction>,
    pub secondary: Option<NoticeAction>,
    pub tertiary: Option<NoticeAction>,
    pub link: NoticeLink,
    /// Mandatory: what this message is about. The same cause again updates the existing message.
    pub cause: String,
    pub scope: NoticeScope,
    /// Posting the same cause again is ignored instead of counted (e.g. an update announced at every check).
    pub once: bool,
    /// Listed without a toast (a warning that needs no action yet).
    pub quiet: bool,
}

impl NewNotice {
    /// A message about `cause`.
    pub fn new(
        cause: impl Into<String>,
        source: impl Into<NoticeSource>,
        severity: NoticeSeverity,
        title: impl Into<String>,
        scope: NoticeScope,
    ) -> NewNotice {
        NewNotice {
            source: source.into(),
            severity,
            title: title.into(),
            body: String::new(),
            code: None,
            primary: None,
            secondary: None,
            tertiary: None,
            link: NoticeLink::default(),
            cause: cause.into(),
            scope,
            once: false,
            quiet: false,
        }
    }

    /// Listed in the centre without a toast.
    pub fn quiet(mut self) -> NewNotice {
        self.quiet = true;
        self
    }

    /// Announced once per cause: posting it again changes nothing.
    pub fn once(mut self) -> NewNotice {
        self.once = true;
        self
    }

    pub fn body(mut self, body: impl Into<String>) -> NewNotice {
        self.body = body.into();
        self
    }

    /// The stable id of the wording.
    pub fn code(mut self, code: impl Into<String>) -> NewNotice {
        self.code = Some(code.into());
        self
    }

    pub fn primary(mut self, a: NoticeAction) -> NewNotice {
        self.primary = Some(a);
        self
    }

    pub fn secondary(mut self, a: NoticeAction) -> NewNotice {
        self.secondary = Some(a);
        self
    }

    pub fn tertiary(mut self, a: NoticeAction) -> NewNotice {
        self.tertiary = Some(a);
        self
    }

    pub fn link(mut self, link: NoticeLink) -> NewNotice {
        self.link = link;
        self
    }
}

/// One line of plain text: tabs and newlines become spaces, other control characters go, at most `max` characters.
pub(crate) fn clean(s: &str, max: usize) -> String {
    s.chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

/// What a post did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Posted {
    pub notice: Notice,
    /// It merged into an existing message (same cause).
    pub merged: bool,
    /// A `once` message that was already there: nothing changed.
    pub unchanged: bool,
}

/// One list of messages (the app's, or one window's project), oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Centre {
    cap: usize,
    notices: Vec<Notice>,
}

impl Centre {
    /// An empty list keeping at most `cap` messages.
    pub fn new(cap: usize) -> Centre {
        Centre {
            cap,
            notices: Vec::new(),
        }
    }

    /// Takes saved messages (bounded by the cap, read ones dropped first).
    pub fn with(cap: usize, notices: Vec<Notice>) -> Centre {
        let mut c = Centre { cap, notices };
        c.notices.sort_by_key(|n| n.at);
        c.trim();
        c
    }

    /// Adds a message, or merges it into the one with the same cause: the count goes up, the time, text, severity and
    /// actions are the latest, and it is unread again. `id` is used for a new message.
    pub fn post(&mut self, new: NewNotice, id: NoticeId, now: i64) -> Posted {
        let key = clean(&new.cause, MAX_KEY);
        let key = if key.is_empty() {
            format!("anonymous:{}", id.0)
        } else {
            key
        };
        let title = clean(&new.title, MAX_TITLE);
        let body = clean(&new.body, MAX_BODY);
        let code = new.code.map(|c| clean(&c, MAX_KEY));
        let (primary, secondary, tertiary) = (
            new.primary.map(NoticeAction::bounded),
            new.secondary.map(NoticeAction::bounded),
            new.tertiary.map(NoticeAction::bounded),
        );
        if let Some(i) = self.notices.iter().position(|n| n.cause == key) {
            if new.once {
                return Posted {
                    notice: self.notices[i].clone(),
                    merged: true,
                    unchanged: true,
                };
            }
            let mut n = self.notices.remove(i);
            n.count = n.count.saturating_add(1);
            n.at = now.max(n.at);
            n.read = false;
            n.title = title;
            n.body = body;
            n.code = code;
            n.severity = new.severity;
            n.primary = primary;
            n.secondary = secondary;
            n.tertiary = tertiary;
            n.link = new.link;
            self.notices.push(n.clone());
            return Posted {
                notice: n,
                merged: true,
                unchanged: false,
            };
        }
        let source = clean(new.source.as_str(), MAX_KEY);
        let n = Notice {
            id,
            source: NoticeSource(if source.is_empty() {
                "app".into()
            } else {
                source
            }),
            severity: new.severity,
            title,
            body,
            code,
            at: now,
            first_at: now,
            count: 1,
            read: false,
            primary,
            secondary,
            tertiary,
            link: new.link,
            cause: key,
            scope: new.scope,
        };
        self.notices.push(n.clone());
        self.trim();
        Posted {
            notice: n,
            merged: false,
            unchanged: false,
        }
    }

    /// Updates the message of `cause` in place (`post` with a cause that is there does the same); false when no message
    /// has this cause, and nothing changes.
    pub fn update_cause(&mut self, cause: &str, new: NewNotice, now: i64) -> bool {
        let Some(id) = self.notices.iter().find(|n| n.cause == cause).map(|n| n.id) else {
            return false;
        };
        let mut new = new;
        new.cause = cause.to_string();
        self.post(new, id, now);
        true
    }

    /// The cause cleared: removes its message. Returns it, or `None` when it was not here.
    pub fn resolve_cause(&mut self, cause: &str) -> Option<Notice> {
        let i = self.notices.iter().position(|n| n.cause == cause)?;
        Some(self.notices.remove(i))
    }

    /// Over the cap: the oldest read messages go first, then the oldest unread ones.
    fn trim(&mut self) {
        while self.notices.len() > self.cap {
            let i = self.notices.iter().position(|n| n.read).unwrap_or(0);
            self.notices.remove(i);
        }
    }

    /// Marks one message read. False when it is not here.
    pub fn mark_read(&mut self, id: NoticeId) -> bool {
        match self.notices.iter_mut().find(|n| n.id == id) {
            Some(n) => {
                n.read = true;
                true
            }
            None => false,
        }
    }

    /// Marks every message read; returns how many were unread.
    pub fn mark_all_read(&mut self) -> usize {
        let mut n = 0;
        for m in self.notices.iter_mut().filter(|m| !m.read) {
            m.read = true;
            n += 1;
        }
        n
    }

    /// Removes a message. False when it is not here.
    pub fn dismiss(&mut self, id: NoticeId) -> bool {
        let before = self.notices.len();
        self.notices.retain(|n| n.id != id);
        self.notices.len() != before
    }

    /// How many are unread.
    pub fn unread(&self) -> usize {
        self.notices.iter().filter(|n| !n.read).count()
    }

    /// Every message, oldest first.
    pub fn notices(&self) -> &[Notice] {
        &self.notices
    }

    /// The message `id`.
    pub fn get(&self, id: NoticeId) -> Option<&Notice> {
        self.notices.iter().find(|n| n.id == id)
    }

    /// The largest id in the list (0 when empty): new ids continue after it.
    pub fn max_id(&self) -> u64 {
        self.notices.iter().map(|n| n.id.0).max().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spike(host: &str) -> NewNotice {
        NewNotice::new(
            format!("live:errorSpike:{host}"),
            "liveAlert",
            NoticeSeverity::Error,
            format!("ERROR spike on {host}"),
            NoticeScope::Project,
        )
        .body("12 errors in a minute")
        .primary(NoticeAction::new("showAlert").arg("alertId", "a1"))
    }

    #[test]
    fn the_same_key_merges_into_one_message_with_a_count() {
        let mut c = Centre::new(10);
        let first = c.post(spike("WEB02"), NoticeId(1), 100);
        assert!(!first.merged);
        c.mark_read(NoticeId(1));
        c.post(spike("WEB01"), NoticeId(2), 150);
        for t in [200, 300, 400] {
            let p = c.post(spike("WEB02"), NoticeId(99), t);
            assert!(p.merged);
        }
        assert_eq!(c.notices().len(), 2);
        let merged = c.get(NoticeId(1)).unwrap();
        assert_eq!(
            (merged.count, merged.first_at, merged.at, merged.read),
            (4, 100, 400, false)
        );
        assert_eq!(
            c.notices().last().unwrap().id,
            NoticeId(1),
            "moved to the newest place"
        );
        assert!(c.get(NoticeId(99)).is_none());
        // A `once` message is announced once.
        let update = || {
            NewNotice::new(
                "update:2",
                "update",
                NoticeSeverity::Info,
                "Version 2 is available",
                NoticeScope::App,
            )
            .once()
        };
        c.post(update(), NoticeId(7), 500);
        c.mark_read(NoticeId(7));
        let again = c.post(update(), NoticeId(8), 600);
        assert!(again.unchanged && again.notice.read && again.notice.count == 1);
        c.dismiss(NoticeId(7));
        // Different causes never merge.
        let plain = |cause: &str| {
            NewNotice::new(
                cause,
                "session",
                NoticeSeverity::Info,
                "Safe mode",
                NoticeScope::App,
            )
        };
        c.post(plain("a"), NoticeId(3), 1);
        c.post(plain("b"), NoticeId(4), 2);
        assert_eq!(c.notices().len(), 4);
    }

    #[test]
    fn caps_trim_read_messages_first() {
        let job = |i: u64| {
            NewNotice::new(
                format!("job:{i}"),
                "job",
                NoticeSeverity::Success,
                format!("job {i}"),
                NoticeScope::Project,
            )
        };
        let mut c = Centre::new(3);
        for i in 1..=3 {
            c.post(job(i), NoticeId(i), i as i64);
        }
        c.mark_read(NoticeId(2));
        c.post(job(4), NoticeId(4), 4);
        let ids: Vec<u64> = c.notices().iter().map(|n| n.id.0).collect();
        assert_eq!(
            ids,
            [1, 3, 4],
            "the read one went, although it was not the oldest"
        );
        c.post(job(5), NoticeId(5), 5);
        let ids: Vec<u64> = c.notices().iter().map(|n| n.id.0).collect();
        assert_eq!(ids, [3, 4, 5], "all unread: the oldest goes");
        let restored = Centre::with(2, c.notices().to_vec());
        assert_eq!(restored.notices().len(), 2);
        assert_eq!(restored.max_id(), 5);
        assert_eq!(Centre::new(1).max_id(), 0);
    }

    #[test]
    fn read_state_dismissal_and_cleaning() {
        let mut c = Centre::new(10);
        let p = c.post(
            NewNotice::new(
                "plugin:p",
                "plugin",
                NoticeSeverity::Error,
                "Plugin\nquarantined\u{7}",
                NoticeScope::Project,
            )
            .body("x".repeat(5000))
            .code("plugin.quarantined\u{0}")
            .secondary(NoticeAction::new("showJobs")),
            NoticeId(1),
            0,
        );
        assert_eq!(p.notice.title, "Plugin quarantined");
        assert_eq!(p.notice.body.chars().count(), MAX_BODY);
        assert_eq!(p.notice.cause, "plugin:p");
        assert_eq!(p.notice.code.as_deref(), Some("plugin.quarantined"));
        c.post(
            NewNotice::new(
                "ai:paused",
                "ai",
                NoticeSeverity::Warning,
                "AI paused",
                NoticeScope::App,
            ),
            NoticeId(2),
            1,
        );
        assert_eq!(c.unread(), 2);
        assert!(c.mark_read(NoticeId(1)));
        assert!(!c.mark_read(NoticeId(9)));
        assert_eq!(c.unread(), 1);
        assert_eq!(c.mark_all_read(), 1);
        assert_eq!(c.mark_all_read(), 0);
        assert!(c.dismiss(NoticeId(1)));
        assert!(!c.dismiss(NoticeId(1)));
        assert_eq!(c.notices().len(), 1);
        let wire = serde_json::to_value(&c.notices()[0]).unwrap();
        assert_eq!(
            (
                wire["source"].as_str(),
                wire["severity"].as_str(),
                wire["scope"].as_str()
            ),
            (Some("ai"), Some("warning"), Some("app"))
        );
        let action = serde_json::to_value(NoticeAction::new("viewJob").arg("jobId", 3)).unwrap();
        assert_eq!(action, json!({"type": "viewJob", "jobId": 3}));
        let back: NoticeAction =
            serde_json::from_value(json!({"type": "fixTimestamps", "path": "/a", "format": null}))
                .unwrap();
        assert_eq!(
            back,
            NoticeAction::new("fixTimestamps")
                .arg("path", "/a")
                .arg("format", Value::Null)
        );
        assert!(
            serde_json::from_value::<NoticeAction>(json!({"path": "/a"})).is_err(),
            "an action has a type"
        );
    }

    #[test]
    fn sources_are_the_apps_ids_and_a_blank_one_is_the_app() {
        let mut c = Centre::new(10);
        let n = c
            .post(
                NewNotice::new("a", " \u{1}", NoticeSeverity::Info, "a", NoticeScope::App),
                NoticeId(1),
                0,
            )
            .notice;
        assert_eq!(n.source.as_str(), "app");
        assert_eq!(
            serde_json::to_value(NoticeSource::from("workspace")).unwrap(),
            json!("workspace")
        );
        assert_eq!(
            serde_json::from_value::<NoticeSource>(json!("workspace")).unwrap(),
            NoticeSource::new("workspace")
        );
        assert!(serde_json::from_value::<NoticeSource>(json!(1)).is_err());
        assert_eq!(NoticeSource::health().as_str(), "health");
    }

    #[test]
    fn actions_are_bounded_and_type_is_not_an_argument() {
        let mut big = NoticeAction::new(format!("x{}", "y".repeat(500))).arg("type", "evil");
        for i in 0..20 {
            big = big.arg(format!("a{i:02}"), i);
        }
        assert!(!big.args.contains_key("type"));
        let n = Centre::new(1).post(
            NewNotice::new("c", "s", NoticeSeverity::Info, "t", NoticeScope::App).tertiary(big),
            NoticeId(1),
            0,
        );
        let t = n.notice.tertiary.unwrap();
        assert_eq!(t.kind.chars().count(), MAX_KEY);
        assert_eq!(t.args.len(), MAX_ACTION_ARGS);
    }

    #[test]
    fn a_third_action_is_kept_merged_and_optional_on_the_wire() {
        let ask = |to: &str| {
            let key = format!("runtime:cpu@{to}");
            NewNotice::new(
                "ai-update:runtime:cpu",
                "ai",
                NoticeSeverity::Info,
                format!("Update to {to}"),
                NoticeScope::App,
            )
            .primary(NoticeAction::new("updateAi").arg("key", key.clone()))
            .secondary(NoticeAction::new("laterAiUpdate").arg("key", key.clone()))
            .tertiary(NoticeAction::new("skipAiUpdate").arg("key", key))
            .link(NoticeLink {
                job_id: Some(4),
                ..NoticeLink::default()
            })
        };
        let mut c = Centre::new(10);
        c.post(ask("b1"), NoticeId(1), 1);
        let merged = c.post(ask("b2"), NoticeId(2), 2).notice;
        assert_eq!(
            merged.tertiary,
            Some(NoticeAction::new("skipAiUpdate").arg("key", "runtime:cpu@b2"))
        );
        let wire = serde_json::to_value(&merged).unwrap();
        assert_eq!(
            wire["primary"],
            json!({"type": "updateAi", "key": "runtime:cpu@b2"})
        );
        assert_eq!(wire["secondary"]["type"], "laterAiUpdate");
        assert_eq!(wire["tertiary"]["type"], "skipAiUpdate");
        assert_eq!(
            wire["link"],
            json!({"window": null, "jobId": 4, "path": null})
        );
        let mut old = wire.clone();
        let o = old.as_object_mut().unwrap();
        o.remove("tertiary");
        o.remove("code");
        o.insert("link".into(), json!({"window": "main"}));
        let back: Notice = serde_json::from_value(old).unwrap();
        assert_eq!(
            back.tertiary, None,
            "messages saved before the third action still load"
        );
        assert_eq!(back.link.window.as_deref(), Some("main"));
    }

    #[test]
    fn a_blank_cause_is_replaced_by_a_unique_one_so_it_never_merges_by_accident() {
        let mut c = Centre::new(10);
        let a = c.post(
            NewNotice::new("  ", "session", NoticeSeverity::Info, "a", NoticeScope::App),
            NoticeId(1),
            0,
        );
        let b = c.post(
            NewNotice::new("", "session", NoticeSeverity::Info, "b", NoticeScope::App),
            NoticeId(2),
            1,
        );
        assert!(!a.merged && !b.merged);
        assert_ne!(a.notice.cause, b.notice.cause);
    }

    #[test]
    fn update_cause_changes_the_message_in_place_and_reports_a_cause_that_is_not_there() {
        let mut c = Centre::new(10);
        c.post(spike("WEB02"), NoticeId(1), 100);
        let newer = NewNotice::new(
            "ignored",
            "liveAlert",
            NoticeSeverity::Critical,
            "Worse",
            NoticeScope::Project,
        )
        .body("more")
        .quiet();
        assert!(newer.quiet);
        assert!(c.update_cause("live:errorSpike:WEB02", newer, 200));
        assert_eq!(c.notices().len(), 1);
        let n = &c.notices()[0];
        assert_eq!(
            (
                n.id,
                n.title.as_str(),
                n.severity,
                n.count,
                n.cause.as_str()
            ),
            (
                NoticeId(1),
                "Worse",
                NoticeSeverity::Critical,
                2,
                "live:errorSpike:WEB02"
            )
        );
        assert!(!c.update_cause("nope", spike("x"), 300));
        assert_eq!(c.notices().len(), 1);
    }

    #[test]
    fn a_cause_that_clears_removes_its_message_only() {
        let mut c = Centre::new(10);
        c.post(spike("WEB01"), NoticeId(1), 1);
        c.post(spike("WEB02"), NoticeId(2), 2);
        assert_eq!(
            c.resolve_cause("live:errorSpike:WEB01").map(|n| n.id),
            Some(NoticeId(1))
        );
        assert_eq!(c.resolve_cause("live:errorSpike:WEB01"), None);
        assert_eq!(c.notices().len(), 1);
    }

    #[test]
    fn the_cause_severity_and_shell_actions_are_spelled_like_the_ui() {
        let n = Centre::new(1).post(spike("W"), NoticeId(1), 0).notice;
        let wire = serde_json::to_value(&n).unwrap();
        assert_eq!(wire["cause"], "live:errorSpike:W");
        assert!(wire.get("dedupeKey").is_none());
        assert_eq!(
            serde_json::to_value(NoticeSeverity::Critical).unwrap(),
            json!("critical")
        );
        assert_eq!(
            serde_json::to_value(NoticeAction::repair_service("store")).unwrap(),
            json!({"type": "repairService", "service": "store"})
        );
        assert_eq!(
            serde_json::to_value(NoticeAction::open_settings("browser")).unwrap(),
            json!({"type": "openSettings", "section": "browser"})
        );
    }
}
