//! The OS readers behind the extras (WP-13) and the backup checks (WP-14). Every function runs read-only tools through a
//! [`ToolRunner`] (so tests script the answers) with a timeout, and hands their output to the pure parsers in
//! `dyno_core`. The OS is a parameter (`std::env::consts::OS` values), so every branch is tested on every host.
//! A tool that is missing, slow or refused becomes an "unavailable: reason" in the report, never an error.

use crate::runner::ToolRunner;
use dyno_core::backup::{self, BackupState, BackupStatus, BackupTool};
use dyno_core::extras::boot::{self, BootStep};
use dyno_core::extras::gpu::{self, GpuReport};
use dyno_core::extras::sleep::{self, SleepReport};
use dyno_core::extras::software::{self, SoftwareInventory};
use dyno_core::extras::usage::{self, EnergySample};
use dyno_core::helper_extras::{parse_requests, parse_versions};
use dyno_core::ports::PlatformError;
use dyno_core::time::{LocalZone, Timestamp};
use std::time::Duration;

/// How long a quick tool may take.
const QUICK: Duration = Duration::from_secs(10);
/// How long the slow ones (software lists, event logs) may take.
const SLOW: Duration = Duration::from_secs(45);

fn run(r: &dyn ToolRunner, program: &str, args: &[&str], timeout: Duration) -> Result<String, PlatformError> {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    r.run(program, &args, timeout)
}

/// Why a tool could not be read, as one sentence.
fn why(tool: &str, e: &PlatformError) -> String {
    match e {
        PlatformError::Unsupported(m)
        | PlatformError::AccessDenied(m)
        | PlatformError::Timeout(m)
        | PlatformError::Failed(m)
        | PlatformError::NotFound(m) => {
            if m.is_empty() {
                format!("{tool} could not be read.")
            } else {
                m.clone()
            }
        }
    }
}

/// What keeps this Windows computer awake, read with the caller's own rights (`powercfg /requests`; many Windows versions
/// refuse without administrator rights). The text is parsed by the one parser the helper uses too.
///
/// # Errors
/// The tool's refusal or failure.
pub fn windows_wake_requests(r: &dyn ToolRunner) -> Result<Vec<sleep::SleepBlocker>, PlatformError> {
    let text = run(r, "powercfg", &["/requests"], QUICK)?;
    Ok(sleep::blockers_from_requests(&parse_requests(&text)))
}

/// The sentence for a refused `powercfg /requests`.
pub fn windows_wake_refused(e: &PlatformError) -> String {
    format!(
        "Windows only lets an administrator list what keeps it awake ({}).",
        why("powercfg", e)
    )
}

/// Sleep blockers and the last wakes.
pub fn sleep_report(r: &dyn ToolRunner, os: &str) -> SleepReport {
    sleep_report_with(r, os, &mut || windows_wake_requests(r).map_err(|e| windows_wake_refused(&e)))
}

/// [`sleep_report`] where the caller reads what keeps a Windows computer awake (the agent asks the helper first). `windows` is
/// called only for `os == "windows"`; its `Err` is the sentence for `blockers_unavailable`.
pub fn sleep_report_with(
    r: &dyn ToolRunner,
    os: &str,
    windows: &mut dyn FnMut() -> Result<Vec<sleep::SleepBlocker>, String>,
) -> SleepReport {
    let mut report = SleepReport::default();
    match os {
        "macos" => {
            match run(r, "/usr/bin/pmset", &["-g", "assertions"], QUICK) {
                Ok(t) => report.blockers = sleep::parse_pmset_assertions(&t),
                Err(e) => report.blockers_unavailable = Some(why("pmset", &e)),
            }
            match run(r, "/usr/bin/pmset", &["-g", "log"], SLOW) {
                Ok(t) => report.wakes = sleep::parse_pmset_wakes(&t),
                Err(e) => report.wakes_unavailable = Some(why("pmset", &e)),
            }
        }
        "windows" => {
            // `powercfg /requests` needs administrator rights on many Windows versions; say so instead of showing nothing.
            match windows() {
                Ok(b) => report.blockers = b,
                Err(why) => report.blockers_unavailable = Some(why),
            }
            match run(r, "powercfg", &["/lastwake"], QUICK) {
                Ok(t) => report.wakes = sleep::parse_powercfg_lastwake(&t),
                Err(e) => report.wakes_unavailable = Some(why("powercfg", &e)),
            }
        }
        "linux" => {
            match run(r, "systemd-inhibit", &["--list", "--no-pager"], QUICK) {
                Ok(t) => report.blockers = sleep::parse_systemd_inhibit(&t),
                Err(e) => report.blockers_unavailable = Some(why("systemd-inhibit", &e)),
            }
            report.wakes_unavailable =
                Some("Linux keeps no simple record of what woke the computer; check the journal (journalctl -b -t systemd-sleep).".into());
        }
        _ => {
            report.blockers_unavailable = Some("This system has no sleep report Dyno can read.".into());
            report.wakes_unavailable = report.blockers_unavailable.clone();
        }
    }
    report
}

