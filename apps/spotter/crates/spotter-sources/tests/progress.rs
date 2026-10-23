//! Progress and cancel of downloads and copies: bytes from the first one, one scale per phase, the checksum
//! computed while writing, no second read pass, and Cancel in every phase.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::checksum::Sha256Digest;
use spotter_core::job::{Cancel, Job, Phase, Progress, Unit, CANCELLED};
use spotter_core::release::Release;
use spotter_core::version::VersionScheme;
use spotter_sources::download::download_job;
use spotter_sources::http::Http;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const CHUNK: usize = 64 * 1024;
const CHUNKS: usize = 16;

fn body() -> Vec<u8> {
    (0..CHUNK * CHUNKS).map(|i| (i % 251) as u8).collect()
}

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

/// Serves `/f` slowly in chunks (so there is time to look and to cancel), honouring `Range`, and `/f.sha256`.
async fn serve() -> String {
    let data = Arc::new(body());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let data = data.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 2048];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = sock.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                let text = String::from_utf8_lossy(&buf).to_string();
                let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
                if path == "/f.sha256" {
                    let sum = format!("{}  f\n", Sha256Digest::of(&data));
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", sum.len());
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(sum.as_bytes()).await;
                    return;
                }
                let from: usize = text
                    .lines()
                    .find_map(|l| l.to_ascii_lowercase().strip_prefix("range: bytes=").map(str::to_string))
                    .and_then(|r| r.trim_end_matches('-').parse().ok())
                    .unwrap_or(0);
                let rest = &data[from..];
                let head = if from > 0 {
                    format!(
                        "HTTP/1.1 206 Partial\r\nContent-Range: bytes {from}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        data.len() - 1,
                        data.len(),
                        rest.len()
                    )
                } else {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", rest.len())
                };
                let _ = sock.write_all(head.as_bytes()).await;
                for piece in rest.chunks(CHUNK) {
                    if sock.write_all(piece).await.is_err() {
                        return;
                    }
                    let _ = sock.flush().await;
                    tokio::time::sleep(Duration::from_millis(15)).await;
                }
            });
        }
    });
    base
}

fn assert_increasing(seen: &Seen, phase: Phase) {
    let seen = seen.lock().unwrap();
    let mine: Vec<&Progress> = seen.iter().filter(|p| p.phase == phase).collect();
    assert!(mine.len() >= 3, "{phase:?} reported {} times", mine.len());
    assert!(mine.windows(2).all(|w| w[0].done <= w[1].done), "{phase:?} never goes back");
    assert!(mine.last().unwrap().done > mine[0].done, "{phase:?} advances");
    assert!(mine.iter().all(|p| p.unit == Unit::Bytes));
}

#[tokio::test]
async fn a_download_reports_bytes_from_the_first_one_and_hashes_while_writing() {
    let base = serve().await;
    let http = Http::new("test").unwrap();
    let dest = tempfile::tempdir().unwrap();
    let (job, seen) = recording(|_, _| {});
    let known = Sha256Digest::of(&body());
    let d = download_job(&http, &release(&format!("{base}/f"), Some(known)), dest.path(), true, &job)
        .await
        .unwrap();
    assert!(d.verified, "the digest computed while writing is the file's digest");
    assert_eq!(Sha256Digest::of(&std::fs::read(&d.path).unwrap()), known);
    // The very first report is already the download, at zero bytes, before the server has answered.
    let first = seen.lock().unwrap()[0].clone();
    assert_eq!((first.phase, first.done, first.unit), (Phase::Downloading, 0, Unit::Bytes));
    assert_increasing(&seen, Phase::Downloading);
    let last = seen.lock().unwrap().last().unwrap().clone();
    assert_eq!((last.done, last.total), (d.size, Some(d.size)));
    // No second pass over the data: with the digest known there is exactly one phase.
    assert_eq!(phases(&seen), vec![Phase::Downloading]);
}

