//! Progress and cancel of copies from a drive or share: bytes from the first one, one scale per phase, the checksum
//! computed while writing, no second read pass, and Cancel. (Downloads over http are `downloads.rs`.)
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::checksum::Sha256Digest;
use spotter_core::job::{Cancel, Job, Phase, Progress, Unit, CANCELLED};
use spotter_core::release::Release;
use spotter_core::version::VersionScheme;
use spotter_sources::download::{download_release, Fetch};
use spotter_sources::http::Http;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn release(location: &str, sha256: Option<Sha256Digest>) -> Release {
    Release {
        version: VersionScheme::Semver.extract("1.2.0").unwrap(),
        name: "app".into(),
        location: location.into(),
        published_at: None,
        size: None,
        sha256,
        detail: Default::default(),
    }
}

type Seen = Arc<Mutex<Vec<Progress>>>;

/// A job that records every report (no throttling) and runs `on` for each, which may cancel.
fn recording(on: impl Fn(&Progress, &Cancel) + Send + Sync + 'static) -> (Job, Seen) {
    let seen: Seen = Arc::default();
    let cancel = Cancel::new();
    let (sink, c) = (seen.clone(), cancel.clone());
    let job = Job::new(cancel, move |p| {
        on(p, &c);
        sink.lock().unwrap().push(p.clone());
    })
    .with_interval(Duration::ZERO);
    (job, seen)
}

fn phases(seen: &Seen) -> Vec<Phase> {
    let mut out: Vec<Phase> = Vec::new();
    for p in seen.lock().unwrap().iter() {
        if out.last() != Some(&p.phase) {
            out.push(p.phase);
        }
    }
    out
}

fn assert_increasing(seen: &Seen, phase: Phase) {
    let seen = seen.lock().unwrap();
    let mine: Vec<&Progress> = seen.iter().filter(|p| p.phase == phase).collect();
    assert!(mine.len() >= 3, "{phase:?} reported {} times", mine.len());
    assert!(mine.windows(2).all(|w| w[0].done <= w[1].done), "{phase:?} never goes back");
    assert!(mine.last().unwrap().done > mine[0].done, "{phase:?} advances");
    assert!(mine.iter().all(|p| p.unit == Unit::Bytes));
}

/// Copies `release` as the app does, with a downloader that is never used for a path.
async fn copy_of(
    release: &Release,
    folder: &std::path::Path,
    verify: bool,
    job: &Job,
) -> Result<spotter_sources::download::Downloaded, String> {
    let registry = Arc::new(paddock_jobs::JobRegistry::new(Arc::new(Quiet)));
    let ctx = registry.register("sp.download", "Copying", None);
    let downloader = Arc::new(paddock_download::Downloader::http(paddock_download::HttpConfig::default()).unwrap());
    let http = Http::new("t").unwrap();
    let fetch = Fetch {
        downloader: &downloader,
        http: &http,
        ctx: &ctx,
    };
    download_release(&fetch, release, folder, verify, job).await
}

struct Quiet;
impl paddock_jobs::EventSink for Quiet {
    fn emit(&self, _: &paddock_jobs::JobSnapshot) {}
}

#[tokio::test]
async fn a_copy_reports_bytes_from_the_start_hashes_while_writing_and_can_be_cancelled() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let big: Vec<u8> = (0..(5 << 20)).map(|i| (i % 253) as u8).collect();
    let file = src.path().join("R1.02.00T01.zip");
    std::fs::write(&file, &big).unwrap();
    let known = Sha256Digest::of(&big);
    let (job, seen) = recording(|_, _| {});
    let d = copy_of(&release(&file.to_string_lossy(), Some(known)), dest.path(), true, &job)
        .await
        .unwrap();
    assert!(d.verified);
    assert_eq!(phases(&seen), vec![Phase::Copying]);
    assert_eq!(seen.lock().unwrap()[0].done, 0);
    assert_increasing(&seen, Phase::Copying);
    assert_eq!(seen.lock().unwrap().last().unwrap().total, Some(big.len() as u64));

    let (job, _) = recording(|p, c| {
        if p.phase == Phase::Copying && p.done > 0 {
            c.cancel();
        }
    });
    let e = copy_of(&release(&file.to_string_lossy(), None), dest.path(), false, &job)
        .await
        .unwrap_err();
    assert_eq!(e, CANCELLED);
}
