//! The slow, blocking steps the supervisor runs on worker threads: release discovery, download, verification,
//! the binary swap and its roll-back, asking a binary for its version, and registration. Each returns a result
//! for the supervisor to feed into the core's state machines; none of them touches supervision state.

use crate::jobs::JobHub;
use crate::net::{Http, MAX_BINARY};
use crate::process::{quiet_command, run_with_timeout};
use paddock_download::{Downloader, HttpConfig, Integrity, Request, Sha256Digest as PinnedDigest};
use paddock_jobs::JobError;
use pitcrew_core::checksum::{agreed, checksum_for, MAX_CHECKSUM_FILE};
use pitcrew_core::distribution::{RunnerKind, MAX_RELEASE_BODY};
use pitcrew_core::layout;
use pitcrew_core::platform::Platform;
use pitcrew_core::redact::redact;
use pitcrew_core::update::Channel;
use pitcrew_core::version::RunnerVersion;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long `--version` may take.
pub const VERSION_TIMEOUT: Duration = Duration::from_secs(20);
/// How long `register` may take.
pub const REGISTER_TIMEOUT: Duration = Duration::from_secs(120);

/// Rewrites a public URL (tests point downloads at a local server).
pub type Rewrite = std::sync::Arc<dyn Fn(&str) -> String + Send + Sync>;

/// Where release information comes from (overridable in tests).
#[derive(Clone)]
pub struct Sources {
    /// The client.
    pub http: Http,
    /// Rewrites a public URL (tests point it at a local server).
    pub rewrite: Option<Rewrite>,
    /// The one downloader of the service: its host caps hold across every download.
    pub downloader: std::sync::Arc<Downloader>,
    /// The service's jobs: a download is a job with phases and Cancel.
    pub jobs: JobHub,
}

impl Sources {
    /// The real sources.
    pub fn public() -> Self {
        let config = HttpConfig {
            user_agent: concat!("Pitcrew/", env!("CARGO_PKG_VERSION")).to_string(),
            ..HttpConfig::default()
        };
        let downloader = Downloader::http(config).unwrap_or_else(|_| Downloader::new(std::sync::Arc::new(Unavailable)));
        Sources {
            http: Http::new(),
            rewrite: None,
            downloader: std::sync::Arc::new(downloader),
            jobs: JobHub::new(),
        }
    }

    fn url(&self, u: &str) -> String {
        self.rewrite.as_ref().map_or_else(|| u.to_string(), |f| f(u))
    }
}

/// The newest version on `channel`.
pub fn latest(src: &Sources, kind: RunnerKind, channel: Channel) -> Result<RunnerVersion, String> {
    match channel {
        Channel::Stable => {
            let body = src.http.text(&src.url(kind.latest_url()), MAX_RELEASE_BODY, None)?;
            kind.parse_latest(&body).map_err(|e| e.to_string())
        }
        Channel::Prerelease => {
            let body = src.http.text(&src.url(kind.releases_url()), MAX_RELEASE_BODY * 4, None)?;
            kind.parse_releases(&body)
                .map_err(|e| e.to_string())?
                .into_iter()
                .next()
                .ok_or_else(|| "the release list is empty".to_string())
        }
    }
}

/// The publisher's checksum of the release's file: read from two hosts that must agree (the second is skipped, and the note
/// says so, only when it cannot be reached). Refuses releases that are signed (signature checking is not built yet).
fn pinned_checksum(src: &Sources, kind: RunnerKind, files: &pitcrew_core::distribution::ReleaseFiles) -> Result<(String, String), String> {
    let mut urls = files.checksum_urls.iter();
    let first_url = urls.next().ok_or("the release publishes no checksum")?;
    let first_text = src.http.text(&src.url(first_url), MAX_CHECKSUM_FILE, None)?;
    let first = checksum_for(&first_text, &files.asset_name).map_err(|e| e.to_string())?;
    let mut note = String::from("checksum verified");
    let second = match urls.next() {
        Some(u) => match src.http.text(&src.url(u), MAX_CHECKSUM_FILE, None) {
            Ok(text) => {
                note = "checksum verified by two sources".into();
                Some(checksum_for(&text, &files.asset_name).map_err(|e| format!("the second checksum source: {e}"))?)
            }
            Err(e) => {
                note = format!("checksum verified by one source (the second could not be reached: {e})");
                None
            }
        },
        None => None,
    };
    let expected = agreed(&files.asset_name, first, second).map_err(|e| e.to_string())?;
    if files.signature_url.is_some() {
        return Err(format!(
            "{} releases are signed and signature checking is not built yet; the update is not installed",
            kind.display_name()
        ));
    }
    Ok((expected.to_string(), note))
}

