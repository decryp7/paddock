//! `dev ci build-jobs`: CARGO_BUILD_JOBS (and NEXTEST_TEST_THREADS) for a CI job, the port of `conformance/ci/build-jobs.sh`
//! (docs/plans/ci-in-rust.md, WP-1). The one place CI sets them (ER-22): the `build-jobs` action and setup-rust run it as an
//! early step, and a conformance test fails when a workflow, action or CI script sets CARGO_BUILD_JOBS itself.
//!
//! ```text
//! dev ci build-jobs [<split>]
//! ```
//!
//! `<split>` is how many builds the job runs at once (`cargo mutants --jobs N`): each gets 1/`<split>` of the share. The command
//! writes `CARGO_BUILD_JOBS=<n>` and `NEXTEST_TEST_THREADS=<n>` to `$GITHUB_ENV` (or prints the assignments when `GITHUB_ENV`
//! is unset), or leaves them unset where the runner class keeps cargo's own default, and says why on standard error
//! (`build-jobs: ...`).
//!
//! The behaviour is the shell's, case for case (`tests/it/build_jobs.rs` runs both on the same inputs and compares the exit
//! code, `$GITHUB_ENV`, standard output and standard error). The rules, from the script:
//!
//! 1. `PADDOCK_CI_BUILD_JOBS` (a runner's `runner.envs`), a whole number >= 1: used as it is, divided by the split.
//! 2. Otherwise the smaller of the cores share, `(cores - PADDOCK_CI_RESERVED_CORES) / PADDOCK_CI_JOBS_PER_HOST`, and the memory
//!    share, `(RAM - PADDOCK_CI_RESERVED_RAM_MB) / (PADDOCK_CI_JOBS_PER_HOST x PADDOCK_CI_RUSTC_MB)`, divided by the split, at
//!    least 1. Without the `runner.envs` the runner class's known layout decides ([`class_default`]); Linux keeps cargo's
//!    default until its share is measured (docs/benchmarks.md, Gaps).
//! 3. NEXTEST_TEST_THREADS: the runner's `PADDOCK_CI_TEST_THREADS`, else the class's measured number, else the job's core share.
//!
//! The host's cores come from `nproc`, then `sysctl -n hw.ncpu`, then `NUMBER_OF_PROCESSORS`, then `getconf` (as the script
//! did, so a container's cpuset is honoured the same way); its RAM from the cgroup's `memory.max` when it is a number, else
//! `/proc/meminfo`, else `sysctl -n hw.memsize`. `PADDOCK_CI_CORES` and `PADDOCK_CI_RAM_MB` replace the probes (tests). The probes
//! are programs, so they run through [`ToolCall`] (a bounded run, a cleared environment: no token of the job reaches them).
//!
//! A failure is one `::error::` line with the cause then `Next step:`, exit 1; a warning stays a warning and exit 0.

use crate::ci_isolation::home;
use crate::ci_tool::{ToolCall, error_line};
use clap::Args;
use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;

/// How long a probe (`nproc`, `sysctl`, `getconf`) may run.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// The estimate of one debug rustc's memory when the runner does not say (docs/benchmarks.md, Gaps).
const DEFAULT_RUSTC_MB: i64 = 1536;

/// `dev ci build-jobs`.
#[derive(Args, Debug)]
pub struct BuildJobsArgs {
    /// How many builds this job runs at once; each gets 1/<split> of the share.
    #[arg(default_value = "1", allow_hyphen_values = true)]
    pub split: String,
}

/// The host's numbers, as the probes (or the test variables) give them: text, because the script compared them as text.
pub trait Host {
    /// The cores as text (`nproc`, `sysctl`, `NUMBER_OF_PROCESSORS`, `getconf`), empty when nothing answered.
    fn cores(&self, env: &HashMap<String, String>) -> String;
    /// The RAM in MB as text, empty when nothing answered.
    fn ram_mb(&self, env: &HashMap<String, String>) -> String;
}

/// The machine this process runs on.
pub struct Probes;

/// `$(program args)` with stderr dropped: its trimmed output, `None` when it is missing or fails.
fn probe(env: &HashMap<String, String>, program: &str, args: &[&str]) -> Option<String> {
    let call = ToolCall::new(program).args(args.iter().copied()).timeout(PROBE_TIMEOUT);
    let ran = call.run(env, &mut std::io::sink()).ok()?;
    ran.success.then(|| ran.stdout.trim_end_matches('\n').to_string())
}

