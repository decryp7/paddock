//! The core without the HTTP transport: request parsing, header parsing, host caps, errors, and the engine over a scripted
//! [`Transport`] for answers a real server rarely gives (short bodies without an error, interrupted reads, panics, TLS errors).

use crate::support;

use paddock_download::{
    ByteRange, Config, ContentRange, DEFAULT_HOST_CAP, DownloadError, Downloader, Fetch, Get,
    Headers, HostCaps, Integrity, MAX_URL_LEN, Request, Response, Sha256Digest, Transport,
    TransportError, TransportErrorKind, host_of, parse_content_range, parse_length,
    parse_retry_after, phases,
};
use paddock_jobs::{CancelReason, JobError};
use std::io::{self, Cursor, Read};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};
use support::{blob, job, sha256_hex};

// ---- pure parts ----

#[test]
fn digests_parse_from_hex_in_either_case_and_show_lower_case() {
    let hex = sha256_hex(b"abc");
    let d = Sha256Digest::from_hex(&hex.to_uppercase()).unwrap();
    assert_eq!((d.to_hex(), d.to_string()), (hex.clone(), hex.clone()));
    assert_eq!(format!("{d:?}"), format!("Sha256Digest({hex})"));
    for bad in ["", "abc", &"g".repeat(64), &"a".repeat(63)] {
        assert!(
            matches!(
                Sha256Digest::from_hex(bad),
                Err(DownloadError::InvalidRequest(_))
            ),
            "{bad}"
        );
    }
    assert!(Sha256Digest::from_hex(&format!(" {hex}\n")).is_ok());
    assert!(Sha256Digest::from_hex(&format!("{}g", &hex[..63])).is_err());
}

#[test]
fn requests_parse_the_host_and_refuse_unusable_input() {
    let r = Request::new(
        "HTTPS://User:pw@Example.COM:8443/a/b?x=1#f",
        "/tmp/x/f.bin",
        Integrity::Unpinned,
    )
    .unwrap();
    assert_eq!(r.host(), "example.com:8443");
    assert_eq!(r.url(), "HTTPS://User:pw@Example.COM:8443/a/b?x=1#f");
    assert_eq!(
        (r.integrity(), r.pinned_size()),
        (Integrity::Unpinned, None)
    );
    assert_eq!(r.clone().size(3).pinned_size(), Some(3));
    assert!(r.part_path().ends_with("f.bin.part") && r.resume_path().ends_with("f.bin.part.json"));
    assert_eq!(host_of("http://h?q").unwrap(), "h");
    assert_eq!(host_of("http://h#x").unwrap(), "h");
    for bad in [
        "ftp://h/f",
        "http://",
        "http:///f",
        "http://:80/f",
        "http://h/a b",
        "http://h/\u{7}",
        "file:///etc/passwd",
    ] {
        let e = host_of(bad).unwrap_err();
        assert!(matches!(e, DownloadError::InvalidRequest(_)), "{bad}");
    }
    let long = format!("http://h/{}", "a".repeat(MAX_URL_LEN));
    assert!(host_of(&long).unwrap_err().to_string().contains("too long"));
    let e = host_of(&format!("ftp://h/{}?secret=1", "b".repeat(200)))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains('…') && !e.contains("secret"),
        "the message shortens the address and drops the query: {e}"
    );
    assert!(
        Request::new("http://h/f", "/", Integrity::Unpinned).is_err(),
        "a destination without a file name"
    );
}

#[test]
fn content_range_length_and_retry_after_are_parsed_strictly() {
    assert_eq!(
        parse_content_range("bytes 0-9/100"),
        Some(ContentRange::Bytes {
            start: 0,
            end: 9,
            total: Some(100)
        })
    );
    assert_eq!(
        parse_content_range(" bytes 5-5/*"),
        Some(ContentRange::Bytes {
            start: 5,
            end: 5,
            total: None
        })
    );
    assert_eq!(
        parse_content_range("bytes */42"),
        Some(ContentRange::Unsatisfied { total: 42 })
    );
    for bad in [
        "bytes 9-0/100",
        "bytes 0-100/100",
        "items 0-1/2",
        "bytes 0-1",
        "bytes x-1/2",
        "bytes 0-y/2",
        "bytes 0-1/z",
        "bytes */*",
        "bytes 01/2",
    ] {
        assert_eq!(parse_content_range(bad), None, "{bad}");
    }
    assert_eq!((parse_length(" 12 "), parse_length("-1")), (Some(12), None));
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    assert_eq!(parse_retry_after("7", now), Some(Duration::from_secs(7)));
    let later = httpdate::fmt_http_date(now + Duration::from_secs(30));
    assert_eq!(
        parse_retry_after(&later, now),
        Some(Duration::from_secs(30))
    );
    assert_eq!(
        parse_retry_after(&httpdate::fmt_http_date(now - Duration::from_secs(5)), now),
        Some(Duration::ZERO)
    );
    assert_eq!(parse_retry_after("soon", now), None);
    assert_eq!(
        ByteRange {
            start: 4,
            end: None
        }
        .header(),
        "bytes=4-"
    );
    assert_eq!(
        ByteRange {
            start: 4,
            end: Some(9)
        }
        .header(),
        "bytes=4-9"
    );
}

