//! The helper's tool side (ADR 0027 §7): the pinned installer over a staging folder, the smartctl reader, and the choice of
//! both per system. The pure parts (pins, the check, the command line) are `dyno_platform::tool_install`; this file adds the
//! one-install-at-a-time rule and the real staging folder.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use crate::protect::{GuardedRunner, OnUntrusted, TrustGuard};
use dyno_core::helper::HelperHost;
use dyno_core::helper_policy::InstallGuard;
use dyno_core::helper_requests::ToolKind;
use dyno_core::helper_wire::ToolStatus;
use dyno_core::ports::PlatformError;
use dyno_core::smart::{SmartReader, ToolInstaller};
use dyno_platform::runner::{SystemRunner, Tool, ToolOutput, ToolRunner};
use dyno_platform::smart_reader::SmartctlReader;
use dyno_platform::tool_install::{install, smartctl_path, InstallPorts, InstallRoots, PINS};
use paddock_jobs::time::Clock;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Installs the pinned tool packages, one at a time whoever asks.
pub struct PinnedInstaller {
    host: HelperHost,
    roots: InstallRoots,
    ports: Mutex<Box<dyn InstallPorts>>,
    clock: Arc<dyn Clock>,
    guard: InstallGuard,
    trust: Arc<TrustGuard>,
}

impl PinnedInstaller {
    /// An installer over `ports`.
    pub fn new(host: HelperHost, roots: InstallRoots, ports: Box<dyn InstallPorts>, clock: Arc<dyn Clock>, trust: Arc<TrustGuard>) -> Self {
        PinnedInstaller {
            host,
            roots,
            ports: Mutex::new(ports),
            clock,
            guard: InstallGuard::new(),
            trust,
        }
    }
}

impl ToolInstaller for PinnedInstaller {
    fn install(&self, tool: ToolKind, bytes: &[u8]) -> Result<ToolStatus, PlatformError> {
        let Some(_ticket) = self.guard.try_begin() else {
            return Err(PlatformError::Failed(
                "Another tool is being installed; try again in a minute.".into(),
            ));
        };
        // Before anything is written or run as SYSTEM: the tools and staging folders are locked and checked (an existing
        // one that is not protected is emptied and made again), or the install is refused with the next step.
        // (A system with no pinned package for the tool says so from `install`, and locks nothing.)
        let pinned = dyno_platform::tool_install::pin_for(tool, self.host).is_some();
        if !pinned {
            let mut ports = self.ports.lock().unwrap_or_else(|p| p.into_inner());
            return install(ports.as_mut(), &self.clock, self.host, tool, bytes, &self.roots).map_err(PlatformError::from);
        }
        if let Err(e) = self.trust.ensure_root(Path::new(&self.roots.tools), OnUntrusted::Purge) {
            log::warn!(Error = e.to_string().as_str(); "InstallTool refused: the tools folder is not protected");
            return Err(e.into());
        }
        if let Err(e) = self.trust.reset_root(Path::new(&self.roots.staging)) {
            log::warn!(Error = e.to_string().as_str(); "InstallTool refused: the staging folder is not protected");
            return Err(e.into());
        }
        let mut ports = self.ports.lock().unwrap_or_else(|p| p.into_inner());
        install(ports.as_mut(), &self.clock, self.host, tool, bytes, &self.roots).map_err(PlatformError::from)
    }

    fn installed(&self) -> Vec<ToolStatus> {
        let tools = std::path::Path::new(&self.roots.tools);
        PINS.iter()
            .filter(|p| p.host == self.host)
            .filter(|p| {
                let path = smartctl_path(tools, p.version);
                let found = path.is_file();
                log::info!(Smartctl = path.to_string_lossy().as_ref(), Found = found; "Helper: looked for an installed tool");
                found
            })
            .map(|p| ToolStatus {
                tool: p.tool,
                installed: Some(p.version.to_string()),
            })
            .collect()
    }
}

/// The real staging folder: files are created new (never over something that is there), and the folder is emptied after.
pub struct FsPorts {
    staging: PathBuf,
    runner: Box<dyn ToolRunner>,
    trust: Arc<TrustGuard>,
}

impl FsPorts {
    /// Ports over `staging`, running programs through `runner`.
    pub fn new(staging: PathBuf, runner: Box<dyn ToolRunner>, trust: Arc<TrustGuard>) -> Self {
        FsPorts { staging, runner, trust }
    }

    /// The real runner, which runs only a checked and held program from `staging`.
    pub fn system(staging: PathBuf, trust: Arc<TrustGuard>) -> Self {
        let runner = GuardedRunner::new(trust.clone(), Some(staging.clone()), Box::new(SystemRunner));
        FsPorts::new(staging, Box::new(runner), trust)
    }
}

