//! The opt-in download of a pinned tool for the helper (ADR 0027 section 7; WP-12): a job with phases and Cancel that fetches the
//! pinned file with `paddock-download` over `paddock-net`'s `Egress` (https, the pin's hosts only, public addresses, the pinned
//! size as a cap), checks the pinned **size and SHA-256** (while writing, and again on the bytes it hands over), and only then
//! streams the bytes to the helper's `InstallTool`, which checks them against its own pinned table again. The OpenPGP signature
//! of the publisher is checked when a pin is added (`scripts/verify-tool-pin.sh`), not here: the app carries no OpenPGP code.
//! Dyno never bundles or mirrors the tool (GPL-2.0-or-later) and the staging file is deleted whatever happens. Logs carry phases,
//! byte counts, durations and failure kinds; never an address.

use crate::helper_link::{HelperLink, HelperTransport};
use crate::tool_pins::ToolPin;
use dyno_core::helper_requests::ToolKind;
use dyno_core::helper_wire::ToolStatus;
use dyno_core::ports::PlatformError;
use dyno_core::tool_offer::{InstalledTool, ToolSource};
use paddock_download::{Config, DownloadError, Downloader, Integrity, Request, Sha256Digest};
use paddock_jobs::{JobContext, JobError, Phase};
use paddock_net::{Egress, Policy, Reach, SystemLookup};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// The job phase for checking the bytes before they are handed over.
pub const CHECK: &str = "check";
/// The job phase for the helper's install.
pub const INSTALL: &str = "install";

/// Where the checked bytes go: the helper's `InstallTool`.
pub trait ToolSink {
    /// Installs `bytes` (the helper checks them against its pinned table).
    ///
    /// # Errors
    /// What the helper refused, in words.
    fn install(&self, tool: ToolKind, bytes: Vec<u8>) -> Result<ToolStatus, PlatformError>;
}

impl<T: HelperTransport> ToolSink for HelperLink<T> {
    fn install(&self, tool: ToolKind, bytes: Vec<u8>) -> Result<ToolStatus, PlatformError> {
        self.install_tool(tool, bytes)
    }
}

/// Why the tool was not installed (each message says what happened and what to do).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    /// The download did not complete.
    #[error("{0}")]
    Download(String),
    /// The bytes are not the pinned file; nothing was handed over.
    #[error("The download is not the file Dyno pinned ({0}), so nothing was installed. Try again later; if it happens again, report it.")]
    NotThePinnedFile(&'static str),
    /// The helper refused or did not answer.
    #[error("The helper did not install it: {0}")]
    Helper(String),
    /// Stopped by the user.
    #[error("Stopped.")]
    Cancelled,
}

impl From<ToolError> for JobError {
    fn from(e: ToolError) -> JobError {
        match e {
            ToolError::Cancelled => JobError::Cancelled(paddock_jobs::CancelReason::User),
            other => JobError::Failed(other.to_string()),
        }
    }
}

fn map_download(e: DownloadError) -> ToolError {
    match e {
        DownloadError::Cancelled(_) => ToolError::Cancelled,
        DownloadError::ChecksumMismatch { .. } => ToolError::NotThePinnedFile("its checksum differs"),
        DownloadError::SizeMismatch { .. } | DownloadError::TooLarge { .. } => ToolError::NotThePinnedFile("its size differs"),
        DownloadError::Http { status, .. } => ToolError::Download(format!(
            "The download site answered {status}. Try again later; the file may have moved."
        )),
        DownloadError::DiskFull | DownloadError::NotEnoughSpace { .. } => {
            ToolError::Download("There is not enough free disk space to download the tool. Free some space and try again.".into())
        }
        DownloadError::Io(e) => ToolError::Download(format!(
            "The tool could not be saved ({}). Check free space and permissions.",
            e.kind()
        )),
        other => ToolError::Download(format!("{other}. Check the network and try again.")),
    }
}

