//! The 0.3.0 additions around the downloader: request headers, small JSON/text GETs, the free-space message, an external cancel
//! token, the phase constants, the "resumable?" accessor and contained event-sink panics.
#![cfg(feature = "http")]

use crate::support;

use paddock_download::{
    Config, DownloadError, Downloader, Fetch, HttpConfig, Integrity, ProxyMode, Request,
    Sha256Digest, phases,
};
use paddock_jobs::{
    CancelReason, CancelToken, EventSink, FakeClock, JobRegistry, JobSnapshot, JobState,
};
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use support::{Resp, Server, blob, job, partial, sha256_hex};

fn fast() -> Config {
    Config {
        connections: 2,
        segment_threshold: 64 * 1024,
        min_block: 32 * 1024,
        max_block: 64 * 1024,
        attempts: 3,
        backoff: Duration::from_millis(2),
        max_wait: Duration::from_secs(2),
        max_throttles: 3,
        save_every: Duration::from_millis(10),
        ..Config::default()
    }
}

fn downloader() -> Downloader {
    Downloader::http(HttpConfig {
        proxy: ProxyMode::Direct,
        ..HttpConfig::default()
    })
    .unwrap()
    .with_config(fast())
}

fn pinned(data: &[u8]) -> Integrity {
    Integrity::Sha256(Sha256Digest::from_hex(&sha256_hex(data)).unwrap())
}

#[derive(Debug, Deserialize, PartialEq)]
struct Release {
    tag: String,
    assets: Vec<String>,
}

fn json(body: &str) -> Resp {
    Resp::new(200, body.as_bytes().to_vec()).header("content-type", "application/json")
}

// ---- custom headers ----

#[test]
fn added_headers_go_with_the_probe_every_range_and_the_stream() {
    let data = blob(300 * 1024);
    let dir = tempfile::tempdir().unwrap();
    for ranges in [true, false] {
        let d2 = data.clone();
        let server = Server::start(move |r, _| {
            if ranges {
                partial(r, &d2)
            } else {
                Resp::new(200, d2.clone())
            }
        });
        let req = Request::new(
            &server.url("/f"),
            dir.path().join(format!("f{ranges}")),
            pinned(&data),
        )
        .unwrap()
        .header("Authorization", "Bearer s3cret")
        .unwrap()
        .header("X-Trace", "abc")
        .unwrap();
        let (ctx, _) = job();
        downloader().download(&ctx, &req).unwrap();
        let log = server.log();
        assert!(log.len() >= if ranges { 2 } else { 1 }, "{ranges}");
        for r in &log {
            assert_eq!(
                r.headers.get("authorization").map(String::as_str),
                Some("Bearer s3cret"),
                "{r:?}"
            );
            assert_eq!(r.headers.get("x-trace").map(String::as_str), Some("abc"));
        }
    }
}

#[test]
fn headers_the_downloader_owns_or_that_are_malformed_are_refused_and_values_stay_hidden() {
    let base = || Request::new("https://h.test/f", "/tmp/x/f", Integrity::Unpinned).unwrap();
    for name in [
        "Range",
        "if-range",
        "Accept-Encoding",
        "Host",
        "Content-Length",
        "Proxy-Authorization",
        "Transfer-Encoding",
    ] {
        assert!(
            matches!(
                base().header(name, "x"),
                Err(DownloadError::InvalidRequest(_))
            ),
            "{name}"
        );
    }
    for (n, v) in [
        ("", "x"),
        ("Bad Name", "x"),
        ("X", "a\r\nInjected: 1"),
        ("X", "a\u{0}b"),
    ] {
        assert!(
            matches!(base().header(n, v), Err(DownloadError::InvalidRequest(_))),
            "{n:?}"
        );
    }
    let mut req = base();
    for i in 0..paddock_download::MAX_HEADERS {
        req = req.header(&format!("X-{i}"), "v").unwrap();
    }
    assert!(req.clone().header("X-Over", "v").is_err());
    let shown = format!(
        "{:?}",
        base().header("Authorization", "Bearer s3cret").unwrap()
    );
    assert!(
        !shown.contains("s3cret") && shown.contains("Authorization"),
        "{shown}"
    );
}

// ---- JSON and text GETs ----

