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
    /// `/Library/LaunchAgents` (every user).
    SystemLaunchAgent,
    /// A macOS login item.
    LoginItem,
}

impl StartupLocation {
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

/// Who an entry belongs to, for the startup manager's filter (WP-34).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupOrigin {
    /// Part of the operating system (Apple's agents, Windows' own tasks, system services).
    Os,
    /// Not the OS's, but the program names a publisher (signed).
    Signed,
    /// Everything else: unsigned or unknown.
    Other,
}

fn norm_path(s: &str) -> String {
    s.replace('\\', "/").to_lowercase()
}

/// Whether the entry is part of the operating system `os` (by its label or the folder it runs from).
pub fn is_os_owned(entry: &StartupEntry, os: crate::os::Os) -> bool {
    use crate::os::Os;
    let exe = entry.exe.as_deref().map(norm_path).unwrap_or_default();
    let name = norm_path(&entry.name);
    let under = |prefixes: &[&str]| prefixes.iter().any(|p| exe.starts_with(p));
    match os {
        Os::Macos => {
            name.starts_with("com.apple.")
                || under(&[
                    "/system/",
                    "/usr/libexec/",
                    "/usr/sbin/",
                    "/usr/bin/",
                    "/sbin/",
                    "/bin/",
                    "/library/apple/",
                ])
        }
        Os::Windows => {
            let after_drive = exe.get(2..).unwrap_or("");
            let in_windows = exe.as_bytes().get(1) == Some(&b':') && after_drive.starts_with("/windows/");
            in_windows
                || name.starts_with("/microsoft/windows/")
                || norm_path(&entry.id).contains(":/microsoft/windows/")
                || entry
                    .publisher
                    .as_deref()
                    .is_some_and(|p| p.eq_ignore_ascii_case("Microsoft Windows"))
        }
        Os::Linux => under(&["/usr/lib/", "/usr/lib64/", "/usr/libexec/", "/usr/sbin/", "/lib/", "/sbin/"]),
    }
}

/// Classifies an entry: the OS's own first, then signed (a publisher is known), else other.
pub fn origin(entry: &StartupEntry, os: crate::os::Os) -> StartupOrigin {
    if is_os_owned(entry, os) {
        StartupOrigin::Os
    } else if entry.publisher.as_deref().is_some_and(|p| !p.trim().is_empty()) {
        StartupOrigin::Signed
    } else {
        StartupOrigin::Other
    }
}

/// What the startup manager hides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StartupFilter {
    /// Hide the operating system's own entries.
    pub hide_os: bool,
    /// Hide entries with a known publisher (the OS's are counted separately).
    pub hide_signed: bool,
}

/// The startup list after the filter, with the counts the toggles show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupOverview {
    /// The entries left after filtering.
    pub entries: Vec<StartupEntry>,
    /// Every entry before filtering.
    pub total: u32,
    /// How many are the operating system's.
    pub os_count: u32,
    /// How many are signed and not the OS's.
    pub signed_count: u32,
    /// What could not be read (a source the OS refused or that failed), one sentence each: the list is then partial and the page
    /// says so instead of presenting it as everything.
    #[serde(default)]
    pub unavailable: Vec<String>,
}

/// Applies `filter` to `entries`.
pub fn overview(entries: Vec<StartupEntry>, filter: StartupFilter, os: crate::os::Os) -> StartupOverview {
    let total = entries.len() as u32;
    let (mut os_count, mut signed_count) = (0, 0);
    let kept = entries
        .into_iter()
        .filter(|e| match origin(e, os) {
            StartupOrigin::Os => {
                os_count += 1;
                !filter.hide_os
            }
            StartupOrigin::Signed => {
                signed_count += 1;
                !filter.hide_signed
            }
            StartupOrigin::Other => true,
        })
        .collect();
    StartupOverview {
        entries: kept,
        total,
        os_count,
        signed_count,
        unavailable: Vec::new(),
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
            command: format!("\"{exe}\" --hidden"),
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
        assert!(all.iter().all(|l| !l.label().is_empty()));
    }
}

