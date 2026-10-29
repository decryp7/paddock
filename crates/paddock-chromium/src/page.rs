//! One page (tab) of a running browser: navigate, observe, read. A page knows when it is gone (the user closed its window
//! or tab, it crashed, the connection ended): every call then fails with [`Error::Closed`], whatever the browser answers.

use crate::cdp::{Connection, Event, Waited};
use crate::challenge::{self, Challenge, Signals};
use crate::ready::{self, Observation, Readiness, Rule};
use crate::{Error, Result};
use paddock_jobs::CancelToken;
use paddock_jobs::time::{Clock, Deadline};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

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
    /// The event mark from before the tab was created (or adopted): a failed load's trace starts here.
    pub(crate) born: u64,
    /// The navigation that was still running when the page was attached (see [`pending_at_enable`]), and the event mark
    /// from before `Page.enable` reported it: the first [`Page::navigate`] waits until it has settled.
    pub(crate) initial: Option<(Initial, u64)>,
    /// Set once `initial` has settled (its events may since have left the bounded event log).
    pub(crate) initial_settled: AtomicBool,
}

/// A navigation of the tab's main frame that `Page.enable` reported as running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Initial {
    frame: String,
    loader: String,
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
        let born = conn.mark();
        let t = conn.call(
            None,
            "Target.createTarget",
            json!({"url": crate::launch::START_PAGE}),
            CALL,
        )?;
        let target = str_of(&t, "targetId")?;
        Page::attach_to(conn, target, clock, born)
    }

    /// Attaches to the existing page `target` (the one the browser opened with).
    pub(crate) fn adopt(
        conn: Arc<Connection>,
        target: String,
        clock: Arc<dyn Clock>,
    ) -> Result<Page> {
        let born = conn.mark();
        Page::attach_to(conn, target, clock, born)
    }

    fn attach_to(
        conn: Arc<Connection>,
        target: String,
        clock: Arc<dyn Clock>,
        born: u64,
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
        let mut p = Page {
            conn,
            session,
            target,
            clock,
            born,
            initial: None,
            initial_settled: AtomicBool::new(false),
        };
        let enabling = p.conn.mark();
        p.call("Page.enable", json!({}))?;
        // A tab's own first navigation (`about:blank` from `Target.createTarget`, or the start page) may still be
        // running: `Page.enable` then reports it as `Page.frameStartedNavigating` before its reply (Chromium's
        // PageHandler::Enable, read 2026-10-28). A `Page.navigate` sent meanwhile can come back `net::ERR_ABORTED`.
        // Only the main frame's navigation is the tab's own (its frame id from the frame tree; for a page target Chrome
        // uses the target id, the fallback).
        let main = p
            .call("Page.getFrameTree", json!({}))
            .ok()
            .and_then(|r| {
                r.pointer("/frameTree/frame/id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| p.target.clone());
        p.initial = pending_at_enable(&p.conn.events_since(enabling), &p.session, &main)
            .map(|i| (i, enabling));
        // For a load that fails, what the browser reported about it (see `trace`): a handful of lifecycle events per
        // document, and the document request's network events (the connection keeps only documents' network events).
        let _ = p.call("Page.setLifecycleEventsEnabled", json!({"enabled": true}));
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

    /// Waits, at most `timeout`, until the navigation that was running when the page was attached has committed or
    /// stopped; returns at once when there was none or it already did.
    fn settle_initial(&self, timeout: Duration) -> Result<()> {
        let Some((initial, since)) = &self.initial else {
            return Ok(());
        };
        if self.initial_settled.load(Ordering::Acquire) {
            return Ok(());
        }
        let (s, t) = (self.session.clone(), self.target.clone());
        let waited = self.conn.wait_event_or_lost(*since, timeout, |e| {
            settles(e, initial, &s) || ends(e, &s, &t)
        });
        if self.is_closed() {
            return Err(Error::Closed);
        }
        // Lost: the events since `Page.enable` outran the bounded log, so the settling one may have gone with them. A
        // page that busy is past its first navigation; waiting out the whole timeout for an event nobody can see would
        // only turn a load into a timeout.
        if let Waited::TimedOut = waited {
            return Err(Error::Timeout("the tab's first page"));
        }
        self.initial_settled.store(true, Ordering::Release);
        Ok(())
    }

    /// Loads `url` and waits for the load event (or `timeout`). On a new tab it first waits (within the same `timeout`)
    /// until the tab's own first navigation has settled, so that one cannot cancel this load.
    pub fn navigate(&self, url: &str, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        self.settle_initial(timeout)?;
        let timeout = timeout.saturating_sub(start.elapsed());
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
            let trace = trace(&self.conn.events_since(self.born), since, &self.session);
            return Err(not_loaded(e, &r, newer.as_deref(), &trace));
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
            .map_or_else(|| "another address".to_string(), brief_url)
    })
}

/// The navigation of the main frame `main_frame` that `Page.enable` reported as running (its
/// `Page.frameStartedNavigating` for this session, sent before its reply), if any; a child frame's navigation and a
/// same-document change are not one.
fn pending_at_enable(events: &[(u64, Event)], session: &str, main_frame: &str) -> Option<Initial> {
    events.iter().find_map(|(_, e)| {
        let p = &e.params;
        (e.method == "Page.frameStartedNavigating"
            && e.session.as_deref() == Some(session)
            && p.get("frameId").and_then(Value::as_str) == Some(main_frame)
            && !matches!(
                p.get("navigationType").and_then(Value::as_str),
                Some("sameDocument" | "historySameDocument")
            ))
        .then(|| Initial {
            frame: p
                .get("frameId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            loader: p
                .get("loaderId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })
    })
}

/// An event that ends the running navigation `initial`: it committed (`Page.frameNavigated` with its loader) or its
/// frame stopped loading (committed and loaded, failed, or cancelled).
fn settles(e: &Event, initial: &Initial, session: &str) -> bool {
    let p = &e.params;
    let id = |ptr: &str| p.pointer(ptr).and_then(Value::as_str);
    e.session.as_deref() == Some(session)
        && match e.method.as_str() {
            "Page.frameStoppedLoading" => id("/frameId") == Some(&initial.frame),
            "Page.frameNavigated" => {
                id("/frame/id") == Some(&initial.frame)
                    && id("/frame/loaderId") == Some(&initial.loader)
            }
            _ => false,
        }
}

/// Why `Page.navigate` answered with `error` (its `errorText`) instead of a page. Chrome reports every navigation it
/// cancels before the page commits as `net::ERR_ABORTED`: a response that is a download or has no content (a 204), and a
/// navigation that another one replaced. A download and a replacement get their own message; anything else keeps the
/// code, and an abort that is neither adds what Chrome reported about the tab until then (`trace`), so a cause nobody has
/// named yet can be read off it. Other codes (an unknown host, a refused connection) already say what happened.
fn not_loaded(error: &str, reply: &Value, replaced_by: Option<&str>, trace: &str) -> Error {
    if reply.get("isDownload").and_then(Value::as_bool) == Some(true) {
        return Error::Protocol("the address is a file download, not a page".into());
    }
    match replaced_by {
        Some(url) => Error::Protocol(format!(
            "the page did not load: another navigation (to {url}) replaced it ({error})"
        )),
        None if trace.is_empty() || error != "net::ERR_ABORTED" => {
            Error::Protocol(format!("the page did not load: {error}"))
        }
        None => Error::Protocol(format!(
            "the page did not load: {error} (what the browser reported: {trace})"
        )),
    }
}

/// The most recent entries [`trace`] keeps.
const TRACE_ENTRIES: usize = 24;
/// The longest text [`trace`] returns.
const TRACE_CHARS: usize = 2000;

/// A short account of the tab's navigation events in `events` (from its creation on), with a marker where
/// `Page.navigate` was sent (`navigate`, an event mark): requested and started navigations, commits, lifecycle steps,
/// the document request's network events (sent, answered, failed with its reason), downloads, dialogs, crashes and
/// detaches. Bounded to the last [`TRACE_ENTRIES`] entries and [`TRACE_CHARS`] characters.
fn trace(events: &[(u64, Event)], navigate: u64, session: &str) -> String {
    let mut entries: Vec<String> = Vec::new();
    let mut marked = false;
    for (seq, e) in events {
        if !marked && *seq > navigate {
            entries.push("[Page.navigate sent]".into());
            marked = true;
        }
        let ours = e.session.as_deref() == Some(session);
        let browser_level = e.session.is_none()
            && (e.method.starts_with("Target.") || e.method.starts_with("Inspector."));
        if let Some(d) = (ours || browser_level).then(|| describe(e)).flatten() {
            entries.push(d);
        }
    }
    if !marked {
        entries.push("[Page.navigate sent]".into());
    }
    let skip = entries.len().saturating_sub(TRACE_ENTRIES);
    let mut out = if skip > 0 {
        format!("... {skip} earlier; ")
    } else {
        String::new()
    };
    out.push_str(&entries[skip..].join("; "));
    if out.chars().count() > TRACE_CHARS {
        out = out.chars().take(TRACE_CHARS).collect::<String>() + "...";
    }
    out
}

/// An address cut to its scheme, host and path: no user name, password, query or fragment (the trace ends up in error
/// messages and logs). An address that is not hierarchical (no `//` right after the scheme: `data:`, `javascript:`,
/// `blob:`) keeps only its scheme, except `about:` pages; text without a scheme is left out.
fn brief_url(url: &str) -> String {
    let url = url.find(['?', '#']).map_or(url, |i| &url[..i]);
    let Some((scheme, rest)) = url.split_once(':') else {
        return String::new();
    };
    if scheme == "about" {
        return url.to_string();
    }
    match rest.strip_prefix("//") {
        Some(rest) => {
            let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
            let host = authority.rsplit('@').next().unwrap_or(authority);
            format!("{scheme}://{host}{path}")
        }
        None => format!("{scheme}:"),
    }
}

/// One trace entry, or `None` for an event that says nothing about why a load stopped.
fn describe(e: &Event) -> Option<String> {
    let p = &e.params;
    let s = |ptr: &str| -> String { p.pointer(ptr).and_then(Value::as_str).unwrap_or("").into() };
    let url = |ptr: &str| brief_url(&s(ptr));
    let raw = |k: &str| p.pointer(k).map(Value::to_string).unwrap_or_default();
    let short = e.method.rsplit('.').next().unwrap_or(&e.method);
    let with = |fields: &[(&str, String)]| {
        let f: Vec<String> = fields
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        format!("{short} {}", f.join(" ")).trim_end().to_string()
    };
    let document = s("/type") == "Document";
    Some(match e.method.as_str() {
        "Page.frameRequestedNavigation" => with(&[("reason", s("/reason")), ("url", url("/url"))]),
        "Page.frameStartedNavigating" => {
            with(&[("type", s("/navigationType")), ("url", url("/url"))])
        }
        "Page.frameNavigated" => with(&[("url", url("/frame/url"))]),
        "Page.frameStartedLoading"
        | "Page.frameStoppedLoading"
        | "Page.loadEventFired"
        | "Page.domContentEventFired" => short.to_string(),
        "Page.frameDetached" => with(&[("reason", s("/reason"))]),
        "Page.lifecycleEvent" => with(&[("name", s("/name"))]),
        "Page.downloadWillBegin" => with(&[("url", url("/url"))]),
        "Page.javascriptDialogOpening" => with(&[("type", s("/type"))]),
        "Network.requestWillBeSent" if document => with(&[("url", url("/request/url"))]),
        "Network.responseReceived" if document => with(&[
            ("status", raw("/response/status")),
            ("mime", s("/response/mimeType")),
        ]),
        "Network.loadingFailed" => with(&[
            ("type", s("/type")),
            ("error", s("/errorText")),
            ("canceled", raw("/canceled")),
            ("blocked", s("/blockedReason")),
            ("cors", s("/corsErrorStatus/corsError")),
        ]),
        "Inspector.targetCrashed"
        | "Inspector.detached"
        | "Target.targetCrashed"
        | "Target.detachedFromTarget" => {
            with(&[("reason", s("/reason")), ("status", s("/status"))])
        }
        _ => return None,
    })
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
        let m = not_loaded("net::ERR_ABORTED", &r, None, "").to_string();
        assert!(m.contains("file download"), "{m}");
    }

    #[test]
    fn a_replaced_load_names_the_navigation_that_replaced_it() {
        let r = json!({"frameId": "F", "loaderId": "L1", "isDownload": false, "errorText": "net::ERR_ABORTED"});
        let m = not_loaded("net::ERR_ABORTED", &r, Some("http://a.test/next"), "x").to_string();
        assert!(
            m.contains("replaced")
                && m.contains("http://a.test/next")
                && m.contains("net::ERR_ABORTED"),
            "{m}"
        );
        let plain = not_loaded(
            "net::ERR_NAME_NOT_RESOLVED",
            &r,
            None,
            "[Page.navigate sent]",
        )
        .to_string();
        assert!(
            plain.ends_with("the page did not load: net::ERR_NAME_NOT_RESOLVED"),
            "{plain}"
        );
    }

    fn ev(method: &str, session: Option<&str>, params: Value) -> Event {
        Event {
            method: method.into(),
            session: session.map(str::to_string),
            params,
        }
    }

    #[test]
    fn an_unexplained_abort_carries_what_the_browser_reported() {
        let events = vec![
            (1, started("S", "F", "L0", "about:blank")),
            (
                2,
                ev("Page.lifecycleEvent", Some("S"), json!({"name": "init"})),
            ),
            (
                3,
                ev(
                    "Network.requestWillBeSent",
                    Some("S"),
                    json!({"type": "Document", "request": {"url": "http://a.test/"}}),
                ),
            ),
            // Subresources, other tabs and silent events stay out.
            (
                4,
                ev(
                    "Network.requestWillBeSent",
                    Some("S"),
                    json!({"type": "Script", "request": {"url": "http://a.test/x.js"}}),
                ),
            ),
            (
                5,
                ev(
                    "Page.frameNavigated",
                    Some("T"),
                    json!({"frame": {"url": "http://other.test/"}}),
                ),
            ),
            (6, ev("Network.dataReceived", Some("S"), json!({}))),
            (
                7,
                ev(
                    "Network.responseReceived",
                    Some("S"),
                    json!({"type": "Document", "response": {"status": 200, "mimeType": "text/html"}}),
                ),
            ),
            (
                8,
                ev(
                    "Network.loadingFailed",
                    Some("S"),
                    json!({"type": "Document", "errorText": "net::ERR_ABORTED", "canceled": true, "blockedReason": "inspector", "corsErrorStatus": {"corsError": "X"}}),
                ),
            ),
            (9, ev("Inspector.targetCrashed", None, json!({}))),
            (
                10,
                ev(
                    "Page.frameNavigated",
                    Some("S"),
                    json!({"frame": {"url": "http://a.test/"}}),
                ),
            ),
            (
                11,
                ev(
                    "Page.frameRequestedNavigation",
                    Some("S"),
                    json!({"reason": "scriptInitiated", "url": "http://a.test/n"}),
                ),
            ),
            (
                12,
                ev(
                    "Page.frameStoppedLoading",
                    Some("S"),
                    json!({"frameId": "F"}),
                ),
            ),
            (
                13,
                ev("Page.frameDetached", Some("S"), json!({"reason": "remove"})),
            ),
            (
                14,
                ev(
                    "Page.downloadWillBegin",
                    Some("S"),
                    json!({"url": "http://a.test/z.zip"}),
                ),
            ),
            (
                15,
                ev(
                    "Page.javascriptDialogOpening",
                    Some("S"),
                    json!({"type": "beforeunload"}),
                ),
            ),
            (
                16,
                ev("Target.detachedFromTarget", None, json!({"reason": "gone"})),
            ),
            (17, ev("Target.attachedToTarget", None, json!({}))),
        ];
        let t = trace(&events, 2, "S");
        assert_eq!(
            t,
            "frameStartedNavigating type=differentDocument url=about:blank; lifecycleEvent name=init; [Page.navigate sent]; \
             requestWillBeSent url=http://a.test/; responseReceived status=200 mime=text/html; \
             loadingFailed type=Document error=net::ERR_ABORTED canceled=true blocked=inspector cors=X; targetCrashed; \
             frameNavigated url=http://a.test/; frameRequestedNavigation reason=scriptInitiated url=http://a.test/n; \
             frameStoppedLoading; frameDetached reason=remove; downloadWillBegin url=http://a.test/z.zip; \
             javascriptDialogOpening type=beforeunload; detachedFromTarget reason=gone"
        );
        let r = json!({"frameId": "F", "loaderId": "L1", "isDownload": false});
        let m = not_loaded("net::ERR_ABORTED", &r, None, &t).to_string();
        assert!(
            m.contains("net::ERR_ABORTED (what the browser reported: frameStartedNavigating"),
            "{m}"
        );
        // Nothing after the navigation still marks where it was sent.
        assert_eq!(
            trace(&events[..1], 9, "S"),
            "frameStartedNavigating type=differentDocument url=about:blank; [Page.navigate sent]"
        );
        assert_eq!(trace(&[], 0, "S"), "[Page.navigate sent]");
    }

    #[test]
    fn addresses_in_the_trace_keep_only_scheme_host_and_path() {
        assert_eq!(
            brief_url("https://user:secret@a.test:8443/p/q?token=abc#frag"),
            "https://a.test:8443/p/q"
        );
        assert_eq!(brief_url("http://a.test"), "http://a.test");
        assert_eq!(brief_url("http://a.test#x"), "http://a.test");
        assert_eq!(brief_url("about:blank"), "about:blank");
        assert_eq!(brief_url("data:text/html,<p>secret</p>"), "data:");
        // A `://` later in the text does not make it hierarchical.
        assert_eq!(
            brief_url("data:text/html,<a href=http://x/secret>"),
            "data:"
        );
        assert_eq!(
            brief_url("javascript:fetch('https://x.test/steal')"),
            "javascript:"
        );
        assert_eq!(brief_url("no scheme here"), "");
        assert_eq!(brief_url(""), "");
        let e = ev(
            "Network.requestWillBeSent",
            Some("S"),
            json!({"type": "Document", "request": {"url": "https://a.test/x?session=1"}}),
        );
        assert_eq!(
            describe(&e).unwrap(),
            "requestWillBeSent url=https://a.test/x"
        );
    }

    #[test]
    fn the_trace_is_bounded() {
        let many: Vec<(u64, Event)> = (0..100)
            .map(|i| {
                (
                    i,
                    ev(
                        "Page.lifecycleEvent",
                        Some("S"),
                        json!({"name": "x".repeat(200)}),
                    ),
                )
            })
            .collect();
        let t = trace(&many, 1000, "S");
        assert!(t.starts_with("... 77 earlier; "), "{}", &t[..40]);
        assert!(t.chars().count() <= TRACE_CHARS + 3 && t.ends_with("..."));
    }

    #[test]
    fn the_navigation_running_at_enable_is_found_and_settles_on_its_commit_or_stop() {
        let at_enable = vec![
            (
                1,
                ev("Page.lifecycleEvent", Some("S"), json!({"name": "init"})),
            ),
            (2, started("T", "G", "L9", "about:blank")),
            // A child frame's navigation is not the tab's: only the main frame's is waited for.
            (3, started("S", "child", "LC", "about:blank")),
            (4, started("S", "F", "L0", "about:blank")),
        ];
        assert_eq!(pending_at_enable(&at_enable[..3], "S", "F"), None);
        let i = pending_at_enable(&at_enable, "S", "F").unwrap();
        assert_eq!(
            i,
            Initial {
                frame: "F".into(),
                loader: "L0".into()
            }
        );
        // Nothing running, or only a same-document change: nothing to wait for.
        assert_eq!(pending_at_enable(&at_enable[..2], "S", "F"), None);
        let mut same = started("S", "F", "L1", "about:blank#x");
        same.params["navigationType"] = json!("sameDocument");
        assert_eq!(pending_at_enable(&[(1, same)], "S", "F"), None);
        let nav = |frame: &str, loader: &str| {
            ev(
                "Page.frameNavigated",
                Some("S"),
                json!({"frame": {"id": frame, "loaderId": loader}}),
            )
        };
        assert!(settles(&nav("F", "L0"), &i, "S"));
        assert!(settles(
            &ev(
                "Page.frameStoppedLoading",
                Some("S"),
                json!({"frameId": "F"})
            ),
            &i,
            "S"
        ));
        // Another loader, another frame, another tab or another event does not settle it.
        assert!(!settles(&nav("F", "L1"), &i, "S"));
        assert!(!settles(&nav("child", "L0"), &i, "S"));
        assert!(!settles(
            &ev(
                "Page.frameStoppedLoading",
                Some("S"),
                json!({"frameId": "child"})
            ),
            &i,
            "S"
        ));
        assert!(!settles(
            &ev(
                "Page.frameStoppedLoading",
                Some("T"),
                json!({"frameId": "F"})
            ),
            &i,
            "S"
        ));
        assert!(!settles(
            &ev(
                "Page.lifecycleEvent",
                Some("S"),
                json!({"frameId": "F", "name": "load"})
            ),
            &i,
            "S"
        ));
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
        // The address that replaced the load is named without its query or fragment (error text reaches the logs).
        assert_eq!(
            replaced_by(
                &started("S", "F", "L4", "http://a.test/next?token=abc#f"),
                "S",
                &r
            )
            .as_deref(),
            Some("http://a.test/next")
        );
        // A replacement without an address still counts.
        let mut e = started("S", "F", "L3", "");
        e.params.as_object_mut().unwrap().remove("url");
        assert_eq!(replaced_by(&e, "S", &r).as_deref(), Some("another address"));
    }
}