/// The policy of the tool door: the pin's hosts only, https only, public addresses, the pinned size as the body cap.
pub fn policy(pin: &ToolPin) -> Policy {
    let hosts: Vec<&str> = pin.hosts.iter().map(String::as_str).collect();
    let mut p = Policy::default()
        .reach(Reach::Public)
        .max_redirects(3)
        .max_body(pin.size)
        .timeout(Duration::from_secs(120))
        .allow_hosts(&hosts);
    p.user_agent = format!("Dyno/{}", env!("CARGO_PKG_VERSION"));
    p
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Downloads pinned tools.
pub struct ToolDownloader {
    downloader: Downloader,
}

impl ToolDownloader {
    /// Over a given door (tests pass a fake wire) and download settings.
    pub fn new(egress: Arc<Egress>, config: Config) -> ToolDownloader {
        ToolDownloader {
            downloader: Downloader::new(Arc::new(egress.download_transport())).with_config(config),
        }
    }

    /// The real door for `pin`: the system resolver and the real wire with the system's proxy settings.
    pub fn real(pin: &ToolPin, clock: Arc<dyn paddock_jobs::time::Clock>) -> Result<ToolDownloader, ToolError> {
        let p = policy(pin);
        let wire = paddock_net::UreqWire::new(&p, paddock_download::ProxyMode::System).map_err(|e| ToolError::Download(e.to_string()))?;
        let egress = Egress::new(p, Arc::new(wire), Arc::new(SystemLookup), clock);
        Ok(ToolDownloader::new(Arc::new(egress), Config::default()))
    }

    /// Runs the job body: download into `staging` (a folder this job owns), check, hand to `sink`, delete the file.
    ///
    /// # Errors
    /// [`ToolError`], with the staging file already removed.
    pub fn run(&self, ctx: &JobContext, pin: &ToolPin, staging: &Path, sink: &dyn ToolSink) -> Result<ToolStatus, ToolError> {
        let dest = staging.join(&pin.file);
        let result = self.fetch_and_install(ctx, pin, &dest, sink);
        let _ = std::fs::remove_file(&dest);
        let _ = std::fs::remove_file(dest.with_extension("part"));
        match &result {
            Ok(_) => log::info!(target: "dyno::tool", "tool installed: tool={:?} version={}", pin.tool, pin.version),
            Err(e) => log::warn!(target: "dyno::tool", "tool not installed: tool={:?} kind={}", pin.tool, kind(e)),
        }
        result
    }

    fn fetch_and_install(&self, ctx: &JobContext, pin: &ToolPin, dest: &Path, sink: &dyn ToolSink) -> Result<ToolStatus, ToolError> {
        ctx.check().map_err(|_| ToolError::Cancelled)?;
        std::fs::create_dir_all(dest.parent().unwrap_or(Path::new(".")))
            .map_err(|e| ToolError::Download(format!("The staging folder could not be made ({}).", e.kind())))?;
        let _ = std::fs::remove_file(dest);
        let digest = Sha256Digest::from_hex(&pin.sha256).map_err(|e| ToolError::Download(e.to_string()))?;
        let request = Request::new(&pin.url, dest, Integrity::Sha256(digest))
            .map_err(|e| ToolError::Download(e.to_string()))?
            .size(pin.size)
            .max_bytes(pin.size);
        // connect, download and verify (size and SHA-256 while writing) are the downloader's own phases.
        self.downloader.download(ctx, &request).map_err(map_download)?;

        ctx.phase(Phase::items(CHECK, "Checking the download", 1));
        ctx.check().map_err(|_| ToolError::Cancelled)?;
        let bytes = std::fs::read(dest).map_err(|e| ToolError::Download(format!("The download could not be read back ({}).", e.kind())))?;
        // The bytes handed over are checked once more: the file sat on disk between the download and now.
        if bytes.len() as u64 != pin.size {
            return Err(ToolError::NotThePinnedFile("its size differs"));
        }
        if hex(&Sha256::digest(&bytes)) != pin.sha256 {
            return Err(ToolError::NotThePinnedFile("its checksum differs"));
        }

        ctx.phase(Phase::items(INSTALL, "Installing it through the helper", 1));
        ctx.check().map_err(|_| ToolError::Cancelled)?;
        sink.install(pin.tool, bytes).map_err(|e| ToolError::Helper(e.to_string()))
    }
}

fn kind(e: &ToolError) -> &'static str {
    match e {
        ToolError::Download(_) => "download",
        ToolError::NotThePinnedFile(_) => "not-the-pinned-file",
        ToolError::Helper(_) => "helper",
        ToolError::Cancelled => "cancelled",
    }
}

