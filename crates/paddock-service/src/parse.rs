//! Reading what the OS tools print, as pure functions over captured text, tested with real output. Anything unexpected
//! reads as "unknown", never as healthy.

use crate::render::windows_program_of;

/// What `launchctl print <domain>/<label>` says about a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JobReport {
    /// launchd knows the job at all (an empty report means it dropped it).
    pub found: bool,
    /// `state = running`.
    pub running: bool,
    /// `job state = spawn failed`.
    pub spawn_failed: bool,
    /// `last exit code = 78` (EX_CONFIG): launchd refused the program's code identity.
    pub exit_config: bool,
    /// `needs LWCR update`.
    pub needs_lwcr_update: bool,
    /// `has LWCR`: a launch constraint from the registered build is stored.
    pub has_lwcr: bool,
}

/// Reads the report of `launchctl print` (catalogue R03).
pub fn parse_job(launchctl_print: &str) -> JobReport {
    let mut job = JobReport::default();
    for line in launchctl_print.lines().map(str::trim) {
        if line.starts_with("state = ") {
            job.found = true;
            job.running = line == "state = running";
        } else if line == "job state = spawn failed" {
            job.spawn_failed = true;
        } else if line.starts_with("last exit code = 78") {
            job.exit_config = true;
        } else if line.starts_with("properties = ") {
            job.needs_lwcr_update = line.contains("needs LWCR update");
            job.has_lwcr = line.contains("has LWCR");
        }
    }
    job
}

impl JobReport {
    /// Whether launchd holds a registration it can no longer start. macOS keeps the launch constraint of the build that
    /// was registered; every ad-hoc signed build has another cdhash, so after an update launchd refuses the new binary
    /// (spawn failed, exit 78 EX_CONFIG, "needs LWCR update") until the job is replaced. A job that holds a constraint
    /// and is not running counts too: the caller only acts on it when the service does not answer.
    pub fn stale(&self) -> bool {
        self.found
            && (self.spawn_failed
                || self.exit_config
                || self.needs_lwcr_update
                || (self.has_lwcr && !self.running))
    }
}

/// Reads `label`'s entry in `launchctl print-disabled <domain>`: `"label" => disabled` (older systems print
/// `true`/`false`). `None` when the label is not listed.
pub fn parse_print_disabled(output: &str, label: &str) -> Option<bool> {
    let quoted = format!("\"{label}\"");
    output.lines().find_map(|line| {
        let rest = line
            .trim()
            .strip_prefix(&quoted)?
            .trim_start()
            .strip_prefix("=>")?;
        match rest.trim() {
            "disabled" | "true" => Some(true),
            "enabled" | "false" => Some(false),
            _ => None,
        }
    })
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The program a launchd plist starts: the first `<string>` of `ProgramArguments` (or `Program`). `None` when the text
/// has neither.
pub fn plist_program(plist: &str) -> Option<String> {
    for key in ["<key>Program</key>", "<key>ProgramArguments</key>"] {
        let Some(at) = plist.find(key) else { continue };
        let rest = &plist[at + key.len()..];
        let start = rest.find("<string>")? + "<string>".len();
        let len = rest[start..].find("</string>")?;
        let value = xml_unescape(rest[start..start + len].trim());
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

/// The executable of a unit's `ExecStart=` line (quoted or not, prefixes like `-` or `@` skipped).
pub fn unit_program(unit: &str) -> Option<String> {
    let line = unit
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("ExecStart="))?;
    let line = line.trim_start_matches(['-', '@', '+', '!', ':']);
    let word = if let Some(rest) = line.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => out.push(chars.next()?),
                c => out.push(c),
            }
        }
        out.replace("%%", "%").replace("$$", "$")
    } else {
        line.split_whitespace().next()?.to_string()
    };
    (!word.is_empty()).then_some(word)
}

/// What `systemctl is-active` printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitActivity {
    /// `active`.
    Active,
    /// `activating`, `deactivating`, `reloading`.
    Changing,
    /// `inactive`.
    Inactive,
    /// `failed`.
    Failed,
    /// Anything else.
    Unknown,
}

