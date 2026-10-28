//! One page (tab) of a running browser: navigate, observe, read. A page knows when it is gone (the user closed its window
//! or tab, it crashed, the connection ended): every call then fails with [`Error::Closed`], whatever the browser answers.

use crate::cdp::{Connection, Event};
use crate::challenge::{self, Challenge, Signals};
use crate::ready::{self, Observation, Readiness, Rule};
use crate::{Error, Result};
use paddock_jobs::CancelToken;
use paddock_jobs::time::{Clock, Deadline};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

const CALL: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(100);
/// The largest document [`Page::capture`] returns.
pub const MAX_HTML: usize = 16 << 20;
/// How much visible text a snapshot carries.
pub const TEXT_EXCERPT: usize = 4000;
/// URL patterns blocked by [`Page::block_heavy`].
const HEAVY: &[&str] = &[
    "*.png", "*.jpg", "*.jpeg", "*.gif", "*.webp", "*.avif", "*.svg", "*.ico", "*.woff", "*.woff2",
    "*.ttf", "*.otf", "*.mp4", "*.webm", "*.mp3", "*.ogg",
];

/// Runs in every new document once [`Page::track_dom_changes`] was called: remembers when the DOM last changed.
const TRACKER: &str = "if (window.top === window) { window.__quietSince = performance.now(); \
new MutationObserver(function () { window.__quietSince = performance.now(); }) \
.observe(document, {subtree: true, childList: true, attributes: true, characterData: true}); }";

/// A tab of the browser.
pub struct Page {
    pub(crate) conn: Arc<Connection>,
    pub(crate) session: String,
    pub(crate) target: String,
    /// The browser's clock (`LaunchOptions::clock`): [`Page::wait_ready`] polls and times out on it.
    pub(crate) clock: Arc<dyn Clock>,
}

/// The address and title of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub url: String,
    pub title: String,
}

/// What a page looks like right now (one script round trip).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub url: String,
    pub title: String,
    /// At most [`TEXT_EXCERPT`] characters of the visible text.
    pub text: String,
    /// `document.readyState === "complete"`.
    pub load_finished: bool,
    /// Whether the awaited element exists (`None` when none is awaited).
    pub element_present: Option<bool>,
    /// Milliseconds since the DOM last changed; 0 without [`Page::track_dom_changes`].
    pub quiet_ms: u64,
    /// A password field exists.
    pub has_password_field: bool,
}

impl Snapshot {
    /// The challenge signals of this snapshot.
    pub fn signals(&self) -> Signals<'_> {
        Signals {
            url: &self.url,
            title: &self.title,
            text: &self.text,
            has_password_field: self.has_password_field,
            target_present: self.element_present == Some(true),
        }
    }
}

/// How waiting for a page ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Ready to read.
    Ready,
    /// A wall that needs the user.
    Challenge(Challenge),
    /// The time ran out; the last verdict says what was missing.
    TimedOut(Readiness),
}

impl Page {
    /// Opens a new page (a tab beside the others) and attaches to it.
    pub(crate) fn attach(conn: Arc<Connection>, clock: Arc<dyn Clock>) -> Result<Page> {
        let t = conn.call(
            None,
            "Target.createTarget",
            json!({"url": crate::launch::START_PAGE}),
            CALL,
        )?;
        let target = str_of(&t, "targetId")?;
        Page::adopt(conn, target, clock)
    }

    /// Attaches to the existing page `target` (the one the browser opened with).
    pub(crate) fn adopt(
        conn: Arc<Connection>,
        target: String,
        clock: Arc<dyn Clock>,
    ) -> Result<Page> {
        let s = conn.call(
            None,
            "Target.attachToTarget",
            json!({"targetId": target, "flatten": true}),
            CALL,
        )?;
        let session = str_of(&s, "sessionId")?;
        conn.watch(&session);
        conn.watch(&target);
        let p = Page {
            conn,
            session,
            target,
            clock,
        };
        p.call("Page.enable", json!({}))?;
        p.call("Network.enable", json!({}))?;
        // Downloads are never wanted from a page that is only read.
        let _ = p.conn.call(
            None,
            "Browser.setDownloadBehavior",
            json!({"behavior": "deny"}),
            CALL,
        );
        Ok(p)
    }

