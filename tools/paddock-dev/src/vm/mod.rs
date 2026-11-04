//! `dev vm`: the VM broker (ADR 0054 §1.5). The only thing that starts, reverts or stops a VM for the lifecycle suite, ADR
//! 0053's PM cases and ADR 0040's GUI session; check C97 refuses scripts that call the VM tools themselves.
//!
//! - **One lease per VM** ([`lease`]): `dev vm lease <vm> --for <purpose>` returns a lease held by the calling process
//!   (pid plus start time, rule 53). A second caller is refused with the holder's purpose and age. A VM that runs without a
//!   lease is refused too, naming the process. Every other command takes the lease id.
//! - **Preflight before every start or revert** ([`preflight`]): the `.vmx` parses, its disk exists, no leftover lock folders,
//!   nobody else runs the VM. A failed preflight is exit 4 ("could not run") with the reasons, never a test FAIL.
//! - **Headless only**: `vmrun ... nogui` ([`fusion`]), `tart run --no-graphics` ([`tart`]).
//! - **The `.vmx` is edited here only** ([`vmx`]): parsed edit, temporary file, rename, no backup restored.
//! - **Results carry the lease id**: `dev vm verify <id>` says whether this broker issued it.
//!
//! The hypervisor tools and the clock are traits ([`Runner`], [`Hypervisor`], [`wait::Clock`]), so the logic runs under tests
//! with fakes and never starts a VM.

pub mod fusion;
pub mod lease;
pub mod preflight;
pub mod tart;
pub mod vmx;

use crate::wait::Clock;
use clap::{Args, Subcommand};
use lease::{DEFAULT_PATIENCE_SECS, Lease, LeaseError, LeaseStore};
use paddock_cli::{CliError, Report};
use paddock_proc::{Limits, Outcome, ProcessId, command as proc_command, run_bounded};
use preflight::{FusionFacts, Intent, LockDir, Preflight, TartFacts};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The Windows guest of the lifecycle suite.
pub const WINDOWS_VM: &str = "paddock-win11-test";
const DEFAULT_WINDOWS_VMX: &str = "/Volumes/d7sst7/vms/paddock-win11-test/vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx";
const DEFAULT_FUSION_APP: &str = "/Applications/VMware Fusion.app";
const DEFAULT_STATE: &str = "/tmp/paddock-vm";
const DEFAULT_TART_HOME: &str = "/Volumes/d7sst7/dev-targets/tart-home";
/// Seconds a start waits for the hypervisor to report the guest running.
const START_WAIT_SECS: u64 = 180;
/// Seconds a stop waits for a graceful power-off before it powers off hard.
const DEFAULT_GRACE_SECS: u64 = 120;

/// The result of a host command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Out {
    /// Exit code (124 when it timed out).
    pub code: i32,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// It was ended at its time limit.
    pub timed_out: bool,
}

/// How the broker runs host programs.
pub trait Runner {
    /// Runs `program` with `args` and `env` added, ended after `timeout`.
    fn run(&self, program: &str, args: &[String], env: &[(String, String)], timeout: Duration) -> Result<Out, String>;
    /// Starts `program` so that it outlives this process, its output appended to `log`. Returns its pid.
    fn spawn_detached(&self, program: &str, args: &[String], env: &[(String, String)], log: &Path) -> Result<u32, String>;
}

/// What a hypervisor adapter does to one guest.
pub trait Hypervisor {
    /// Whether the guest runs now.
    fn is_running(&self) -> Result<bool, String>;
    /// Starts it headless; `extra` are adapter-specific arguments. Returns a detail line for the log.
    fn start(&self, extra: &[String]) -> Result<String, String>;
    /// Asks it to shut down (may fail when the guest cannot be asked; the caller then powers off hard).
    fn stop_soft(&self) -> Result<(), String>;
    /// Powers it off.
    fn stop_hard(&self) -> Result<(), String>;
    /// Returns it to `snapshot` (Fusion) or to a fresh clone of the base image `snapshot` (Tart); it is then off.
    fn revert(&self, snapshot: &str) -> Result<(), String>;
    /// Takes a snapshot.
    fn snapshot(&self, name: &str) -> Result<(), String>;
    /// The snapshots it has.
    fn snapshots(&self) -> Result<Vec<String>, String>;
}

/// The real [`Runner`]: bounded children through `paddock-proc`.
pub struct HostRunner;

impl Runner for HostRunner {
    fn run(&self, program: &str, args: &[String], env: &[(String, String)], timeout: Duration) -> Result<Out, String> {
        let mut cmd = proc_command(program);
        cmd.args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = run_bounded(cmd, Limits::new(timeout)).map_err(|e| e.to_string())?;
        let (code, timed_out) = match out.outcome {
            Outcome::Exited(s) => (s.code().unwrap_or(1), false),
            Outcome::TimedOut | Outcome::OutputCap => (124, true),
        };
        Ok(Out {
            code,
            stdout: out.stdout_text(),
            stderr: out.stderr_text(),
            timed_out,
        })
    }

    fn spawn_detached(&self, program: &str, args: &[String], env: &[(String, String)], log: &Path) -> Result<u32, String> {
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map_err(|e| format!("{}: {e}", log.display()))?;
        let err = file.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = proc_command(program);
        cmd.args(args).stdin(std::process::Stdio::null()).stdout(file).stderr(err);
        for (k, v) in env {
            cmd.env(k, v);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let child = cmd.spawn().map_err(|e| format!("could not start {program}: {e}"))?;
        Ok(child.id())
    }
}

/// Polls `f` every `every` until it returns `Ok(true)`, `Err`, or `limit` has passed. `Ok(false)` means it timed out.
pub fn wait_until(
    clock: &mut dyn Clock,
    limit: Duration,
    every: Duration,
    mut f: impl FnMut() -> Result<bool, String>,
) -> Result<bool, String> {
    let begin = clock.elapsed();
    loop {
        if f()? {
            return Ok(true);
        }
        if clock.elapsed().saturating_sub(begin) >= limit {
            return Ok(false);
        }
        clock.sleep(every);
    }
}

/// Starts the guest and waits until the hypervisor reports it running.
pub fn do_start(hv: &dyn Hypervisor, clock: &mut dyn Clock, extra: &[String], limit: Duration) -> Result<String, String> {
    let detail = hv.start(extra)?;
    if wait_until(clock, limit, Duration::from_secs(2), || hv.is_running())? {
        Ok(detail)
    } else {
        Err(format!(
            "started ({detail}) but the hypervisor does not report the guest running after {} s",
            limit.as_secs()
        ))
    }
}

/// How a stop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// It was not running.
    AlreadyOff,
    /// It shut down within the grace period.
    Graceful,
    /// It had to be powered off.
    Hard,
}

