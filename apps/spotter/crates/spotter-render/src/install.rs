//! Installing a pinned Chromium archive: download (size capped), SHA-256 check, safe extraction, locating the
//! executable and a self-test. Everything lands in the app data folder; nothing outside it is touched.

use crate::discover::{self, Browser, Flavor, Os};
use crate::manifest::{Manifest, Pinned};
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Largest total size an archive may expand to.
pub const MAX_EXPANDED: u64 = 3 << 30;
/// Largest download.
pub const MAX_DOWNLOAD: u64 = 1 << 30;

fn err(m: impl Into<String>) -> Error {
    Error::Install(m.into())
}

/// Lowercase hex SHA-256 of a file.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path).map_err(|e| err(format!("the download could not be read: {e}")))?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|e| err(e.to_string()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}

/// Verifies the archive against `expected` (hex, any case) and extracts it into `dest`; paths that would leave
/// `dest` and archives that expand beyond [`MAX_EXPANDED`] are refused.
pub fn extract_verified(archive: &Path, expected_sha256: &str, dest: &Path) -> Result<()> {
    let got = sha256_file(archive)?;
    if !got.eq_ignore_ascii_case(expected_sha256.trim()) {
        return Err(err("the download does not match its pinned checksum and was discarded"));
    }
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive).map_err(|e| err(e.to_string()))?)
        .map_err(|e| err(format!("the download is not a valid archive: {e}")))?;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(|e| err(e.to_string()))?;
        f.enclosed_name().ok_or_else(|| err("the archive contains an unsafe path"))?;
        total += f.size();
        if total > MAX_EXPANDED {
            return Err(err("the archive expands to more than the allowed size"));
        }
    }
    std::fs::create_dir_all(dest).map_err(|e| err(e.to_string()))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| err(e.to_string()))?;
        let rel = f.enclosed_name().ok_or_else(|| err("the archive contains an unsafe path"))?;
        let out = dest.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| err(e.to_string()))?;
            continue;
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(|e| err(e.to_string()))?;
        }
        let mut o = std::fs::File::create(&out).map_err(|e| err(e.to_string()))?;
        std::io::copy(&mut f, &mut o).map_err(|e| err(e.to_string()))?;
        #[cfg(unix)]
        if let Some(mode) = f.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode));
        }
    }
    Ok(())
}

/// The executable inside an extracted tree.
pub fn locate_executable(root: &Path, os: Os) -> Option<Browser> {
    let want: &[&str] = match os {
        Os::Windows => &["chrome.exe"],
        Os::Mac => &["Google Chrome for Testing", "Chromium"],
        Os::Linux => &["chrome"],
    };
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).ok()?.flatten() {
            let p: PathBuf = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| want.contains(&n)) && !p.to_string_lossy().contains("Helper") {
                return Some(Browser {
                    flavor: Flavor::Chromium,
                    path: p,
                });
            }
        }
    }
    None
}

/// Runs `<exe> --version` with a timeout and returns its output; the self-test of an installed browser.
pub fn self_test(exe: &Path) -> Result<String> {
    let mut child = std::process::Command::new(exe)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| err(format!("the browser does not run: {e}")))?;
    let end = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(s)) if s.success() => break,
            Ok(Some(_)) => return Err(err("the browser exited with an error")),
            Ok(None) if std::time::Instant::now() < end => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(err("the browser did not answer its self-test"));
            }
        }
    }
    let mut s = String::new();
    if let Some(mut o) = child.stdout.take() {
        let _ = o.read_to_string(&mut s);
    }
    Ok(s.trim().to_string())
}

/// Downloads `url` to `dest`, hashing as it goes. Redirects are not followed off the original scheme rules of
/// reqwest's default policy (limited); the size is capped by [`MAX_DOWNLOAD`]. `progress(done, total)` is called per chunk.
pub async fn download(url: &str, dest: &Path, progress: &(dyn Fn(u64, Option<u64>) + Sync)) -> Result<String> {
    use tokio::io::AsyncWriteExt;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(60 * 30))
        .build()
        .map_err(|e| err(e.to_string()))?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| err(format!("the download failed: {e}")))?
        .error_for_status()
        .map_err(|e| err(format!("the download failed: {e}")))?;
    let total = resp.content_length();
    if total.is_some_and(|t| t > MAX_DOWNLOAD) {
        return Err(err("the download is larger than the allowed size"));
    }
    let mut file = tokio::fs::File::create(dest).await.map_err(|e| err(e.to_string()))?;
    let mut hash = Sha256::new();
    let mut done = 0u64;
    while let Some(chunk) = resp.chunk().await.map_err(|e| err(format!("the download failed: {e}")))? {
        done += chunk.len() as u64;
        if done > MAX_DOWNLOAD {
            return Err(err("the download is larger than the allowed size"));
        }
        hash.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| err(e.to_string()))?;
        progress(done, total);
    }
    file.flush().await.map_err(|e| err(e.to_string()))?;
    Ok(hex(&hash.finalize()))
}

