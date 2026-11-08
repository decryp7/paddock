//! Live test of research's results-page step against the real search engines, in a real Chromium (ADR 0043; it replaces
//! the web view probe), and the same probe over recorded engine pages, which CI runs. The live test is ignored and never
//! runs in CI (tests do not depend on the real network): run it by hand with
//! `DEBRIEF_LIVE_SEARCH=1 cargo test -p debrief-app --lib live_search -- --ignored --nocapture`. It uses the same ports and
//! functions research does (`research::search`, `research::read_results`, `research::open_result`) through
//! [`ChromiumPage`], once per built-in engine, and fails with diagnostics (page kind, link counts, ready state, a
//! screenshot) unless an engine gives at least [`MIN_LINKS`] result links or a recognised consent/CAPTCHA hand-off.
//! `DEBRIEF_E2E_LIVE_SEARCH_OUT` names an absolute folder for `live-search.json` and the screenshots; the queries are
//! neutral (never user data) and the report keeps page addresses without their query string. The recorded probe
//! (`tests::recorded`) runs the same steps, verdicts and report over what the results-page collector returned on each
//! engine's page (`live_fixtures/*.json`, invented results), with a fake clock.

use super::renderer::ChromiumPage;
use super::research::ReadOnlyDriver;
use super::sessions::Renderers;
use debrief_ai::browser::research::{self, ResearchEnv, ResultsOutcome, ResultsRead};
use debrief_ai::browser::{
    AgentLimits, ChatBrowser, PageDiagnostics, PageDriver, PageKind, PageReader, ResultsReader, SearchEngine, SearchSettings,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const OUT_VAR: &str = "DEBRIEF_E2E_LIVE_SEARCH_OUT";
pub const QUERY_VAR: &str = "DEBRIEF_E2E_LIVE_SEARCH_QUERY";
pub const ENGINES_VAR: &str = "DEBRIEF_E2E_LIVE_SEARCH_ENGINES";
pub const DEFAULT_QUERY: &str = "System.TimeoutException";
pub const REPORT: &str = "live-search.json";
const MAX_QUERY: usize = 100;
pub const MIN_LINKS: usize = 3;
const PACE: Duration = Duration::from_secs(3);

static SLEEP: fn(Duration) = std::thread::sleep;
static NOW: fn() -> Instant = Instant::now;

/// The browser one engine is probed in, and the clock research waits by: the real Chromium and the wall clock in the
/// live test, a recorded web and a fake clock in the recorded one.
pub struct Probe<'a> {
    pub reader: &'a dyn PageReader,
    pub results: &'a dyn ResultsReader,
    pub driver: &'a dyn PageDriver,
    pub sleep: &'a (dyn Fn(Duration) + Sync),
    pub now: &'a (dyn Fn() -> Instant + Sync),
    /// Saves a screenshot of the results page shown to the path given.
    pub screenshot: &'a dyn Fn(&Path) -> Result<(), String>,
}

/// How one engine did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Pass,
    Warn,
    Fail,
}

/// The verdict on what research read and on opening its first result (`Ok(site)` where it was shown, or why not), and a
/// line saying why.
pub fn verdict(outcome: &ResultsOutcome, first: Option<&Result<String, String>>) -> (Verdict, String) {
    match outcome {
        ResultsOutcome::Links(l) if l.len() >= MIN_LINKS => match first {
            Some(Ok(site)) => (Verdict::Pass, format!("{} result links; the first one opened on {site}", l.len())),
            Some(Err(why)) => (Verdict::Fail, format!("{} result links, but the first one {why}", l.len())),
            None => (Verdict::Fail, format!("{} result links; the first one was not opened", l.len())),
        },
        ResultsOutcome::Links(l) => (
            Verdict::Fail,
            format!("only {} result links (at least {MIN_LINKS} expected)", l.len()),
        ),
        ResultsOutcome::NoLinks => (Verdict::Fail, "the results page showed no candidate result links".into()),
        ResultsOutcome::HandOff(kind) => (Verdict::Warn, format!("handed to the user: {}", describe(*kind))),
        ResultsOutcome::NotShown => (Verdict::Fail, "the results page never showed".into()),
    }
}

