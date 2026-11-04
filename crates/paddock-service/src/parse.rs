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
    /// `pid = <n>`: the process launchd runs for the job now (only while it runs).
    pub pid: Option<u32>,
    /// `last exit code = <n>`: how the job's last process exited (`None`: it never exited, or a signal ended it).
    pub last_exit_code: Option<i32>,
    /// `last terminating signal = …`: a signal ended the job's last process.
    pub last_signal: bool,
    /// The signal number in `last terminating signal = Killed: 9` (`None`: no signal, or none named).
    pub signal: Option<i32>,
    /// `runs = <n>`: how often launchd started the job's process.
    pub runs: Option<u32>,
}

/// Reads the report of `launchctl print` (catalogue R03). Only the job's own keys count: nested blocks (`arguments`,
/// `environment`, and on macOS 26 the resource and jetsam coalitions with their own `state = active`) are skipped.
pub fn parse_job(launchctl_print: &str) -> JobReport {
    let mut job = JobReport::default();
    // 0 before the `<target> = {` header (fakes print bare lines), 1 inside it, more inside a nested block.
    let mut depth = 0usize;
    for line in launchctl_print.lines().map(str::trim) {
        if line == "}" {
            depth = depth.saturating_sub(1);
            continue;
        }
        let own = depth <= 1;
        if line.ends_with('{') {
            depth += 1;
            continue;
        }
        if !own {
            continue;
        }
        if line.starts_with("state = ") {
            job.found = true;
            job.running = line == "state = running";
        } else if line == "job state = spawn failed" {
            job.spawn_failed = true;
        } else if let Some(code) = line.strip_prefix("last exit code = ") {
            // `78: EX_CONFIG` on newer systems, `78` on older ones, `(never exited)` before the first exit.
            job.last_exit_code = code.split(':').next().and_then(|c| c.trim().parse().ok());
            job.exit_config = job.last_exit_code == Some(78);
        } else if let Some(pid) = line.strip_prefix("pid = ") {
            job.pid = pid.trim().parse().ok().filter(|p| *p > 0);
        } else if let Some(sig) = line.strip_prefix("last terminating signal = ") {
            job.last_signal = true;
            job.signal = sig.rsplit(':').next().and_then(|n| n.trim().parse().ok());
        } else if let Some(runs) = line.strip_prefix("runs = ") {
            job.runs = runs.trim().parse().ok();
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
    /// (spawn failed, exit 78 EX_CONFIG, "needs LWCR update") until the job is replaced. `has LWCR` alone is not a sign:
    /// every healthy `SMAppService` job has it, and a pending or restarting job is not refused. A running job is not
    /// refused whatever an earlier run left in the report.
    pub fn stale(&self) -> bool {
        self.found
            && !self.running
            && (self.spawn_failed || self.exit_config || self.needs_lwcr_update)
    }

    /// Whether the registration is to be replaced now: launchd refuses it ([`stale`](Self::stale)) and the service does
    /// not answer (`answers`). The one decision every app uses; a running or pending job is never replaced because the
    /// app has not connected yet, which would kill the healthy agent.
    pub fn needs_repair(&self, answers: bool) -> bool {
        self.stale() && !answers
    }

    /// The observed state in words, for the log and the failure message.
    pub fn describe(&self) -> String {
        if !self.found {
            return "launchd does not know the job".into();
        }
        let mut parts = vec![if self.running {
            "running"
        } else {
            "not running"
        }];
        for (on, what) in [
            (self.spawn_failed, "spawn failed"),
            (self.exit_config, "exit code 78 (EX_CONFIG)"),
            (self.needs_lwcr_update, "needs LWCR update"),
            (self.has_lwcr, "has a launch constraint"),
        ] {
            if on {
                parts.push(what);
            }
        }
        parts.join(", ")
    }
}

/// `launchctl <args>` through `sys` (the real one has a timeout that ends the whole tree and reads the output while the
/// tool runs, so a long listing such as `print-disabled` never blocks on a full pipe). `None` when it cannot be run,
/// times out or exits non-zero. Output past the system's cap ([`crate::system::MAX_HELPER_OUTPUT`]) comes back truncated.
pub fn launchctl_output(sys: &mut dyn crate::port::System, args: &[&str]) -> Option<String> {
    let out = sys.run(crate::paths::LAUNCHCTL, args).ok()?;
    out.success().then_some(out.stdout)
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

/// The process id in `sc queryex` output (`PID : 1234`); `None` when the service has no process (`0`).
pub fn sc_pid(sc_queryex: &str) -> Option<u32> {
    sc_field(sc_queryex, "PID")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
        .filter(|p| *p != 0)
}

/// The privileges in `sc qprivs` output (`Se…Privilege` words, sorted, once each; the names do not depend on the language).
pub fn sc_privileges(sc_qprivs: &str) -> Vec<String> {
    let mut found: Vec<String> = sc_qprivs
        .split_whitespace()
        .filter(|w| {
            w.starts_with("Se")
                && w.ends_with("Privilege")
                && w.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .map(str::to_string)
        .collect();
    found.sort();
    found.dedup();
    found
}

/// What `systemctl show -p FragmentPath,UnitFileState,Result,NRestarts,ExecMainStatus,ExecMainCode` printed (`Key=Value`
/// lines; a key that is missing, empty or not a number reads as `None`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnitShow {
    /// `FragmentPath`: the unit file in force (empty for a masked or unknown unit).
    pub fragment: Option<String>,
    /// `UnitFileState`: `enabled`, `disabled`, `static`, `masked`, ...
    pub unit_file_state: Option<String>,
    /// `NRestarts`.
    pub restarts: Option<u32>,
    /// `ExecMainStatus`: the exit status, or the signal number when `ExecMainCode` says killed.
    pub exec_main_status: Option<i32>,
    /// `ExecMainCode`: `CLD_EXITED` (1), `CLD_KILLED` (2) or `CLD_DUMPED` (3); 0 when the process never ended.
    pub exec_main_code: Option<i32>,
}

/// Reads `systemctl show` output.
pub fn parse_systemctl_show(stdout: &str) -> UnitShow {
    let mut show = UnitShow::default();
    for (key, value) in stdout.lines().filter_map(|l| l.split_once('=')) {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "FragmentPath" => show.fragment = Some(value.to_string()),
            "UnitFileState" => show.unit_file_state = Some(value.to_string()),
            "NRestarts" => show.restarts = value.parse().ok(),
            "ExecMainStatus" => show.exec_main_status = value.parse().ok(),
            "ExecMainCode" => show.exec_main_code = value.parse().ok(),
            _ => {}
        }
    }
    show
}

/// The exit codes in `sc query` output of a stopped service: `(WIN32_EXIT_CODE, SERVICE_EXIT_CODE)`.
pub fn sc_exit_codes(sc_query: &str) -> Option<(u32, u32)> {
    let first = |name: &str| {
        sc_field(sc_query, name)?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    };
    Some((first("WIN32_EXIT_CODE")?, first("SERVICE_EXIT_CODE")?))
}

/// `ERROR_SERVICE_SPECIFIC_ERROR`: the real code is `SERVICE_EXIT_CODE`.
pub const SC_SERVICE_SPECIFIC: u32 = 1066;
/// `SERVICE_DEMAND_START` start type.
pub const SC_DEMAND_START: u32 = 3;
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
    #[test]
    fn the_service_process_id_is_read_from_queryex() {
        let text = "SERVICE_NAME: DynoHelper\n        TYPE               : 10  WIN32_OWN_PROCESS\n        STATE              : 4  RUNNING\n        PID                : 9884\n        FLAGS              :\n";
        assert_eq!(super::sc_pid(text), Some(9884));
        assert_eq!(super::sc_pid(&text.replace("9884", "0")), None);
        assert_eq!(super::sc_pid("nothing here"), None);
    }

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
            !parse_job("\tstate = spawn scheduled\n\tproperties = keepalive | has LWCR\n").stale(),
            "a constraint alone is not a refusal"
        );
        assert!(
            parse_job("\tstate = waiting\n\tproperties = keepalive | needs LWCR update\n").stale()
        );
        let healthy = parse_job(HEALTHY);
        assert!(healthy.found && healthy.running && healthy.has_lwcr && !healthy.stale());
        assert!(!parse_job("").found && !parse_job("").stale());
    }

    /// The real `launchctl print` of a bundled agent that runs on macOS 26 (trimmed): the job's own `state = running` is
    /// followed by nested blocks (resource and jetsam coalitions) with their own `state = active`.
    pub(crate) const RUNNING_MACOS_26: &str = "gui/501/net.decryptology.paddock-shell-demo.agent = {\n\tactive count = 1\n\tpath = (submitted by smd.87)\n\ttype = Submitted\n\tmanaged_by = com.apple.xpc.ServiceManagement\n\tstate = running\n\n\tprogram identifier = Contents/MacOS/shell-demo-agent (mode: 2)\n\targuments = {\n\t\tshell-demo-agent\n\t\trun\n\t}\n\n\tpid = 880\n\tlast exit code = (never exited)\n\n\tresource coalition = {\n\t\tID = 903\n\t\ttype = resource\n\t\tstate = active\n\t\tactive count = 1\n\t}\n\n\tjetsam coalition = {\n\t\tID = 904\n\t\ttype = jetsam\n\t\tstate = active\n\t\tactive count = 1\n\t}\n\n\tspawn type = background (5)\n\tjob state = running\n\n\tproperties = partial import | keepalive | runatload | low priority i/o | resolve program | has LWCR | tle system\n}\n";

    #[test]
    fn a_nested_block_state_is_not_the_jobs_state() {
        // Regression (C21 VM smoke, macOS 26): the coalitions' `state = active` overwrote the job's `state = running`,
        // so a healthy agent with a launch constraint read as stale ("refused") and Repair was offered for nothing.
        let job = parse_job(RUNNING_MACOS_26);
        assert!(job.found && job.running && job.has_lwcr, "{job:?}");
        assert!(!job.stale());
        // A nested block's keys never count, whichever way round.
        let nested_first = "x = {\n\tthing = {\n\t\tstate = running\n\t\tlast exit code = 78\n\t}\n\tstate = waiting\n}\n";
        let job = parse_job(nested_first);
        assert!(job.found && !job.running && !job.exit_config, "{job:?}");
        // An unbalanced closing brace does not wrap the depth round.
        assert!(parse_job("}\n}\nstate = running\n").running);
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
        let privs = "[SC] QueryServiceConfig2 SUCCESS\r\n\r\nSERVICE_NAME: DynoHelper\r\n        PRIVILEGES       : SeChangeNotifyPrivilege\r\n                           SeChangeNotifyPrivilege\r\n";
        assert_eq!(sc_privileges(privs), ["SeChangeNotifyPrivilege"]);
        assert!(sc_privileges("PRIVILEGES : \r\n").is_empty());
    }

    /// A real `launchctl print` of a healthy job (macOS 26.6.2, 2026-10-31): the whole report, label and ids invented.
    const REAL_RUNNING: &str = include_str!("../tests/fixtures/launchctl-print-running.txt");

    #[test]
    fn an_unknown_job_is_not_stale_whatever_a_stray_line_says() {
        assert!(!parse_job("\tjob state = spawn failed\n").stale());
        assert!(!parse_job("\tlast exit code = 78\n").stale());
    }

    #[test]
    fn a_real_running_print_is_not_stale() {
        let job = parse_job(REAL_RUNNING);
        assert!(job.found && job.running && job.has_lwcr, "{job:?}");
        assert!(
            !job.spawn_failed && !job.exit_config && !job.needs_lwcr_update,
            "{job:?}"
        );
        assert!(!job.stale());
    }

    #[test]
    fn a_pending_job_with_a_constraint_is_not_refused_and_not_repaired() {
        // Regression (Dyno B4): a pending job at login ("spawn scheduled", `has LWCR`) read as stale and its repair
        // killed the healthy agent.
        let pending = REAL_RUNNING
            .replacen("\tstate = running", "\tstate = spawn scheduled", 1)
            .replace("job state = running", "job state = pending");
        let job = parse_job(&pending);
        assert!(
            job.found && !job.running && job.has_lwcr && !job.stale(),
            "{job:?}"
        );
        assert!(!job.needs_repair(false));
        let failed = pending.replace("job state = pending", "job state = spawn failed");
        assert!(parse_job(&failed).stale() && parse_job(&failed).needs_repair(false));
        // `last exit code = 78` in a nested block is not the job's.
        let nested = REAL_RUNNING.replace(
            "\t\tname = net.example.app.agent\n\t}\n\n\tjetsam",
            "\t\tlast exit code = 78\n\t}\n\n\tjetsam",
        );
        assert!(!parse_job(&nested).exit_config);
    }

    #[test]
    fn repair_is_only_for_a_refused_job_that_does_not_answer() {
        let refused = parse_job(SPAWN_FAILED);
        assert!(refused.needs_repair(false));
        assert!(
            !refused.needs_repair(true),
            "an answering service is left alone"
        );
        // A running job is never replaced, even with a refusal sign left from an earlier run.
        let mut running = parse_job(REAL_RUNNING);
        running.exit_config = true;
        running.needs_lwcr_update = true;
        assert!(!running.stale() && !running.needs_repair(false));
        assert!(
            !JobReport::default().needs_repair(false),
            "an unknown job is not repaired"
        );
    }

    #[test]
    fn the_observed_state_is_described_in_words() {
        assert_eq!(
            JobReport::default().describe(),
            "launchd does not know the job"
        );
        assert_eq!(
            parse_job(REAL_RUNNING).describe(),
            "running, has a launch constraint"
        );
        assert_eq!(
            parse_job(SPAWN_FAILED).describe(),
            "not running, spawn failed, exit code 78 (EX_CONFIG), has a launch constraint"
        );
        assert_eq!(
            parse_job("state = waiting\nproperties = needs LWCR update\n").describe(),
            "not running, needs LWCR update"
        );
    }

    #[test]
    fn launchctl_output_is_the_stdout_of_a_successful_call_only() {
        use crate::port::{Output, RunError};
        use crate::testing::FakeSystem;
        let mut sys = FakeSystem::new();
        sys.answer("/bin/launchctl print gui/501/x", Output::ok(REAL_RUNNING));
        sys.answer("/bin/launchctl print gui/501/gone", Output::code(113));
        sys.on("/bin/launchctl print gui/501/slow", |_, _| {
            Err(RunError::TimedOut("launchctl".into()))
        });
        assert_eq!(
            launchctl_output(&mut sys, &["print", "gui/501/x"]).as_deref(),
            Some(REAL_RUNNING)
        );
        assert_eq!(launchctl_output(&mut sys, &["print", "gui/501/gone"]), None);
        assert_eq!(launchctl_output(&mut sys, &["print", "gui/501/slow"]), None);
    }

    /// A launcher that runs a program as a launchd job tracks it by launchd's report: the pid while it runs, then how it
    /// exited (Dyno ADR 0026, amendment "the app starts its agent through launchd"). Only the job's own keys count.
    #[test]
    fn the_pid_and_the_last_exit_are_read_from_the_jobs_own_keys() {
        let running = parse_job(HEALTHY);
        assert_eq!(
            (running.pid, running.last_exit_code, running.last_signal),
            (Some(88412), None, false)
        );
        // A real macOS 26 report of a job that runs again after an earlier exit: the coalitions' blocks are skipped.
        let real = parse_job(REAL_RUNNING);
        assert_eq!(
            (real.running, real.pid, real.last_exit_code),
            (true, Some(978), Some(3))
        );
        let exited =
            parse_job("x = {\n\tstate = not running\n\truns = 1\n\tlast exit code = 3\n}\n");
        assert_eq!(
            (
                exited.found,
                exited.running,
                exited.pid,
                exited.last_exit_code
            ),
            (true, false, None, Some(3))
        );
        assert!(!exited.exit_config && !exited.stale());
        let refused = parse_job(SPAWN_FAILED);
        assert_eq!(refused.last_exit_code, Some(78));
        assert!(refused.exit_config);
        let signalled = parse_job(
            "x = {\n\tstate = not running\n\tlast exit code = (never exited)\n\tlast terminating signal = Terminated: 15\n}\n",
        );
        assert_eq!(
            (signalled.last_exit_code, signalled.last_signal),
            (None, true)
        );
        // A pid or an exit code inside a nested block (a coalition, the environment) is not the job's.
        let nested = parse_job(
            "x = {\n\tstate = not running\n\tthing = {\n\t\tpid = 7\n\t\tlast exit code = 9\n\t}\n}\n",
        );
        assert_eq!((nested.pid, nested.last_exit_code), (None, None));
        assert_eq!(parse_job("x = {\n\tpid = 0\n\tpid = junk\n}\n").pid, None);
        let other = parse_job("x = {\n\tlast exit code = 780\n}\n");
        assert_eq!(
            (other.last_exit_code, other.exit_config),
            (Some(780), false)
        );
    }
}