/// What Settings offers for smartmontools right now.
pub fn offer(shared: &crate::agent::Shared) -> dyno_core::tool_offer::ToolOffer {
    use dyno_core::helper_wire::HelperState;
    let host = shared.helper.host();
    let pin = crate::tool_pins::pin_for(ToolKind::Smartmontools, host).map(|p| dyno_core::tool_offer::PinFacts {
        version: p.version.clone(),
        size: p.size,
        from: p.hosts.first().cloned().unwrap_or_default(),
        licence: p.licence.clone(),
        source_url: p.source_url.clone(),
    });
    let ready = shared.helper.status().state == HelperState::Ready;
    let from_helper = if ready {
        shared
            .helper
            .link()
            .tools()
            .into_iter()
            .find(|t| t.tool == ToolKind::Smartmontools)
            .and_then(|t| t.installed)
            .map(|version| InstalledTool {
                version,
                source: ToolSource::Helper,
            })
    } else {
        None
    };
    // The helper's copy is what a ready helper reads with; without one, the copy this account can run (Dyno's own, or a
    // person's own install) is what the local reader uses, so the card names that one.
    let installed = from_helper.or_else(|| local_copy(&shared.dirs.tools()));
    let offer = dyno_core::tool_offer::smartmontools_offer(host, pin, ready, installed);
    log::info!(
        target: "dyno::tools",
        "Tool offer: helper ready {ready}, state {:?}, source {:?}",
        offer.state,
        offer.source
    );
    offer
}

/// The smartctl this computer would read with on its own, and which kind of copy it is.
fn local_copy(own_tools: &std::path::Path) -> Option<InstalledTool> {
    let path = dyno_platform::hardware::locate_smartctl(own_tools)?;
    let version = dyno_platform::hardware::smartctl_version(&path).unwrap_or_else(|| "unknown".to_string());
    let source = if path.starts_with(own_tools) {
        ToolSource::Dyno
    } else {
        ToolSource::Own(path.to_string_lossy().into_owned())
    };
    Some(InstalledTool { version, source })
}

/// The job body of `InstallTool`: the pin of this system, the real door, the staging folder under the cache folder, the helper.
pub fn install(shared: &Arc<crate::agent::Shared>, ctx: &JobContext, tool: ToolKind) -> Result<ToolStatus, JobError> {
    use dyno_core::helper_wire::HelperState;
    let fail = |m: &str| JobError::Failed(m.to_string());
    ctx.phase(Phase::discovering("check", "Checking what to download"));
    ctx.check()?;
    let Some(pin) = crate::tool_pins::pin_for(tool, shared.helper.host()) else {
        return Err(fail(
            "There is no download for this tool on this system. Install it with your package manager.",
        ));
    };
    if pin.install == crate::tool_pins::InstallVia::OwnCopy {
        let downloader = ToolDownloader::real(&pin, Arc::new(paddock_jobs::time::SystemClock))?;
        let own = crate::tool_own_copy::OwnCopy::new(
            Arc::new(dyno_platform::runner::SystemRunner),
            shared.dirs.tool_staging().join("own-copy"),
            shared.dirs.tools(),
            pin.version.clone(),
        );
        let status = downloader.run(ctx, &pin, &shared.dirs.tool_staging(), &own)?;
        // The SSD wear reader looks again now instead of at its next slow cycle (hours away).
        crate::hardware::slow_cycle(shared);
        return Ok(status);
    }
    if shared.helper.status().state != HelperState::Ready {
        return Err(fail(
            "Turn on administrator readings first: the helper installs the tool. Then try again.",
        ));
    }
    let downloader = ToolDownloader::real(&pin, Arc::new(paddock_jobs::time::SystemClock))?;
    let staging = shared.dirs.tool_staging();
    let helper_version = shared.helper.status().version.unwrap_or_else(|| "unknown".into());
    log::info!(target: "dyno::tools", "Installing {tool:?} through the helper (helper version {helper_version})");
    let status = downloader.run(ctx, &pin, &staging, shared.helper.link())?;
    // The helper said it installed the tool; its own list must show it, or the card would go back to "Not installed" with no
    // message (an older helper answers an empty list).
    ensure_listed(&shared.helper.link().tools(), tool, &helper_version)?;
    log::info!(target: "dyno::tools", "Installed {tool:?} through the helper (helper version {helper_version}); reading disk health again");
    // The wear reader looks again now (through the helper), not at its next slow cycle (hours away).
    crate::hardware::request_reread(shared, Duration::ZERO);
    Ok(status)
}

