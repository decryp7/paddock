//! `InstallTool` for the helper (ADR 0027 section 7, WP-8): the pin table, and the installer over a small port. The helper never
//! trusts the agent's catalogue: the bytes arrive over the connection, are written create-new into a staging folder while they
//! are hashed, must match the size and SHA-256 compiled in here, and only then does the installer run, with `TEMP`/`TMP` inside
//! the staging folder. Windows only (decision O2); macOS SMART runs in the agent, and there is no upstream Linux binary.
//!
//! The pins are recorded with their evidence in `crates/dyno-helper/tools/smartmontools-7.5.pin.md`.

use crate::runner::{Tool, ToolOutput};
use dyno_core::helper::HelperHost;
use dyno_core::helper_requests::ToolKind;
use dyno_core::helper_wire::ToolStatus;
use dyno_core::ports::PlatformError;
use paddock_jobs::time::Clock;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// How long the NSIS installer may run.
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(120);

/// One pinned upstream package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pin {
    /// The tool.
    pub tool: ToolKind,
    /// Its version.
    pub version: &'static str,
    /// The system it installs on.
    pub host: HelperHost,
    /// The package file's name, used as the staging file (never a caller's name).
    pub file: &'static str,
    /// Exact size in bytes.
    pub size: u64,
    /// SHA-256, lower-case hex.
    pub sha256: &'static str,
}

/// The pins compiled into the helper. A new smartmontools reaches users only with a new helper.
pub const PINS: [Pin; 1] = [Pin {
    tool: ToolKind::Smartmontools,
    version: "7.5",
    host: HelperHost::Windows,
    file: "smartmontools-7.5.win32-setup.exe",
    size: 1_508_408,
    sha256: "896337fcc253220614cf8cdbd5cf2321c5aa326a37a04160a672a281e6104c70",
}];

/// The pin for `tool` on `host`.
pub fn pin_for(tool: ToolKind, host: HelperHost) -> Option<&'static Pin> {
    PINS.iter().find(|p| p.tool == tool && p.host == host)
}

/// The NSIS components that leave no `smartd` service, menu entry, uninstaller key or PATH change (WP-0, measured).
pub const WINDOWS_COMPONENTS: &str = "x64,smartctl,drivedb";

/// The folder the pinned `version` of smartmontools is installed into under the helper's `tools` folder. The installer, the
/// tools list in `Info` and the helper's SMART reader all use this one definition (rule 55).
pub fn install_folder(tools: &Path, version: &str) -> PathBuf {
    tools.join(format!("smartmontools-{version}"))
}

/// The `smartctl.exe` of the pinned `version` inside [`install_folder`].
pub fn smartctl_path(tools: &Path, version: &str) -> PathBuf {
    install_folder(tools, version).join("bin").join("smartctl.exe")
}

/// Where the install happens (the helper's protected folders).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRoots {
    /// The staging folder, `...\tools\.staging`.
    pub staging: String,
    /// The tools folder, `...\tools`.
    pub tools: String,
}

/// What the installer needs from the machine.
pub trait InstallPorts: Send {
    /// Creates `name` in the staging folder, failing if it exists (create-new), and returns a writer.
    fn create_staged(&mut self, name: &str) -> std::io::Result<Box<dyn Write>>;
    /// Runs a program to completion within `timeout`.
    fn run(&mut self, tool: &Tool, timeout: Duration) -> Result<ToolOutput, PlatformError>;
    /// Whether a file exists.
    fn exists(&self, path: &str) -> bool;
    /// Deletes the staging folder's contents.
    fn clear_staging(&mut self);
    /// Deletes the install folder `path` (and what is in it), so a file an earlier install left cannot pass for this one.
    fn remove_dir(&mut self, path: &str);
}

