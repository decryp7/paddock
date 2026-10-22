//! What starts at login, how much it costs, and the startup manager's decisions.
//!
//! Impact uses the thresholds Windows Task Manager documents for "Startup impact" (CPU time and disk bytes in the
//! first minutes after login): High above 1 s CPU or 3 MB of disk I/O, Medium above 300 ms or 300 KB, else Low.

use serde::{Deserialize, Serialize};

/// Where a startup entry is registered.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupLocation {
    /// `HKCU\…\Run` (this user; no admin needed to disable).
    UserRunKey,
    /// `HKLM\…\Run` (every user; disabling needs the privileged helper).
    MachineRunKey,
    /// The user's Startup folder.
    UserStartupFolder,
    /// The all-users Startup folder (needs the helper).
    CommonStartupFolder,
    /// A scheduled task that runs at log-on.
    ScheduledTask,
    /// `~/Library/LaunchAgents` (this user).
    UserLaunchAgent,
    /// `/Library/LaunchAgents` (every user; needs the helper).
    SystemLaunchAgent,
    /// A macOS login item.
    LoginItem,
}

impl StartupLocation {
    /// Whether changing it needs administrator rights (and so goes through the privileged helper).
    pub fn needs_admin(&self) -> bool {
        matches!(
            self,
            StartupLocation::MachineRunKey
                | StartupLocation::CommonStartupFolder
                | StartupLocation::ScheduledTask
                | StartupLocation::SystemLaunchAgent
        )
    }

    /// The user-facing name.
    pub fn label(&self) -> &'static str {
        match self {
            StartupLocation::UserRunKey => "Registry (this user)",
            StartupLocation::MachineRunKey => "Registry (all users)",
            StartupLocation::UserStartupFolder => "Startup folder (this user)",
            StartupLocation::CommonStartupFolder => "Startup folder (all users)",
            StartupLocation::ScheduledTask => "Scheduled task",
            StartupLocation::UserLaunchAgent => "Launch agent (this user)",
            StartupLocation::SystemLaunchAgent => "Launch agent (all users)",
            StartupLocation::LoginItem => "Login item",
        }
    }
}

/// Whether an entry runs at login.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupState {
    /// Runs at login.
    Enabled,
    /// Does not run.
    Disabled,
    /// Disabled at the OS level; Dyno starts it this many seconds after login.
    Delayed(u32),
}

/// Measured startup cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImpactRating {
    /// Not measured yet (it didn't run since Dyno started watching logins).
    NotMeasured,
    /// Under 300 ms CPU and 300 KB disk.
    Low,
    /// Under 1 s CPU and 3 MB disk.
    Medium,
    /// More.
    High,
}

/// What a startup entry cost at the last login.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupImpact {
    /// CPU seconds used in the first minutes after login.
    pub cpu_secs: f32,
    /// Bytes read and written in that time.
    pub disk_bytes: u64,
    /// The rating.
    pub rating: ImpactRating,
}

/// Rates measured CPU time and disk bytes.
pub fn rate(cpu_secs: f32, disk_bytes: u64) -> ImpactRating {
    if cpu_secs > 1.0 || disk_bytes > 3_000_000 {
        ImpactRating::High
    } else if cpu_secs > 0.3 || disk_bytes > 300_000 {
        ImpactRating::Medium
    } else {
        ImpactRating::Low
    }
}

/// One thing that starts at login.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupEntry {
    /// Stable id: location plus the entry's own name.
    pub id: String,
    /// The entry's name (registry value, file, task or agent label).
    pub name: String,
    /// The command it runs.
    pub command: String,
    /// The executable the command starts, when it can be parsed out.
    pub exe: Option<String>,
    /// Where it is registered.
    pub location: StartupLocation,
    /// Whether it runs.
    pub state: StartupState,
    /// The executable's publisher (signer), when known.
    pub publisher: Option<String>,
    /// Its measured cost.
    pub impact: StartupImpact,
}

/// The executable a startup command line starts: the quoted first token, or everything up to the first `.exe`.
pub fn exe_of_command(command: &str) -> Option<String> {
    let c = command.trim();
    if let Some(rest) = c.strip_prefix('"') {
        return rest.split('"').next().filter(|s| !s.is_empty()).map(str::to_string);
    }
    let lower = c.to_ascii_lowercase();
    if let Some(i) = lower.find(".exe") {
        return Some(c[..i + 4].to_string());
    }
    c.split_whitespace().next().map(str::to_string)
}