/// Stops the guest: asks it to shut down, waits `grace`, then powers it off.
pub fn do_stop(hv: &dyn Hypervisor, clock: &mut dyn Clock, grace: Duration, log: &mut dyn FnMut(&str)) -> Result<Stopped, String> {
    if !hv.is_running()? {
        return Ok(Stopped::AlreadyOff);
    }
    if let Err(e) = hv.stop_soft() {
        log(&format!("soft stop failed ({e}); waiting for the guest, then powering off"));
    }
    if wait_until(clock, grace, Duration::from_secs(3), || hv.is_running().map(|r| !r))? {
        return Ok(Stopped::Graceful);
    }
    log(&format!("still running after {} s; powering off hard", grace.as_secs()));
    hv.stop_hard()?;
    if wait_until(clock, Duration::from_secs(60), Duration::from_secs(2), || {
        hv.is_running().map(|r| !r)
    })? {
        Ok(Stopped::Hard)
    } else {
        Err("the guest is still running 60 s after a hard power-off".into())
    }
}

/// Which hypervisor a VM name belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A VMware Fusion guest at this `.vmx`.
    Fusion(PathBuf),
    /// A Tart guest.
    Tart,
}

/// The kind of `name`: the Windows test VM is Fusion, everything else Tart.
pub fn kind_of(name: &str, windows_vmx: &Path) -> Kind {
    if name == WINDOWS_VM {
        Kind::Fusion(windows_vmx.to_path_buf())
    } else {
        Kind::Tart
    }
}

/// Base images are never started, stopped or deleted by the broker: a guest is a clone of one.
pub fn is_base_image(name: &str) -> bool {
    name.ends_with("-base") || name.starts_with("ghcr.io/")
}

/// The broker's `vm` command.
#[derive(Args)]
pub struct VmArgs {
    #[command(subcommand)]
    command: VmCommand,
}

/// The broker's commands.
#[derive(Subcommand)]
pub enum VmCommand {
    /// The known leases and whether each guest runs.
    List,
    /// Take the lease on a guest. Exit 4 if it is leased, or runs without a lease (the holder or process is named).
    Lease {
        /// The guest: `paddock-win11-test` (Fusion), or a Tart guest name.
        vm: String,
        /// What the lease is for (shown to anyone refused).
        #[arg(long = "for")]
        purpose: String,
        /// Seconds before the lease lapses even if its holder lives.
        #[arg(long, default_value_t = DEFAULT_PATIENCE_SECS)]
        patience: u64,
        /// Seconds to wait for a held guest instead of refusing at once.
        #[arg(long, default_value_t = 0)]
        wait: u64,
        /// The process that holds the lease (default: the caller's parent). It is released when that process ends.
        #[arg(long)]
        holder_pid: Option<u32>,
        /// Take a guest that runs without a lease, on the caller's word that it owns it (for stopping a guest a person started).
        /// A runner never passes this; the processes found are written to the journal.
        #[arg(long)]
        adopt: bool,
    },
    /// Whether a guest runs and who leases it. Needs no lease; changes nothing.
    Status {
        /// The guest.
        vm: String,
    },
    /// The snapshots of the leased guest.
    Snapshots {
        /// The lease id.
        lease: String,
    },
    /// Set a device-presence key in the leased Fusion guest's `.vmx` (`sata0:0.present`, ...): the one parsed edit that setup needs.
    Set {
        /// The lease id.
        lease: String,
        /// The key, such as `sata0:0.present`.
        #[arg(long)]
        key: String,
        /// `TRUE` or `FALSE`.
        #[arg(long)]
        value: String,
    },
    /// Give a lease back.
    Release {
        /// The lease id.
        lease: String,
    },
    /// Say whether this broker issued a lease id (live or released): what a release gate checks on a result file.
    Verify {
        /// The lease id.
        lease: String,
    },
    /// What a start would check, without starting.
    Preflight {
        /// The lease id.
        lease: String,
    },
    /// Preflight, then start headless and wait until it runs.
    Start {
        /// The lease id.
        lease: String,
        /// An extra argument for `tart run` (`--vnc-experimental`, `--rosetta=<tag>`, `--dir=<spec>`).
        #[arg(long = "arg", allow_hyphen_values = true)]
        args: Vec<String>,
        /// Largest 1-minute load average at which a start is allowed.
        #[arg(long, default_value_t = 25.0)]
        max_load: f64,
    },
    /// Shut the guest down, then power it off if it is still up after the grace period.
    Stop {
        /// The lease id.
        lease: String,
        /// Seconds to wait for a graceful shutdown.
        #[arg(long, default_value_t = DEFAULT_GRACE_SECS)]
        grace: u64,
    },
    /// Return a stopped guest to a snapshot (Fusion), or to a fresh clone of a base image (Tart).
    Revert {
        /// The lease id.
        lease: String,
        /// The snapshot name, or for Tart the base image to clone.
        #[arg(long)]
        to: String,
    },
    /// Take a snapshot (Fusion).
    Snapshot {
        /// The lease id.
        lease: String,
        /// The snapshot name.
        #[arg(long)]
        name: String,
    },
    /// Switch the guest's built-in VNC server on for the next start, or off (the one `.vmx` edit besides the disk-name repair).
    Vnc {
        /// The lease id.
        lease: String,
        /// The VNC port.
        #[arg(long, conflicts_with = "off")]
        port: Option<u16>,
        /// The environment variable that holds the VNC password (never given on the command line).
        #[arg(long, requires = "port")]
        password_env: Option<String>,
        /// Remove the VNC lines.
        #[arg(long)]
        off: bool,
    },
    /// Clone a Tart base image as the leased guest.
    Clone {
        /// The lease id.
        lease: String,
        /// The base image.
        #[arg(long)]
        base: String,
    },
    /// Delete the leased Tart guest (never a base image).
    Delete {
        /// The lease id.
        lease: String,
    },
}

