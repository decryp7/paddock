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
            return Err(Error::Protocol(format!("the page did not load: {e}")));
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

fn str_of(v: &Value, k: &str) -> Result<String> {
    v.get(k)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Protocol(format!("no {k} in the reply")))
}
