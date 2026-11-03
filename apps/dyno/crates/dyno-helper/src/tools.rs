//! The helper's tool side (ADR 0027 §7): the pinned installer over a staging folder, the smartctl reader, and the choice of
//! both per system. The pure parts (pins, the check, the command line) are `dyno_platform::tool_install`; this file adds the
//! one-install-at-a-time rule and the real staging folder.

use dyno_core::helper::HelperHost;
use dyno_core::helper_policy::InstallGuard;
use dyno_core::helper_requests::ToolKind;
use dyno_core::helper_wire::ToolStatus;
use dyno_core::ports::PlatformError;
use dyno_core::smart::{SmartReader, ToolInstaller};
use dyno_platform::runner::{SystemRunner, Tool, ToolOutput, ToolRunner};
use dyno_platform::smart_reader::SmartctlReader;
use dyno_platform::tool_install::{install, InstallPorts, InstallRoots};
use paddock_jobs::time::Clock;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Installs the pinned tool packages, one at a time whoever asks.
pub struct PinnedInstaller {
    host: HelperHost,
    roots: InstallRoots,
    ports: Mutex<Box<dyn InstallPorts>>,
    clock: Arc<dyn Clock>,
    guard: InstallGuard,
}

impl PinnedInstaller {
    /// An installer over `ports`.
    pub fn new(host: HelperHost, roots: InstallRoots, ports: Box<dyn InstallPorts>, clock: Arc<dyn Clock>) -> Self {
        PinnedInstaller {
            host,
            roots,
            ports: Mutex::new(ports),
            clock,
            guard: InstallGuard::new(),
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
        let mut ports = self.ports.lock().unwrap_or_else(|p| p.into_inner());
        install(ports.as_mut(), &self.clock, self.host, tool, bytes, &self.roots).map_err(PlatformError::from)
    }
}

/// The real staging folder: files are created new (never over something that is there), and the folder is emptied after.
pub struct FsPorts {
    staging: PathBuf,
    runner: Box<dyn ToolRunner>,
}

impl FsPorts {
    /// Ports over `staging`, running programs through `runner`.
    pub fn new(staging: PathBuf, runner: Box<dyn ToolRunner>) -> Self {
        FsPorts { staging, runner }
    }

    /// The real runner.
    pub fn system(staging: PathBuf) -> Self {
        FsPorts::new(staging, Box::new(SystemRunner))
    }
}

impl InstallPorts for FsPorts {
    fn create_staged(&mut self, name: &str) -> std::io::Result<Box<dyn Write>> {
        // The name is the pin's constant file name; a separator would be a bug, and is refused all the same.
        if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
        }
        std::fs::create_dir_all(&self.staging)?;
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
        let _ = std::fs::remove_dir_all(&self.staging);
    }
}

