//! Readers for the hardware's own reports (WP-23, WP-24): the OS's CPU speed limit, SSD wear and battery capacity, each
//! behind a small port from `dyno_core::ports`. Observe-only: they read, never change a setting. External tools run
//! through [`crate::process`] with timeouts; anything missing or refused is an unavailable reading that says why and
//! what would help. The `Fake*` readers are for tests only (feature `test-support`, ADR 0033).
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use crate::parse::hardware::{parse_smartctl_ssd, smartctl_refusal_kind, Refusal};
use crate::parse::smart::{scan_devices, ScanDevice};
use crate::smartctl_search::{self, SearchEnv, SystemEnv};
use dyno_core::hardware::{BatteryCapacity, DiskWear, OsThrottle};
use dyno_core::helper::HelperHost;
use dyno_core::metrics::Reading;
use dyno_core::ports::{BatteryCapacitySource, DiskWearSource, ThrottleSignals};
use std::path::{Path, PathBuf};
use std::time::Duration;

const TOOL_TIMEOUT: Duration = Duration::from_secs(10);
use dyno_core::hardware::NO_BATTERY;

/// The reader of the OS's CPU speed limit for this OS.
pub fn throttle_signals() -> Box<dyn ThrottleSignals> {
    #[cfg(target_os = "macos")]
    {
        Box::new(PmsetTherm)
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(LinuxThrottle::new(PathBuf::from("/sys")))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Box::new(NoThrottleSignals(
            "Windows does not report why the CPU slows down; Dyno infers it from the clock, the heat and the power mode.",
        ))
    }
}

/// The reader of SSD wear for this OS.
pub fn disk_wear() -> Box<dyn DiskWearSource> {
    Box::new(SmartctlWear::new(system_smartctl()).with_search(Box::new(SystemEnv)))
}

/// The reader of the battery's capacity for this OS.
pub fn battery_capacity() -> Box<dyn BatteryCapacitySource> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacBattery)
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(SysfsBattery::new(PathBuf::from("/sys/class/power_supply")))
    }
    #[cfg(windows)]
    {
        Box::new(WindowsBattery)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        Box::new(crate::unsupported::NotReadHere::new(
            "Battery capacity",
            "Battery capacity is not read on this OS yet.",
        ))
    }
}

/// The SSD wear reader that tries Dyno's own copy of smartctl first (`<tools>/smartmontools-*/smartctl`, newest name first), then
/// the usual places, then any smartmontools the person installed themselves (Program Files, the registry, the PATH).
pub fn disk_wear_with_own_tools(tools: &Path) -> Box<dyn DiskWearSource> {
    Box::new(
        SmartctlWear::new(system_smartctl())
            .with_own_tools(tools.to_path_buf())
            .with_search(Box::new(SystemEnv)),
    )
}

/// The smartctl this computer would use for a local reading, if any (Dyno's own copy first, then the places searched).
pub fn locate_smartctl(own_tools: &Path) -> Option<PathBuf> {
    SmartctlWear::new(system_smartctl())
        .with_own_tools(own_tools.to_path_buf())
        .with_search(Box::new(SystemEnv))
        .locate()
}

/// The version `program --version` prints, when it runs.
pub fn smartctl_version(program: &Path) -> Option<String> {
    let mut cmd = crate::process::command(program);
    cmd.arg("--version");
    crate::process::output_text(cmd, TOOL_TIMEOUT)
        .ok()
        .and_then(|t| crate::parse::hardware::parse_smartctl_version(&t))
}

/// Dyno's own copies of smartctl (`<tools>/smartmontools-*/smartctl`), newest name first.
fn own_copies(own_tools: Option<&Path>) -> Vec<PathBuf> {
    let mut own: Vec<PathBuf> = own_tools
        .and_then(|t| std::fs::read_dir(t).ok())
        .map(|d| {
            d.flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("smartmontools-"))
                .map(|e| e.path().join(if cfg!(windows) { "smartctl.exe" } else { "smartctl" }))
                .collect()
        })
        .unwrap_or_default();
    own.sort();
    own.reverse();
    own
}

/// The fixed places on Unix systems; the Windows places come from the environment ([`crate::smartctl_search`]).
fn system_smartctl() -> Vec<PathBuf> {
    [
        "/opt/homebrew/sbin/smartctl",
        "/opt/homebrew/bin/smartctl",
        "/usr/local/sbin/smartctl",
        "/usr/local/bin/smartctl",
        "/usr/sbin/smartctl",
        "/usr/bin/smartctl",
    ]
    .iter()
    .map(PathBuf::from)
    .collect()
}

/// The system this build runs on, as the words of a message need it.
pub fn this_host() -> HelperHost {
    if cfg!(windows) {
        HelperHost::Windows
    } else if cfg!(target_os = "macos") {
        HelperHost::Macos
    } else {
        HelperHost::Linux
    }
}

/// What to say when smartmontools is not found, with the next step for this OS.
pub fn not_installed_text(host: HelperHost) -> &'static str {
    match host {
        HelperHost::Windows => "Dyno reads SSD wear through smartmontools, which it did not find. Install it in Settings > Disk health tool, or install smartmontools yourself (Dyno looks in Program Files and on the PATH).",
        HelperHost::Macos => "Dyno reads SSD wear through smartmontools, which it did not find. Install it in Settings > Disk health tool, or with Homebrew (brew install smartmontools).",
        HelperHost::Linux => "Dyno reads SSD wear through smartmontools, which it did not find. Install it with your package manager (for example: sudo apt install smartmontools).",
    }
}

