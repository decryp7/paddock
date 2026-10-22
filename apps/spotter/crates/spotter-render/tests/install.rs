//! The download-and-verify mechanism against a local server with a fake archive.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use spotter_render::discover::Os;
use spotter_render::install::{install_pinned, sha256_file};
use spotter_render::manifest::Pinned;
use std::io::Write;
use std::path::Path;
use support::{serve, Reply};

fn fake_zip(dir: &Path) -> (Vec<u8>, String) {
    let p = dir.join("fake.zip");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
    z.start_file("chrome-linux/chrome", zip::write::SimpleFileOptions::default())
        .unwrap();
    z.write_all(b"#!/bin/sh\necho Chromium 1\n").unwrap();
    z.finish().unwrap();
    (std::fs::read(&p).unwrap(), sha256_file(&p).unwrap())
}

fn ok_verify(_: &Path) -> spotter_render::Result<String> {
    Ok("Chromium 1".into())
}

fn pinned(port: u16, sha: &str) -> Pinned {
    Pinned {
        version: "1".into(),
        url: format!("http://127.0.0.1:{port}/a.zip"),
        sha256: sha.into(),
    }
}

#[tokio::test]
async fn installs_a_verified_archive_into_the_data_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let (bytes, sha) = fake_zip(tmp.path());
    let port = serve(move |_| Reply::Bytes(bytes.clone())).await;
    let data = tmp.path().join("data");
    let seen = std::sync::atomic::AtomicU64::new(0);
    let b = install_pinned(&pinned(port, &sha), Os::Linux, &data, &ok_verify, &|d, _| {
        seen.store(d, std::sync::atomic::Ordering::Relaxed)
    })
    .await
    .unwrap();
    assert!(b.path.starts_with(data.join("chromium")) && b.path.is_file(), "{:?}", b.path);
    assert!(seen.load(std::sync::atomic::Ordering::Relaxed) > 0);
    assert!(!data.join("chromium.work").exists());
    // And it is the downloaded location the status lookup reads (Linux layout).
    assert!(spotter_render::install::locate_executable(&data.join("chromium"), Os::Linux).is_some());
}

#[tokio::test]
async fn a_wrong_checksum_is_refused_and_nothing_is_installed() {
    let tmp = tempfile::tempdir().unwrap();
    let (bytes, _) = fake_zip(tmp.path());
    let port = serve(move |_| Reply::Bytes(bytes.clone())).await;
    let data = tmp.path().join("data");
    let e = install_pinned(&pinned(port, &"0".repeat(64)), Os::Linux, &data, &ok_verify, &|_, _| {})
        .await
        .unwrap_err();
    assert!(e.to_string().contains("checksum"), "{e}");
    assert!(!data.join("chromium").exists());
    assert!(!data.join("chromium.work").exists());
}

#[tokio::test]
async fn http_errors_and_failed_self_tests_install_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let port = serve(|_| Reply::Status(404)).await;
    let e = install_pinned(&pinned(port, &"0".repeat(64)), Os::Linux, &data, &ok_verify, &|_, _| {})
        .await
        .unwrap_err();
    assert!(e.to_string().contains("download failed"), "{e}");

    let (bytes, sha) = fake_zip(tmp.path());
    let port = serve(move |_| Reply::Bytes(bytes.clone())).await;
    let bad = |_: &Path| -> spotter_render::Result<String> { Err(spotter_render::Error::Install("does not run".into())) };
    let e = install_pinned(&pinned(port, &sha), Os::Linux, &data, &bad, &|_, _| {})
        .await
        .unwrap_err();
    assert!(e.to_string().contains("does not run"));
    assert!(!data.join("chromium").exists());

    // An archive without a browser inside.
    let e = install_pinned(&pinned(port, &sha), Os::Windows, &data, &ok_verify, &|_, _| {})
        .await
        .unwrap_err();
    assert!(e.to_string().contains("does not contain a browser"));
}

/// Needs a real Chrome for Testing archive for this platform: `SPOTTER_TEST_CHROMIUM_ZIP=/path/to.zip cargo test -p
/// spotter-render --test install -- --ignored`. Checks that the archive's hash matches the shipped manifest, that it
/// extracts (with its links) and that the browser answers `--version`.
#[tokio::test]
#[ignore = "needs a downloaded Chrome for Testing archive"]
async fn the_real_pinned_archive_extracts_and_runs() {
    let Ok(zip) = std::env::var("SPOTTER_TEST_CHROMIUM_ZIP") else {
        return;
    };
    let zip = Path::new(&zip);
    let manifest = spotter_render::manifest::Manifest::embedded();
    let pinned = manifest.pinned(&spotter_render::manifest::platform_key()).unwrap();
    assert_eq!(sha256_file(zip).unwrap(), pinned.sha256, "the archive is not the pinned build");
    let out = tempfile::tempdir().unwrap();
    spotter_render::install::extract_verified(zip, &pinned.sha256, &out.path().join("tree")).unwrap();
    let os = if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Mac
    } else {
        Os::Linux
    };
    let exe = spotter_render::install::locate_executable(&out.path().join("tree"), os).unwrap();
    let version = spotter_render::install::self_test(&exe.path).unwrap();
    assert!(version.contains(&manifest.version[..3]), "{version}");
}