impl Host for Probes {
    fn cores(&self, env: &HashMap<String, String>) -> String {
        if let Some(v) = env.get("PADDOCK_CI_CORES").filter(|v| !v.is_empty()) {
            return v.clone();
        }
        let mut n = probe(env, "nproc", &[]).unwrap_or_default();
        if !is_count(&n) {
            n = probe(env, "sysctl", &["-n", "hw.ncpu"]).unwrap_or_default();
        }
        if !is_count(&n) {
            n = env.get("NUMBER_OF_PROCESSORS").cloned().unwrap_or_default();
        }
        if !is_count(&n) {
            n = probe(env, "getconf", &["_NPROCESSORS_ONLN"]).unwrap_or_default();
        }
        n
    }

    fn ram_mb(&self, env: &HashMap<String, String>) -> String {
        if let Some(v) = env.get("PADDOCK_CI_RAM_MB").filter(|v| !v.is_empty()) {
            return v.clone();
        }
        // Native Windows has none of the files below (the script ran under Git bash, which emulates /proc): ask the system.
        if let Some(mb) = windows_ram_mb() {
            return mb.to_string();
        }
        if let Ok(max) = std::fs::read_to_string("/sys/fs/cgroup/memory.max") {
            let max = max.trim_end_matches('\n');
            if is_count(max)
                && let Ok(bytes) = max.parse::<u128>()
            {
                return (bytes / 1_048_576).to_string();
            }
        }
        if let Ok(info) = std::fs::read_to_string("/proc/meminfo") {
            let kb = info
                .lines()
                .find_map(|l| l.strip_prefix("MemTotal:"))
                .and_then(|r| r.split_whitespace().next())
                .and_then(|n| n.parse::<u64>().ok());
            if let Some(kb) = kb {
                let mb = (kb / 1024).to_string();
                if is_count(&mb) {
                    return mb;
                }
            }
        }
        if let Some(bytes) = probe(env, "sysctl", &["-n", "hw.memsize"])
            && is_count(&bytes)
            && let Ok(b) = bytes.parse::<u128>()
        {
            return (b / 1_048_576).to_string();
        }
        String::new()
    }
}

/// The physical memory in MB from `GlobalMemoryStatusEx`; `None` where the call fails (and on every other OS).
#[cfg(windows)]
#[allow(unsafe_code)] // one FFI call with a struct of the documented size
pub fn windows_ram_mb() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: an all-zero MEMORYSTATUSEX is valid; dwLength is set to its size as the API requires.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: `status` is a live, correctly sized MEMORYSTATUSEX.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0 && status.ullTotalPhys > 0).then_some(status.ullTotalPhys / 1_048_576)
}

/// Not Windows: nothing to ask.
#[cfg(not(windows))]
pub fn windows_ram_mb() -> Option<u64> {
    None
}

/// A whole number >= 1, written without leading zeros (`010` and `08` are refused: the shell read them as octal).
pub fn is_count(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && !s.starts_with('0')
}

/// A whole number >= 0 written without leading zeros.
pub fn is_amount(s: &str) -> bool {
    s == "0" || is_count(s)
}

/// What a runner class is assumed to be when its `runner.envs` do not say (verified 2026-10-30 from Gitea's job list, and
/// 2026-11-09 for Windows and the shared Mac; docs/benchmarks.md, "CI share on the shared Mac").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// Jobs the host runs at once (`-`: the class keeps cargo's default, Linux).
    Share,
    /// Cores kept for other work.
    Cores,
    /// RAM in MB kept for other work.
    Ram,
    /// Test threads (`-`: the class has no measured number).
    Threads,
}

/// The class default of a setting as the script's text: a number, or `-` for "not set for this class".
pub fn class_default(os: &str, what: Setting) -> &'static str {
    match (os, what) {
        ("Linux", Setting::Share) => "-",
        ("Windows", Setting::Share) => "3",
        ("macOS", Setting::Threads) | ("Windows", Setting::Threads) => "8",
        ("macOS", Setting::Share) => "3",
        ("macOS", Setting::Cores) => "2",
        ("macOS", Setting::Ram) => "32768",
        (_, Setting::Threads) => "-",
        (_, Setting::Share) => "1",
        (_, Setting::Cores) => "0",
        (_, Setting::Ram) => "2048",
    }
}