impl InstallPorts for FsPorts {
    fn create_staged(&mut self, name: &str) -> std::io::Result<Box<dyn Write>> {
        // The name is the pin's constant file name; a separator would be a bug, and is refused all the same.
        if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
        }
        // The staging folder is removed after every install: made again locked, never as an inheriting folder.
        self.trust
            .ensure_root(&self.staging, OnUntrusted::Purge)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e.to_string()))?;
        // A leftover of an earlier run (a package its installer still held when the folder was cleared) is removed, links
        // and folders without being followed, so a second install does not fail on "already exists".
        remove_entry_no_follow(&self.staging.join(name))?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.staging.join(name))?;
        Ok(Box::new(file))
    }

    fn run(&mut self, tool: &Tool, timeout: Duration) -> Result<ToolOutput, PlatformError> {
        self.runner.run_tool(tool, timeout)
    }

    fn exists(&self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }

    fn clear_staging(&mut self) {
        match std::fs::remove_dir_all(&self.staging) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!(error = e.to_string().as_str(); "Helper: the tool staging folder could not be cleared"),
        }
    }

    fn remove_dir(&mut self, path: &str) -> std::io::Result<()> {
        // `path` is the pin's own folder under the helper's tools folder, built by `install`; never a caller's text. Links
        // in it are removed, never followed.
        remove_entry_no_follow(Path::new(path))
    }
}

/// Removes `path` if there is something: a file, or a folder or link without following it.
fn remove_entry_no_follow(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path)),
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// The smartctl reader of this system: the helper's own copy on Windows (`InstallTool` put it there), the distribution's on
/// Linux. macOS reads SMART in the agent (WP-0), so there the helper says so.
pub fn smart_reader(host: HelperHost, tools_dir: &std::path::Path, clock: Arc<dyn Clock>, trust: Arc<TrustGuard>) -> Box<dyn SmartReader> {
    match host {
        HelperHost::Windows => {
            let version = dyno_platform::tool_install::pin_for(ToolKind::Smartmontools, host).map_or("", |p| p.version);
            let path = smartctl_path(tools_dir, version);
            Box::new(SmartctlReader::new(
                path.to_string_lossy().into_owned(),
                Box::new(GuardedRunner::new(trust, Some(tools_dir.to_path_buf()), Box::new(SystemRunner))),
                clock,
            ))
        }
        HelperHost::Linux => Box::new(SmartctlReader::new(
            "/usr/sbin/smartctl",
            Box::new(GuardedRunner::new(trust, None, Box::new(SystemRunner))),
            clock,
        )),
        HelperHost::Macos => Box::new(NotHere(
            "On macOS Dyno reads SMART with its own copy of smartctl; the helper does not.",
        )),
    }
}

/// The firewall reader of this system: `pf` on macOS, nftables on Linux (both as root, fixed arguments). Windows Defender Firewall
/// is read by the agent itself, so there the helper says so.
pub fn firewall_reader(host: HelperHost, clock: Arc<dyn Clock>) -> Box<dyn dyno_core::firewall::FirewallReader> {
    match host {
        HelperHost::Macos => Box::new(dyno_platform::firewall_readers::PfReader::new(Box::new(SystemRunner), clock)),
        HelperHost::Linux => Box::new(dyno_platform::firewall_readers::NftReader::new(
            "/usr/sbin/nft",
            Box::new(SystemRunner),
            clock,
        )),
        HelperHost::Windows => Box::new(NotHere(
            "The agent reads Windows Defender Firewall itself; the helper has nothing to add.",
        )),
    }
}

/// Who holds which socket or file: Linux reads `/proc` as root.
pub fn holder_source(host: HelperHost) -> Box<dyn dyno_core::helper_holders::HolderSource> {
    match host {
        #[cfg(unix)]
        HelperHost::Linux => Box::new(dyno_platform::helper_holders::ProcHolders::new(std::time::Duration::from_secs(20))),
        _ => Box::new(dyno_core::helper_holders::NoHolders),
    }
}

/// The Windows extras reader (`powercfg`, `wbadmin`); nothing elsewhere.
pub fn extras_reader(host: HelperHost) -> Box<dyn dyno_core::helper_extras::ExtrasReader> {
    match host {
        HelperHost::Windows => Box::new(dyno_platform::helper_extras::WindowsExtras(Box::new(SystemRunner))),
        _ => Box::new(dyno_core::helper_extras::NoExtras),
    }
}

/// The sensor reader of this system.
pub fn counter_reader(host: HelperHost) -> Box<dyn dyno_core::helper_counters::CounterReader> {
    Box::new(dyno_platform::helper_counters::SystemCounters::new(host, Box::new(SystemRunner)))
}