/// Downloads `version` to `<binary>.download` as a job (`paddock-download`: segmented Range requests where the host allows,
/// per-host caps, 429/`Retry-After` back-off, a resumable `.part`, the free-space check, Cancel in every phase). The
/// publisher's checksum is read first (two hosts that must agree) and pinned, so the SHA-256 computed while writing is
/// compared when the last byte lands and nothing is read twice; a file that does not match is never put in place. Returns a
/// note for the log.
pub fn download(src: &Sources, kind: RunnerKind, version: &RunnerVersion, platform: Platform, binary: &Path) -> Result<String, String> {
    let files = kind.release_files(version, platform).map_err(|e| e.to_string())?;
    let dest = layout::download_path(binary);
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let (digest, note) = pinned_checksum(src, kind, &files)?;
    let url = src.url(&files.binary_url);
    src.http.check(&url)?;
    let pinned = PinnedDigest::from_hex(&digest).map_err(|e| e.to_string())?;
    let request = Request::new(&url, &dest, Integrity::Sha256(pinned))
        .map_err(|e| e.to_string())?
        .max_bytes(MAX_BINARY);
    let ctx = src.jobs.registry().register(
        "pitcrew.download",
        "Downloading the runner",
        Some(&format!("{} {version}", kind.display_name())),
    );
    ctx.run(|ctx| src.downloader.download(ctx, &request).map(drop).map_err(JobError::from))
        .map_err(|e| format!("the download of {} {version} failed: {e}", kind.display_name()))?;
    make_executable(&dest)?;
    Ok(note)
}

/// Checks `<binary>.download`: it runs and reports the version it was downloaded for (its checksum was compared as it was
/// written, in [`download`]).
pub fn verify_download(
    _src: &Sources,
    _kind: RunnerKind,
    version: &RunnerVersion,
    _platform: Platform,
    binary: &Path,
) -> Result<String, String> {
    let dest = layout::download_path(binary);
    match binary_version(&dest) {
        Some(v) if &v == version => Ok(format!("the download runs and reports version {version}")),
        Some(v) => Err(format!("the download reports version {v}, not {version}")),
        None => Err("the download does not run or does not report its version".into()),
    }
}

/// A transport for a service whose HTTPS client could not be built: every download fails with that reason.
struct Unavailable;

impl paddock_download::Transport for Unavailable {
    fn get(&self, _: &paddock_download::Get<'_>) -> Result<paddock_download::Response, paddock_download::TransportError> {
        Err(paddock_download::TransportError::new(
            paddock_download::TransportErrorKind::Invalid,
            "the HTTPS client could not be set up",
        ))
    }
}

/// The version a binary reports with `--version`.
pub fn binary_version(binary: &Path) -> Option<RunnerVersion> {
    let mut cmd = quiet_command(binary);
    cmd.arg("--version");
    let out = run_with_timeout(cmd, VERSION_TIMEOUT).ok()?;
    RunnerVersion::from_version_output(&format!("{} {}", out.stdout, out.stderr)).ok()
}

/// Puts the verified download in place, keeping the installed binary as `<binary>.previous`.
pub fn swap(binary: &Path) -> Result<(), String> {
    let download = layout::download_path(binary);
    let previous = layout::previous_path(binary);
    let had_binary = binary.exists();
    if had_binary {
        let _ = std::fs::remove_file(&previous);
        std::fs::rename(binary, &previous).map_err(|e| format!("moving the installed binary aside failed: {e}"))?;
    }
    if let Err(e) = std::fs::rename(&download, binary) {
        if had_binary {
            // Put the old one back; the runner then starts as before.
            let _ = std::fs::rename(&previous, binary);
        }
        return Err(format!("{e}"));
    }
    Ok(())
}