/// The variable when it is set and not empty, else the default.
fn setting(env: &HashMap<String, String>, name: &str, default: &str) -> String {
    env.get(name)
        .filter(|v| !v.is_empty())
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

fn var<'a>(env: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    env.get(name).map(String::as_str).filter(|v| !v.is_empty())
}

/// Where the command writes: standard output (values, `::` lines), standard error (`build-jobs: ...`), the job's `$GITHUB_ENV`.
struct Step<'a> {
    env: &'a HashMap<String, String>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
    windows: bool,
}

impl Step<'_> {
    fn say(&mut self, text: &str) {
        let _ = writeln!(self.err, "build-jobs: {text}");
    }

    fn print(&mut self, text: &str) {
        let _ = writeln!(self.out, "{text}");
    }

    /// One `::error::` line and exit 1.
    fn fail(&mut self, cause: &str, next: &str) -> i32 {
        self.print(&error_line(cause, next));
        1
    }

    fn put(&mut self, name: &str, value: &str, why: &str) -> Result<(), i32> {
        self.say(&format!("{name}={value} ({why})"));
        match var(self.env, "GITHUB_ENV") {
            Some(file) => {
                let file = paddock_env::winpath::from_git_bash(file, home::os_of(self.windows));
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&file)
                    .and_then(|mut f| writeln!(f, "{name}={value}"))
                    .map_err(|e| {
                        self.fail(
                            &format!("cannot append to GITHUB_ENV ({file}): {e}"),
                            "check the runner's job folder, then re-run the job.",
                        )
                    })
            }
            None => {
                self.print(&format!("{name}={value}"));
                Ok(())
            }
        }
    }

    /// NEXTEST_TEST_THREADS: the runner's `PADDOCK_CI_TEST_THREADS`, else the class's measured number, else the given share;
    /// divided by the split.
    fn put_threads(&mut self, threads: i64, why: &str, split: i64) -> Result<(), i32> {
        let os = var(self.env, "RUNNER_OS").unwrap_or("").to_string();
        let class = class_default(&os, Setting::Threads);
        let (mut t, mut why) = (threads, why.to_string());
        if let Some(runner) = var(self.env, "PADDOCK_CI_TEST_THREADS") {
            t = number(runner) / split;
            why = format!("the runner's PADDOCK_CI_TEST_THREADS / {split} test runs at once");
        } else if class != "-" {
            t = number(class) / split;
            why = format!("the {os} class's measured test threads / {split} test runs at once");
        }
        if t < 1 {
            t = 1;
        }
        self.put("NEXTEST_TEST_THREADS", &t.to_string(), &why)
    }
}

/// A value already checked by [`is_count`] or [`is_amount`], as a number (too large for 63 bits: the largest).
fn number(s: &str) -> i64 {
    s.parse::<i64>().unwrap_or(i64::MAX)
}

/// The command over this process's environment.
pub fn run(args: &BuildJobsArgs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let env: HashMap<String, String> = std::env::vars().collect();
    let windows = cfg!(windows) || var(&env, "PADDOCK_CI_ASSUME_WINDOWS") == Some("1");
    run_with(&env, &Probes, windows, args, out, err)
}