/// The process sweep of this system: `sysinfo` plus the OS's own account list.
pub fn process_sweeper(host: HelperHost) -> Box<dyn dyno_core::helper_processes::ProcessSweeper> {
    Box::new(dyno_platform::helper_sweep::SystemSweeper::new(host, Box::new(SystemRunner)))
}

/// The scheduled-wake handler of this system (Dyno ADR 0032 D2): systemd timers on Linux, `pmset` on macOS; Windows schedules in the agent.
pub fn wake_scheduler(host: HelperHost) -> Box<dyn dyno_core::wake::WakeScheduler> {
    match host {
        HelperHost::Linux => Box::new(dyno_platform::wakes::SystemdWakes(Box::new(SystemRunner))),
        HelperHost::Macos => Box::new(dyno_platform::wakes::PmsetWakes::new(
            Box::new(SystemRunner),
            Box::new(dyno_platform::wakes::system_offset_at),
        )),
        HelperHost::Windows => Box::new(dyno_core::wake::NoWakes),
    }
}

/// The wake-on-LAN reader of this system (Linux only).
pub fn wol_reader(host: HelperHost) -> Box<dyn dyno_core::wake::WolReader> {
    match host {
        HelperHost::Linux => Box::new(dyno_platform::wakes::EthtoolWol(Box::new(SystemRunner))),
        _ => Box::new(dyno_core::wake::NoWakes),
    }
}

/// The installer of this system.
pub fn installer(host: HelperHost, data_dir: &std::path::Path, clock: Arc<dyn Clock>, trust: Arc<TrustGuard>) -> Box<dyn ToolInstaller> {
    let tools = data_dir.join("tools");
    let staging = data_dir.join("staging");
    let roots = InstallRoots {
        staging: staging.to_string_lossy().into_owned(),
        tools: tools.to_string_lossy().into_owned(),
    };
    Box::new(PinnedInstaller::new(
        host,
        roots,
        Box::new(FsPorts::system(staging, trust.clone())),
        clock,
        trust,
    ))
}