#[test]
fn host_caps_count_per_host_and_clamp_their_limits() {
    let caps = HostCaps::new(0).with_cap("A.example", 99);
    assert_eq!(
        (caps.cap("a.example"), caps.cap("b.example")),
        (paddock_download::MAX_HOST_CAP, 1)
    );
    assert_eq!(
        HostCaps::default().cap("x"),
        paddock_download::DEFAULT_HOST_CAP
    );
    assert_eq!((caps.in_use("nobody"), caps.peak("nobody")), (0, 0));
    // A later back-off wins over an earlier one, an earlier one does not shorten it.
    caps.back_off("h", Duration::from_secs(5));
    caps.back_off("h", Duration::from_secs(1));
    assert_eq!(caps.held("h"), 0);
}

#[test]
fn errors_say_what_to_do_and_which_keep_the_partial_file() {
    let host = || "h".to_string();
    let cases: Vec<(DownloadError, &str, bool)> = vec![
        (DownloadError::InvalidRequest("bad".into()), "bad", false),
        (
            DownloadError::Cancelled(CancelReason::Deadline),
            "time limit",
            true,
        ),
        (
            DownloadError::Http {
                host: host(),
                status: 403,
            },
            "HTTP 403",
            false,
        ),
        (
            DownloadError::Network {
                host: host(),
                detail: "reset".into(),
            },
            "kept failing (reset)",
            true,
        ),
        (DownloadError::Throttled { host: host() }, "429", true),
        (
            DownloadError::Stalled {
                host: host(),
                secs: 9,
            },
            "for 9 s",
            true,
        ),
        (
            DownloadError::TooLarge { size: 9, limit: 1 },
            "1-byte download limit",
            false,
        ),
        (
            DownloadError::SizeMismatch {
                expected: 1,
                actual: 2,
            },
            "should be 1 bytes",
            false,
        ),
        (
            DownloadError::ChecksumMismatch {
                expected: "ab".into(),
                actual: "cd".into(),
            },
            "expected ab…, got cd…",
            false,
        ),
        (
            DownloadError::Changed {
                host: host(),
                detail: "d".into(),
            },
            "changed on h",
            false,
        ),
        (
            DownloadError::Protocol {
                host: host(),
                detail: "d".into(),
            },
            "cannot be used",
            false,
        ),
        (DownloadError::DiskFull, "disk is full", true),
        (DownloadError::Io(io::Error::other("boom")), "boom", true),
    ];
    for (e, text, keeps) in cases {
        assert!(e.to_string().contains(text), "{e}");
        assert_eq!(e.keeps_partial(), keeps, "{e:?}");
        let has_source = std::error::Error::source(&e).is_some();
        assert_eq!(has_source, matches!(e, DownloadError::Io(_)));
    }
    assert!(matches!(
        DownloadError::from(io::Error::from(io::ErrorKind::StorageFull)),
        DownloadError::DiskFull
    ));
    assert!(matches!(
        JobError::from(DownloadError::Cancelled(CancelReason::User)),
        JobError::Cancelled(CancelReason::User)
    ));
    assert!(
        matches!(JobError::from(DownloadError::DiskFull), JobError::Failed(m) if m.contains("disk"))
    );
    for (kind, text, transient) in [
        (TransportErrorKind::Connect, "cannot connect", true),
        (TransportErrorKind::Timeout, "in time", true),
        (TransportErrorKind::Tls, "verified", false),
        (TransportErrorKind::Proxy, "proxy", false),
        (TransportErrorKind::Invalid, "invalid", false),
    ] {
        let e = TransportError::new(kind, "x");
        assert!(e.to_string().contains(text) && e.to_string().ends_with("(x)"));
        assert_eq!(e.is_transient(), transient);
    }
}

#[test]
fn the_config_is_clamped_and_exposed() {
    let d = Downloader::new(Arc::new(Script::new(|_, _| unreachable!())))
        .with_config(Config {
            connections: 99,
            attempts: 0,
            min_block: 0,
            max_buffer: 0,
            ..Config::default()
        })
        .with_hosts(Arc::new(HostCaps::new(2)));
    assert_eq!(
        (
            d.config().connections,
            d.config().attempts,
            d.config().min_block
        ),
        (paddock_download::MAX_CONNECTIONS, 1, 1)
    );
    assert_eq!(d.hosts().cap("any"), 2);
    let defaults = Config::default();
    assert_eq!(defaults.connections, 4);
    assert!(
        defaults.stall_timeout > defaults.max_wait,
        "a deliberate wait never counts as a stall"
    );
}