/// Reads `systemctl is-active` output.
pub fn parse_is_active(stdout: &str) -> UnitActivity {
    match stdout.trim() {
        "active" => UnitActivity::Active,
        "activating" | "deactivating" | "reloading" | "refreshing" => UnitActivity::Changing,
        "inactive" => UnitActivity::Inactive,
        "failed" => UnitActivity::Failed,
        _ => UnitActivity::Unknown,
    }
}

/// Whether `systemctl is-enabled` says the user switched the unit off (`disabled`, `masked`).
pub fn is_switched_off(is_enabled_stdout: &str) -> bool {
    matches!(
        is_enabled_stdout.trim(),
        "disabled" | "masked" | "masked-runtime"
    )
}

/// What a failed `systemctl` says, with the usual cause spelled out. `None` for the usual cause too, so the caller can
/// tell an unreachable user manager ([`systemd_unreachable`]) from a refusal.
pub fn explain_systemctl(stderr: &str) -> String {
    let first = stderr
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("it gave no reason")
        .trim();
    if systemd_unreachable(stderr) {
        format!(
            "{first}. The systemd user instance is not reachable: log in on the machine (or ssh with a full session) and try again"
        )
    } else {
        first.to_string()
    }
}

/// Whether `systemctl` could not reach the (user) manager at all.
pub fn systemd_unreachable(stderr: &str) -> bool {
    [
        "Failed to connect to bus",
        "No medium found",
        "XDG_RUNTIME_DIR",
        "System has not been booted with systemd",
    ]
    .iter()
    .any(|m| stderr.contains(m))
}

/// The data of `value` in `reg query <key> /v <value>` output (`    Dyno    REG_SZ    "C:\..." run`).
pub fn reg_value(reg_query: &str, value: &str) -> Option<(String, String)> {
    reg_query.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix(value)?;
        if !rest.starts_with([' ', '\t']) {
            return None;
        }
        let mut parts = rest.trim_start().splitn(2, [' ', '\t']);
        let kind = parts.next()?.to_string();
        if !kind.starts_with("REG_") {
            return None;
        }
        Some((kind, parts.next().unwrap_or("").trim().to_string()))
    })
}

/// The program a `Run` value starts.
pub fn run_value_program(reg_query: &str, value: &str) -> Option<String> {
    reg_value(reg_query, value).and_then(|(_, data)| windows_program_of(&data))
}

/// Whether the `StartupApproved\Run` entry (`REG_BINARY`, `02...` on, `03...` off) says the user switched it off.
pub fn startup_switched_off(reg_query: &str, value: &str) -> bool {
    reg_value(reg_query, value)
        .filter(|(kind, _)| kind == "REG_BINARY")
        .and_then(|(_, data)| u8::from_str_radix(data.get(..2)?, 16).ok())
        .is_some_and(|b| b & 1 == 1)
}

/// The service state number in `sc query` output (`STATE : 4  RUNNING`; numbers do not depend on the language).
pub fn sc_state(sc_query: &str) -> Option<u32> {
    sc_field(sc_query, "STATE")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// `SERVICE_RUNNING`.
pub const SC_RUNNING: u32 = 4;
/// `SERVICE_STOPPED`.
pub const SC_STOPPED: u32 = 1;
/// `SERVICE_DISABLED` start type.
pub const SC_DISABLED: u32 = 4;

fn sc_field<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim() == name).then(|| v.trim())
    })
}

/// The program and start type in `sc qc` output.
pub fn sc_config(sc_qc: &str) -> (Option<String>, Option<u32>) {
    let program = sc_qc.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim() == "BINARY_PATH_NAME").then(|| windows_program_of(v.trim()))?
    });
    let start =
        sc_field(sc_qc, "START_TYPE").and_then(|v| v.split_whitespace().next()?.parse().ok());
    (program, start)
}