/// What to say when the OS withholds a drive from this account, naming the drives and the next step for this OS.
pub fn needs_admin_text(host: HelperHost, drives: &[String]) -> String {
    let which = if drives.is_empty() {
        String::new()
    } else {
        format!(" ({})", drives.join(", "))
    };
    match host {
        HelperHost::Windows | HelperHost::Linux => {
            format!("Reading these drives{which} needs Administrator readings (Settings): the helper reads them with the rights they need.")
        }
        HelperHost::Macos => format!("Reading these drives{which} needs administrator rights, which Dyno does not ask for on macOS."),
    }
}

// ------------------------------------------------------------------------------------------------ CPU speed limit

/// macOS: `pmset -g therm` (read-only; it only prints).
#[cfg(target_os = "macos")]
pub struct PmsetTherm;

#[cfg(target_os = "macos")]
impl ThrottleSignals for PmsetTherm {
    fn read(&mut self) -> Reading<OsThrottle> {
        let mut cmd = crate::process::command("/usr/bin/pmset");
        cmd.args(["-g", "therm"]);
        match crate::process::output_text(cmd, TOOL_TIMEOUT) {
            Ok(text) => match crate::parse::hardware::parse_pmset_therm(&text) {
                Some(t) => Reading::Available(t),
                None => Reading::unavailable("macOS did not say whether it limits the CPU's speed."),
            },
            Err(e) => Reading::unavailable(e.to_string()),
        }
    }
}

/// Linux: the kernel's thermal-throttle counters and the cpufreq limits under `<root>` (normally `/sys`). A counter that
/// rose since the previous read means the CPU throttled for heat in between.
pub struct LinuxThrottle {
    root: PathBuf,
    last_count: Option<u64>,
}

impl LinuxThrottle {
    /// A reader over a sysfs root.
    pub fn new(root: PathBuf) -> LinuxThrottle {
        LinuxThrottle { root, last_count: None }
    }
}

fn read_u64(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn cpu_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(root.join("devices/system/cpu")) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.strip_prefix("cpu")
                    .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
            })
        })
        .collect();
    v.sort();
    v.truncate(1024);
    v
}

impl ThrottleSignals for LinuxThrottle {
    fn read(&mut self) -> Reading<OsThrottle> {
        let cpus = cpu_dirs(&self.root);
        let counters: Vec<u64> = cpus
            .iter()
            .flat_map(|c| ["core_throttle_count", "package_throttle_count"].map(|f| c.join("thermal_throttle").join(f)))
            .filter_map(|p| read_u64(&p))
            .collect();
        let limits: Vec<u8> = cpus
            .iter()
            .filter_map(|c| {
                let max = read_u64(&c.join("cpufreq/cpuinfo_max_freq"))?;
                let cap = read_u64(&c.join("cpufreq/scaling_max_freq"))?;
                (max > 0).then(|| (cap.min(max) * 100 / max) as u8)
            })
            .collect();
        if counters.is_empty() && limits.is_empty() {
            return Reading::unavailable(
                "This kernel exposes no CPU throttling counters or frequency limits (common in virtual machines).",
            );
        }
        let total: u64 = counters.iter().sum();
        let thermal = if counters.is_empty() {
            None
        } else {
            let rose = self.last_count.map(|prev| total > prev);
            self.last_count = Some(total);
            rose
        };
        let speed = limits.iter().copied().min();
        Reading::Available(OsThrottle {
            speed_limit_percent: speed,
            thermal,
            power: speed.map(|s| s < 95),
        })
    }
}

/// A system that does not say.
pub struct NoThrottleSignals(pub &'static str);

impl ThrottleSignals for NoThrottleSignals {
    fn read(&mut self) -> Reading<OsThrottle> {
        Reading::unavailable(self.0)
    }
}

// ---------------------------------------------------------------------------------------------------- SSD wear

/// SSD wear through smartmontools (`smartctl -a -j`, read-only). Without it, or without the rights, it says what would help.
pub struct SmartctlWear {
    candidates: Vec<PathBuf>,
    /// Dyno's own tools folder, looked at on every read so a copy installed later is found without a restart.
    own_tools: Option<PathBuf>,
    /// Where else to look (Program Files, the registry, the PATH), asked on every read.
    search: Option<Box<dyn SearchEnv + Send>>,
    host: HelperHost,
}

/// One finished `smartctl` run.
struct Run {
    stdout: String,
    code: Option<i32>,
    ms: u64,
}

/// At most this many drives are read per cycle.
const MAX_DRIVES: usize = 16;

impl SmartctlWear {
    /// A reader that looks for `smartctl` at these paths.
    #[allow(
        clippy::disallowed_methods,
        reason = "a platform adapter for the build OS: the readers here run on this machine; a composition for another host calls with_host"
    )]
    pub fn new(candidates: Vec<PathBuf>) -> SmartctlWear {
        SmartctlWear {
            candidates,
            own_tools: None,
            search: None,
            host: this_host(),
        }
    }

    /// Also looks for Dyno's own copies under this folder, before the other places, on every read.
    pub fn with_own_tools(mut self, tools: PathBuf) -> SmartctlWear {
        self.own_tools = Some(tools);
        self
    }

    /// Also looks where a person's own install may be (on every read, after the fixed places).
    pub fn with_search(mut self, env: Box<dyn SearchEnv + Send>) -> SmartctlWear {
        self.search = Some(env);
        self
    }

    /// Words its messages for this system (tests).
    pub fn with_host(mut self, host: HelperHost) -> SmartctlWear {
        self.host = host;
        self
    }

    /// The `smartctl` this reader would use now.
    pub fn locate(&self) -> Option<PathBuf> {
        let own = own_copies(self.own_tools.as_deref());
        own.iter().chain(self.candidates.iter()).find(|p| p.is_file()).cloned().or_else(|| {
            let env = self.search.as_deref()?;
            smartctl_search::first_existing(env, self.host == HelperHost::Windows, &|p| p.is_file())
        })
    }

    fn run(&self, program: &Path, args: &[&str]) -> Result<Run, String> {
        let mut cmd = crate::process::command(program);
        cmd.args(args);
        let started = std::time::Instant::now();
        // smartctl's exit status is a bit mask of findings, not a failure flag: judge the JSON.
        let out = crate::process::run_with_timeout(cmd, TOOL_TIMEOUT).map_err(|e| e.to_string())?;
        Ok(Run {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            code: out.status.code(),
            ms: started.elapsed().as_millis() as u64,
        })
    }
}