fn describe(kind: PageKind) -> &'static str {
    match kind {
        PageKind::Consent => "a consent page",
        PageKind::Captcha => "a human check",
        PageKind::Redirect => "a redirect page",
        PageKind::Results => "a results page",
    }
}

/// One engine's line of the report.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineReport {
    pub engine: &'static str,
    pub label: &'static str,
    pub verdict: Verdict,
    pub note: String,
    pub links: usize,
    /// The result links' sites (never their addresses).
    pub result_sites: Vec<String>,
    pub looks: usize,
    pub ready: bool,
    pub elapsed_ms: u64,
    pub page: Option<PageDiagnostics>,
    pub screenshot: Option<String>,
    pub screenshot_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report<'a> {
    query: &'a str,
    platform: &'static str,
    done: bool,
    engines: &'a [EngineReport],
}

/// The built-in engines (every one but Custom), or those `only` lists by id (comma-separated; unknown ids are ignored).
pub fn selected(only: Option<&str>) -> Vec<SearchEngine> {
    let all = SearchEngine::ALL.into_iter().filter(|e| *e != SearchEngine::Custom);
    match only.map(str::trim).filter(|s| !s.is_empty()) {
        None => all.collect(),
        Some(list) => all.filter(|e| list.split(',').any(|id| id.trim() == e.id())).collect(),
    }
}

/// The query: the variable's (trimmed, printable, at most [`MAX_QUERY`] characters) or [`DEFAULT_QUERY`].
pub fn query_from(var: Option<String>) -> Result<String, String> {
    match var {
        None => Ok(DEFAULT_QUERY.into()),
        Some(q) => {
            let q = q.trim();
            if q.is_empty() || q.chars().count() > MAX_QUERY || q.chars().any(char::is_control) {
                Err(format!("{QUERY_VAR} must be 1 to {MAX_QUERY} printable characters"))
            } else {
                Ok(q.to_string())
            }
        }
    }
}

struct NoLog;
impl debrief_ai::browser::ActionLog for NoLog {
    fn record(&self, _: &debrief_ai::browser::ActionEntry) {}
}

fn screenshot(renderers: &Renderers, path: &Path) -> Result<(), String> {
    use base64::Engine as _;
    let s = renderers.current("live").ok_or("the browser is not open")?;
    let v = s.page().screenshot_png().map_err(|e| e.to_string())?;
    let png = base64::engine::general_purpose::STANDARD.decode(v).map_err(|e| e.to_string())?;
    std::fs::write(path, png).map_err(|e| e.to_string())
}

fn read(
    probe: &Probe<'_>,
    engine: SearchEngine,
    query: &str,
    results_shown: &mut dyn FnMut(),
) -> Result<(ResultsRead, Option<Result<String, String>>), String> {
    let cancel = AtomicBool::new(false);
    let env = ResearchEnv {
        reader: probe.reader,
        results: probe.results,
        driver: probe.driver,
        search: SearchSettings { engine, custom: None },
        log: &NoLog,
        id: "live".into(),
        cancel: &cancel,
        pacing: debrief_ai::browser::Pacing::default(),
        jitter: &|| 0.5,
        searches: &debrief_ai::browser::SearchClock::default(),
        cache: None,
        total: research::TOTAL_TIME,
        sleep: probe.sleep,
        now: probe.now,
    };
    research::search(&env, query)
        .and_then(|url| {
            let read = research::read_results(&env, &url, 10)?;
            results_shown();
            let first = match &read.outcome {
                ResultsOutcome::Links(links) => match links.first() {
                    Some((link, _)) => {
                        (probe.sleep)(debrief_ai::browser::gentle::DEFAULT_PAUSE_MIN);
                        Some(research::open_result(&env, link, &url, 1)?.map(|o| o.url.site().to_string()))
                    }
                    None => None,
                },
                _ => None,
            };
            Ok((read, first))
        })
        .map_err(|e| e.to_string())
}

