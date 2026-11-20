//! Tests of the health gate: the logic against a scripted look, the real `curl` look against a stub HTTP server, and the
//! wiring into `dev wait --ci-run` and `dev ci dispatch` (those two live in their own test modules and use [`Scripted`]).
use super::*;
use crate::wait::{Outcome, Probe};
use std::collections::VecDeque;
use std::io::Read;
use std::net::TcpListener;

/// A look scripted per service: the answers in order, the last one repeating. A URL it has no script for is up (200).
#[derive(Default)]
pub(crate) struct Scripted {
    scripts: Vec<(String, VecDeque<Reach>)>,
    /// Every URL looked at, in order.
    pub(crate) looks: Vec<String>,
}

impl Scripted {
    pub(crate) fn new() -> Scripted {
        Scripted::default()
    }
    /// The answers for URLs containing `part`.
    pub(crate) fn script(mut self, part: &str, answers: Vec<Reach>) -> Scripted {
        self.scripts.push((part.to_string(), answers.into()));
        self
    }
}

impl Reacher for Scripted {
    fn reach(&mut self, url: &str) -> Reach {
        self.looks.push(url.to_string());
        match self.scripts.iter_mut().find(|(p, _)| url.contains(p.as_str())) {
            Some((_, q)) if q.len() > 1 => q.pop_front().unwrap(),
            Some((_, q)) => q[0].clone(),
            None => Reach::Answered(200),
        }
    }
}