/// Why one drive could not be opened, in plain words for this system.
fn open_failed_reason(host: HelperHost, name: &str) -> String {
    if host == HelperHost::Macos && name.to_ascii_lowercase().contains("usb") {
        "macOS cannot read health from USB drives.".into()
    } else {
        "smartctl could not open one of the drives.".into()
    }
}

/// A device name for the log: the first characters (an Apple silicon name is a path of several hundred).
fn short(name: &str) -> String {
    let head: String = name.chars().take(48).collect();
    if head.len() < name.len() {
        format!("{head}... ({} characters)", name.len())
    } else {
        head
    }
}

/// The arguments for one drive of the scan: `-a -j [-d <type>] <name>`.
fn drive_args(d: &ScanDevice) -> Vec<&str> {
    let mut args = vec!["-a", "-j"];
    if let Some(kind) = d.kind.as_deref() {
        args.extend(["-d", kind]);
    }
    args.push(&d.name);
    args
}

impl DiskWearSource for SmartctlWear {
    fn read(&mut self) -> Reading<Vec<DiskWear>> {
        let Some(program) = self.locate() else {
            log::info!(target: "dyno::hardware", "SSD wear: smartctl not found (own copies, fixed places, Program Files, registry, PATH)");
            return Reading::unavailable(not_installed_text(self.host));
        };
        log::info!(target: "dyno::hardware", "SSD wear: using {}", program.display());
        let scan = match self.run(&program, &["--scan", "-j"]) {
            Ok(r) => {
                log::info!(target: "dyno::hardware", "SSD wear: scan exit {:?} in {} ms", r.code, r.ms);
                r.stdout
            }
            Err(e) => {
                log::warn!(target: "dyno::hardware", "SSD wear: the scan did not run: {e}");
                return Reading::unavailable(format!("smartctl could not list the drives: {e}"));
            }
        };
        let (devices, rejected) = scan_devices(&scan, MAX_DRIVES);
        log::info!(target: "dyno::hardware", "SSD wear: the scan listed {} drive(s), {rejected} left out as unusable names", devices.len());
        if devices.is_empty() {
            return Reading::unavailable(if rejected > 0 {
                "smartctl listed drives, but none with a name Dyno can pass to it safely."
            } else {
                "smartctl found no drives it can read."
            });
        }
        let (mut found, mut denied, mut other) = (Vec::new(), Vec::new(), None::<String>);
        // Per-drive reasons of drives that cannot be read for a reason that is no failure of the whole reading (USB on macOS).
        let mut unreadable: Vec<String> = Vec::new();
        for d in &devices {
            let run = match self.run(&program, &drive_args(d)) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!(target: "dyno::hardware", "SSD wear: {} skipped, smartctl did not run: {e}", short(&d.name));
                    other = other.or(Some(e));
                    continue;
                }
            };
            match parse_smartctl_ssd(&run.stdout) {
                Some(w) => {
                    // One SSD can be listed several times (Apple silicon: a namespace each): it counts once.
                    let again = found.iter().any(|f: &DiskWear| f.id == w.id);
                    log::info!(target: "dyno::hardware", "SSD wear: {} read (exit {:?}, {} ms){}", short(&d.name), run.code, run.ms, if again { ", the same SSD as an earlier entry" } else { "" });
                    if !again {
                        found.push(w);
                    }
                }
                None => {
                    let why = match smartctl_refusal_kind(&run.stdout) {
                        Some(Refusal::Access) => {
                            denied.push(short(&d.name));
                            "the OS withheld it from this account (needs Administrator readings)"
                        }
                        Some(Refusal::Unsupported) => "it does not expose SMART data to apps",
                        Some(Refusal::OpenFailed) => {
                            let reason = open_failed_reason(self.host, &d.name);
                            if !unreadable.contains(&reason) {
                                unreadable.push(reason);
                            }
                            "smartctl could not open this drive (not a failure of the other drives)"
                        }
                        None => "it reports no SSD wear counters (a hard disk, or no counters)",
                    };
                    log::info!(target: "dyno::hardware", "SSD wear: {} skipped (exit {:?}, {} ms): {why}", short(&d.name), run.code, run.ms);
                }
            }
        }
        if !denied.is_empty() {
            // Drives the OS withheld are named, whether or not others were read, so a missing drive is explained.
            log::info!(target: "dyno::hardware", "SSD wear: {} drive(s) need Administrator readings: {}", denied.len(), denied.join(", "));
        }
        if found.is_empty() {
            Reading::unavailable(if !denied.is_empty() {
                needs_admin_text(self.host, &denied)
            } else if let Some(e) = other {
                format!("smartctl could not read the drives: {e}")
            } else if !unreadable.is_empty() {
                unreadable.join(" ")
            } else {
                "No SSD reports wear counters to apps.".into()
            })
        } else {
            Reading::Available(found)
        }
    }
}