#[test]
fn get_json_parses_the_answer_and_asks_for_json() {
    let server = Server::start(|_, _| json(r#"{"tag":"v1.2.3","assets":["a.zip","b.zip"]}"#));
    let got: Release = downloader()
        .get_json(
            &server.url("/releases/latest"),
            &Fetch::new().header("Authorization", "token t").unwrap(),
        )
        .unwrap();
    assert_eq!(
        got,
        Release {
            tag: "v1.2.3".into(),
            assets: vec!["a.zip".into(), "b.zip".into()]
        }
    );
    let r = &server.log()[0];
    assert_eq!(
        r.headers.get("accept").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(
        r.headers.get("authorization").map(String::as_str),
        Some("token t")
    );
    assert!(r.range().is_none());
    // The caller's own Accept is kept.
    let _: serde_json::Value = downloader()
        .get_json(
            &server.url("/x"),
            &Fetch::new()
                .header("Accept", "application/vnd.x+json")
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        server.log()[1].headers.get("accept").map(String::as_str),
        Some("application/vnd.x+json")
    );
    assert_eq!(
        downloader()
            .get_text(&server.url("/t"), &Fetch::new())
            .unwrap()
            .len(),
        43
    );
}

#[test]
fn get_json_failures_are_typed_and_never_quote_the_body() {
    let d = downloader();
    let wrong = Server::start(|_, _| json(r#"{"tag": 7, "password": "hunter2"}"#));
    let e = d
        .get_json::<Release>(&wrong.url("/"), &Fetch::new())
        .unwrap_err();
    assert!(
        matches!(&e, DownloadError::Protocol { detail, .. } if detail.contains("line 1")),
        "{e:?}"
    );
    assert!(!e.to_string().contains("hunter2"));
    let junk = Server::start(|_, _| Resp::new(200, b"<html>".to_vec()));
    assert!(matches!(
        d.get_json::<Release>(&junk.url("/"), &Fetch::new()),
        Err(DownloadError::Protocol { .. })
    ));
    let missing = Server::start(|_, _| Resp::new(404, ""));
    assert!(matches!(
        d.get_json::<Release>(&missing.url("/"), &Fetch::new()),
        Err(DownloadError::Http { status: 404, .. })
    ));
    assert_eq!(missing.log().len(), 1, "a 404 is final");
    let bin = Server::start(|_, _| Resp::new(200, vec![0xff, 0xfe]));
    assert!(matches!(
        d.get_text(&bin.url("/"), &Fetch::new()),
        Err(DownloadError::Protocol { .. })
    ));
    let gz = Server::start(|_, _| json("{}").header("content-encoding", "gzip"));
    assert!(matches!(
        d.get_bytes(&gz.url("/"), &Fetch::new()),
        Err(DownloadError::Protocol { .. })
    ));
}

#[test]
fn get_bytes_is_bounded_and_retries_throttling_and_server_errors() {
    let d = downloader();
    let big = Server::start(|_, _| Resp::new(200, blob(10_000)));
    let e = d
        .get_bytes(&big.url("/"), &Fetch::new().max_bytes(1000))
        .unwrap_err();
    assert!(
        matches!(e, DownloadError::TooLarge { limit: 1000, .. }),
        "{e:?}"
    );
    // Without Content-Length the limit is enforced while reading.
    let chunked = Server::start(|_, _| Resp::new(200, blob(10_000)).no_length());
    let e = d
        .get_bytes(&chunked.url("/"), &Fetch::new().max_bytes(1000))
        .unwrap_err();
    assert!(
        matches!(e, DownloadError::TooLarge { limit: 1000, .. }),
        "{e:?}"
    );
    // 503 with Retry-After, then 500, then the answer.
    let flaky = Server::start(|_, n| match n {
        0 => Resp::new(503, "").header("retry-after", "0"),
        1 => Resp::new(500, ""),
        _ => json(r#"{"tag":"v","assets":[]}"#),
    });
    let r: Release = d.get_json(&flaky.url("/"), &Fetch::new()).unwrap();
    assert_eq!(r.tag, "v");
    assert_eq!(flaky.log().len(), 3);
    // Always failing: gives up with a network error after the configured attempts.
    let down = Server::start(|_, _| Resp::new(500, ""));
    assert!(matches!(
        d.get_bytes(&down.url("/"), &Fetch::new()),
        Err(DownloadError::Network { .. })
    ));
    assert_eq!(down.log().len(), 3);
}

#[test]
fn get_bytes_notices_a_cancel_before_connecting_and_while_reading() {
    let server = Server::start(|_, _| Resp::new(200, blob(100_000)));
    let token = CancelToken::new();
    token.cancel();
    let e = downloader()
        .get_bytes(&server.url("/"), &Fetch::new().cancel(&token))
        .unwrap_err();
    assert!(
        matches!(e, DownloadError::Cancelled(CancelReason::User)),
        "{e:?}"
    );
    assert!(server.log().is_empty(), "no request was sent");
    let expired = CancelToken::with_deadline(Duration::ZERO);
    let e = downloader()
        .get_bytes(&server.url("/"), &Fetch::new().cancel(&expired))
        .unwrap_err();
    assert!(
        matches!(e, DownloadError::Cancelled(CancelReason::Deadline)),
        "{e:?}"
    );
}

// ---- free space ----

#[test]
fn not_enough_space_names_the_folder_and_the_numbers() {
    let data = blob(100 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("models").join("big.bin");
    let req = Request::new(&server.url("/f"), &dest, pinned(&data)).unwrap();
    let d = downloader().with_free_space(|_| Ok(1000));
    let (ctx, _) = job();
    let e = d.download(&ctx, &req).unwrap_err();
    let DownloadError::NotEnoughSpace { needed, free, path } = &e else {
        panic!("{e:?}")
    };
    assert_eq!((*free, path.as_path()), (1000, dest.parent().unwrap()));
    assert!(*needed > 100 * 1024);
    let text = e.to_string();
    assert!(
        text.contains(&dest.parent().unwrap().display().to_string())
            && text.contains(", 1 KB is free"),
        "{text}"
    );
    assert!(e.keeps_partial());
}

// ---- external cancel token ----

#[test]
fn an_external_token_stops_a_download_in_flight_and_the_partial_file_resumes() {
    let data = blob(2 * 1024 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2).rate(16 * 1024 * 1024));
    let dir = tempfile::tempdir().unwrap();
    let req = Request::new(&server.url("/f"), dir.path().join("f.bin"), pinned(&data)).unwrap();
    let d = downloader();
    let token = CancelToken::new();
    let (ctx, rec) = job();
    rec.cancel_external_at(&token, phases::DOWNLOAD, 512 * 1024);
    let e = d.download_with_cancel(&ctx, &req, &token).unwrap_err();
    assert!(
        matches!(e, DownloadError::Cancelled(CancelReason::User)),
        "{e:?}"
    );
    assert!(
        !ctx.cancel_token().is_cancelled(),
        "the job itself was not cancelled"
    );
    assert!(!req.dest().exists());
    // The state it left is something to continue.
    let r = req.resumable().expect("resumable after a stop");
    assert!(r.bytes > 0 && r.size == data.len() as u64);
    let (ctx, _) = job();
    let got = d.download(&ctx, &req).unwrap();
    assert_eq!(std::fs::read(&got.path).unwrap(), data);
    assert!(got.resumed_from > 0);
    assert!(
        req.resumable().is_none(),
        "nothing left to continue once finished"
    );
    // A token that is already cancelled stops it before any request, with its own reason.
    let expired = CancelToken::with_deadline(Duration::ZERO);
    let req2 = Request::new(&server.url("/g"), dir.path().join("g.bin"), pinned(&data)).unwrap();
    let before = server.log().len();
    let (ctx, _) = job();
    let e = d.download_with_cancel(&ctx, &req2, &expired).unwrap_err();
    assert!(
        matches!(e, DownloadError::Cancelled(CancelReason::Deadline)),
        "{e:?}"
    );
    assert_eq!(server.log().len(), before);
}

#[test]
fn resumable_is_none_without_state_or_with_a_short_partial_file() {
    let dir = tempfile::tempdir().unwrap();
    let req = Request::new(
        "https://h.test/f",
        dir.path().join("f.bin"),
        Integrity::Unpinned,
    )
    .unwrap();
    assert!(req.resumable().is_none());
    std::fs::write(req.part_path(), b"abc").unwrap();
    assert!(
        req.resumable().is_none(),
        "a partial file without its state"
    );
    std::fs::write(req.resume_path(), b"not json").unwrap();
    assert!(req.resumable().is_none(), "damaged state");
    let state = r#"{"version":1,"url":"https://h.test/f","size":100,"sha256":null,"validator":"\"v\"","hashed":50}"#;
    std::fs::write(req.resume_path(), state).unwrap();
    assert!(
        req.resumable().is_none(),
        "the partial file is shorter than the state promises"
    );
    std::fs::write(req.part_path(), vec![0u8; 60]).unwrap();
    let r = req.resumable().unwrap();
    assert_eq!((r.bytes, r.size), (50, 100));
    assert!(
        req.clone().size(99).resumable().is_none(),
        "another pinned size"
    );
}

// ---- phase constants ----

#[test]
fn the_phase_constants_cover_what_a_download_reports_and_leave_room_for_the_callers() {
    let data = blob(100 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2));
    let dir = tempfile::tempdir().unwrap();
    let req = Request::new(&server.url("/f"), dir.path().join("f.bin"), pinned(&data)).unwrap();
    let (ctx, rec) = job();
    downloader().download(&ctx, &req).unwrap();
    let seen = rec.phases();
    assert_eq!(seen, [phases::CONNECT, phases::DOWNLOAD, phases::VERIFY]);
    assert!(seen.iter().all(|p| phases::is_download_phase(p)));
    for added in [phases::UNPACK, phases::INSTALL, phases::SELF_TEST] {
        assert!(!phases::is_download_phase(added));
    }
    // The labels the snapshots carry are the constants.
    let labels: Vec<String> = rec
        .snaps
        .lock()
        .unwrap()
        .iter()
        .filter_map(|s| match &s.state {
            JobState::Running { phase: Some(p) } => Some(p.phase.label.clone()),
            _ => None,
        })
        .collect();
    assert!(
        labels.iter().any(|l| l == phases::CONNECT_LABEL)
            && labels.iter().any(|l| l == phases::VERIFY_LABEL)
    );
}

// ---- event sink panics ----

struct Panicky(AtomicU64);

impl EventSink for Panicky {
    fn emit(&self, s: &JobSnapshot) {
        if matches!(&s.state, JobState::Running { phase: Some(p) } if p.phase.id == phases::DOWNLOAD)
        {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("sink bug");
        }
    }
}

#[test]
fn a_panicking_event_sink_does_not_stop_the_download_and_is_reported() {
    let data = blob(300 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2));
    let dir = tempfile::tempdir().unwrap();
    let req = Request::new(&server.url("/f"), dir.path().join("f.bin"), pinned(&data)).unwrap();
    let sink = Arc::new(Panicky(AtomicU64::new(0)));
    let jobs = JobRegistry::with_clock(sink.clone(), Arc::new(FakeClock::new()), 0);
    let ctx = jobs.register("download", "Downloading", None);
    let got = downloader().download(&ctx, &req).unwrap();
    assert_eq!(std::fs::read(&got.path).unwrap(), data);
    assert!(
        got.sink_panics > 0 && got.sink_panics == sink.0.load(Ordering::SeqCst),
        "{} panics",
        got.sink_panics
    );
}

#[test]
fn a_healthy_sink_reports_no_panics() {
    let data = blob(1000);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2));
    let dir = tempfile::tempdir().unwrap();
    let req = Request::new(&server.url("/f"), dir.path().join("f.bin"), pinned(&data)).unwrap();
    let (ctx, _) = job();
    assert_eq!(downloader().download(&ctx, &req).unwrap().sink_panics, 0);
}

#[test]
fn a_fetch_takes_a_bounded_number_of_headers_and_a_timeout() {
    let mut fetch = Fetch::new().timeout(Duration::from_secs(5));
    let mut refused = None;
    for i in 0..1000 {
        match fetch.header(&format!("X-Extra-{i}"), "v") {
            Ok(next) => fetch = next,
            Err(e) => {
                refused = Some(e);
                break;
            }
        }
    }
    assert!(matches!(refused, Some(DownloadError::InvalidRequest(m)) if m.contains("at most")));
}

#[test]
fn get_bytes_tries_again_when_the_body_is_cut_and_gives_up_when_nothing_answers() {
    let d = downloader();
    // The first answer promises 5000 bytes and delivers 100: another attempt.
    let cut = Server::start(|_, n| match n {
        0 => Resp::new(200, blob(5000)).cut(100),
        _ => Resp::new(200, blob(5000)),
    });
    assert_eq!(
        d.get_bytes(&cut.url("/"), &Fetch::new()).unwrap().len(),
        5000
    );
    assert_eq!(cut.log().len(), 2);
    // Nothing listens on the port: the connection failure is retried, then reported by host.
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}/", listener.local_addr().unwrap())
    };
    assert!(matches!(
        d.get_bytes(&closed, &Fetch::new()),
        Err(DownloadError::Network { .. })
    ));
}
