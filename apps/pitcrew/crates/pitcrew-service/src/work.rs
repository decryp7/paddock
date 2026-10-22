//! The slow, blocking steps the supervisor runs on worker threads: release discovery, download, verification,
//! the binary swap and its roll-back, asking a binary for its version, and registration. Each returns a result
//! for the supervisor to feed into the core's state machines; none of them touches supervision state.

use crate::net::{file_digest, Http, MAX_BINARY};
use crate::process::{quiet_command, run_with_timeout};
use pitcrew_core::checksum::{agreed, checksum_for, verify, MAX_CHECKSUM_FILE};
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
}

impl Sources {
    /// The real sources.
    pub fn public() -> Self {
        Sources {
            http: Http::new(),
            rewrite: None,
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

/// Downloads `version` to `<binary>.download`.
pub fn download(src: &Sources, kind: RunnerKind, version: &RunnerVersion, platform: Platform, binary: &Path) -> Result<(), String> {
    let files = kind.release_files(version, platform).map_err(|e| e.to_string())?;
    let dest = layout::download_path(binary);
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    src.http.download(&src.url(&files.binary_url), &dest, MAX_BINARY)?;
    make_executable(&dest)?;
    Ok(())
}

/// Verifies `<binary>.download`: its SHA-256 against the publisher's checksum, read from two hosts that must agree
/// (the second is skipped, and the result says so, only when it cannot be reached), then asks it for its version.
/// Returns a note for the log.
pub fn verify_download(
    src: &Sources,
    kind: RunnerKind,
    version: &RunnerVersion,
    platform: Platform,
    binary: &Path,
) -> Result<String, String> {
    let files = kind.release_files(version, platform).map_err(|e| e.to_string())?;
    let dest = layout::download_path(binary);
    let actual = file_digest(&dest)?;
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
    verify(expected, actual).map_err(|e| e.to_string())?;
    if files.signature_url.is_some() {
        return Err(format!(
            "{} releases are signed and signature checking is not built yet; the update is not installed",
            kind.display_name()
        ));
    }
    match binary_version(&dest) {
        Some(v) if &v == version => Ok(note),
        Some(v) => Err(format!("the download reports version {v}, not {version}")),
        None => Err("the download does not run or does not report its version".into()),
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

/// Runs the runner's `register` command. Errors are redacted (the token never appears in them).
pub fn register(r: &Registration<'_>) -> Result<(), String> {
    if r.dir.join(layout::REGISTRATION_FILE).exists() {
        return Err(
            "this folder is registered already; remove the runner on the server and delete its .runner file to register it again".into(),
        );
    }
    ensure_runner_config(r.binary, &r.config, r.dir)?;
    let mut cmd = quiet_command(r.binary);
    cmd.args(layout::register_args(&r.config, r.server, r.token, r.name, r.labels))
        .current_dir(r.dir);
    let out = run_with_timeout(cmd, REGISTER_TIMEOUT).map_err(|e| e.to_string())?;
    let text = format!("{}\n{}", out.stdout, out.stderr).replace(r.token, "[redacted]");
    if out.success && r.dir.join(layout::REGISTRATION_FILE).exists() {
        Ok(())
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
    fn checks_downloads_and_refuses_disagreeing_checksums() {
        let v = RunnerVersion::parse("5.2.0").unwrap();
        let asset = "gitea-runner-5.2.0-linux-amd64";
        let bytes = b"not really a binary".to_vec();
        let digest = Sha256Digest::of_bytes(&bytes).to_string();
        let other = Sha256Digest::of_bytes(b"x").to_string();
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
            (format!("/gitea-runner/5.2.0/{asset}"), 200, bytes.clone()),
            (
                format!("/gitea-runner/5.2.0/{asset}.sha256"),
                200,
                format!("{digest}  {asset}\n").into_bytes(),
            ),
            (
                "/gitea/runner/releases/download/v5.2.0/checksums.txt".into(),
                200,
                format!("{other}  {asset}\n").into_bytes(),
            ),
        ]);
        let src = sources(base);
        assert_eq!(latest(&src, RunnerKind::GiteaRunner, Channel::Stable).unwrap(), v);
        assert_eq!(
            latest(&src, RunnerKind::GiteaRunner, Channel::Prerelease).unwrap().to_string(),
            "5.3.0-rc.1"
        );
        let dir = std::env::temp_dir().join(format!("pitcrew-work-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let binary = dir.join("bin/gitea-runner");
        download(&src, RunnerKind::GiteaRunner, &v, platform(), &binary).unwrap();
        assert!(layout::download_path(&binary).exists());
        let err = verify_download(&src, RunnerKind::GiteaRunner, &v, platform(), &binary).unwrap_err();
        assert!(err.contains("disagree"), "{err}");
        discard_download(&binary);
        assert!(!layout::download_path(&binary).exists());
        assert!(download(
            &src,
            RunnerKind::GiteaRunner,
            &RunnerVersion::parse("9.9.9").unwrap(),
            platform(),
            &binary
        )
        .is_err());
        let _ = std::fs::remove_dir_all(&dir);
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
