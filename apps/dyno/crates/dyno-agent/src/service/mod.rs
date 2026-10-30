//! Installing the agent as a per-user background service (ADR 0010): a launch agent on macOS, a systemd user unit on
//! Linux, a logon `Run` entry on Windows (a real SCM service runs in session 0, where the user's idle time, foreground
//! window, per-user startup items and data folder are out of reach). The app drives these through `dyno-agent service`.
//!
//! The text of the unit files is built by pure functions in [`spec`], so it is tested on every OS; the OS commands
//! run behind [`Runner`] so tests never touch the real launchd, systemd or registry.

pub mod spec;

use dyno_platform::process::{command, run_with_timeout};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// launchd label, systemd unit and Run value name.
pub const LABEL: &str = "net.decryptology.dyno.agent";

/// What the OS says about the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Not registered.
    NotInstalled,
    /// Registered; `running` says whether the OS has it up, `current` whether it points at this executable.
    Installed {
        /// The OS has the job running (or loaded).
        running: bool,
        /// The registration names the executable that is asking.
        current: bool,
    },
}

/// Runs an OS command and returns (success, stdout). Injectable.
pub trait Runner: Send {
    /// Runs `program args…` with a timeout.
    fn run(&mut self, program: &str, args: &[&str]) -> Result<(bool, String), String>;
}

/// The real thing.
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&mut self, program: &str, args: &[&str]) -> Result<(bool, String), String> {
        let mut cmd = command(program);
        cmd.args(args);
        let out = run_with_timeout(cmd, Duration::from_secs(20)).map_err(|e| e.to_string())?;
        Ok((out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned()))
    }
}

/// How long launchd gets to drop a booted-out job before it is bootstrapped again, and how often it is asked.
pub const GONE_WAIT: Duration = Duration::from_secs(20);
/// See [`GONE_WAIT`].
pub const GONE_POLL: Duration = Duration::from_secs(1);

/// A per-user service manager for the current OS.
pub struct Service {
    exe: PathBuf,
    home: PathBuf,
    uid: u32,
    runner: Box<dyn Runner>,
    os: Os,
    pause: Box<dyn FnMut(Duration) + Send>,
}

/// Which flavour of service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// launchd.
    Mac,
    /// systemd user.
    Linux,
    /// HKCU Run.
    Windows,
}

impl Os {
    /// The OS this build runs on.
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Linux
        }
    }
}

/// On macOS the app's bundle registers the agent itself (`SMAppService`, the plist inside the bundle) under the same
/// launchd label as [`Service`] writes: the CLI's `install` would add a second plist, `uninstall` would boot out the real
/// agent and `status` would report "not installed". A `dyno-agent` running from inside a `.app` therefore refuses every
/// `service` subcommand with the way to manage it instead. Other OSes and a `dyno-agent` outside a bundle are not affected.
pub fn refuse_inside_app_bundle(os: Os, exe: &Path) -> Result<(), String> {
    if os != Os::Mac {
        return Ok(());
    }
    // `<name>.app/Contents/MacOS/<program>`: the program inside a bundle, not any folder that happens to end in `.app`.
    let parts: Vec<&str> = exe.components().filter_map(|c| c.as_os_str().to_str()).collect();
    let in_bundle = parts
        .windows(4)
        .any(|w| w[0].len() > 4 && w[0].ends_with(".app") && w[1] == "Contents" && w[2] == "MacOS" && !w[3].is_empty());
    if in_bundle {
        return Err(
            "this dyno-agent is part of the Dyno app, which registers it with macOS itself under the same label \
             (System Settings > General > Login Items, or Dyno's Settings > Launch at login); `service` commands would \
             conflict with that registration, so they are not available here"
                .into(),
        );
    }
    Ok(())
}

impl Service {
    /// The service for the current user and OS, pointing at `exe`.
    pub fn for_current_user(exe: PathBuf) -> Result<Service, String> {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .ok_or("Dyno could not find your home folder.")?;
        let uid = current_uid();
        Ok(Service::with(exe, home, uid, Box::new(SystemRunner), Os::current()))
    }

    /// Explicit parts (tests).
    pub fn with(exe: PathBuf, home: PathBuf, uid: u32, runner: Box<dyn Runner>, os: Os) -> Service {
        Service {
            exe,
            home,
            uid,
            runner,
            os,
            pause: Box::new(std::thread::sleep),
        }
    }

    /// The same service waiting with `pause` instead of sleeping (tests).
    #[must_use]
    pub fn with_pause(mut self, pause: impl FnMut(Duration) + Send + 'static) -> Service {
        self.pause = Box::new(pause);
        self
    }

    fn plist_path(&self) -> PathBuf {
        self.home.join("Library").join("LaunchAgents").join(format!("{LABEL}.plist"))
    }

    fn unit_path(&self) -> PathBuf {
        self.home.join(".config").join("systemd").join("user").join("dyno-agent.service")
    }

    fn domain(&self) -> String {
        format!("gui/{}", self.uid)
    }