/// Puts `<binary>.previous` back (the failed binary is deleted).
pub fn restore_previous(binary: &Path) -> Result<(), String> {
    let previous = layout::previous_path(binary);
    if !previous.exists() {
        return Err("there is no previous binary".into());
    }
    let _ = std::fs::remove_file(binary);
    std::fs::rename(&previous, binary).map_err(|e| e.to_string())
}

/// Deletes `<binary>.download`.
pub fn discard_download(binary: &Path) {
    let _ = std::fs::remove_file(layout::download_path(binary));
}

#[cfg(unix)]
fn make_executable(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).map_err(|e| format!("{}: {e}", p.display()))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<(), String> {
    Ok(())
}

/// What registration needs.
pub struct Registration<'a> {
    /// The runner's folder.
    pub dir: &'a Path,
    /// Its binary.
    pub binary: &'a Path,
    /// Its configuration file.
    pub config: PathBuf,
    /// The server.
    pub server: &'a str,
    /// The token (passed to `register` only).
    pub token: &'a str,
    /// The name the server shows.
    pub name: &'a str,
    /// Labels.
    pub labels: &'a [String],
}

/// Writes the runner configuration if it has none (`generate-config`, with jobs and cache inside the folder).
pub fn ensure_runner_config(binary: &Path, config: &Path, dir: &Path) -> Result<(), String> {
    if config.exists() {
        return Ok(());
    }
    let mut cmd = quiet_command(binary);
    cmd.arg("generate-config").current_dir(dir);
    let out = run_with_timeout(cmd, VERSION_TIMEOUT).map_err(|e| e.to_string())?;
    if !out.success || out.stdout.trim().is_empty() {
        return Err(format!(
            "the runner could not write its default configuration: {}",
            redact(out.stderr.trim())
        ));
    }
    crate::store::write_atomic(config, &layout::adjust_generated_config(&out.stdout, dir)).map_err(|e| e.to_string())
}

/// How a registration ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registered {
    /// The runner's `register` command registered the folder with the server.
    New,
    /// The folder was registered already (an adopted runner folder): it is used as it is and the token is not sent.
    Adopted,
}

