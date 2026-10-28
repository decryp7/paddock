//! Installing a pinned Chromium archive: download (size capped), SHA-256 check, safe extraction, locating the
//! executable and a self-test. Everything lands in the app data folder; nothing outside it is touched.

use crate::discover::{self, Browser, Flavor, Os};
use crate::manifest::{Manifest, Pinned};
use crate::{Error, Result};
use paddock_download::{DownloadError, Downloader, Integrity, Request, Sha256Digest};
use paddock_jobs::JobContext;
use sha2::{Digest, Sha256};
use spotter_core::job::{Job, Phase, Unit};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
/// `dest` and archives that expand beyond [`MAX_EXPANDED`] are refused. This reads the archive once more to hash it;
/// the installer does not (it takes the digest computed while downloading and calls [`extract`]).
pub fn extract_verified(archive: &Path, expected_sha256: &str, dest: &Path) -> Result<()> {
    let got = sha256_file(archive)?;
    if !got.eq_ignore_ascii_case(expected_sha256.trim()) {
        return Err(err("the download does not match its pinned checksum and was discarded"));
    }
    extract(archive, dest, &Job::none())
}

/// Extracts a zip into `dest` without hashing it, reporting [`Phase::Unpacking`] in bytes of the expanded size and
/// stopping with [`Error::Cancelled`] when `job` is cancelled. The size and path checks are the same as
/// [`extract_verified`]'s.
pub fn extract(archive: &Path, dest: &Path, job: &Job) -> Result<()> {
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
    job.report(Phase::Unpacking, Unit::Bytes, 0, Some(total));
    let mut done = 0u64;
    let mut buf = vec![0u8; 256 << 10];
    for i in 0..zip.len() {
        if job.is_cancelled() {
            return Err(Error::Cancelled);
        }
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
        if f.is_symlink() {
            // macOS builds link their frameworks (`Versions/Current`); a link is recreated only when it stays inside.
            let mut target = String::new();
            f.read_to_string(&mut target).map_err(|e| err(e.to_string()))?;
            link_inside(dest, &out, &target)?;
            continue;
        }
        let mut o = std::fs::File::create(&out).map_err(|e| err(e.to_string()))?;
        loop {
            let n = f.read(&mut buf).map_err(|e| err(e.to_string()))?;
            if n == 0 {
                break;
            }
            std::io::Write::write_all(&mut o, &buf[..n]).map_err(|e| err(e.to_string()))?;
            done += n as u64;
            job.report(Phase::Unpacking, Unit::Bytes, done, Some(total));
            if job.is_cancelled() {
                return Err(Error::Cancelled);
            }
        }
        #[cfg(unix)]
        if let Some(mode) = f.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode));
        }
    }
    job.report_done(Phase::Unpacking, Unit::Bytes, total, Some(total));
    Ok(())
}

/// Creates the symbolic link `link` -> `target` when `target` is relative and resolves inside `root`; anything else
/// (an absolute target, a path that climbs out of the tree) is refused like a zip-slip path.
fn link_inside(root: &Path, link: &Path, target: &str) -> Result<()> {
    use std::path::Component;
    let t = Path::new(target);
    if target.is_empty() || target.contains('\0') || t.is_absolute() {
        return Err(err("the archive contains an unsafe link"));
    }
    let mut at: Vec<_> = link
        .parent()
        .unwrap_or(root)
        .strip_prefix(root)
        .map_err(|_| err("the archive contains an unsafe link"))?
        .components()
        .collect();
    for c in t.components() {
        match c {
            Component::Normal(_) => at.push(c),
            Component::ParentDir => {
                at.pop().ok_or_else(|| err("the archive contains a link that leaves its folder"))?;
            }
            Component::CurDir => {}
            _ => return Err(err("the archive contains an unsafe link")),
        }
    }
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(link);
        std::os::unix::fs::symlink(target, link).map_err(|e| err(e.to_string()))?;
    }
    #[cfg(not(unix))]
    let _ = link;
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

/// The Spotter error for a download that ended without a file.
fn download_error(e: DownloadError) -> Error {
    match e {
        DownloadError::Cancelled(_) => Error::Cancelled,
        DownloadError::ChecksumMismatch { .. } => err("the download does not match its pinned checksum and was discarded"),
        DownloadError::TooLarge { .. } => err("the download is larger than the allowed size"),
        other => err(format!("the download failed: {other}")),
    }
}