    /// Registers and starts the service. Registering again replaces an older registration.
    pub fn install(&mut self) -> Result<(), String> {
        match self.os {
            Os::Mac => {
                let log = self.home.join("Library").join("Logs").join("Dyno");
                std::fs::create_dir_all(&log).map_err(|e| format!("Could not create {}: {e}", log.display()))?;
                write(&self.plist_path(), &spec::launch_agent_plist(&self.exe, &log))?;
                // Re-register from scratch: after an app update launchd may hold the old definition, or fail to
                // spawn the new binary until it is bootstrapped again. Bootstrapping over a job launchd still holds
                // does nothing, so wait (bounded) until it has dropped it (Paddock rule 24).
                let target = format!("{}/{LABEL}", self.domain());
                let _ = self.runner.run("/bin/launchctl", &["bootout", &target]);
                let mut waited = Duration::ZERO;
                while self.job_loaded(&target) && waited < GONE_WAIT {
                    (self.pause)(GONE_POLL);
                    waited += GONE_POLL;
                }
                let path = self.plist_path();
                self.check("/bin/launchctl", &["bootstrap", &self.domain(), &path.to_string_lossy()])
            }
            Os::Linux => {
                write(&self.unit_path(), &spec::systemd_unit(&self.exe))?;
                self.check("systemctl", &["--user", "daemon-reload"])?;
                self.check("systemctl", &["--user", "enable", "--now", "dyno-agent.service"])
            }
            Os::Windows => self.check(
                "reg",
                &[
                    "add",
                    spec::WINDOWS_RUN_KEY,
                    "/v",
                    "Dyno",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &spec::windows_run_value(&self.exe),
                    "/f",
                ],
            ),
        }
    }

    /// Stops and removes the service. Removing what is not installed succeeds.
    pub fn uninstall(&mut self) -> Result<(), String> {
        match self.os {
            Os::Mac => {
                let _ = self
                    .runner
                    .run("/bin/launchctl", &["bootout", &format!("{}/{LABEL}", self.domain())]);
                remove(&self.plist_path())
            }
            Os::Linux => {
                let _ = self.runner.run("systemctl", &["--user", "disable", "--now", "dyno-agent.service"]);
                remove(&self.unit_path())?;
                let _ = self.runner.run("systemctl", &["--user", "daemon-reload"]);
                Ok(())
            }
            Os::Windows => {
                let _ = self.runner.run("reg", &["delete", spec::WINDOWS_RUN_KEY, "/v", "Dyno", "/f"]);
                Ok(())
            }
        }
    }

    /// What is registered and whether it runs.
    pub fn status(&mut self) -> Status {
        match self.os {
            Os::Mac => {
                let Ok(text) = std::fs::read_to_string(self.plist_path()) else {
                    return Status::NotInstalled;
                };
                let current = text.contains(&*self.exe.to_string_lossy());
                let running = self
                    .runner
                    .run("/bin/launchctl", &["print", &format!("{}/{LABEL}", self.domain())])
                    .map(|(ok, out)| ok && out.contains("state = running"))
                    .unwrap_or(false);
                Status::Installed { running, current }
            }
            Os::Linux => {
                let Ok(text) = std::fs::read_to_string(self.unit_path()) else {
                    return Status::NotInstalled;
                };
                let current = text.contains(&*self.exe.to_string_lossy());
                let running = self
                    .runner
                    .run("systemctl", &["--user", "is-active", "dyno-agent.service"])
                    .map(|(ok, out)| ok && out.trim() == "active")
                    .unwrap_or(false);
                Status::Installed { running, current }
            }
            Os::Windows => match self.runner.run("reg", &["query", spec::WINDOWS_RUN_KEY, "/v", "Dyno"]) {
                Ok((true, out)) => Status::Installed {
                    running: true,
                    current: out.contains(&*self.exe.to_string_lossy()),
                },
                _ => Status::NotInstalled,
            },
        }
    }

    /// Whether the OS service manager starts the agent now and restarts it when it exits, so nobody else may run one
    /// beside it: a bootstrapped launch agent (`KeepAlive`), a systemd user unit that is active or restarting
    /// (`Restart=always`; `is-active` says `activating` during `RestartSec`, and exits non-zero then). An enabled but
    /// stopped or failed unit starts nothing now. A Windows `Run` entry starts it only at the next logon: never.
    pub fn supervised(&mut self) -> bool {
        match self.os {
            Os::Mac => self.plist_path().exists(),
            Os::Linux => self
                .runner
                .run("systemctl", &["--user", "is-active", "dyno-agent.service"])
                .is_ok_and(|(_, out)| matches!(out.trim(), "active" | "activating")),
            Os::Windows => false,
        }
    }

    /// Re-registers when the service is installed but stale (another executable, or launchd cannot spawn it).
    /// Returns whether it did. Not installed stays not installed: Dyno never installs itself unasked.
    pub fn ensure(&mut self) -> Result<bool, String> {
        match self.status() {
            Status::Installed { running, current } if !current || (!running && self.os == Os::Mac) => {
                self.install()?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Whether launchd still knows the job `target` (`launchctl print` succeeds and reports a state).
    fn job_loaded(&mut self, target: &str) -> bool {
        self.runner
            .run("/bin/launchctl", &["print", target])
            .is_ok_and(|(ok, out)| ok && out.contains("state = "))
    }

    fn check(&mut self, program: &str, args: &[&str]) -> Result<(), String> {
        match self.runner.run(program, args) {
            Ok((true, _)) => Ok(()),
            Ok((false, _)) => Err(format!("`{program} {}` failed.", args.first().copied().unwrap_or(""))),
            Err(e) => Err(e),
        }
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

#[cfg(not(unix))]
fn current_uid() -> u32 {
    0
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("Could not write {}: {e}", path.display()))
}

fn remove(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Could not remove {}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests;
