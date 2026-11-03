//! The helper's sensor readings (ADR 0027 WP-6): Linux sysfs (thermal zones, hwmon fans, RAPL energy; the energy counters are
//! root-only since the 2020 side-channel fix), macOS `powermetrics` (root), Windows `MSAcpi_ThermalZoneTemperature` (WMI
//! `root/wmi`, administrators). The sysfs readers take the `/sys` root so a test builds a tree in a temporary folder; the text
//! parsers are pure. The helper reads only these fixed places; no request names a path.

use crate::runner::ToolRunner;
use dyno_core::helper::HelperHost;
use dyno_core::helper_counters::{CounterReader, CounterValue, EnergyDomain, FanSpeed, ThermalZone, MAX_ENTRIES};
use dyno_core::helper_requests::CounterId;
use dyno_core::ports::PlatformError;
use std::path::Path;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);
const POWERMETRICS: &str = "/usr/bin/powermetrics";
const POWERSHELL: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";
const MSACPI: &str = "Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature | Select-Object InstanceName,CurrentTemperature | ConvertTo-Json -Compress";
/// The largest sensor file or tool output read.
const MAX_TEXT: u64 = 1 << 20;

fn read_small(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path).ok()?.take(4096).read_to_string(&mut text).ok()?;
    Some(text.trim().to_string())
}

fn entries(dir: &Path, prefix: &str) -> Vec<std::path::PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<_> = rd
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
        .map(|e| e.path())
        .collect();
    v.sort();
    v.truncate(MAX_ENTRIES);
    v
}

/// Thermal zones (`class/thermal/thermal_zone*/{type,temp}`) and hwmon temperatures (`class/hwmon/hwmon*/temp*_input`).
pub fn linux_thermal(sys: &Path) -> Vec<ThermalZone> {
    let mut out = Vec::new();
    for zone in entries(&sys.join("class/thermal"), "thermal_zone") {
        let Some(milli) = read_small(&zone.join("temp")).and_then(|t| t.parse::<i32>().ok()) else {
            continue;
        };
        let label = read_small(&zone.join("type")).unwrap_or_else(|| "thermal zone".into());
        out.push(ThermalZone {
            label,
            milli_celsius: milli,
        });
    }
    for hw in entries(&sys.join("class/hwmon"), "hwmon") {
        let name = read_small(&hw.join("name")).unwrap_or_else(|| "hwmon".into());
        for t in entries(&hw, "temp").into_iter().filter(|p| p.to_string_lossy().ends_with("_input")) {
            let Some(milli) = read_small(&t).and_then(|x| x.parse::<i32>().ok()) else {
                continue;
            };
            let stem = t.file_name().map(|n| n.to_string_lossy().replace("_input", "")).unwrap_or_default();
            let label = read_small(&hw.join(format!("{stem}_label"))).unwrap_or_else(|| format!("{name} {stem}"));
            out.push(ThermalZone {
                label,
                milli_celsius: milli,
            });
        }
    }
    out.truncate(MAX_ENTRIES);
    out
}

/// Fans (`class/hwmon/hwmon*/fan*_input`, rpm).
pub fn linux_fans(sys: &Path) -> Vec<FanSpeed> {
    let mut out = Vec::new();
    for hw in entries(&sys.join("class/hwmon"), "hwmon") {
        let name = read_small(&hw.join("name")).unwrap_or_else(|| "hwmon".into());
        for f in entries(&hw, "fan").into_iter().filter(|p| p.to_string_lossy().ends_with("_input")) {
            let Some(rpm) = read_small(&f).and_then(|x| x.parse::<u32>().ok()) else {
                continue;
            };
            let stem = f.file_name().map(|n| n.to_string_lossy().replace("_input", "")).unwrap_or_default();
            let label = read_small(&hw.join(format!("{stem}_label"))).unwrap_or_else(|| format!("{name} {stem}"));
            out.push(FanSpeed { label, rpm });
        }
    }
    out.truncate(MAX_ENTRIES);
    out
}