/// What the OS says about the last start-up: its total (when it tells), what slowed it, and when the computer started
/// (macOS, which keeps no breakdown).
pub struct BootFacts {
    /// Total seconds.
    pub total_secs: Option<f32>,
    /// What slowed it.
    pub steps: Vec<BootStep>,
    /// When the computer started, when the OS tells.
    pub started_at: Option<Timestamp>,
    /// Why the breakdown is unavailable.
    pub unavailable: Option<String>,
}

/// Reads the boot facts.
pub fn boot_facts(r: &dyn ToolRunner, os: &str) -> BootFacts {
    let mut f = BootFacts {
        total_secs: None,
        steps: Vec::new(),
        started_at: None,
        unavailable: None,
    };
    match os {
        "linux" => {
            match run(r, "systemd-analyze", &["time"], QUICK) {
                Ok(t) => f.total_secs = boot::parse_systemd_time(&t),
                Err(e) => f.unavailable = Some(why("systemd-analyze", &e)),
            }
            if f.unavailable.is_none() {
                match run(r, "systemd-analyze", &["blame", "--no-pager"], QUICK) {
                    Ok(t) => f.steps = boot::parse_systemd_blame(&t),
                    Err(e) => f.unavailable = Some(why("systemd-analyze", &e)),
                }
            }
        }
        "windows" => {
            let args_for = |query: String, count: &str| -> Vec<String> {
                vec![
                    "qe".into(),
                    "Microsoft-Windows-Diagnostics-Performance/Operational".into(),
                    format!("/q:{query}"),
                    format!("/c:{count}"),
                    "/rd:true".into(),
                    "/f:xml".into(),
                ]
            };
            match r.run("wevtutil", &args_for("*[System[(EventID=100)]]".into(), "1"), SLOW) {
                Ok(t) => f.total_secs = boot::parse_windows_boot_total(&t),
                Err(e) => f.unavailable = Some(why("wevtutil", &e)),
            }
            if f.unavailable.is_none() {
                match r.run(
                    "wevtutil",
                    &args_for("*[System[(EventID >= 101 and EventID <= 110)]]".into(), "20"),
                    SLOW,
                ) {
                    Ok(t) => f.steps = boot::parse_windows_boot_degradations(&t),
                    Err(e) => f.unavailable = Some(why("wevtutil", &e)),
                }
            }
        }
        "macos" => {
            if let Ok(t) = run(r, "/usr/sbin/sysctl", &["kern.boottime"], QUICK) {
                f.started_at = boot::parse_macos_boottime(&t);
            }
            f.unavailable =
                Some("macOS keeps no record of what slowed start-up; Dyno shows how long each start-up took until it settled.".into());
        }
        _ => f.unavailable = Some("This system keeps no boot breakdown Dyno can read.".into()),
    }
    f
}

/// Energy impact per process where the OS reports it (macOS), or why not.
pub fn energy(r: &dyn ToolRunner, os: &str) -> Result<Vec<EnergySample>, String> {
    match os {
        "macos" => run(
            r,
            "/usr/bin/top",
            &["-l", "2", "-s", "1", "-n", "40", "-o", "power", "-stats", "pid,command,power"],
            SLOW,
        )
        .map(|t| usage::parse_top_power(&t))
        .map_err(|e| why("top", &e)),
        "windows" => Err("Windows reports energy per app only through event tracing, which needs administrator rights.".into()),
        _ => Err("This system has no per-app energy meter an ordinary program can read.".into()),
    }
}