/// Probes one engine and reports how it did; the screenshot of its results page goes into `out`.
pub fn probe(probe: &Probe<'_>, engine: SearchEngine, query: &str, out: &Path) -> EngineReport {
    let started = (probe.now)();
    let file = format!("live-{}.png", engine.id());
    let mut shot = Err("no screenshot taken".to_string());
    let read = read(probe, engine, query, &mut || shot = (probe.screenshot)(&out.join(&file)));
    let (verdict, note, read) = match read {
        Ok((r, first)) => {
            let (v, n) = verdict(&r.outcome, first.as_ref());
            (v, n, Some(r))
        }
        // A page that waits for a person (consent, CAPTCHA) is the recognised hand-off, not a failure.
        Err(e) if e.contains("still needs you") => (Verdict::Warn, format!("handed to the user: {e}"), None),
        Err(e) => (Verdict::Fail, e, None),
    };
    let elapsed_ms = u64::try_from((probe.now)().saturating_duration_since(started).as_millis()).unwrap_or(u64::MAX);
    let (screenshot, screenshot_error) = match shot {
        Ok(()) => (Some(file), None),
        Err(e) => (None, Some(e)),
    };
    let links: Vec<String> = match read.as_ref().map(|r| &r.outcome) {
        Some(ResultsOutcome::Links(l)) => l.iter().map(|(u, _)| u.site().to_string()).collect(),
        _ => vec![],
    };
    EngineReport {
        engine: engine.id(),
        label: engine.label(),
        verdict,
        note,
        links: links.len(),
        result_sites: links,
        looks: read.as_ref().map_or(0, |r| r.looks),
        ready: read.as_ref().is_some_and(|r| r.ready),
        elapsed_ms,
        page: read.and_then(|r| r.page).map(|p| p.diagnostics()),
        screenshot,
        screenshot_error,
    }
}

/// Probes `engines` one after the other (`pause` between them), writes the report into `out` and returns the failed
/// engines' lines ("Google: why").
pub fn probe_all(
    query: &str,
    engines: &[SearchEngine],
    out: &Path,
    pause: &dyn Fn(Duration),
    probe_one: &mut dyn FnMut(SearchEngine) -> EngineReport,
) -> Result<Vec<String>, String> {
    let mut reports = Vec::new();
    for (i, engine) in engines.iter().enumerate() {
        if i > 0 {
            pause(PACE);
        }
        reports.push(probe_one(*engine));
    }
    let report = Report {
        query,
        platform: std::env::consts::OS,
        done: true,
        engines: &reports,
    };
    let json = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(out.join(REPORT), json).map_err(|e| format!("cannot write the report: {e}"))?;
    Ok(reports
        .iter()
        .filter(|e| e.verdict == Verdict::Fail)
        .map(|e| format!("{}: {}", e.label, e.note))
        .collect())
}

/// Every built-in engine against the real service; needs a Chromium-based browser and the network. Manual runs only:
/// CI runs [`tests::recorded`] instead.
#[test]
#[ignore = "live: needs a browser and the network (manual runs only)"]
fn live_search_every_engine_gives_results_or_a_recognised_hand_off() {
    if std::env::var("DEBRIEF_LIVE_SEARCH").is_err() {
        eprintln!("set DEBRIEF_LIVE_SEARCH=1 to run the live search test");
        return;
    }
    let out = std::env::var_os(OUT_VAR).map(PathBuf::from).unwrap_or_else(|| {
        // A fresh private folder that is kept so the screenshots can be looked at; its path is printed.
        let kept = tempfile::Builder::new().prefix("debrief-live-search-").tempdir().unwrap().keep();
        eprintln!("results are written to {} (set {OUT_VAR} to choose the folder)", kept.display());
        kept
    });
    assert!(out.is_absolute(), "{OUT_VAR} must be an absolute folder");
    std::fs::create_dir_all(&out).unwrap();
    let query = query_from(std::env::var(QUERY_VAR).ok()).unwrap();
    let renderers = Arc::new(Renderers::default());
    let engines = selected(std::env::var(ENGINES_VAR).ok().as_deref());
    let failed = probe_all(&query, &engines, &out, &SLEEP, &mut |engine| {
        let control = Arc::new(ChatBrowser::new(AgentLimits::default()));
        control.set_agent(true, AgentLimits::default());
        let page = |drive: bool| ChromiumPage {
            renderers: renderers.clone(),
            owner: "live".into(),
            may_drive: drive.then(|| Box::new(|| true) as Box<dyn Fn() -> bool + Send + Sync>),
            rules: Some(control.clone()),
            product: "Debrief".into(),
            notify: Arc::new(|_| {}),
            lease: Default::default(),
        };
        let reader = page(false);
        let driver = ReadOnlyDriver {
            page: page(true),
            control: control.clone(),
        };
        let shoot = |path: &Path| screenshot(&renderers, path);
        let chromium = Probe {
            reader: &reader,
            results: &reader,
            driver: &driver,
            sleep: &SLEEP,
            now: &NOW,
            screenshot: &shoot,
        };
        probe(&chromium, engine, &query, &out)
    });
    renderers.kill_all();
    let failed = failed.unwrap();
    assert!(failed.is_empty(), "live search failed: {failed:?} (report in {})", out.display());
}