/// RAPL energy domains (`class/powercap/intel-rapl*/{name,energy_uj,max_energy_range_uj}`), cumulative microjoules.
pub fn linux_rapl(sys: &Path) -> Result<Vec<EnergyDomain>, PlatformError> {
    let mut out = Vec::new();
    let mut denied = false;
    for d in entries(&sys.join("class/powercap"), "intel-rapl:")
        .into_iter()
        .chain(entries(&sys.join("class/powercap"), "amd-rapl:"))
    {
        match std::fs::read_to_string(d.join("energy_uj")) {
            Ok(t) => {
                let Ok(uj) = t.trim().parse::<u64>() else { continue };
                out.push(EnergyDomain {
                    label: read_small(&d.join("name")).unwrap_or_else(|| "energy".into()),
                    microjoules: Some(uj),
                    wraps_at_microjoules: read_small(&d.join("max_energy_range_uj")).and_then(|x| x.parse().ok()),
                    milliwatts: None,
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied = true,
            Err(_) => {}
        }
    }
    match (out.is_empty(), denied) {
        (true, true) => Err(PlatformError::AccessDenied(
            "The CPU energy counters are readable only by an administrator.".into(),
        )),
        (true, false) => Err(PlatformError::Unsupported(
            "This computer does not publish CPU energy counters.".into(),
        )),
        _ => Ok(out),
    }
}

/// `powermetrics` text: lines such as `CPU Power: 1234 mW` and `Fan: 2150 rpm`.
pub fn parse_powermetrics(text: &str) -> (Vec<EnergyDomain>, Vec<FanSpeed>) {
    let mut energy = Vec::new();
    let mut fans = Vec::new();
    for line in text.lines().take(20_000) {
        let Some((key, val)) = line.split_once(':') else { continue };
        let (key, val) = (key.trim(), val.trim());
        let number = |unit: &str| val.strip_suffix(unit).and_then(|n| n.trim().parse::<f64>().ok());
        if let Some(mw) = number("mW") {
            if key.ends_with("Power") || key.contains("Power (") {
                energy.push(EnergyDomain {
                    label: key.chars().take(64).collect(),
                    microjoules: None,
                    wraps_at_microjoules: None,
                    milliwatts: Some(mw.clamp(0.0, f64::from(u32::MAX)) as u32),
                });
            }
        } else if key.to_ascii_lowercase().starts_with("fan") {
            if let Some(rpm) = number("rpm") {
                fans.push(FanSpeed {
                    label: key.chars().take(64).collect(),
                    rpm: rpm.clamp(0.0, f64::from(u32::MAX)) as u32,
                });
            }
        }
    }
    energy.truncate(MAX_ENTRIES);
    fans.truncate(MAX_ENTRIES);
    (energy, fans)
}

/// `MSAcpi_ThermalZoneTemperature` rows as JSON; `CurrentTemperature` is tenths of a kelvin. A bare object (one zone) is
/// accepted as well as an array.
pub fn parse_msacpi(text: &str) -> Vec<ThermalZone> {
    if text.len() as u64 > MAX_TEXT {
        return Vec::new();
    }
    let value: serde_json::Value = match serde_json::from_str(text.trim().trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let rows = match value {
        serde_json::Value::Array(a) => a,
        v @ serde_json::Value::Object(_) => vec![v],
        _ => Vec::new(),
    };
    rows.iter()
        .filter_map(|r| {
            let tenths_k = r.get("CurrentTemperature")?.as_f64()?;
            let milli = (tenths_k * 100.0 - 273_150.0).round();
            // A zone that reports 0 K or something absurd is a firmware placeholder.
            (-100_000.0..=300_000.0).contains(&milli).then(|| ThermalZone {
                label: r
                    .get("InstanceName")
                    .and_then(|n| n.as_str())
                    .unwrap_or("thermal zone")
                    .chars()
                    .take(64)
                    .collect(),
                milli_celsius: milli as i32,
            })
        })
        .take(MAX_ENTRIES)
        .collect()
}

/// The real reader of this system.
pub struct SystemCounters {
    host: HelperHost,
    sys: std::path::PathBuf,
    runner: Box<dyn ToolRunner>,
}

impl SystemCounters {
    /// A reader for `host` (sysfs at `/sys`).
    pub fn new(host: HelperHost, runner: Box<dyn ToolRunner>) -> SystemCounters {
        SystemCounters {
            host,
            sys: "/sys".into(),
            runner,
        }
    }

    /// A reader of a sysfs tree elsewhere (tests).
    pub fn with_sys_root(mut self, sys: impl Into<std::path::PathBuf>) -> SystemCounters {
        self.sys = sys.into();
        self
    }

    fn powermetrics(&self) -> Result<(Vec<EnergyDomain>, Vec<FanSpeed>), PlatformError> {
        let args = ["--samplers", "cpu_power,smc", "-n", "1", "-i", "200"].map(String::from);
        Ok(parse_powermetrics(&self.runner.run(POWERMETRICS, &args, TIMEOUT)?))
    }
}

fn nothing(what: &str) -> PlatformError {
    PlatformError::Unsupported(format!("This computer does not report {what} to software."))
}

impl CounterReader for SystemCounters {
    fn read(&self, id: CounterId) -> Result<CounterValue, PlatformError> {
        match (self.host, id) {
            (HelperHost::Linux, CounterId::Thermal) => Ok(CounterValue::Thermal(linux_thermal(&self.sys))),
            (HelperHost::Linux, CounterId::Fans) => Ok(CounterValue::Fans(linux_fans(&self.sys))),
            (HelperHost::Linux, CounterId::CpuEnergy) => linux_rapl(&self.sys).map(CounterValue::CpuEnergy),
            (HelperHost::Macos, CounterId::CpuEnergy) => self.powermetrics().map(|(e, _)| CounterValue::CpuEnergy(e)),
            (HelperHost::Macos, CounterId::Fans) => self.powermetrics().map(|(_, f)| CounterValue::Fans(f)),
            (HelperHost::Macos, CounterId::Thermal) => Err(PlatformError::Unsupported(
                "macOS reports a thermal pressure level, not temperatures, to software.".into(),
            )),
            (HelperHost::Windows, CounterId::Thermal) => {
                let args = ["-NoProfile", "-NonInteractive", "-Command", MSACPI].map(String::from);
                let zones = parse_msacpi(&self.runner.run(POWERSHELL, &args, TIMEOUT)?);
                if zones.is_empty() {
                    Err(nothing("thermal zones"))
                } else {
                    Ok(CounterValue::Thermal(zones))
                }
            }
            (HelperHost::Windows, CounterId::Fans) => Err(nothing("fan speeds")),
            (HelperHost::Windows, CounterId::CpuEnergy) => Err(nothing("CPU energy")),
        }
    }
}

/// The memory slot reader: the family's own inventory reader, run as the helper's account (root on Linux, where the slot table
/// is root-only). Its layout type has no serial number field.
pub struct SystemMemory;

impl dyno_core::helper_memory::MemoryReader for SystemMemory {
    fn read(&self) -> dyno_core::inventory::MemoryLayout {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        crate::inventory::read_memory_layout(sys.total_memory())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{Tool, ToolOutput};

    fn put(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn a_sysfs_tree_gives_zones_fans_and_energy_domains() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        put(r, "class/thermal/thermal_zone0/type", "acpitz\n");
        put(r, "class/thermal/thermal_zone0/temp", "47000\n");
        put(r, "class/hwmon/hwmon2/name", "coretemp\n");
        put(r, "class/hwmon/hwmon2/temp1_input", "51000\n");
        put(r, "class/hwmon/hwmon2/temp1_label", "Package id 0\n");
        put(r, "class/hwmon/hwmon3/name", "dell_smm\n");
        put(r, "class/hwmon/hwmon3/fan1_input", "2150\n");
        put(r, "class/powercap/intel-rapl:0/name", "package-0\n");
        put(r, "class/powercap/intel-rapl:0/energy_uj", "123456789\n");
        put(r, "class/powercap/intel-rapl:0/max_energy_range_uj", "262143328850\n");
        let zones = linux_thermal(r);
        assert_eq!(
            zones.iter().map(|z| (z.label.as_str(), z.milli_celsius)).collect::<Vec<_>>(),
            [("acpitz", 47_000), ("Package id 0", 51_000)]
        );
        assert_eq!(
            linux_fans(r),
            [FanSpeed {
                label: "dell_smm fan1".into(),
                rpm: 2150
            }]
        );
        let e = linux_rapl(r).unwrap();
        assert_eq!(
            (e[0].label.as_str(), e[0].microjoules, e[0].wraps_at_microjoules),
            ("package-0", Some(123_456_789), Some(262_143_328_850))
        );
    }

    #[test]
    fn missing_trees_are_empty_or_a_reason_never_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        assert!(linux_thermal(dir.path()).is_empty() && linux_fans(dir.path()).is_empty());
        assert!(matches!(linux_rapl(dir.path()), Err(PlatformError::Unsupported(_))));
        let junk = tempfile::tempdir().unwrap();
        put(junk.path(), "class/thermal/thermal_zone0/temp", "not a number");
        assert!(linux_thermal(junk.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_energy_counters_say_they_need_an_administrator() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "class/powercap/intel-rapl:0/energy_uj", "1\n");
        let f = dir.path().join("class/powercap/intel-rapl:0/energy_uj");
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Running as root reads it anyway; the assertion holds for both outcomes.
        match linux_rapl(dir.path()) {
            Err(PlatformError::AccessDenied(m)) => assert!(m.contains("administrator")),
            Ok(e) => assert_eq!(e.len(), 1),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn powermetrics_text_gives_power_and_fans() {
        let text = "**** Processor usage ****\nCPU Power: 1234 mW\nGPU Power: 56 mW\nCombined Power (CPU + GPU + ANE): 1300 mW\nFan: 2150 rpm\nCurrent pressure level: Nominal\n";
        let (e, f) = parse_powermetrics(text);
        assert_eq!(
            e.iter().map(|d| (d.label.as_str(), d.milliwatts)).collect::<Vec<_>>(),
            [
                ("CPU Power", Some(1234)),
                ("GPU Power", Some(56)),
                ("Combined Power (CPU + GPU + ANE)", Some(1300))
            ]
        );
        assert_eq!(
            f,
            [FanSpeed {
                label: "Fan".into(),
                rpm: 2150
            }]
        );
    }

    #[test]
    fn msacpi_rows_convert_tenths_of_kelvin_and_drop_placeholders() {
        let one = r#"{"InstanceName":"ACPI\\ThermalZone\\TZ00_0","CurrentTemperature":3182}"#;
        let z = parse_msacpi(one);
        assert_eq!((z.len(), z[0].milli_celsius), (1, 45_050));
        let many = r#"[{"InstanceName":"a","CurrentTemperature":3000},{"InstanceName":"b","CurrentTemperature":0},{"x":1}]"#;
        assert_eq!(parse_msacpi(many).len(), 1);
        assert!(parse_msacpi("garbage").is_empty());
    }

    struct Scripted(&'static str);
    impl ToolRunner for Scripted {
        fn run_tool(&self, _: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
            Ok(ToolOutput {
                ok: true,
                stdout: self.0.into(),
                error: String::new(),
            })
        }
    }

    #[test]
    fn each_host_answers_what_it_can_and_says_why_for_the_rest() {
        let mac = SystemCounters::new(HelperHost::Macos, Box::new(Scripted("CPU Power: 10 mW\nFan: 900 rpm\n")));
        assert!(matches!(mac.read(CounterId::CpuEnergy), Ok(CounterValue::CpuEnergy(e)) if e.len() == 1));
        assert!(matches!(mac.read(CounterId::Fans), Ok(CounterValue::Fans(f)) if f[0].rpm == 900));
        assert!(matches!(mac.read(CounterId::Thermal), Err(PlatformError::Unsupported(_))));
        let win = SystemCounters::new(
            HelperHost::Windows,
            Box::new(Scripted(r#"{"InstanceName":"z","CurrentTemperature":3000}"#)),
        );
        assert!(matches!(win.read(CounterId::Thermal), Ok(CounterValue::Thermal(z)) if z[0].milli_celsius == 26_850));
        assert!(matches!(win.read(CounterId::Fans), Err(PlatformError::Unsupported(_))));
        let empty = SystemCounters::new(HelperHost::Windows, Box::new(Scripted("")));
        assert!(empty.read(CounterId::Thermal).is_err());
    }
}
