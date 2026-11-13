//! The Windows twin of `build_jobs.rs` (and a golden of the Rust output on every OS): `dev ci build-jobs` as a real process with
//! the exact files and lines the shell script gave for the same inputs (the parity cases in `build_jobs.rs` prove the equality on
//! Unix, where the script runs; this file keeps the answers as literals so the Windows and Linux legs check them too, ER-9 and
//! docs/plans/ci-in-rust.md section 7). The last cases use the real probes of the host: a Windows runner must report its memory
//! (`GlobalMemoryStatusEx`; /proc and sysctl do not exist there).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_testkit::TestCommand;
use std::path::PathBuf;

struct Run {
    code: i32,
    env_file: String,
    stdout: String,
    stderr: String,
}

/// Starts the tool under test by program name with a cleared environment (what the runner gives a step, no more).
#[allow(clippy::disallowed_methods)]
fn build_jobs(vars: &[(&str, &str)], split: Option<&str>, with_env_file: bool) -> Run {
    let dir = tempfile::tempdir().unwrap();
    let env_file: PathBuf = dir.path().join("github-env");
    let mut cmd = TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"));
    cmd.args(["ci", "build-jobs"]).args(split).env_clear();
    // What a program needs to start on every OS; never a token.
    for key in ["PATH", "SystemRoot", "SYSTEMROOT", "TEMP", "TMP", "NUMBER_OF_PROCESSORS", "HOME"] {
        if let Ok(v) = std::env::var(key) {
            cmd.env(key, v);
        }
    }
    if with_env_file {
        cmd.env("GITHUB_ENV", &env_file);
    }
    cmd.envs(vars.iter().copied());
    let out = cmd.output().unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        env_file: std::fs::read_to_string(&env_file).unwrap_or_default(),
        stdout: String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
        stderr: String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n"),
    }
}

fn host<'a>(os: &'a str, cores: &'a str, ram: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![("RUNNER_OS", os), ("PADDOCK_CI_CORES", cores), ("PADDOCK_CI_RAM_MB", ram)]
}

#[test]
fn the_shared_mac_gets_a_third_of_its_cores_and_eight_test_threads() {
    let r = build_jobs(&host("macOS", "14", "65536"), None, true);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.env_file, "CARGO_BUILD_JOBS=4\nNEXTEST_TEST_THREADS=8\n");
    assert_eq!(
        r.stderr,
        "build-jobs: this host: 14 cores, 65536 MB of RAM, runner class macOS\n\
         build-jobs: CARGO_BUILD_JOBS=4 ((14 cores - 2 kept) / 3 jobs per host; the macOS class default of 3 jobs per host / 1 builds at once)\n\
         build-jobs: NEXTEST_TEST_THREADS=8 (the macOS class's measured test threads / 1 test runs at once)\n"
    );
}

#[test]
fn windows_gets_three_jobs_per_host_and_says_it_assumed_the_capacity() {
    let r = build_jobs(&host("Windows", "16", "32768"), None, true);
    assert_eq!(r.env_file, "CARGO_BUILD_JOBS=5\nNEXTEST_TEST_THREADS=8\n");
    assert!(
        r.stdout
            .starts_with("::notice::build-jobs assumes the Windows runner's capacity is 3"),
        "{}",
        r.stdout
    );
}

#[test]
fn memory_is_the_ceiling_when_it_is_smaller_than_the_cores() {
    let mut vars = host("macOS", "14", "4096");
    vars.push(("PADDOCK_CI_RESERVED_RAM_MB", "1024"));
    let r = build_jobs(&vars, None, true);
    assert_eq!(r.env_file, "CARGO_BUILD_JOBS=1\nNEXTEST_TEST_THREADS=8\n");
    assert!(
        r.stderr
            .contains("(4096 MB - 1024 MB kept) / (3 jobs per host x 1536 MB per rustc)"),
        "{}",
        r.stderr
    );
}

#[test]
fn linux_keeps_cargos_default_unless_the_job_splits() {
    let r = build_jobs(&host("Linux", "16", "131072"), None, true);
    assert_eq!(r.env_file, "");
    assert!(
        r.stderr.contains("CARGO_BUILD_JOBS stays unset: cargo uses all 16 cores"),
        "{}",
        r.stderr
    );
    assert_eq!(
        build_jobs(&host("Linux", "16", "131072"), Some("4"), true).env_file,
        "CARGO_BUILD_JOBS=4\n"
    );
}