#[cfg(test)]
mod tests {
    use super::*;
    use debrief_ai::browser::WebUrl;

    /// The live probe over recorded engine pages, with a fake clock: what CI runs instead of the real engines.
    pub(super) mod recorded {
        use super::*;
        use debrief_ai::browser::{BrowserError, DomSnapshot, Element, PageCapture, ResultsPage, ScrollDirection};
        use std::collections::HashMap;
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Mutex;

        /// What the results-page collector returned on each engine's page for the default query (invented results).
        pub const GOOGLE: &str = include_str!("live_fixtures/google.json");
        pub const DUCKDUCKGO: &str = include_str!("live_fixtures/duckduckgo.json");
        pub const BING: &str = include_str!("live_fixtures/bing.json");
        /// Google's cookie consent page instead of its results.
        pub const GOOGLE_CONSENT: &str = include_str!("live_fixtures/google-consent.json");

        /// A recorded web: each engine's site answers with its recorded results page, the engines' own redirects go
        /// to their result, and any other address shows a page of its own.
        pub struct RecordedWeb {
            pages: HashMap<String, ResultsPage>,
            redirects: HashMap<String, String>,
            at: Mutex<String>,
        }

        impl RecordedWeb {
            pub fn new(recorded: &[&str]) -> RecordedWeb {
                let pages = recorded
                    .iter()
                    .map(|json| {
                        let reply: serde_json::Map<String, serde_json::Value> = serde_json::from_str(json).expect("a recorded reply");
                        let page = debrief_ai::browser::results_page_from(&reply);
                        (site(&page.url), page)
                    })
                    .collect();
                RecordedWeb {
                    pages,
                    redirects: HashMap::new(),
                    at: Mutex::new("about:blank".into()),
                }
            }

            /// An engine's own redirect link and where it goes.
            pub fn redirect(mut self, from: &str, to: &str) -> RecordedWeb {
                self.redirects.insert(from.into(), to.into());
                self
            }

            fn at(&self) -> String {
                self.at.lock().unwrap().clone()
            }
        }

        /// A recorded page answers for its engine's whole site (a consent page answers for the engine it stands in for).
        fn site(url: &str) -> String {
            let host = WebUrl::parse(url).map(|u| u.host().to_ascii_lowercase()).unwrap_or_default();
            host.trim_start_matches("www.").trim_start_matches("consent.").to_string()
        }

        impl PageReader for RecordedWeb {
            fn capture(&self) -> Result<PageCapture, BrowserError> {
                Ok(PageCapture {
                    title: "A result".into(),
                    url: self.at(),
                    text: "What the result says.".into(),
                    truncated: false,
                    removed: vec![],
                })
            }
            fn snapshot(&self) -> Result<DomSnapshot, BrowserError> {
                Ok(DomSnapshot {
                    url: self.at(),
                    title: "A result".into(),
                    elements: vec![],
                    text: "What the result says.".into(),
                    hand_off: None,
                })
            }
        }

        impl ResultsReader for RecordedWeb {
            fn results_page(&self, _region: &[&str]) -> Result<ResultsPage, BrowserError> {
                self.pages.get(&site(&self.at())).cloned().ok_or(BrowserError::Timeout)
            }
        }

