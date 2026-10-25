//! The only outbound HTTP client (ADR 0001): downloads engines and models the user asked for, and Hugging Face
//! search/listing. Every URL is a [`DownloadUrl`] (https, allow-listed host), every redirect is checked against the
//! same list, offline mode refuses before any socket opens, and request handlers have no way to reach this module
//! (they get the library/engine managers, which call it from their own tasks).

use futures_util::StreamExt;
use pitwall_core::privacy::{check_online, DownloadUrl, EgressError};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

/// Why a download failed.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    /// Not allowed (offline, wrong host).
    #[error("{0}")]
    Egress(#[from] EgressError),
    /// The network failed.
    #[error("the download failed: {0}. Check the connection and retry; the part already downloaded is kept.")]
    Network(String),
    /// The server answered with an error.
    #[error("the server answered {0}{1}")]
    Status(u16, &'static str),
    /// The file on disk could not be written.
    #[error("cannot write the file: {0}")]
    Io(String),
    /// The checksum differs.
    #[error("the downloaded file does not match its checksum (expected {expected}, got {actual}); it was discarded")]
    Checksum {
        /// Expected hex.
        expected: String,
        /// Actual hex.
        actual: String,
    },
    /// Cancelled by the user.
    #[error("the download was cancelled")]
    Cancelled,
}

/// A cooperative cancel flag shared with the UI.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    /// Ask to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    /// Whether stop was asked.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The outbound client.
#[derive(Clone)]
pub struct Egress {
    client: reqwest::Client,
    offline: Arc<AtomicBool>,
}

fn hint(status: u16) -> &'static str {
    match status {
        401 | 403 => ": the file may be gated; accept its licence on huggingface.co first",
        404 => ": the file was not found",
        429 => ": rate limited, retry later",
        _ => "",
    }
}

impl Egress {
    /// A client; `offline` is shared with the settings so toggling takes effect immediately.
    #[allow(clippy::disallowed_methods)] // the single allowed place (ADR 0001)
    pub fn new(offline: Arc<AtomicBool>) -> Egress {
        let policy = reqwest::redirect::Policy::custom(|a| {
            if a.previous().len() >= 6 {
                return a.error("too many redirects");
            }
            match DownloadUrl::parse(a.url().as_str()) {
                Ok(_) => a.follow(),
                Err(e) => a.error(e.to_string()),
            }
        });
        let client = reqwest::Client::builder()
            .redirect(policy)
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .user_agent(concat!("Pitwall/", env!("CARGO_PKG_VERSION")))
            .https_only(true)
            .no_proxy()
            .build()
            .unwrap_or_default();
        Egress { client, offline }
    }

    /// Whether offline mode is on.
    pub fn offline(&self) -> bool {
        self.offline.load(Ordering::SeqCst)
    }

    /// GET a small JSON document (Hugging Face API), at most `max` bytes.
    pub async fn get_json(&self, url: &DownloadUrl, max: usize) -> Result<serde_json::Value, DownloadError> {
        check_online(self.offline())?;
        let r = self
            .client
            .get(url.as_str())
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| DownloadError::Network(e.to_string()))?;
        if !r.status().is_success() {
            return Err(DownloadError::Status(r.status().as_u16(), hint(r.status().as_u16())));
        }
        let mut body = Vec::new();
        let mut s = r.bytes_stream();
        while let Some(c) = s.next().await {
            let c = c.map_err(|e| DownloadError::Network(e.to_string()))?;
            if body.len() + c.len() > max {
                return Err(DownloadError::Network(format!("the answer is larger than {max} bytes")));
            }
            body.extend_from_slice(&c);
        }
        serde_json::from_slice(&body).map_err(|e| DownloadError::Network(format!("not JSON: {e}")))
    }

    /// Download `url` into `dest`, resuming a partial file at `dest` (the caller passes the `.part` path), calling
    /// `progress(done, total)` as bytes arrive, and verifying `sha256` (when known) before returning the hex digest.
    pub async fn download(
        &self,
        url: &DownloadUrl,
        dest: &Path,
        sha256: Option<&str>,
        cancel: &CancelFlag,
        mut progress: impl FnMut(u64, Option<u64>) + Send,
    ) -> Result<String, DownloadError> {
        check_online(self.offline())?;
        if let Some(d) = dest.parent() {
            tokio::fs::create_dir_all(d).await.map_err(|e| DownloadError::Io(e.to_string()))?;
        }
        let have = tokio::fs::metadata(dest).await.map(|m| m.len()).unwrap_or(0);
        let mut req = self.client.get(url.as_str());
        if have > 0 {
            req = req.header("range", format!("bytes={have}-"));
        }
        let r = req.send().await.map_err(|e| DownloadError::Network(e.to_string()))?;
        let status = r.status().as_u16();
        let (mut done, total, append) = match status {
            206 => (have, r.content_length().map(|l| l + have), true),
            200 => (0, r.content_length(), false),
            // The partial file already covers everything: just verify it.
            416 => (have, Some(have), true),
            s => return Err(DownloadError::Status(s, hint(s))),
        };
        if status != 416 {
            let mut f = tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(!append)
                .open(dest)
                .await
                .map_err(|e| DownloadError::Io(e.to_string()))?;
            if append {
                f.seek(std::io::SeekFrom::End(0))
                    .await
                    .map_err(|e| DownloadError::Io(e.to_string()))?;
            }
            let mut s = r.bytes_stream();
            while let Some(c) = s.next().await {
                if cancel.is_cancelled() {
                    let _ = f.flush().await;
                    return Err(DownloadError::Cancelled);
                }
                let c = c.map_err(|e| DownloadError::Network(e.to_string()))?;
                f.write_all(&c).await.map_err(|e| DownloadError::Io(e.to_string()))?;
                done += c.len() as u64;
                progress(done, total);
            }
            f.flush().await.map_err(|e| DownloadError::Io(e.to_string()))?;
        }
        if total.is_some_and(|t| t != done) {
            return Err(DownloadError::Network(format!(
                "the connection closed after {done} of {} bytes",
                total.unwrap_or(0)
            )));
        }
        let actual = sha256_file(dest).await.map_err(|e| DownloadError::Io(e.to_string()))?;
        if let Some(expected) = sha256 {
            if !expected.eq_ignore_ascii_case(&actual) {
                let _ = tokio::fs::remove_file(dest).await;
                return Err(DownloadError::Checksum {
                    expected: expected.to_string(),
                    actual,
                });
            }
        }
        Ok(actual)
    }
}

/// SHA-256 of a file, hex, read in chunks off the async threads.
pub async fn sha256_file(path: &Path) -> std::io::Result<String> {
    let p = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut f = std::fs::File::open(p)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
    })
    .await
    .map_err(std::io::Error::other)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn offline_refuses_before_any_connection() {
        let e = Egress::new(Arc::new(AtomicBool::new(true)));
        let u = DownloadUrl::parse("https://huggingface.co/x").unwrap();
        assert!(matches!(e.get_json(&u, 10).await, Err(DownloadError::Egress(EgressError::Offline))));
        let d = tempfile::tempdir().unwrap();
        let r = e.download(&u, &d.path().join("f"), None, &CancelFlag::default(), |_, _| {}).await;
        assert!(matches!(r, Err(DownloadError::Egress(EgressError::Offline))));
    }

    #[tokio::test]
    async fn checksum_of_a_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).await.unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn cancel_flag_and_hints() {
        let c = CancelFlag::default();
        assert!(!c.is_cancelled());
        c.clone().cancel();
        assert!(c.is_cancelled());
        assert!(hint(403).contains("gated") && hint(500).is_empty());
    }
}