#[test]
fn without_a_github_env_the_assignments_are_printed() {
    let r = build_jobs(&host("macOS", "14", "65536"), None, false);
    assert!(r.stdout.starts_with("CARGO_BUILD_JOBS=4\nNEXTEST_TEST_THREADS=8\n"), "{}", r.stdout);
}

#[test]
fn a_wrong_value_is_one_error_line_with_the_next_step() {
    let mut vars = host("macOS", "14", "65536");
    vars.push(("PADDOCK_CI_JOBS_PER_HOST", "08"));
    let r = build_jobs(&vars, None, true);
    assert_eq!(r.code, 1);
    assert_eq!(r.stdout.lines().count(), 1, "{}", r.stdout);
    assert!(
        r.stdout
            .starts_with("::error::PADDOCK_CI_JOBS_PER_HOST must be a whole number >= 1, not '08' Next step: "),
        "{}",
        r.stdout
    );
}

/// The host's own probes, on every OS: the cores and the RAM are numbers. On Windows the RAM comes from `GlobalMemoryStatusEx`; a
/// probe of /proc or sysctl would leave it empty and the memory ceiling would be lost (the review of WP-1).
#[test]
fn the_real_probes_find_this_hosts_cores_and_memory() {
    let os = if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    };
    let r = build_jobs(&[("RUNNER_OS", os)], None, true);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let line = r.stderr.lines().next().unwrap();
    let rest = line.strip_prefix("build-jobs: this host: ").unwrap_or_else(|| panic!("{line}"));
    let (cores, rest) = rest.split_once(" cores, ").unwrap_or_else(|| panic!("{line}"));
    let (mb, _) = rest.split_once(" MB of RAM").unwrap_or_else(|| panic!("no RAM found: {line}"));
    assert!(cores.parse::<u32>().is_ok_and(|n| n >= 1), "cores: {line}");
    let mb = mb.parse::<u64>().unwrap_or_else(|_| panic!("RAM: {line}"));
    assert!(mb >= 256, "RAM: {line}");
    // The number is the machine's, not just any number: within 5% of what the OS itself reports (MemTotal, `hw.memsize`, the CIM
    // class `Win32_ComputerSystem`; Windows keeps some physical memory reserved, so it never reads above the total).
    let expected = os_total_memory_mb();
    let low = expected * 95 / 100;
    assert!(
        (low..=expected * 105 / 100).contains(&mb),
        "this host reports {expected} MB; build-jobs read {mb} MB ({line})"
    );
}

/// The host's total physical memory in MB, asked of the OS by another route than the tool's probe.
fn os_total_memory_mb() -> u64 {
    #[allow(clippy::disallowed_methods)]
    let ask = |program: &str, args: &[&str]| -> String {
        let out = std::process::Command::new(program)
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("{program}: {e}"));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    if cfg!(windows) {
        let bytes = ask(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
            ],
        );
        bytes.parse::<u64>().unwrap_or_else(|_| panic!("TotalPhysicalMemory: {bytes:?}")) / (1024 * 1024)
    } else if cfg!(target_os = "macos") {
        let bytes = ask("sysctl", &["-n", "hw.memsize"]);
        bytes.parse::<u64>().unwrap_or_else(|_| panic!("hw.memsize: {bytes:?}")) / (1024 * 1024)
    } else {
        let meminfo = std::fs::read_to_string("/proc/meminfo").expect("/proc/meminfo");
        let kb = meminfo
            .lines()
            .find_map(|l| l.strip_prefix("MemTotal:"))
            .and_then(|r| r.trim().strip_suffix("kB"))
            .and_then(|n| n.trim().parse::<u64>().ok())
            .expect("MemTotal in /proc/meminfo");
        let total = kb / 1024;
        // A container's memory limit (cgroup v2 `memory.max`, a number when one is set) is what build-jobs reads first.
        match std::fs::read_to_string("/sys/fs/cgroup/memory.max")
            .ok()
            .and_then(|m| m.trim().parse::<u128>().ok())
        {
            Some(bytes) => total.min(u64::try_from(bytes / 1_048_576).unwrap_or(u64::MAX)),
            None => total,
        }
    }
}
