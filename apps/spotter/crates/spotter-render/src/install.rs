//! Installing a pinned Chromium archive: download (size capped), SHA-256 check, safe extraction, locating the
//! executable and a self-test. Everything lands in the app data folder; nothing outside it is touched.

use crate::discover::{self, Browser, Flavor, Os};
use crate::launch::{CappedSleep, Quit, SELF_TEST_CAP};
use crate::manifest::{Manifest, Pinned};
use crate::{Error, Result};
use paddock_download::{DownloadError, Downloader, Integrity, Request, Sha256Digest};
use paddock_jobs::JobContext;
use paddock_proc::StallCause;
use sha2::{Digest, Sha256};
use spotter_core::job::{Cancel, Job, Phase, Unit};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// The version of the browser at `exe`, for the status line and the install check: `<exe> --version` where that answers, judged on
/// the program's run state rather than a fixed time (see [`self_test_with`]), and ended after [`SELF_TEST_CAP`] at the most. On
/// Windows the browser is not started (Chrome and Edge start the browser for `--version`): the version comes from the
/// `<version>.manifest` beside the program.
pub fn self_test(exe: &Path) -> Result<String> {
    self_test_until(exe, Instant::now() + SELF_TEST_CAP, &Cancel::new())
}

/// [`self_test`] for the install job: also ends, with [`Error::Cancelled`], when `job` is cancelled.
pub fn self_test_job(exe: &Path, job: &Job) -> Result<String> {
    self_test_until(exe, Instant::now() + SELF_TEST_CAP, job.cancel_flag())
}

/// [`self_test`] with an explicit `deadline` (several self-tests share one) and `cancel`.
pub fn self_test_until(exe: &Path, deadline: Instant, cancel: &Cancel) -> Result<String> {
    let pace = CappedSleep::until(deadline).cancelled_by(cancel.clone());
    self_test_with(exe, paddock_proc::tree_cpu_time, pace, || cancel.is_cancelled())
}

/// How long the output of the self-test is waited for once the program has exited, and the most of it that is kept.
const OUTPUT_GRACE: Duration = Duration::from_secs(2);
const OUTPUT_MAX: usize = 4096;

/// The first [`OUTPUT_MAX`] bytes of a pipe, read on its own thread so a program that writes more than a pipe holds (or a
/// helper it started that keeps the pipe open) can never block the wait; the rest is read and dropped.
struct BoundedRead {
    kept: Arc<std::sync::Mutex<Vec<u8>>>,
    eof: std::sync::mpsc::Receiver<()>,
}

impl BoundedRead {
    fn start(mut pipe: impl Read + Send + 'static) -> BoundedRead {
        let kept = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (tx, eof) = std::sync::mpsc::channel();
        let shared = kept.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n @ 1..) = pipe.read(&mut buf) {
                let mut k = shared.lock().unwrap_or_else(|p| p.into_inner());
                let room = OUTPUT_MAX.saturating_sub(k.len()).min(n);
                k.extend_from_slice(&buf[..room]);
            }
            let _ = tx.send(());
        });
        BoundedRead { kept, eof }
    }

    /// What was read, once the pipe closed or `grace` passed (a helper may hold it open).
    fn text(self, grace: Duration) -> String {
        let _ = self.eof.recv_timeout(grace);
        let k = self.kept.lock().unwrap_or_else(|p| p.into_inner());
        String::from_utf8_lossy(&k).trim().to_string()
    }
}

/// The version named by a `<version>.manifest` file beside `exe` (every Windows Chrome and Edge build has one), as `<program>
/// <version>`; `None` when there is none. This is how `paddock-chromium` reads it: the program is not started.
pub(crate) fn version_from_manifest(exe: &Path) -> Option<String> {
    let version = std::fs::read_dir(exe.parent()?)
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".manifest").map(str::to_string))
        .filter_map(|v| {
            let parts: Option<Vec<u32>> = v.split('.').map(|p| p.parse().ok()).collect();
            parts.filter(|p| p.len() >= 2).map(|p| (p, v))
        })
        .max()?
        .1;
    let name = exe.file_stem()?.to_str()?;
    Some(format!("{name} {version}"))
}

