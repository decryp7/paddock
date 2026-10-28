//! Release downloads over the shell's downloader against an in-process HTTP server (no network): a host with byte ranges
//! (segmented) and one without, 429 with Retry-After, a connection dropped mid-body, a wrong checksum, the publisher's
//! checksum file, a full disk, and Cancel with a resume afterwards.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod support;

use spotter_core::checksum::Sha256Digest;
use spotter_core::job::{Job, CANCELLED};
use spotter_core::release::Release;
use spotter_core::version::VersionScheme;
use spotter_sources::download::{download_release, Fetch};
use spotter_sources::http::Http;
use std::time::{Duration, Instant};
use support::{blob, downloader, job, Host, Server};

fn release(server: &Server, sha256: Option<Sha256Digest>) -> Release {
    Release {
        version: VersionScheme::Semver.extract("1.2.0").unwrap(),
        name: "f.bin".into(),
        location: server.url("/f.bin"),
        published_at: None,
        size: None,
        sha256,
        detail: Default::default(),
    }
}

async fn run(
    server: &Server,
    sha256: Option<Sha256Digest>,
    verify: bool,
    dir: &std::path::Path,
) -> Result<spotter_sources::download::Downloaded, String> {
    let (_registry, ctx) = job();
    let downloader = downloader();
    let http = Http::new("test").unwrap();
    let fetch = Fetch {
        downloader: &downloader,
        http: &http,
        ctx: &ctx,
    };
    download_release(&fetch, &release(server, sha256), dir, verify, &Job::none()).await
}

#[tokio::test]
async fn a_host_with_byte_ranges_is_downloaded_in_segments_and_verified_while_writing() {
    let data = blob(2 * 1024 * 1024 + 77);
    let server = Server::start(data.clone(), Host::default());
    let dir = tempfile::tempdir().unwrap();
    let got = run(&server, Some(Sha256Digest::of(&data)), true, dir.path()).await.unwrap();
    assert!(got.verified);
    assert_eq!(got.size, data.len() as u64);
    assert_eq!(std::fs::read(&got.path).unwrap(), data);
    assert!(server.range_requests() > 1, "segmented: {:?}", server.log());
    assert!(!dir.path().join("f.bin.part").exists(), "only the finished file is left");
}

#[tokio::test]
async fn a_host_without_byte_ranges_is_downloaded_in_one_stream() {
    let data = blob(900 * 1024);
    let server = Server::start(
        data.clone(),
        Host {
            ranges: false,
            ..Host::default()
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let got = run(&server, None, false, dir.path()).await.unwrap();
    assert!(!got.verified, "no checksum was known");
    assert_eq!(std::fs::read(&got.path).unwrap(), data);
    assert_eq!(server.range_requests(), 0);
}

#[tokio::test]
async fn throttling_with_retry_after_is_waited_out() {
    let data = blob(600 * 1024);
    let server = Server::start(
        data.clone(),
        Host {
            throttle_first: 2,
            ..Host::default()
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let got = run(&server, Some(Sha256Digest::of(&data)), true, dir.path()).await.unwrap();
    assert_eq!(std::fs::read(&got.path).unwrap(), data);
    assert!(server.log().len() > 3, "the throttled requests were repeated: {:?}", server.log());
}

#[tokio::test]
async fn a_connection_dropped_mid_body_is_retried_and_the_file_is_still_exact() {
    let data = blob(1024 * 1024);
    let server = Server::start(
        data.clone(),
        Host {
            drop_first_body_after: Some(100 * 1024),
            ..Host::default()
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let got = run(&server, Some(Sha256Digest::of(&data)), true, dir.path()).await.unwrap();
    assert!(got.verified);
    assert_eq!(std::fs::read(&got.path).unwrap(), data);
}

#[tokio::test]
async fn a_wrong_checksum_deletes_the_file_and_says_so() {
    let data = blob(400 * 1024);
    let server = Server::start(data.clone(), Host::default());
    let dir = tempfile::tempdir().unwrap();
    let wrong = Sha256Digest::of(b"something else");
    let e = run(&server, Some(wrong), true, dir.path()).await.unwrap_err();
    assert!(
        e.contains("does not match the publisher's checksum") && e.contains("deleted"),
        "{e}"
    );
    assert!(!dir.path().join("f.bin").exists() && !dir.path().join("f.bin.part").exists());
}

#[tokio::test]
async fn the_publishers_checksum_file_is_used_when_the_release_has_no_digest() {
    let data = blob(300 * 1024);
    let sum = format!("{}  f.bin\n", Sha256Digest::of(&data));
    let server = Server::start(
        data.clone(),
        Host {
            sidecar: Some(sum),
            ..Host::default()
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let got = run(&server, None, true, dir.path()).await.unwrap();
    assert!(got.verified, "verified against the .sha256 next to the release");
    let bad = Server::start(
        data.clone(),
        Host {
            sidecar: Some(format!("{}  f.bin\n", "0".repeat(64))),
            ..Host::default()
        },
    );
    let e = run(&bad, None, true, dir.path()).await.unwrap_err();
    assert!(e.contains("does not match the publisher's checksum"), "{e}");
}

#[tokio::test]
async fn a_full_disk_is_refused_before_anything_is_written() {
    let data = blob(300 * 1024);
    let server = Server::start(data, Host::default());
    let dir = tempfile::tempdir().unwrap();
    let (_registry, ctx) = job();
    let downloader = std::sync::Arc::new(
        paddock_download::Downloader::http(paddock_download::HttpConfig::default())
            .unwrap()
            .with_free_space(|_| Ok(10 * 1024)),
    );
    let http = Http::new("test").unwrap();
    let fetch = Fetch {
        downloader: &downloader,
        http: &http,
        ctx: &ctx,
    };
    let e = download_release(&fetch, &release(&server, None), dir.path(), false, &Job::none())
        .await
        .unwrap_err();
    assert!(e.to_lowercase().contains("space") || e.to_lowercase().contains("disk"), "{e}");
    assert!(!dir.path().join("f.bin").exists());
}

#[tokio::test]
async fn cancel_stops_a_slow_download_at_once_keeps_the_part_and_a_later_run_finishes() {
    let data = blob(3 * 1024 * 1024);
    let server = Server::start(
        data.clone(),
        Host {
            slow_chunk: 8 * 1024,
            ..Host::default()
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let (_registry, ctx) = job();
    let downloader = downloader();
    let http = Http::new("test").unwrap();
    let token = ctx.cancel_token().clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        token.cancel();
    });
    let fetch = Fetch {
        downloader: &downloader,
        http: &http,
        ctx: &ctx,
    };
    let started = Instant::now();
    let e = download_release(
        &fetch,
        &release(&server, Some(Sha256Digest::of(&data))),
        dir.path(),
        true,
        &Job::none(),
    )
    .await
    .unwrap_err();
    assert_eq!(e, CANCELLED);
    assert!(started.elapsed() < Duration::from_secs(3), "cancel took {:?}", started.elapsed());
    assert!(!dir.path().join("f.bin").exists(), "an unfinished file never looks finished");
    assert!(dir.path().join("f.bin.part").exists(), "the part is kept for a resume");
    let fast = Server::start(data.clone(), Host::default());
    let got = run(&fast, Some(Sha256Digest::of(&data)), true, dir.path()).await.unwrap();
    assert_eq!(std::fs::read(&got.path).unwrap(), data);
}