// ---------------------------------------------------------------------------------------------------- battery

/// macOS: `ioreg -rn AppleSmartBattery` (read-only).
#[cfg(target_os = "macos")]
pub struct MacBattery;

#[cfg(target_os = "macos")]
impl BatteryCapacitySource for MacBattery {
    fn read(&mut self) -> Reading<BatteryCapacity> {
        let mut cmd = crate::process::command("/usr/sbin/ioreg");
        cmd.args(["-rn", "AppleSmartBattery"]);
        match crate::process::output_text(cmd, TOOL_TIMEOUT) {
            Ok(text) => {
                crate::parse::hardware::parse_mac_battery(&text).map_or_else(|| Reading::unavailable(NO_BATTERY), Reading::Available)
            }
            Err(e) => Reading::unavailable(e.to_string()),
        }
    }
}

/// Linux (and anything with `/sys/class/power_supply`): the first battery's energy or charge figures.
pub struct SysfsBattery {
    dir: PathBuf,
}

impl SysfsBattery {
    /// A reader over a `power_supply` directory.
    pub fn new(dir: PathBuf) -> SysfsBattery {
        SysfsBattery { dir }
    }
}

impl BatteryCapacitySource for SysfsBattery {
    fn read(&mut self) -> Reading<BatteryCapacity> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return Reading::unavailable(NO_BATTERY);
        };
        let mut dirs: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
        dirs.sort();
        for d in dirs {
            let is_battery = std::fs::read_to_string(d.join("type")).is_ok_and(|t| t.trim().eq_ignore_ascii_case("battery"));
            if !is_battery {
                continue;
            }
            let pair = |full: &str, design: &str| Some((read_u64(&d.join(full))?, read_u64(&d.join(design))?));
            let Some((full, design)) = pair("energy_full", "energy_full_design").or_else(|| pair("charge_full", "charge_full_design"))
            else {
                continue;
            };
            let (Ok(full), Ok(design)) = (u32::try_from(full), u32::try_from(design)) else {
                continue;
            };
            if design == 0 {
                continue;
            }
            let id = std::fs::read_to_string(d.join("serial_number"))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| d.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "battery".into());
            return Reading::Available(BatteryCapacity {
                id,
                design,
                full,
                cycles: read_u64(&d.join("cycle_count"))
                    .filter(|c| *c > 0)
                    .and_then(|c| u32::try_from(c).ok()),
                rated_cycles: None,
            });
        }
        Reading::unavailable(NO_BATTERY)
    }
}

/// The most of a battery report that is read (a real one is a few tens of kilobytes).
#[cfg(any(windows, test))]
const MAX_REPORT_BYTES: u64 = 4 * 1024 * 1024;

/// A name nobody can guess ahead of time: the standard library's per-process random hash keys over the pid and the clock.
#[cfg(any(windows, test))]
fn unguessable_suffix() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u32(std::process::id());
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    format!("{:016x}", h.finish())
}

/// Makes a folder under `temp` that did not exist before (an unguessable name) through the shared private-folder type (ADR 0092,
/// rule 55: this file's own Win32 copy of it is gone): owner-only from creation (on Windows a protected descriptor that grants only
/// the owner), made relative to a held parent, and refused when `temp` or the folder is one another account can change or replace.
/// The temp folder itself may be shared (`C:\Windows\Temp` when Dyno runs elevated), so no fixed name is ever used there; a name
/// that already exists is checked like any existing folder and refused unless it is ours and closed to others.
#[cfg(any(windows, test))]
fn fresh_private_folder(temp: &Path) -> std::io::Result<PathBuf> {
    let dir = temp.join(format!("Dyno-{}", unguessable_suffix()));
    match paddock_env::private::PrivateDir::create_all(&dir) {
        Ok(held) => Ok(held.path().to_path_buf()),
        Err(e) => {
            log::warn!(Why = e.reason(); "The battery report folder is not safe; no report is read");
            Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, e.to_string()))
        }
    }
}

/// Reads the report through one open handle: what the handle names must be a regular file (the open follows a link, so the target
/// is what is checked), at most [`MAX_REPORT_BYTES`].
#[cfg(any(windows, test))]
fn read_report(path: &Path) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_REPORT_BYTES {
        return None;
    }
    let mut text = String::new();
    file.take(MAX_REPORT_BYTES).read_to_string(&mut text).ok()?;
    Some(text)
}