/// The smartctl reader of this system: the helper's own copy on Windows (`InstallTool` put it there), the distribution's on
/// Linux. macOS reads SMART in the agent (WP-0), so there the helper says so.
pub fn smart_reader(host: HelperHost, tools_dir: &std::path::Path, clock: Arc<dyn Clock>) -> Box<dyn SmartReader> {
    match host {
        HelperHost::Windows => {
            let path = tools_dir.join("smartmontools-7.5").join("bin").join("smartctl.exe");
            Box::new(SmartctlReader::new(
                path.to_string_lossy().into_owned(),
                Box::new(SystemRunner),
                clock,
            ))
        }
        HelperHost::Linux => Box::new(SmartctlReader::new("/usr/sbin/smartctl", Box::new(SystemRunner), clock)),
        HelperHost::Macos => Box::new(NotHere(
            "On macOS Dyno reads SMART with its own copy of smartctl; the helper does not.",
        )),
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
pub fn installer(host: HelperHost, data_dir: &std::path::Path, clock: Arc<dyn Clock>) -> Box<dyn ToolInstaller> {
    let tools = data_dir.join("tools");
    let staging = data_dir.join("staging");
    let roots = InstallRoots {
        staging: staging.to_string_lossy().into_owned(),
        tools: tools.to_string_lossy().into_owned(),
    };
    Box::new(PinnedInstaller::new(host, roots, Box::new(FsPorts::system(staging)), clock))
}

/// A reading this system does not have.
pub(crate) struct NotHere(pub(crate) &'static str);

impl SmartReader for NotHere {
    fn read(&self, _query: dyno_core::helper_requests::SmartQuery) -> Result<dyno_core::smart::SmartReply, PlatformError> {
        Err(PlatformError::Unsupported(self.0.into()))
    }
}

impl ToolInstaller for NotHere {
    fn install(&self, _tool: ToolKind, _bytes: &[u8]) -> Result<ToolStatus, PlatformError> {
        Err(PlatformError::Unsupported(self.0.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    }

    fn roots() -> InstallRoots {
        InstallRoots {
            staging: "C:\\s".into(),
            tools: "C:\\t".into(),
        }
    }

    #[test]
    fn a_package_that_is_not_the_pinned_one_is_refused_before_anything_runs() {
        let i = PinnedInstaller::new(HelperHost::Windows, roots(), Box::new(Fake), Arc::new(FakeClock::default()));
        let e = i.install(ToolKind::Smartmontools, b"short").unwrap_err();
        assert!(e.to_string().contains("expects"), "{e}");
        let linux = PinnedInstaller::new(HelperHost::Linux, roots(), Box::new(Fake), Arc::new(FakeClock::default()));
        assert!(matches!(
            linux.install(ToolKind::Smartmontools, b"x").unwrap_err(),
            PlatformError::Unsupported(_)
        ));
    }

    #[test]
    fn a_second_install_while_one_runs_is_turned_away() {
        let i = PinnedInstaller::new(HelperHost::Windows, roots(), Box::new(Fake), Arc::new(FakeClock::default()));
        let _held = i.guard.try_begin().unwrap();
        assert!(i
            .install(ToolKind::Smartmontools, b"x")
            .unwrap_err()
            .to_string()
            .contains("Another tool"));
    }

    #[test]
    fn the_real_staging_folder_creates_new_files_only_and_is_emptied() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        let mut ports = FsPorts::system(staging.clone());
        ports.create_staged("setup.exe").unwrap().write_all(b"x").unwrap();
        assert_eq!(
            ports.create_staged("setup.exe").err().unwrap().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        for bad in ["", "a/b", "a\\b", ".hidden"] {
            assert_eq!(ports.create_staged(bad).err().unwrap().kind(), std::io::ErrorKind::InvalidInput);
        }
        assert!(ports.exists(&staging.join("setup.exe").to_string_lossy()));
        assert!(!ports.exists(&staging.to_string_lossy()), "a folder is not a program");
        ports.clear_staging();
        assert!(!staging.exists());
    }

    #[test]
    fn macos_leaves_smart_and_installs_to_the_agent_and_the_others_get_a_reader() {
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::default());
        let mac = smart_reader(HelperHost::Macos, std::path::Path::new("/t"), clock.clone());
        assert!(matches!(
            mac.read(SmartQuery::default()).unwrap_err(),
            PlatformError::Unsupported(_)
        ));
        let mac_installer = installer(HelperHost::Macos, std::path::Path::new("/nonexistent-dyno-test"), clock.clone());
        assert!(matches!(
            mac_installer.install(ToolKind::Smartmontools, b"x").unwrap_err(),
            PlatformError::Unsupported(_)
        ));
        assert!(NotHere("x").install(ToolKind::Smartmontools, b"").is_err());
        let _ = smart_reader(HelperHost::Windows, std::path::Path::new("C:\\t"), clock.clone());
        let _ = smart_reader(HelperHost::Linux, std::path::Path::new("/t"), clock);
    }
}