/// Runs the runner's `register` command, unless the folder is registered already. Errors are redacted (the token
/// never appears in them).
pub fn register(r: &Registration<'_>) -> Result<Registered, String> {
    if r.dir.join(layout::REGISTRATION_FILE).exists() {
        return Ok(Registered::Adopted);
    }
    ensure_runner_config(r.binary, &r.config, r.dir)?;
    let mut cmd = quiet_command(r.binary);
    cmd.args(layout::register_args(&r.config, r.server, r.token, r.name, r.labels))
        .current_dir(r.dir);
    let out = run_with_timeout(cmd, REGISTER_TIMEOUT).map_err(|e| e.to_string())?;
    let text = format!("{}\n{}", out.stdout, out.stderr).replace(r.token, "[redacted]");
    if out.success && r.dir.join(layout::REGISTRATION_FILE).exists() {
        Ok(Registered::New)
    } else {
        let last: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).rev().take(3).collect();
        let detail = last.into_iter().rev().collect::<Vec<_>>().join(" / ");
        Err(format!("the runner refused to register: {}", redact(&detail)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::tests::serve;
    use pitcrew_core::checksum::Sha256Digest;
    use pitcrew_core::platform::{Arch, Os};

    fn platform() -> Platform {
        Platform {
            os: Os::Linux,
            arch: Arch::Amd64,
        }
    }

    fn sources(base: String) -> Sources {
        Sources {
            http: Http::for_tests(),
            downloader: std::sync::Arc::new(Downloader::http(HttpConfig::default()).unwrap()),
            jobs: JobHub::new(),
            rewrite: Some(std::sync::Arc::new(move |u: &str| {
                let path = u
                    .split("://")
                    .nth(1)
                    .and_then(|r| r.find('/').map(|i| r[i..].to_string()))
                    .unwrap_or_default();
                format!("{base}{}", path.split('?').next().unwrap_or(""))
            })),
        }
    }

    #[test]
    fn discovers_releases_over_http() {
        let base = serve(vec![
            (
                "/gitea-runner/version.json".into(),
                200,
                br#"{"latest":{"version":"5.2.0"}}"#.to_vec(),
            ),
            (
                "/api/v1/repos/gitea/runner/releases".into(),
                200,
                br#"[{"tag_name":"v5.3.0-rc.1"},{"tag_name":"v5.2.0"}]"#.to_vec(),
            ),
        ]);
        let src = sources(base);
        assert_eq!(latest(&src, RunnerKind::GiteaRunner, Channel::Stable).unwrap().to_string(), "5.2.0");
        assert_eq!(
            latest(&src, RunnerKind::GiteaRunner, Channel::Prerelease).unwrap().to_string(),
            "5.3.0-rc.1"
        );
    }

    // ---- downloads, against an in-process HTTP server (no network) -----------------------------------------------------

    const ASSET: &str = "gitea-runner-5.2.0-linux-amd64";

    /// What the test server does to the requests for the binary.
    #[derive(Default)]
    struct Behaviour {
        /// Pause after every 16 KiB of the body (a slow host, so a download can be cancelled in flight).
        slow: std::time::Duration,
        /// Answer the first this-many requests for the binary with 429 and `Retry-After: 0`.
        throttle: std::sync::atomic::AtomicU32,
        /// Requests for the binary seen, and the `Range` headers among them.
        gets: std::sync::Mutex<Vec<Option<String>>>,
    }

    /// A server with Range support: the binary at `/gitea-runner/5.2.0/<asset>`, and the two checksum files that agree on
    /// `digest`.
    fn ranged_server(body: Vec<u8>, digest: String, behaviour: std::sync::Arc<Behaviour>) -> String {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (body, digest, behaviour) = (body.clone(), digest.clone(), behaviour.clone());
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut first = String::new();
                    let _ = reader.read_line(&mut first);
                    let mut range = None;
                    loop {
                        let mut l = String::new();
                        if reader.read_line(&mut l).unwrap_or(0) == 0 || l == "\r\n" {
                            break;
                        }
                        if let Some(v) = l.to_ascii_lowercase().strip_prefix("range:") {
                            range = Some(v.trim().to_string());
                        }
                    }
                    let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let mut s = stream;
                    if path.ends_with(".sha256") || path.ends_with("checksums.txt") {
                        let text = format!("{digest}  {ASSET}\n");
                        let _ = write!(
                            s,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                            text.len()
                        );
                        return;
                    }
                    if !path.ends_with(ASSET) {
                        let _ = write!(s, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        return;
                    }
                    behaviour.gets.lock().unwrap().push(range.clone());
                    if behaviour.throttle.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                        behaviour.throttle.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                        let _ = write!(
                            s,
                            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        );
                        return;
                    }
                    let total = body.len();
                    let (status, from, to) = match range
                        .as_deref()
                        .and_then(|r| r.strip_prefix("bytes="))
                        .and_then(|r| r.split_once('-'))
                    {
                        Some((a, b)) => {
                            let from: usize = a.trim().parse().unwrap_or(0);
                            let to: usize = b.trim().parse().unwrap_or(total - 1).min(total - 1);
                            ("206 Partial Content", from, to)
                        }
                        None => ("200 OK", 0, total - 1),
                    };
                    let part = &body[from..=to];
                    let content_range = if status.starts_with("206") {
                        format!("Content-Range: bytes {from}-{to}/{total}\r\n")
                    } else {
                        String::new()
                    };
                    let _ = write!(s, "HTTP/1.1 {status}\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\n{content_range}ETag: \"v1\"\r\nConnection: close\r\n\r\n", part.len());
                    for chunk in part.chunks(16 * 1024) {
                        if s.write_all(chunk).is_err() {
                            return;
                        }
                        std::thread::sleep(behaviour.slow);
                    }
                });
            }
        });
        base
    }

    fn payload() -> Vec<u8> {
        (0..300_000u32).map(|i| (i % 251) as u8).collect()
    }

    /// A job's snapshots as the service would push them to the app.
    fn watch(src: &Sources) -> std::sync::mpsc::Receiver<crate::supervisor::Msg> {
        let (tx, rx) = std::sync::mpsc::channel();
        src.jobs.attach(tx);
        rx
    }

    fn snapshots(rx: &std::sync::mpsc::Receiver<crate::supervisor::Msg>) -> Vec<paddock_jobs::JobSnapshot> {
        rx.try_iter()
            .filter_map(|m| match m {
                crate::supervisor::Msg::Job(j) => Some(*j),
                _ => None,
            })
            .collect()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pitcrew-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_download_is_a_job_with_phases_hashed_while_writing_and_the_checksum_agreed_by_two_sources() {
        let body = payload();
        let digest = Sha256Digest::of_bytes(&body).to_string();
        let behaviour = std::sync::Arc::new(Behaviour::default());
        let src = sources(ranged_server(body.clone(), digest, behaviour));
        let rx = watch(&src);
        let dir = temp("dl-ok");
        let binary = dir.join("bin/gitea-runner");
        let v = RunnerVersion::parse("5.2.0").unwrap();
        let note = download(&src, RunnerKind::GiteaRunner, &v, platform(), &binary).unwrap();
        assert_eq!(note, "checksum verified by two sources");
        assert_eq!(std::fs::read(layout::download_path(&binary)).unwrap(), body);
        let jobs = snapshots(&rx);
        assert!(
            jobs.iter()
                .all(|j| j.kind.0 == "pitcrew.download" && j.title == "Downloading the runner"),
            "{jobs:?}"
        );
        let phases: Vec<String> = jobs
            .iter()
            .filter_map(|j| match &j.state {
                paddock_jobs::JobState::Running { phase: Some(p) } => Some(p.phase.id.clone()),
                _ => None,
            })
            .fold(Vec::new(), |mut acc, id| {
                if acc.last() != Some(&id) {
                    acc.push(id);
                }
                acc
            });
        assert_eq!(phases, ["connect", "download", "verify"], "each phase on its own scale, in order");
        assert!(
            matches!(jobs.last().unwrap().state, paddock_jobs::JobState::Done),
            "{:?}",
            jobs.last()
        );
        // The download itself no longer needs a separate verification: it is a program that reports its version.
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disagreeing_checksums_stop_the_download_before_a_byte_is_fetched() {
        let body = payload();
        let behaviour = std::sync::Arc::new(Behaviour::default());
        // The server's two checksum files are the same text, so make the second source disagree through another host path.
        let base = serve(vec![
            (
                format!("/gitea-runner/5.2.0/{ASSET}.sha256"),
                200,
                format!("{}  {ASSET}\n", Sha256Digest::of_bytes(&body)).into_bytes(),
            ),
            (
                "/gitea/runner/releases/download/v5.2.0/checksums.txt".into(),
                200,
                format!("{}  {ASSET}\n", Sha256Digest::of_bytes(b"x")).into_bytes(),
            ),
            (format!("/gitea-runner/5.2.0/{ASSET}"), 200, body),
        ]);
        let src = sources(base);
        let dir = temp("dl-disagree");
        let binary = dir.join("bin/gitea-runner");
        let v = RunnerVersion::parse("5.2.0").unwrap();
        let err = download(&src, RunnerKind::GiteaRunner, &v, platform(), &binary).unwrap_err();
        assert!(err.contains("disagree"), "{err}");
        assert!(!layout::download_path(&binary).exists());
        assert!(behaviour.gets.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_does_not_match_the_pinned_checksum_is_never_put_in_place() {
        let body = payload();
        let wrong = Sha256Digest::of_bytes(b"not this").to_string();
        let src = sources(ranged_server(body, wrong, std::sync::Arc::new(Behaviour::default())));
        let dir = temp("dl-bad");
        let binary = dir.join("bin/gitea-runner");
        let v = RunnerVersion::parse("5.2.0").unwrap();
        let err = download(&src, RunnerKind::GiteaRunner, &v, platform(), &binary).unwrap_err();
        assert!(
            err.to_lowercase().contains("checksum") || err.to_lowercase().contains("match") || err.to_lowercase().contains("sha"),
            "{err}"
        );
        assert!(!layout::download_path(&binary).exists(), "nothing was put in place");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_throttled_host_is_waited_for_and_the_download_still_completes() {
        let body = payload();
        let digest = Sha256Digest::of_bytes(&body).to_string();
        let behaviour = std::sync::Arc::new(Behaviour::default());
        behaviour.throttle.store(2, std::sync::atomic::Ordering::SeqCst);
        let src = sources(ranged_server(body.clone(), digest, behaviour.clone()));
        let dir = temp("dl-429");
        let binary = dir.join("bin/gitea-runner");
        let v = RunnerVersion::parse("5.2.0").unwrap();
        download(&src, RunnerKind::GiteaRunner, &v, platform(), &binary).unwrap();
        assert_eq!(std::fs::read(layout::download_path(&binary)).unwrap(), body);
        assert!(
            behaviour.gets.lock().unwrap().len() >= 3,
            "the 429s were retried: {:?}",
            behaviour.gets.lock().unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_ends_a_download_in_flight_and_the_next_one_resumes_from_what_was_kept() {
        let body = payload();
        let digest = Sha256Digest::of_bytes(&body).to_string();
        let behaviour = std::sync::Arc::new(Behaviour {
            slow: Duration::from_millis(40),
            ..Behaviour::default()
        });
        let src = sources(ranged_server(body.clone(), digest, behaviour.clone()));
        let rx = watch(&src);
        let dir = temp("dl-cancel");
        let binary = dir.join("bin/gitea-runner");
        let v = RunnerVersion::parse("5.2.0").unwrap();
        let worker = {
            let (src, binary, v) = (src.clone(), binary.clone(), v.clone());
            std::thread::spawn(move || download(&src, RunnerKind::GiteaRunner, &v, platform(), &binary))
        };
        // Cancel once some bytes are on disk: the job reports its download phase.
        let id = loop {
            let seen = rx.recv_timeout(Duration::from_secs(20)).expect("the job reports");
            if let crate::supervisor::Msg::Job(j) = seen {
                if let paddock_jobs::JobState::Running { phase: Some(p) } = &j.state {
                    if p.phase.id == "download" && p.done > 0 {
                        break j.id.0;
                    }
                }
            }
        };
        assert!(src.jobs.cancel(id), "the job is known and running");
        let err = worker.join().unwrap().unwrap_err();
        assert!(err.contains("cancel"), "{err}");
        assert!(!layout::download_path(&binary).exists(), "not a finished file");
        assert!(
            layout::download_path(&binary).with_extension("download.part").exists()
                || std::fs::read_dir(binary.parent().unwrap())
                    .unwrap()
                    .flatten()
                    .any(|e| e.file_name().to_string_lossy().ends_with(".part")),
            "the partial is kept"
        );
        // The second run asks only for what is missing (a Range request starting after zero) and ends with the right file.
        behaviour.gets.lock().unwrap().clear();
        let rx2 = watch(&src);
        download(&src, RunnerKind::GiteaRunner, &v, platform(), &binary).unwrap();
        assert_eq!(std::fs::read(layout::download_path(&binary)).unwrap(), body);
        let resumed = behaviour.gets.lock().unwrap().iter().flatten().any(|r| !r.starts_with("bytes=0-"));
        assert!(resumed, "resumed with a Range request: {:?}", behaviour.gets.lock().unwrap());
        assert!(snapshots(&rx2)
            .iter()
            .any(|j| matches!(&j.state, paddock_jobs::JobState::Running { phase: Some(p) } if p.phase.id == "resume")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_release_is_refused_before_downloading() {
        let src = sources("http://127.0.0.1:1".into());
        let binary = temp("dl-none").join("bin/gitea-runner");
        assert!(download(
            &src,
            RunnerKind::GiteaRunner,
            &RunnerVersion::parse("9.9.9").unwrap(),
            platform(),
            &binary
        )
        .is_err());
    }

    #[test]
    fn swaps_and_restores_binaries() {
        let dir = std::env::temp_dir().join(format!("pitcrew-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let binary = dir.join("gitea-runner");
        assert!(swap(&binary).is_err(), "nothing downloaded");
        std::fs::write(&binary, b"old").unwrap();
        assert!(swap(&binary).is_err());
        assert_eq!(std::fs::read(&binary).unwrap(), b"old", "a failed swap keeps the installed binary");
        std::fs::write(layout::download_path(&binary), b"new").unwrap();
        swap(&binary).unwrap();
        assert_eq!(std::fs::read(&binary).unwrap(), b"new");
        assert_eq!(std::fs::read(layout::previous_path(&binary)).unwrap(), b"old");
        restore_previous(&binary).unwrap();
        assert_eq!(std::fs::read(&binary).unwrap(), b"old");
        assert!(restore_previous(&binary).is_err(), "nothing to go back to");
        assert_eq!(binary_version(&binary), None, "not a program");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