        impl PageDriver for RecordedWeb {
            fn navigate(&self, url: &WebUrl) -> Result<(), BrowserError> {
                let to = self
                    .redirects
                    .get(url.as_str())
                    .cloned()
                    .unwrap_or_else(|| url.as_str().to_string());
                *self.at.lock().unwrap() = to;
                Ok(())
            }
            fn click(&self, _: &Element) -> Result<(), BrowserError> {
                panic!("research never clicks")
            }
            fn type_text(&self, _: &Element, _: &str, _: bool) -> Result<(), BrowserError> {
                panic!("research never types")
            }
            fn scroll(&self, _: ScrollDirection) -> Result<(), BrowserError> {
                panic!("research never scrolls")
            }
            fn back(&self) -> Result<(), BrowserError> {
                panic!("research never goes back")
            }
        }

        /// A clock that only moves when research waits on it.
        #[derive(Default)]
        pub struct FakeClock {
            start: Option<Instant>,
            waited_ms: AtomicU64,
        }

        impl FakeClock {
            pub fn new() -> FakeClock {
                FakeClock {
                    start: Some(Instant::now()),
                    waited_ms: AtomicU64::new(0),
                }
            }
            pub fn sleep(&self, d: Duration) {
                self.waited_ms
                    .fetch_add(u64::try_from(d.as_millis()).unwrap_or(u64::MAX), Ordering::SeqCst);
            }
            pub fn now(&self) -> Instant {
                self.start.expect("FakeClock::new") + Duration::from_millis(self.waited_ms.load(Ordering::SeqCst))
            }
        }

        /// Runs the live probe over `web` for `engines`; returns the failed lines and the report written.
        pub fn run(web: &RecordedWeb, engines: &[SearchEngine], out: &Path) -> (Vec<String>, serde_json::Value) {
            let clock = FakeClock::new();
            let sleep = |d: Duration| clock.sleep(d);
            let now = || clock.now();
            let shoot = |path: &Path| std::fs::write(path, b"png").map_err(|e| e.to_string());
            let recorded = Probe {
                reader: web,
                results: web,
                driver: web,
                sleep: &sleep,
                now: &now,
                screenshot: &shoot,
            };
            let failed = probe_all(DEFAULT_QUERY, engines, out, &sleep, &mut |engine| {
                probe(&recorded, engine, DEFAULT_QUERY, out)
            })
            .unwrap();
            let report = serde_json::from_slice(&std::fs::read(out.join(REPORT)).unwrap()).unwrap();
            (failed, report)
        }
    }

