//! How the Pitcrew service is registered with the OS (ADR 0003): the Windows service's identity and recovery
//! actions, the macOS launch agent's property list and the Linux systemd user unit.

use std::time::Duration;

/// The Windows service's name.
pub const WINDOWS_SERVICE_NAME: &str = "Pitcrew";
/// The Windows service's display name.
pub const WINDOWS_DISPLAY_NAME: &str = "Pitcrew runner manager";
/// The Windows service's description.
pub const WINDOWS_DESCRIPTION: &str = "Keeps CI runners for Gitea and Forgejo running, healthy and up to date.";
/// The launchd label of the macOS launch agent.
pub const LAUNCHD_LABEL: &str = "net.decryptology.pitcrew.service";

/// The systemd user unit's name on Linux.
pub const SYSTEMD_UNIT: &str = "pitcrew.service";

/// What the Service Control Manager does when the service itself fails: restart it after these delays (the last
/// one repeats), forgetting failures after `reset_after`. The runners have their own supervision inside the service.
pub const SCM_RESTART_DELAYS: [Duration; 3] = [Duration::from_secs(10), Duration::from_secs(60), Duration::from_secs(300)];
/// When the SCM forgets earlier failures.
pub const SCM_RESET_AFTER: Duration = Duration::from_secs(24 * 3600);

/// The macOS launch agent that runs the service in the user's session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchAgentSpec {
    /// The launchd label.
    pub label: String,
    /// The program and its arguments.
    pub program_arguments: Vec<String>,
    /// The working directory.
    pub working_directory: String,
    /// Where launchd writes the service's standard output and error (the runners' logs are separate).
    pub log_path: String,
}

/// Renders the launch agent's property list. Values are XML-escaped; the agent loads only in the user's GUI
/// (Aqua) session, starts at log-in and is restarted by launchd if it exits (at most every 10 s).
pub fn launch_agent_plist(spec: &LaunchAgentSpec) -> String {
    let args: String = spec
        .program_arguments
        .iter()
        .map(|a| format!("\n        <string>{}</string>", xml_escape(a)))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>{args}
    </array>
    <key>WorkingDirectory</key>
    <string>{wd}</string>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>ThrottleInterval</key>
    <integer>10</integer>
    <key>LimitLoadToSessionType</key>
    <string>Aqua</string>
    <key>ProcessType</key>
    <string>Standard</string>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
</dict>
</plist>
"#,
        label = xml_escape(&spec.label),
        wd = xml_escape(&spec.working_directory),
        log = xml_escape(&spec.log_path),
    )
}

/// The Linux systemd user unit that runs the service in the user's session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemdUnitSpec {
    /// The service program and its arguments.
    pub exec: Vec<String>,
    /// The working directory.
    pub working_directory: String,
}

/// Quotes one word of an `ExecStart=` line: systemd splits on whitespace, honours double quotes and backslash
/// escapes, and expands `%` specifiers and `$` variables, so those are doubled.
pub fn systemd_quote(word: &str) -> String {
    let mut out = String::with_capacity(word.len() + 2);
    out.push('"');
    for c in word.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '%' => out.push_str("%%"),
            '$' => out.push_str("$$"),
            '\n' | '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Renders the unit. `KillMode=mixed` sends the stop signal to the service only and kills leftovers (runners that
/// ignored the service's own graceful stop) after `TimeoutStopSec`; the service stops the runners itself, waiting for
/// a running job. The unit restarts the service if it fails (at most every 10 s). Lingering (running without a
/// log-in) is the user's choice: `loginctl enable-linger` (docs/linux.md).
pub fn systemd_unit(spec: &SystemdUnitSpec) -> String {
    let exec: Vec<String> = spec.exec.iter().map(|a| systemd_quote(a)).collect();
    format!(
        "[Unit]
Description=Pitcrew runner manager
Documentation=https://dev.decryptology.net/decryp7/Pitcrew
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={exec}
WorkingDirectory={wd}
Restart=on-failure
RestartSec=10
KillMode=mixed
TimeoutStopSec=300

[Install]
WantedBy=default.target
",
        exec = exec.join(" "),
        wd = systemd_quote(&spec.working_directory),
    )
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters are not allowed in XML 1.0; drop them.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_an_aqua_launch_agent() {
        let spec = LaunchAgentSpec {
            label: LAUNCHD_LABEL.into(),
            program_arguments: vec!["/Applications/Pitcrew.app/Contents/MacOS/pitcrew-service".into(), "run".into()],
            working_directory: "/Users/ci/Library/Application Support/Pitcrew".into(),
            log_path: "/Users/ci/Library/Logs/Pitcrew/service.log".into(),
        };
        let plist = launch_agent_plist(&spec);
        assert!(plist.contains("<string>net.decryptology.pitcrew.service</string>"));
        assert!(plist.contains("<string>/Applications/Pitcrew.app/Contents/MacOS/pitcrew-service</string>\n        <string>run</string>"));
        assert!(plist.contains("<key>LimitLoadToSessionType</key>\n    <string>Aqua</string>"));
        assert!(plist.contains("<key>KeepAlive</key>\n    <true/>"));
    }

    #[test]
    fn escapes_values() {
        let spec = LaunchAgentSpec {
            label: "a&b".into(),
            program_arguments: vec!["<x>\"'\u{1}".into()],
            working_directory: "/".into(),
            log_path: "/l".into(),
        };
        let plist = launch_agent_plist(&spec);
        assert!(plist.contains("<string>a&amp;b</string>"));
        assert!(plist.contains("<string>&lt;x&gt;&quot;&apos;</string>"));
    }

    #[test]
    fn recovery_delays_grow() {
        assert!(SCM_RESTART_DELAYS.windows(2).all(|w| w[0] < w[1]));
        assert!(SCM_RESET_AFTER > SCM_RESTART_DELAYS[2]);
        assert!(!WINDOWS_SERVICE_NAME.contains(' '));
        assert!(!WINDOWS_DESCRIPTION.is_empty() && !WINDOWS_DISPLAY_NAME.is_empty());
    }

    #[test]
    fn the_systemd_unit_runs_the_service_in_the_users_session() {
        let unit = systemd_unit(&SystemdUnitSpec {
            exec: vec!["/home/ci/.local/share/Pitcrew/bin/pitcrew-service".into(), "run".into()],
            working_directory: "/home/ci/.local/share/Pitcrew".into(),
        });
        assert!(unit.contains("ExecStart=\"/home/ci/.local/share/Pitcrew/bin/pitcrew-service\" \"run\"\n"));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(unit.contains("Restart=on-failure") && unit.contains("KillMode=mixed"));
        assert!(unit.contains("WorkingDirectory=\"/home/ci/.local/share/Pitcrew\""));
        assert_eq!(SYSTEMD_UNIT, "pitcrew.service");
    }

    #[test]
    fn systemd_words_are_quoted_so_nothing_is_expanded_or_split() {
        assert_eq!(systemd_quote("/opt/my apps/x"), "\"/opt/my apps/x\"");
        assert_eq!(systemd_quote("50%$HOME"), "\"50%%$$HOME\"");
        assert_eq!(systemd_quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(systemd_quote("a\nExecStart=/bin/evil"), "\"a ExecStart=/bin/evil\"");
    }
}
