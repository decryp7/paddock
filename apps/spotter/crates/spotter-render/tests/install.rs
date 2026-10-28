//! Installing the pinned browser: the archive comes through the shell's downloader (`paddock-download`) from a local
//! server (no network), is verified against the pinned checksum while it is written, unpacked safely and self-tested.
//! Phases, errors and Cancel in every phase.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use paddock_download::{Config, Downloader, HttpConfig};
use paddock_jobs::{EventSink, JobContext, JobRegistry, JobSnapshot, JobState};
use spotter_core::job::{Cancel, Job, Phase, Progress, Unit};
use spotter_render::discover::Os;
use spotter_render::install::{extract, install_pinned, sha256_file};
use spotter_render::manifest::Pinned;
use spotter_render::Error;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::{serve, Reply};

const BIG: usize = 4 << 20;

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

/// The phases of the shell job, in the order they were announced.
#[derive(Default)]
struct Phases(Mutex<Vec<String>>);
impl EventSink for Phases {
    fn emit(&self, s: &JobSnapshot) {
        if let JobState::Running { phase: Some(p) } = &s.state {
            let mut seen = self.0.lock().unwrap();
            if seen.last() != Some(&p.phase.id) {
                seen.push(p.phase.id.clone());
            }
        }
    }
}

/// The app's downloader with short waits, and a shell job to download in.
fn shell() -> (Arc<Downloader>, JobContext, Arc<Phases>, Arc<JobRegistry>) {
    let config = Config {
        attempts: 3,
        backoff: Duration::from_millis(5),
        max_wait: Duration::from_secs(2),
        max_throttles: 3,
        stall_timeout: Duration::from_secs(10),
        disk_reserve: 0,
        ..Config::default()
    };
    let downloader = Arc::new(Downloader::http(HttpConfig::default()).unwrap().with_config(config));
    let phases = Arc::new(Phases::default());
    let registry = Arc::new(JobRegistry::new(phases.clone()));
    let ctx = registry.register("sp.install-renderer", "Installing the browser", None);
    ctx.reporter().start();
    (downloader, ctx, phases, registry)
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

fn phases_of(seen: &Seen) -> Vec<Phase> {
    let mut out: Vec<Phase> = Vec::new();
    for p in seen.lock().unwrap().iter() {
        if out.last() != Some(&p.phase) {
            out.push(p.phase);
        }
    }
    out
}

#[tokio::test]
async fn installs_a_verified_archive_into_the_data_dir_with_labelled_phases() {
    let tmp = tempfile::tempdir().unwrap();
    let (bytes, sha) = fake_zip(tmp.path());
    let port = serve(move |_| Reply::Bytes(bytes.clone())).await;
    let data = tmp.path().join("data");
    let (dl, ctx, shell_phases, _registry) = shell();
    let (job, seen) = recording(|_, _| {});
    let b = install_pinned(&pinned(port, &sha), Os::Linux, &data, &ok_verify, &dl, &ctx, &job)
        .await
        .unwrap();
    assert!(b.path.starts_with(data.join("chromium")) && b.path.is_file(), "{:?}", b.path);
    assert!(!data.join("chromium.work").exists() && !data.join("chromium-1.zip").exists() && !data.join("chromium-1.zip.part").exists());
    assert!(spotter_render::install::locate_executable(&data.join("chromium"), Os::Linux).is_some());
    let net = shell_phases.0.lock().unwrap().clone();
    assert_eq!(
        net,
        vec!["connect", "download", "verify"],
        "the download's own phases, none going back"
    );
    assert_eq!(phases_of(&seen), vec![Phase::Unpacking, Phase::SelfTest]);
    let un: Vec<_> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|p| p.phase == Phase::Unpacking)
        .cloned()
        .collect();
    assert!(un.len() >= 3 && un.windows(2).all(|w| w[0].done <= w[1].done) && un.iter().all(|p| p.unit == Unit::Bytes));
    assert_eq!(un.last().unwrap().done, un.last().unwrap().total.unwrap());
}

#[tokio::test]
async fn a_throttled_host_is_waited_out() {
    let tmp = tempfile::tempdir().unwrap();
    let (bytes, sha) = fake_zip(tmp.path());
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    let port = serve(move |_| {
        if counter.fetch_add(1, Ordering::SeqCst) < 2 {
            Reply::Status(429)
        } else {
            Reply::Bytes(bytes.clone())
        }
    })
    .await;
    let (dl, ctx, _, _registry) = shell();
    install_pinned(
        &pinned(port, &sha),
        Os::Linux,
        &tmp.path().join("data"),
        &ok_verify,
        &dl,
        &ctx,
        &Job::none(),
    )
    .await
    .unwrap();
    assert!(asked.load(Ordering::SeqCst) > 2);
}