/// Fails with the reason when the helper's list does not show `tool` after an install.
pub fn ensure_listed(tools: &[ToolStatus], tool: ToolKind, helper_version: &str) -> Result<(), JobError> {
    if tools.iter().any(|t| t.tool == tool && t.installed.is_some()) {
        return Ok(());
    }
    log::warn!(target: "dyno::tools", "The helper (version {helper_version}) installed {tool:?} but does not list it");
    Err(JobError::Failed(format!(
        "The helper (version {helper_version}) says it installed the tool but does not list it. Turn the helper off and on again in Settings to update it, then try again; send the log if it persists."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_helper_that_installed_but_does_not_list_the_tool_is_an_error_with_the_next_step() {
        let listed = [ToolStatus {
            tool: ToolKind::Smartmontools,
            installed: Some("7.5".into()),
        }];
        assert!(ensure_listed(&listed, ToolKind::Smartmontools, "1.0.0").is_ok());
        for empty in [
            &[][..],
            &[ToolStatus {
                tool: ToolKind::Smartmontools,
                installed: None,
            }][..],
        ] {
            let e = ensure_listed(empty, ToolKind::Smartmontools, "1.0.0").unwrap_err().to_string();
            assert!(
                e.contains("1.0.0") && e.contains("does not list it") && e.contains("try again"),
                "{e}"
            );
        }
    }
    use dyno_core::helper::HelperHost;
    use paddock_jobs::time::FakeClock;
    use paddock_jobs::{EventSink, JobRegistry, JobSnapshot};
    use paddock_net::{FakeWire, Reply, StaticLookup};
    use std::sync::Mutex;

    struct Null;
    impl EventSink for Null {
        fn emit(&self, _: &JobSnapshot) {}
    }
    fn ctx() -> JobContext {
        JobRegistry::new(Arc::new(Null)).register("test", "test", None)
    }

    /// A pin for bytes the test makes up (the real file is never downloaded in a test).
    fn pin_for_bytes(bytes: &[u8]) -> ToolPin {
        ToolPin {
            tool: ToolKind::Smartmontools,
            version: "7.5".into(),
            host: HelperHost::Windows,
            file: "tool-setup.exe".into(),
            url: "https://github.com/example/tool/releases/download/v1/tool-setup.exe".into(),
            size: bytes.len() as u64,
            sha256: hex(&Sha256::digest(bytes)),
            hosts: vec!["github.com".into(), "objects.githubusercontent.com".into()],
            licence: "GPL-2.0-or-later".into(),
            source_url: "https://example.org".into(),
            verified: "2026-11-03".into(),
            install: crate::tool_pins::InstallVia::Helper,
        }
    }

    struct Rig {
        d: ToolDownloader,
        wire: Arc<FakeWire>,
        dir: tempfile::TempDir,
    }

    fn rig(pin: &ToolPin, replies: Vec<Reply>) -> Rig {
        let mut wire = FakeWire::new();
        for r in replies {
            wire = wire.then(r);
        }
        let wire = Arc::new(wire);
        let lookup = Arc::new(
            StaticLookup::new()
                .with("github.com", &["140.82.112.3"])
                .with("objects.githubusercontent.com", &["185.199.108.133"]),
        );
        let egress = Egress::new(policy(pin), wire.clone(), lookup, Arc::new(FakeClock::new()));
        let config = Config {
            attempts: 1,
            max_throttles: 1,
            backoff: Duration::from_millis(1),
            max_wait: Duration::from_millis(5),
            ..Config::default()
        };
        Rig {
            d: ToolDownloader::new(Arc::new(egress), config),
            wire,
            dir: tempfile::tempdir().unwrap(),
        }
    }

    #[derive(Default)]
    struct Sink {
        got: Mutex<Vec<Vec<u8>>>,
        refuse: bool,
    }
    impl ToolSink for Sink {
        fn install(&self, tool: ToolKind, bytes: Vec<u8>) -> Result<ToolStatus, PlatformError> {
            if self.refuse {
                return Err(PlatformError::Failed("The helper refused the package.".into()));
            }
            self.got.lock().unwrap().push(bytes);
            Ok(ToolStatus {
                tool,
                installed: Some("7.5".into()),
            })
        }
    }

    fn staging_is_empty(r: &Rig) -> bool {
        std::fs::read_dir(r.dir.path()).map(|d| d.count() == 0).unwrap_or(true)
    }

    #[test]
    fn the_pinned_bytes_are_downloaded_checked_handed_to_the_helper_and_removed() {
        let bytes: Vec<u8> = (0..4000u32).map(|i| (i % 251) as u8).collect();
        let pin = pin_for_bytes(&bytes);
        let r = rig(&pin, vec![Reply::ok(&bytes)]);
        let sink = Sink::default();
        let st = r.d.run(&ctx(), &pin, r.dir.path(), &sink).unwrap();
        assert_eq!(st.installed.as_deref(), Some("7.5"));
        assert_eq!(sink.got.lock().unwrap().as_slice(), [bytes]);
        assert!(staging_is_empty(&r), "no installer is left behind");
        assert_eq!(r.wire.sends(), 1);
        assert!(r.wire.seen()[0].url.starts_with("https://github.com/"));
    }

    #[test]
    fn a_file_that_is_not_the_pinned_one_is_never_handed_over() {
        let good: Vec<u8> = vec![7; 3000];
        let pin = pin_for_bytes(&good);
        for (name, body) in [
            ("other bytes of the same size", vec![8u8; 3000]),
            ("one byte short", vec![7u8; 2999]),
            ("one byte over", vec![7u8; 3001]),
            ("empty", vec![]),
        ] {
            let r = rig(&pin, vec![Reply::ok(&body)]);
            let sink = Sink::default();
            let e = r.d.run(&ctx(), &pin, r.dir.path(), &sink).unwrap_err();
            assert!(
                matches!(e, ToolError::NotThePinnedFile(_)) || matches!(e, ToolError::Download(_)),
                "{name}: {e:?}"
            );
            assert!(sink.got.lock().unwrap().is_empty(), "{name}: nothing reached the helper");
            assert!(staging_is_empty(&r), "{name}: nothing is left on disk");
        }
    }

    #[test]
    fn an_http_error_a_dead_host_and_a_redirect_off_the_list_fail_with_a_next_step() {
        let bytes = vec![1u8; 1000];
        let pin = pin_for_bytes(&bytes);
        let r = rig(&pin, vec![Reply::status(404)]);
        let e = r.d.run(&ctx(), &pin, r.dir.path(), &Sink::default()).unwrap_err();
        assert!(e.to_string().contains("404") && e.to_string().contains("Try again later"), "{e}");
        let r = rig(&pin, vec![Reply::redirect(302, "https://evil.example/tool-setup.exe")]);
        let e = r.d.run(&ctx(), &pin, r.dir.path(), &Sink::default()).unwrap_err();
        assert!(matches!(e, ToolError::Download(_)), "{e:?}");
        assert!(e.to_string().contains("Check the network"), "{e}");
        assert_eq!(r.wire.sends(), 1, "the off-list host was not contacted");
    }

    #[test]
    fn a_helper_that_refuses_is_reported_and_the_file_is_still_removed() {
        let bytes = vec![5u8; 2000];
        let pin = pin_for_bytes(&bytes);
        let r = rig(&pin, vec![Reply::ok(&bytes)]);
        let e =
            r.d.run(
                &ctx(),
                &pin,
                r.dir.path(),
                &Sink {
                    refuse: true,
                    ..Sink::default()
                },
            )
            .unwrap_err();
        assert_eq!(e, ToolError::Helper("The helper refused the package.".into()));
        assert!(staging_is_empty(&r));
    }

    #[test]
    fn a_stop_before_the_download_and_before_the_hand_over_changes_nothing() {
        let bytes = vec![9u8; 1500];
        let pin = pin_for_bytes(&bytes);
        let r = rig(&pin, vec![Reply::ok(&bytes)]);
        let job = ctx();
        job.cancel_token().cancel();
        let sink = Sink::default();
        assert_eq!(r.d.run(&job, &pin, r.dir.path(), &sink).unwrap_err(), ToolError::Cancelled);
        assert_eq!(r.wire.sends(), 0, "no request was made after the stop");
        assert!(sink.got.lock().unwrap().is_empty());
        assert!(staging_is_empty(&r));
        assert!(matches!(JobError::from(ToolError::Cancelled), JobError::Cancelled(_)));
    }

    #[test]
    fn the_policy_pins_the_hosts_https_and_the_size() {
        let pin = pin_for_bytes(&[1; 10]);
        let p = policy(&pin);
        assert!(p.host_allowed("github.com") && p.host_allowed("objects.githubusercontent.com"));
        assert!(!p.host_allowed("evil.example") && !p.host_allowed("raw.githubusercontent.com"));
        assert_eq!(p.max_body, 10);
        assert!(p.user_agent.starts_with("Dyno/"));
    }

    #[test]
    fn the_real_pin_downloads_nothing_in_a_test_and_its_policy_allows_only_github() {
        let pin = crate::tool_pins::pin_for(ToolKind::Smartmontools, HelperHost::Windows).unwrap();
        let p = policy(&pin);
        assert!(p.host_allowed("github.com") && p.host_allowed("release-assets.githubusercontent.com"));
        assert_eq!(p.max_body, 1_508_408);
    }
}
