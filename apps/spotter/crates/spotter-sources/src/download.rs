//! Downloading or copying a release into a folder. A release at an http(s) address is fetched by `paddock-download`
//! (rule 15): segmented Range downloads with per-host caps, 429 back-off, resume of a kept `<name>.part`, one SHA-256
//! computed while writing and checked against the publisher's digest (or a `.sha256` file next to the release), the
//! free-space check and Cancel in every phase, reported as phases of the shell's job. A release on a drive or share is
//! copied here with the same `.part`, resume and checksum rules. A file that fails verification is deleted, never left
//! looking finished.

use crate::http::{read_bounded, Http};
use paddock_download::{DownloadError, Downloader, Integrity, Request};
use paddock_jobs::{JobContext, Phase as ShellPhase};
use spotter_core::action::{file_name, resume_offset};
use spotter_core::checksum::{find_in_checksum_file, Hasher, Sha256Digest};
use spotter_core::job::{Job, Phase, Unit, CANCELLED};
use spotter_core::limits::MAX_CHECKSUM_FILE;
use spotter_core::release::Release;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

/// Everything a download needs besides the release: the app's one downloader (its host caps hold across downloads), the
/// metadata client for the checksum file, and the shell job the network phases report to.
pub struct Fetch<'a> {
    /// The app's downloader.
    pub downloader: &'a Arc<Downloader>,
    /// Small requests (the `.sha256` file).
    pub http: &'a Http,
    /// The shell job.
    pub ctx: &'a JobContext,
}

/// Read and write chunk for local files and shares.
const IO_BUFFER: usize = 1 << 20;
/// How often a stalled network wait looks at the cancel flag.
const CANCEL_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// Largest file Spotter downloads (64 GiB).
pub const MAX_DOWNLOAD: u64 = 64 << 30;

/// A finished download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    /// Where the file is.
    pub path: PathBuf,
    /// Its size.
    pub size: u64,
    /// Whether a checksum was verified.
    pub verified: bool,
}

fn is_http(location: &str) -> bool {
    let l = location.to_ascii_lowercase();
    l.starts_with("https://") || l.starts_with("http://")
}

/// The message of a download that ended without a file.
fn describe(e: DownloadError, name: &str) -> String {
    match e {
        DownloadError::Cancelled(_) => CANCELLED.to_string(),
        DownloadError::ChecksumMismatch { expected, actual } => {
            format!("{name} does not match the publisher's checksum (expected {expected}, got {actual}); the file was deleted.")
        }
        DownloadError::TooLarge { .. } => format!("{name} is larger than {} GiB.", MAX_DOWNLOAD >> 30),
        DownloadError::Network { .. } | DownloadError::Stalled { .. } | DownloadError::Throttled { .. } => {
            format!("The download of {name} was interrupted ({e}); it resumes next time.")
        }
        other => other.to_string(),
    }
}

/// Downloads (URL) or copies (path) `release` into `folder`, reporting to the shell job in `fetch` (and to `job` for a
/// copy) and stopping when either is cancelled; the `.part` file stays so the next run resumes it. The checksum is
/// computed while writing; nothing is read a second time.
pub async fn download_release(fetch: &Fetch<'_>, release: &Release, folder: &Path, verify: bool, job: &Job) -> Result<Downloaded, String> {
    let location = release.location.as_str();
    let remote = is_http(location);
    if !remote && !Path::new(location).is_absolute() {
        return Err(format!(
            "{location} is not a file or an http(s) address, so it cannot be downloaded."
        ));
    }
    tokio::fs::create_dir_all(folder)
        .await
        .map_err(|e| format!("The folder {} could not be created: {e}", folder.display()))?;
    let name = file_name(release);
    let target = folder.join(&name);
    let expected = match release.sha256 {
        Some(d) => Some(d),
        None if verify => {
            if remote {
                fetch.ctx.phase(ShellPhase::discovering("checksum", "Fetching the checksum"));
            } else {
                job.report(Phase::FetchingChecksum, Unit::Count, 0, None);
            }
            or_cancel(job, sidecar(fetch.http, location, &name)).await?
        }
        None => None,
    };
    stopped(job)?;
    if remote {
        return fetch_remote(fetch, location, &target, &name, expected).await;
    }
    let part = folder.join(format!("{name}.part"));
    let mut hasher = Hasher::new();
    let size = copy(Path::new(location), &part, &mut hasher, job).await?;
    let digest = hasher.finish();
    let verified = match expected {
        Some(want) if want != digest => {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(format!(
                "{name} does not match the publisher's checksum (expected {want}, got {digest}); the file was deleted."
            ));
        }
        Some(_) => true,
        None => false,
    };
    let _ = tokio::fs::remove_file(&target).await;
    tokio::fs::rename(&part, &target)
        .await
        .map_err(|e| format!("{} could not be put in place: {e}", target.display()))?;
    Ok(Downloaded {
        path: target,
        size,
        verified,
    })
}

/// The network half: one `paddock-download` request on a blocking thread (it supervises its transfer threads and notices
/// a cancel of the shell job within a fraction of a second, in every phase).
async fn fetch_remote(
    fetch: &Fetch<'_>,
    url: &str,
    target: &Path,
    name: &str,
    expected: Option<Sha256Digest>,
) -> Result<Downloaded, String> {
    let integrity = match expected {
        Some(d) => Integrity::Sha256(paddock_download::Sha256Digest::from_hex(&d.to_string()).map_err(|e| e.to_string())?),
        None => Integrity::Unpinned,
    };
    let request = Request::new(url, target, integrity)
        .map_err(|e| e.to_string())?
        .max_bytes(MAX_DOWNLOAD);
    let (downloader, ctx) = (fetch.downloader.clone(), fetch.ctx.clone());
    let done = tokio::task::spawn_blocking(move || downloader.download(&ctx, &request))
        .await
        .map_err(|e| format!("The download stopped unexpectedly: {e}"))?
        .map_err(|e| describe(e, name))?;
    Ok(Downloaded {
        path: done.path,
        size: done.bytes,
        verified: expected.is_some(),
    })
}