// ---- the engine over a scripted transport ----

type Answer = Result<Response, TransportError>;
type Answerer = Box<dyn Fn(&Get<'_>, usize) -> Answer + Send + Sync>;

struct Script {
    calls: AtomicUsize,
    gets: Mutex<Vec<(Option<ByteRange>, Option<String>)>>,
    answer: Answerer,
}

impl Script {
    fn new(answer: impl Fn(&Get<'_>, usize) -> Answer + Send + Sync + 'static) -> Script {
        Script {
            calls: AtomicUsize::new(0),
            gets: Mutex::new(Vec::new()),
            answer: Box::new(answer),
        }
    }
}

impl Transport for Script {
    fn get(&self, g: &Get<'_>) -> Answer {
        self.gets
            .lock()
            .unwrap()
            .push((g.range, g.if_range.map(str::to_string)));
        (self.answer)(g, self.calls.fetch_add(1, Ordering::SeqCst))
    }
}

fn resp(status: u16, headers: Headers, body: impl Read + Send + 'static) -> Answer {
    Ok(Response {
        status,
        headers,
        body: Box::new(body),
    })
}

fn ranged(data: &[u8], g: &Get<'_>) -> Answer {
    let r = g.range.unwrap();
    let end = r
        .end
        .map_or(data.len() - 1, |e| (e as usize).min(data.len() - 1));
    let headers = Headers {
        content_range: Some(format!("bytes {}-{end}/{}", r.start, data.len())),
        etag: Some("\"e\"".into()),
        ..Headers::default()
    };
    resp(
        206,
        headers,
        Cursor::new(data[r.start as usize..=end].to_vec()),
    )
}

fn whole(data: &[u8]) -> Answer {
    let headers = Headers {
        content_length: Some(data.len().to_string()),
        ..Headers::default()
    };
    resp(200, headers, Cursor::new(data.to_vec()))
}

fn quick() -> Config {
    Config {
        connections: 3,
        segment_threshold: 64 * 1024,
        min_block: 16 * 1024,
        max_block: 64 * 1024,
        max_buffer: 256 * 1024,
        attempts: 3,
        backoff: Duration::from_millis(1),
        max_throttles: 2,
        save_every: Duration::from_millis(5),
        ..Config::default()
    }
}

fn run(
    t: Script,
    req: &Request,
) -> (
    Result<paddock_download::Downloaded, DownloadError>,
    Arc<Script>,
) {
    let t = Arc::new(t);
    let d = Downloader::new(t.clone()).with_config(quick());
    let (ctx, _) = job();
    (d.download(&ctx, req), t)
}

fn req_in(dir: &tempfile::TempDir, integrity: Integrity) -> Request {
    Request::new("http://scripted.test/f", dir.path().join("f"), integrity).unwrap()
}

/// A body that yields `data` in pieces, with one `Interrupted` first, then an error or a quiet early end.
struct Flaky {
    data: Cursor<Vec<u8>>,
    interrupted: bool,
    fail_after: Option<usize>,
    read: usize,
}

impl Read for Flaky {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        if let Some(n) = self.fail_after
            && self.read >= n
        {
            return Err(io::Error::new(io::ErrorKind::ConnectionReset, "reset"));
        }
        let max = self
            .fail_after
            .map_or(buf.len(), |n| (n - self.read).min(buf.len()));
        let got = self.data.read(&mut buf[..max])?;
        self.read += got;
        Ok(got)
    }
}

#[test]
fn short_bodies_interrupted_reads_and_extra_bytes_are_handled() {
    let data = blob(200 * 1024);
    let (d2, dir) = (data.clone(), tempfile::tempdir().unwrap());
    // Ranges: the first answer for each range is short without an error, every body starts with Interrupted, and the last
    // answers send a few bytes more than asked for.
    let seen = Mutex::new(std::collections::HashSet::new());
    let t = Script::new(move |g, _| {
        let r = g.range.unwrap();
        if r == (ByteRange {
            start: 0,
            end: Some(0),
        }) {
            return ranged(&d2, g);
        }
        let mut answer = ranged(
            &d2,
            &Get {
                range: Some(ByteRange {
                    start: r.start,
                    end: None,
                }),
                ..*g
            },
        )
        .unwrap();
        let full = {
            let mut v = Vec::new();
            answer.body.read_to_end(&mut v).unwrap();
            v
        };
        let want = (r.end.unwrap() - r.start + 1) as usize;
        let first = seen.lock().unwrap().insert(r.start);
        let body = if first {
            full[..want / 2].to_vec()
        } else {
            full
        };
        answer.body = Box::new(Flaky {
            data: Cursor::new(body),
            interrupted: false,
            fail_after: None,
            read: 0,
        });
        Ok(answer)
    });
    let (got, script) = run(
        t,
        &req_in(
            &dir,
            Integrity::Sha256(Sha256Digest::from_hex(&sha256_hex(&data)).unwrap()),
        ),
    );
    let got = got.unwrap();
    assert_eq!(std::fs::read(&got.path).unwrap(), data);
    assert!(
        script
            .gets
            .lock()
            .unwrap()
            .iter()
            .skip(1)
            .all(|(_, v)| v.as_deref() == Some("\"e\"")),
        "If-Range on every range"
    );
}

#[test]
fn a_tls_failure_is_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let (got, t) = run(
        Script::new(|_, _| {
            Err(TransportError::new(
                TransportErrorKind::Tls,
                "unknown issuer",
            ))
        }),
        &req_in(&dir, Integrity::Unpinned),
    );
    assert!(
        matches!(got, Err(DownloadError::Network { detail, .. }) if detail.contains("unknown issuer"))
    );
    assert_eq!(t.calls.load(Ordering::SeqCst), 1);
    // The same on a range after the probe, and transient failures on a range are retried.
    let data = blob(100 * 1024);
    let d2 = data.clone();
    let t = Script::new(move |g, n| match n {
        0 => ranged(&d2, g),
        1 => Err(TransportError::new(TransportErrorKind::Timeout, "slow")),
        _ => Err(TransportError::new(TransportErrorKind::Tls, "bad cert")),
    });
    let (got, _) = run(t, &req_in(&dir, Integrity::Unpinned));
    assert!(
        matches!(got, Err(DownloadError::Network { detail, .. }) if detail.contains("bad cert"))
    );
}

