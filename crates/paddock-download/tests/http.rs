//! The downloader over the real HTTP transport against an in-process server: hosts with and without byte ranges, 429 and
//! Retry-After, dropped and truncated connections, digest and size mismatches, resume, per-host caps, and cancel in every phase.
#![cfg(feature = "http")]

mod support;

use paddock_download::{
    Config, DownloadError, Downloader, HostCaps, HttpConfig, Integrity, ProxyMode, Request,
    Sha256Digest, phases,
};
use paddock_jobs::CancelReason;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use support::{Resp, Server, blob, job, partial, partial_with, sha256_hex};

fn fast() -> Config {
    Config {
        connections: 4,
        segment_threshold: 256 * 1024,
        min_block: 64 * 1024,
        max_block: 256 * 1024,
        max_buffer: 1024 * 1024,
        attempts: 4,
        backoff: Duration::from_millis(5),
        max_wait: Duration::from_secs(5),
        max_throttles: 3,
        stall_timeout: Duration::from_secs(10),
        range_timeout: Duration::from_secs(10),
        save_every: Duration::from_millis(20),
        ..Config::default()
    }
}

/// No proxy, whatever the machine running the tests is set up with.
fn direct() -> HttpConfig {
    HttpConfig {
        proxy: ProxyMode::Direct,
        ..HttpConfig::default()
    }
}

fn downloader(config: Config) -> Downloader {
    Downloader::http(direct()).unwrap().with_config(config)
}

fn pinned(data: &[u8]) -> Integrity {
    Integrity::Sha256(Sha256Digest::from_hex(&sha256_hex(data)).unwrap())
}

fn request(server: &Server, dir: &tempfile::TempDir, data: &[u8]) -> Request {
    Request::new(
        &server.url("/f.bin"),
        dir.path().join("out/f.bin"),
        pinned(data),
    )
    .unwrap()
}

fn ranges_requested(server: &Server) -> Vec<(usize, Option<usize>)> {
    server
        .log()
        .iter()
        .filter_map(|r| r.range())
        .filter(|r| *r != (0, Some(0)))
        .collect()
}

#[test]
fn a_range_host_is_downloaded_in_segments_hashed_in_order_and_renamed_into_place() {
    let data = blob(3 * 1024 * 1024 + 123);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2).rate(8 * 1024 * 1024));
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data).size(data.len() as u64);
    let (ctx, rec) = job();
    let got = downloader(fast()).download(&ctx, &req).unwrap();
    assert_eq!(std::fs::read(req.dest()).unwrap(), data);
    assert_eq!(got.sha256.to_hex(), sha256_hex(&data));
    assert_eq!(
        (got.bytes, got.resumed_from, got.ranges),
        (data.len() as u64, 0, true)
    );
    assert!(
        got.connections > 1 && got.connections <= 4,
        "used {} connections",
        got.connections
    );
    assert!(!req.part_path().exists() && !req.resume_path().exists());
    assert_eq!(
        rec.phases(),
        [phases::CONNECT, phases::DOWNLOAD, phases::VERIFY]
    );
    rec.assert_monotonic();
    let last_download = rec
        .progress()
        .into_iter()
        .rev()
        .find(|p| p.0 == phases::DOWNLOAD)
        .unwrap();
    assert_eq!(
        (last_download.1, last_download.2),
        (data.len() as u64, Some(data.len() as u64))
    );
    // Every byte was asked for once, in ranges of at most max_block, with If-Range.
    let mut covered: Vec<_> = ranges_requested(&server)
        .iter()
        .map(|&(a, b)| (a, b.unwrap() + 1))
        .collect();
    covered.sort();
    assert!(
        covered.windows(2).all(|w| w[0].1 <= w[1].0),
        "overlapping ranges: {covered:?}"
    );
    assert_eq!(
        covered.iter().map(|(a, b)| b - a).sum::<usize>(),
        data.len()
    );
    assert!(
        server
            .log()
            .iter()
            .skip(1)
            .all(|r| r.headers.get("if-range").map(String::as_str) == Some("\"v1\""))
    );
    assert!(
        server
            .log()
            .iter()
            .all(|r| r.headers.get("accept-encoding").map(String::as_str) == Some("identity"))
    );
}

#[test]
fn a_small_file_uses_one_connection() {
    let data = blob(100 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2));
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data);
    let (ctx, _) = job();
    let got = downloader(fast()).download(&ctx, &req).unwrap();
    assert_eq!((got.connections, got.bytes), (1, data.len() as u64));
    // Two ranges (the smallest block is 64 KiB), one after the other.
    assert_eq!(
        ranges_requested(&server),
        [(0, Some(65_535)), (65_536, Some(102_399))]
    );
}

