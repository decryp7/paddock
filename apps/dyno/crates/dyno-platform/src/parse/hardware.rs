//! Parsers for what the OS and its tools say about CPU speed limits, SSD wear and battery capacity: `pmset -g therm`,
//! `smartctl -j`, `ioreg -rn AppleSmartBattery` and `powercfg /batteryreport /xml`. Pure, compiled on every OS.

use dyno_core::hardware::{BatteryCapacity, DiskWear, OsThrottle};
use serde_json::Value;

/// `pmset -g therm`: the speed limit and whether a heat or power warning has been recorded. `None` when the output is
/// not recognised at all.
pub fn parse_pmset_therm(text: &str) -> Option<OsThrottle> {
    let value_of = |key: &str| -> Option<u8> {
        let line = text.lines().find(|l| l.trim_start().starts_with(key))?;
        let digits: String = line.split('=').nth(1)?.trim().chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().ok().map(|v| v.min(100) as u8)
    };
    let speed = value_of("CPU_Speed_Limit");
    let scheduler = value_of("CPU_Scheduler_Limit");
    let lower = text.to_ascii_lowercase();
    let no_thermal = lower.contains("no thermal warning level has been recorded");
    let no_perf = lower.contains("no performance warning level has been recorded");
    let no_power = lower.contains("no cpu power status has been recorded");
    if speed.is_none() && scheduler.is_none() && !no_thermal && !no_perf && !no_power {
        return None;
    }
    let limit = match (speed, scheduler) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let limited = limit.is_some_and(|l| l < 100);
    // A recorded thermal warning level is a heat limit even when the numeric limit is absent.
    let thermal_warning = lower
        .lines()
        .any(|l| l.contains("thermal warning level") && !l.contains("no thermal") && l.chars().any(|c| c.is_ascii_digit()));
    Some(OsThrottle {
        speed_limit_percent: limit,
        thermal: if limited || thermal_warning {
            Some(true)
        } else if no_thermal || limit == Some(100) {
            Some(false)
        } else {
            None
        },
        power: if no_power { Some(false) } else { None },
    })
}

/// Why a `smartctl` run produced no data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The OS withheld the drive from this account (needs administrator rights).
    Access,
    /// The drive does not expose SMART data to apps.
    Unsupported,
}

/// Why a `smartctl` run produced no data, as a kind; `None` when it produced data or said nothing useful.
pub fn smartctl_refusal_kind(json: &str) -> Option<Refusal> {
    let v: Value = serde_json::from_str(json).ok()?;
    let msgs = v["smartctl"]["messages"].as_array()?;
    let text: Vec<&str> = msgs.iter().filter_map(|m| m["string"].as_str()).collect();
    let joined = text.join(" ").to_ascii_lowercase();
    // "Access is denied" is Windows' wording for the same refusal as Unix's "Permission denied".
    if joined.contains("permission denied")
        || joined.contains("access is denied")
        || joined.contains("access denied")
        || joined.contains("operation not permitted")
        || joined.contains("administrator")
    {
        Some(Refusal::Access)
    } else if joined.contains("unsupported") || joined.contains("unknown usb bridge") || joined.contains("smart support is: unavailable") {
        Some(Refusal::Unsupported)
    } else {
        None
    }
}

/// Why a `smartctl` run produced no data, in words for the user; `None` when it produced data.
pub fn smartctl_refusal(json: &str) -> Option<String> {
    smartctl_refusal_kind(json).map(|k| match k {
        Refusal::Access => "Reading SSD wear needs administrator rights.".to_string(),
        Refusal::Unsupported => "This drive does not expose SMART data to apps.".to_string(),
    })
}

/// The version in `smartctl --version` output (`smartctl 7.5 2025-04-30 r5714 [x86_64-w64-mingw32-...]`).
pub fn parse_smartctl_version(text: &str) -> Option<String> {
    let first = text.lines().find(|l| l.trim_start().starts_with("smartctl "))?;
    let v = first.trim_start().strip_prefix("smartctl ")?.split_whitespace().next()?;
    (v.len() < 24 && v.chars().next().is_some_and(|c| c.is_ascii_digit())).then(|| v.to_string())
}