#[test]
fn a_cancel_that_arrives_with_a_failed_attempt_ends_as_cancelled_without_another_request() {
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    let token = ctx.cancel_token().clone();
    let t = Arc::new(Script::new(move |_, _| {
        // A coverage test for the halt path of the back-off after a failed attempt: the cancel arrives with the failure, so
        // the download ends as cancelled after one request. It does not prove the back-off checks the cancel: the next
        // attempt checks it too.
        token.cancel();
        Err(TransportError::new(TransportErrorKind::Timeout, "slow"))
    }));
    let d = Downloader::new(t.clone()).with_config(quick());
    let got = d.download(&ctx, &req_in(&dir, Integrity::Unpinned));
    assert!(
        matches!(got, Err(DownloadError::Cancelled(CancelReason::User))),
        "{got:?}"
    );
    assert_eq!(t.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_panicking_transport_is_contained() {
    let dir = tempfile::tempdir().unwrap();
    let (got, _) = run(
        Script::new(|_, _| panic!("transport bug")),
        &req_in(&dir, Integrity::Unpinned),
    );
    assert!(
        matches!(&got, Err(DownloadError::Io(e)) if e.to_string().contains("transport bug")),
        "{got:?}"
    );
    let data = blob(100 * 1024);
    let d2 = data.clone();
    let t = Script::new(move |g, n| {
        if n == 0 {
            ranged(&d2, g)
        } else {
            std::panic::panic_any(format!("range bug {n}"))
        }
    });
    let (got, _) = run(t, &req_in(&dir, Integrity::Unpinned));
    assert!(
        matches!(&got, Err(DownloadError::Io(e)) if e.to_string().contains("range bug")),
        "{got:?}"
    );
    let t = Script::new(|_, _| std::panic::panic_any(7u8));
    let (got, _) = run(t, &req_in(&dir, Integrity::Unpinned));
    assert!(
        matches!(&got, Err(DownloadError::Io(e)) if e.to_string().contains("unknown panic")),
        "{got:?}"
    );
}

#[test]
fn a_stream_that_keeps_failing_or_reconnects_badly_gives_up() {
    let data = blob(50_000);
    let dir = tempfile::tempdir().unwrap();
    let d2 = data.clone();
    let flaky = move || {
        let headers = Headers {
            content_length: Some(d2.len().to_string()),
            ..Headers::default()
        };
        resp(
            200,
            headers,
            Flaky {
                data: Cursor::new(d2.clone()),
                interrupted: false,
                fail_after: Some(1000),
                read: 0,
            },
        )
    };
    // Every answer resets after 1000 bytes: the stream skips what it has, gets nothing new, and gives up.
    let f = flaky.clone();
    let (got, t) = run(
        Script::new(move |_, _| f()),
        &req_in(&dir, Integrity::Unpinned),
    );
    assert!(matches!(got, Err(DownloadError::Network { .. })), "{got:?}");
    assert_eq!(t.calls.load(Ordering::SeqCst), 3);
    // Reconnecting times out, then the next attempt fails for good.
    let f = flaky.clone();
    let t = Script::new(move |_, n| match n {
        0 => f(),
        1 => Err(TransportError::new(TransportErrorKind::Connect, "refused")),
        _ => Err(TransportError::new(TransportErrorKind::Proxy, "proxy down")),
    });
    let (got, _) = run(t, &req_in(&dir, Integrity::Unpinned));
    assert!(
        matches!(got, Err(DownloadError::Network { detail, .. }) if detail.contains("proxy down"))
    );
    // A host that sends more than its Content-Length.
    let d3 = data.clone();
    let t = Script::new(move |_, _| {
        resp(
            200,
            Headers {
                content_length: Some("10".into()),
                ..Headers::default()
            },
            Cursor::new(d3.clone()),
        )
    });
    let (got, _) = run(t, &req_in(&dir, Integrity::Unpinned));
    assert!(
        matches!(got, Err(DownloadError::SizeMismatch { expected: 10, .. })),
        "{got:?}"
    );
    // An unsized stream shorter than the pinned size is refused at the end.
    let t = Script::new(|_, _| resp(200, Headers::default(), Cursor::new(vec![1u8; 5])));
    let req = req_in(&dir, Integrity::Unpinned).size(10);
    let (got, _) = run(t, &req);
    assert!(matches!(
        got,
        Err(DownloadError::SizeMismatch {
            expected: 10,
            actual: 5
        })
    ));
    assert!(!req.part_path().exists());
}

#[test]
fn an_unpinned_ranged_download_returns_its_digest_and_resumes_by_validator() {
    let data = blob(150 * 1024);
    let d2 = data.clone();
    let dir = tempfile::tempdir().unwrap();
    let req = req_in(&dir, Integrity::Unpinned);
    let (got, _) = run(Script::new(move |g, _| ranged(&d2, g)), &req);
    assert_eq!(got.unwrap().sha256.to_hex(), sha256_hex(&data));
}

#[test]
fn a_damaged_or_oversized_resume_file_starts_over() {
    let data = blob(100 * 1024);
    let dir = tempfile::tempdir().unwrap();
    let req = req_in(&dir, Integrity::Unpinned);
    for state in [
        "{not json".to_string(),
        "x".repeat(70 * 1024),
        r#"{"version":2,"url":"u","size":1,"sha256":null,"validator":null,"hashed":0}"#.into(),
    ] {
        std::fs::write(req.part_path(), &data).unwrap();
        std::fs::write(req.resume_path(), state).unwrap();
        let d2 = data.clone();
        let (got, _) = run(Script::new(move |g, _| ranged(&d2, g)), &req);
        assert_eq!(got.unwrap().resumed_from, 0);
    }
}

#[test]
fn a_destination_that_cannot_be_created_is_an_io_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("file"), b"").unwrap();
    let req = Request::new(
        "http://scripted.test/f",
        dir.path().join("file/sub/f"),
        Integrity::Unpinned,
    )
    .unwrap();
    let (got, t) = run(Script::new(|_, _| unreachable!()), &req);
    assert!(matches!(got, Err(DownloadError::Io(_))));
    assert_eq!(t.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn a_slow_range_is_split_so_neither_the_tail_nor_the_hash_waits_on_it() {
    // The fake decides what is slow, not the clock (ADR 0031): the first range is held open until another connection takes
    // over part of it, and the other ranges are answered only once the first one was asked for, so no connection can finish
    // and split the first range before its request is sent (CI 7110: a late first worker found its range already split
    // down to below `2 * min_block`, so nothing was left to split while it was slow). If the engine stops splitting a slow
    // range, the held range never moves: the download ends as `Stalled` (or nextest's timeout ends the test), and the
    // guard below then lets the held threads go.
    let data = blob(2 * 1024 * 1024);
    let d2 = data.clone();
    let gate = Arc::new(Gate::default());
    let _release = Abandon(gate.clone());
    let g2 = gate.clone();
    let t = Script::new(move |g, _| {
        let r = g.range.unwrap();
        let mut a = ranged(&d2, g)?;
        if r.start == 0 && r.end != Some(0) {
            g2.slow_asked(r.end.unwrap());
            a.body = Box::new(Held {
                gate: g2.clone(),
                body: a.body,
            });
        } else if r.start > 0 {
            g2.other_asked(r.start);
        }
        Ok(a)
    });
    let dir = tempfile::tempdir().unwrap();
    let t = Arc::new(t);
    // Three connections take the three ranges inside the buffer window, so none is free to split before the first range is
    // asked for. The held connections keep their host permits, so the count must stay within the host cap.
    let config = Config {
        connections: 3,
        min_block: 8 * 1024,
        max_block: 128 * 1024,
        ..quick()
    };
    assert!(config.connections <= DEFAULT_HOST_CAP);
    let d = Downloader::new(t.clone()).with_config(config.clone());
    let (ctx, rec) = job();
    let got = d
        .download(&ctx, &req_in(&dir, Integrity::Unpinned))
        .unwrap();
    assert_eq!(std::fs::read(got.path).unwrap(), data);
    rec.assert_monotonic();
    let ranges: Vec<ByteRange> = t
        .gets
        .lock()
        .unwrap()
        .iter()
        .skip(1)
        .filter_map(|(r, _)| *r)
        .collect();
    let first_end = ranges
        .iter()
        .find(|r| r.start == 0)
        .and_then(|r| r.end)
        .unwrap();
    assert!(
        first_end + 1 >= 2 * config.min_block,
        "the first range was split before it was asked for: {ranges:?}"
    );
    assert!(
        ranges.iter().any(|r| r.start > 0 && r.start <= first_end),
        "the slow first range was not split: {ranges:?}"
    );
}

/// What the scripted server has seen: the end of the slow first range once asked for, and whether another range started
/// inside it since. `abandoned` ends every wait once the test is over, so no held thread outlives it.
#[derive(Default)]
struct Gate {
    seen: Mutex<Seen>,
    moved: Condvar,
}

#[derive(Default)]
struct Seen {
    slow_end: Option<u64>,
    split: bool,
    abandoned: bool,
}

impl Gate {
    fn slow_asked(&self, end: u64) {
        self.seen.lock().unwrap().slow_end = Some(end);
        self.moved.notify_all();
    }

    /// Waits until the slow range was asked for, then notes whether `start` lies inside it.
    fn other_asked(&self, start: u64) {
        let mut s = self
            .moved
            .wait_while(self.seen.lock().unwrap(), |s| {
                s.slow_end.is_none() && !s.abandoned
            })
            .unwrap();
        if s.slow_end.is_some_and(|end| start <= end) {
            s.split = true;
            self.moved.notify_all();
        }
    }

    fn wait_for_split(&self) {
        drop(
            self.moved
                .wait_while(self.seen.lock().unwrap(), |s| !s.split && !s.abandoned)
                .unwrap(),
        );
    }

    fn abandon(&self) {
        // Not `unwrap`: this runs while a failed assert unwinds, and a poisoned lock must not turn that into an abort.
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .abandoned = true;
        self.moved.notify_all();
    }
}

/// Releases every wait of the gate when the test ends, however it ends.
struct Abandon(Arc<Gate>);

impl Drop for Abandon {
    fn drop(&mut self) {
        self.0.abandon();
    }
}

/// The slow range's body: no byte until another connection took over part of the range.
struct Held {
    gate: Arc<Gate>,
    body: Box<dyn Read + Send>,
}

impl Read for Held {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.gate.wait_for_split();
        self.body.read(buf)
    }
}

#[test]
fn the_phases_of_a_scripted_download_are_in_order() {
    let data = blob(10);
    let d2 = data.clone();
    let dir = tempfile::tempdir().unwrap();
    let t = Arc::new(Script::new(move |_, _| whole(&d2)));
    let d = Downloader::new(t).with_config(quick());
    let (ctx, rec) = job();
    d.download(&ctx, &req_in(&dir, Integrity::Unpinned))
        .unwrap();
    assert_eq!(
        rec.phases(),
        [phases::CONNECT, phases::DOWNLOAD, phases::VERIFY]
    );
}

fn plain_ranged(data: &[u8], g: &Get<'_>) -> Answer {
    let mut a = ranged(data, g)?;
    a.headers.etag = None;
    Ok(a)
}

fn state(
    req: &Request,
    size: usize,
    sha256: Option<&str>,
    validator: Option<&str>,
    hashed: usize,
) -> String {
    let q = |v: Option<&str>| v.map_or("null".to_string(), |v| format!("{v:?}"));
    format!(
        r#"{{"version":1,"url":"{}","size":{size},"sha256":{},"validator":{},"hashed":{hashed}}}"#,
        req.url(),
        q(sha256),
        q(validator)
    )
}

#[test]
fn without_a_validator_only_a_pinned_checksum_allows_a_resume() {
    let data = blob(400 * 1024);
    let hex = sha256_hex(&data);
    let dir = tempfile::tempdir().unwrap();
    let pinned = req_in(
        &dir,
        Integrity::Sha256(Sha256Digest::from_hex(&hex).unwrap()),
    );
    let resume = |req: &Request, saved: String| {
        std::fs::write(req.part_path(), &data).unwrap();
        std::fs::write(req.resume_path(), saved).unwrap();
        let d2 = data.clone();
        let (got, _) = run(Script::new(move |g, _| plain_ranged(&d2, g)), req);
        got.unwrap().resumed_from
    };
    // Pinned, no validator then or now: resumed.
    assert_eq!(
        resume(
            &pinned,
            state(&pinned, data.len(), Some(&hex), None, 100_000)
        ),
        100_000
    );
    // Another pinned checksum, a validator then but none now, another size: started over.
    assert_eq!(
        resume(
            &pinned,
            state(&pinned, data.len(), Some(&"0".repeat(64)), None, 100_000)
        ),
        0
    );
    assert_eq!(
        resume(
            &pinned,
            state(&pinned, data.len(), Some(&hex), Some("\"old\""), 100_000)
        ),
        0
    );
    assert_eq!(
        resume(
            &pinned,
            state(&pinned, data.len() + 1, Some(&hex), None, 100_000)
        ),
        0
    );
    // Unpinned and no validator: nothing can prove it is the same file.
    let unpinned = req_in(&dir, Integrity::Unpinned);
    assert_eq!(
        resume(&unpinned, state(&unpinned, data.len(), None, None, 100_000)),
        0
    );
    // A saved prefix longer than the partial file is not trusted.
    std::fs::write(pinned.part_path(), &data[..10]).unwrap();
    std::fs::write(
        pinned.resume_path(),
        state(&pinned, data.len(), Some(&hex), None, 100_000),
    )
    .unwrap();
    let d2 = data.clone();
    let (got, _) = run(Script::new(move |g, _| plain_ranged(&d2, g)), &pinned);
    assert_eq!(got.unwrap().resumed_from, 0);
}

#[test]
fn a_response_shows_its_status_and_headers_but_not_its_body() {
    let r = resp(
        206,
        Headers {
            etag: Some("\"e\"".into()),
            ..Headers::default()
        },
        Cursor::new(b"secret".to_vec()),
    )
    .unwrap();
    let text = format!("{r:?}");
    assert!(
        text.contains("206") && text.contains("\\\"e\\\"") && !text.contains("secret"),
        "{text}"
    );
}

#[test]
fn a_reconnect_shorter_than_what_was_written_is_retried() {
    let data = blob(5000);
    let d2 = data.clone();
    let dir = tempfile::tempdir().unwrap();
    // No length: the first answer resets after 3000 bytes, the second ends quietly after 1000, the third is whole.
    let t = Script::new(move |_, n| {
        let body: Box<dyn Read + Send> = match n {
            0 => Box::new(Flaky {
                data: Cursor::new(d2.clone()),
                interrupted: true,
                fail_after: Some(3000),
                read: 0,
            }),
            1 => Box::new(Cursor::new(d2[..1000].to_vec())),
            _ => Box::new(Cursor::new(d2.clone())),
        };
        Ok(Response {
            status: 200,
            headers: Headers::default(),
            body,
        })
    });
    let (got, t) = run(t, &req_in(&dir, Integrity::Unpinned));
    assert_eq!(std::fs::read(got.unwrap().path).unwrap(), data);
    assert_eq!(t.calls.load(Ordering::SeqCst), 3);
}

#[test]
fn a_weak_etag_is_not_a_validator_but_last_modified_is() {
    let data = blob(200 * 1024);
    let dir = tempfile::tempdir().unwrap();
    let req = req_in(&dir, Integrity::Unpinned);
    let lm = "Wed, 21 Oct 2026 07:28:00 GMT";
    let d2 = data.clone();
    let answer = move |g: &Get<'_>| {
        let mut a = ranged(&d2, g)?;
        a.headers.etag = Some("W/\"weak\"".into());
        a.headers.last_modified = Some(format!(" {lm} "));
        Ok(a)
    };
    std::fs::write(req.part_path(), &data).unwrap();
    std::fs::write(
        req.resume_path(),
        state(&req, data.len(), None, Some(lm), 50_000),
    )
    .unwrap();
    let a2 = answer.clone();
    let (got, t) = run(Script::new(move |g, _| a2(g)), &req);
    assert_eq!(got.unwrap().resumed_from, 50_000);
    assert!(
        t.gets
            .lock()
            .unwrap()
            .iter()
            .skip(1)
            .all(|(_, v)| v.as_deref() == Some(lm)),
        "If-Range carries Last-Modified"
    );
    // An empty Last-Modified validates nothing.
    let d3 = data.clone();
    let t = Script::new(move |g, _| {
        let mut a = ranged(&d3, g)?;
        a.headers.etag = Some(" ".into());
        a.headers.last_modified = Some(" ".into());
        Ok(a)
    });
    let (_, t) = run(t, &req);
    assert!(t.gets.lock().unwrap().iter().all(|(_, v)| v.is_none()));
}