/// The settings the broker reads from the environment.
#[derive(Debug, Clone)]
pub struct Env {
    /// Where leases and the journal live (`PADDOCK_VM_STATE`).
    pub state: PathBuf,
    /// The Fusion app folder (`FUSION_APP`).
    pub fusion_app: PathBuf,
    /// The Windows guest's `.vmx` (`PADDOCK_WIN_VMX`).
    pub windows_vmx: PathBuf,
    /// `TART_HOME`.
    pub tart_home: PathBuf,
    /// The `tart` binary (`TART`).
    pub tart: PathBuf,
}

impl Env {
    /// From the process environment.
    pub fn from_process() -> Env {
        let var = |k: &str, d: &str| PathBuf::from(std::env::var_os(k).unwrap_or_else(|| d.into()));
        Env {
            state: var("PADDOCK_VM_STATE", DEFAULT_STATE),
            fusion_app: var("FUSION_APP", DEFAULT_FUSION_APP),
            windows_vmx: var("PADDOCK_WIN_VMX", DEFAULT_WINDOWS_VMX),
            tart_home: var("TART_HOME", DEFAULT_TART_HOME),
            tart: var("TART", "tart"),
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

/// The real sleeper for waits.
struct RealClock(crate::wait::SystemClock);
impl Clock for RealClock {
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
    fn sleep(&mut self, d: Duration) {
        self.0.sleep(d);
    }
}

fn parent_pid() -> Option<u32> {
    #[cfg(unix)]
    {
        Some(std::os::unix::process::parent_id())
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn alive(p: ProcessId) -> bool {
    p.is_alive()
}

fn lease_err(e: LeaseError) -> CliError {
    match e {
        LeaseError::Held { .. } => CliError::refused(e.to_string())
            .with_hint("Wait for the holder to finish, or pass --wait <seconds>. A lease is freed when its holder process ends."),
        LeaseError::Unknown(_) | LeaseError::Stale(..) => {
            CliError::refused(e.to_string()).with_hint("Take a new lease: ./dev vm lease <vm> --for <purpose>.")
        }
        LeaseError::Io(_) => CliError::failure(e.to_string()),
    }
}

/// The journal: one line per phase in `<state>/log/<vm>.log` and on standard error (ER-27).
struct Journal<'a> {
    file: PathBuf,
    err: &'a mut dyn Write,
}

impl Journal<'_> {
    fn line(&mut self, lease: &str, text: &str) {
        let line = format!("{} {} {}", now_ms(), lease, text);
        let _ = writeln!(self.err, "vm: {text}");
        if let Some(dir) = self.file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.file) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Runs a `vm` command. `err` receives the phase lines.
pub fn command(args: &VmArgs, err: &mut dyn Write) -> Result<Report, CliError> {
    run_with(
        &args.command,
        &Env::from_process(),
        &HostRunner,
        &mut RealClock(crate::wait::SystemClock::new()),
        err,
    )
}

fn read_files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

fn lock_dirs(dir: &Path, now: SystemTime) -> Vec<LockDir> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<LockDir> = rd
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".lck") && e.path().is_dir())
        .map(|e| LockDir {
            name: e.file_name().to_string_lossy().into_owned(),
            age_secs: e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .map_or(0, |d| d.as_secs()),
            owner: read_files(&e.path()),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Everything a command needs about the guest it works on.
struct Guest<'a> {
    name: String,
    kind: Kind,
    env: &'a Env,
    runner: &'a dyn Runner,
}

impl Guest<'_> {
    fn base(&self) -> String {
        match &self.kind {
            Kind::Fusion(vmx) => vmx.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            Kind::Tart => self.name.clone(),
        }
    }

    fn password(&self) -> Result<Option<String>, String> {
        let Kind::Fusion(vmx) = &self.kind else { return Ok(None) };
        let Ok(text) = std::fs::read_to_string(vmx) else { return Ok(None) };
        if !vmx::Vmx::parse(&text).has_prefix("encryption.") {
            return Ok(None);
        }
        let out = self.runner.run(
            "security",
            &[
                "find-generic-password".into(),
                "-s".into(),
                "paddock-win11-test-vm".into(),
                "-a".into(),
                "vmrun".into(),
                "-w".into(),
            ],
            &[],
            Duration::from_secs(20),
        )?;
        if out.code == 0 {
            Ok(Some(out.stdout.trim().to_string()))
        } else {
            Err("the guest is encrypted and the login keychain has no item paddock-win11-test-vm/vmrun".into())
        }
    }

    fn with_hv<T>(&self, f: impl FnOnce(&dyn Hypervisor) -> T) -> Result<T, String> {
        match &self.kind {
            Kind::Fusion(vmx) => {
                let hv = fusion::Fusion {
                    runner: self.runner,
                    vmrun: fusion::vmrun_path(&self.env.fusion_app),
                    vmx: vmx.clone(),
                    password: self.password()?,
                };
                Ok(f(&hv))
            }
            Kind::Tart => {
                let hv = self.tart();
                Ok(f(&hv))
            }
        }
    }

    fn tart(&self) -> tart::Tart<'_> {
        tart::Tart {
            runner: self.runner,
            tart: self.env.tart.clone(),
            name: self.name.clone(),
            home: self.env.tart_home.clone(),
            log: self.env.state.join("logs").join(format!("{}.log", self.name)),
        }
    }

    fn ps(&self) -> Vec<preflight::ProcLine> {
        self.runner
            .run("ps", &["-axo".into(), "pid=,command=".into()], &[], Duration::from_secs(20))
            .map(|o| preflight::parse_ps(&o.stdout))
            .unwrap_or_default()
    }

    /// The processes that run the guest, for a refusal.
    fn processes(&self) -> Vec<preflight::ProcLine> {
        let ps = self.ps();
        match &self.kind {
            Kind::Fusion(vmx) => preflight::fusion_processes(&vmx.display().to_string(), &ps),
            Kind::Tart => preflight::tart_processes(&self.name, &ps),
        }
    }

    fn preflight(&self, intent: Intent, must_exist: bool) -> Result<Preflight, String> {
        match &self.kind {
            Kind::Fusion(vmx) => {
                let dir = vmx.parent().unwrap_or(Path::new("."));
                let password = self.password();
                let running = self
                    .runner
                    .run(
                        &fusion::vmrun_path(&self.env.fusion_app).display().to_string(),
                        &["-T".into(), "fusion".into(), "list".into()],
                        &[],
                        Duration::from_secs(60),
                    )
                    .map(|o| preflight::parse_vmrun_list(&o.stdout))?;
                let facts = FusionFacts {
                    vmx_text: std::fs::read_to_string(vmx).ok(),
                    dir_files: read_files(dir),
                    locks: lock_dirs(dir, SystemTime::now()),
                    running,
                    vmx_path: vmx.display().to_string(),
                    base: self.base(),
                    vmrun_found: fusion::vmrun_path(&self.env.fusion_app).is_file(),
                    missing_password: password.is_err(),
                };
                Ok(preflight::fusion(&facts, intent))
            }
            Kind::Tart => {
                let t = self.tart();
                let listed = if self.env.tart_home.is_dir() {
                    t.list_json().and_then(|j| preflight::parse_tart_list(&j))?
                } else {
                    Vec::new()
                };
                let facts = TartFacts {
                    tart_found: self
                        .runner
                        .run(
                            &self.env.tart.display().to_string(),
                            &["--version".into()],
                            &[],
                            Duration::from_secs(20),
                        )
                        .is_ok_and(|o| o.code == 0),
                    home_exists: self.env.tart_home.is_dir(),
                    listed: listed.into_iter().find(|v| v.name == self.name),
                };
                Ok(preflight::tart(&facts, intent, must_exist))
            }
        }
    }

    /// Points a stale redo-log name in the `.vmx` at the newest existing one (a revert renumbers them).
    fn repair_disk_name(&self, journal: &mut Journal<'_>, lease: &str) -> Result<(), String> {
        let Kind::Fusion(path) = &self.kind else { return Ok(()) };
        let Ok(text) = std::fs::read_to_string(path) else { return Ok(()) };
        let files = read_files(path.parent().unwrap_or(Path::new(".")));
        let fixed = vmx::Vmx::parse(&text).fix_disk_name(&self.base(), &files);
        if fixed.render() != vmx::Vmx::parse(&text).render() {
            vmx::write_atomic(path, &fixed.render()).map_err(|e| format!("{}: {e}", path.display()))?;
            journal.line(lease, "vmx named a missing redo log: pointed at the newest existing one");
        }
        Ok(())
    }
}

fn guest<'a>(name: &str, env: &'a Env, runner: &'a dyn Runner) -> Guest<'a> {
    Guest {
        name: name.to_string(),
        kind: kind_of(name, &env.windows_vmx),
        env,
        runner,
    }
}

fn could_not_run(p: &Preflight) -> CliError {
    CliError::refused(format!("could not run: {}", p.problems.join("; ")))
        .with_hint("This is a preflight result, not a test failure. Fix the first reason, then run again.")
}

/// The 1-minute load average, from `sysctl -n vm.loadavg` (`{ 1.2 3.4 5.6 }`) or `/proc/loadavg`; `None` when neither answers.
fn host_load(runner: &dyn Runner) -> Option<f64> {
    if let Ok(t) = std::fs::read_to_string("/proc/loadavg") {
        return t.split_whitespace().next().and_then(|v| v.parse().ok());
    }
    let out = runner
        .run("sysctl", &["-n".into(), "vm.loadavg".into()], &[], Duration::from_secs(10))
        .ok()?;
    out.stdout
        .split_whitespace()
        .find_map(|w| w.trim_matches(|c| c == '{' || c == '}').parse::<f64>().ok())
}

/// The command with its dependencies passed in.
pub fn run_with(cmd: &VmCommand, env: &Env, runner: &dyn Runner, clock: &mut dyn Clock, err: &mut dyn Write) -> Result<Report, CliError> {
    let store = LeaseStore::new(&env.state);
    let live = |id: &str| store.require(id, now_ms(), &alive).map_err(lease_err);
    match cmd {
        VmCommand::List => {
            let rows: Vec<Value> = store
                .all()
                .iter()
                .map(|l| json!({"lease": l.to_json(), "live": l.state(now_ms(), alive(l.holder)).is_live()}))
                .collect();
            Ok(Report::new(json!({"leases": rows}), format!("{} lease(s)", rows.len())))
        }
        VmCommand::Verify { lease } => {
            if store.issued(lease) {
                Ok(Report::new(
                    json!({"lease": lease, "issued": true}),
                    format!("lease {lease} was issued by this broker"),
                ))
            } else {
                Err(CliError::refused(format!("lease {lease} was not issued by this broker"))
                    .with_hint("Results must come from a run under ./dev vm lease."))
            }
        }
        VmCommand::Lease {
            vm,
            purpose,
            patience,
            wait,
            holder_pid,
            adopt,
        } => {
            let pid = holder_pid
                .or_else(parent_pid)
                .ok_or_else(|| CliError::usage("--holder-pid is required on this platform"))?;
            let holder = ProcessId::of(pid).ok_or_else(|| CliError::usage(format!("holder pid {pid} is not running")))?;
            let g = guest(vm, env, runner);
            let mut journal = Journal {
                file: env.state.join("log").join(format!("{vm}.log")),
                err,
            };
            let mut last = None;
            let got = wait_until(clock, Duration::from_secs(*wait), Duration::from_secs(2), || {
                match store.acquire(vm, purpose, holder, *patience, now_ms(), &alive) {
                    Ok(l) => {
                        last = Some(Ok(l));
                        Ok(true)
                    }
                    Err(e @ LeaseError::Held { .. }) => {
                        last = Some(Err(e));
                        Ok(false)
                    }
                    Err(e) => Err(e.to_string()),
                }
            })
            .map_err(CliError::failure)?;
            let lease = match (got, last) {
                (true, Some(Ok(l))) => l,
                (_, Some(Err(e))) => return Err(lease_err(e)),
                _ => return Err(CliError::failure("no lease was issued")),
            };
            if matches!(g.kind, Kind::Tart) && is_base_image(vm) {
                let _ = store.release(&lease.id);
                return Err(CliError::refused(format!("{vm} is a base image; lease a clone of it"))
                    .with_hint("Clone it under a new name with ./dev vm clone."));
            }
            // A guest that runs without a live lease is somebody's run outside the broker: refuse and name the process.
            let running = g.with_hv(|hv| hv.is_running()).and_then(|r| r);
            match running {
                Ok(true) if *adopt => {
                    let who: Vec<String> = g.processes().iter().map(|p| format!("pid {}", p.pid)).collect();
                    journal.line(
                        &lease.id,
                        &format!(
                            "adopted a running guest without a lease ({}); the caller says it owns it",
                            who.join(", ")
                        ),
                    );
                }
                Ok(true) => {
                    let procs = g.processes();
                    let _ = store.release(&lease.id);
                    let who: Vec<String> = procs.iter().map(|p| format!("pid {}", p.pid)).collect();
                    journal.line(&lease.id, &format!("refused: {vm} runs without a lease ({})", who.join(", ")));
                    return Err(CliError::refused(format!(
                        "{vm} is running without a lease ({}); a run outside the broker owns it",
                        if who.is_empty() {
                            "process not found".into()
                        } else {
                            who.join(", ")
                        }
                    ))
                    .with_hint("Wait for that run to end, then lease again. The broker never stops a guest it does not hold."));
                }
                Ok(false) => {}
                Err(e) => {
                    let _ = store.release(&lease.id);
                    return Err(CliError::failure(format!("could not tell whether {vm} runs: {e}")));
                }
            }
            journal.line(&lease.id, &format!("lease issued for \"{purpose}\" to {holder}"));
            Ok(Report::new(
                json!({"lease": lease.id, "vm": vm, "purpose": purpose, "holder": {"pid": holder.pid(), "startMicros": holder.started().as_micros()}, "patienceSecs": patience}),
                lease.id.clone(),
            ))
        }
        VmCommand::Status { vm } => {
            let g = guest(vm, env, runner);
            let running = g.with_hv(|hv| hv.is_running()).and_then(|r| r).map_err(CliError::failure)?;
            let leased = store
                .current(vm)
                .map_err(lease_err)?
                .filter(|l| l.state(now_ms(), alive(l.holder)).is_live());
            let who: Vec<u32> = if running {
                g.processes().iter().map(|p| p.pid).collect()
            } else {
                Vec::new()
            };
            Ok(Report::new(
                json!({"vm": vm, "running": running, "lease": leased.as_ref().map(|l| l.to_json()), "processes": who}),
                if running { "running" } else { "stopped" },
            ))
        }
        VmCommand::Snapshots { lease } => {
            let l = live(lease)?;
            let list = guest(&l.vm, env, runner)
                .with_hv(|hv| hv.snapshots())
                .and_then(|r| r)
                .map_err(CliError::failure)?;
            Ok(Report::new(json!({"lease": lease, "snapshots": list}), list.join("\n")))
        }
        VmCommand::Set { lease, key, value } => {
            let l = live(lease)?;
            let g = guest(&l.vm, env, runner);
            let Kind::Fusion(path) = &g.kind else {
                return Err(CliError::usage("set applies to the Fusion guest only"));
            };
            if !vmx::is_presence_key(key) || !matches!(value.as_str(), "TRUE" | "FALSE") {
                return Err(CliError::usage(format!(
                    "only <bus>N:N.present keys with TRUE or FALSE can be set, not {key} = {value}"
                )));
            }
            if g.with_hv(|hv| hv.is_running()).and_then(|r| r).map_err(CliError::failure)? {
                return Err(CliError::refused("the guest is running; a device changes only while it is off"));
            }
            let text = std::fs::read_to_string(path).map_err(|e| CliError::failure(format!("{}: {e}", path.display())))?;
            let mut edited = vmx::Vmx::parse(&text);
            edited.set(key, value);
            vmx::write_atomic(path, &edited.render()).map_err(|e| CliError::failure(format!("{}: {e}", path.display())))?;
            Journal {
                file: env.state.join("log").join(format!("{}.log", l.vm)),
                err,
            }
            .line(lease, &format!("vmx: {key} = {value}"));
            Ok(Report::new(
                json!({"lease": lease, "key": key, "value": value}),
                format!("{key} = {value}"),
            ))
        }
        VmCommand::Release { lease } => {
            let released = store.release(lease).map_err(lease_err)?;
            let vm = lease.rsplitn(3, '.').nth(2).unwrap_or("");
            Journal {
                file: env.state.join("log").join(format!("{vm}.log")),
                err,
            }
            .line(
                lease,
                if released {
                    "lease released"
                } else {
                    "release: no such lease (already gone)"
                },
            );
            Ok(Report::new(
                json!({"lease": lease, "released": released}),
                if released { "released" } else { "no such lease" },
            ))
        }
        VmCommand::Preflight { lease } => {
            let l = live(lease)?;
            let g = guest(&l.vm, env, runner);
            let p = g.preflight(Intent::Change, true).map_err(CliError::failure)?;
            if p.ok() {
                Ok(Report::new(
                    json!({"lease": lease, "vm": l.vm, "notes": p.notes}),
                    "preflight passed",
                ))
            } else {
                Err(could_not_run(&p))
            }
        }
        VmCommand::Start { lease, args, max_load } => {
            let l = live(lease)?;
            let g = guest(&l.vm, env, runner);
            let mut journal = Journal {
                file: env.state.join("log").join(format!("{}.log", l.vm)),
                err,
            };
            if is_base_image(&l.vm) {
                return Err(CliError::refused(format!("{} is a base image and is never started", l.vm)));
            }
            g.repair_disk_name(&mut journal, lease).map_err(CliError::failure)?;
            let p = g.preflight(Intent::Change, true).map_err(CliError::failure)?;
            journal.line(
                lease,
                &format!("preflight: {}", if p.ok() { "passed".to_string() } else { p.problems.join("; ") }),
            );
            for n in &p.notes {
                journal.line(lease, &format!("preflight note: {n}"));
            }
            if !p.ok() {
                return Err(could_not_run(&p));
            }
            if let Some(load) = host_load(runner).filter(|load| load >= max_load) {
                return Err(
                    CliError::refused(format!("could not run: the 1-minute load {load:.1} is not under {max_load}"))
                        .with_hint("Try again later; the machine is shared."),
                );
            }
            let begin = std::time::Instant::now();
            let detail = g
                .with_hv(|hv| do_start(hv, clock, args, Duration::from_secs(START_WAIT_SECS)))
                .and_then(|r| r)
                .map_err(|e| {
                    journal.line(lease, &format!("start failed: {e}"));
                    CliError::failure(format!("start failed: {e}"))
                        .with_hint("See the journal under PADDOCK_VM_STATE/log and the hypervisor's own log.")
                })?;
            journal.line(lease, &format!("started headless in {} s: {detail}", begin.elapsed().as_secs()));
            Ok(Report::new(
                json!({"lease": lease, "vm": l.vm, "running": true, "detail": detail, "seconds": begin.elapsed().as_secs()}),
                "running",
            ))
        }
        VmCommand::Stop { lease, grace } => {
            let l = live(lease)?;
            let g = guest(&l.vm, env, runner);
            let mut journal = Journal {
                file: env.state.join("log").join(format!("{}.log", l.vm)),
                err,
            };
            let begin = std::time::Instant::now();
            let mut note = |m: &str| journal.line(lease, m);
            let how = g
                .with_hv(|hv| do_stop(hv, clock, Duration::from_secs(*grace), &mut note))
                .and_then(|r| r)
                .map_err(CliError::failure)?;
            journal.line(lease, &format!("stop: {how:?} after {} s", begin.elapsed().as_secs()));
            Ok(Report::new(
                json!({"lease": lease, "vm": l.vm, "stopped": format!("{how:?}")}),
                format!("{how:?}"),
            ))
        }
        VmCommand::Revert { lease, to } => {
            let l = live(lease)?;
            let g = guest(&l.vm, env, runner);
            let mut journal = Journal {
                file: env.state.join("log").join(format!("{}.log", l.vm)),
                err,
            };
            if is_base_image(&l.vm) {
                return Err(CliError::refused(format!(
                    "{} is a base image and is never reverted or replaced",
                    l.vm
                )));
            }
            let p = g
                .preflight(Intent::Change, !matches!(g.kind, Kind::Tart))
                .map_err(CliError::failure)?;
            // A Tart revert replaces the clone, so it may run (the adapter stops it); a Fusion revert needs a guest that is off.
            let blocking: Vec<&String> = p
                .problems
                .iter()
                .filter(|m| !(matches!(g.kind, Kind::Tart) && (m.contains("already exists") || m.contains("already running"))))
                .collect();
            if !blocking.is_empty() {
                journal.line(lease, &format!("revert preflight: {}", p.problems.join("; ")));
                return Err(could_not_run(&Preflight {
                    problems: blocking.into_iter().cloned().collect(),
                    notes: vec![],
                }));
            }
            let begin = std::time::Instant::now();
            g.with_hv(|hv| hv.revert(to)).and_then(|r| r).map_err(|e| {
                journal.line(lease, &format!("revert to {to} failed: {e}"));
                CliError::failure(format!("revert to {to} failed: {e}"))
            })?;
            g.repair_disk_name(&mut journal, lease).map_err(CliError::failure)?;
            journal.line(lease, &format!("reverted to {to} in {} s", begin.elapsed().as_secs()));
            Ok(Report::new(
                json!({"lease": lease, "vm": l.vm, "snapshot": to, "seconds": begin.elapsed().as_secs()}),
                format!("reverted to {to}"),
            ))
        }
        VmCommand::Snapshot { lease, name } => {
            let l = live(lease)?;
            let g = guest(&l.vm, env, runner);
            g.with_hv(|hv| hv.snapshot(name)).and_then(|r| r).map_err(CliError::failure)?;
            Journal {
                file: env.state.join("log").join(format!("{}.log", l.vm)),
                err,
            }
            .line(lease, &format!("snapshot {name} taken"));
            Ok(Report::new(
                json!({"lease": lease, "snapshot": name}),
                format!("snapshot {name} taken"),
            ))
        }
        VmCommand::Vnc {
            lease,
            port,
            password_env,
            off,
        } => {
            let l = live(lease)?;
            let g = guest(&l.vm, env, runner);
            let Kind::Fusion(path) = &g.kind else {
                return Err(CliError::usage("vnc applies to the Fusion guest only"));
            };
            if g.with_hv(|hv| hv.is_running()).and_then(|r| r).map_err(CliError::failure)? {
                return Err(CliError::refused(
                    "the guest is running; the VNC lines change only between a revert and a start",
                ));
            }
            let text = std::fs::read_to_string(path).map_err(|e| CliError::failure(format!("{}: {e}", path.display())))?;
            let edited = if *off {
                vmx::Vmx::parse(&text).without_vnc()
            } else {
                let (Some(port), Some(var)) = (port, password_env) else {
                    return Err(CliError::usage("--port and --password-env are needed to switch VNC on"));
                };
                let pw = std::env::var(var).map_err(|_| CliError::usage(format!("{var} is not set")))?;
                vmx::Vmx::parse(&text).with_vnc(*port, &pw)
            };
            let fixed = edited.fix_disk_name(&g.base(), &read_files(path.parent().unwrap_or(Path::new("."))));
            vmx::write_atomic(path, &fixed.render()).map_err(|e| CliError::failure(format!("{}: {e}", path.display())))?;
            Journal {
                file: env.state.join("log").join(format!("{}.log", l.vm)),
                err,
            }
            .line(lease, if *off { "vmx: VNC lines removed" } else { "vmx: VNC lines added" });
            Ok(Report::new(
                json!({"lease": lease, "vnc": !*off}),
                if *off { "VNC off" } else { "VNC on" },
            ))
        }
        VmCommand::Clone { lease, base } => {
            let l = live(lease)?;
            let g = guest(&l.vm, env, runner);
            if !matches!(g.kind, Kind::Tart) || is_base_image(&l.vm) {
                return Err(CliError::usage("clone applies to a Tart guest that is not a base image"));
            }
            let mut journal = Journal {
                file: env.state.join("log").join(format!("{}.log", l.vm)),
                err,
            };
            let p = g.preflight(Intent::Change, false).map_err(CliError::failure)?;
            if !p.ok() {
                return Err(could_not_run(&p));
            }
            let begin = std::time::Instant::now();
            g.tart().clone_from(base).map_err(|e| {
                journal.line(lease, &format!("clone of {base} failed: {e}"));
                CliError::failure(format!("clone of {base} failed: {e}"))
            })?;
            journal.line(lease, &format!("cloned {base} in {} s", begin.elapsed().as_secs()));
            Ok(Report::new(
                json!({"lease": lease, "vm": l.vm, "base": base}),
                format!("cloned {base}"),
            ))
        }
        VmCommand::Delete { lease } => {
            let l = live(lease)?;
            let g = guest(&l.vm, env, runner);
            if !matches!(g.kind, Kind::Tart) || is_base_image(&l.vm) {
                return Err(CliError::refused(
                    "only a leased Tart clone that is not a base image can be deleted",
                ));
            }
            g.tart().delete().map_err(CliError::failure)?;
            Journal {
                file: env.state.join("log").join(format!("{}.log", l.vm)),
                err,
            }
            .line(lease, "deleted");
            Ok(Report::new(json!({"lease": lease, "vm": l.vm, "deleted": true}), "deleted"))
        }
    }
}

/// The lease a gate reads from a result: unused outside tests, kept for the C97 fixture docs.
#[allow(dead_code)]
fn _lease_type_check(_: &Lease) {}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::cell::RefCell;