#[tokio::test]
async fn a_wrong_checksum_is_refused_and_nothing_is_installed_or_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let (bytes, _) = fake_zip(tmp.path());
    let port = serve(move |_| Reply::Bytes(bytes.clone())).await;
    let data = tmp.path().join("data");
    let (dl, ctx, _, _registry) = shell();
    let e = install_pinned(
        &pinned(port, &"0".repeat(64)),
        Os::Linux,
        &data,
        &ok_verify,
        &dl,
        &ctx,
        &Job::none(),
    )
    .await
    .unwrap_err();
    assert!(e.to_string().contains("checksum"), "{e}");
    assert!(!data.join("chromium").exists() && !data.join("chromium.work").exists());
    assert!(
        !data.join("chromium-1.zip").exists() && !data.join("chromium-1.zip.part").exists(),
        "a bad file is not kept"
    );
}

#[tokio::test]
async fn http_errors_failed_self_tests_and_archives_without_a_browser_install_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let (dl, ctx, _, _registry) = shell();
    let port = serve(|_| Reply::Status(404)).await;
    let e = install_pinned(
        &pinned(port, &"0".repeat(64)),
        Os::Linux,
        &data,
        &ok_verify,
        &dl,
        &ctx,
        &Job::none(),
    )
    .await
    .unwrap_err();
    assert!(e.to_string().contains("download failed"), "{e}");

    let (bytes, sha) = fake_zip(tmp.path());
    let port = serve(move |_| Reply::Bytes(bytes.clone())).await;
    let (dl, ctx, _, _registry) = shell();
    let bad = |_: &Path| -> spotter_render::Result<String> { Err(spotter_render::Error::Install("does not run".into())) };
    let e = install_pinned(&pinned(port, &sha), Os::Linux, &data, &bad, &dl, &ctx, &Job::none())
        .await
        .unwrap_err();
    assert!(e.to_string().contains("does not run"));
    assert!(!data.join("chromium").exists());

    let (dl, ctx, _, _registry) = shell();
    let e = install_pinned(&pinned(port, &sha), Os::Windows, &data, &ok_verify, &dl, &ctx, &Job::none())
        .await
        .unwrap_err();
    assert!(e.to_string().contains("does not contain a browser"));
}

#[tokio::test]
async fn cancel_while_unpacking_and_in_the_self_test_install_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (bytes, sha) = fake_zip(tmp.path());
    let port = serve(move |_| Reply::Bytes(bytes.clone())).await;
    let data = tmp.path().join("data");
    let (dl, ctx, _, _registry) = shell();
    let (job, seen) = recording(|p, c| {
        if p.phase == Phase::Unpacking && p.done > 0 {
            c.cancel();
        }
    });
    let e = install_pinned(&pinned(port, &sha), Os::Linux, &data, &ok_verify, &dl, &ctx, &job)
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert!(!phases_of(&seen).contains(&Phase::SelfTest));
    assert!(!data.join("chromium").exists() && !data.join("chromium.work").exists());

    let (dl, ctx, _, _registry) = shell();
    let (job, _) = recording(|p, c| {
        if p.phase == Phase::SelfTest {
            c.cancel();
        }
    });
    let e = install_pinned(&pinned(port, &sha), Os::Linux, &data, &ok_verify, &dl, &ctx, &job)
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert!(!data.join("chromium").exists() && !data.join("chromium.work").exists());
}

#[tokio::test]
async fn cancel_while_the_server_does_not_answer_is_heard_within_a_moment() {
    let tmp = tempfile::tempdir().unwrap();
    let port = serve(|_| Reply::Hang).await;
    let (dl, ctx, _, _registry) = shell();
    let token = ctx.cancel_token().clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        token.cancel();
    });
    let began = std::time::Instant::now();
    let e = install_pinned(
        &pinned(port, &"0".repeat(64)),
        Os::Linux,
        &tmp.path().join("data"),
        &ok_verify,
        &dl,
        &ctx,
        &Job::none(),
    )
    .await
    .unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert!(began.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn unpacking_needs_no_checksum_the_download_already_took_it() {
    let tmp = tempfile::tempdir().unwrap();
    let other = tmp.path().join("other.zip");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&other).unwrap());
    z.start_file("x/chrome", zip::write::SimpleFileOptions::default()).unwrap();
    z.write_all(b"x").unwrap();
    z.finish().unwrap();
    extract(&other, &tmp.path().join("out"), &Job::none()).unwrap();
    assert!(tmp.path().join("out/x/chrome").is_file());
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
