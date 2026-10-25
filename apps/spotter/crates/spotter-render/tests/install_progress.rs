//! The installer's phases, progress and cancel: Downloading, Unpacking and Testing each on their own scale, the
//! checksum taken while downloading (the archive is not read again to hash it), and Cancel in every phase.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use spotter_core::job::{Cancel, Job, Phase, Progress, Unit};
use spotter_render::discover::Os;
use spotter_render::install::{download, extract, install_pinned, sha256_file};
use spotter_render::manifest::Pinned;
use spotter_render::Error;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::{serve, Reply};

const BIG: usize = 6 << 20;

/// A zip with a small browser script and a large stored file, so every phase reports more than once.
fn fake_zip(dir: &Path) -> (Vec<u8>, String) {
    let p = dir.join("fake.zip");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    z.start_file("chrome-linux/chrome", stored).unwrap();
    z.write_all(b"#!/bin/sh\necho Chromium 1\n").unwrap();
    z.start_file("chrome-linux/locales.pak", stored).unwrap();
    z.write_all(&(0..BIG).map(|i| (i % 249) as u8).collect::<Vec<_>>()).unwrap();
    z.finish().unwrap();
    (std::fs::read(&p).unwrap(), sha256_file(&p).unwrap())
}

fn pinned(port: u16, sha: &str) -> Pinned {
    Pinned {
        version: "1".into(),
        url: format!("http://127.0.0.1:{port}/a.zip"),
        sha256: sha.into(),
    }
}

type Seen = Arc<Mutex<Vec<Progress>>>;

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

fn ok_verify(_: &Path) -> spotter_render::Result<String> {
    Ok("Chromium 1".into())
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

#[tokio::test]
async fn the_install_has_three_labelled_phases_each_on_its_own_scale() {
    let tmp = tempfile::tempdir().unwrap();
    let (bytes, sha) = fake_zip(tmp.path());
    let size = bytes.len() as u64;
    let port = serve(move |_| Reply::Bytes(bytes.clone())).await;
    let (job, seen) = recording(|_, _| {});
    install_pinned(&pinned(port, &sha), Os::Linux, &tmp.path().join("data"), &ok_verify, &job)
        .await
        .unwrap();
    assert_eq!(
        phases(&seen),
        vec![Phase::Downloading, Phase::Unpacking, Phase::SelfTest],
        "no phase comes back"
    );
    let seen = seen.lock().unwrap();
    let first = &seen[0];
    assert_eq!(
        (first.phase, first.done, first.unit),
        (Phase::Downloading, 0, Unit::Bytes),
        "bytes from the first moment"
    );
    let dl: Vec<_> = seen.iter().filter(|p| p.phase == Phase::Downloading).collect();
    assert!(dl.len() >= 3 && dl.windows(2).all(|w| w[0].done <= w[1].done));
    assert_eq!((dl.last().unwrap().done, dl.last().unwrap().total), (size, Some(size)));
    let un: Vec<_> = seen.iter().filter(|p| p.phase == Phase::Unpacking).collect();
    assert!(un.len() >= 3 && un.windows(2).all(|w| w[0].done <= w[1].done));
    assert!(un.iter().all(|p| p.unit == Unit::Bytes));
    assert_eq!(
        un.last().unwrap().done,
        un.last().unwrap().total.unwrap(),
        "unpacking reaches its own total"
    );
    assert!(un.last().unwrap().total.unwrap() >= BIG as u64);
}

#[tokio::test]
async fn the_digest_is_taken_while_downloading_and_unpacking_does_not_hash_again() {
    let tmp = tempfile::tempdir().unwrap();
    let (bytes, sha) = fake_zip(tmp.path());
    let port = serve(move |_| Reply::Bytes(bytes.clone())).await;
    let dest = tmp.path().join("got.zip");
    let digest = download(&format!("http://127.0.0.1:{port}/a.zip"), &dest, &Job::none())
        .await
        .unwrap();
    assert_eq!(digest, sha256_file(&dest).unwrap(), "the hash of what was written");
    assert_eq!(digest, sha);
    // `extract` has no checksum to compare with at all: an archive that is not the pinned one still unpacks.
    let other = tmp.path().join("other.zip");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&other).unwrap());
    z.start_file("x/chrome", zip::write::SimpleFileOptions::default()).unwrap();
    z.write_all(b"x").unwrap();
    z.finish().unwrap();
    extract(&other, &tmp.path().join("out"), &Job::none()).unwrap();
    assert!(tmp.path().join("out/x/chrome").is_file());
}

#[tokio::test]
async fn cancel_in_the_download_leaves_nothing_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let (bytes, sha) = fake_zip(tmp.path());
    let port = serve(move |_| Reply::Bytes(bytes.clone())).await;
    let data = tmp.path().join("data");
    let (job, _) = recording(|p, c| {
        if p.phase == Phase::Downloading && p.done > 0 {
            c.cancel();
        }
    });
    let e = install_pinned(&pinned(port, &sha), Os::Linux, &data, &ok_verify, &job)
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert!(!data.join("chromium").exists() && !data.join("chromium.work").exists());
}

#[tokio::test]
async fn cancel_while_unpacking_and_in_the_self_test_install_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (bytes, sha) = fake_zip(tmp.path());
    let port = serve(move |_| Reply::Bytes(bytes.clone())).await;
    let data = tmp.path().join("data");
    let (job, seen) = recording(|p, c| {
        if p.phase == Phase::Unpacking && p.done > 0 {
            c.cancel();
        }
    });
    let e = install_pinned(&pinned(port, &sha), Os::Linux, &data, &ok_verify, &job)
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert!(!phases(&seen).contains(&Phase::SelfTest));
    assert!(!data.join("chromium").exists() && !data.join("chromium.work").exists());

    let (job, _) = recording(|p, c| {
        if p.phase == Phase::SelfTest {
            c.cancel();
        }
    });
    let e = install_pinned(&pinned(port, &sha), Os::Linux, &data, &ok_verify, &job)
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert!(!data.join("chromium").exists() && !data.join("chromium.work").exists());
}

#[tokio::test]
async fn cancel_while_the_server_does_not_answer_is_heard_within_a_moment() {
    let tmp = tempfile::tempdir().unwrap();
    let port = serve(|_| Reply::Hang).await;
    let (job, _) = recording(|_, _| {});
    let flag = job.cancel_flag().clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        flag.cancel();
    });
    let began = std::time::Instant::now();
    let e = install_pinned(
        &pinned(port, &"0".repeat(64)),
        Os::Linux,
        &tmp.path().join("data"),
        &ok_verify,
        &job,
    )
    .await
    .unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert!(began.elapsed() < Duration::from_secs(5));
}