/// One drive from `smartctl -a -j <device>`. `None` for hard disks and for drives with no wear counter at all.
pub fn parse_smartctl_ssd(json: &str) -> Option<DiskWear> {
    let v: Value = serde_json::from_str(json).ok()?;
    if v["rotation_rate"].as_u64().is_some_and(|r| r > 0) {
        return None;
    }
    let name = v["model_name"]
        .as_str()
        .or_else(|| v["device"]["name"].as_str())?
        .trim()
        .to_string();
    let serial = v["serial_number"].as_str().map(str::trim).filter(|s| !s.is_empty());
    let capacity = v["user_capacity"]["bytes"].as_u64();
    let mut wear = DiskWear {
        id: serial.map_or_else(|| name.clone(), |s| format!("{name} {s}")),
        name,
        capacity,
        percent_used: None,
        bytes_written: None,
        rated_bytes: None,
        power_on_hours: v["power_on_time"]["hours"].as_u64(),
    };
    let nvme = &v["nvme_smart_health_information_log"];
    if nvme.is_object() {
        wear.percent_used = nvme["percentage_used"].as_f64().map(|p| p as f32);
        // An NVMe "data unit" is 1000 blocks of 512 bytes.
        wear.bytes_written = nvme["data_units_written"].as_u64().and_then(|u| u.checked_mul(512_000));
        wear.power_on_hours = wear.power_on_hours.or_else(|| nvme["power_on_hours"].as_u64());
    } else if let Some(table) = v["ata_smart_attributes"]["table"].as_array() {
        let attr = |id: u64| table.iter().find(|a| a["id"].as_u64() == Some(id));
        // Normalised values count down from 100 (or 253) as the drive wears: used = 100 - value.
        wear.percent_used = [177u64, 231, 233, 202]
            .iter()
            .find_map(|id| attr(*id))
            .and_then(|a| a["value"].as_u64())
            .filter(|v| (1..=100).contains(v))
            .map(|v| (100 - v) as f32);
        wear.bytes_written = attr(241)
            .filter(|a| a["name"].as_str().is_some_and(|n| n.contains("LBAs_Written")))
            .and_then(|a| a["raw"]["value"].as_u64())
            .and_then(|lbas| lbas.checked_mul(512));
    }
    (wear.percent_used.is_some() || wear.bytes_written.is_some()).then_some(wear)
}

fn first_number(text: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{key}\" = ");
    let start = text.find(&pat)? + pat.len();
    let digits: String = text[start..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// `ioreg -rn AppleSmartBattery`: capacities in mAh (Apple silicon reports the live maximum in
/// `AppleRawMaxCapacity`; `MaxCapacity` there is a percentage). `None` when no battery is installed.
pub fn parse_mac_battery(text: &str) -> Option<BatteryCapacity> {
    if text.contains("\"BatteryInstalled\" = No") || !text.contains("AppleSmartBattery") {
        return None;
    }
    let design = first_number(text, "DesignCapacity").filter(|d| *d > 100)?;
    let full = first_number(text, "AppleRawMaxCapacity")
        .or_else(|| first_number(text, "NominalChargeCapacity"))
        .or_else(|| first_number(text, "MaxCapacity").filter(|m| *m > 100))?;
    Some(BatteryCapacity {
        id: "battery".into(),
        design: u32::try_from(design).ok()?,
        full: u32::try_from(full).ok()?,
        cycles: first_number(text, "CycleCount").and_then(|c| u32::try_from(c).ok()),
        rated_cycles: first_number(text, "DesignCycleCount9C")
            .filter(|c| *c > 0)
            .and_then(|c| u32::try_from(c).ok()),
    })
}

fn xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{tag}>"))?;
    Some(xml[start..start + end].trim().to_string())
}

/// `powercfg /batteryreport /xml`: the first battery's design and full-charge capacities (mWh) and cycle count.
pub fn parse_powercfg_battery(xml: &str) -> Option<BatteryCapacity> {
    let battery = xml_tag(xml, "Battery")?;
    let num = |t: &str| xml_tag(&battery, t).and_then(|s| s.parse::<u32>().ok());
    let design = num("DesignCapacity").filter(|d| *d > 0)?;
    let full = num("FullChargeCapacity")?;
    Some(BatteryCapacity {
        id: xml_tag(&battery, "Id").unwrap_or_else(|| "battery".into()),
        design,
        full,
        cycles: num("CycleCount").filter(|c| *c > 0),
        rated_cycles: None,
    })
}

