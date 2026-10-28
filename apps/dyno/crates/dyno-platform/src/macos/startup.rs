//! The startup manager on macOS: launch agents in `~/Library/LaunchAgents` and `/Library/LaunchAgents`, listed only.
//! Enabled/disabled is launchd's own override database (`launchctl print-disabled`). `launchctl` sits behind
//! [`Launchctl`] so tests never touch the real user's launchd.

use crate::parse::launchd::{parse_launch_agent, parse_print_disabled, LaunchAgent};
use dyno_core::ports::{PlatformError, StartupItems};
use dyno_core::startup::{exe_of_command, ImpactRating, StartupEntry, StartupImpact, StartupLocation, StartupState};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Most plist files read per folder.
const MAX_AGENTS: usize = 2_000;

/// The launchd operations the startup manager needs.
pub trait Launchctl: Send {
    /// `launchctl print-disabled gui/<uid>` output.
    fn print_disabled(&mut self) -> Result<String, PlatformError>;
}

/// The real `launchctl` for the current user's GUI domain.
pub struct RealLaunchctl {
    domain: String,
}

impl RealLaunchctl {
    /// For the current user.
    pub fn new() -> Self {
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        RealLaunchctl {
            domain: format!("gui/{uid}"),
        }
    }

    fn run(&self, args: &[&str]) -> Result<String, PlatformError> {
        let mut cmd = crate::process::command("/bin/launchctl");
        cmd.args(args);
        let out = crate::process::run_with_timeout(cmd, Duration::from_secs(15))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            Err(PlatformError::Failed(format!(
                "launchctl said: {}",
                err.lines().next().unwrap_or("it failed").trim()
            )))
        }
    }
}

impl Default for RealLaunchctl {
    fn default() -> Self {
        Self::new()
    }
}

impl Launchctl for RealLaunchctl {
    fn print_disabled(&mut self) -> Result<String, PlatformError> {
        self.run(&["print-disabled", &self.domain])
    }
}

/// Launch agents as startup entries.
pub struct LaunchAgents {
    user_dir: Option<PathBuf>,
    system_dir: Option<PathBuf>,
    launchctl: Box<dyn Launchctl>,
}

impl LaunchAgents {
    /// The current user's folders and the real `launchctl`.
    pub fn for_current_user() -> Self {
        let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
        LaunchAgents {
            user_dir: home.map(|h| h.join("Library").join("LaunchAgents")),
            system_dir: Some(PathBuf::from("/Library/LaunchAgents")),
            launchctl: Box::new(RealLaunchctl::new()),
        }
    }

    /// Explicit folders and launchctl (tests).
    pub fn with(user_dir: Option<PathBuf>, system_dir: Option<PathBuf>, launchctl: Box<dyn Launchctl>) -> Self {
        LaunchAgents {
            user_dir,
            system_dir,
            launchctl,
        }
    }

    fn read_dir(dir: &Path, location: StartupLocation, disabled: &[(String, bool)], out: &mut Vec<StartupEntry>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten().take(MAX_AGENTS) {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("plist") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let Some(agent) = parse_launch_agent(&bytes) else { continue };
            if !agent.starts_at_login() {
                continue;
            }
            out.push(entry_of(&agent, location.clone(), disabled));
        }
    }
}

fn entry_of(agent: &LaunchAgent, location: StartupLocation, disabled: &[(String, bool)]) -> StartupEntry {
    let override_state = disabled.iter().find(|(l, _)| *l == agent.label).map(|(_, d)| *d);
    let is_disabled = override_state.unwrap_or(agent.disabled);
    let command = agent.command();
    let prefix = if location == StartupLocation::UserLaunchAgent {
        "user"
    } else {
        "system"
    };
    StartupEntry {
        id: format!("agent:{prefix}:{}", agent.label),
        name: agent.label.clone(),
        exe: agent.executable().map(str::to_string).or_else(|| exe_of_command(&command)),
        command,
        location,
        state: if is_disabled {
            StartupState::Disabled
        } else {
            StartupState::Enabled
        },
        publisher: None,
        impact: StartupImpact {
            cpu_secs: 0.0,
            disk_bytes: 0,
            rating: ImpactRating::NotMeasured,
        },
    }
}

impl StartupItems for LaunchAgents {
    fn list(&mut self) -> Result<Vec<StartupEntry>, PlatformError> {
        // Without launchctl's answer, the plist's own Disabled key is all we know.
        let disabled = self
            .launchctl
            .print_disabled()
            .map(|t| parse_print_disabled(&t))
            .unwrap_or_default();
        let mut out = Vec::new();
        if let Some(d) = &self.user_dir {
            Self::read_dir(d, StartupLocation::UserLaunchAgent, &disabled, &mut out);
        }
        if let Some(d) = &self.system_dir {
            Self::read_dir(d, StartupLocation::SystemLaunchAgent, &disabled, &mut out);
        }
        out.sort_by_key(|e| e.name.to_lowercase());
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Fake {
        disabled_text: String,
    }
    impl Launchctl for Fake {
        fn print_disabled(&mut self) -> Result<String, PlatformError> {
            Ok(self.disabled_text.clone())
        }
    }

    fn plist(label: &str, run_at_load: bool) -> String {
        format!(
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key>\
<array><string>/Applications/X.app/x</string></array><key>RunAtLoad</key><{}/></dict></plist>",
            run_at_load
        )
    }

    fn setup() -> (tempfile::TempDir, Fake, LaunchAgents) {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user");
        let system = dir.path().join("system");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        std::fs::write(user.join("a.plist"), plist("com.example.a", true)).unwrap();
        std::fs::write(user.join("b.plist"), plist("com.example.b", true)).unwrap();
        std::fs::write(user.join("manual.plist"), plist("com.example.manual", false)).unwrap();
        std::fs::write(user.join("junk.plist"), b"not a plist").unwrap();
        std::fs::write(user.join("readme.txt"), b"x").unwrap();
        std::fs::write(system.join("s.plist"), plist("com.vendor.s", true)).unwrap();
        let fake = Fake {
            disabled_text: "disabled services = {\n\t\"com.example.b\" => disabled\n\t\"com.example.a\" => enabled\n}".into(),
            ..Fake::default()
        };
        let la = LaunchAgents::with(Some(user), Some(system), Box::new(fake.clone()));
        (dir, fake, la)
    }

    #[test]
    fn lists_login_agents_with_launchds_disabled_state() {
        let (_d, _f, mut la) = setup();
        let list = la.list().unwrap();
        let names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["com.example.a", "com.example.b", "com.vendor.s"]);
        assert_eq!(list[0].state, StartupState::Enabled);
        assert_eq!(list[1].state, StartupState::Disabled);
        assert_eq!(list[2].location, StartupLocation::SystemLaunchAgent);
        assert_eq!(list[0].exe.as_deref(), Some("/Applications/X.app/x"));
    }

    #[test]
    fn missing_folders_are_an_empty_list() {
        let mut la = LaunchAgents::with(None, Some(PathBuf::from("/dyno/none")), Box::new(Fake::default()));
        assert!(la.list().unwrap().is_empty());
    }

    #[test]
    fn this_macs_agents_list_without_changing_anything() {
        let mut la = LaunchAgents::for_current_user();
        let list = la.list().unwrap();
        assert!(list.iter().all(|e| !e.id.is_empty() && !e.name.is_empty()));
    }
}