/// A reading this system does not have.
pub(crate) struct NotHere(pub(crate) &'static str);

impl SmartReader for NotHere {
    fn read(&self, _query: dyno_core::helper_requests::SmartQuery) -> Result<dyno_core::smart::SmartReply, PlatformError> {
        Err(PlatformError::Unsupported(self.0.into()))
    }
}

impl dyno_core::firewall::FirewallReader for NotHere {
    fn read(&self) -> Result<dyno_core::firewall::FirewallSource, PlatformError> {
        Err(PlatformError::Unsupported(self.0.into()))
    }
}

impl ToolInstaller for NotHere {
    fn install(&self, _tool: ToolKind, _bytes: &[u8]) -> Result<ToolStatus, PlatformError> {
        Err(PlatformError::Unsupported(self.0.into()))
    }

    fn installed(&self) -> Vec<ToolStatus> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protect::tests::trusting;
    use dyno_core::helper_requests::SmartQuery;
    use paddock_jobs::time::FakeClock;

    struct Fake;
    impl InstallPorts for Fake {
        fn create_staged(&mut self, _: &str) -> std::io::Result<Box<dyn Write>> {
            Ok(Box::new(std::io::sink()))
        }
        fn run(&mut self, _: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
            Ok(ToolOutput {
                ok: true,
                stdout: String::new(),
                error: String::new(),
            })
        }
        fn exists(&self, _: &str) -> bool {
            true
        }
        fn clear_staging(&mut self) {}
        fn remove_dir(&mut self, _: &str) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn roots() -> InstallRoots {
        InstallRoots {
            staging: "/h/staging".into(),
            tools: "/h/tools".into(),
        }
    }

    #[test]
    fn a_package_that_is_not_the_pinned_one_is_refused_before_anything_runs() {
        let i = PinnedInstaller::new(
            HelperHost::Windows,
            roots(),
            Box::new(Fake),
            Arc::new(FakeClock::default()),
            trusting(),
        );
        let e = i.install(ToolKind::Smartmontools, b"short").unwrap_err();
        assert!(e.to_string().contains("expects"), "{e}");
        let linux = PinnedInstaller::new(
            HelperHost::Linux,
            roots(),
            Box::new(Fake),
            Arc::new(FakeClock::default()),
            trusting(),
        );
        assert!(matches!(
            linux.install(ToolKind::Smartmontools, b"x").unwrap_err(),
            PlatformError::Unsupported(_)
        ));
    }

    #[test]
    fn the_tools_list_comes_from_the_tools_folder() {
        let dir = tempfile::tempdir().unwrap();
        let tools = dir.path().join("tools");
        let roots = InstallRoots {
            staging: dir.path().join("staging").to_string_lossy().into_owned(),
            tools: tools.to_string_lossy().into_owned(),
        };
        let i = PinnedInstaller::new(
            HelperHost::Windows,
            roots,
            Box::new(Fake),
            Arc::new(FakeClock::default()),
            trusting(),
        );
        assert!(i.installed().is_empty(), "nothing in the folder is nothing installed");
        std::fs::create_dir_all(tools.join("smartmontools-7.5").join("bin")).unwrap();
        assert!(i.installed().is_empty(), "a folder without smartctl.exe is not an install");
        let expected = smartctl_path(&tools, "7.5");
        std::fs::create_dir_all(expected.parent().unwrap()).unwrap();
        std::fs::write(&expected, b"x").unwrap();
        assert_eq!(
            i.installed(),
            vec![ToolStatus {
                tool: ToolKind::Smartmontools,
                installed: Some("7.5".into())
            }]
        );
        assert!(NotHere("x").installed().is_empty());
    }

    #[test]
    fn a_second_install_while_one_runs_is_turned_away() {
        let i = PinnedInstaller::new(
            HelperHost::Windows,
            roots(),
            Box::new(Fake),
            Arc::new(FakeClock::default()),
            trusting(),
        );
        let _held = i.guard.try_begin().unwrap();
        assert!(i
            .install(ToolKind::Smartmontools, b"x")
            .unwrap_err()
            .to_string()
            .contains("Another tool"));
    }

    // Unix only: `trusting()` is a Unix-style fake trust port rooted at `/`, which no Windows temporary folder is below; the
    // Windows lock of a staging folder is tested by the real trust port in `protect`.
    #[cfg(unix)]
    #[test]
    fn the_real_staging_folder_creates_new_files_only_and_is_emptied() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        let mut ports = FsPorts::system(staging.clone(), trusting());
        ports.create_staged("setup.exe").unwrap().write_all(b"x").unwrap();
        // A leftover of an earlier run is replaced, not an error: a second install must not fail on "already exists".
        ports.create_staged("setup.exe").unwrap().write_all(b"y").unwrap();
        assert_eq!(std::fs::read(staging.join("setup.exe")).unwrap(), b"y");
        for bad in ["", "a/b", "a\\b", ".hidden"] {
            assert_eq!(ports.create_staged(bad).err().unwrap().kind(), std::io::ErrorKind::InvalidInput);
        }
        assert!(ports.exists(&staging.join("setup.exe").to_string_lossy()));
        assert!(!ports.exists(&staging.to_string_lossy()), "a folder is not a program");
        ports.clear_staging();
        assert!(!staging.exists());
    }

    // Unix only: `trusting()` is a Unix-style fake trust port rooted at `/`, which no Windows temporary folder is below; the
    // Windows lock of a staging folder is tested by the real trust port in `protect`.
    #[cfg(unix)]
    #[test]
    fn a_second_install_works_when_the_first_left_its_package_behind() {
        // The VM: the first install's staging could not be cleared (its installer still held the file), and the next attempt
        // failed with Staging("entity already exists").
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        let pin = dyno_platform::tool_install::pin_for(ToolKind::Smartmontools, HelperHost::Windows).unwrap();
        std::fs::write(staging.join(pin.file), b"left over").unwrap();
        std::fs::create_dir_all(staging.join("left-over-folder")).unwrap();
        let mut ports = FsPorts::system(staging.clone(), trusting());
        ports.create_staged(pin.file).unwrap().write_all(b"fresh").unwrap();
        assert_eq!(std::fs::read(staging.join(pin.file)).unwrap(), b"fresh");
    }

    #[test]
    fn macos_leaves_smart_and_installs_to_the_agent_and_the_others_get_a_reader() {
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::default());
        let mac = smart_reader(HelperHost::Macos, std::path::Path::new("/t"), clock.clone(), trusting());
        assert!(matches!(
            mac.read(SmartQuery::default()).unwrap_err(),
            PlatformError::Unsupported(_)
        ));
        let mac_installer = installer(
            HelperHost::Macos,
            std::path::Path::new("/nonexistent-dyno-test"),
            clock.clone(),
            trusting(),
        );
        assert!(matches!(
            mac_installer.install(ToolKind::Smartmontools, b"x").unwrap_err(),
            PlatformError::Unsupported(_)
        ));
        assert!(NotHere("x").install(ToolKind::Smartmontools, b"").is_err());
        let _ = smart_reader(HelperHost::Windows, std::path::Path::new("C:\\t"), clock.clone(), trusting());
        let _ = smart_reader(HelperHost::Linux, std::path::Path::new("/t"), clock, trusting());
    }
}