    /// A runner that answers from a script and records the command lines it was given.
    pub struct FakeRunner {
        answers: RefCell<Vec<(i32, String)>>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeRunner {
        pub fn new(answers: Vec<(i32, &str)>) -> FakeRunner {
            FakeRunner {
                answers: RefCell::new(answers.into_iter().rev().map(|(c, s)| (c, s.to_string())).collect()),
                calls: RefCell::new(vec![]),
            }
        }
        pub fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl Runner for FakeRunner {
        fn run(&self, program: &str, args: &[String], env: &[(String, String)], _t: Duration) -> Result<Out, String> {
            let env: Vec<String> = env.iter().map(|(k, v)| format!(" {k}={v}")).collect();
            self.calls
                .borrow_mut()
                .push(format!("{program} {}{}", args.join(" "), env.concat()));
            let (code, stdout) = self.answers.borrow_mut().pop().unwrap_or((0, String::new()));
            Ok(Out {
                code,
                stdout,
                stderr: String::new(),
                timed_out: false,
            })
        }
        fn spawn_detached(&self, program: &str, args: &[String], env: &[(String, String)], _log: &Path) -> Result<u32, String> {
            let env: Vec<String> = env.iter().map(|(k, v)| format!(" {k}={v}")).collect();
            self.calls
                .borrow_mut()
                .push(format!("detached {program} {}{}", args.join(" "), env.concat()));
            Ok(4242)
        }
    }

    /// A clock whose sleep advances time.
    pub struct FakeClock(pub RefCell<Duration>);
    impl FakeClock {
        pub fn new() -> FakeClock {
            FakeClock(RefCell::new(Duration::ZERO))
        }
    }
    impl Clock for FakeClock {
        fn elapsed(&self) -> Duration {
            *self.0.borrow()
        }
        fn sleep(&mut self, d: Duration) {
            *self.0.borrow_mut() += d;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::FakeClock;
    use super::*;
    use std::cell::Cell;

    /// A guest that turns off after `off_after` polls once asked to stop softly.
    struct Fake {
        running: Cell<bool>,
        soft_ok: bool,
        stops_on_soft: bool,
        calls: std::cell::RefCell<Vec<&'static str>>,
    }
    impl Fake {
        fn new(running: bool, soft_ok: bool, stops_on_soft: bool) -> Fake {
            Fake {
                running: Cell::new(running),
                soft_ok,
                stops_on_soft,
                calls: Default::default(),
            }
        }
    }
    impl Hypervisor for Fake {
        fn is_running(&self) -> Result<bool, String> {
            Ok(self.running.get())
        }
        fn start(&self, _: &[String]) -> Result<String, String> {
            self.calls.borrow_mut().push("start");
            self.running.set(true);
            Ok("fake".into())
        }
        fn stop_soft(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("soft");
            if self.stops_on_soft {
                self.running.set(false);
            }
            if self.soft_ok { Ok(()) } else { Err("no tools".into()) }
        }
        fn stop_hard(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("hard");
            self.running.set(false);
            Ok(())
        }
        fn revert(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn snapshot(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn snapshots(&self) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
    }

    #[test]
    fn stop_of_a_stopped_guest_does_nothing() {
        let f = Fake::new(false, true, true);
        assert_eq!(
            do_stop(&f, &mut FakeClock::new(), Duration::from_secs(10), &mut |_| {}).unwrap(),
            Stopped::AlreadyOff
        );
        assert!(f.calls.borrow().is_empty());
    }

    #[test]
    fn stop_is_graceful_when_the_guest_goes_off_in_time() {
        let f = Fake::new(true, true, true);
        assert_eq!(
            do_stop(&f, &mut FakeClock::new(), Duration::from_secs(10), &mut |_| {}).unwrap(),
            Stopped::Graceful
        );
        assert_eq!(*f.calls.borrow(), vec!["soft"]);
    }

    #[test]
    fn stop_powers_off_hard_after_the_grace_period_and_logs_a_failed_soft_stop() {
        let f = Fake::new(true, false, false);
        let mut clock = FakeClock::new();
        let mut lines = vec![];
        assert_eq!(
            do_stop(&f, &mut clock, Duration::from_secs(30), &mut |m| lines.push(m.to_string())).unwrap(),
            Stopped::Hard
        );
        assert_eq!(*f.calls.borrow(), vec!["soft", "hard"]);
        assert!(
            lines[0].contains("soft stop failed (no tools)") && lines[1].contains("powering off hard"),
            "{lines:?}"
        );
        assert!(clock.elapsed() >= Duration::from_secs(30));
    }

    #[test]
    fn start_waits_until_the_hypervisor_reports_running() {
        let f = Fake::new(false, true, true);
        assert_eq!(do_start(&f, &mut FakeClock::new(), &[], Duration::from_secs(10)).unwrap(), "fake");
    }

    #[test]
    fn wait_until_gives_up_at_the_limit_without_sleeping_for_real() {
        let mut c = FakeClock::new();
        assert!(!wait_until(&mut c, Duration::from_secs(9), Duration::from_secs(2), || Ok(false)).unwrap());
        assert!(c.elapsed() >= Duration::from_secs(9) && c.elapsed() < Duration::from_secs(12));
        assert_eq!(
            wait_until(&mut c, Duration::from_secs(9), Duration::from_secs(2), || Err("x".into())),
            Err("x".into())
        );
    }

    #[test]
    fn names_choose_the_hypervisor_and_bases_are_protected() {
        let vmx = Path::new("/v/w.vmx");
        assert_eq!(kind_of(WINDOWS_VM, vmx), Kind::Fusion(vmx.to_path_buf()));
        assert_eq!(kind_of("dyno-vm-1", vmx), Kind::Tart);
        assert!(is_base_image("linux-gnome-base") && is_base_image("ghcr.io/cirruslabs/macos") && !is_base_image("dyno-vm-1"));
    }

    fn env(dir: &Path) -> Env {
        Env {
            state: dir.join("state"),
            fusion_app: dir.join("Fusion.app"),
            windows_vmx: dir.join("w/w.vmx"),
            tart_home: dir.join("tart"),
            tart: "/t/tart".into(),
        }
    }

    fn run(cmd: &VmCommand, env: &Env, runner: &dyn Runner) -> Result<Report, CliError> {
        run_with(cmd, env, runner, &mut FakeClock::new(), &mut Vec::new())
    }

    fn lease_cmd(vm: &str, purpose: &str) -> VmCommand {
        VmCommand::Lease {
            vm: vm.into(),
            purpose: purpose.into(),
            patience: 3600,
            wait: 0,
            holder_pid: Some(std::process::id()),
            adopt: false,
        }
    }

    #[test]
    fn a_lease_then_a_second_lease_is_refused_with_the_holders_purpose() {
        let dir = tempfile::tempdir().unwrap();
        let e = env(dir.path());
        std::fs::create_dir_all(&e.tart_home).unwrap();
        // is_running asks `tart list` each time.
        let r = test_support::FakeRunner::new(vec![(0, "[]"), (0, "[]")]);
        let first = run(&lease_cmd("dyno-vm-1", "first run"), &e, &r).unwrap();
        assert!(first.data["lease"].as_str().unwrap().starts_with("dyno-vm-1."));
        let err = run(&lease_cmd("dyno-vm-1", "second run"), &e, &r).unwrap_err();
        assert_eq!(err.exit, paddock_cli::Exit::Refused);
        assert!(err.message.contains("first run"), "{}", err.message);
        // Releasing frees it again.
        run(
            &VmCommand::Release {
                lease: first.data["lease"].as_str().unwrap().into(),
            },
            &e,
            &r,
        )
        .unwrap();
        let r2 = test_support::FakeRunner::new(vec![(0, "[]")]);
        assert!(run(&lease_cmd("dyno-vm-1", "third"), &e, &r2).is_ok());
    }

    #[test]
    fn a_guest_running_without_a_lease_is_refused_naming_the_process_and_the_lease_is_not_kept() {
        let dir = tempfile::tempdir().unwrap();
        let e = env(dir.path());
        std::fs::create_dir_all(&e.tart_home).unwrap();
        let r = test_support::FakeRunner::new(vec![
            (0, r#"[{"Name":"dyno-vm-1","Running":true}]"#),
            (0, "  77 /opt/homebrew/bin/tart run --no-graphics dyno-vm-1\n"),
        ]);
        let err = run(&lease_cmd("dyno-vm-1", "me"), &e, &r).unwrap_err();
        assert_eq!(err.exit, paddock_cli::Exit::Refused);
        assert!(
            err.message.contains("running without a lease") && err.message.contains("pid 77"),
            "{}",
            err.message
        );
        assert!(LeaseStore::new(&e.state).all().is_empty());
    }

    #[test]
    fn a_base_image_cannot_be_leased() {
        let dir = tempfile::tempdir().unwrap();
        let e = env(dir.path());
        let r = test_support::FakeRunner::new(vec![]);
        let err = run(&lease_cmd("linux-gnome-base", "x"), &e, &r).unwrap_err();
        assert!(err.message.contains("base image"));
        assert!(LeaseStore::new(&e.state).all().is_empty());
    }

    #[test]
    fn commands_need_a_live_lease() {
        let dir = tempfile::tempdir().unwrap();
        let e = env(dir.path());
        let r = test_support::FakeRunner::new(vec![]);
        for cmd in [
            VmCommand::Start {
                lease: "x.1.2".into(),
                args: vec![],
                max_load: 100.0,
            },
            VmCommand::Stop {
                lease: "x.1.2".into(),
                grace: 1,
            },
            VmCommand::Revert {
                lease: "x.1.2".into(),
                to: "s".into(),
            },
        ] {
            let err = run(&cmd, &e, &r).unwrap_err();
            assert_eq!(err.exit, paddock_cli::Exit::Refused);
        }
        assert!(r.calls().is_empty(), "no hypervisor call without a lease");
    }

    #[test]
    fn a_failed_preflight_is_could_not_run_and_nothing_is_started() {
        let dir = tempfile::tempdir().unwrap();
        let e = env(dir.path());
        std::fs::create_dir_all(&e.tart_home).unwrap();
        let r = test_support::FakeRunner::new(vec![(0, "[]"), (0, "[]"), (0, "tart 2.0")]);
        let l = run(&lease_cmd("dyno-vm-1", "me"), &e, &r).unwrap();
        let err = run(
            &VmCommand::Start {
                lease: l.data["lease"].as_str().unwrap().into(),
                args: vec![],
                max_load: 1e9,
            },
            &e,
            &r,
        )
        .unwrap_err();
        assert_eq!(err.exit, paddock_cli::Exit::Refused);
        assert!(
            err.message.starts_with("could not run:") && err.message.contains("does not show the guest"),
            "{}",
            err.message
        );
        assert!(!r.calls().iter().any(|c| c.starts_with("detached")));
    }

    #[test]
    fn verify_knows_released_leases_and_not_invented_ones() {
        let dir = tempfile::tempdir().unwrap();
        let e = env(dir.path());
        std::fs::create_dir_all(&e.tart_home).unwrap();
        let r = test_support::FakeRunner::new(vec![(0, "[]")]);
        let l = run(&lease_cmd("dyno-vm-1", "me"), &e, &r).unwrap();
        let id: String = l.data["lease"].as_str().unwrap().into();
        run(&VmCommand::Release { lease: id.clone() }, &e, &r).unwrap();
        assert!(run(&VmCommand::Verify { lease: id }, &e, &r).is_ok());
        assert!(
            run(
                &VmCommand::Verify {
                    lease: "dyno-vm-1.1.2".into()
                },
                &e,
                &r
            )
            .is_err()
        );
    }
}