/// The wording of a self-test that did not finish, with the elapsed time and a next step.
fn unanswered(why: &str, elapsed: Duration) -> Error {
    err(format!(
        "the browser did not answer its self-test ({why}, after {} s); try again, or check that antivirus is not blocking it",
        elapsed.as_secs()
    ))
}

/// [`self_test`] judged on the program's run state by the shared start judge (ADR 0031): a first start that is slow because
/// the disk is cold or the machine is busy is waited for while it works, a hung one is given up on after its idle ticks, and one
/// that exits is judged by its status. `cpu` and `pace` are seams for tests (a fake clock); `cancelled` says whether the end of
/// the wait is the user's Cancel (then [`Error::Cancelled`]) or the cap. The outcome is logged (rule 56): the program's file name
/// only, how it ended, its exit code and the time it took.
#[expect(
    clippy::disallowed_methods,
    reason = "the browser's self-test: std Command, as the driver is copied between apps"
)]
pub(crate) fn self_test_with(
    exe: &Path,
    cpu: impl FnMut(paddock_proc::ProcessId) -> Option<Duration>,
    pace: impl paddock_proc::Pace,
    cancelled: impl Fn() -> bool,
) -> Result<String> {
    // Test builds: a self-test starts the browser too (headless), so it asks the same gate as a launch (rule 7).
    #[cfg(any(test, feature = "testing"))]
    crate::discover::check_test_launch(exe, paddock_testkit::live_browser::BrowserWindow::Headless).map_err(err)?;
    let started = Instant::now();
    let name = exe.file_name().and_then(|n| n.to_str()).unwrap_or("browser");
    let done = |outcome: &str, code: Option<i32>| {
        log::info!(
            "browser self-test of {name}: {outcome}, exit code {}, {} ms",
            code.map_or_else(|| "none".to_string(), |c| c.to_string()),
            started.elapsed().as_millis()
        );
    };
    if cfg!(windows) {
        // `--version` starts the browser on Windows; the version is read, not asked for.
        let v = version_from_manifest(exe);
        done(if v.is_some() { "ok (version file)" } else { "no version file" }, None);
        return v.ok_or_else(|| {
            err("the browser's version could not be read (no version file beside it); reinstall it, or choose another browser")
        });
    }
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut process = paddock_proc::Supervised::spawn(cmd).map_err(|e| {
        done("did not start", None);
        err(format!("the browser does not run: {e}"))
    })?;
    // The pipe is read on its own thread from the start: a large output cannot make the program look idle.
    let output = process.take_stdout().map(BoundedRead::start);
    let quit = crate::launch::wait_to_exit(&mut process, cpu, pace);
    match quit {
        Quit::Exited => {}
        Quit::Stuck(cause) => {
            process.kill();
            let why = match cause {
                StallCause::Spinning(d) => format!("it used {} s of processor time without finishing", d.as_secs()),
                StallCause::Idle(d) | StallCause::Hung(d) => format!("no activity for {} s", d.as_secs()),
                _ => "it looks stuck".to_string(),
            };
            done(&format!("stuck ({cause:?})"), None);
            return Err(unanswered(&why, started.elapsed()));
        }
        Quit::Capped => {
            process.kill();
            if cancelled() {
                done("cancelled", None);
                return Err(Error::Cancelled);
            }
            done("capped", None);
            return Err(unanswered(
                &format!("gave up at the {} s limit", SELF_TEST_CAP.as_secs()),
                started.elapsed(),
            ));
        }
    }
    let status = process.wait();
    let code = status.and_then(|s| s.code());
    if !status.is_some_and(|s| s.success()) {
        done("exited with an error", code);
        return Err(err(format!(
            "the browser exited with an error{}; reinstall it, or choose another browser",
            code.map_or_else(String::new, |c| format!(" (code {c})"))
        )));
    }
    done("ok", code);
    Ok(output.map(|o| o.text(OUTPUT_GRACE)).unwrap_or_default())
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

/// The self-test of an extracted browser, run on a blocking thread.
pub type Verify = Arc<dyn Fn(&Path) -> Result<String> + Send + Sync>;

/// Downloads the pinned build for this platform from the embedded manifest into `data_dir` and returns the browser.
/// `downloader` is the app's one (its per-host caps hold across downloads); `ctx` is the shell job the download reports
/// its phases to; `job` carries the unpacking and the self-test.
pub async fn install(data_dir: &Path, downloader: &Arc<Downloader>, ctx: &JobContext, job: &Job) -> Result<Browser> {
    let pinned = Manifest::embedded().pinned(&crate::manifest::platform_key())?;
    install_pinned(
        &pinned,
        Os::CURRENT,
        data_dir,
        {
            let job = job.clone();
            Arc::new(move |exe: &Path| self_test_job(exe, &job))
        },
        downloader,
        ctx,
        job,
    )
    .await
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
    verify: Verify,
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
        let (check, path) = (verify.clone(), exe.path.clone());
        tokio::task::spawn_blocking(move || check(&path))
            .await
            .map_err(|e| err(e.to_string()))??;
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
    fn self_test_never_starts_an_installed_browser_in_a_test_build() {
        // Regression (review): `renderer_status()` self-tests `find_installed()`, so a status test on Windows ran the
        // user's msedge.exe. In test builds the self-test asks the launch gate first.
        // conformance: no-launch — the gate under test refuses each candidate before anything starts
        for b in crate::discover::candidates(crate::discover::Os::CURRENT, &|k| std::env::var(k).ok()) {
            let e = self_test(&b.path).unwrap_err().to_string();
            assert!(e.contains("installed"), "{}: {e}", b.path.display());
        }
    }

    /// 1 ms ticks for the judge: it counts ticks (the idle limit is 400 of them at 50 ms), so the verdicts are the same as in real
    /// time; the process is real and needs the time. Past the guard the wait is cancelled, so a broken judge fails in seconds.
    #[cfg(unix)]
    struct FastTicks(u32);

    #[cfg(unix)]
    impl paddock_proc::Pace for &mut FastTicks {
        fn wait(&mut self, _tick: Duration) -> bool {
            std::thread::sleep(Duration::from_millis(1));
            self.0 += 1;
            self.0 < 20_000
        }
    }

    #[cfg(unix)]
    fn fake_browser(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-chrome.sh");
        std::fs::write(&path, format!("#!/bin/sh\n# paddock-fake-browser\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// A pace that releases the fake browser (creates `go`) once `after` ticks have passed, so how long the "slow" start lasts is
    /// counted in ticks, not in host speed (a loaded host stretches a 1 ms tick several times over).
    #[cfg(unix)]
    struct ReleaseAfter {
        after: u32,
        go: PathBuf,
        ticks: u32,
    }

    #[cfg(unix)]
    impl paddock_proc::Pace for &mut ReleaseAfter {
        fn wait(&mut self, _tick: Duration) -> bool {
            std::thread::sleep(Duration::from_millis(1));
            self.ticks += 1;
            if self.ticks == self.after {
                std::fs::write(&self.go, b"go").unwrap();
            }
            self.ticks < 20_000
        }
    }

    /// A first start that takes far longer than the old fixed 15 s but keeps working is waited for and answers. The browser
    /// finishes only after more ticks than the idle limit, and its CPU use grows at every reading, so only run state keeps it alive.
    #[cfg(unix)]
    #[test]
    fn a_slow_self_test_that_keeps_working_is_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        let go = dir.path().join("go");
        let exe = fake_browser(
            dir.path(),
            &format!("while [ ! -e '{}' ]; do sleep 0.01; done; echo 'Chromium 1.2.3'", go.display()),
        );
        let mut n = 0u64;
        let mut pace = ReleaseAfter {
            after: idle_ticks() + 100,
            go,
            ticks: 0,
        };
        let out = self_test_with(
            &exe,
            move |_| {
                n += 1;
                Some(Duration::from_micros(n))
            },
            &mut pace,
            || false,
        )
        .unwrap();
        assert_eq!(out, "Chromium 1.2.3");
        assert!(
            pace.ticks > idle_ticks(),
            "waited {} ticks, past the idle limit, because it was working",
            pace.ticks
        );
    }

    /// The idle limit in ticks.
    #[cfg(unix)]
    fn idle_ticks() -> u32 {
        (crate::launch::SELF_TEST_PATIENCE.idle.as_millis() / crate::launch::SELF_TEST_PATIENCE.tick.as_millis()) as u32
    }

    /// Whether the process whose pid the fake wrote is gone (never started, reaped or a zombie).
    #[cfg(unix)]
    fn is_gone(pid_file: &Path) -> bool {
        let pid: u32 = std::fs::read_to_string(pid_file)
            .expect("the fake wrote its pid")
            .trim()
            .parse()
            .unwrap();
        paddock_proc::ProcessId::of(pid).is_none_or(|id| paddock_proc::run_state(id) == paddock_proc::RunState::Exited)
    }

    #[cfg(unix)]
    fn hanging_browser(dir: &Path) -> (PathBuf, PathBuf) {
        let pid_file = dir.join("pid");
        (
            fake_browser(dir, &format!("echo $$ > '{}'; exec sleep 300", pid_file.display())),
            pid_file,
        )
    }

    /// A self-test that does nothing and does not exit is hung: given up on after the idle ticks (60 s of them, not fewer: the
    /// old 20 s gave up on a first start blocked in a scan), not after a wall-clock time, and the process is ended.
    #[cfg(unix)]
    #[test]
    fn a_hung_self_test_is_given_up_on_by_run_state_and_ended() {
        let dir = tempfile::tempdir().unwrap();
        let (exe, pid_file) = hanging_browser(dir.path());
        let mut ticks = FastTicks(0);
        let e = self_test_with(&exe, |_| Some(Duration::from_secs(1)), &mut ticks, || false)
            .unwrap_err()
            .to_string();
        assert!(e.contains("did not answer"), "{e}");
        assert!(
            e.contains("no activity for 60 s") && e.contains("try again") && e.contains("antivirus"),
            "{e}"
        );
        assert!(
            ticks.0 >= idle_ticks(),
            "gave up after {} ticks, before the idle patience of {}",
            ticks.0,
            idle_ticks()
        );
        assert!(ticks.0 < 2_000, "judged after {} ticks", ticks.0);
        assert!(is_gone(&pid_file), "the hung browser was ended");
    }

    /// A pace that ends the wait as soon as the fake has written its pid (so there is a process to end).
    #[cfg(unix)]
    struct EndsOnceStarted(PathBuf);

    #[cfg(unix)]
    impl paddock_proc::Pace for EndsOnceStarted {
        fn wait(&mut self, _: Duration) -> bool {
            std::thread::sleep(Duration::from_millis(1));
            !std::fs::read_to_string(&self.0).is_ok_and(|s| !s.trim().is_empty())
        }
    }

    /// The cap ends the wait whatever the judge thinks: the program is killed and the error says it was the limit.
    #[cfg(unix)]
    #[test]
    fn the_cap_ends_a_wait_and_the_program() {
        let dir = tempfile::tempdir().unwrap();
        let (exe, pid_file) = hanging_browser(dir.path());
        let e = self_test_with(&exe, |_| Some(Duration::from_secs(1)), EndsOnceStarted(pid_file.clone()), || false).unwrap_err();
        assert!(e.to_string().contains("180 s limit") && e.to_string().contains("try again"), "{e}");
        assert!(is_gone(&pid_file));
    }

    /// Cancel during the self-test is `Error::Cancelled` and ends the program.
    #[cfg(unix)]
    #[test]
    fn cancel_ends_a_self_test() {
        let dir = tempfile::tempdir().unwrap();
        let (exe, pid_file) = hanging_browser(dir.path());
        let e = self_test_with(&exe, |_| Some(Duration::from_secs(1)), EndsOnceStarted(pid_file.clone()), || true).unwrap_err();
        assert!(matches!(e, Error::Cancelled), "{e}");
        assert!(is_gone(&pid_file));
    }

    /// The real pace: a job that is cancelled ends the production self-test after a tick, not after the idle patience.
    #[cfg(unix)]
    #[test]
    fn a_cancelled_job_ends_the_real_self_test_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let (exe, pid_file) = hanging_browser(dir.path());
        let job = Job::none();
        let canceller = job.cancel_flag().clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            canceller.cancel();
        });
        let started = Instant::now();
        let e = self_test_job(&exe, &job).unwrap_err();
        t.join().unwrap();
        assert!(matches!(e, Error::Cancelled), "{e}");
        assert!(started.elapsed() < Duration::from_secs(20), "took {:?}", started.elapsed());
        assert!(is_gone(&pid_file));
    }

    /// The real pace has an end: a deadline in the past ends the production self-test after a tick.
    #[cfg(unix)]
    #[test]
    fn the_real_pace_stops_at_its_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let (exe, pid_file) = hanging_browser(dir.path());
        let started = Instant::now();
        let e = self_test_until(&exe, Instant::now() + Duration::from_millis(2000), &Cancel::new()).unwrap_err();
        assert!(e.to_string().contains("limit"), "{e}");
        assert!(started.elapsed() < Duration::from_secs(20), "took {:?}", started.elapsed());
        assert!(is_gone(&pid_file));
    }

    /// More output than a pipe holds is read as it comes: the program neither blocks nor looks idle, and only the start is kept.
    #[cfg(unix)]
    #[test]
    fn a_large_output_does_not_block_the_self_test() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake_browser(dir.path(), "head -c 300000 /dev/zero | tr '\\0' 'a'");
        let out = self_test_with(&exe, |_| None, &mut FastTicks(0), || false).unwrap();
        assert_eq!(out.len(), OUTPUT_MAX);
    }

    /// A helper that inherits the pipe and outlives the program cannot hold the reply: what was written is returned.
    #[cfg(unix)]
    #[test]
    fn a_helper_holding_the_pipe_does_not_block_the_self_test() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake_browser(dir.path(), "sleep 30 &\necho 'Chromium 4.5.6'");
        let started = Instant::now();
        let out = self_test_with(&exe, |_| None, &mut FastTicks(0), || false).unwrap();
        assert_eq!(out, "Chromium 4.5.6");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    /// Rule 56: the outcome is logged with the program's file name (never its folder), the exit code and the time.
    #[cfg(unix)]
    #[test]
    fn the_outcome_is_logged_without_the_user_path() {
        capture_logs();
        let dir = tempfile::tempdir().unwrap();
        let exe = fake_browser(dir.path(), "exit 3");
        let _ = self_test_with(&exe, |_| None, &mut FastTicks(0), || false);
        let all = LOG.lock().unwrap().join("\n");
        let line = all
            .lines()
            .find(|l| l.contains("fake-chrome.sh") && l.contains("exited with an error"))
            .expect(&all);
        assert!(line.contains("exit code 3") && line.contains(" ms"), "{line}");
        assert!(!line.contains(dir.path().to_str().unwrap()), "{line}");
        let (hung, _) = hanging_browser(dir.path());
        let _ = self_test_with(&hung, |_| Some(Duration::from_secs(1)), &mut FastTicks(0), || false);
        assert!(
            LOG.lock().unwrap().iter().any(|l| l.contains("stuck (Idle(")),
            "{:?}",
            LOG.lock().unwrap()
        );
    }

    #[cfg(unix)]
    static LOG: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    #[cfg(unix)]
    struct Capture;
    #[cfg(unix)]
    impl log::Log for Capture {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }
        fn log(&self, r: &log::Record) {
            LOG.lock().unwrap().push(format!("{}", r.args()));
        }
        fn flush(&self) {}
    }

    #[cfg(unix)]
    fn capture_logs() {
        let _ = log::set_logger(&Capture);
        log::set_max_level(log::LevelFilter::Info);
    }

    #[test]
    fn the_version_is_read_from_the_manifest_beside_the_program() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("chrome.exe");
        assert_eq!(version_from_manifest(&exe), None);
        std::fs::write(d.path().join("notes.manifest"), "").unwrap();
        std::fs::write(d.path().join("9.0.1.manifest"), "").unwrap();
        std::fs::write(d.path().join("10.0.1.2.manifest"), "").unwrap();
        assert_eq!(version_from_manifest(&exe).as_deref(), Some("chrome 10.0.1.2"));
        assert_eq!(version_from_manifest(Path::new("/")), None);
    }

    /// A self-test that exits with an error is judged by its exit (not as hung): the message says it exited with an error.
    #[cfg(unix)]
    #[test]
    fn a_self_test_that_exits_with_an_error_fails_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake_browser(dir.path(), "exit 3");
        let e = self_test_with(&exe, |_| None, &mut FastTicks(0), || false).unwrap_err().to_string();
        assert!(e.contains("exited with an error"), "{e}");
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