    /// Tracks DOM changes in every document this page loads from now on, so [`Snapshot::quiet_ms`] and
    /// [`Page::wait_ready`] see when the page stopped changing. This adds a small script to the page; an app that must
    /// never touch page content does not call it.
    pub fn track_dom_changes(&self) -> Result<()> {
        self.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source": TRACKER}),
        )
        .map(|_| ())
    }

    /// The page is gone: its window or tab was closed, it crashed, or the browser connection ended.
    pub fn is_closed(&self) -> bool {
        self.conn.is_closed() || self.conn.is_gone(&self.session) || self.conn.is_gone(&self.target)
    }

    /// Asks the browser whether this page still exists (for a close the browser did not report as an event).
    /// `Ok(false)`: it is gone; an error says nothing either way.
    pub fn exists(&self) -> Result<bool> {
        if self.is_closed() {
            return Ok(false);
        }
        let r = self.conn.call(None, "Target.getTargets", json!({}), CALL)?;
        Ok(r.get("targetInfos")
            .and_then(Value::as_array)
            .is_some_and(|t| {
                t.iter()
                    .any(|i| i.get("targetId").and_then(Value::as_str) == Some(&self.target))
            }))
    }

    pub(crate) fn call(&self, method: &str, params: Value) -> Result<Value> {
        if self.is_closed() {
            return Err(Error::Closed);
        }
        self.closed_wins(self.conn.call(Some(&self.session), method, params, CALL))
    }

    fn closed_wins<T>(&self, r: Result<T>) -> Result<T> {
        if self.is_closed() {
            return Err(Error::Closed);
        }
        r
    }

    /// Waits for the page's load event; a page closed meanwhile ends the wait at once.
    pub(crate) fn wait_load(&self, since: u64, timeout: Duration) -> Result<()> {
        let (s, t) = (self.session.clone(), self.target.clone());
        let ev = self.conn.wait_event(since, timeout, |e| {
            (e.method == "Page.loadEventFired" && e.session.as_deref() == Some(&s))
                || ends(e, &s, &t)
        });
        if self.is_closed() {
            return Err(Error::Closed);
        }
        ev.map(|_| ()).ok_or(Error::Timeout("the page load"))
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
                .wait_event(since, Duration::ZERO, |ev| {
                    replaced_by(ev, &self.session, &r).is_some()
                })
                .and_then(|ev| replaced_by(&ev, &self.session, &r));
            return Err(not_loaded(e, &r, newer.as_deref()));
        }
        self.wait_load(since, timeout)
    }

    /// Goes back one entry in the page's history and waits for it to load (a slow load is not an error).
    pub fn back(&self, timeout: Duration) -> Result<()> {
        let h = self.call("Page.getNavigationHistory", json!({}))?;
        let cur = h.get("currentIndex").and_then(Value::as_i64).unwrap_or(0);
        let id = (cur > 0)
            .then(|| h.get("entries").and_then(|e| e.get((cur - 1) as usize)))
            .flatten()
            .and_then(|e| e.get("id"))
            .and_then(Value::as_i64)
            .ok_or_else(|| Error::Protocol("there is no earlier page".into()))?;
        let since = self.conn.mark();
        self.call("Page.navigateToHistoryEntry", json!({"entryId": id}))?;
        match self.wait_load(since, timeout) {
            Err(Error::Closed) => Err(Error::Closed),
            _ => Ok(()),
        }
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

    /// The page's address and title, from the browser (no script in the page).
    pub fn info(&self) -> Result<Info> {
        let r = self.closed_wins(self.conn.call(
            None,
            "Target.getTargetInfo",
            json!({"targetId": self.target}),
            CALL,
        ))?;
        let g = |k: &str| {
            r.pointer(&format!("/targetInfo/{k}"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        Ok(Info {
            url: g("url"),
            title: g("title"),
        })
    }

    /// A bounded excerpt of the visible text.
    pub fn text_excerpt(&self) -> Result<String> {
        let v = self.evaluate(&format!(
            "document.body ? document.body.innerText.slice(0, {TEXT_EXCERPT}) : ''"
        ))?;
        Ok(v.as_str().unwrap_or("").to_string())
    }

    /// Whether the document finished loading.
    pub fn load_finished(&self) -> Result<bool> {
        Ok(self.evaluate("document.readyState")?.as_str() == Some("complete"))
    }

    /// One observation of the page. `selector` is the element a caller waits for.
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
text: document.body ? document.body.innerText.slice(0, {TEXT_EXCERPT}) : ''}})"
        );
        let v = self.evaluate(&expr)?;
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Ok(Snapshot {
            url: s("url"),
            title: s("title"),
            text: s("text").chars().take(TEXT_EXCERPT).collect(),
            load_finished: v.get("state").and_then(Value::as_str) == Some("complete"),
            element_present: v.get("present").and_then(Value::as_bool),
            quiet_ms: v
                .get("quiet")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .max(0.0) as u64,
            has_password_field: v.get("pw").and_then(Value::as_bool).unwrap_or(false),
        })
    }

    /// The challenge this page shows, if any.
    pub fn challenge(&self) -> Result<Option<Challenge>> {
        Ok(challenge::detect(&self.snapshot(None)?.signals()))
    }

    /// Polls until the page is ready, shows a challenge (once loaded), or `timeout` passes; at least one poll is made.
    /// Needs [`Page::track_dom_changes`] before the navigation, or the DOM never counts as quiet.
    pub fn wait_ready(
        &self,
        selector: Option<&str>,
        rule: &Rule,
        timeout: Duration,
    ) -> Result<Outcome> {
        let end = Deadline::after(self.clock.clone(), timeout);
        loop {
            let snap = self.snapshot(selector)?;
            let last = ready::readiness(
                &Observation {
                    load_finished: snap.load_finished,
                    element_present: snap.element_present,
                    region_items: None,
                    quiet: Duration::from_millis(snap.quiet_ms),
                },
                rule,
            );
            if snap.load_finished
                && let Some(c) = challenge::detect(&snap.signals())
            {
                return Ok(Outcome::Challenge(c));
            }
            if last == Readiness::Ready {
                return Ok(Outcome::Ready);
            }
            if end.passed() {
                return Ok(Outcome::TimedOut(last));
            }
            // A fresh token: nothing cancels a poll, the page's own state ends the loop.
            let _ = self.clock.wait(POLL, &CancelToken::new());
        }
    }

    /// The document's `outerHTML` and the current address; a document over [`MAX_HTML`] is refused.
    pub fn capture(&self) -> Result<(String, String)> {
        self.capture_within(MAX_HTML)
    }

    /// [`capture`](Self::capture) with the cap as a parameter, so a test can prove the refusal with a few bytes instead of
    /// moving [`MAX_HTML`] through a debug build on a loaded machine (that took longer than [`CALL`] on a CI runner).
    pub(crate) fn capture_within(&self, max: usize) -> Result<(String, String)> {
        let v =
            self.evaluate("({html: document.documentElement.outerHTML, url: location.href})")?;
        let html = v
            .get("html")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if html.len() > max {
            return Err(Error::Protocol(
                "the page is larger than the allowed size".into(),
            ));
        }
        Ok((
            html,
            v.get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        ))
    }

    /// A PNG of the page, base64-encoded (diagnostics).
    pub fn screenshot_png(&self) -> Result<String> {
        let r = self.call("Page.captureScreenshot", json!({"format": "png"}))?;
        str_of(&r, "data")
    }

    /// Blocks images, fonts and media (not needed when nobody looks at the page), or lifts the block.
    pub fn block_heavy(&self, on: bool) -> Result<()> {
        let urls: &[&str] = if on { HEAVY } else { &[] };
        self.call("Network.setBlockedURLs", json!({"urls": urls}))
            .map(|_| ())
    }
}