/// A sample NVMe `smartctl` answer for the readers' tests (only the Unix tests use a fake `smartctl` script).
#[cfg(all(test, unix))]
pub(crate) fn tests_nvme() -> &'static str {
    tests::NVME
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE: &str = "Note: No thermal warning level has been recorded\nNote: No performance warning level has been recorded\nNote: No CPU power status has been recorded\n";

    #[test]
    fn pmset_with_nothing_recorded_means_no_limit() {
        let t = parse_pmset_therm(IDLE).unwrap();
        assert_eq!(
            t,
            OsThrottle {
                speed_limit_percent: None,
                thermal: Some(false),
                power: Some(false)
            }
        );
        assert!(!t.limited());
    }

    #[test]
    fn pmset_reports_the_speed_limit() {
        let text = "CPU_Scheduler_Limit\t= 100\nCPU_Available_CPUs\t= 8\nCPU_Speed_Limit \t= 70\n";
        let t = parse_pmset_therm(text).unwrap();
        assert_eq!((t.speed_limit_percent, t.thermal), (Some(70), Some(true)));
        let full = parse_pmset_therm("CPU_Speed_Limit = 100\nCPU_Scheduler_Limit = 100").unwrap();
        assert_eq!((full.speed_limit_percent, full.thermal), (Some(100), Some(false)));
        assert!(!full.limited());
        let warn = parse_pmset_therm("Thermal warning level: 2\nNote: No CPU power status has been recorded").unwrap();
        assert_eq!(warn.thermal, Some(true));
        let sched = parse_pmset_therm("CPU_Scheduler_Limit = 60").unwrap();
        assert_eq!(sched.speed_limit_percent, Some(60));
        assert!(parse_pmset_therm("nonsense").is_none());
        assert!(parse_pmset_therm("").is_none());
    }

    pub(super) const NVME: &str = r#"{"smartctl":{"messages":[]},"model_name":"APPLE SSD AP0512Z","serial_number":"ABC123","user_capacity":{"bytes":500277790720},
        "power_on_time":{"hours":812},"nvme_smart_health_information_log":{"percentage_used":3,"data_units_written":20000000,"power_on_hours":812}}"#;

    #[test]
    fn nvme_wear_is_read() {
        let w = parse_smartctl_ssd(NVME).unwrap();
        assert_eq!(w.name, "APPLE SSD AP0512Z");
        assert_eq!(w.id, "APPLE SSD AP0512Z ABC123");
        assert_eq!(w.percent_used, Some(3.0));
        assert_eq!(w.bytes_written, Some(20_000_000 * 512_000));
        assert_eq!((w.capacity, w.power_on_hours), (Some(500_277_790_720), Some(812)));
    }

    #[test]
    fn sata_wear_uses_the_normalised_attribute() {
        let sata = r#"{"model_name":"CT500MX500SSD1","rotation_rate":0,"ata_smart_attributes":{"table":[
            {"id":5,"name":"Reallocated","value":100,"raw":{"value":0}},
            {"id":202,"name":"Percent_Lifetime_Remain","value":96,"raw":{"value":4}},
            {"id":241,"name":"Total_LBAs_Written","value":100,"raw":{"value":1000000}}]}}"#;
        let w = parse_smartctl_ssd(sata).unwrap();
        assert_eq!(w.percent_used, Some(4.0));
        assert_eq!(w.bytes_written, Some(512_000_000));
        assert_eq!(w.id, "CT500MX500SSD1");
        // A vendor that names attribute 241 differently gives no byte count rather than a wrong one.
        let odd = sata.replace("Total_LBAs_Written", "Host_Writes_32MiB");
        assert_eq!(parse_smartctl_ssd(&odd).unwrap().bytes_written, None);
    }

    #[test]
    fn hard_disks_and_blank_drives_are_skipped() {
        assert!(parse_smartctl_ssd(r#"{"model_name":"WD Blue","rotation_rate":5400}"#).is_none());
        assert!(parse_smartctl_ssd(r#"{"model_name":"Mystery"}"#).is_none());
        assert!(parse_smartctl_ssd("not json").is_none());
        assert!(parse_smartctl_ssd(r#"{"rotation_rate":0,"ata_smart_attributes":{"table":[]}}"#).is_none());
        let out_of_range =
            r#"{"model_name":"x","ata_smart_attributes":{"table":[{"id":177,"name":"Wear","value":253,"raw":{"value":0}}]}}"#;
        assert!(
            parse_smartctl_ssd(out_of_range).is_none(),
            "253 is a vendor placeholder, not 153 % used"
        );
    }

    #[test]
    fn scans_and_refusals_are_understood() {
        assert_eq!(
            parse_smartctl_version("smartctl 7.5 2025-04-30 r5714 [x86_64-w64-mingw32-w10-22H2] (sf-7.5-1)\n").as_deref(),
            Some("7.5")
        );
        assert_eq!(parse_smartctl_version("smartctl 8.0 nightly").as_deref(), Some("8.0"));
        assert_eq!(parse_smartctl_version("nope"), None);
        let win = r#"{"smartctl":{"messages":[{"string":"Smartctl open device: /dev/sdb failed: Access is denied"}]}}"#;
        assert_eq!(smartctl_refusal_kind(win), Some(Refusal::Access), "Windows words it differently");
        let denied = r#"{"smartctl":{"messages":[{"string":"Smartctl open device: /dev/disk0 failed: Permission denied"}]}}"#;
        assert!(smartctl_refusal(denied).unwrap().contains("administrator"));
        let unsupported = r#"{"smartctl":{"messages":[{"string":"Unsupported device"}]}}"#;
        assert!(smartctl_refusal(unsupported).unwrap().contains("does not expose"));
        assert!(smartctl_refusal(NVME).is_none());
        assert!(smartctl_refusal("{").is_none());
    }

    #[test]
    fn mac_battery_capacities() {
        let text = r#"+-o AppleSmartBattery  <class AppleSmartBattery>
      "MaxCapacity" = 100
      "BatteryInstalled" = Yes
      "DesignCapacity" = 6249
      "AppleRawMaxCapacity" = 5712
      "CycleCount" = 211
      "DesignCycleCount9C" = 1000
      "BatteryData" = {"DesignCapacity"=1}"#;
        let b = parse_mac_battery(text).unwrap();
        assert_eq!((b.design, b.full, b.cycles, b.rated_cycles), (6249, 5712, Some(211), Some(1000)));
        assert!((b.health_percent().unwrap() - 91.4).abs() < 0.1);
        // No battery installed (a desktop Mac).
        assert!(parse_mac_battery("+-o AppleSmartBattery\n \"BatteryInstalled\" = No\n \"DesignCapacity\" = 0").is_none());
        assert!(parse_mac_battery("").is_none());
        // Older Intel Macs: MaxCapacity is the live mAh figure.
        let intel = "AppleSmartBattery\n \"BatteryInstalled\" = Yes\n \"DesignCapacity\" = 8440\n \"MaxCapacity\" = 7000\n";
        assert_eq!(parse_mac_battery(intel).unwrap().full, 7000);
    }

    #[test]
    fn windows_battery_report() {
        let xml = "<BatteryReport><Batteries><Battery><Id>DELL 1234</Id><DesignCapacity>57000</DesignCapacity>\
                   <FullChargeCapacity>51300</FullChargeCapacity><CycleCount>150</CycleCount></Battery></Batteries></BatteryReport>";
        let b = parse_powercfg_battery(xml).unwrap();
        assert_eq!(
            (b.id.as_str(), b.design, b.full, b.cycles),
            ("DELL 1234", 57_000, 51_300, Some(150))
        );
        let no_cycles = xml.replace("<CycleCount>150</CycleCount>", "<CycleCount>0</CycleCount>");
        assert_eq!(parse_powercfg_battery(&no_cycles).unwrap().cycles, None);
        assert!(parse_powercfg_battery("<BatteryReport></BatteryReport>").is_none());
        assert!(
            parse_powercfg_battery("<Battery><DesignCapacity>0</DesignCapacity><FullChargeCapacity>1</FullChargeCapacity></Battery>")
                .is_none()
        );
    }
}