/// GPU use per process. `pause` is how long to wait between the two macOS reads (a rate needs two).
pub fn gpu_report(r: &dyn ToolRunner, os: &str, pause: Duration) -> GpuReport {
    match os {
        "macos" => {
            let read =
                || run(r, "/usr/sbin/ioreg", &["-r", "-c", "AGXAccelerator", "-l", "-w0"], QUICK).map(|t| gpu::parse_ioreg_gpu_time(&t));
            let started = std::time::Instant::now();
            let first = match read() {
                Ok(a) => a,
                Err(e) => return unavailable(&why("ioreg", &e)),
            };
            std::thread::sleep(pause);
            let second = match read() {
                Ok(a) => a,
                Err(e) => return unavailable(&why("ioreg", &e)),
            };
            if first.is_empty() && second.is_empty() {
                return unavailable("This Mac's graphics driver does not report per-process use.");
            }
            let elapsed = started.elapsed().as_millis().max(1) as u64;
            GpuReport {
                processes: gpu::macos_rates(&first, &second, elapsed),
                source: Some("ioreg (accelerator client time)".into()),
                unavailable: None,
            }
        }
        "windows" => match r.run(
            "typeperf",
            &["\\GPU Engine(*)\\Utilization Percentage".to_string(), "-sc".into(), "1".into()],
            QUICK,
        ) {
            Ok(t) => GpuReport {
                processes: gpu::parse_typeperf_gpu(&t),
                source: Some("GPU Engine performance counters".into()),
                unavailable: None,
            },
            Err(e) => unavailable(&why("typeperf", &e)),
        },
        _ => match run(r, "nvidia-smi", &["pmon", "-c", "1", "-s", "um"], QUICK) {
            Ok(t) => GpuReport {
                processes: gpu::parse_nvidia_pmon(&t),
                source: Some("nvidia-smi pmon".into()),
                unavailable: None,
            },
            Err(PlatformError::Unsupported(_)) => {
                unavailable("Per-process GPU use on Linux needs the vendor's tool (NVIDIA's nvidia-smi); none was found.")
            }
            Err(e) => unavailable(&why("nvidia-smi", &e)),
        },
    }
}

fn unavailable(why: &str) -> GpuReport {
    GpuReport {
        unavailable: Some(why.into()),
        ..Default::default()
    }
}

/// The installed software.
pub fn software_list(r: &dyn ToolRunner, os: &str) -> SoftwareInventory {
    match os {
        "macos" => match run(
            r,
            "/usr/sbin/system_profiler",
            &["SPApplicationsDataType", "-json"],
            Duration::from_secs(90),
        ) {
            Ok(t) => SoftwareInventory::from_items(software::parse_system_profiler(&t)),
            Err(e) => SoftwareInventory::unavailable(why("system_profiler", &e)),
        },
        "windows" => {
            let mut items = Vec::new();
            let mut last_err = None;
            for key in [
                "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                "HKLM\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            ] {
                match run(r, "reg", &["query", key, "/s"], SLOW) {
                    Ok(t) => items.extend(software::parse_reg_uninstall(&t)),
                    Err(e) => last_err = Some(e),
                }
            }
            match (items.is_empty(), last_err) {
                (true, Some(e)) => SoftwareInventory::unavailable(why("reg", &e)),
                _ => SoftwareInventory::from_items(items),
            }
        }
        _ => {
            let attempts: [(&str, &[&str]); 3] = [
                ("dpkg-query", &["-W", "-f=${Package}\t${Version}\n"]),
                ("rpm", &["-qa", "--qf", "%{NAME}\t%{VERSION}-%{RELEASE}\n"]),
                ("pacman", &["-Q"]),
            ];
            for (program, args) in attempts {
                if let Ok(t) = run(r, program, args, SLOW) {
                    return SoftwareInventory::from_items(software::parse_package_list(&t));
                }
            }
            SoftwareInventory::unavailable("No package manager Dyno knows (dpkg, rpm, pacman) answered.")
        }
    }
}

/// What the backup check needs from the agent: which tools exist and how to run them.
pub trait BackupEnv {
    /// Whether `tool`'s program exists (always true for tools read from the OS's own logs).
    fn available(&self, tool: BackupTool) -> bool;
    /// A runner for `tool` (resolves its program through the login-shell `PATH`, niced, with the timeout the caller gives).
    fn runner(&self, tool: BackupTool) -> Box<dyn ToolRunner + '_>;
}

/// The backup tools that exist on this OS and are installed, in the order they are checked. Tools that are not installed
/// are left out: "no backup tool found" is a normal state.
pub fn backup_tools_present(env: &dyn BackupEnv, os: &str) -> Vec<BackupTool> {
    BackupTool::ALL.into_iter().filter(|t| t.on_os(os) && env.available(*t)).collect()
}