/// `sc.exe` exit codes (it exits with the Win32 error).
pub mod sc_exit {
    /// Access denied: needs administrator rights.
    pub const ACCESS_DENIED: i32 = 5;
    /// The service does not exist.
    pub const DOES_NOT_EXIST: i32 = 1060;
    /// The service has not been started.
    pub const NOT_ACTIVE: i32 = 1062;
    /// Marked for deletion (the Services console holds it).
    pub const MARKED_FOR_DELETE: i32 = 1072;
    /// Exists already.
    pub const EXISTS: i32 = 1073;
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABEL: &str = "net.decryptology.pitwall.service";
    /// The real `launchctl print gui/501/net.decryptology.pitwall.service` of a Mac after an update (trimmed).
    pub(crate) const SPAWN_FAILED: &str = "gui/501/net.decryptology.pitwall.service = {\n\tactive count = 0\n\tpath = (submitted by smd.362)\n\ttype = Submitted\n\tmanaged_by = com.apple.xpc.ServiceManagement\n\tstate = spawn scheduled\n\n\tprogram identifier = Contents/MacOS/pitwall-server (mode: 2)\n\tparent bundle identifier = net.decryptology.pitwall\n\tparent bundle version = 1.0.2\n\truns = 534\n\tlast exit code = 78: EX_CONFIG\n\n\tjob state = spawn failed\n\tsanitizer flags = 0x0\n\n\tproperties = partial import | keepalive | runatload | resolve program | has LWCR\n}\n";
    pub(crate) const HEALTHY: &str = "gui/501/net.decryptology.pitwall.service = {\n\tstate = running\n\tpid = 88412\n\truns = 1\n\tlast exit code = (never exited)\n\tjob state = running\n\tproperties = keepalive | runatload | has LWCR\n}\n";

    #[test]
    fn a_registration_launchd_cannot_start_is_stale() {
        let job = parse_job(SPAWN_FAILED);
        assert!(job.found && !job.running && job.spawn_failed && job.exit_config && job.has_lwcr);
        assert!(!job.needs_lwcr_update && job.stale());
        assert!(parse_job("\tstate = spawn scheduled\n\tlast exit code = 78: EX_CONFIG\n").stale());
        assert!(parse_job("\tstate = spawn scheduled\n\tjob state = spawn failed\n").stale());
        assert!(
            parse_job("\tstate = spawn scheduled\n\tproperties = keepalive | has LWCR\n").stale()
        );
        assert!(
            parse_job("\tstate = waiting\n\tproperties = keepalive | needs LWCR update\n").stale()
        );
        let healthy = parse_job(HEALTHY);
        assert!(healthy.found && healthy.running && healthy.has_lwcr && !healthy.stale());
        assert!(!parse_job("").found && !parse_job("").stale());
    }

    #[test]
    fn the_disabled_list_is_parsed() {
        let out = format!(
            "disabled services = {{\n\t\"com.apple.x\" => enabled\n\t\"{LABEL}\" => disabled\n}}\n"
        );
        assert_eq!(parse_print_disabled(&out, LABEL), Some(true));
        assert_eq!(
            parse_print_disabled(&out.replace("=> disabled", "=> enabled"), LABEL),
            Some(false)
        );
        assert_eq!(
            parse_print_disabled(&out.replace("=> disabled", "=> true"), LABEL),
            Some(true)
        );
        assert_eq!(
            parse_print_disabled(&out.replace("=> disabled", "=> false"), LABEL),
            Some(false)
        );
        assert_eq!(
            parse_print_disabled(&out.replace("=> disabled", "=> maybe"), LABEL),
            None
        );
        assert_eq!(parse_print_disabled("", LABEL), None);
        assert_eq!(
            parse_print_disabled(&format!("\"{LABEL}\" disabled"), LABEL),
            None
        );
    }

    #[test]
    fn the_program_of_a_plist_is_read() {
        let p = "<key>ProgramArguments</key>\n<array>\n<string>/A &amp; B/x</string>\n<string>run</string></array>";
        assert_eq!(plist_program(p).as_deref(), Some("/A & B/x"));
        assert_eq!(
            plist_program("<key>Program</key><string>/p</string>").as_deref(),
            Some("/p")
        );
        assert_eq!(plist_program("<key>Label</key><string>x</string>"), None);
        assert_eq!(
            plist_program("<key>ProgramArguments</key><array></array>"),
            None
        );
        assert_eq!(plist_program("<key>ProgramArguments</key><string>"), None);
        assert_eq!(
            plist_program(
                "<key>Program</key><string> </string><key>ProgramArguments</key><string>/q</string>"
            )
            .as_deref(),
            Some("/q")
        );
    }

