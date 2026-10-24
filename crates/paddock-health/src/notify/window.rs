//! Notices a window posts (`SHELL_PORTS.notify` in `@decryp7/paddock-ui/shell`): the UI's own messages ("the app
//! context failed to load") reach the engine's centre, so every window lists them (rule 34: the engine is the single
//! source of truth) instead of each window keeping its own.
//!
//! Untrusted input from a webview, parsed once into a [`NewNotice`]: the severity is closed, the title and body are
//! bounded and must not be blank, the kind is a short id, and the cause is the engine's (`ui:<kind>:<title>`), so a
//! window can never address another source's message (a `health:` cause, a job's). The same notice again merges into
//! the listed one with its count, like every other cause.

use super::centre::{MAX_BODY, MAX_TITLE, NewNotice, NoticeScope, NoticeSeverity, NoticeSource};
use crate::error::HealthError;
use serde::Deserialize;

/// The cause prefix of window notices (`LOCAL_CAUSE` in the UI).
pub const WINDOW_CAUSE: &str = "ui:";
/// The source of a window notice without a kind of its own.
pub const WINDOW_SOURCE: &str = "app";
/// The longest kind.
pub const MAX_KIND: usize = 64;

/// Which list a window notice goes to.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowNoticeScope {
    /// The app's list: every window shows it (the default).
    #[default]
    App,
    /// The posting window's list (its dialogs and pop-outs show it too).
    Window,
}

/// What a window posts with `notifications_post`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowNotice {
    /// `info`, `success`, `warning` or `error` (`critical` is the engine's).
    pub severity: NoticeSeverity,
    /// One sentence for the user (1 to [`MAX_TITLE`] characters).
    pub title: String,
    /// More detail (at most [`MAX_BODY`] characters).
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub body: Option<String>,
    /// What kind of notice (`[a-z0-9.-]`, at most [`MAX_KIND`]); its source in the list. Not `health`.
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub kind: Option<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub scope: Option<WindowNoticeScope>,
}