#[test]
fn a_host_without_ranges_is_streamed_once_and_a_drop_restarts_and_skips_what_was_written() {
    let data = blob(700 * 1024);
    let d2 = data.clone();
    // Ignores Range; the first two answers drop the connection part-way.
    let server = Server::start(move |_, n| {
        let r = Resp::new(200, d2.clone()).header("etag", "\"s\"");
        if n < 2 {
            r.cut(200 * 1024 * (n + 1))
        } else {
            r
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data);
    let (ctx, rec) = job();
    let got = downloader(fast()).download(&ctx, &req).unwrap();
    assert_eq!(std::fs::read(req.dest()).unwrap(), data);
    assert!(!got.ranges && got.connections == 1);
    assert_eq!(server.log().len(), 3);
    assert_eq!(
        rec.phases(),
        [phases::CONNECT, phases::DOWNLOAD, phases::VERIFY]
    );
    rec.assert_monotonic();
    assert!(!req.resume_path().exists());
}

#[test]
fn a_stream_without_length_shows_a_growing_count_and_ends_at_eof() {
    let data = blob(300 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |_, _| Resp::new(200, d2.clone()).no_length());
    let dir = tempfile::tempdir().unwrap();
    let req = Request::new(&server.url("/f"), dir.path().join("f"), Integrity::Unpinned).unwrap();
    let (ctx, rec) = job();
    let got = downloader(fast()).download(&ctx, &req).unwrap();
    assert_eq!(got.sha256.to_hex(), sha256_hex(&data));
    let download: Vec<_> = rec
        .progress()
        .into_iter()
        .filter(|p| p.0 == phases::DOWNLOAD)
        .collect();
    assert!(
        download.iter().all(|p| p.2.is_none()),
        "no total while the host does not say"
    );
    assert!(download.last().unwrap().1 == data.len() as u64);
    rec.assert_monotonic();
}

#[test]
fn a_stream_over_the_limit_is_refused_while_streaming() {
    let server = Server::start(|_, _| Resp::new(200, blob(200 * 1024)).no_length());
    let dir = tempfile::tempdir().unwrap();
    let req = Request::new(&server.url("/f"), dir.path().join("f"), Integrity::Unpinned)
        .unwrap()
        .max_bytes(100 * 1024);
    let (ctx, _) = job();
    let e = downloader(fast()).download(&ctx, &req).unwrap_err();
    assert!(
        matches!(e, DownloadError::TooLarge { limit: 102_400, .. }),
        "{e:?}"
    );
    assert!(!req.part_path().exists() && !req.dest().exists());
}

#[test]
fn an_announced_size_over_the_limit_or_unlike_the_pinned_one_is_refused_before_writing() {
    let data = blob(64 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2));
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    let big = request(&server, &dir, &data).max_bytes(1000);
    assert!(matches!(
        downloader(fast()).download(&ctx, &big),
        Err(DownloadError::TooLarge {
            size: 65_536,
            limit: 1000
        })
    ));
    let wrong = request(&server, &dir, &data).size(5);
    assert!(matches!(
        downloader(fast()).download(&ctx, &wrong),
        Err(DownloadError::SizeMismatch {
            expected: 5,
            actual: 65_536
        })
    ));
    assert!(!wrong.part_path().exists());
    // The same for a host without ranges (Content-Length).
    let d3 = data.clone();
    let plain = Server::start(move |_, _| Resp::new(200, d3.clone()));
    let req = request(&plain, &dir, &data).size(7);
    assert!(matches!(
        downloader(fast()).download(&ctx, &req),
        Err(DownloadError::SizeMismatch { expected: 7, .. })
    ));
}

#[test]
fn a_digest_mismatch_deletes_everything_and_says_so() {
    let data = blob(400 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2));
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, b"something else");
    let (ctx, rec) = job();
    let e = downloader(fast()).download(&ctx, &req).unwrap_err();
    match &e {
        DownloadError::ChecksumMismatch { actual, .. } => assert_eq!(*actual, sha256_hex(&data)),
        other => panic!("{other:?}"),
    }
    assert!(e.to_string().contains("checksum does not match") && !e.keeps_partial());
    assert!(!req.dest().exists() && !req.part_path().exists() && !req.resume_path().exists());
    assert_eq!(
        rec.phases().last().map(String::as_str),
        Some(phases::VERIFY)
    );
}

#[test]
fn a_digest_mismatch_on_a_stream_deletes_the_partial_file() {
    let server = Server::start(|_, _| Resp::new(200, blob(1000)));
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, b"x");
    let (ctx, _) = job();
    assert!(matches!(
        downloader(fast()).download(&ctx, &req),
        Err(DownloadError::ChecksumMismatch { .. })
    ));
    assert!(!req.part_path().exists() && !req.dest().exists());
}

