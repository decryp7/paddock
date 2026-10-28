//! One page (tab) of a running browser: navigate, observe, read.

use crate::cdp::{Connection, Event};
use crate::challenge::{self, Challenge};
use crate::ready::{self, Observation, Readiness, Rule};
use crate::{Error, Result};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// The event mark from before the tab was created: a failed load's trace starts here, so it shows the tab's own
    /// first (`about:blank`) navigation too.
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
        let born = conn.mark();
        let t = conn.call(None, "Target.createTarget", json!({"url": "about:blank"}), CALL)?;
        let target = str_of(&t, "targetId")?;
        let s = conn.call(None, "Target.attachToTarget", json!({"targetId": target, "flatten": true}), CALL)?;
        let session = str_of(&s, "sessionId")?;
        let mut p = Page {
            conn,
            session,
            born,
            initial: None,
            initial_settled: AtomicBool::new(false),
        };
        let enabling = p.conn.mark();
        p.call("Page.enable", json!({}))?;
        // The tab's own first navigation (`about:blank` from `Target.createTarget`) may still be running: `Page.enable`
        // then reports it as `Page.frameStartedNavigating` before its reply (Chromium's PageHandler::Enable, read
        // 2026-10-28). A `Page.navigate` sent meanwhile can come back `net::ERR_ABORTED`.
        p.initial = pending_at_enable(&p.conn.events_since(enabling), &p.session).map(|i| (i, enabling));
        // For a load that fails, what the browser reported about it (see `trace`): a handful of lifecycle events per
        // document, and the document request's network events (`Network.loadingFailed` carries `canceled`,
        // `blockedReason` and `corsErrorStatus`; the connection keeps only documents' network events). Enabled here,
        // before any load: sent while a `Page.navigate` is still unanswered, `Network.enable` got no answer within the
        // call timeout (measured 2026-10-28, Chrome for Testing 156). Nothing reads response bodies, so the buffers are small.
        let _ = p.call("Page.setLifecycleEventsEnabled", json!({"enabled": true}));
        let _ = p.call(
            "Network.enable",
            json!({"maxTotalBufferSize": 1 << 20, "maxResourceBufferSize": 256 << 10}),
        );
        p.call("Page.addScriptToEvaluateOnNewDocument", json!({"source": TRACKER}))?;
        // Downloads are never wanted from a page that is only read.
        let _ = p.conn.call(None, "Browser.setDownloadBehavior", json!({"behavior": "deny"}), CALL);
        Ok(p)
    }

    fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.conn.call(Some(&self.session), method, params, CALL)
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
        let ev = self.conn.wait_event(*since, timeout, |e| settles(e, initial, &self.session));
        if self.conn.is_closed() {
            return Err(Error::Protocol(
                "the browser closed the connection while the page was loading".into(),
            ));
        }
        ev.ok_or(Error::Timeout("the tab's first page"))?;
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
                .wait_event(since, Duration::ZERO, |ev| replaced_by(ev, &self.session, &r).is_some())
                .and_then(|ev| replaced_by(&ev, &self.session, &r));
            let trace = trace(&self.conn.events_since(self.born), since, &self.session);
            return Err(not_loaded(e, &r, newer.as_deref(), &trace));
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
/// navigation that another one replaced. A download and a replacement get their own message; anything else keeps the code,
/// and an abort that is neither adds what Chrome reported about the tab until then (`trace`), so a cause nobody has named
/// yet can be read off it. Other codes (an unknown host, a refused connection) already say what happened.
fn not_loaded(error: &str, reply: &Value, replaced_by: Option<&str>, trace: &str) -> Error {
    if reply.get("isDownload").and_then(Value::as_bool) == Some(true) {
        return Error::Protocol("the address is a file download, not a page".into());
    }
    match replaced_by {
        Some(url) => Error::Protocol(format!(
            "the page did not load: another navigation (to {url}) replaced it ({error})"
        )),
        None if trace.is_empty() || error != "net::ERR_ABORTED" => Error::Protocol(format!("the page did not load: {error}")),
        None => Error::Protocol(format!("the page did not load: {error} (what the browser reported: {trace})")),
    }
}