fn valid_kind(kind: &str) -> bool {
    !kind.is_empty()
        && kind.len() <= MAX_KIND
        && kind
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

/// Whether `s` has a character the centre would drop or change (a control character other than a space).
fn has_control(s: &str) -> bool {
    s.chars().any(char::is_control)
}

impl WindowNotice {
    /// The scope it asks for (default: the app's list).
    pub fn scope(&self) -> WindowNoticeScope {
        self.scope.unwrap_or_default()
    }

    /// The centre's post for it, or why it is refused (every refusal is `invalid`, for the window's own log).
    pub fn into_new(self) -> Result<NewNotice, HealthError> {
        if self.severity == NoticeSeverity::Critical {
            return Err(HealthError::invalid(
                "a window cannot post a critical notice",
            ));
        }
        let title = self.title.trim();
        if title.is_empty() {
            return Err(HealthError::invalid("a notice needs a title"));
        }
        if title.chars().count() > MAX_TITLE || has_control(title) {
            return Err(HealthError::invalid(format!(
                "a notice title is one line of at most {MAX_TITLE} characters"
            )));
        }
        let body = self.body.unwrap_or_default();
        if body.chars().count() > MAX_BODY {
            return Err(HealthError::invalid(format!(
                "a notice body has at most {MAX_BODY} characters"
            )));
        }
        let kind = match self.kind {
            None => None,
            Some(k) if valid_kind(&k) && k != NoticeSource::HEALTH => Some(k),
            Some(_) => {
                return Err(HealthError::invalid(format!(
                    "a notice kind is an id of [a-z0-9.-], at most {MAX_KIND} characters, and not {}",
                    NoticeSource::HEALTH
                )));
            }
        };
        let key = kind.as_deref().unwrap_or(severity_name(self.severity));
        let scope = match self.scope.unwrap_or_default() {
            WindowNoticeScope::App => NoticeScope::App,
            WindowNoticeScope::Window => NoticeScope::Project,
        };
        Ok(NewNotice::new(
            format!("{WINDOW_CAUSE}{key}:{title}"),
            kind.as_deref().unwrap_or(WINDOW_SOURCE),
            self.severity,
            title,
            scope,
        )
        .body(body))
    }
}

fn severity_name(s: NoticeSeverity) -> &'static str {
    match s {
        NoticeSeverity::Info => "info",
        NoticeSeverity::Success => "success",
        NoticeSeverity::Warning => "warning",
        NoticeSeverity::Error => "error",
        NoticeSeverity::Critical => "critical",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HealthErrorCode;
    use serde_json::json;

    fn parse(v: serde_json::Value) -> Result<NewNotice, HealthError> {
        serde_json::from_value::<WindowNotice>(v)
            .map_err(|e| HealthError::invalid(e.to_string()))?
            .into_new()
    }

    #[test]
    fn a_notice_becomes_a_cause_keyed_app_message() {
        let n = parse(json!({ "severity": "error", "title": "  The context failed to load.  " }))
            .unwrap();
        assert_eq!(n.cause, "ui:error:The context failed to load.");
        assert_eq!(n.source.as_str(), "app");
        assert_eq!(n.title, "The context failed to load.");
        assert_eq!(n.scope, NoticeScope::App);
        assert!(n.body.is_empty());
        let k = parse(json!({ "severity": "info", "title": "Saved", "kind": "sd.save", "body": "All of it.", "scope": "window" })).unwrap();
        assert_eq!(
            (k.cause.as_str(), k.source.as_str(), k.body.as_str()),
            ("ui:sd.save:Saved", "sd.save", "All of it.")
        );
        assert_eq!(k.scope, NoticeScope::Project);
        let w: WindowNotice =
            serde_json::from_value(json!({ "severity": "info", "title": "x" })).unwrap();
        assert_eq!(w.scope(), WindowNoticeScope::App);
        for (s, name) in [
            (NoticeSeverity::Info, "info"),
            (NoticeSeverity::Success, "success"),
            (NoticeSeverity::Warning, "warning"),
            (NoticeSeverity::Error, "error"),
            (NoticeSeverity::Critical, "critical"),
        ] {
            assert_eq!(severity_name(s), name);
        }
    }

    #[test]
    fn bad_notices_are_refused_with_a_reason() {
        let long = "x".repeat(MAX_TITLE + 1);
        let body = "y".repeat(MAX_BODY + 1);
        let kind = "k".repeat(MAX_KIND + 1);
        for (v, why) in [
            (json!({ "severity": "critical", "title": "x" }), "critical"),
            (
                json!({ "severity": "info", "title": "   " }),
                "needs a title",
            ),
            (json!({ "severity": "info", "title": long }), "one line"),
            (
                json!({ "severity": "info", "title": "two\nlines" }),
                "one line",
            ),
            (
                json!({ "severity": "info", "title": "x", "body": body }),
                "body",
            ),
            (
                json!({ "severity": "info", "title": "x", "kind": "health" }),
                "not health",
            ),
            (
                json!({ "severity": "info", "title": "x", "kind": "Bad Kind" }),
                "[a-z0-9.-]",
            ),
            (
                json!({ "severity": "info", "title": "x", "kind": "" }),
                "[a-z0-9.-]",
            ),
            (
                json!({ "severity": "info", "title": "x", "kind": kind }),
                "[a-z0-9.-]",
            ),
        ] {
            let e = parse(v.clone()).unwrap_err();
            assert_eq!(e.code, HealthErrorCode::Invalid, "{v}");
            assert!(e.message.contains(why), "{v}: {}", e.message);
        }
        // Unknown fields (a cause of its own, an action to run) are not part of the contract.
        for v in [
            json!({ "severity": "info", "title": "x", "cause": "health:store" }),
            json!({ "severity": "info", "title": "x", "primary": { "type": "repairService" } }),
            json!({ "severity": "loud", "title": "x" }),
        ] {
            assert!(serde_json::from_value::<WindowNotice>(v).is_err());
        }
    }
}