/// `Err(CANCELLED)` once the job was cancelled.
fn stopped(job: &Job) -> Result<(), String> {
    if job.is_cancelled() {
        Err(CANCELLED.to_string())
    } else {
        Ok(())
    }
}

/// Awaits `work`, giving up with [`CANCELLED`] soon after the job is cancelled (a stalled connection must not hold Cancel).
async fn or_cancel<T>(job: &Job, work: impl std::future::Future<Output = T>) -> Result<T, String> {
    tokio::pin!(work);
    loop {
        tokio::select! {
            out = &mut work => return Ok(out),
            () = tokio::time::sleep(CANCEL_POLL) => stopped(job)?,
        }
    }
}

/// The digest in a `.sha256` file next to the release, when there is one.
async fn sidecar(http: &Http, location: &str, name: &str) -> Option<Sha256Digest> {
    let sum_location = format!("{}.sha256", location.split(['?', '#']).next().unwrap_or(location));
    let text = if is_http(location) {
        let response = http
            .client()
            .get(&sum_location)
            .timeout(crate::http::REQUEST_TIMEOUT)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        read_bounded(response, MAX_CHECKSUM_FILE, &sum_location).await.ok()?
    } else {
        let meta = tokio::fs::metadata(&sum_location).await.ok()?;
        if meta.len() > MAX_CHECKSUM_FILE as u64 {
            return None;
        }
        tokio::fs::read(&sum_location).await.ok()?
    };
    find_in_checksum_file(&String::from_utf8_lossy(&text), name)
}

/// The size of the part left by an earlier run (0 when there is none).
async fn part_len(part: &Path) -> u64 {
    tokio::fs::metadata(part).await.map(|m| m.len()).unwrap_or(0)
}

/// Feeds the first `len` bytes of `part` to `hasher`, reporting [`Phase::CheckingExisting`]. SHA-256 cannot be
/// continued from a saved state, so a resumed download has to read what it already has once; it says so.
async fn hash_existing(part: &Path, len: u64, hasher: &mut Hasher, job: &Job) -> Result<u64, String> {
    if len == 0 {
        return Ok(0);
    }
    let mut f = match tokio::fs::File::open(part).await {
        Ok(f) => f,
        Err(_) => return Ok(0),
    };
    let mut buf = vec![0u8; IO_BUFFER];
    let mut n = 0u64;
    while n < len {
        stopped(job)?;
        let want = usize::try_from((len - n).min(IO_BUFFER as u64)).unwrap_or(IO_BUFFER);
        let read = f
            .read(&mut buf[..want])
            .await
            .map_err(|e| format!("{} could not be read: {e}", part.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        n += read as u64;
        job.report(Phase::CheckingExisting, Unit::Bytes, n, Some(len));
    }
    job.report_done(Phase::CheckingExisting, Unit::Bytes, n, Some(len));
    stopped(job)?;
    Ok(n)
}

async fn open_part(part: &Path, offset: u64) -> Result<tokio::fs::File, String> {
    let mut f = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(offset == 0)
        .open(part)
        .await
        .map_err(|e| format!("{} could not be written: {e}", part.display()))?;
    f.seek(std::io::SeekFrom::Start(offset)).await.map_err(|e| e.to_string())?;
    Ok(f)
}

async fn copy(source: &Path, part: &Path, hasher: &mut Hasher, job: &Job) -> Result<u64, String> {
    let meta = tokio::fs::metadata(source)
        .await
        .map_err(|e| format!("{} could not be read: {e}", source.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is a folder; only files can be copied.", source.display()));
    }
    let total = meta.len();
    if total > MAX_DOWNLOAD {
        return Err(format!("{} is larger than {} GiB.", source.display(), MAX_DOWNLOAD >> 30));
    }
    let existing = part_len(part).await;
    let mut have = hash_existing(part, existing, hasher, job).await?;
    if resume_offset(have, Some(total)) == 0 && have > 0 {
        *hasher = Hasher::new();
        have = 0;
    }
    let mut src = tokio::fs::File::open(source)
        .await
        .map_err(|e| format!("{} could not be read: {e}", source.display()))?;
    src.seek(std::io::SeekFrom::Start(have)).await.map_err(|e| e.to_string())?;
    let mut file = open_part(part, have).await?;
    let mut buf = vec![0u8; IO_BUFFER];
    let mut written = have;
    job.report_done(Phase::Copying, Unit::Bytes, written, Some(total));
    loop {
        stopped(job)?;
        let n = src
            .read(&mut buf)
            .await
            .map_err(|e| format!("Copying {} was interrupted ({e}); it resumes next time.", source.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])
            .await
            .map_err(|e| format!("{} could not be written: {e}", part.display()))?;
        written += n as u64;
        job.report(Phase::Copying, Unit::Bytes, written, Some(total));
    }
    file.flush().await.map_err(|e| e.to_string())?;
    job.report_done(Phase::Copying, Unit::Bytes, written, Some(total));
    Ok(written)
}
