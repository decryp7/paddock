//! The kept executable downloader (ADR 0070 D5: the FFmpeg runtime and the research browser's download use it until M2c)
//! against a local mock of GitHub releases: which hosts and files it fetches, resume after a cancel, a corrupt archive
//! refused, the segmented range download through a redirect, and a host that ignores `Range`. Never the real network.

use crate::common;
use common::{blob, partial, sha256_hex, Mock, Resp};
use debrief_ai::models::{DiskSpace, DownloadOptions, Downloader, ModelSource, Progress};
use debrief_ai::runtimes::{GithubReleases, ReleaseConfig, RuntimeCatalog};
use debrief_ai::AiError;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

struct Disk;
impl DiskSpace for Disk {
    fn available_bytes(&self, _: &Path) -> Option<u64> {
        None
    }
}

fn catalog(name: &str, data: &[u8]) -> RuntimeCatalog {
    RuntimeCatalog::parse(&format!(
        r#"{{"version":1,"release":"v9","tag":"b7","platforms":{{"test":[{{"id":"cpu","assets":[{{"name":"{name}","url":"https://github.com/ggml-org/llama.cpp/releases/download/b7/{name}","size":{},"sha256":"{}"}}]}}]}}}}"#,
        data.len(),
        sha256_hex(data)
    ))
    .unwrap()
}

/// GitHub: the release URL redirects to the "CDN", which serves byte ranges when `ranges` and ignores them otherwise.
fn github(name: &'static str, data: Vec<u8>, ranges: bool, delay_ms: u64) -> Mock {
    Mock::start(move |req, port| {
        let file = req
            .path
            .split('?')
            .next()
            .unwrap_or("")
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string();
        if file != name {
            return Resp::new(404, "");
        }
        if req.path.starts_with("/ggml-org/") {
            return Resp::redirect(&format!("http://127.0.0.1:{port}/cdn/{name}?sig=secret"));
        }
        let mut r = if ranges {
            partial(req, &data)
        } else {
            Resp::new(200, data.clone())
        };
        r.delay_ms = delay_ms;
        r
    })
}

fn source(mock: &Mock, catalog: &RuntimeCatalog) -> GithubReleases {
    GithubReleases::new(
        ReleaseConfig {
            loopback_base: Some(mock.url()),
            chunk_bytes: 4096,
            ..ReleaseConfig::default()
        },
        catalog.clone(),
    )
    .unwrap()
}

fn opts(connections: usize) -> DownloadOptions {
    DownloadOptions {
        max_bytes: 10_000_000,
        margin_bytes: 0,
        attempts: 2,
        backoff: Duration::from_millis(1),
        connections,
        segment_threshold: 32 * 1024,
        min_block: 8 * 1024,
        host_gate: None,
    }
}

/// What a runtime download accepts as a file name (the FFmpeg runtime's rule: an archive, no separators).
fn archive_name(n: &str) -> bool {
    n.ends_with(".zip") && !n.contains(['/', '\\']) && !n.contains("..")
}

fn download(src: &GithubReleases, name: &str, dir: &Path, o: DownloadOptions, p: &Progress) -> debrief_ai::Result<std::path::PathBuf> {
    let remote = src.describe("b7", name)?;
    Downloader {
        source: src,
        disk: &Disk,
        opts: o,
    }
    .download_checked(&remote, dir, p, archive_name)
}

#[test]
fn only_github_hosts_and_catalog_assets_are_fetched() {
    let data = blob(1000);
    let c = catalog("a.zip", &data);
    let evil = Mock::start(|_, _| Resp::redirect("https://evil.example/a.zip"));
    let dir = tempfile::tempdir().unwrap();
    let e = download(&source(&evil, &c), "a.zip", dir.path(), opts(1), &Progress::new()).unwrap_err();
    assert!(matches!(&e, AiError::Refused(m) if m.contains("evil.example")), "{e:?}");
    let src = source(&evil, &c);
    assert!(
        matches!(src.describe("b7", "other.zip"), Err(AiError::Refused(_))),
        "a file the catalog does not list"
    );
    assert!(
        matches!(src.describe("b6", "a.zip"), Err(AiError::Refused(_))),
        "another release than the pinned one"
    );
    let d = src.describe("b7", "a.zip").unwrap();
    assert_eq!((d.size, d.sha256.len()), (data.len() as u64, 64));
}

#[test]
fn a_cancelled_download_resumes() {
    let data = blob(60_000);
    let c = catalog("a.zip", &data);
    let mock = github("a.zip", data.clone(), true, 20);
    let dir = tempfile::tempdir().unwrap();
    let p = Arc::new(Progress::new());
    let canceller = {
        let p = p.clone();
        std::thread::spawn(move || {
            // A hang guard, not a verdict: the download below fails with its own message if the bytes never arrive.
            let _ = paddock_testkit::wait::poll_until_within(Duration::from_secs(20), || p.snapshot().bytes >= 8192);
            p.cancel();
        })
    };
    let r = download(&source(&mock, &c), "a.zip", dir.path(), opts(1), &p);
    canceller.join().unwrap();
    assert_eq!(r.unwrap_err(), AiError::Cancelled);
    assert!(dir.path().join("a.zip.part").is_file(), "the partial download is kept");
    let before = mock.log().len();
    let path = download(&source(&mock, &c), "a.zip", dir.path(), opts(1), &Progress::new()).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), data);
    assert!(
        mock.log()[before..].iter().filter_map(common::range_of).any(|(a, _)| a > 0),
        "the second run continued where the first stopped"
    );
}

#[test]
fn a_corrupt_download_is_refused_and_nothing_appears_under_the_real_name() {
    let data = blob(5000);
    let c = catalog("a.zip", &data);
    let mut tampered = data.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xff;
    let mock = github("a.zip", tampered, true, 0);
    let dir = tempfile::tempdir().unwrap();
    let e = download(&source(&mock, &c), "a.zip", dir.path(), opts(1), &Progress::new()).unwrap_err();
    assert!(matches!(e, AiError::Integrity(_)), "{e:?}");
    assert!(!dir.path().join("a.zip").exists());
}

#[test]
fn a_range_capable_cdn_downloads_in_segments_and_matches_the_original() {
    let data = blob(400_000);
    let c = catalog("big.zip", &data);
    let mock = github("big.zip", data.clone(), true, 0);
    let src = source(&mock, &c);
    assert!(src.supports_ranges(&src.describe("b7", "big.zip").unwrap()));
    let dir = tempfile::tempdir().unwrap();
    let path = download(&src, "big.zip", dir.path(), opts(4), &Progress::new()).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), data);
    let ranged = mock.log().iter().filter_map(common::range_of).count();
    assert!(ranged > 1, "several range requests, not one stream: {ranged}");
}

#[test]
fn a_host_that_ignores_range_is_no_range_host_and_the_download_says_so() {
    let data = blob(100_000);
    let c = catalog("big.zip", &data);
    let mock = github("big.zip", data.clone(), false, 0);
    let src = source(&mock, &c);
    assert!(
        !src.supports_ranges(&src.describe("b7", "big.zip").unwrap()),
        "a 200 to a Range request is no range support"
    );
    let dir = tempfile::tempdir().unwrap();
    let e = download(&src, "big.zip", dir.path(), opts(4), &Progress::new()).unwrap_err();
    assert!(
        matches!(&e, AiError::Network(m) if m.contains("ignored the resume request")),
        "{e:?}"
    );
    assert!(!dir.path().join("big.zip").exists(), "nothing appears under the real name");
}