/// The navigation `Page.enable` reported as running (its `Page.frameStartedNavigating` for this session, sent before
/// its reply), if any; a same-document change is not one.
fn pending_at_enable(events: &[(u64, Event)], session: &str) -> Option<Initial> {
    events.iter().find_map(|(_, e)| {
        let p = &e.params;
        (e.method == "Page.frameStartedNavigating"
            && e.session.as_deref() == Some(session)
            && !matches!(
                p.get("navigationType").and_then(Value::as_str),
                Some("sameDocument" | "historySameDocument")
            ))
        .then(|| Initial {
            frame: p.get("frameId").and_then(Value::as_str).unwrap_or("").to_string(),
            loader: p.get("loaderId").and_then(Value::as_str).unwrap_or("").to_string(),
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
            "Page.frameNavigated" => id("/frame/id") == Some(&initial.frame) && id("/frame/loaderId") == Some(&initial.loader),
            _ => false,
        }
}

/// The most recent entries [`trace`] keeps.
const TRACE_ENTRIES: usize = 24;
/// The longest text [`trace`] returns.
const TRACE_CHARS: usize = 2000;

/// A short account of the tab's navigation events in `events` (from its creation on), with `navigate` marking where
/// `Page.navigate` was sent: requested and started navigations, commits, lifecycle steps, the document request's network
/// events (sent, answered, failed with its reason), downloads, dialogs, crashes and detaches. Bounded to the last
/// [`TRACE_ENTRIES`] entries and [`TRACE_CHARS`] characters.
fn trace(events: &[(u64, Event)], navigate: u64, session: &str) -> String {
    let mut entries: Vec<String> = Vec::new();
    let mut marked = false;
    for (seq, e) in events {
        if !marked && *seq > navigate {
            entries.push("[Page.navigate sent]".into());
            marked = true;
        }
        let ours = e.session.as_deref() == Some(session);
        let browser_level = e.session.is_none() && (e.method.starts_with("Target.") || e.method.starts_with("Inspector."));
        if !(ours || browser_level) {
            continue;
        }
        if let Some(d) = describe(e) {
            entries.push(d);
        }
    }
    if !marked {
        entries.push("[Page.navigate sent]".into());
    }
    let skip = entries.len().saturating_sub(TRACE_ENTRIES);
    let mut out = if skip > 0 { format!("... {skip} earlier; ") } else { String::new() };
    out.push_str(&entries[skip..].join("; "));
    if out.chars().count() > TRACE_CHARS {
        out = out.chars().take(TRACE_CHARS).collect::<String>() + "...";
    }
    out
}

/// One trace entry, or `None` for an event that says nothing about why a load stopped.
fn describe(e: &Event) -> Option<String> {
    let p = &e.params;
    let s = |ptr: &str| p.pointer(ptr).and_then(Value::as_str).unwrap_or("");
    let short = |m: &str| m.rsplit('.').next().unwrap_or(m).to_string();
    let with = |fields: &[(&str, String)]| {
        let f: Vec<String> = fields
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        format!("{} {}", short(&e.method), f.join(" ")).trim_end().to_string()
    };
    let document = s("/type") == "Document";
    Some(match e.method.as_str() {
        "Page.frameRequestedNavigation" => with(&[("reason", s("/reason").into()), ("url", s("/url").into())]),
        "Page.frameStartedNavigating" => with(&[("type", s("/navigationType").into()), ("url", s("/url").into())]),
        "Page.frameNavigated" => with(&[("url", s("/frame/url").into())]),
        "Page.frameStartedLoading" | "Page.frameStoppedLoading" | "Page.loadEventFired" | "Page.domContentEventFired" => short(&e.method),
        "Page.frameDetached" => with(&[("reason", s("/reason").into())]),
        "Page.lifecycleEvent" => with(&[("name", s("/name").into())]),
        "Page.downloadWillBegin" => with(&[("url", s("/url").into())]),
        "Page.javascriptDialogOpening" => with(&[("type", s("/type").into())]),
        "Network.requestWillBeSent" if document => with(&[("url", s("/request/url").into())]),
        "Network.responseReceived" if document => with(&[
            ("status", p.pointer("/response/status").map(Value::to_string).unwrap_or_default()),
            ("mime", s("/response/mimeType").into()),
        ]),
        "Network.loadingFailed" => with(&[
            ("type", s("/type").into()),
            ("error", s("/errorText").into()),
            ("canceled", p.get("canceled").map(Value::to_string).unwrap_or_default()),
            ("blocked", s("/blockedReason").into()),
            ("cors", s("/corsErrorStatus/corsError").into()),
        ]),
        "Inspector.targetCrashed" | "Inspector.detached" | "Target.targetCrashed" | "Target.detachedFromTarget" => {
            with(&[("reason", s("/reason").into()), ("status", s("/status").into())])
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
        let m = not_loaded("net::ERR_ABORTED", &r, None, "").to_string();
        assert!(m.contains("file download"), "{m}");
    }

    #[test]
    fn a_replaced_load_names_the_navigation_that_replaced_it() {
        let r = json!({"frameId": "F", "loaderId": "L1", "isDownload": false, "errorText": "net::ERR_ABORTED"});
        let m = not_loaded("net::ERR_ABORTED", &r, Some("http://a.test/next"), "x").to_string();
        assert!(
            m.contains("replaced") && m.contains("http://a.test/next") && m.contains("net::ERR_ABORTED"),
            "{m}"
        );
        let plain = not_loaded("net::ERR_NAME_NOT_RESOLVED", &r, None, "[Page.navigate sent]").to_string();
        assert!(plain.ends_with("the page did not load: net::ERR_NAME_NOT_RESOLVED"), "{plain}");
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
            (
                1,
                ev(
                    "Page.frameStartedNavigating",
                    Some("S"),
                    json!({"url": "about:blank", "navigationType": "differentDocument"}),
                ),
            ),
            (2, ev("Page.lifecycleEvent", Some("S"), json!({"name": "init"}))),
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
                ev("Page.frameNavigated", Some("T"), json!({"frame": {"url": "http://other.test/"}})),
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
                    json!({"type": "Document", "errorText": "net::ERR_ABORTED", "canceled": true, "blockedReason": "inspector"}),
                ),
            ),
            (9, ev("Inspector.targetCrashed", None, json!({}))),
        ];
        let t = trace(&events, 2, "S");
        assert_eq!(
            t,
            "frameStartedNavigating type=differentDocument url=about:blank; lifecycleEvent name=init; [Page.navigate sent]; \
             requestWillBeSent url=http://a.test/; responseReceived status=200 mime=text/html; \
             loadingFailed type=Document error=net::ERR_ABORTED canceled=true blocked=inspector; targetCrashed"
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
    fn the_navigation_running_at_enable_is_found_and_settles_on_its_commit_or_stop() {
        let at_enable = vec![
            (1, ev("Page.lifecycleEvent", Some("S"), json!({"name": "init"}))),
            (2, started("T", "G", "L9", "about:blank")),
            (3, started("S", "F", "L0", "about:blank")),
        ];
        let i = pending_at_enable(&at_enable, "S").unwrap();
        assert_eq!(
            i,
            Initial {
                frame: "F".into(),
                loader: "L0".into()
            }
        );
        // Nothing running, or only a same-document change: nothing to wait for.
        assert_eq!(pending_at_enable(&at_enable[..2], "S"), None);
        let mut same = started("S", "F", "L1", "about:blank#x");
        same.params["navigationType"] = json!("sameDocument");
        assert_eq!(pending_at_enable(&[(1, same)], "S"), None);
        let nav = |frame: &str, loader: &str| {
            ev(
                "Page.frameNavigated",
                Some("S"),
                json!({"frame": {"id": frame, "loaderId": loader}}),
            )
        };
        let stopped = |s: &str, frame: &str| ev("Page.frameStoppedLoading", Some(s), json!({"frameId": frame}));
        assert!(settles(&nav("F", "L0"), &i, "S"));
        assert!(settles(&stopped("S", "F"), &i, "S"));
        // Another loader, another frame, another tab or another event does not settle it.
        assert!(!settles(&nav("F", "L1"), &i, "S"));
        assert!(!settles(&nav("child", "L0"), &i, "S"));
        assert!(!settles(&stopped("S", "child"), &i, "S"));
        assert!(!settles(&stopped("T", "F"), &i, "S"));
        assert!(!settles(
            &ev("Page.lifecycleEvent", Some("S"), json!({"frameId": "F", "name": "load"})),
            &i,
            "S"
        ));
    }

    #[test]
    fn the_trace_is_bounded() {
        let many: Vec<(u64, Event)> = (0..100)
            .map(|i| (i, ev("Page.lifecycleEvent", Some("S"), json!({"name": "x".repeat(200)}))))
            .collect();
        let t = trace(&many, 1000, "S");
        assert!(t.starts_with("... 77 earlier; "), "{}", &t[..40]);
        assert!(t.chars().count() <= TRACE_CHARS + 3 && t.ends_with("..."), "{}", t.len());
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
