//! One page (tab) of a running browser: navigate, observe, read.

use crate::cdp::Connection;
use crate::challenge::{self, Challenge};
use crate::ready::{self, Observation, Readiness, Rule};
use crate::{Error, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CALL: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(100);
/// Largest document returned.
pub const MAX_HTML: usize = 16 << 20;

/// Runs in every new document: remembers when the DOM last changed (the "quiet" part of the readiness rule).
const TRACKER: &str = "if (window.top === window) { window.__quietSince = performance.now(); \
new MutationObserver(function () { window.__quietSince = performance.now(); }) \
.observe(document, {subtree: true, childList: true, attributes: true, characterData: true}); }";

/// A tab.
pub struct Page {
    pub(crate) conn: Arc<Connection>,
    pub(crate) session: String,
}

/// What a page looks like right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Current address.
    pub url: String,
    /// Document title.
    pub title: String,
    /// Bounded excerpt of the visible text.
    pub text: String,
    /// The poll's readiness input.
    pub load_finished: bool,
    /// Whether the awaited element exists (`None` when none is awaited).
    pub element_present: Option<bool>,
    /// DOM-quiet time in milliseconds.
    pub quiet_ms: u64,
    /// A password field exists.
    pub has_password_field: bool,
}

/// How waiting for a page ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Ready to read.
    Ready,
    /// A wall that needs the user.
    Challenge(Challenge),
    /// The time ran out; the last readiness verdict says what was missing.
    TimedOut(Readiness),
}

impl Page {
    pub(crate) fn attach(conn: Arc<Connection>) -> Result<Page> {
        let t = conn.call(None, "Target.createTarget", json!({"url": "about:blank"}), CALL)?;
        let target = str_of(&t, "targetId")?;
        let s = conn.call(None, "Target.attachToTarget", json!({"targetId": target, "flatten": true}), CALL)?;
        let session = str_of(&s, "sessionId")?;
        let p = Page { conn, session };
        p.call("Page.enable", json!({}))?;
        p.call("Page.addScriptToEvaluateOnNewDocument", json!({"source": TRACKER}))?;
        // Downloads are never wanted from a page that is only read.
        let _ = p.conn.call(None, "Browser.setDownloadBehavior", json!({"behavior": "deny"}), CALL);
        Ok(p)
    }

    fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.conn.call(Some(&self.session), method, params, CALL)
    }

    /// Loads `url` and waits for the load event (or `timeout`).
    pub fn navigate(&self, url: &str, timeout: Duration) -> Result<()> {
        let since = self.conn.mark();
        let r = self.call("Page.navigate", json!({"url": url}))?;
        if let Some(e) = r.get("errorText").and_then(Value::as_str) {
            // Chrome cancels this navigation in the same step that starts the one replacing it, and sends that start
            // just after this reply. Any later reply comes after it, so one more (browser-side) call is the barrier.
            let _ = self.call("Page.getNavigationHistory", json!({}));
            let newer = self
                .conn
                .wait_event(since, Duration::ZERO, |ev| replaced_by(ev, &self.session, &r).is_some())
                .and_then(|ev| replaced_by(&ev, &self.session, &r));
            return Err(not_loaded(e, &r, newer.as_deref()));
        }
        let s = self.session.clone();
        self.conn
            .wait_event(since, timeout, |e| {
                e.method == "Page.loadEventFired" && e.session.as_deref() == Some(&s)
            })
            .map(|_| ())
            .ok_or_else(|| {
                // A closed connection means the browser (or its page) went away, which is not a slow page: say so.
                if self.conn.is_closed() {
                    Error::Protocol("the browser closed the connection while the page was loading".into())
                } else {
                    Error::Timeout("the page load")
                }
            })
    }

    /// Evaluates a JavaScript expression in the page and returns its value.
    pub fn evaluate(&self, expression: &str) -> Result<Value> {
        let r = self.call(
            "Runtime.evaluate",
            json!({"expression": expression, "returnByValue": true, "awaitPromise": true, "timeout": 8000}),
        )?;
        if let Some(d) = r.get("exceptionDetails") {
            let m = d
                .pointer("/exception/description")
                .or_else(|| d.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("script error");
            return Err(Error::Protocol(m.to_string()));
        }
        Ok(r.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }

    /// One observation of the page. `selector` is the element the watch waits for.
    pub fn snapshot(&self, selector: Option<&str>) -> Result<Snapshot> {
        let sel = match selector {
            // JSON string literals are valid JavaScript string literals.
            Some(s) => format!(
                "(function(){{try{{return !!document.querySelector({})}}catch(e){{return false}}}})()",
                json!(s)
            ),
            None => "null".to_string(),
        };
        let expr = format!(
            "({{state: document.readyState, present: {sel}, quiet: window.__quietSince === undefined ? 0 : performance.now() - window.__quietSince, \
pw: !!document.querySelector('input[type=password]'), url: location.href, title: document.title, \
text: document.body ? document.body.innerText.slice(0, 4000) : ''}})"
        );
        let v = self.evaluate(&expr)?;
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Ok(Snapshot {
            url: s("url"),
            title: s("title"),
            text: s("text"),
            load_finished: v.get("state").and_then(Value::as_str) == Some("complete"),
            element_present: v.get("present").and_then(Value::as_bool),
            quiet_ms: v.get("quiet").and_then(Value::as_f64).unwrap_or(0.0).max(0.0) as u64,
            has_password_field: v.get("pw").and_then(Value::as_bool).unwrap_or(false),
        })
    }

    /// Polls until the page is ready, shows a challenge, or `timeout` passes.
    pub fn wait_ready(&self, selector: Option<&str>, rule: &Rule, timeout: Duration) -> Result<Outcome> {
        let end = Instant::now() + timeout;
        loop {
            if self.conn.is_closed() {
                return Err(Error::Closed);
            }
            let snap = self.snapshot(selector)?;
            let obs = Observation {
                load_finished: snap.load_finished,
                element_present: snap.element_present,
                quiet: Duration::from_millis(snap.quiet_ms),
            };
            let last = ready::readiness(&obs, rule);
            let challenge = challenge::detect(&challenge::Signals {
                url: &snap.url,
                title: &snap.title,
                text: &snap.text,
                has_password_field: snap.has_password_field,
                target_present: snap.element_present == Some(true),
            });
            if snap.load_finished {
                if let Some(c) = challenge {
                    return Ok(Outcome::Challenge(c));
                }
            }
            if last == Readiness::Ready {
                return Ok(Outcome::Ready);
            }
            if Instant::now() >= end {
                return Ok(Outcome::TimedOut(last));
            }
            std::thread::sleep(POLL);
        }
    }

    /// The document's `outerHTML` and the current address.
    pub fn capture(&self) -> Result<(String, String)> {
        let v = self.evaluate("({html: document.documentElement.outerHTML, url: location.href})")?;
        let html = v.get("html").and_then(Value::as_str).unwrap_or("").to_string();
        if html.len() > MAX_HTML {
            return Err(Error::Protocol("the page is larger than the allowed size".into()));
        }
        Ok((html, v.get("url").and_then(Value::as_str).unwrap_or("").to_string()))
    }
}

/// The address of a document navigation in the same frame that is not the one `reply` (to `Page.navigate`) belongs to;
/// a same-document change (`pushState`, a fragment) never replaces a load.
fn replaced_by(e: &crate::cdp::Event, session: &str, reply: &Value) -> Option<String> {
    let p = &e.params;
    (e.method == "Page.frameStartedNavigating"
        && !matches!(
            p.get("navigationType").and_then(Value::as_str),
            Some("sameDocument" | "historySameDocument")
        )
        && e.session.as_deref() == Some(session)
        && p.get("frameId") == reply.get("frameId")
        && p.get("loaderId") != reply.get("loaderId"))
    .then(|| p.get("url").and_then(Value::as_str).unwrap_or("another address").to_string())
}

/// Why `Page.navigate` answered with `error` (its `errorText`) instead of a page. Chrome reports every navigation it
/// cancels before the page commits as `net::ERR_ABORTED`: a response that is a download or has no content (a 204), and a
/// navigation that another one replaced. A download and a replacement get their own message; anything else keeps the code.
fn not_loaded(error: &str, reply: &Value, replaced_by: Option<&str>) -> Error {
    if reply.get("isDownload").and_then(Value::as_bool) == Some(true) {
        return Error::Protocol("the address is a file download, not a page".into());
    }
    match replaced_by {
        Some(url) => Error::Protocol(format!(
            "the page did not load: another navigation (to {url}) replaced it ({error})"
        )),
        None => Error::Protocol(format!("the page did not load: {error}")),
    }
}

fn str_of(v: &Value, k: &str) -> Result<String> {
    v.get(k)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Protocol(format!("no {k} in the reply")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdp::Event;

    fn started(session: &str, frame: &str, loader: &str, url: &str) -> Event {
        Event {
            method: "Page.frameStartedNavigating".into(),
            session: Some(session.into()),
            params: json!({"frameId": frame, "loaderId": loader, "url": url, "navigationType": "differentDocument"}),
        }
    }

    #[test]
    fn a_download_is_named_as_one() {
        let r = json!({"frameId": "F", "loaderId": "L1", "isDownload": true, "errorText": "net::ERR_ABORTED"});
        let m = not_loaded("net::ERR_ABORTED", &r, None).to_string();
        assert!(m.contains("file download"), "{m}");
    }

    #[test]
    fn a_replaced_load_names_the_navigation_that_replaced_it() {
        let r = json!({"frameId": "F", "loaderId": "L1", "isDownload": false, "errorText": "net::ERR_ABORTED"});
        let m = not_loaded("net::ERR_ABORTED", &r, Some("http://a.test/next")).to_string();
        assert!(
            m.contains("replaced") && m.contains("http://a.test/next") && m.contains("net::ERR_ABORTED"),
            "{m}"
        );
        let plain = not_loaded("net::ERR_NAME_NOT_RESOLVED", &r, None).to_string();
        assert!(plain.ends_with("the page did not load: net::ERR_NAME_NOT_RESOLVED"), "{plain}");
    }

    #[test]
    fn only_another_loader_in_the_same_frame_and_session_counts_as_a_replacement() {
        let r = json!({"frameId": "F", "loaderId": "L1"});
        assert_eq!(
            replaced_by(&started("S", "F", "L2", "http://a.test/x"), "S", &r).as_deref(),
            Some("http://a.test/x")
        );
        // The load itself, a child frame, another tab and other events are not replacements.
        assert_eq!(replaced_by(&started("S", "F", "L1", "http://a.test/"), "S", &r), None);
        assert_eq!(replaced_by(&started("S", "child", "L2", "http://a.test/ad"), "S", &r), None);
        assert_eq!(replaced_by(&started("T", "F", "L2", "http://a.test/x"), "S", &r), None);
        let other = Event {
            method: "Page.frameStoppedLoading".into(),
            session: Some("S".into()),
            params: json!({"frameId": "F"}),
        };
        assert_eq!(replaced_by(&other, "S", &r), None);
        // A same-document change (pushState, a fragment) of the old page does not cancel the load.
        let mut same = started("S", "F", "L2", "http://a.test/#top");
        same.params["navigationType"] = json!("sameDocument");
        assert_eq!(replaced_by(&same, "S", &r), None);
        same.params["navigationType"] = json!("historySameDocument");
        assert_eq!(replaced_by(&same, "S", &r), None);
        // A replacement without an address still counts.
        let mut e = started("S", "F", "L3", "");
        e.params.as_object_mut().unwrap().remove("url");
        assert_eq!(replaced_by(&e, "S", &r).as_deref(), Some("another address"));
    }
}