/// An event that ends the page with this session and target.
fn ends(e: &Event, session: &str, target: &str) -> bool {
    let id = |k: &str| e.params.get(k).and_then(Value::as_str);
    matches!(
        e.method.as_str(),
        "Target.detachedFromTarget" | "Target.targetDestroyed" | "Target.targetCrashed"
    ) && (id("sessionId") == Some(session) || id("targetId") == Some(target))
}

/// The address of a document navigation in the same frame that is not the one `reply` (to `Page.navigate`) belongs to;
/// a same-document change (`pushState`, a fragment) never replaces a load.
fn replaced_by(e: &Event, session: &str, reply: &Value) -> Option<String> {
    let p = &e.params;
    (e.method == "Page.frameStartedNavigating"
        && !matches!(
            p.get("navigationType").and_then(Value::as_str),
            Some("sameDocument" | "historySameDocument")
        )
        && e.session.as_deref() == Some(session)
        && p.get("frameId") == reply.get("frameId")
        && p.get("loaderId") != reply.get("loaderId"))
    .then(|| {
        p.get("url")
            .and_then(Value::as_str)
            .unwrap_or("another address")
            .to_string()
    })
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

    #[test]
    fn only_events_of_this_page_end_it() {
        let ev = |m: &str, p: Value| Event {
            method: m.into(),
            session: None,
            params: p,
        };
        assert!(ends(
            &ev("Target.detachedFromTarget", json!({"sessionId": "S"})),
            "S",
            "T"
        ));
        assert!(ends(
            &ev("Target.targetCrashed", json!({"targetId": "T"})),
            "S",
            "T"
        ));
        assert!(!ends(
            &ev("Target.targetDestroyed", json!({"targetId": "X"})),
            "S",
            "T"
        ));
        assert!(!ends(
            &ev("Page.loadEventFired", json!({"targetId": "T"})),
            "S",
            "T"
        ));
        assert!(
            str_of(&json!({}), "x")
                .unwrap_err()
                .to_string()
                .contains("no x")
        );
    }

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
            m.contains("replaced")
                && m.contains("http://a.test/next")
                && m.contains("net::ERR_ABORTED"),
            "{m}"
        );
        let plain = not_loaded("net::ERR_NAME_NOT_RESOLVED", &r, None).to_string();
        assert!(
            plain.ends_with("the page did not load: net::ERR_NAME_NOT_RESOLVED"),
            "{plain}"
        );
    }

    #[test]
    fn only_another_loader_in_the_same_frame_and_session_counts_as_a_replacement() {
        let r = json!({"frameId": "F", "loaderId": "L1"});
        assert_eq!(
            replaced_by(&started("S", "F", "L2", "http://a.test/x"), "S", &r).as_deref(),
            Some("http://a.test/x")
        );
        // The load itself, a child frame, another tab and other events are not replacements.
        assert_eq!(
            replaced_by(&started("S", "F", "L1", "http://a.test/"), "S", &r),
            None
        );
        assert_eq!(
            replaced_by(&started("S", "child", "L2", "http://a.test/ad"), "S", &r),
            None
        );
        assert_eq!(
            replaced_by(&started("T", "F", "L2", "http://a.test/x"), "S", &r),
            None
        );
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