#[test]
fn a_sized_stream_that_ends_early_is_retried() {
    let data = blob(5000);
    let d2 = data.clone();
    let dir = tempfile::tempdir().unwrap();
    let t = Script::new(move |_, n| {
        let headers = Headers {
            content_length: Some("5000".into()),
            ..Headers::default()
        };
        let body = if n == 0 {
            d2[..3000].to_vec()
        } else {
            d2.clone()
        };
        resp(200, headers, Cursor::new(body))
    });
    let (got, t) = run(t, &req_in(&dir, Integrity::Unpinned));
    assert_eq!(std::fs::read(got.unwrap().path).unwrap(), data);
    assert_eq!(t.calls.load(Ordering::SeqCst), 2);
}

// ---- free disk space (plan §13: checked before anything is written) ----

#[test]
fn a_pinned_size_that_does_not_fit_the_disk_is_refused_before_connecting() {
    let dir = tempfile::tempdir().unwrap();
    let t = Arc::new(Script::new(|_, _| {
        unreachable!("no request when the disk is too small")
    }));
    let d = Downloader::new(t.clone())
        .with_config(Config {
            disk_reserve: 100,
            ..quick()
        })
        .with_free_space(|_| Ok(1_000));
    let (ctx, _) = job();
    let req = req_in(&dir, Integrity::Unpinned).size(950);
    match d.download(&ctx, &req) {
        Err(
            e @ DownloadError::NotEnoughSpace {
                needed: 1_050,
                free: 1_000,
                ..
            },
        ) => {
            assert!(e.keeps_partial());
            let text = e.to_string();
            assert!(
                text.starts_with("the download needs 1.1 KB of free space in ")
                    && text.ends_with(", 1 KB is free; free some space there or choose another folder, then run it again"),
                "{text}"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(t.calls.load(Ordering::SeqCst), 0);
    assert!(!req.part_path().exists());
}

#[test]
fn the_announced_size_is_checked_once_known_and_a_kept_prefix_counts_as_written() {
    let data = blob(200 * 1024);
    let dir = tempfile::tempdir().unwrap();
    let d2 = data.clone();
    let t = Arc::new(Script::new(move |g, _| match g.range {
        Some(_) => ranged(&d2, g),
        None => whole(&d2),
    }));
    // Too little room for 200 KiB plus the reserve: refused after the probe, nothing written.
    let tight = Downloader::new(t.clone())
        .with_config(Config {
            disk_reserve: 0,
            ..quick()
        })
        .with_free_space(|_| Ok(100 * 1024));
    let (ctx, _) = job();
    let req = req_in(&dir, Integrity::Unpinned);
    assert!(matches!(
        tight.download(&ctx, &req),
        Err(DownloadError::NotEnoughSpace { needed, free, .. }) if needed == 200 * 1024 && free == 100 * 1024
    ));
    assert!(!req.dest().exists());
    // A partial file of 150 KiB leaves 50 KiB to write, which fits (without its resume state it is then fetched again).
    std::fs::write(req.part_path(), vec![0u8; 150 * 1024]).unwrap();
    let (ctx, _) = job();
    assert_eq!(tight.download(&ctx, &req).unwrap().bytes, data.len() as u64);
    std::fs::remove_file(req.dest()).unwrap();
    // When the free space cannot be read the download goes on.
    let blind = Downloader::new(t)
        .with_config(quick())
        .with_free_space(|_| Err(io::Error::other("no statvfs")));
    let (ctx, _) = job();
    assert_eq!(blind.download(&ctx, &req).unwrap().bytes, data.len() as u64);
}

#[test]
fn the_platform_free_space_is_read_and_sizes_read_well() {
    // The default asks the operating system (the temp folder's disk has room for a few bytes plus the reserve).
    let dir = tempfile::tempdir().unwrap();
    let data = blob(4 * 1024);
    let d2 = data.clone();
    let d = Downloader::new(Arc::new(Script::new(move |_, _| whole(&d2)))).with_config(Config {
        disk_reserve: 1,
        ..quick()
    });
    let (ctx, _) = job();
    let req = req_in(&dir, Integrity::Unpinned).size(data.len() as u64);
    assert_eq!(d.download(&ctx, &req).unwrap().bytes, data.len() as u64);
    assert!(Arc::strong_count(d.transport()) >= 1);
    for (n, text) in [
        (12, "12 bytes"),
        (1_000, "1 KB"),
        (1_550_000, "1.6 MB"),
        (999_000, "999 KB"),
        (2_000_000_000, "2 GB"),
        (1_234_000_000_000, "1234 GB"),
    ] {
        assert_eq!(
            DownloadError::NotEnoughSpace {
                needed: n,
                free: 0,
                path: std::path::PathBuf::new()
            }
            .to_string()
            .split(" of free")
            .next()
            .unwrap(),
            format!("the download needs {text}")
        );
    }
}

#[test]
fn a_small_get_that_fails_for_good_is_not_retried_and_names_the_host() {
    let script = Arc::new(Script::new(|_, _| {
        Err(TransportError::new(
            TransportErrorKind::Tls,
            "unknown issuer",
        ))
    }));
    let d = Downloader::new(script.clone());
    let e = d
        .get_bytes("https://updates.example.test/latest.json", &Fetch::new())
        .unwrap_err();
    assert!(
        matches!(&e, DownloadError::Network { host, detail }
            if host == "updates.example.test" && detail.contains("unknown issuer")),
        "{e:?}"
    );
    assert_eq!(script.calls.load(Ordering::SeqCst), 1);
}