#[tokio::test]
async fn looking_up_the_checksum_is_its_own_labelled_phase_before_the_download() {
    let base = serve().await;
    let dest = tempfile::tempdir().unwrap();
    let (job, seen) = recording(|_, _| {});
    let d = download_job(
        &Http::new("t").unwrap(),
        &release(&format!("{base}/f"), None),
        dest.path(),
        true,
        &job,
    )
    .await
    .unwrap();
    assert!(d.verified);
    assert_eq!(phases(&seen), vec![Phase::FetchingChecksum, Phase::Downloading]);
}

#[tokio::test]
async fn resuming_labels_the_check_of_the_part_and_never_restarts_a_bar() {
    let base = serve().await;
    let dest = tempfile::tempdir().unwrap();
    let half = body()[..CHUNK * 6].to_vec();
    std::fs::write(dest.path().join("f.part"), &half).unwrap();
    let (job, seen) = recording(|_, _| {});
    let known = Sha256Digest::of(&body());
    let d = download_job(
        &Http::new("t").unwrap(),
        &release(&format!("{base}/f"), Some(known)),
        dest.path(),
        true,
        &job,
    )
    .await
    .unwrap();
    assert!(d.verified);
    assert_eq!(phases(&seen), vec![Phase::CheckingExisting, Phase::Downloading]);
    let seen = seen.lock().unwrap();
    let check: Vec<_> = seen.iter().filter(|p| p.phase == Phase::CheckingExisting).collect();
    assert_eq!(
        check.last().unwrap().done,
        half.len() as u64,
        "the whole part was checked, with progress"
    );
    assert_eq!(check[0].total, Some(half.len() as u64));
    // The download's bar starts where the part ends and goes up to the full size.
    let dl: Vec<_> = seen.iter().filter(|p| p.phase == Phase::Downloading).collect();
    assert_eq!(dl[0].done, half.len() as u64);
    assert_eq!(dl.last().unwrap().done, body().len() as u64);
}

#[tokio::test]
async fn cancel_stops_a_download_midway_and_keeps_the_part_for_resuming() {
    let base = serve().await;
    let dest = tempfile::tempdir().unwrap();
    let (job, seen) = recording(|p, c| {
        if p.phase == Phase::Downloading && p.done >= (CHUNK * 3) as u64 {
            c.cancel();
        }
    });
    let e = download_job(
        &Http::new("t").unwrap(),
        &release(&format!("{base}/f"), None),
        dest.path(),
        false,
        &job,
    )
    .await
    .unwrap_err();
    assert_eq!(e, CANCELLED);
    assert!(!dest.path().join("f").exists(), "never looks finished");
    assert!(dest.path().join("f.part").exists());
    assert!(seen.lock().unwrap().last().unwrap().done < body().len() as u64);
}

#[tokio::test]
async fn cancel_in_the_checksum_lookup_and_in_the_check_of_the_part() {
    let base = serve().await;
    let dest = tempfile::tempdir().unwrap();
    let (job, _) = recording(|p, c| {
        if p.phase == Phase::FetchingChecksum {
            c.cancel();
        }
    });
    let e = download_job(
        &Http::new("t").unwrap(),
        &release(&format!("{base}/f"), None),
        dest.path(),
        true,
        &job,
    )
    .await
    .unwrap_err();
    assert_eq!(e, CANCELLED);

    std::fs::write(dest.path().join("f.part"), &body()[..CHUNK * 4]).unwrap();
    let (job, seen) = recording(|p, c| {
        if p.phase == Phase::CheckingExisting {
            c.cancel();
        }
    });
    let e = download_job(
        &Http::new("t").unwrap(),
        &release(&format!("{base}/f"), None),
        dest.path(),
        false,
        &job,
    )
    .await
    .unwrap_err();
    assert_eq!(e, CANCELLED);
    assert!(!seen
        .lock()
        .unwrap()
        .iter()
        .any(|p| p.phase == Phase::Downloading && p.done > CHUNK as u64 * 4));
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
    let d = download_job(
        &Http::new("t").unwrap(),
        &release(&file.to_string_lossy(), Some(known)),
        dest.path(),
        true,
        &job,
    )
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
    let e = download_job(
        &Http::new("t").unwrap(),
        &release(&file.to_string_lossy(), None),
        dest.path(),
        false,
        &job,
    )
    .await
    .unwrap_err();
    assert_eq!(e, CANCELLED);
}