/// Runs `produce` with the path of a report file inside a fresh private folder under `temp`, reads the file back, and removes the
/// folder.
#[cfg(any(windows, test))]
fn with_report<T>(temp: &Path, produce: impl FnOnce(&Path) -> T) -> std::io::Result<(T, Option<String>)> {
    let dir = fresh_private_folder(temp)?;
    let file = dir.join("battery-report.xml");
    let made = produce(&file);
    let text = read_report(&file);
    paddock_proc::cleanup::remove_dir_all(&dir);
    Ok((made, text))
}

/// Windows: `powercfg /batteryreport /xml` into a fresh folder under the temp folder that only this run made (never a fixed name in
/// a folder other users can write to), read back through one handle.
#[cfg(windows)]
pub struct WindowsBattery;

#[cfg(windows)]
impl BatteryCapacitySource for WindowsBattery {
    #[expect(
        clippy::disallowed_methods,
        reason = "the report goes into a fresh private folder that `with_report` makes exclusively under the temp folder, never a fixed name"
    )]
    fn read(&mut self) -> Reading<BatteryCapacity> {
        let windir = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let powercfg = windir.join("System32").join("powercfg.exe");
        let run = |file: &Path| {
            let mut cmd = crate::process::command(&powercfg);
            cmd.arg("/batteryreport").arg("/xml").arg("/output").arg(file);
            crate::process::run_with_timeout(cmd, TOOL_TIMEOUT)
        };
        match with_report(&std::env::temp_dir(), run) {
            Err(_) => Reading::unavailable("Dyno could not create a temporary folder for the battery report."),
            Ok((Err(e), _)) => Reading::unavailable(e.to_string()),
            Ok((Ok(_), Some(xml))) => {
                crate::parse::hardware::parse_powercfg_battery(&xml).map_or_else(|| Reading::unavailable(NO_BATTERY), Reading::Available)
            }
            Ok((Ok(_), None)) => Reading::unavailable(NO_BATTERY),
        }
    }
}

// ------------------------------------------------------------------------------------------------------- fakes

/// A throttle reader that answers what it was given, then repeats the last answer.
#[cfg(any(test, feature = "test-support"))]
pub struct FakeThrottle(pub Vec<Reading<OsThrottle>>);

#[cfg(any(test, feature = "test-support"))]
impl ThrottleSignals for FakeThrottle {
    fn read(&mut self) -> Reading<OsThrottle> {
        if self.0.len() > 1 {
            self.0.remove(0)
        } else {
            self.0.first().cloned().unwrap_or_else(|| Reading::unavailable("no reading"))
        }
    }
}

/// An SSD wear reader that answers what it was given.
#[cfg(any(test, feature = "test-support"))]
pub struct FakeWear(pub Reading<Vec<DiskWear>>);

#[cfg(any(test, feature = "test-support"))]
impl DiskWearSource for FakeWear {
    fn read(&mut self) -> Reading<Vec<DiskWear>> {
        self.0.clone()
    }
}

/// A battery reader that answers what it was given.
#[cfg(any(test, feature = "test-support"))]
pub struct FakeBattery(pub Reading<BatteryCapacity>);

