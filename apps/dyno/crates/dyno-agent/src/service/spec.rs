//! The text of the service definitions (pure).

use std::path::Path;

/// The Windows per-user Run key.
pub const WINDOWS_RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The launch agent plist: started at login in the user's GUI session, restarted when it exits, throttled so a crash
/// loop cannot spin.
pub fn launch_agent_plist(exe: &Path, log_dir: &Path) -> String {
    // A launchd path is always POSIX, whatever host builds the text (tests run on every OS).
    let log = format!("{}/agent.log", log_dir.to_string_lossy().trim_end_matches('/'));
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n<dict>\n\
\t<key>Label</key>\n\t<string>{label}</string>\n\
\t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{exe}</string>\n\t\t<string>run</string>\n\t</array>\n\
\t<key>RunAtLoad</key>\n\t<true/>\n\
\t<key>KeepAlive</key>\n\t<true/>\n\
\t<key>ThrottleInterval</key>\n\t<integer>10</integer>\n\
\t<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>\n\
\t<key>ProcessType</key>\n\t<string>Standard</string>\n\
\t<key>StandardErrorPath</key>\n\t<string>{log}</string>\n\
\t<key>StandardOutPath</key>\n\t<string>{log}</string>\n\
</dict>\n</plist>\n",
        label = super::LABEL,
        exe = xml(&exe.to_string_lossy()),
        log = xml(&log)
    )
}

fn systemd_quote(s: &str) -> String {
    if s.contains(|c: char| c.is_whitespace() || c == '"' || c == '\\') {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// The systemd user unit: restarted whenever it exits, low priority, started with the user's manager.
pub fn systemd_unit(exe: &Path) -> String {
    format!(
        "[Unit]\nDescription=Dyno agent (measures and keeps your PC's history)\nDocumentation=https://dev.decryptology.net/decryp7/dyno\n\n\
[Service]\nType=simple\nExecStart={exe} run\nRestart=always\nRestartSec=5\nNice=10\nCPUWeight=20\nIOSchedulingClass=best-effort\nIOSchedulingPriority=7\nMemoryHigh=256M\n\n\
[Install]\nWantedBy=default.target\n",
        exe = systemd_quote(&exe.to_string_lossy())
    )
}

/// The Run value: the quoted executable and its `run` argument.
pub fn windows_run_value(exe: &Path) -> String {
    format!("\"{}\" run", exe.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_names_the_label_the_program_and_keeps_it_alive() {
        let p = launch_agent_plist(
            Path::new("/Applications/Dyno & Co.app/Contents/MacOS/dyno-agent"),
            Path::new("/Users/me/Library/Logs/Dyno"),
        );
        assert!(p.contains("<string>net.decryptology.dyno.agent</string>"));
        assert!(p.contains("/Applications/Dyno &amp; Co.app/Contents/MacOS/dyno-agent"));
        assert!(p.contains("<key>KeepAlive</key>\n\t<true/>"));
        assert!(p.contains("<key>ThrottleInterval</key>\n\t<integer>10</integer>"));
        assert!(p.contains("/Users/me/Library/Logs/Dyno/agent.log"));
        plist::Value::from_reader_xml(p.as_bytes()).unwrap_or_else(|e| panic!("not a valid plist: {e}"));
    }

    #[test]
    fn unit_quotes_paths_with_spaces() {
        let u = systemd_unit(Path::new("/opt/Dyno App/dyno-agent"));
        assert!(u.contains("ExecStart=\"/opt/Dyno App/dyno-agent\" run"));
        assert!(u.contains("Restart=always"));
        assert!(systemd_unit(Path::new("/usr/bin/dyno-agent")).contains("ExecStart=/usr/bin/dyno-agent run"));
    }

    /// The agent answers every request of the app and reads the folders of a disk scan the person watches, so its disk access
    /// is never at Linux's idle I/O class, which reads only when nothing else uses the disk (rule 52): best effort at the
    /// lowest priority instead, which a busy disk slows but never stops.
    #[test]
    fn the_unit_never_uses_the_idle_io_class() {
        let u = systemd_unit(Path::new("/usr/bin/dyno-agent"));
        let io: Vec<&str> = u.lines().filter(|l| l.starts_with("IOScheduling")).collect();
        assert_eq!(io, ["IOSchedulingClass=best-effort", "IOSchedulingPriority=7"]);
    }

    /// Every request of the app is answered inside the agent, so the agent must not be a launchd `Background` job: launchd
    /// spawns those as `POSIX_SPAWN_PROC_TYPE_DAEMON_BACKGROUND`, which xnu makes DARWIN_BG with no exceptions
    /// (`osfmk/kern/task_policy.c`), clamping every thread to the `BACKGROUND` QoS band whatever the thread asks for.
    /// Checked for both plists: the one `dyno-agent install` writes and the one in the bundle that `SMAppService` registers.
    #[test]
    fn the_launch_agent_is_never_a_background_job() {
        let bundled_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src-tauri/macos/net.decryptology.dyno.agent.plist");
        let bundled = std::fs::read(&bundled_path).unwrap_or_else(|e| panic!("{}: {e}", bundled_path.display()));
        let written = launch_agent_plist(
            Path::new("/Applications/Dyno.app/Contents/MacOS/dyno-agent"),
            Path::new("/tmp/logs"),
        );
        for (name, bytes) in [("bundled", bundled.as_slice()), ("written", written.as_bytes())] {
            let value = plist::Value::from_reader_xml(bytes).unwrap_or_else(|e| panic!("{name}: not a valid plist: {e}"));
            let kind = value.as_dictionary().and_then(|d| d.get("ProcessType")).and_then(|v| v.as_string());
            assert_eq!(
                kind,
                Some("Standard"),
                "{name}: the agent answers the app's requests, so it must not be DARWIN_BG"
            );
        }
    }

    #[test]
    fn run_value_is_quoted() {
        assert_eq!(
            windows_run_value(Path::new(r"C:\Program Files\Dyno\dyno-agent.exe")),
            r#""C:\Program Files\Dyno\dyno-agent.exe" run"#
        );
    }
}
