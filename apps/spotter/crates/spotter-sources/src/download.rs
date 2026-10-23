//! Downloading or copying a release into a folder: into `<name>.part`, resuming a part left by an interrupted run,
//! hashing as it goes, verifying the publisher's SHA-256 (or a `.sha256` file next to the release) and only then
//! renaming it into place. A file that fails verification is deleted, never left looking finished.

use crate::http::{read_bounded, transport, Http};
use reqwest::header::{CONTENT_RANGE, RANGE};
use reqwest::StatusCode;
use spotter_core::action::{file_name, resume_offset};
use spotter_core::checksum::{find_in_checksum_file, Hasher, Sha256Digest};
use spotter_core::limits::MAX_CHECKSUM_FILE;
use spotter_core::release::Release;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

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

/// Downloads (URL) or copies (path) `release` into `folder`.
pub async fn download(http: &Http, release: &Release, folder: &Path, verify: bool) -> Result<Downloaded, String> {
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
    let part = folder.join(format!("{name}.part"));
    let expected = match release.sha256 {
        Some(d) => Some(d),
        None if verify => sidecar(http, location, &name).await,
        None => None,
    };
    let mut hasher = Hasher::new();
    let size = if remote {
        fetch(http, location, &part, &mut hasher, MAX_DOWNLOAD).await?
    } else {
        copy(Path::new(location), &part, &mut hasher).await?
    };
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

async fn hash_existing(part: &Path, hasher: &mut Hasher) -> Result<u64, String> {
    let mut f = match tokio::fs::File::open(part).await {
        Ok(f) => f,
        Err(_) => return Ok(0),
    };
    let mut buf = vec![0u8; 1 << 16];
    let mut n = 0u64;
    loop {
        let read = f
            .read(&mut buf)
            .await
            .map_err(|e| format!("{} could not be read: {e}", part.display()))?;
        if read == 0 {
            return Ok(n);
        }
        hasher.update(&buf[..read]);
        n += read as u64;
    }
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

/// Downloads `url` into `part` (resuming it), refusing anything larger than `max` bytes, announced or streamed.
async fn fetch(http: &Http, url: &str, part: &Path, hasher: &mut Hasher, max: u64) -> Result<u64, String> {
    let have = hash_existing(part, hasher).await?;
    let mut req = http.client().get(url);
    if have > 0 {
        req = req.header(RANGE, format!("bytes={have}-"));
    }
    let mut response = req.send().await.map_err(|e| transport(e, url).to_string())?;
    let mut offset = have;
    match response.status() {
        StatusCode::PARTIAL_CONTENT if response.headers().get(CONTENT_RANGE).is_some() => {}
        s if s.is_success() => {
            // The server ignored the range: start over.
            *hasher = Hasher::new();
            offset = 0;
        }
        StatusCode::RANGE_NOT_SATISFIABLE => {
            // The part is no good for this file; start over without a range.
            *hasher = Hasher::new();
            offset = 0;
            response = http.client().get(url).send().await.map_err(|e| transport(e, url).to_string())?;
            if !response.status().is_success() {
                return Err(format!("{url} answered HTTP {}.", response.status().as_u16()));
            }
        }
        s => return Err(format!("{url} answered HTTP {}.", s.as_u16())),
    }
    let total = response.content_length().map(|n| n + offset);
    if total.is_some_and(|t| t > max) {
        return Err(format!("{url} is larger than {} GiB.", max >> 30));
    }
    let offset = resume_offset(offset, total);
    let mut file = open_part(part, offset).await?;
    let mut written = offset;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("The download of {url} was interrupted ({e}); it resumes next time."))?
    {
        written += chunk.len() as u64;
        if written > max {
            return Err(format!("{url} is larger than {} GiB.", max >> 30));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("{} could not be written: {e}", part.display()))?;
    }
    file.flush().await.map_err(|e| e.to_string())?;
    Ok(written)
}

async fn copy(source: &Path, part: &Path, hasher: &mut Hasher) -> Result<u64, String> {
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
    let mut have = hash_existing(part, hasher).await?;
    if resume_offset(have, Some(total)) == 0 && have > 0 {
        *hasher = Hasher::new();
        have = 0;
    }
    let mut src = tokio::fs::File::open(source)
        .await
        .map_err(|e| format!("{} could not be read: {e}", source.display()))?;
    src.seek(std::io::SeekFrom::Start(have)).await.map_err(|e| e.to_string())?;
    let mut file = open_part(part, have).await?;
    let mut buf = vec![0u8; 1 << 16];
    let mut written = have;
    loop {
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
    }
    file.flush().await.map_err(|e| e.to_string())?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// Serves one chunked answer (no length announced) on a local port and returns its address.
    fn chunked_server() -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/stream", l.local_addr().unwrap());
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n6\r\nabcdef\r\n6\r\nghijkl\r\n0\r\n\r\n",
                );
            }
        });
        url
    }

    #[tokio::test]
    async fn a_stream_that_outgrows_the_limit_is_refused_while_reading() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("a.part");
        let http = Http::new("test").unwrap();
        let e = fetch(&http, &chunked_server(), &part, &mut Hasher::new(), 8).await.unwrap_err();
        assert!(e.contains("larger than 0 GiB"), "{e}");
        let ok = fetch(&http, &chunked_server(), &dir.path().join("b.part"), &mut Hasher::new(), 64)
            .await
            .unwrap();
        assert_eq!(ok, 12);
    }
}