#[cfg(any(test, feature = "test-support"))]
impl BatteryCapacitySource for FakeBattery {
    fn read(&mut self) -> Reading<BatteryCapacity> {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_battery_report_goes_into_a_fresh_folder_that_is_gone_afterwards_never_a_fixed_name() {
        let root = temp("battery-fresh");
        // A planted folder with the old fixed name is never used or touched.
        std::fs::create_dir(root.join("Dyno")).unwrap();
        std::fs::write(root.join("Dyno").join("battery-report.xml"), "planted").unwrap();
        let (path, text) = with_report(&root, |f| {
            assert!(f.parent().unwrap().starts_with(&root));
            assert_ne!(f.parent().unwrap(), root.join("Dyno"));
            std::fs::write(f, "real").unwrap();
            f.to_path_buf()
        })
        .unwrap();
        assert_eq!(text.as_deref(), Some("real"));
        assert!(!path.parent().unwrap().exists(), "the folder is removed");
        assert_eq!(
            std::fs::read_to_string(root.join("Dyno").join("battery-report.xml")).unwrap(),
            "planted"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn the_report_folder_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp("battery-mode");
        let d = fresh_private_folder(&root).unwrap();
        assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn two_report_folders_never_share_a_name() {
        let root = temp("battery-unique");
        let a = fresh_private_folder(&root).unwrap();
        let b = fresh_private_folder(&root).unwrap();
        assert_ne!(a, b);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn a_report_folder_under_a_parent_others_can_change_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp("battery-open");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        let e = fresh_private_folder(&root).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Windows: the report folder is made owner-only (the protected descriptor, no inherited entry) and is refused under a temp folder
    /// that Everyone may change. Owed: runs on the Windows runner or VM.
    #[cfg(windows)]
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a Windows-only test runs icacls on its own temporary folder to set and read its access list"
    )]
    fn windows_the_report_folder_has_no_entry_for_others_and_an_open_temp_is_refused() {
        let root = temp("battery-win");
        let d = fresh_private_folder(&root).unwrap();
        let out = std::process::Command::new("icacls").arg(&d).output().unwrap();
        // The output starts with the folder's own path (`C:\\Users\\...`), which is not an entry: the path is cut out, and what is left is
        // checked for entries (`<principal>:(<rights>)`) of the principals that must not be there.
        let text = String::from_utf8_lossy(&out.stdout).replace(&d.display().to_string(), "");
        for other in ["Everyone", "BUILTIN\\Users", "NT AUTHORITY\\Authenticated Users"] {
            assert!(!text.contains(&format!("{other}:(")), "{other} must have no entry: {text}");
        }
        assert!(text.contains(":("), "the folder has entries at all (the parse saw them): {text}");
        let g = std::process::Command::new("icacls")
            .arg(&root)
            .args(["/grant", "*S-1-1-0:(OI)(CI)F"])
            .output()
            .unwrap();
        assert!(g.status.success());
        assert!(fresh_private_folder(&root).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn the_report_is_not_read_when_it_is_not_a_file_or_is_too_big() {
        let root = temp("battery-read");
        let real = root.join("real.xml");
        std::fs::write(&real, "x").unwrap();
        assert_eq!(read_report(&real).as_deref(), Some("x"));
        assert_eq!(read_report(&root), None, "a folder is not a report");
        let big = root.join("big.xml");
        std::fs::File::create(&big).unwrap().set_len(MAX_REPORT_BYTES + 1).unwrap();
        assert_eq!(read_report(&big), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn temp(name: &str) -> PathBuf {
        // A fresh folder the test removes itself (the callers end with `remove_dir_all`).
        paddock_testkit::PrivateTemp::with_prefix(&format!("dyno-it-hw-{name}-"), None)
            .unwrap()
            .keep()
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn linux_throttle_counters_rise_between_reads() {
        let root = temp("throttle");
        let cpu0 = root.join("devices/system/cpu/cpu0");
        write(&cpu0.join("thermal_throttle/core_throttle_count"), "4\n");
        write(&cpu0.join("thermal_throttle/package_throttle_count"), "4\n");
        write(&cpu0.join("cpufreq/cpuinfo_max_freq"), "4000000\n");
        write(&cpu0.join("cpufreq/scaling_max_freq"), "4000000\n");
        write(&root.join("devices/system/cpu/cpufreq/ignored"), "x");
        let mut t = LinuxThrottle::new(root.clone());
        let first = t.read();
        let first = first.value().unwrap();
        assert_eq!(
            (first.thermal, first.speed_limit_percent, first.power),
            (None, Some(100), Some(false)),
            "no baseline yet"
        );
        let same = t.read();
        assert_eq!(same.value().unwrap().thermal, Some(false));
        write(&cpu0.join("thermal_throttle/core_throttle_count"), "9\n");
        assert_eq!(t.read().value().unwrap().thermal, Some(true));
        assert_eq!(t.read().value().unwrap().thermal, Some(false), "the count did not move since");
        // A frequency cap reads as a power limit.
        write(&cpu0.join("cpufreq/scaling_max_freq"), "1200000\n");
        let capped = t.read();
        let capped = capped.value().unwrap();
        assert_eq!((capped.speed_limit_percent, capped.power), (Some(30), Some(true)));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn linux_without_counters_says_so() {
        let root = temp("novm");
        let mut t = LinuxThrottle::new(root.clone());
        assert!(matches!(t.read(), Reading::Unavailable(m) if m.contains("virtual machines")));
        write(&root.join("devices/system/cpu/cpu0/other"), "1");
        assert!(t.read().value().is_none());
        assert!(LinuxThrottle::new(PathBuf::from("/dyno/no/such/root")).read().value().is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sysfs_battery_energy_and_charge_forms() {
        let dir = temp("bat");
        write(&dir.join("AC/type"), "Mains\n");
        write(&dir.join("BAT0/type"), "Battery\n");
        write(&dir.join("BAT0/energy_full_design"), "57000000\n");
        write(&dir.join("BAT0/energy_full"), "51000000\n");
        write(&dir.join("BAT0/cycle_count"), "212\n");
        write(&dir.join("BAT0/serial_number"), "SN42\n");
        let b = SysfsBattery::new(dir.clone()).read();
        let b = b.value().unwrap();
        assert_eq!(
            (b.id.as_str(), b.design, b.full, b.cycles),
            ("SN42", 57_000_000, 51_000_000, Some(212))
        );
        std::fs::remove_file(dir.join("BAT0/energy_full")).unwrap();
        std::fs::remove_file(dir.join("BAT0/energy_full_design")).unwrap();
        std::fs::remove_file(dir.join("BAT0/serial_number")).unwrap();
        write(&dir.join("BAT0/charge_full_design"), "4000000\n");
        write(&dir.join("BAT0/charge_full"), "3500000\n");
        write(&dir.join("BAT0/cycle_count"), "0\n");
        let b = SysfsBattery::new(dir.clone()).read();
        let b = b.value().unwrap();
        assert_eq!((b.id.as_str(), b.full, b.cycles), ("BAT0", 3_500_000, None));
        write(&dir.join("BAT0/charge_full_design"), "0\n");
        assert_eq!(SysfsBattery::new(dir.clone()).read(), Reading::unavailable(NO_BATTERY));
        assert_eq!(
            SysfsBattery::new(PathBuf::from("/dyno/none")).read(),
            Reading::unavailable(NO_BATTERY)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn dynos_own_copy_is_tried_before_the_system_places_and_the_newest_first() {
        let dir = temp("own-tools");
        for v in ["smartmontools-7.4", "smartmontools-7.5"] {
            std::fs::create_dir_all(dir.join(v)).unwrap();
        }
        std::fs::create_dir_all(dir.join("other-tool")).unwrap();
        let c: Vec<PathBuf> = own_copies(Some(&dir));
        let name = |i: usize| c[i].parent().unwrap().file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            (name(0), name(1)),
            ("smartmontools-7.5".to_string(), "smartmontools-7.4".to_string())
        );
        assert!(c.iter().all(|p| !p.to_string_lossy().contains("other-tool")));
        assert_eq!(c.len(), 2);
        assert!(own_copies(Some(&dir.join("missing"))).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_installed_after_the_reader_was_made_is_found_without_a_restart() {
        let _serial = script_tests();
        let work = temp("late-tools");
        let tool = fake_smartctl(
            &work,
            r#"{"devices":[{"name":"/dev/nvme0"}]}"#,
            crate::parse::hardware::tests_nvme(),
        );
        let tools = temp("late-own");
        let mut reader = SmartctlWear::new(vec![PathBuf::from("/dyno/missing")]).with_own_tools(tools.clone());
        assert!(reader.read().value().is_none(), "nothing installed yet");
        let folder = tools.join("smartmontools-7.5");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::copy(&tool, folder.join("smartctl")).unwrap();
        assert_eq!(reader.read().value().unwrap()[0].percent_used, Some(3.0));
        let _ = std::fs::remove_dir_all(work);
        let _ = std::fs::remove_dir_all(tools);
    }

    /// Tests that write a script and run it take turns. Without it, a test that forks while another has its script open for writing
    /// makes that run fail with "Text file busy" (ETXTBSY) on Linux. It orders these tests among themselves only.
    #[cfg(unix)]
    fn script_tests() -> std::sync::MutexGuard<'static, ()> {
        static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
        TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(unix)]
    fn fake_smartctl(dir: &Path, scan: &str, drive: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(dir.join("scan.json"), scan).unwrap();
        std::fs::write(dir.join("drive.json"), drive).unwrap();
        let script = dir.join("smartctl");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"--scan\" ]; then cat '{0}/scan.json'; else cat '{0}/drive.json'; fi\nexit 4\n",
                dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    #[cfg(unix)]
    #[test]
    fn smartctl_output_is_read_even_though_its_exit_status_is_a_bit_mask() {
        let _serial = script_tests();
        let dir = temp("smart");
        let tool = fake_smartctl(&dir, r#"{"devices":[{"name":"/dev/nvme0"}]}"#, crate::parse::hardware::tests_nvme());
        let r = SmartctlWear::new(vec![PathBuf::from("/dyno/missing"), tool]).read();
        let w = &r.value().unwrap()[0];
        assert_eq!(w.percent_used, Some(3.0));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn apple_silicon_scan_with_three_long_names_gives_one_ssd_once() {
        let _serial = script_tests();
        let dir = temp("smart-ios");
        let scan = crate::parse::smart::tests::apple_silicon_scan();
        let tool = fake_smartctl(&dir, &scan, crate::parse::hardware::tests_nvme());
        let r = SmartctlWear::new(vec![tool]).with_host(HelperHost::Macos).read();
        let all = r.value().unwrap_or_else(|| panic!("a read with data is a success, got {r:?}"));
        assert_eq!(all.len(), 1, "three namespaces of one SSD are one disk: {all:?}");
        assert_eq!(all[0].percent_used, Some(3.0));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A smartctl that answers per drive: the USB entries fail to open (as on macOS, no USB SMART passthrough), the rest read.
    #[cfg(unix)]
    fn usb_failing_smartctl(dir: &Path, scan: &str, drive: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let usb = r#"{"smartctl":{"exit_status":2,"messages":[{"string":"Smartctl open device: IOService:/x/usb-drd1@B000000/IOBlockStorageServices failed: IOCreatePlugInInterfaceForService failed","severity":"error"}]}}"#;
        std::fs::write(dir.join("scan.json"), scan).unwrap();
        std::fs::write(dir.join("drive.json"), drive).unwrap();
        std::fs::write(dir.join("usb.json"), usb).unwrap();
        let script = dir.join("smartctl");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"--scan\" ]; then cat '{0}/scan.json'; exit 0; fi\ncase \"$*\" in *usb-*) cat '{0}/usb.json'; exit 2;; *) cat '{0}/drive.json'; exit 4;; esac\n",
                dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_usb_drive_does_not_hide_the_internal_ssd_and_is_explained_truthfully() {
        let _serial = script_tests();
        let dir = temp("smart-usb");
        let scan = crate::parse::smart::tests::internal_and_usb_scan();
        let tool = usb_failing_smartctl(&dir, &scan, crate::parse::hardware::tests_nvme());
        let r = SmartctlWear::new(vec![tool]).with_host(HelperHost::Macos).read();
        let all = r.value().unwrap_or_else(|| panic!("the internal SSD reads, got {r:?}"));
        assert_eq!((all.len(), all[0].percent_used), (1, Some(3.0)));
        // Only USB drives: the reason is about USB on macOS, and never sends a person to the helper or administrator rights.
        let only_usb = temp("smart-usb-only");
        let usb_scan = scan.replace(
            "ans@A000000/AppleANS3NVMeController/NS_01@1",
            "usb-drd0@B000000/IOBlockStorageServices",
        );
        let tool = usb_failing_smartctl(&only_usb, &usb_scan, "{}");
        let r = SmartctlWear::new(vec![tool]).with_host(HelperHost::Macos).read();
        let Reading::Unavailable(m) = r else { panic!("{r:?}") };
        assert!(m.contains("USB"), "{m}");
        assert!(
            !m.contains("helper") && !m.contains("Administrator") && !m.contains("administrator"),
            "{m}"
        );
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(only_usb);
    }

    #[cfg(unix)]
    #[test]
    fn smartctl_refusals_name_the_next_step() {
        let _serial = script_tests();
        let dir = temp("smartdeny");
        let denied = r#"{"smartctl":{"messages":[{"string":"open device failed: Permission denied"}]}}"#;
        let tool = fake_smartctl(&dir, r#"{"devices":[{"name":"/dev/disk0"}]}"#, denied);
        assert!(
            matches!(SmartctlWear::new(vec![tool.clone()]).with_host(HelperHost::Macos).read(), Reading::Unavailable(m) if m.contains("administrator rights"))
        );
        // Windows words the refusal differently and has its own next step.
        let win = r#"{"smartctl":{"messages":[{"string":"Smartctl open device: /dev/sdb failed: Access is denied"}]}}"#;
        let tool_win = fake_smartctl(&temp("smartdeny-win"), r#"{"devices":[{"name":"/dev/sdb","type":"sat"}]}"#, win);
        assert!(matches!(
            SmartctlWear::new(vec![tool_win]).with_host(HelperHost::Windows).read(),
            Reading::Unavailable(m) if m.contains("Administrator readings (Settings)") && m.contains("/dev/sdb")
        ));
        let blank = fake_smartctl(&dir, r#"{"devices":[{"name":"/dev/disk0"}]}"#, "{}");
        assert!(matches!(SmartctlWear::new(vec![blank]).read(), Reading::Unavailable(m) if m.contains("No SSD")));
        let none = fake_smartctl(&dir, "{}", "{}");
        assert!(matches!(SmartctlWear::new(vec![none]).read(), Reading::Unavailable(m) if m.contains("no drives")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_smartctl_is_explained_for_this_os_only() {
        for (host, has, hasnt) in [
            (HelperHost::Windows, "Program Files", "brew"),
            (HelperHost::Macos, "brew install smartmontools", "apt"),
            (HelperHost::Linux, "apt install smartmontools", "brew"),
        ] {
            let r = SmartctlWear::new(vec![PathBuf::from("/dyno/missing/smartctl")])
                .with_host(host)
                .read();
            assert!(
                matches!(&r, Reading::Unavailable(m) if m.contains(has) && !m.contains(hasnt) && !m.contains("helper")),
                "{host:?}: {r:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_device_type_of_the_scan_is_passed_to_smartctl() {
        let _serial = script_tests();
        use std::os::unix::fs::PermissionsExt;
        let dir = temp("smart-d");
        let script = dir.join("smartctl");
        let log = dir.join("args.log");
        let scan = r#"{"devices":[{"name":"/dev/sdb","type":"sat"},{"name":"/dev/nvme0"}]}"#;
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho \"$@\" >> '{log}'\nif [ \"$1\" = \"--scan\" ]; then echo '{scan}'; else echo '{drive}'; fi\nexit 4\n",
                log = log.display(),
                drive = crate::parse::hardware::tests_nvme().replace('\n', " "),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = SmartctlWear::new(vec![script]).read();
        let seen = std::fs::read_to_string(&log).unwrap();
        assert!(seen.lines().any(|l| l == "-a -j -d sat /dev/sdb"), "{seen}");
        assert!(
            seen.lines().any(|l| l == "-a -j /dev/nvme0"),
            "no -d when the scan gave no type: {seen}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_drive_that_prints_nothing_is_explained() {
        let _serial = script_tests();
        use std::os::unix::fs::PermissionsExt;
        let dir = temp("smart-run");
        let script = dir.join("smartctl");
        // The scan works; the drive run exits without any output at all.
        std::fs::write(
            &script,
            "#!/bin/sh\nif [ \"$1\" = \"--scan\" ]; then echo '{\"devices\":[{\"name\":\"/dev/sda\"}]}'; fi\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(SmartctlWear::new(vec![script]).read(), Reading::Unavailable(m) if m.contains("No SSD")));
        assert!(matches!(
            SmartctlWear::new(vec![dir.join("not-executable-dir")]).read(),
            Reading::Unavailable(_)
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fakes_answer_what_they_were_given() {
        let t = OsThrottle {
            thermal: Some(true),
            ..Default::default()
        };
        let mut f = FakeThrottle(vec![Reading::unavailable("first"), Reading::Available(t)]);
        assert!(f.read().value().is_none());
        assert_eq!(f.read().value(), Some(&t));
        assert_eq!(f.read().value(), Some(&t), "the last answer repeats");
        assert!(FakeThrottle(vec![]).read().value().is_none());
        assert!(NoThrottleSignals("x").read().value().is_none());
        assert!(FakeWear(Reading::Available(vec![])).read().value().is_some());
        assert!(FakeBattery(Reading::unavailable("n")).read().value().is_none());
    }

    #[test]
    fn this_machines_readers_never_panic_or_hang() {
        // Read-only calls against the real machine: any answer is fine, a panic or hang is not.
        let _ = throttle_signals().read();
        let _ = battery_capacity().read();
        let _ = disk_wear().read();
    }
}