/// Checks one backup tool (read-only tools with timeouts). One tool failing never affects the others.
pub fn backup_status_of(
    env: &dyn BackupEnv,
    tool: BackupTool,
    now: Timestamp,
    zone: &dyn LocalZone,
    stale_after_hours: u32,
) -> BackupStatus {
    let runner = env.runner(tool);
    check_one(runner.as_ref(), tool, now, zone, stale_after_hours)
}

/// Checks every backup tool that exists on this OS and is installed.
pub fn backup_statuses(env: &dyn BackupEnv, os: &str, now: Timestamp, zone: &dyn LocalZone, stale_after_hours: u32) -> Vec<BackupStatus> {
    backup_tools_present(env, os)
        .into_iter()
        .map(|tool| backup_status_of(env, tool, now, zone, stale_after_hours))
        .collect()
}

fn check_one(r: &dyn ToolRunner, tool: BackupTool, now: Timestamp, zone: &dyn LocalZone, stale: u32) -> BackupStatus {
    match tool {
        BackupTool::TimeMachine => {
            let dest = run(r, "tmutil", &["destinationinfo"], QUICK);
            let latest = run(r, "tmutil", &["latestbackup"], QUICK);
            let d = dest.as_ref().map(String::as_str).map_err(|e| why("tmutil", e));
            let l = latest.as_ref().map(String::as_str).map_err(|e| why("tmutil", e));
            backup::time_machine_status(
                d.as_deref().map_err(String::as_str),
                l.as_deref().map_err(String::as_str),
                now,
                zone,
                stale,
            )
        }
        BackupTool::WindowsBackup => windows_backup(r, now, stale),
        BackupTool::FileHistory => {
            let args: Vec<String> = [
                "qe",
                "Microsoft-Windows-FileHistory-Core/WHC",
                // The finished backups and the errors, newest first: an error after the last good backup means it failed.
                "/q:*[System[(EventID=201) or (Level=1 or Level=2)]]",
                "/c:20",
                "/rd:true",
                "/f:xml",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            match r.run("wevtutil", &args, SLOW) {
                Ok(t) => backup::file_history_status(backup::parse_file_history_events(&t), now, stale),
                Err(e) => file_history_problem(tool, &e),
            }
        }
        BackupTool::Restic => match run(
            r,
            "restic",
            &["snapshots", "--latest", "1", "--json", "--no-lock"],
            Duration::from_secs(60),
        ) {
            Ok(t) => match backup::parse_restic_latest(&t) {
                Some(at) => backup::judged(tool, at, now, stale),
                None => backup::no_backup(tool, "restic's repository has no snapshot yet."),
            },
            Err(e) => repo_problem(tool, &e, "RESTIC_REPOSITORY"),
        },
        BackupTool::Borg => match run(r, "borg", &["list", "--last", "1", "--json"], Duration::from_secs(60)) {
            Ok(t) => match backup::parse_borg_latest(&t, zone) {
                Some(at) => backup::judged(tool, at, now, stale),
                None => backup::no_backup(tool, "Borg's repository has no archive yet."),
            },
            Err(e) => repo_problem(tool, &e, "BORG_REPO"),
        },
        BackupTool::Duplicati => BackupStatus {
            next_step: Some("Open Duplicati's own page to see its last result.".into()),
            ..backup::cannot_check(tool, "Duplicati is installed, but Dyno cannot read its results yet.")
        },
    }
}

/// A restic/Borg failure: usually the repository is not set in the environment Dyno sees.
/// A File History log that could not be read. Windows creates the log the first time File History is turned on, so a log that
/// does not exist means it never ran here: not set up, not a failure. Anything else stays "could not check", with a next step.
fn file_history_problem(tool: BackupTool, e: &PlatformError) -> BackupStatus {
    let m = why("wevtutil", e);
    if m.to_lowercase().contains("channel could not be found") {
        backup::not_configured(
            tool,
            "File History has never run on this computer.",
            "Open Control Panel > File History to turn it on, or ignore this if you back up another way.",
        )
    } else {
        BackupStatus {
            next_step: Some(
                "Choose Check now to try again. If it keeps failing, open File History in Control Panel to see its own state.".into(),
            ),
            ..backup::cannot_check(tool, &m)
        }
    }
}

fn repo_problem(tool: BackupTool, e: &PlatformError, var: &str) -> BackupStatus {
    let m = why(tool.name(), e);
    let lower = m.to_lowercase();
    if lower.contains("repository") || lower.contains("repo") || lower.contains("passphrase") || lower.contains("password") {
        backup::not_configured(
            tool,
            &format!("{} is installed but Dyno could not open its repository.", tool.name()),
            &format!("Export {var} (and a password file or command) in your shell profile so Dyno can read it; Dyno never reads passwords themselves."),
        )
    } else {
        backup::cannot_check(tool, &m)
    }
}

/// What this Windows computer's own tools say about Windows Backup, with the caller's rights: the event log first (no
/// administrator needed), then `wbadmin get versions` (administrator only). The versions are parsed by the one parser the
/// helper uses too.
///
/// # Errors
/// `wbadmin`'s refusal or failure when the event log had nothing.
pub fn windows_backup_facts(r: &dyn ToolRunner) -> Result<backup::WindowsBackupFacts, PlatformError> {
    let args: Vec<String> = [
        "qe",
        "Microsoft-Windows-Backup",
        "/q:*[System[(EventID=4)]]",
        "/c:5",
        "/rd:true",
        "/f:xml",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Ok(t) = r.run("wevtutil", &args, SLOW) {
        if let Some((at, _)) = backup::parse_event_log_latest(&t) {
            return Ok(backup::WindowsBackupFacts {
                count: None,
                latest: Some(at),
            });
        }
    }
    let text = run(r, "wbadmin", &["get", "versions"], SLOW)?;
    Ok(backup::WindowsBackupFacts::from_versions(&parse_versions(&text)))
}

/// The sentence for a refused `wbadmin get versions`.
pub fn windows_backup_refused_text(e: &PlatformError) -> String {
    format!(
        "Windows only lets an administrator list Windows Backup versions ({}).",
        why("wbadmin", e)
    )
}

fn windows_backup(r: &dyn ToolRunner, now: Timestamp, stale: u32) -> BackupStatus {
    match windows_backup_facts(r) {
        Ok(f) => f.status(now, stale),
        Err(e) => backup::cannot_check(BackupTool::WindowsBackup, &windows_backup_refused_text(&e)),
    }
}

/// Whether a status needs attention (used to decide if the check should raise an alarm).
pub fn needs_attention(s: &BackupStatus) -> bool {
    matches!(s.state, BackupState::Stale | BackupState::NoBackupYet | BackupState::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issues::testing::Scripted;
    use dyno_core::time::FixedZone;

    fn ok(s: &str) -> Result<String, PlatformError> {
        Ok(s.to_string())
    }

    fn fail(m: &str) -> Result<String, PlatformError> {
        Err(PlatformError::Failed(m.to_string()))
    }

    #[test]
    fn sleep_per_os_and_when_a_tool_refuses() {
        let r = Scripted::new(vec![
            ok("Listed by owning process:\n   pid 1(Zoom): [0x1] 00:01:00 PreventUserIdleDisplaySleep named: \"call\"\n"),
            ok("2026-10-23 08:01:12 +0800 Wake  Wake from Normal Sleep : due to EC.LidOpen\n"),
        ]);
        let s = sleep_report(&r, "macos");
        assert_eq!(s.blockers.len(), 1);
        assert_eq!(s.wakes.len(), 1);
        assert!(s.blockers_unavailable.is_none());
        assert_eq!(r.calls.lock().unwrap()[0].0, "/usr/bin/pmset");

        let r = Scripted::new(vec![fail("Access is denied."), ok("Wake History Count - 0\n")]);
        let s = sleep_report(&r, "windows");
        assert!(s.blockers_unavailable.unwrap().contains("administrator"));
        assert!(s.wakes.is_empty() && s.wakes_unavailable.is_none());

        let r = Scripted::new(vec![ok(
            "WHO UID USER PID COMM WHAT WHY MODE\nFirefox 1000 sam 2210 firefox sleep Playing audio block\n",
        )]);
        let s = sleep_report(&r, "linux");
        assert_eq!(s.blockers.len(), 1);
        assert!(s.wakes_unavailable.is_some());

        assert!(sleep_report(&Scripted::new(vec![]), "freebsd").blockers_unavailable.is_some());
    }

    #[test]
    fn boot_facts_per_os() {
        let r = Scripted::new(vec![
            ok("Startup finished in 3.0s (kernel) + 20.0s (userspace) = 23.0s\n"),
            ok("12.0s slow.service\n1.0s fast.service\n"),
        ]);
        let f = boot_facts(&r, "linux");
        assert_eq!(f.total_secs, Some(23.0));
        assert_eq!(f.steps[0].name, "slow.service");
        let f = boot_facts(&Scripted::new(vec![fail("systemd-analyze: not found")]), "linux");
        assert!(f.unavailable.is_some() && f.steps.is_empty());

        let xml =
            "<Event xmlns='x'><System><EventID>100</EventID></System><EventData><Data Name='BootTime'>41000</Data></EventData></Event>";
        let r = Scripted::new(vec![ok(xml), ok("")]);
        let f = boot_facts(&r, "windows");
        assert_eq!(f.total_secs, Some(41.0));
        assert!(r.calls.lock().unwrap()[0].1.iter().any(|a| a.contains("EventID=100")));
        assert!(r.calls.lock().unwrap()[1].1.iter().any(|a| a.contains("EventID >= 101")));

        let f = boot_facts(&Scripted::new(vec![ok("kern.boottime: { sec = 1792628306, usec = 0 } x")]), "macos");
        assert_eq!(f.started_at, Some(Timestamp(1_792_628_306_000)));
        assert!(f.unavailable.unwrap().contains("macOS keeps no record"));
        assert!(boot_facts(&Scripted::new(vec![]), "plan9").unavailable.is_some());
    }

    #[test]
    fn energy_only_where_the_os_reports_it() {
        let r = Scripted::new(vec![ok("PID COMMAND POWER\n5 thing 4.5\n")]);
        assert_eq!(energy(&r, "macos").unwrap()[0].impact, 4.5);
        assert!(energy(&Scripted::new(vec![fail("x")]), "macos").is_err());
        assert!(energy(&r, "windows").unwrap_err().contains("administrator"));
        assert!(energy(&r, "linux").is_err());
    }

    #[test]
    fn gpu_per_os() {
        let io1 =
            "  |     \"IOUserClientCreator\" = \"pid 433, WindowServer\"\n  |     \"AppUsage\" = ({\"accumulatedGPUTime\"=1000000000})\n";
        let io2 =
            "  |     \"IOUserClientCreator\" = \"pid 433, WindowServer\"\n  |     \"AppUsage\" = ({\"accumulatedGPUTime\"=1200000000})\n";
        let g = gpu_report(&Scripted::new(vec![ok(io1), ok(io2)]), "macos", Duration::from_millis(20));
        assert_eq!(g.processes.len(), 1);
        assert!(g.processes[0].gpu_percent > 0.0);
        let g = gpu_report(&Scripted::new(vec![ok(""), ok("")]), "macos", Duration::ZERO);
        assert!(g.unavailable.unwrap().contains("does not report"));
        assert!(gpu_report(&Scripted::new(vec![fail("nope")]), "macos", Duration::ZERO)
            .unavailable
            .is_some());

        let t = "\"(PDH-CSV 4.0)\",\"\\\\PC\\GPU Engine(pid_9_luid_0x0_0x1_phys_0_eng_0_engtype_3D)\\Utilization Percentage\"\n\"10/23/2026 10:00:00.1\",\"12.5\"\n";
        let g = gpu_report(&Scripted::new(vec![ok(t)]), "windows", Duration::ZERO);
        assert_eq!(g.processes[0].pid, 9);

        let g = gpu_report(
            &Scripted::new(vec![ok("# gpu pid type sm mem enc dec fb command\n0 7 G 10 2 - - 50 x\n")]),
            "linux",
            Duration::ZERO,
        );
        assert_eq!(g.processes[0].pid, 7);
        let g = gpu_report(
            &Scripted::new(vec![Err(PlatformError::Unsupported("nvidia-smi is not available".into()))]),
            "linux",
            Duration::ZERO,
        );
        assert!(g.unavailable.unwrap().contains("vendor"));
    }

    #[test]
    fn software_per_os() {
        let sp = r#"{"SPApplicationsDataType":[{"_name":"Safari","version":"26"}]}"#;
        assert_eq!(software_list(&Scripted::new(vec![ok(sp)]), "macos").items.len(), 1);
        assert!(software_list(&Scripted::new(vec![fail("x")]), "macos").unavailable.is_some());
        let reg = "HKEY_LOCAL_MACHINE\\x\n    DisplayName    REG_SZ    Editor\n";
        let w = software_list(&Scripted::new(vec![ok(reg), fail("no key"), ok("")]), "windows");
        assert_eq!(w.items.len(), 1);
        assert!(software_list(&Scripted::new(vec![fail("a"), fail("b"), fail("c")]), "windows")
            .unavailable
            .is_some());
        let l = software_list(&Scripted::new(vec![fail("no dpkg"), ok("bash\t5.2-1\n")]), "linux");
        assert_eq!(l.items[0].name, "bash");
        assert!(software_list(&Scripted::new(vec![]), "linux")
            .unavailable
            .unwrap()
            .contains("package manager"));
    }

    struct Env {
        present: Vec<BackupTool>,
        answers: std::sync::Mutex<std::collections::HashMap<BackupTool, Vec<Result<String, PlatformError>>>>,
    }

    struct Per(Scripted);

    impl ToolRunner for Per {
        fn run_tool(&self, tool: &crate::runner::Tool, t: Duration) -> Result<crate::runner::ToolOutput, PlatformError> {
            self.0.run_tool(tool, t)
        }
    }

    impl BackupEnv for Env {
        fn available(&self, tool: BackupTool) -> bool {
            self.present.contains(&tool)
        }
        fn runner(&self, tool: BackupTool) -> Box<dyn ToolRunner + '_> {
            let a = self.answers.lock().unwrap().remove(&tool).unwrap_or_default();
            Box::new(Per(Scripted::new(a)))
        }
    }

    fn env(present: &[BackupTool], answers: Vec<(BackupTool, Vec<Result<String, PlatformError>>)>) -> Env {
        Env {
            present: present.to_vec(),
            answers: std::sync::Mutex::new(answers.into_iter().collect()),
        }
    }

    // 2026-10-23T08:00:00Z
    const NOW: Timestamp = Timestamp(1_792_742_400_000);

    #[test]
    fn no_backup_tool_found_is_a_normal_empty_answer() {
        assert!(backup_statuses(&env(&[], vec![]), "macos", NOW, &FixedZone::UTC, 48).is_empty());
        // A tool that exists on another OS is not even considered.
        assert!(backup_statuses(&env(&[BackupTool::TimeMachine], vec![]), "linux", NOW, &FixedZone::UTC, 48).is_empty());
    }

    #[test]
    fn file_history_that_never_ran_is_not_set_up_and_a_real_failure_gives_a_next_step() {
        // Windows has no File History log until the feature has been turned on once: that is "not set up", not a failure.
        let never = env(
            &[BackupTool::FileHistory],
            vec![(
                BackupTool::FileHistory,
                vec![fail("wevtutil failed: The specified channel could not be found.")],
            )],
        );
        let s = backup_statuses(&never, "windows", NOW, &FixedZone::UTC, 48);
        assert_eq!(s[0].state, BackupState::NotConfigured, "{s:#?}");
        assert!(!s[0].summary.contains("wevtutil"), "plain words: {}", s[0].summary);
        assert!(s[0].next_step.as_deref().is_some_and(|n| n.contains("File History")), "{s:#?}");
        // Any other failure stays "could not check", and now says what to do next.
        let broken = env(
            &[BackupTool::FileHistory],
            vec![(BackupTool::FileHistory, vec![fail("wevtutil failed: Access is denied.")])],
        );
        let s = backup_statuses(&broken, "windows", NOW, &FixedZone::UTC, 48);
        assert_eq!(s[0].state, BackupState::CannotCheck);
        assert!(s[0].next_step.as_deref().is_some_and(|n| n.contains("Check now")), "{s:#?}");
    }

    /// One File History event as `wevtutil qe /f:xml` prints it.
    fn fh_event(id: u32, level: u8, at: &str) -> String {
        format!("<Event xmlns='x'><System><EventID Qualifiers='0'>{id}</EventID><Level>{level}</Level><TimeCreated SystemTime='{at}'/></System></Event>")
    }

    fn file_history(events: &[String]) -> BackupStatus {
        let e = env(
            &[BackupTool::FileHistory],
            vec![(BackupTool::FileHistory, vec![ok(&events.concat())])],
        );
        backup_statuses(&e, "windows", NOW, &FixedZone::UTC, 48).remove(0)
    }

    #[test]
    fn file_history_is_healthy_stale_or_failed_in_words_and_only_the_last_two_raise_an_alarm() {
        // A finished backup an hour ago: healthy, no alarm.
        let healthy = file_history(&[fh_event(201, 4, "2026-10-23T07:00:00.000000000Z")]);
        assert_eq!(healthy.state, BackupState::Healthy, "{healthy:#?}");
        assert!(healthy.summary.contains("1 hour ago"), "{}", healthy.summary);
        assert!(!needs_attention(&healthy) && !backup::observation(&healthy).unwrap().active);
        // Set up, but the last finished backup is 5 days old and the limit is 2 days: stale, in words, with an alarm.
        let stale = file_history(&[fh_event(201, 4, "2026-10-18T08:00:00.000000000Z")]);
        assert_eq!(stale.state, BackupState::Stale, "{stale:#?}");
        assert!(
            stale.summary.contains("5 days ago") && stale.summary.contains("2 days you allow"),
            "{}",
            stale.summary
        );
        assert!(needs_attention(&stale));
        assert!(backup::observation(&stale).is_some_and(|o| o.active && o.title.contains("File History")));
        // The log reports an error after the last good backup: failed, whatever the age, and it says so.
        let failed = file_history(&[
            fh_event(203, 2, "2026-10-23T06:00:00.000000000Z"),
            fh_event(201, 4, "2026-10-23T01:00:00.000000000Z"),
        ]);
        assert_eq!(failed.state, BackupState::Failed, "{failed:#?}");
        assert!(
            failed.summary.contains("error") && failed.summary.contains("2 hours ago"),
            "{}",
            failed.summary
        );
        assert!(failed.next_step.as_deref().is_some_and(|n| n.contains("File History")));
        assert!(needs_attention(&failed));
        let alarm = backup::observation(&failed).unwrap();
        assert!(alarm.active && alarm.title == "File History backup failed", "{alarm:?}");
        // An error older than the last good backup is history: healthy again, and the alarm clears.
        let recovered = file_history(&[
            fh_event(201, 4, "2026-10-23T07:00:00.000000000Z"),
            fh_event(203, 2, "2026-10-22T06:00:00.000000000Z"),
        ]);
        assert_eq!(recovered.state, BackupState::Healthy, "{recovered:#?}");
        assert!(!backup::observation(&recovered).unwrap().active);
        // An error and no finished backup at all is also failed; no event at all keeps the old words.
        assert_eq!(
            file_history(&[fh_event(203, 1, "2026-10-23T06:00:00.000000000Z")]).state,
            BackupState::Failed
        );
        assert_eq!(file_history(&[]).state, BackupState::NoBackupYet);
    }

    #[test]
    fn file_history_asks_the_log_for_finished_backups_and_for_errors() {
        struct Spy(std::sync::Mutex<Vec<String>>);
        impl ToolRunner for Spy {
            fn run_tool(&self, t: &crate::runner::Tool, _: Duration) -> Result<crate::runner::ToolOutput, PlatformError> {
                self.0.lock().unwrap().push(t.args.join(" "));
                Ok(crate::runner::ToolOutput {
                    ok: true,
                    stdout: String::new(),
                    error: String::new(),
                })
            }
        }
        let spy = Spy(Default::default());
        check_one(&spy, BackupTool::FileHistory, NOW, &FixedZone::UTC, 48);
        let q = spy.0.lock().unwrap().remove(0);
        assert!(q.contains("EventID=201") && q.contains("Level=1") && q.contains("Level=2"), "{q}");
    }

    #[test]
    fn each_tool_is_judged_on_its_own_and_one_failure_does_not_hide_the_rest() {
        let e = env(
            &[BackupTool::TimeMachine, BackupTool::Restic, BackupTool::Borg, BackupTool::Duplicati],
            vec![
                (
                    BackupTool::TimeMachine,
                    vec![ok("Name : TimeMachine\n"), ok("/Volumes/x/2026-10-23-150000.backup\n")],
                ),
                (BackupTool::Restic, vec![ok(r#"[{"time":"2026-10-01T00:00:00Z"}]"#)]),
                (BackupTool::Borg, vec![fail("Repository /mnt/borg does not exist.")]),
            ],
        );
        let s = backup_statuses(&e, "macos", NOW, &FixedZone(480), 48);
        assert_eq!(s.len(), 4, "{s:#?}");
        assert_eq!(
            s.iter().find(|x| x.tool == BackupTool::Duplicati).unwrap().state,
            BackupState::CannotCheck
        );
        let by = |t| s.iter().find(|x| x.tool == t).unwrap();
        assert_eq!(by(BackupTool::TimeMachine).state, BackupState::Healthy);
        assert_eq!(by(BackupTool::Restic).state, BackupState::Stale);
        assert!(needs_attention(by(BackupTool::Restic)));
        assert_eq!(by(BackupTool::Borg).state, BackupState::NotConfigured);
        assert!(by(BackupTool::Borg).next_step.as_ref().unwrap().contains("BORG_REPO"));
    }
}