pub(crate) fn both() -> Vec<Service> {
    let env: HashMap<String, String> = [
        ("PADDOCK_SCCACHE_BACKEND", "s3"),
        ("PADDOCK_SCCACHE_S3_BUCKET", "bucket-x"),
        ("PADDOCK_SCCACHE_S3_ENDPOINT", "10.9.9.9:1234"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    services("https://gitea.invalid", &env).0
}

#[derive(Default)]
struct FakeClock(Duration);
impl Clock for FakeClock {
    fn elapsed(&self) -> Duration {
        self.0
    }
    fn sleep(&mut self, d: Duration) {
        self.0 += d;
    }
}

/// The inner probe of a wait: counts its polls.
struct Inner(u32);
impl Probe for Inner {
    fn describe(&self) -> String {
        "the thing".into()
    }
    fn poll(&mut self) -> Result<Poll, String> {
        self.0 += 1;
        Ok(Poll::Done("the thing happened".into()))
    }
    fn next_step(&self) -> String {
        "inner next step".into()
    }
}

fn down(code: u16) -> Reach {
    Reach::Answered(code)
}

#[test]
fn up_means_any_answer_below_500_and_down_means_5xx_or_no_answer() {
    for (r, up) in [
        (Reach::Answered(200), true),
        (Reach::Answered(403), true),
        (Reach::Answered(404), true),
        (Reach::Answered(401), true),
        (Reach::Answered(500), false),
        (Reach::Answered(502), false),
        (Reach::Answered(503), false),
        (Reach::Down("connection refused".into()), false),
    ] {
        assert_eq!(r.why_down().is_none(), up, "{r:?}");
    }
}

#[test]
fn the_services_come_from_the_environment_and_nothing_is_typed_here() {
    let list = both();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].url, "https://gitea.invalid/api/v1/version");
    assert_eq!((list[1].name, list[1].url.as_str()), ("RustFS", "http://10.9.9.9:1234/bucket-x"));
    // TLS follows the sccache setting.
    let tls: HashMap<String, String> = [
        ("PADDOCK_SCCACHE_BACKEND", "s3"),
        ("PADDOCK_SCCACHE_S3_BUCKET", "b"),
        ("PADDOCK_SCCACHE_S3_ENDPOINT", "h:1"),
        ("PADDOCK_SCCACHE_S3_SSL", "true"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    assert_eq!(services("https://g/", &tls).0[1].url, "https://h:1/b");
    // No S3 backend: only Gitea, and no note (a local cache has no service).
    let (only, note) = services("https://g", &HashMap::new());
    assert_eq!((only.len(), note), (1, None));
    // A wrong S3 setting is named, and Gitea is still watched.
    let bad: HashMap<String, String> = [("PADDOCK_SCCACHE_BACKEND".to_string(), "s3".to_string())].into();
    let (only, note) = services("https://g", &bad);
    assert_eq!(only.len(), 1);
    assert!(note.unwrap().contains("PADDOCK_SCCACHE_S3_BUCKET"));
}

#[test]
fn one_line_per_change_of_state_and_a_403_counts_as_up() {
    let mut reach = Scripted::new()
        .script("api/v1/version", vec![down(502), down(502), down(502), down(200)])
        .script("bucket-x", vec![down(403)]);
    let mut h = Health::new(both(), &mut reach);
    let (d1, n1) = h.check();
    assert_eq!(d1.len(), 1, "{d1:?}");
    assert!(d1[0].contains("Gitea") && d1[0].contains("HTTP 502"), "{d1:?}");
    assert!(n1.len() == 1 && n1[0].contains("Gitea is down"), "{n1:?}");
    assert!(!n1[0].contains("RustFS"), "the 403 bucket is up: {n1:?}");
    // Still down: no new line.
    let (_, n2) = h.check();
    let (_, n3) = h.check();
    assert!(n2.is_empty() && n3.is_empty(), "{n2:?} {n3:?}");
    // Back up: one line.
    let (d4, n4) = h.check();
    assert!(d4.is_empty());
    assert_eq!(n4, vec!["Gitea answers again".to_string()]);
}

#[test]
fn a_wait_holds_while_a_service_is_down_and_never_asks_the_inner_probe_then() {
    let mut reach = Scripted::new().script("api/v1/version", vec![down(502), down(502), down(200)]);
    let mut h = Health::new(both(), &mut reach);
    let (mut clock, mut log, mut inner) = (FakeClock::default(), Vec::new(), Inner(0));
    let o = wait_held(
        &mut inner,
        Some(&mut h),
        &mut clock,
        Duration::from_secs(600),
        Duration::from_secs(15),
        &mut log,
    );
    let log = String::from_utf8(log).unwrap();
    assert!(
        matches!(o, Outcome::Done { elapsed, .. } if elapsed == Duration::from_secs(30)),
        "{o:?}\n{log}"
    );
    assert_eq!(inner.0, 1, "the inner probe is asked once, after the service answered");
    assert_eq!(log.matches("holding until it answers").count(), 1, "one line for the outage: {log}");
    assert!(log.contains("Gitea answers again"), "{log}");
}

#[test]
fn a_hold_that_reaches_max_ends_with_a_timeout_that_names_the_service_and_the_next_step() {
    let mut reach = Scripted::new().script("bucket-x", vec![Reach::Down("connection refused".into())]);
    let mut h = Health::new(both(), &mut reach);
    let (mut clock, mut log, mut inner) = (FakeClock::default(), Vec::new(), Inner(0));
    let o = wait_held(
        &mut inner,
        Some(&mut h),
        &mut clock,
        Duration::from_secs(60),
        Duration::from_secs(15),
        &mut log,
    );
    match o {
        Outcome::TimedOut { last, max } => {
            assert_eq!(max, Duration::from_secs(60));
            assert!(
                last.contains("RustFS") && last.contains("10.9.9.9:1234") && last.contains("connection refused"),
                "{last}"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(inner.0, 0, "never asked while down");
    let next = h.next_step().expect("a next step");
    assert!(
        next.contains("RustFS") && next.contains("NAS") && next.ends_with("then run the command again"),
        "{next}"
    );
    assert!(!next.contains("Gitea"), "only the service that is down: {next}");
}

#[test]
fn positive_control_without_the_gate_the_same_inner_probe_runs_through_an_outage() {
    // The health check is what holds: the same script with no `health` asks the inner probe at once.
    let (mut clock, mut log, mut inner) = (FakeClock::default(), Vec::new(), Inner(0));
    let o = wait_held(
        &mut inner,
        None,
        &mut clock,
        Duration::from_secs(60),
        Duration::from_secs(15),
        &mut log,
    );
    assert!(matches!(o, Outcome::Done { .. }));
    assert_eq!(inner.0, 1);
    // And a healthy gate does not hold either.
    let mut reach = Scripted::new();
    let mut h = Health::new(both(), &mut reach);
    let (mut clock, mut log, mut inner) = (FakeClock::default(), Vec::new(), Inner(0));
    let o = wait_held(
        &mut inner,
        Some(&mut h),
        &mut clock,
        Duration::from_secs(60),
        Duration::from_secs(15),
        &mut log,
    );
    assert!(matches!(o, Outcome::Done { elapsed, .. } if elapsed.is_zero()), "{o:?}");
}

/// A stub HTTP server: answers each connection with the next status of `statuses` (the last repeats), then closes. Returns the
/// base URL and the number of requests served so far.
fn stub(statuses: Vec<u16>) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let served = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = served.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            let n = count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let code = statuses[n.min(statuses.len() - 1)];
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf);
            let _ = write!(s, "HTTP/1.1 {code} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    (url, served)
}

#[test]
fn the_real_look_reads_the_status_of_a_stub_server() {
    let (url, _) = stub(vec![403, 502, 200, 404]);
    let mut r = CurlReach;
    let got: Vec<Reach> = (0..4).map(|_| r.reach(&format!("{url}/bucket"))).collect();
    assert_eq!(got, vec![down(403), down(502), down(200), down(404)]);
    assert!(got[0].why_down().is_none() && got[1].why_down().is_some());
}

#[test]
fn the_real_look_calls_a_connection_that_ends_without_an_answer_down() {
    // A held port that hangs up on every connection: curl sees an empty reply, which is no answer (a refusal looks the same to the gate).
    let held = paddock_testkit::net::HeldPort::hang_up();
    let r = CurlReach.reach(&format!("http://127.0.0.1:{}/", held.port()));
    assert!(matches!(r, Reach::Down(_)), "{r:?}");
    assert!(r.why_down().is_some());
}

#[test]
fn a_real_outage_then_recovery_holds_and_goes_on_through_the_stub_server() {
    // Gitea's stub answers 502, 502, then 200; the wait is held for two looks and then asks the inner probe.
    let (url, served) = stub(vec![502, 502, 200]);
    let svc = vec![Service {
        name: "Gitea",
        url: format!("{url}/api/v1/version"),
        next: "check the NAS",
    }];
    let mut reach = CurlReach;
    let mut h = Health::new(svc, &mut reach);
    let (mut clock, mut log, mut inner) = (FakeClock::default(), Vec::new(), Inner(0));
    let o = wait_held(
        &mut inner,
        Some(&mut h),
        &mut clock,
        Duration::from_secs(600),
        Duration::from_secs(15),
        &mut log,
    );
    assert!(
        matches!(o, Outcome::Done { elapsed, .. } if elapsed == Duration::from_secs(30)),
        "{o:?}"
    );
    assert_eq!(served.load(std::sync::atomic::Ordering::SeqCst), 3);
    assert_eq!(inner.0, 1);
}

#[test]
fn the_server_of_a_repository_url_keeps_no_credentials_and_skips_paths() {
    assert_eq!(
        server_of("https://dev.example/decryp7/paddock.git").as_deref(),
        Some("https://dev.example")
    );
    assert_eq!(
        server_of("https://bot:s3cret@dev.example:3000/o/r.git").as_deref(),
        Some("https://dev.example:3000")
    );
    assert_eq!(server_of("http://127.0.0.1:9/x").as_deref(), Some("http://127.0.0.1:9"));
    for none in ["/a/local/clone", "file:///a/clone", "ssh://git@host/o/r", "https://"] {
        assert_eq!(server_of(none), None, "{none}");
    }
}

#[test]
fn a_push_hold_waits_for_gitea_and_names_it_when_it_stays_down() {
    let url = "https://gitea.invalid";
    let mut clock = FakeClock::default();
    let mut log = Vec::new();
    let mut reach = Scripted::new().script("api/v1/version", vec![down(502), down(502), down(200)]);
    let waited = hold_gitea(url, &mut reach, &mut clock, Duration::from_secs(300), &mut log).unwrap();
    assert_eq!(waited, Duration::from_secs(20));
    let mut clock = FakeClock::default();
    let mut reach = Scripted::new().script("api/v1/version", vec![down(503)]);
    let e = hold_gitea(url, &mut reach, &mut clock, Duration::from_secs(60), &mut log).unwrap_err();
    assert!(
        e.starts_with("::error::push held: Gitea (https://gitea.invalid/api/v1/version) is down: HTTP 503"),
        "{e}"
    );
    assert!(e.contains("Next: Gitea runs on the NAS"), "{e}");
    // Positive control: an answering Gitea does not hold at all, a 401 included (it is an answer, not an outage).
    let mut clock = FakeClock::default();
    let mut reach = Scripted::new().script("api/v1/version", vec![down(401)]);
    assert_eq!(
        hold_gitea(url, &mut reach, &mut clock, Duration::from_secs(60), &mut log).unwrap(),
        Duration::ZERO
    );
}

#[test]
fn a_push_of_the_train_tools_is_never_sent_while_gitea_answers_502() {
    use crate::queue::real::{Child, GitTrain};
    let (url, served) = stub(vec![502]);
    let dir = tempfile::tempdir().unwrap();
    let mut g = GitTrain::for_clone(dir.path().to_path_buf(), &format!("{url}/o/r.git"));
    g.timeout = Duration::from_secs(1);
    let e = g
        .run_git(Child::Authed, &["push", "--quiet", "origin", "HEAD:refs/heads/x"])
        .unwrap_err();
    assert!(
        e.contains("::error::push held") && e.contains("Gitea") && e.contains("HTTP 502"),
        "{e}"
    );
    assert!(served.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    // Positive control: Gitea answers (404 for the version path is still an answer), so the push goes on to git, which fails for
    // its own reason (this folder is no repository), and the hold says nothing.
    let (url, _) = stub(vec![404]);
    let g = GitTrain::for_clone(dir.path().to_path_buf(), &format!("{url}/o/r.git"));
    let e = g.run_git(Child::Authed, &["push", "--quiet", "origin", "HEAD:refs/heads/x"]);
    assert!(!matches!(&e, Err(m) if m.contains("push held")), "{e:?}");
}