    #[test]
    fn the_program_of_a_unit_is_read() {
        assert_eq!(
            unit_program("[Service]\nExecStart=\"/opt/my app/x\" \"run\"\n").as_deref(),
            Some("/opt/my app/x")
        );
        assert_eq!(
            unit_program("ExecStart=\"/a\\\"b%%$$\"").as_deref(),
            Some("/a\"b%$")
        );
        assert_eq!(
            unit_program("ExecStart=-/usr/bin/x run").as_deref(),
            Some("/usr/bin/x")
        );
        assert_eq!(unit_program("[Unit]\n"), None);
        assert_eq!(unit_program("ExecStart=\"\""), None);
        assert_eq!(unit_program("ExecStart=\"/a\\"), None);
        assert_eq!(unit_program("ExecStart="), None);
    }

    #[test]
    fn systemctl_output_is_read() {
        assert_eq!(parse_is_active("active\n"), UnitActivity::Active);
        assert_eq!(parse_is_active("activating"), UnitActivity::Changing);
        assert_eq!(parse_is_active("inactive"), UnitActivity::Inactive);
        assert_eq!(parse_is_active("failed"), UnitActivity::Failed);
        assert_eq!(parse_is_active(""), UnitActivity::Unknown);
        assert!(is_switched_off("disabled\n") && is_switched_off("masked"));
        assert!(!is_switched_off("enabled") && !is_switched_off(""));
        assert!(
            explain_systemctl("Failed to connect to bus: No medium found")
                .contains("log in on the machine")
        );
        assert_eq!(
            explain_systemctl("\n  Unit x not found. \n"),
            "Unit x not found."
        );
        assert_eq!(explain_systemctl(""), "it gave no reason");
        assert!(systemd_unreachable(
            "System has not been booted with systemd as init system"
        ));
    }

    #[test]
    fn registry_output_is_read() {
        let q = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    Dyno    REG_SZ    \"C:\\Program Files\\Dyno\\dyno-agent.exe\" run\r\n\r\n";
        assert_eq!(reg_value(q, "Dyno").unwrap().0, "REG_SZ");
        assert_eq!(
            run_value_program(q, "Dyno").as_deref(),
            Some("C:\\Program Files\\Dyno\\dyno-agent.exe")
        );
        assert_eq!(
            run_value_program(q, "Dy"),
            None,
            "a prefix of another name is not the value"
        );
        assert_eq!(run_value_program(q, "Other"), None);
        assert_eq!(reg_value("    Dyno    NOTREG x", "Dyno"), None);
        let off = "    Dyno    REG_BINARY    030000000000000000000000\r\n";
        let on = "    Dyno    REG_BINARY    020000000000000000000000\r\n";
        assert!(startup_switched_off(off, "Dyno"));
        assert!(!startup_switched_off(on, "Dyno"));
        assert!(!startup_switched_off("", "Dyno"));
        assert!(!startup_switched_off("    Dyno    REG_SZ    03", "Dyno"));
        assert!(!startup_switched_off(
            "    Dyno    REG_BINARY    zz",
            "Dyno"
        ));
    }

    #[test]
    fn sc_output_is_read() {
        let q = "\r\nSERVICE_NAME: Pitwall\r\n        TYPE               : 10  WIN32_OWN_PROCESS\r\n        STATE              : 4  RUNNING\r\n";
        assert_eq!(sc_state(q), Some(SC_RUNNING));
        assert_eq!(sc_state("STATE : 1  STOPPED"), Some(SC_STOPPED));
        assert_eq!(sc_state("nothing"), None);
        let qc = "[SC] QueryServiceConfig SUCCESS\r\n\r\nSERVICE_NAME: Pitwall\r\n        START_TYPE         : 2   AUTO_START  (DELAYED)\r\n        BINARY_PATH_NAME   : \"C:\\Program Files\\Pitwall\\pitwall-server.exe\" --service\r\n";
        assert_eq!(
            sc_config(qc),
            (
                Some("C:\\Program Files\\Pitwall\\pitwall-server.exe".into()),
                Some(2)
            )
        );
        assert_eq!(sc_config(""), (None, None));
        assert_eq!(sc_exit::EXISTS, 1073);
    }
}