    /// The live probe's steps, verdicts and report over each engine's recorded results page: results are read, the
    /// first one opens (directly, or through the engine's redirect), and nothing needs the network or a browser.
    #[test]
    fn every_engine_passes_on_its_recorded_results_page() {
        use recorded::*;
        let out = crate::test_support::scratch_dir("live-search");
        let web = RecordedWeb::new(&[GOOGLE, DUCKDUCKGO, BING]).redirect(
            "https://duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.example.org%2Fapi%2Fsystem.timeoutexception&rut=abc",
            "https://docs.example.org/api/system.timeoutexception",
        );
        let (failed, report) = run(&web, &selected(None), &out);
        assert_eq!(failed, Vec::<String>::new());
        let engines = report["engines"].as_array().unwrap();
        assert_eq!(
            engines.iter().map(|e| e["engine"].as_str().unwrap()).collect::<Vec<_>>(),
            ["google", "duckDuckGo", "bing"]
        );
        for e in engines {
            assert_eq!(e["verdict"], "pass", "{e}");
            assert!(e["links"].as_u64().unwrap() >= MIN_LINKS as u64, "{e}");
            assert!(e["ready"].as_bool().unwrap(), "read because it was ready: {e}");
            assert!(e["note"].as_str().unwrap().contains("the first one opened on"), "{e}");
            // The organic results come first (the engine's own links on its site never count); sites only, never
            // addresses.
            let sites: Vec<&str> = e["resultSites"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
            assert!(sites[..3].iter().all(|s| s.contains(".example.")), "{sites:?}");
            assert!(
                sites
                    .iter()
                    .all(|s| !s.contains('/') && !s.ends_with("duckduckgo.com") && *s != "www.google.com" && *s != "www.bing.com"),
                "{sites:?}"
            );
            let shot = e["screenshot"].as_str().unwrap();
            assert!(out.join(shot).is_file(), "the results page's screenshot is saved");
            // The page's diagnostics keep no query string.
            assert!(!e["page"].to_string().contains("TimeoutException&") && !e["page"]["path"].as_str().unwrap().contains('?'));
        }
        assert_eq!(report["query"], DEFAULT_QUERY);
        assert_eq!(report["done"], true);
    }

    /// A consent page is the recognised hand-off (a warning); an engine whose page shows too few results fails, and
    /// the run says which and why.
    #[test]
    fn a_consent_page_warns_and_a_thin_results_page_fails_the_run() {
        use recorded::*;
        let out = crate::test_support::scratch_dir("live-search-handoff");
        let mut thin: serde_json::Value = serde_json::from_str(BING).unwrap();
        thin["links"].as_array_mut().unwrap().truncate(2);
        let thin = thin.to_string();
        let web = RecordedWeb::new(&[GOOGLE_CONSENT, &thin]);
        let (failed, report) = run(&web, &[SearchEngine::Google, SearchEngine::Bing], &out);
        let engines = report["engines"].as_array().unwrap();
        assert_eq!(engines[0]["verdict"], "warn", "{}", engines[0]);
        assert!(engines[0]["note"].as_str().unwrap().contains("a consent page"), "{}", engines[0]);
        assert_eq!(engines[1]["verdict"], "fail");
        assert_eq!(failed, [format!("Bing: only 1 result links (at least {MIN_LINKS} expected)")]);
    }

    fn links(n: usize) -> ResultsOutcome {
        ResultsOutcome::Links(
            (0..n)
                .map(|i| (WebUrl::parse(&format!("https://r{i}.example/")).unwrap(), "r".into()))
                .collect(),
        )
    }

    #[test]
    fn three_result_links_pass_hand_offs_warn_anything_else_fails() {
        let opened = Ok("learn.example.org".to_string());
        assert_eq!(verdict(&links(3), Some(&opened)).0, Verdict::Pass);
        let (v, note) = verdict(&links(3), Some(&Err("did not load (timeout)".into())));
        assert_eq!(v, Verdict::Fail, "the first result must open too");
        assert!(note.ends_with("the first one did not load (timeout)"), "{note}");
        assert_eq!(verdict(&links(3), None).0, Verdict::Fail);
        assert_eq!(verdict(&links(2), Some(&opened)).0, Verdict::Fail);
        assert_eq!(verdict(&ResultsOutcome::NoLinks, None).0, Verdict::Fail);
        assert_eq!(verdict(&ResultsOutcome::NotShown, None).0, Verdict::Fail);
        for kind in [PageKind::Consent, PageKind::Captcha, PageKind::Redirect] {
            let (v, note) = verdict(&ResultsOutcome::HandOff(kind), None);
            assert_eq!(v, Verdict::Warn);
            assert!(note.starts_with("handed to the user"), "{note}");
        }
    }

    #[test]
    fn every_builtin_engine_is_probed_unless_some_are_chosen() {
        use SearchEngine::*;
        assert_eq!(selected(None), [Google, DuckDuckGo, Bing]);
        assert_eq!(selected(Some(" ")), selected(None));
        assert_eq!(selected(Some("bing, google,custom,yahoo")), [Google, Bing]);
    }

    #[test]
    fn the_query_is_neutral_by_default_and_checked_when_given() {
        assert_eq!(query_from(None).unwrap(), DEFAULT_QUERY);
        assert_eq!(
            query_from(Some("  HTTP 504 Gateway Timeout ".into())).unwrap(),
            "HTTP 504 Gateway Timeout"
        );
        assert!(query_from(Some(" ".into())).is_err());
        assert!(query_from(Some("a\nb".into())).is_err());
        assert!(query_from(Some("x".repeat(MAX_QUERY + 1))).is_err());
    }
}
