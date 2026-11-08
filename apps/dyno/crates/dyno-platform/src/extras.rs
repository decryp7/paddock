//! The OS readers behind the extras (WP-13). Every function runs read-only tools through a
//! [`ToolRunner`] (so tests script the answers) with a timeout, and hands their output to the pure parsers in
//! `dyno_core`. The OS is a parameter (`std::env::consts::OS` values), so every branch is tested on every host.
//! A tool that is missing, slow or refused becomes an "unavailable: reason" in the report, never an error.

use crate::runner::ToolRunner;
use dyno_core::extras::boot::{self, BootStep};
use dyno_core::extras::gpu::{self, GpuReport};
use dyno_core::extras::sleep::{self, SleepReport};
use dyno_core::extras::software::{self, SoftwareInventory};
use dyno_core::extras::usage::{self, EnergySample};
use dyno_core::helper_extras::parse_requests;
use dyno_core::ports::PlatformError;
use dyno_core::time::Timestamp;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issues::testing::Scripted;

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

    // 2026-10-23T08:00:00Z
}