/// Why an install did not happen. Each says what to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// This system installs nothing through the helper.
    Unsupported(String),
    /// The package is not the size the helper has pinned.
    WrongSize {
        /// The bytes received.
        got: u64,
        /// The pinned size.
        want: u64,
    },
    /// The package's SHA-256 is not the pinned one.
    WrongHash,
    /// The staging file could not be written.
    Staging(String),
    /// The installer ran and failed.
    Installer(String),
}

impl From<InstallError> for PlatformError {
    fn from(e: InstallError) -> Self {
        match e {
            InstallError::Unsupported(m) => PlatformError::Unsupported(m),
            InstallError::WrongSize { got, want } => PlatformError::Failed(format!(
                "The package is {got} bytes but this helper expects {want}. Download it again; nothing was installed."
            )),
            InstallError::WrongHash => PlatformError::Failed(
                "The package does not match the checksum this helper expects. Download it again; nothing was installed.".into(),
            ),
            InstallError::Staging(m) => PlatformError::Failed(format!("The package could not be written: {m}. Nothing was installed.")),
            InstallError::Installer(m) => PlatformError::Failed(m),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The installer's command line: the NSIS options are `[/S] [/SO component,...] [/D=INSTDIR]`, and `/D=` must come last.
pub fn windows_command(setup: &str, dest: &str, staging: &str) -> Tool {
    Tool {
        program: setup.to_string(),
        args: vec!["/S".into(), "/SO".into(), WINDOWS_COMPONENTS.into(), format!("/D={dest}")],
        env: vec![("TEMP".into(), staging.to_string()), ("TMP".into(), staging.to_string())],
    }
}

/// Checks `bytes` against the pin, stages them, runs the installer and confirms `bin\smartctl.exe` is there. The staging folder
/// is cleared on every outcome.
pub fn install(
    ports: &mut dyn InstallPorts,
    clock: &Arc<dyn Clock>,
    host: HelperHost,
    tool: ToolKind,
    bytes: &[u8],
    roots: &InstallRoots,
) -> Result<ToolStatus, InstallError> {
    let started = clock.monotonic();
    let ms = |from: Duration| clock.monotonic().saturating_sub(from).as_millis() as u64;
    let Some(pin) = pin_for(tool, host) else {
        let why = match host {
            HelperHost::Macos => "On macOS Dyno reads SMART with its own copy of smartctl; the helper installs nothing.",
            HelperHost::Linux => "Install the smartmontools package of your distribution; Dyno then uses /usr/sbin/smartctl.",
            HelperHost::Windows => "This helper has no pinned package for that tool.",
        };
        log::warn!("InstallTool refused: no pin for this tool on this system");
        return Err(InstallError::Unsupported(why.into()));
    };
    if bytes.len() as u64 != pin.size {
        log::warn!(Got = bytes.len() as u64, Want = pin.size; "InstallTool refused: wrong package size");
        return Err(InstallError::WrongSize {
            got: bytes.len() as u64,
            want: pin.size,
        });
    }
    let result = stage_and_run(ports, clock, pin, bytes, roots);
    ports.clear_staging();
    match &result {
        Ok(_) => log::info!(Version = pin.version, Ms = ms(started); "InstallTool finished"),
        Err(e) => log::warn!(Error = format!("{e:?}").as_str(), Ms = ms(started); "InstallTool failed; staging cleared"),
    }
    result
}

fn stage_and_run(
    ports: &mut dyn InstallPorts,
    clock: &Arc<dyn Clock>,
    pin: &Pin,
    bytes: &[u8],
    roots: &InstallRoots,
) -> Result<ToolStatus, InstallError> {
    let phase = clock.monotonic();
    let mut writer = ports
        .create_staged(pin.file)
        .map_err(|e| InstallError::Staging(e.kind().to_string()))?;
    let mut hasher = Sha256::new();
    for chunk in bytes.chunks(64 * 1024) {
        hasher.update(chunk);
        writer.write_all(chunk).map_err(|e| InstallError::Staging(e.kind().to_string()))?;
    }
    writer.flush().map_err(|e| InstallError::Staging(e.kind().to_string()))?;
    drop(writer);
    if hex(&hasher.finalize()) != pin.sha256 {
        log::warn!("InstallTool refused: the package checksum does not match the pinned one");
        return Err(InstallError::WrongHash);
    }
    log::info!(Ms = clock.monotonic().saturating_sub(phase).as_millis() as u64; "InstallTool: package staged and checked");
    let dest = install_folder(Path::new(&roots.tools), pin.version).to_string_lossy().into_owned();
    let setup = format!("{}\\{}", roots.staging, pin.file);
    let cmd = windows_command(&setup, &dest, &roots.staging);
    ports.remove_dir(&dest);
    let run = clock.monotonic();
    let out = ports
        .run(&cmd, INSTALL_TIMEOUT)
        .map_err(|e| InstallError::Installer(format!("The installer did not finish: {e}")))?;
    log::info!(Ok = out.ok, Dest = dest.as_str(), Stderr = out.error.as_str(), Ms = clock.monotonic().saturating_sub(run).as_millis() as u64; "InstallTool: installer ran");
    if !out.ok {
        return Err(InstallError::Installer(format!(
            "The installer failed ({}). Nothing was kept; try again.",
            if out.error.is_empty() { "no message" } else { out.error.as_str() }
        )));
    }
    let smartctl = smartctl_path(Path::new(&roots.tools), pin.version).to_string_lossy().into_owned();
    let found = ports.exists(&smartctl);
    log::info!(Smartctl = smartctl.as_str(), Found = found; "InstallTool: checked the installed smartctl");
    if !found {
        return Err(InstallError::Installer(
            "The installer finished but smartctl is not where it should be. Try again.".into(),
        ));
    }
    Ok(ToolStatus {
        tool: pin.tool,
        installed: Some(pin.version.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::time::FakeClock;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Log {
        staged: Vec<(String, Vec<u8>)>,
        runs: Vec<(Tool, Duration)>,
        cleared: usize,
        /// "remove" and "run", in the order they happened.
        events: Vec<&'static str>,
        removed: Vec<String>,
    }
    struct Fake {
        log: Arc<Mutex<Log>>,
        run: Result<ToolOutput, PlatformError>,
        installed: bool,
        stage_fails: bool,
        /// A `smartctl.exe` an earlier install left, which goes when its folder is removed.
        stale: bool,
    }
    struct Sink(Arc<Mutex<Log>>, String);
    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            let mut l = self.0.lock().unwrap();
            let name = self.1.clone();
            match l.staged.iter_mut().find(|(n, _)| *n == name) {
                Some((_, v)) => v.extend_from_slice(b),
                None => l.staged.push((name, b.to_vec())),
            }
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl InstallPorts for Fake {
        fn create_staged(&mut self, name: &str) -> std::io::Result<Box<dyn Write>> {
            if self.stage_fails || self.log.lock().unwrap().staged.iter().any(|(n, _)| n == name) {
                return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
            }
            Ok(Box::new(Sink(self.log.clone(), name.to_string())))
        }
        fn run(&mut self, t: &Tool, timeout: Duration) -> Result<ToolOutput, PlatformError> {
            let mut l = self.log.lock().unwrap();
            l.runs.push((t.clone(), timeout));
            l.events.push("run");
            self.run.clone()
        }
        fn exists(&self, _: &str) -> bool {
            self.installed || (self.stale && self.log.lock().unwrap().removed.is_empty())
        }
        fn clear_staging(&mut self) {
            self.log.lock().unwrap().cleared += 1;
        }
        fn remove_dir(&mut self, path: &str) {
            let mut l = self.log.lock().unwrap();
            l.removed.push(path.to_string());
            l.events.push("remove");
        }
    }

    fn rig(run: Result<ToolOutput, PlatformError>, installed: bool) -> (Fake, Arc<Mutex<Log>>) {
        let log = Arc::new(Mutex::new(Log::default()));
        (
            Fake {
                log: log.clone(),
                run,
                installed,
                stage_fails: false,
                stale: false,
            },
            log,
        )
    }
    fn good() -> Result<ToolOutput, PlatformError> {
        Ok(ToolOutput {
            ok: true,
            stdout: String::new(),
            error: String::new(),
        })
    }
    fn roots() -> InstallRoots {
        InstallRoots {
            staging: r"C:\PF\Dyno Helper\tools\.staging".into(),
            tools: r"C:\PF\Dyno Helper\tools".into(),
        }
    }
    fn clock() -> Arc<dyn Clock> {
        Arc::new(FakeClock::new())
    }

    /// A package of the pinned size whose hash we cannot match (the real bytes are not in the repo): the hash gate is what
    /// runs here. The accepting path is tested with a test pin below.
    fn pinned_size_junk() -> Vec<u8> {
        vec![7; PINS[0].size as usize]
    }

    #[test]
    fn the_pin_table_has_the_wp0_values_for_windows_only() {
        let p = pin_for(ToolKind::Smartmontools, HelperHost::Windows).unwrap();
        assert_eq!((p.version, p.size, p.sha256.len()), ("7.5", 1_508_408, 64));
        assert!(p.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert!(pin_for(ToolKind::Smartmontools, HelperHost::Macos).is_none());
        assert!(pin_for(ToolKind::Smartmontools, HelperHost::Linux).is_none());
    }

    #[test]
    fn the_command_line_selects_components_puts_dest_last_and_confines_temp() {
        let t = windows_command(r"C:\s\setup.exe", r"C:\t\smartmontools-7.5", r"C:\s");
        assert_eq!(t.args, ["/S", "/SO", "x64,smartctl,drivedb", r"/D=C:\t\smartmontools-7.5"]);
        assert_eq!(
            t.env,
            [("TEMP".to_string(), r"C:\s".to_string()), ("TMP".to_string(), r"C:\s".to_string())]
        );
    }

    #[test]
    fn other_systems_install_nothing_and_say_why() {
        for (host, word) in [(HelperHost::Macos, "own copy"), (HelperHost::Linux, "distribution")] {
            let (mut f, log) = rig(good(), true);
            let e = install(&mut f, &clock(), host, ToolKind::Smartmontools, &[1], &roots()).unwrap_err();
            assert!(matches!(&e, InstallError::Unsupported(m) if m.contains(word)), "{e:?}");
            assert!(log.lock().unwrap().staged.is_empty() && log.lock().unwrap().runs.is_empty());
        }
    }

    #[test]
    fn one_byte_over_or_under_the_pinned_size_is_refused_before_anything_is_written() {
        for len in [PINS[0].size - 1, PINS[0].size + 1, 0] {
            let (mut f, log) = rig(good(), true);
            let bytes = vec![0; len as usize];
            let e = install(&mut f, &clock(), HelperHost::Windows, ToolKind::Smartmontools, &bytes, &roots()).unwrap_err();
            assert_eq!(
                e,
                InstallError::WrongSize {
                    got: len,
                    want: PINS[0].size
                }
            );
            assert!(log.lock().unwrap().staged.is_empty());
        }
    }

    #[test]
    fn a_wrong_hash_is_refused_after_staging_and_the_installer_never_runs() {
        let (mut f, log) = rig(good(), true);
        let e = install(
            &mut f,
            &clock(),
            HelperHost::Windows,
            ToolKind::Smartmontools,
            &pinned_size_junk(),
            &roots(),
        )
        .unwrap_err();
        assert_eq!(e, InstallError::WrongHash);
        let l = log.lock().unwrap();
        assert!(l.runs.is_empty());
        assert_eq!(l.cleared, 1, "staging is cleared on every outcome");
        assert!(matches!(PlatformError::from(InstallError::WrongHash), PlatformError::Failed(m) if m.contains("nothing was installed")));
    }

    #[test]
    fn a_second_install_cannot_overwrite_a_staged_file() {
        let (mut f, _) = rig(good(), true);
        f.log.lock().unwrap().staged.push((PINS[0].file.to_string(), vec![]));
        let e = install(
            &mut f,
            &clock(),
            HelperHost::Windows,
            ToolKind::Smartmontools,
            &pinned_size_junk(),
            &roots(),
        )
        .unwrap_err();
        assert!(matches!(e, InstallError::Staging(_)));
    }

    #[test]
    fn the_folder_the_installer_logs_is_the_folder_the_tools_list_and_the_reader_use() {
        let bytes = vec![1u8; 1000];
        let sha: &'static str = Box::leak(hex(&Sha256::digest(&bytes)).into_boxed_str());
        let pin = Pin {
            size: 1000,
            sha256: sha,
            file: "standin-setup.exe",
            ..PINS[0]
        };
        let (mut f, log) = rig(good(), true);
        let r = InstallRoots {
            staging: "S".into(),
            tools: "T".into(),
        };
        stage_and_run(&mut f, &clock(), &pin, &bytes, &r).unwrap();
        let l = log.lock().unwrap();
        let folder = install_folder(Path::new("T"), "7.5");
        assert_eq!(l.removed, [folder.to_string_lossy()]);
        assert!(l.runs[0].0.args.last().unwrap().ends_with(&*folder.to_string_lossy()));
        assert!(smartctl_path(Path::new("T"), "7.5").starts_with(&folder));
    }

    #[test]
    fn a_file_an_earlier_install_left_is_removed_first_and_cannot_pass_for_this_install() {
        let bytes = vec![1u8; 1000];
        let sha: &'static str = Box::leak(hex(&Sha256::digest(&bytes)).into_boxed_str());
        let pin = Pin {
            size: 1000,
            sha256: sha,
            file: "standin-setup.exe",
            ..PINS[0]
        };
        // The installer "succeeds" but writes nothing: only the stale file would be there.
        let (mut f, log) = rig(good(), false);
        f.stale = true;
        let got = stage_and_run(&mut f, &clock(), &pin, &bytes, &roots());
        assert!(matches!(got, Err(InstallError::Installer(_))), "{got:?}");
        let l = log.lock().unwrap();
        assert_eq!(l.events, ["remove", "run"], "the old folder goes before the installer runs");
        assert!(l.removed[0].ends_with("smartmontools-7.5"), "{:?}", l.removed);
    }

    #[test]
    fn a_matching_package_runs_the_installer_in_staging_and_reports_the_version() {
        // Hash a stand-in package and run the same stage-and-run path with a pin for it.
        let bytes = vec![1u8; 1000];
        let sha = hex(&Sha256::digest(&bytes));
        let sha: &'static str = Box::leak(sha.into_boxed_str());
        let pin = Pin {
            size: 1000,
            sha256: sha,
            file: "standin-setup.exe",
            ..PINS[0]
        };
        for (run, installed, ok) in [
            (good(), true, true),
            (good(), false, false),
            (
                Ok(ToolOutput {
                    ok: false,
                    stdout: String::new(),
                    error: "exit 2".into(),
                }),
                true,
                false,
            ),
            (Err(PlatformError::Timeout("took 120 s".into())), true, false),
        ] {
            let (mut f, log) = rig(run, installed);
            let got = stage_and_run(&mut f, &clock(), &pin, &bytes, &roots());
            assert_eq!(got.is_ok(), ok, "{got:?}");
            let l = log.lock().unwrap();
            assert_eq!(l.staged[0].1, bytes, "the staged file holds exactly the streamed bytes");
            let (cmd, timeout) = &l.runs[0];
            assert_eq!(*timeout, INSTALL_TIMEOUT);
            assert!(cmd.program.ends_with("standin-setup.exe") && cmd.args.last().unwrap().ends_with("smartmontools-7.5"));
            if ok {
                assert_eq!(
                    got.unwrap(),
                    ToolStatus {
                        tool: ToolKind::Smartmontools,
                        installed: Some("7.5".into())
                    }
                );
            }
        }
    }
}