#[test]
fn truncated_segments_are_fetched_again_from_where_they_stopped() {
    let data = blob(1024 * 1024);
    let d2 = data.clone();
    let cuts = Arc::new(AtomicUsize::new(0));
    let c2 = cuts.clone();
    // Every range request is cut half-way the first time it is seen.
    let seen = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let server = Server::start(move |r, _| {
        let resp = partial(r, &d2);
        match r.range() {
            Some((a, Some(b))) if b > a && seen.lock().unwrap().insert(a) => {
                c2.fetch_add(1, Ordering::SeqCst);
                resp.cut((b - a) / 2)
            }
            _ => resp,
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data);
    let (ctx, rec) = job();
    let got = downloader(fast()).download(&ctx, &req).unwrap();
    assert_eq!(std::fs::read(&got.path).unwrap(), data);
    assert!(cuts.load(Ordering::SeqCst) >= 4);
    rec.assert_monotonic();
}

#[test]
fn a_connection_that_keeps_dropping_fails_with_a_network_error_and_keeps_the_partial_file() {
    let data = blob(600 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, n| {
        if n == 0 {
            partial(r, &d2)
        } else {
            partial(r, &d2).cut(0)
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data);
    let (ctx, _) = job();
    let e = downloader(fast()).download(&ctx, &req).unwrap_err();
    assert!(matches!(&e, DownloadError::Network { .. }), "{e:?}");
    assert!(e.keeps_partial() && req.part_path().exists() && req.resume_path().exists());
}

/// A downloader whose host caps (and so every wait of its downloads) run on a fake clock.
fn on_fake_clock() -> (
    paddock_download::Downloader,
    Arc<HostCaps>,
    paddock_jobs::time::FakeClock,
) {
    let clock = paddock_jobs::time::FakeClock::new();
    let hosts = Arc::new(HostCaps::default().with_clock(Arc::new(clock.clone())));
    (downloader(fast()).with_hosts(hosts.clone()), hosts, clock)
}

/// A hang guard around a state the download reaches after an event the test caused (never a timing assertion).
fn eventually(f: impl Fn() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < Duration::from_secs(60), "never reached");
        std::thread::yield_now();
    }
}

#[test]
fn http_429_with_retry_after_holds_the_host_for_that_long_and_the_download_still_completes() {
    let data = blob(1024 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, n| {
        if n == 1 {
            Resp::new(429, "slow down").header("retry-after", "1")
        } else {
            partial(r, &d2)
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data);
    let (d, hosts, clock) = on_fake_clock();
    let (ctx, rec) = job();
    let run = std::thread::spawn(move || d.download(&ctx, &req).map(|g| g.path));
    // The throttled range waits for the host on the clock: the download cannot finish before the second passes.
    eventually(|| hosts.held(&server.host()) >= 1);
    assert!(!run.is_finished());
    clock.advance(Duration::from_secs(1));
    let got = run.join().unwrap().unwrap();
    assert_eq!(std::fs::read(&got).unwrap(), data);
    assert!(
        server.log().len() >= 3,
        "the throttled range was fetched again"
    );
    rec.assert_monotonic();
}

#[test]
fn retry_after_holds_every_request_to_the_host() {
    let data = blob(64 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, n| {
        if n == 0 {
            Resp::new(503, "").header("retry-after", "1")
        } else {
            partial(r, &d2)
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let (d, hosts, clock) = on_fake_clock();
    let (ctx, _) = job();
    let req = request(&server, &dir, &data);
    let run = std::thread::spawn(move || d.download(&ctx, &req).map(|_| ()));
    eventually(|| hosts.held(&server.host()) >= 1);
    // Held: no other request reached the host.
    assert_eq!(server.log().len(), 1);
    clock.advance(Duration::from_secs(1));
    run.join().unwrap().unwrap();
    assert!(server.log().len() >= 2);
}

#[test]
fn a_host_that_keeps_throttling_one_connection_ends_the_download_as_throttled() {
    let data = blob(600 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, n| {
        if n == 0 {
            partial(r, &d2)
        } else {
            Resp::new(429, "")
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data);
    let (ctx, _) = job();
    let e = downloader(fast()).download(&ctx, &req).unwrap_err();
    assert!(matches!(e, DownloadError::Throttled { .. }), "{e:?}");
    assert!(e.keeps_partial() && req.resume_path().exists());
    // The probe itself gives up the same way.
    let always = Server::start(|_, _| Resp::new(429, ""));
    let e = downloader(fast())
        .download(&ctx, &request(&always, &dir, &data))
        .unwrap_err();
    assert!(matches!(e, DownloadError::Throttled { .. }));
}

#[test]
fn server_errors_are_retried_and_client_errors_are_not() {
    let data = blob(10_000);
    let d2 = data.clone();
    let server = Server::start(move |r, n| {
        if n < 2 {
            Resp::new(500, "")
        } else {
            partial(r, &d2)
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    downloader(fast())
        .download(&ctx, &request(&server, &dir, &data))
        .unwrap();
    assert_eq!(server.log().len(), 4);

    let missing = Server::start(|_, _| Resp::new(404, ""));
    let e = downloader(fast())
        .download(&ctx, &request(&missing, &dir, &data))
        .unwrap_err();
    assert!(matches!(e, DownloadError::Http { status: 404, .. }) && e.to_string().contains("404"));
    assert_eq!(missing.log().len(), 1);

    let broken = Server::start(|_, _| Resp::new(502, ""));
    let e = downloader(fast())
        .download(&ctx, &request(&broken, &dir, &data))
        .unwrap_err();
    assert!(matches!(e, DownloadError::Network { .. }) && broken.log().len() == 4);
}

#[test]
fn errors_on_a_range_or_a_stream_are_typed() {
    let data = blob(600 * 1024);
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    // A range answered 404 after the probe; a range answered 500 every time.
    for (status, check) in [(404u16, "Http"), (500, "Network")] {
        let d2 = data.clone();
        let s = Server::start(move |r, n| {
            if n == 0 {
                partial(r, &d2)
            } else {
                Resp::new(status, "")
            }
        });
        let e = downloader(fast())
            .download(&ctx, &request(&s, &dir, &data))
            .unwrap_err();
        assert!(format!("{e:?}").starts_with(check), "{e:?}");
    }
    // A stream: 404 and 500 on reconnect, 429 on reconnect until giving up.
    for (status, check) in [(404u16, "Http"), (500, "Network"), (429, "Throttled")] {
        let d2 = data.clone();
        let s = Server::start(move |_, n| {
            if n == 0 {
                Resp::new(200, d2.clone()).cut(1000)
            } else {
                Resp::new(status, "")
            }
        });
        let e = downloader(fast())
            .download(&ctx, &request(&s, &dir, &data))
            .unwrap_err();
        assert!(format!("{e:?}").starts_with(check), "{e:?}");
    }
}

#[test]
fn a_file_that_changes_during_the_download_is_refused_and_deleted() {
    let data = blob(600 * 1024);
    let d2 = data.clone();
    // The probe sees v1; the ranges see v2, so If-Range makes the host send the whole file with 200.
    let server = Server::start(move |r, n| {
        if n == 0 {
            partial(r, &d2)
        } else {
            partial_with(r, &d2, "\"v2\"")
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data);
    let (ctx, _) = job();
    let e = downloader(fast()).download(&ctx, &req).unwrap_err();
    assert!(
        matches!(e, DownloadError::Changed { .. }) && !e.keeps_partial(),
        "{e:?}"
    );
    assert!(!req.part_path().exists() && !req.resume_path().exists());
    // A stream whose host describes another file after a drop.
    let d3 = data.clone();
    let s = Server::start(move |_, n| {
        if n == 0 {
            Resp::new(200, d3.clone()).header("etag", "\"a\"").cut(1000)
        } else {
            Resp::new(200, d3.clone()).header("etag", "\"b\"")
        }
    });
    assert!(matches!(
        downloader(fast()).download(&ctx, &request(&s, &dir, &data)),
        Err(DownloadError::Changed { .. })
    ));
}

#[test]
fn compressed_answers_and_wrong_ranges_are_refused() {
    let data = blob(600 * 1024);
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    let gz = Server::start(|_, _| Resp::new(200, "x").header("content-encoding", "gzip"));
    let e = downloader(fast())
        .download(&ctx, &request(&gz, &dir, &data))
        .unwrap_err();
    assert!(
        matches!(&e, DownloadError::Protocol { detail, .. } if detail.contains("gzip")),
        "{e:?}"
    );
    let d2 = data.clone();
    let gz_range = Server::start(move |r, n| {
        if n == 0 {
            partial(r, &d2)
        } else {
            partial(r, &d2).header("content-encoding", "br")
        }
    });
    assert!(matches!(
        downloader(fast()).download(&ctx, &request(&gz_range, &dir, &data)),
        Err(DownloadError::Protocol { .. })
    ));
    let d3 = data.clone();
    let wrong = Server::start(move |r, n| {
        if n == 0 {
            partial(r, &d3)
        } else {
            Resp::new(206, d3[..10].to_vec())
                .header("content-range", &format!("bytes 0-9/{}", d3.len()))
        }
    });
    let e = downloader(fast())
        .download(&ctx, &request(&wrong, &dir, &data))
        .unwrap_err();
    assert!(
        matches!(e, DownloadError::Protocol { .. }) && e.to_string().contains("cannot be used")
    );
    let d4 = data.clone();
    let gz_stream = Server::start(move |_, n| {
        if n == 0 {
            Resp::new(200, d4.clone()).cut(10)
        } else {
            Resp::new(200, d4.clone()).header("content-encoding", "gzip")
        }
    });
    assert!(matches!(
        downloader(fast()).download(&ctx, &request(&gz_stream, &dir, &data)),
        Err(DownloadError::Protocol { .. })
    ));
}

#[test]
fn a_probe_answer_without_a_usable_range_falls_back_to_one_stream() {
    let data = blob(50_000);
    let d2 = data.clone();
    // 206 with an unknown total, then the whole file on the plain GET.
    let server = Server::start(move |r, _| {
        if r.range().is_some() {
            Resp::new(206, d2[..1].to_vec()).header("content-range", "bytes 0-0/*")
        } else {
            Resp::new(200, d2.clone())
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    let got = downloader(fast())
        .download(&ctx, &request(&server, &dir, &data))
        .unwrap();
    assert!(!got.ranges);
    assert_eq!(std::fs::read(&got.path).unwrap(), data);
}

#[test]
fn empty_files_download_over_both_kinds_of_host() {
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    let ranged = Server::start(|r, _| partial(r, &[]));
    let got = downloader(fast())
        .download(&ctx, &request(&ranged, &dir, &[]))
        .unwrap();
    assert_eq!(
        (got.bytes, got.ranges, got.sha256.to_hex()),
        (0, true, sha256_hex(&[]))
    );
    assert_eq!(std::fs::read(&got.path).unwrap(), b"");
    let plain = Server::start(|_, _| Resp::new(200, Vec::new()));
    let got = downloader(fast())
        .download(&ctx, &request(&plain, &dir, &[]))
        .unwrap();
    assert_eq!((got.bytes, got.ranges), (0, false));
    let odd = Server::start(|_, _| Resp::new(416, Vec::new()).header("content-range", "bytes */9"));
    assert!(matches!(
        downloader(fast()).download(&ctx, &request(&odd, &dir, &[])),
        Err(DownloadError::Http { status: 416, .. })
    ));
}

#[test]
fn redirects_are_followed_with_the_range() {
    let data = blob(600 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, _| {
        if r.path == "/f.bin" {
            Resp::new(302, "").header("location", "/cdn/f.bin")
        } else {
            partial(r, &d2)
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    let got = downloader(fast())
        .download(&ctx, &request(&server, &dir, &data))
        .unwrap();
    assert!(got.ranges);
    assert_eq!(std::fs::read(&got.path).unwrap(), data);
}

#[test]
fn a_stopped_download_resumes_from_the_hashed_prefix() {
    let data = blob(2 * 1024 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2).rate(16 * 1024 * 1024));
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data);
    let d = downloader(fast());
    let (ctx, rec) = job();
    rec.cancel_at(phases::DOWNLOAD, 1024 * 1024);
    let e = d.download(&ctx, &req).unwrap_err();
    assert!(
        matches!(e, DownloadError::Cancelled(CancelReason::User)),
        "{e:?}"
    );
    assert!(req.part_path().exists() && req.resume_path().exists() && !req.dest().exists());
    let before = server.log().len();

    let (ctx, rec) = job();
    let got = d.download(&ctx, &req).unwrap();
    assert_eq!(std::fs::read(&got.path).unwrap(), data);
    assert!(got.resumed_from > 0, "nothing was resumed");
    assert_eq!(
        rec.phases(),
        [
            phases::CONNECT,
            phases::RESUME,
            phases::DOWNLOAD,
            phases::VERIFY
        ]
    );
    rec.assert_monotonic();
    // The download phase starts at the resumed bytes, and the second run asked only for what was missing.
    let first_download = rec
        .progress()
        .into_iter()
        .find(|p| p.0 == phases::DOWNLOAD)
        .unwrap();
    assert!(first_download.1 == 0 || first_download.1 == got.resumed_from);
    let asked: usize = server.log()[before..]
        .iter()
        .filter_map(|r| r.range())
        .filter(|r| *r != (0, Some(0)))
        .map(|(a, b)| b.unwrap() + 1 - a)
        .sum();
    assert_eq!(asked as u64, data.len() as u64 - got.resumed_from);
}

#[test]
fn a_partial_file_of_another_version_is_not_resumed() {
    let data = blob(600 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial_with(r, &d2, "\"v9\""));
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data);
    std::fs::create_dir_all(req.dest().parent().unwrap()).unwrap();
    std::fs::write(req.part_path(), vec![7u8; data.len()]).unwrap();
    let state = format!(
        r#"{{"version":1,"url":"{}","size":{},"sha256":"{}","validator":"\"v1\"","hashed":300000}}"#,
        req.url(),
        data.len(),
        sha256_hex(&data)
    );
    std::fs::write(req.resume_path(), state).unwrap();
    let (ctx, rec) = job();
    let got = downloader(fast()).download(&ctx, &req).unwrap();
    assert_eq!(got.resumed_from, 0);
    assert!(!rec.phases().contains(&phases::RESUME.to_string()));
    assert_eq!(std::fs::read(&got.path).unwrap(), data);
}

#[test]
fn a_complete_partial_file_cancelled_while_verifying_is_finished_without_a_request_for_bytes() {
    let data = blob(300 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2));
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data);
    let d = downloader(fast());
    let (ctx, rec) = job();
    rec.cancel_at(phases::VERIFY, 0);
    assert!(matches!(
        d.download(&ctx, &req),
        Err(DownloadError::Cancelled(_))
    ));
    assert!(
        req.part_path().exists() && !req.dest().exists(),
        "nothing is renamed after a cancel"
    );
    let before = server.log().len();
    let (ctx, _) = job();
    let got = d.download(&ctx, &req).unwrap();
    assert_eq!(got.resumed_from, data.len() as u64);
    assert_eq!(server.log().len(), before + 1, "only the probe");
}

#[test]
fn the_host_cap_holds_across_downloads() {
    let data = blob(1024 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2).rate(4 * 1024 * 1024));
    let dir = tempfile::tempdir().unwrap();
    let hosts = Arc::new(HostCaps::new(4).with_cap(&server.host(), 2));
    let d = Arc::new(downloader(fast()).with_hosts(hosts.clone()));
    let threads: Vec<_> = (0..2)
        .map(|i| {
            let (d, data) = (d.clone(), data.clone());
            let req = Request::new(
                &server.url("/f.bin"),
                dir.path().join(format!("f{i}")),
                pinned(&data),
            )
            .unwrap();
            std::thread::spawn(move || {
                let (ctx, _) = job();
                d.download(&ctx, &req).map(|g| g.bytes)
            })
        })
        .collect();
    for t in threads {
        assert_eq!(t.join().unwrap().unwrap(), data.len() as u64);
    }
    assert!(
        server.peak() <= 2,
        "the server saw {} connections at once",
        server.peak()
    );
    assert_eq!(hosts.peak(&server.host()), 2);
    assert_eq!(hosts.in_use(&server.host()), 0);
}

#[test]
fn a_stalled_transfer_ends_as_stalled_and_keeps_the_partial_file() {
    let data = blob(600 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, n| {
        if n == 0 {
            partial(r, &d2)
        } else {
            partial(r, &d2).stall(1000)
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data);
    let (ctx, _) = job();
    let config = Config {
        stall_timeout: Duration::from_millis(300),
        ..fast()
    };
    let e = downloader(config).download(&ctx, &req).unwrap_err();
    assert!(
        matches!(&e, DownloadError::Stalled { host, .. } if *host == server.host()),
        "{e:?}"
    );
    assert!(e.keeps_partial() && req.resume_path().exists());
}

#[test]
fn an_unreachable_host_is_a_network_error_after_the_attempts() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dir = tempfile::tempdir().unwrap();
    let req = Request::new(
        &format!("http://127.0.0.1:{port}/f"),
        dir.path().join("f"),
        Integrity::Unpinned,
    )
    .unwrap();
    let (ctx, _) = job();
    let e = downloader(fast()).download(&ctx, &req).unwrap_err();
    assert!(matches!(e, DownloadError::Network { .. }), "{e:?}");
}

#[test]
fn an_invalid_proxy_is_refused_when_the_mode_is_built() {
    assert!(matches!(
        ProxyMode::explicit("http://[::1"),
        Err(DownloadError::InvalidRequest(_))
    ));
    assert!(matches!(
        ProxyMode::explicit("socks5://h:1080"),
        Err(DownloadError::InvalidRequest(_))
    ));
}

// ---- a cancel ends the download in every phase (the outcome is asserted, never how long it took) ----

#[test]
fn cancel_while_connecting_returns_at_once() {
    let server = Server::start(|r, _| partial(r, b"data").delay(Duration::from_secs(5)));
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    let token = ctx.cancel_token().clone();
    let req = request(&server, &dir, b"data");
    let run = std::thread::spawn(move || downloader(fast()).download(&ctx, &req).map(|_| ()));
    // The host holds the answer for five seconds; the cancel comes once it has the request.
    eventually(|| !server.log().is_empty());
    token.cancel();
    let e = run.join().unwrap().unwrap_err();
    assert!(matches!(e, DownloadError::Cancelled(CancelReason::User)));
}

#[test]
fn cancel_while_waiting_for_retry_after_returns_at_once() {
    let server = Server::start(|_, _| Resp::new(429, "").header("retry-after", "30"));
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    let token = ctx.cancel_token().clone();
    let config = Config {
        max_wait: Duration::from_secs(30),
        ..fast()
    };
    let clock = paddock_jobs::time::FakeClock::new();
    let hosts = Arc::new(HostCaps::default().with_clock(Arc::new(clock)));
    let d = downloader(config).with_hosts(hosts.clone());
    let req = request(&server, &dir, b"data");
    let run = std::thread::spawn(move || d.download(&ctx, &req).map(|_| ()));
    // Thirty seconds on a clock that does not move: only the cancel ends the wait.
    eventually(|| hosts.held(&server.host()) >= 1);
    token.cancel();
    assert!(matches!(
        run.join().unwrap(),
        Err(DownloadError::Cancelled(_))
    ));
}

#[test]
fn cancel_while_the_host_is_held_back_returns_at_once() {
    let server = Server::start(|r, _| partial(r, b"data"));
    let dir = tempfile::tempdir().unwrap();
    let clock = paddock_jobs::time::FakeClock::new();
    let hosts = Arc::new(HostCaps::new(1).with_clock(Arc::new(clock)));
    let d = downloader(fast()).with_hosts(hosts.clone());
    let (ctx, _) = job();
    let token = ctx.cancel_token().clone();
    hosts.back_off(&server.host(), Duration::from_secs(30));
    let req = request(&server, &dir, b"data");
    let run = std::thread::spawn(move || d.download(&ctx, &req).map(|_| ()));
    eventually(|| hosts.held(&server.host()) >= 1);
    token.cancel();
    assert!(matches!(
        run.join().unwrap(),
        Err(DownloadError::Cancelled(_))
    ));
    assert!(server.log().is_empty());
    // The waiter (on the probe's own thread) leaves at its next look at the cancel.
    eventually(|| hosts.held(&server.host()) == 0);
}

#[test]
fn cancel_while_resuming_returns_at_once_and_keeps_the_state() {
    let data = blob(4 * 1024 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2));
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data);
    std::fs::create_dir_all(req.dest().parent().unwrap()).unwrap();
    std::fs::write(req.part_path(), &data).unwrap();
    let state = format!(
        r#"{{"version":1,"url":"{}","size":{},"sha256":"{}","validator":"\"v1\"","hashed":{}}}"#,
        req.url(),
        data.len(),
        sha256_hex(&data),
        data.len() - 1
    );
    std::fs::write(req.resume_path(), &state).unwrap();
    let (ctx, rec) = job();
    rec.cancel_at(phases::RESUME, 1);
    let e = downloader(fast()).download(&ctx, &req).unwrap_err();
    assert!(matches!(e, DownloadError::Cancelled(_)));
    assert_eq!(
        std::fs::read_to_string(req.resume_path()).unwrap(),
        state,
        "the resume state is unchanged"
    );
}

#[test]
fn cancel_while_downloading_a_stalled_range_returns_at_once_and_keeps_the_partial_file() {
    let data = blob(600 * 1024);
    let d2 = data.clone();
    let server = Server::start(move |r, n| {
        if n == 0 {
            partial(r, &d2)
        } else {
            partial(r, &d2).stall(10_000)
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let req = request(&server, &dir, &data);
    let (ctx, rec) = job();
    rec.cancel_at(phases::DOWNLOAD, 10_000);
    let e = downloader(fast()).download(&ctx, &req).unwrap_err();
    assert!(matches!(e, DownloadError::Cancelled(_)));
    assert!(req.part_path().exists() && req.resume_path().exists() && !req.dest().exists());
}

#[test]
fn cancel_while_streaming_returns_at_once_and_removes_the_unresumable_partial_file() {
    let server = Server::start(|_, _| Resp::new(200, blob(100_000)).stall(5000));
    let dir = tempfile::tempdir().unwrap();
    let req = Request::new(&server.url("/f"), dir.path().join("f"), Integrity::Unpinned).unwrap();
    let (ctx, rec) = job();
    rec.cancel_at(phases::DOWNLOAD, 5000);
    let e = downloader(fast()).download(&ctx, &req).unwrap_err();
    assert!(matches!(e, DownloadError::Cancelled(_)));
    assert!(!req.part_path().exists() && !req.dest().exists());
}

#[test]
fn cancel_while_verifying_leaves_the_partial_file_and_no_destination() {
    let server = Server::start(|_, _| Resp::new(200, b"abc".to_vec()));
    let dir = tempfile::tempdir().unwrap();
    let req = Request::new(&server.url("/f"), dir.path().join("f"), Integrity::Unpinned).unwrap();
    let (ctx, rec) = job();
    rec.cancel_at(phases::VERIFY, 0);
    let e = downloader(fast()).download(&ctx, &req).unwrap_err();
    assert!(matches!(e, DownloadError::Cancelled(_)));
    assert!(!req.dest().exists());
}

#[test]
fn a_job_cancelled_before_it_starts_does_nothing() {
    let server = Server::start(|r, _| partial(r, b"data"));
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    ctx.cancel_token().cancel();
    let e = downloader(fast())
        .download(&ctx, &request(&server, &dir, b"data"))
        .unwrap_err();
    assert!(matches!(e, DownloadError::Cancelled(_)));
    assert!(server.log().is_empty());
}

fn net_detail(r: Result<paddock_download::Downloaded, DownloadError>) -> String {
    match r {
        Err(DownloadError::Network { detail, .. }) => detail,
        other => panic!("expected a network error, got {other:?}"),
    }
}

#[test]
fn transport_failures_are_classified() {
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    // No answer in time: retried, then a network error.
    let slow = Server::start(|r, _| partial(r, b"x").delay(Duration::from_secs(3)));
    let d = Downloader::http(HttpConfig {
        response_timeout: Duration::from_millis(100),
        ..direct()
    })
    .unwrap()
    .with_config(fast());
    let detail = net_detail(d.download(&ctx, &request(&slow, &dir, b"x")));
    assert!(detail.contains("in time"), "{detail}");
    // HTTPS to a host that answers in plain text: the handshake fails and is not retried.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicUsize::new(0));
    let a2 = accepted.clone();
    std::thread::spawn(move || {
        for mut conn in listener.incoming().flatten() {
            a2.fetch_add(1, Ordering::SeqCst);
            // Read the client's hello, answer in plain text and keep the connection open a moment, so the client sees the
            // answer rather than a reset.
            std::thread::spawn(move || {
                let _ = std::io::Read::read(&mut conn, &mut [0u8; 4096]);
                let _ = std::io::Write::write_all(
                    &mut conn,
                    b"HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\n\r\n",
                );
                std::thread::sleep(Duration::from_millis(500));
            });
        }
    });
    let req = Request::new(
        &format!("https://127.0.0.1:{port}/f"),
        dir.path().join("t"),
        Integrity::Unpinned,
    )
    .unwrap();
    let detail = net_detail(downloader(fast()).download(&ctx, &req));
    assert!(detail.contains("verified"), "{detail}");
    assert!(
        accepted.load(Ordering::SeqCst) < 4,
        "not retried by the downloader"
    );
    let plain = Server::start(|r, _| partial(r, b"x"));
    // A proxy that is not there.
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let d = Downloader::http(HttpConfig {
        proxy: ProxyMode::explicit(&format!("http://127.0.0.1:{closed}")).unwrap(),
        ..HttpConfig::default()
    })
    .unwrap()
    .with_config(fast());
    let detail = net_detail(d.download(&ctx, &request(&plain, &dir, b"x")));
    assert!(detail.contains("cannot connect"), "{detail}");
    // A proxy that refuses to tunnel to an HTTPS host (the host itself is never contacted).
    let refusing = Server::start(|_, _| Resp::new(407, ""));
    let d = Downloader::http(HttpConfig {
        proxy: ProxyMode::explicit(&refusing.url("")).unwrap(),
        ..HttpConfig::default()
    })
    .unwrap()
    .with_config(fast());
    let req = Request::new(
        "https://example.invalid/f",
        dir.path().join("p"),
        Integrity::Unpinned,
    )
    .unwrap();
    let detail = net_detail(d.download(&ctx, &req));
    assert!(detail.contains("proxy failed"), "{detail}");
    assert_eq!(refusing.log()[0].method, "CONNECT");
    // A redirect loop.
    let looping = Server::start(|_, _| Resp::new(302, "").header("location", "/again"));
    let req = Request::new(
        &looping.url("/f"),
        dir.path().join("u"),
        Integrity::Unpinned,
    )
    .unwrap();
    let detail = net_detail(downloader(fast()).download(&ctx, &req));
    assert!(detail.contains("invalid"), "{detail}");
    assert_eq!(
        looping.log().len(),
        11,
        "one attempt: the request and ten redirects"
    );
}