/// The command over an explicit environment and host (tests).
pub fn run_with(
    env: &HashMap<String, String>,
    host: &dyn Host,
    windows: bool,
    args: &BuildJobsArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let mut step = Step { env, out, err, windows };
    match decide(&mut step, host, &args.split) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

fn decide(step: &mut Step<'_>, host: &dyn Host, split_text: &str) -> Result<(), i32> {
    let env = step.env;
    if !is_count(split_text) {
        return Err(step.fail(
            &format!("the split (builds at once in this job) must be a whole number >= 1, not '{split_text}'"),
            "fix the split input of the build-jobs action.",
        ));
    }
    let split = number(split_text);
    if let Some(t) = var(env, "PADDOCK_CI_TEST_THREADS")
        && !is_count(t)
    {
        return Err(step.fail(
            &format!("PADDOCK_CI_TEST_THREADS must be a whole number >= 1, not '{t}'"),
            "fix the variable in the runner's runner.envs.",
        ));
    }
    let os = var(env, "RUNNER_OS").unwrap_or("").to_string();
    let n = host.cores(env);
    let mem = host.ram_mb(env);
    step.say(&format!(
        "this host: {} cores, {} MB of RAM, runner class {}",
        or_unknown(&n, "unknown"),
        or_unknown(&mem, "unknown"),
        or_unknown(&os, "unknown")
    ));

    if let Some(bj) = var(env, "PADDOCK_CI_BUILD_JOBS") {
        if !is_count(bj) {
            return Err(step.fail(
                &format!("PADDOCK_CI_BUILD_JOBS must be a whole number >= 1, not '{bj}'"),
                "fix the variable in the runner's runner.envs.",
            ));
        }
        let jobs = (number(bj) / split).max(1);
        step.put(
            "CARGO_BUILD_JOBS",
            &jobs.to_string(),
            &format!("the runner's PADDOCK_CI_BUILD_JOBS / {split} builds at once"),
        )?;
        return step.put_threads(
            jobs,
            &format!("the runner's PADDOCK_CI_BUILD_JOBS / {split} test runs at once"),
            split,
        );
    }

    let share = setting(env, "PADDOCK_CI_JOBS_PER_HOST", class_default(&os, Setting::Share));
    let share_src = if var(env, "PADDOCK_CI_JOBS_PER_HOST").is_some() {
        "the runner's PADDOCK_CI_JOBS_PER_HOST".to_string()
    } else {
        let shown = if os.is_empty() { "unknown" } else { &os };
        if os == "Windows" {
            step.print(&format!(
                "::notice::build-jobs assumes the Windows runner's capacity is {share} (the class default); set PADDOCK_CI_JOBS_PER_HOST or fix the class table in ci_build_jobs.rs if it differs"
            ));
        }
        format!("the {shown} class default of {share} jobs per host")
    };
    if share == "-" {
        // Linux keeps cargo's default (every core per job) until its share is measured. A split job (cargo mutants) still
        // divides the cores between its builds.
        if split > 1 && is_count(&n) {
            let jobs = (number(&n) / split).max(1);
            return step.put(
                "CARGO_BUILD_JOBS",
                &jobs.to_string(),
                &format!("{n} cores / {split} builds at once; otherwise the runner class keeps cargo's default"),
            );
        }
        step.say(&format!(
            "CARGO_BUILD_JOBS stays unset: cargo uses all {} cores, as before (this runner class's share is not measured yet)",
            or_unknown(&n, "its")
        ));
        return Ok(());
    }
    if !is_count(&share) {
        return Err(step.fail(
            &format!("PADDOCK_CI_JOBS_PER_HOST must be a whole number >= 1, not '{share}'"),
            "fix the variable in the runner's runner.envs.",
        ));
    }
    let reserved = setting(env, "PADDOCK_CI_RESERVED_CORES", class_default(&os, Setting::Cores));
    if !is_amount(&reserved) {
        return Err(step.fail(
            &format!("PADDOCK_CI_RESERVED_CORES must be a whole number >= 0 without leading zeros, not '{reserved}'"),
            "fix the variable in the runner's runner.envs.",
        ));
    }
    let ram_kept = setting(env, "PADDOCK_CI_RESERVED_RAM_MB", class_default(&os, Setting::Ram));
    if !is_amount(&ram_kept) {
        return Err(step.fail(
            &format!("PADDOCK_CI_RESERVED_RAM_MB must be a whole number >= 0 without leading zeros, not '{ram_kept}'"),
            "fix the variable in the runner's runner.envs.",
        ));
    }
    let per_rustc = setting(env, "PADDOCK_CI_RUSTC_MB", &DEFAULT_RUSTC_MB.to_string());
    if !is_count(&per_rustc) {
        return Err(step.fail(
            &format!("PADDOCK_CI_RUSTC_MB must be a whole number >= 1, not '{per_rustc}'"),
            "fix the variable in the runner's runner.envs.",
        ));
    }

    if !is_count(&n) {
        // Unknown cores: leave cargo's own default and say so; never fail a build over this.
        step.print("::warning::could not count this host's cores; CARGO_BUILD_JOBS stays unset (cargo uses every core it sees)");
        return Ok(());
    }
    let (cores, reserved_n, share_n) = (number(&n), number(&reserved), number(&share));
    if reserved_n >= cores {
        step.print(&format!(
            "::warning::PADDOCK_CI_RESERVED_CORES ({reserved}) keeps all of this host's {n} cores; the job gets 1 build job"
        ));
    }
    let mut jobs = (cores - reserved_n) / share_n;
    let mut why = format!("({n} cores - {reserved} kept) / {share} jobs per host; {share_src}");
    // The tests' share is the cores alone: the memory ceiling below is per rustc, not per test.
    let threads = jobs / split;
    let threads_why = format!("({n} cores - {reserved} kept) / {share} jobs per host / {split} test runs at once");
    if is_count(&mem) {
        let by_ram = (number(&mem) - number(&ram_kept)) / share_n.saturating_mul(number(&per_rustc));
        if by_ram < jobs {
            jobs = by_ram;
            why = format!("({mem} MB - {ram_kept} MB kept) / ({share} jobs per host x {per_rustc} MB per rustc); {share_src}");
        }
    } else {
        step.say("RAM unknown: no memory ceiling");
    }
    jobs = (jobs / split).max(1);
    step.put("CARGO_BUILD_JOBS", &jobs.to_string(), &format!("{why} / {split} builds at once"))?;
    step.put_threads(threads, &threads_why, split)
}

fn or_unknown<'a>(text: &'a str, unknown: &'a str) -> &'a str {
    if text.is_empty() { unknown } else { text }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn windows_reports_its_physical_memory() {
        let mb = windows_ram_mb().expect("GlobalMemoryStatusEx answers");
        assert!(mb >= 512, "{mb} MB");
        assert_eq!(
            Probes.ram_mb(&HashMap::new()),
            mb.to_string(),
            "the probe uses it, not the Unix files"
        );
    }

    struct Fixed(&'static str, &'static str);
    impl Host for Fixed {
        fn cores(&self, _: &HashMap<String, String>) -> String {
            self.0.to_string()
        }
        fn ram_mb(&self, _: &HashMap<String, String>) -> String {
            self.1.to_string()
        }
    }

    fn go(split: &str, host: &Fixed, vars: &[(&str, &str)]) -> (i32, String, String) {
        let env: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let args = BuildJobsArgs { split: split.to_string() };
        let code = run_with(&env, host, false, &args, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn counts_refuse_zero_and_leading_zeros() {
        for good in ["1", "9", "12", "100"] {
            assert!(is_count(good), "{good}");
        }
        for bad in ["", "0", "010", "08", "-1", "1x", " 1", "+1"] {
            assert!(!is_count(bad), "{bad:?}");
        }
        assert!(is_amount("0") && is_amount("5") && !is_amount("00") && !is_amount(""));
    }

    #[test]
    fn the_shared_mac_gets_a_third_of_its_cores_and_the_measured_test_threads() {
        let (code, out, err) = go("1", &Fixed("14", "65536"), &[("RUNNER_OS", "macOS")]);
        assert_eq!(code, 0);
        assert_eq!(out, "CARGO_BUILD_JOBS=4\nNEXTEST_TEST_THREADS=8\n");
        assert!(
            err.contains("build-jobs: this host: 14 cores, 65536 MB of RAM, runner class macOS"),
            "{err}"
        );
    }

    #[test]
    fn linux_keeps_cargos_default_unless_the_job_splits() {
        let (_, out, err) = go("1", &Fixed("16", "131072"), &[("RUNNER_OS", "Linux")]);
        assert_eq!(out, "");
        assert!(err.contains("CARGO_BUILD_JOBS stays unset: cargo uses all 16 cores"), "{err}");
        let (_, out, _) = go("4", &Fixed("16", "131072"), &[("RUNNER_OS", "Linux")]);
        assert_eq!(out, "CARGO_BUILD_JOBS=4\n");
    }

    #[test]
    fn a_failure_is_one_error_line_with_a_next_step() {
        let (code, out, _) = go("0", &Fixed("4", "8192"), &[]);
        assert_eq!(code, 1);
        assert_eq!(out.lines().count(), 1);
        assert!(out.starts_with("::error::the split") && out.contains(" Next step: "), "{out}");
    }

    #[test]
    fn a_negative_share_still_gives_one_job() {
        let (_, out, _) = go("1", &Fixed("2", "8192"), &[("RUNNER_OS", "macOS")]);
        assert!(
            out.contains("CARGO_BUILD_JOBS=1\n") && out.contains("NEXTEST_TEST_THREADS=8"),
            "{out}"
        );
    }
}