/// Downloads the pinned build for this platform from the embedded manifest into `data_dir` and returns the browser.
pub async fn install(data_dir: &Path, progress: &(dyn Fn(u64, Option<u64>) + Sync)) -> Result<Browser> {
    let pinned = Manifest::embedded().pinned(&crate::manifest::platform_key())?;
    install_pinned(&pinned, Os::CURRENT, data_dir, &self_test, progress).await
}

/// [`install`] for an explicit pinned entry, OS and self-test (tests use a local server and a fake archive).
/// The old install stays until the new one verified, then it is replaced.
pub async fn install_pinned(
    pinned: &Pinned,
    os: Os,
    data_dir: &Path,
    verify: &(dyn Fn(&Path) -> Result<String> + Sync),
    progress: &(dyn Fn(u64, Option<u64>) + Sync),
) -> Result<Browser> {
    let final_dir = discover::downloaded_dir(data_dir);
    let work = data_dir.join("chromium.work");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| err(format!("the install folder could not be created: {e}")))?;
    let result = async {
        let archive = work.join("download.zip");
        let got = download(&pinned.url, &archive, progress).await?;
        if !got.eq_ignore_ascii_case(&pinned.sha256) {
            return Err(err("the download does not match its pinned checksum and was discarded"));
        }
        let (a, s, w) = (archive.clone(), pinned.sha256.clone(), work.join("tree"));
        tokio::task::spawn_blocking(move || extract_verified(&a, &s, &w))
            .await
            .map_err(|e| err(e.to_string()))??;
        let exe = locate_executable(&work.join("tree"), os).ok_or_else(|| err("the archive does not contain a browser"))?;
        verify(&exe.path)?;
        let _ = std::fs::remove_file(&archive);
        let rel = exe.path.strip_prefix(work.join("tree")).map(Path::to_path_buf).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&final_dir);
        std::fs::rename(work.join("tree"), &final_dir).map_err(|e| err(format!("the browser could not be moved into place: {e}")))?;
        Ok(Browser {
            flavor: Flavor::Chromium,
            path: final_dir.join(rel),
        })
    }
    .await;
    let _ = std::fs::remove_dir_all(&work);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    pub(crate) fn zip_with(path: &Path, name: &str, body: &[u8]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(body).unwrap();
        z.finish().unwrap();
    }

    #[test]
    fn checksum_mismatch_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.zip");
        zip_with(&a, "chrome-linux/chrome", b"x");
        let e = extract_verified(&a, "00", &d.path().join("out")).unwrap_err();
        assert!(e.to_string().contains("checksum"));
        assert!(!d.path().join("out").exists());
    }

    #[test]
    fn verified_archive_extracts_and_is_located() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.zip");
        zip_with(&a, "chrome-linux/chrome", b"x");
        let sum = sha256_file(&a).unwrap().to_uppercase();
        extract_verified(&a, &sum, &d.path().join("out")).unwrap();
        let b = locate_executable(&d.path().join("out"), Os::Linux).unwrap();
        assert!(b.path.ends_with("chrome-linux/chrome"));
        assert!(locate_executable(&d.path().join("out"), Os::Windows).is_none());
    }

    #[test]
    fn zip_slip_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.zip");
        zip_with(&a, "../evil", b"x");
        let sum = sha256_file(&a).unwrap();
        assert!(extract_verified(&a, &sum, &d.path().join("out")).is_err());
        assert!(!d.path().join("evil").exists());
    }

    #[test]
    fn not_a_zip_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.zip");
        std::fs::write(&a, b"plain").unwrap();
        let sum = sha256_file(&a).unwrap();
        assert!(extract_verified(&a, &sum, &d.path().join("out"))
            .unwrap_err()
            .to_string()
            .contains("valid archive"));
    }

    #[test]
    fn self_test_fails_for_a_missing_program() {
        assert!(self_test(Path::new("/nonexistent/x")).is_err());
    }
}