/// Downloads the pinned build for this platform from the embedded manifest into `data_dir` and returns the browser.
/// `downloader` is the app's one (its per-host caps hold across downloads); `ctx` is the shell job the download reports
/// its phases to; `job` carries the unpacking and the self-test.
pub async fn install(data_dir: &Path, downloader: &Arc<Downloader>, ctx: &JobContext, job: &Job) -> Result<Browser> {
    let pinned = Manifest::embedded().pinned(&crate::manifest::platform_key())?;
    install_pinned(&pinned, Os::CURRENT, data_dir, &self_test, downloader, ctx, job).await
}

/// [`install`] for an explicit pinned entry, OS and self-test (tests use a local server and a fake archive). Phases, each
/// on its own scale: connecting, resuming a kept part, downloading (segmented, hashed while written, by
/// `paddock-download`), verifying, unpacking (bytes of the expanded size, no second hash) and testing. The archive's
/// `.part` stays in the data folder when the install is cancelled or the connection drops, so the next run resumes it; the
/// old install stays until the new one verified, then it is replaced.
pub async fn install_pinned(
    pinned: &Pinned,
    os: Os,
    data_dir: &Path,
    verify: &(dyn Fn(&Path) -> Result<String> + Sync),
    downloader: &Arc<Downloader>,
    ctx: &JobContext,
    job: &Job,
) -> Result<Browser> {
    let final_dir = discover::downloaded_dir(data_dir);
    let work = data_dir.join("chromium.work");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| err(format!("the install folder could not be created: {e}")))?;
    // Outside the work folder, which is wiped: a kept `.part` is what a later run resumes.
    let archive = data_dir.join(format!("chromium-{}.zip", pinned.version));
    let result = async {
        let digest = Sha256Digest::from_hex(&pinned.sha256).map_err(download_error)?;
        let request = Request::new(&pinned.url, &archive, Integrity::Sha256(digest))
            .map_err(download_error)?
            .max_bytes(MAX_DOWNLOAD);
        let (d, c) = (downloader.clone(), ctx.clone());
        tokio::task::spawn_blocking(move || d.download(&c, &request))
            .await
            .map_err(|e| err(e.to_string()))?
            .map_err(download_error)?;
        let (a, w, j) = (archive.clone(), work.join("tree"), job.clone());
        tokio::task::spawn_blocking(move || extract(&a, &w, &j))
            .await
            .map_err(|e| err(e.to_string()))??;
        let exe = locate_executable(&work.join("tree"), os).ok_or_else(|| err("the archive does not contain a browser"))?;
        job.report(Phase::SelfTest, Unit::Count, 0, None);
        if job.is_cancelled() {
            return Err(Error::Cancelled);
        }
        verify(&exe.path)?;
        job.report_done(Phase::SelfTest, Unit::Count, 1, Some(1));
        if job.is_cancelled() {
            return Err(Error::Cancelled);
        }
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

    #[cfg(unix)]
    #[test]
    fn links_inside_the_tree_are_recreated_and_links_out_of_it_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let make = |name: &str, target: &str| {
            let a = d.path().join(format!("{name}.zip"));
            let mut z = zip::ZipWriter::new(std::fs::File::create(&a).unwrap());
            z.start_file("app/Versions/1/lib", zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(b"x").unwrap();
            z.add_symlink("app/Versions/Current", target, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.finish().unwrap();
            a
        };
        let ok = make("ok", "1");
        extract_verified(&ok, &sha256_file(&ok).unwrap(), &d.path().join("out")).unwrap();
        let link = d.path().join("out/app/Versions/Current");
        assert!(link.is_symlink());
        assert!(link.join("lib").is_file(), "the link resolves inside the tree");
        for (i, bad) in ["/etc", "../../../..", "../../../../etc"].iter().enumerate() {
            let a = make(&format!("bad{i}"), bad);
            let out = d.path().join(format!("out{i}"));
            assert!(extract_verified(&a, &sha256_file(&a).unwrap(), &out).is_err(), "{bad}");
        }
    }
}