#[cfg(test)]
mod filter_tests {
    use super::fixtures::entry;
    use super::*;
    use crate::os::Os;

    fn signed(name: &str, exe: &str, publisher: &str) -> StartupEntry {
        let mut e = entry(name, exe, ImpactRating::Low);
        e.publisher = Some(publisher.into());
        e
    }

    #[test]
    fn the_os_owns_its_labels_and_folders() {
        let apple = entry("com.apple.Spotlight", "/System/Library/CoreServices/x", ImpactRating::Low);
        assert!(is_os_owned(&apple, Os::Macos));
        assert!(is_os_owned(&entry("Other", "/usr/libexec/foo", ImpactRating::Low), Os::Macos));
        assert!(!is_os_owned(
            &entry("com.slack", "/Applications/Slack.app/x", ImpactRating::Low),
            Os::Macos
        ));
        // Windows paths use either separator and any case; only the drive's Windows folder counts.
        assert!(is_os_owned(
            &entry(
                "SecurityHealth",
                "C:\\WINDOWS\\system32\\SecurityHealthSystray.exe",
                ImpactRating::Low
            ),
            Os::Windows
        ));
        assert!(!is_os_owned(
            &entry("Slack", "C:\\Users\\me\\windows\\slack.exe", ImpactRating::Low),
            Os::Windows
        ));
        let mut task = entry("\\Microsoft\\Windows\\Defrag\\ScheduledDefrag", "", ImpactRating::Low);
        task.exe = None;
        assert!(is_os_owned(&task, Os::Windows));
        assert!(is_os_owned(&signed("x", "D:\\a.exe", "Microsoft Windows"), Os::Windows));
        assert!(is_os_owned(
            &entry("gnome-keyring", "/usr/libexec/gnome-keyring-daemon", ImpactRating::Low),
            Os::Linux
        ));
        assert!(!is_os_owned(
            &entry("syncthing", "/home/me/bin/syncthing", ImpactRating::Low),
            Os::Linux
        ));
    }

    #[test]
    fn origin_puts_the_os_before_signed() {
        assert_eq!(origin(&signed("a", "/Applications/a", "Acme"), Os::Macos), StartupOrigin::Signed);
        assert_eq!(origin(&signed("b", "/usr/bin/b", "Apple"), Os::Macos), StartupOrigin::Os);
        assert_eq!(origin(&entry("c", "/opt/c", ImpactRating::Low), Os::Macos), StartupOrigin::Other);
        assert_eq!(origin(&signed("d", "/opt/d", "  "), Os::Macos), StartupOrigin::Other);
    }

    #[test]
    fn the_filter_hides_and_counts() {
        let list = vec![
            entry("com.apple.a", "/System/a", ImpactRating::Low),
            signed("Slack", "/Applications/Slack", "Slack Technologies"),
            entry("mystery", "/tmp/mystery", ImpactRating::High),
        ];
        let all = overview(list.clone(), StartupFilter::default(), Os::Macos);
        assert_eq!((all.entries.len(), all.total, all.os_count, all.signed_count), (3, 3, 1, 1));
        let no_os = overview(
            list.clone(),
            StartupFilter {
                hide_os: true,
                hide_signed: false,
            },
            Os::Macos,
        );
        assert_eq!(
            no_os.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["Slack", "mystery"]
        );
        let both = overview(
            list.clone(),
            StartupFilter {
                hide_os: true,
                hide_signed: true,
            },
            Os::Macos,
        );
        assert_eq!(both.entries.len(), 1);
        assert_eq!(both.entries[0].name, "mystery");
        // The counts do not depend on the filter, so the toggles can always show them.
        assert_eq!((both.total, both.os_count, both.signed_count), (3, 1, 1));
        assert!(overview(vec![], StartupFilter::default(), Os::Linux).entries.is_empty());
    }
}