/// The startup entry that launched an executable, if any (paths compare case-insensitively).
pub fn source_of<'a>(exe: &str, entries: &'a [StartupEntry]) -> Option<&'a StartupEntry> {
    let norm = |s: &str| s.replace('\\', "/").to_lowercase();
    let target = norm(exe);
    entries
        .iter()
        .find(|e| e.exe.as_deref().map(norm).as_deref() == Some(target.as_str()))
}

/// The most seconds a delayed start may wait.
pub const MAX_DELAY_SECS: u32 = 600;

/// Checks a requested startup state change.
pub fn validate_change(entry: &StartupEntry, to: StartupState) -> Result<(), String> {
    match to {
        StartupState::Delayed(s) if s == 0 || s > MAX_DELAY_SECS => Err(format!("A delay is between 1 and {MAX_DELAY_SECS} seconds.")),
        _ if entry.state == to => Err(format!("{} is already {}.", entry.name, state_label(to))),
        _ => Ok(()),
    }
}

/// The user-facing word for a state.
pub fn state_label(s: StartupState) -> &'static str {
    match s {
        StartupState::Enabled => "enabled",
        StartupState::Disabled => "disabled",
        StartupState::Delayed(_) => "delayed",
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    /// An enabled user Run-key entry.
    pub fn entry(name: &str, exe: &str, rating: ImpactRating) -> StartupEntry {
        StartupEntry {
            id: format!("userRunKey:{name}"),
            name: name.into(),
            command: format!("\"{exe}\" --background"),
            exe: Some(exe.into()),
            location: StartupLocation::UserRunKey,
            state: StartupState::Enabled,
            publisher: None,
            impact: StartupImpact {
                cpu_secs: 2.0,
                disk_bytes: 0,
                rating,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::entry;
    use super::*;

    #[test]
    fn rating_thresholds() {
        assert_eq!(rate(0.1, 1000), ImpactRating::Low);
        assert_eq!(rate(0.5, 0), ImpactRating::Medium);
        assert_eq!(rate(0.0, 400_000), ImpactRating::Medium);
        assert_eq!(rate(1.5, 0), ImpactRating::High);
        assert_eq!(rate(0.0, 4_000_000), ImpactRating::High);
    }

    #[test]
    fn commands_yield_executables() {
        assert_eq!(exe_of_command("\"C:\\A B\\x.exe\" -a").as_deref(), Some("C:\\A B\\x.exe"));
        assert_eq!(exe_of_command("C:\\Prog Files\\y.EXE /s").as_deref(), Some("C:\\Prog Files\\y.EXE"));
        assert_eq!(exe_of_command("/usr/bin/thing --x").as_deref(), Some("/usr/bin/thing"));
        assert_eq!(exe_of_command("\"\""), None);
        assert_eq!(exe_of_command("   "), None);
    }

    #[test]
    fn sources_and_locations() {
        let es = vec![entry("Slack", "C:\\Users\\me\\Slack\\slack.exe", ImpactRating::High)];
        assert_eq!(
            source_of("c:/users/me/slack/SLACK.exe", &es).map(|e| e.name.as_str()),
            Some("Slack")
        );
        assert!(source_of("c:/other.exe", &es).is_none());
        let all = [
            StartupLocation::UserRunKey,
            StartupLocation::MachineRunKey,
            StartupLocation::UserStartupFolder,
            StartupLocation::CommonStartupFolder,
            StartupLocation::ScheduledTask,
            StartupLocation::UserLaunchAgent,
            StartupLocation::SystemLaunchAgent,
            StartupLocation::LoginItem,
        ];
        let admin: Vec<bool> = all.iter().map(StartupLocation::needs_admin).collect();
        assert_eq!(admin, [false, true, false, true, true, false, true, false]);
        assert!(all.iter().all(|l| !l.label().is_empty()));
    }

    #[test]
    fn changes_are_checked() {
        let e = entry("Slack", "C:\\s.exe", ImpactRating::Low);
        assert!(validate_change(&e, StartupState::Enabled).is_err());
        assert_eq!(validate_change(&e, StartupState::Disabled), Ok(()));
        assert_eq!(validate_change(&e, StartupState::Delayed(30)), Ok(()));
        assert!(validate_change(&e, StartupState::Delayed(0)).is_err());
        assert!(validate_change(&e, StartupState::Delayed(MAX_DELAY_SECS + 1)).is_err());
        assert_eq!(state_label(StartupState::Disabled), "disabled");
        assert_eq!(state_label(StartupState::Delayed(3)), "delayed");
    }
}
