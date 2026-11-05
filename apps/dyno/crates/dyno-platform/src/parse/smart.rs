//! `smartctl --scan-open -j` and `smartctl -a -j <device>` for the helper's `Smart` request (ADR 0027 section 5 and 7).
//!
//! smartctl's exit status is a bit mask (smartctl(8)): bit 0 command line error, bit 1 device open failed, bit 2 a SMART
//! command failed, bits 3 and up findings about the disk. Only bits 0 and 1 mean "no data". Bit 2 is a good read when a health
//! log was returned (WP-0: Apple silicon NVMe exits 4 because the controller refuses the error-information log page).
//! Serial numbers and WWNs are never copied out: [`SmartDisk`] has no field for them.

use super::hardware::{parse_smartctl_ssd, smartctl_refusal};
use dyno_core::smart::{SmartDisk, MAX_SMART_TEXT};
use serde_json::Value;

/// A device from the scan, validated so it is safe as one argument of `smartctl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanDevice {
    /// The name smartctl printed.
    pub name: String,
    /// The `-d` type, when the scan gave one.
    pub kind: Option<String>,
}

/// The longest device name taken from a scan. Apple silicon names an NVMe controller by its IOService path (150 to 400 characters
/// seen), which is longer than the 128 characters shown to people ([`MAX_SMART_TEXT`]). The name is one argument of a process
/// started without a shell, so its length and `/`, `@` and `:` are harmless; it still may not look like an option or hold control
/// characters.
const MAX_SCAN_NAME: usize = 1024;

fn name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= MAX_SCAN_NAME && !n.starts_with('-') && !n.chars().any(char::is_control)
}

fn kind_ok(k: &str) -> bool {
    !k.is_empty() && k.len() <= 16 && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b',' || b == b'+')
}

/// The devices of `smartctl --scan-open -j`, at most `max`; entries with odd names or types are left out.
pub fn parse_scan(json: &str, max: usize) -> Vec<ScanDevice> {
    scan_devices(json, max).0
}

/// [`parse_scan`] and how many entries were left out as odd (for the log line).
pub fn scan_devices(json: &str, max: usize) -> (Vec<ScanDevice>, usize) {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return (Vec::new(), 0);
    };
    let Some(list) = v["devices"].as_array() else {
        return (Vec::new(), 0);
    };
    let mut rejected = 0;
    let devices = list
        .iter()
        .filter_map(|d| {
            let parsed = (|| {
                let name = d["name"].as_str().filter(|n| name_ok(n))?.to_string();
                let kind = match d["type"].as_str() {
                    Some(k) if kind_ok(k) => Some(k.to_string()),
                    Some(_) => return None,
                    None => None,
                };
                Some(ScanDevice { name, kind })
            })();
            if parsed.is_none() {
                rejected += 1;
            }
            parsed
        })
        .take(max)
        .collect();
    (devices, rejected)
}

/// Why a disk could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartParseError {
    /// The output was not smartctl's JSON.
    NotJson,
    /// smartctl could not open the device or took a bad command line (exit bits 0 and 1); the words say which.
    NoAccess(String),
    /// Exit bit 2 without a health log, or no SMART data at all.
    NoData(String),
}

impl std::fmt::Display for SmartParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SmartParseError::NotJson => f.write_str("smartctl printed something other than its JSON."),
            SmartParseError::NoAccess(m) | SmartParseError::NoData(m) => f.write_str(m),
        }
    }
}

/// One disk from `smartctl -a -j`. `exit` is the process exit code, used when the JSON does not carry `exit_status`.
pub fn parse_disk(device: &str, json: &str, exit: Option<i32>) -> Result<SmartDisk, SmartParseError> {
    let v: Value = serde_json::from_str(json).map_err(|_| SmartParseError::NotJson)?;
    let status = v["smartctl"]["exit_status"].as_u64().or(exit.map(|e| e as u64)).unwrap_or(0);
    if status & 0b11 != 0 {
        return Err(SmartParseError::NoAccess(smartctl_refusal(json).unwrap_or_else(|| {
            "smartctl could not open this disk. Run it from an administrator account or check the drive.".into()
        })));
    }
    let nvme = &v["nvme_smart_health_information_log"];
    let has_health = nvme.is_object() || v["ata_smart_attributes"]["table"].is_array();
    if status & 0b100 != 0 && !has_health {
        return Err(SmartParseError::NoData(
            "A SMART command failed and the drive returned no health log.".into(),
        ));
    }
    let passed = v["smart_status"]["passed"].as_bool();
    if !has_health && passed.is_none() {
        return Err(SmartParseError::NoData(
            smartctl_refusal(json).unwrap_or_else(|| "This drive does not expose SMART data to apps.".into()),
        ));
    }
    let text = |s: &str| s.trim().chars().take(MAX_SMART_TEXT).collect::<String>();
    // The wear reading comes from the one shared parser (rule 55), so the helper's and the agent's readings cannot drift.
    let wear = parse_smartctl_ssd(json);
    let model = v["model_name"]
        .as_str()
        .map(text)
        .filter(|m| !m.is_empty())
        .or_else(|| wear.as_ref().map(|w| text(&w.name)))
        .unwrap_or_else(|| "Unknown disk".into());
    let int = |x: &Value| x.as_i64();
    let temperature_c = int(&v["temperature"]["current"])
        .or_else(|| int(&nvme["temperature"]))
        .and_then(|t| i32::try_from(t).ok())
        .filter(|t| (-60..=200).contains(t));
    // Normalised ATA wear attributes are interpreted by the shared parser; its id (which carries a serial) is dropped.
    let percent_used = nvme["percentage_used"]
        .as_f64()
        .or_else(|| parse_smartctl_ssd(json).and_then(|w| w.percent_used).map(f64::from))
        .filter(|p| *p >= 0.0)
        .map(|p| p.round().min(f64::from(u16::MAX)) as u16);
    Ok(SmartDisk {
        device: text(device),
        model,
        capacity: v["user_capacity"]["bytes"].as_u64(),
        temperature_c,
        percent_used,
        media_errors: nvme["media_errors"].as_u64(),
        power_on_hours: v["power_on_time"]["hours"].as_u64().or_else(|| nvme["power_on_hours"].as_u64()),
        unsafe_shutdowns: nvme["unsafe_shutdowns"].as_u64(),
        passed,
        wear_id: wear.as_ref().map(|w| w.id.clone()),
        bytes_written: wear.as_ref().and_then(|w| w.bytes_written),
        rated_bytes: wear.as_ref().and_then(|w| w.rated_bytes),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Invented, same shape as an Apple silicon NVMe read (WP-0): exit 4, health log present, a serial that must not survive.
    const MAC_NVME: &str = r#"{"smartctl":{"exit_status":4,"messages":[{"string":"Read 1 entries from Error Information Log failed: GetLogPage failed","severity":"error"}]},
"model_name":"APPLE SSD EXAMPLE","serial_number":"ZZ99SERIAL","wwn":{"naa":5,"oui":1,"id":2},"user_capacity":{"bytes":500000000000},
"smart_status":{"passed":true},"power_on_time":{"hours":321},
"nvme_smart_health_information_log":{"temperature":38,"percentage_used":3,"media_errors":0,"power_on_hours":321,"unsafe_shutdowns":7}}"#;

    #[test]
    fn exit_bit_2_with_a_health_log_is_a_good_read_and_no_serial_survives() {
        let d = parse_disk("disk0", MAC_NVME, Some(4)).unwrap();
        assert_eq!(
            (
                d.model.as_str(),
                d.temperature_c,
                d.percent_used,
                d.media_errors,
                d.unsafe_shutdowns,
                d.passed
            ),
            ("APPLE SSD EXAMPLE", Some(38), Some(3), Some(0), Some(7), Some(true))
        );
        assert_eq!((d.capacity, d.power_on_hours), (Some(500_000_000_000), Some(321)));
        // No WWN ever; the serial only inside the wear id (the history key).
        let all = format!("{d:?}");
        assert!(!all.contains("naa"), "{all}");
        assert_eq!(d.wear_id.as_deref(), Some("APPLE SSD EXAMPLE ZZ99SERIAL"));
    }

    #[test]
    fn the_json_exit_status_wins_over_the_process_code() {
        assert!(parse_disk("d", MAC_NVME, Some(0)).is_ok());
        let no_status = MAC_NVME.replace("\"exit_status\":4", "\"x\":0");
        assert!(parse_disk("d", &no_status, Some(4)).is_ok());
        assert!(matches!(parse_disk("d", &no_status, Some(2)), Err(SmartParseError::NoAccess(_))));
    }

    #[test]
    fn bits_0_and_1_mean_no_access_and_bit_2_alone_means_no_data() {
        let json = r#"{"smartctl":{"exit_status":2,"messages":[{"string":"Smartctl open device: /dev/sda failed: Permission denied"}]}}"#;
        match parse_disk("sda", json, None) {
            Err(SmartParseError::NoAccess(m)) => assert!(m.contains("administrator"), "{m}"),
            other => panic!("{other:?}"),
        }
        let bit2 = r#"{"smartctl":{"exit_status":4},"model_name":"X"}"#;
        assert!(matches!(parse_disk("sda", bit2, None), Err(SmartParseError::NoData(_))));
        assert!(matches!(parse_disk("sda", "not json", None), Err(SmartParseError::NotJson)));
        // Findings about the disk (bits 3 and up) are readings, not failures.
        let failing = MAC_NVME
            .replace("\"exit_status\":4", "\"exit_status\":8")
            .replace("\"passed\":true", "\"passed\":false");
        assert_eq!(parse_disk("d", &failing, None).unwrap().passed, Some(false));
    }

    #[test]
    fn sata_wear_comes_from_the_shared_attribute_rules() {
        let ata = r#"{"smartctl":{"exit_status":0},"model_name":"SATA EXAMPLE","serial_number":"S1","smart_status":{"passed":true},
"temperature":{"current":31},"power_on_time":{"hours":9},"ata_smart_attributes":{"table":[{"id":177,"name":"Wear_Leveling_Count","value":97,"raw":{"value":3}}]}}"#;
        let d = parse_disk("sda", ata, Some(0)).unwrap();
        assert_eq!((d.percent_used, d.temperature_c, d.media_errors), (Some(3), Some(31), None));
        assert_eq!(d.wear_id.as_deref(), Some("SATA EXAMPLE S1"));
    }

    #[test]
    fn the_scan_keeps_safe_names_and_types_up_to_the_cap() {
        let scan = r#"{"devices":[{"name":"/dev/sda","type":"nvme"},{"name":"-oops","type":"sat"},{"name":"/dev/sdb","type":"sat;rm"},
{"name":"IOService:/AppleARMPE/arm-io/AppleANS3CGv2Controller/NS_01@1","type":"nvme"},{"name":"/dev/sdc"},{"name":""}]}"#;
        let all = parse_scan(scan, 32);
        let names: Vec<_> = all.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "/dev/sda",
                "IOService:/AppleARMPE/arm-io/AppleANS3CGv2Controller/NS_01@1",
                "/dev/sdc"
            ]
        );
        assert_eq!(all[2].kind, None);
        assert_eq!(parse_scan(scan, 1).len(), 1);
        assert!(parse_scan("garbage", 32).is_empty());
    }

    /// Captured shape (invented values): Apple silicon lists one NVMe controller as several IOService paths, 150 to 370 characters long.
    pub(crate) fn apple_silicon_scan() -> String {
        let long = |ans: &str, tail: &str| {
            format!("IOService:/AppleARMPE/arm-io@10F00000/AppleH16GFamilyIO/{ans}@A000000/AppleANS3NVMeController/{tail}")
        };
        let a = format!("{}/{}", long("ans", "NS_01@1"), "IOBlockStorageServices/".repeat(2));
        let b = format!("{}/{}", long("ans", "NS_02@2"), "IOBlockStorageServices/".repeat(10));
        let c = format!("{}/{}", long("ans", "NS_03@3"), "IOBlockStorageServices/".repeat(10));
        format!(
            r#"{{"json_format_version":[1,0],"smartctl":{{"exit_status":0}},"devices":[{{"name":"{a}","info_name":"{a}","type":"nvme","protocol":"NVMe"}},{{"name":"{b}","info_name":"{b}","type":"nvme","protocol":"NVMe"}},{{"name":"{c}","info_name":"{c}","type":"nvme","protocol":"NVMe"}}]}}"#
        )
    }

    /// Captured shape (invented values): one internal NVMe controller and two USB drives, which smartctl also lists as `nvme`.
    #[cfg(unix)]
    pub(crate) fn internal_and_usb_scan() -> String {
        let internal = "IOService:/AppleARMPE/arm-io@10F00000/AppleH16GFamilyIO/ans@A000000/AppleANS3NVMeController/NS_01@1";
        let usb = |n: u8| {
            format!("IOService:/AppleARMPE/arm-io@10F00000/AppleT8140USBXHCI/usb-drd{n}@B000000/AppleUSBXHCI/usb-device@1/IOUSBHostInterface@0/IOBlockStorageServices")
        };
        let entry = |name: &str| format!(r#"{{"name":"{name}","info_name":"{name}","type":"nvme","protocol":"NVMe"}}"#);
        format!(
            r#"{{"smartctl":{{"exit_status":0}},"devices":[{},{},{}]}}"#,
            entry(internal),
            entry(&usb(1)),
            entry(&usb(2))
        )
    }

    #[test]
    fn apple_silicon_scan_names_over_128_characters_are_kept() {
        let scan = apple_silicon_scan();
        let (devices, rejected) = scan_devices(&scan, 16);
        assert_eq!(
            (devices.len(), rejected),
            (3, 0),
            "long IOService names are valid arguments, got {devices:?}"
        );
        assert!(devices.iter().any(|d| d.name.len() > 300));
        assert!(devices.iter().all(|d| d.kind.as_deref() == Some("nvme")));
        // Still strict: absurd lengths, leading dashes and control characters are rejected and counted.
        let bad = format!(
            r#"{{"devices":[{{"name":"{}"}},{{"name":"-x"}},{{"name":"a\nb"}},{{"name":"/dev/disk0"}}]}}"#,
            "x".repeat(5000)
        );
        let (devices, rejected) = scan_devices(&bad, 16);
        assert_eq!((devices.len(), rejected), (1, 3));
    }
}

#[cfg(test)]
mod same_reading {
    use super::*;

    /// Invented smartctl JSON (never a user's): an NVMe disk with a serial, bytes written and a power-on count.
    const NVME: &str = r#"{"smartctl":{"exit_status":4},"model_name":"EXAMPLE NVME 1TB","serial_number":"EX123456","user_capacity":{"bytes":1000204886016},
"smart_status":{"passed":true},"power_on_time":{"hours":4321},
"nvme_smart_health_information_log":{"temperature":41,"percentage_used":7,"data_units_written":166000000,"media_errors":0,"power_on_hours":4321,"unsafe_shutdowns":5}}"#;
    /// A SATA disk with the wear attributes and no model name.
    const SATA: &str = r#"{"smartctl":{"exit_status":0},"serial_number":"EXS1","device":{"name":"/dev/sdb"},"smart_status":{"passed":true},
"power_on_time":{"hours":9},"ata_smart_attributes":{"table":[{"id":177,"name":"Wear_Leveling_Count","value":97,"raw":{"value":3}}]}}"#;

    #[test]
    fn the_helper_path_and_the_agents_own_path_make_the_same_wear_reading_with_the_same_id() {
        for json in [NVME, SATA] {
            let local = parse_smartctl_ssd(json).expect("an SSD");
            let through_helper = parse_disk("/dev/x", json, None).unwrap().to_wear().expect("an SSD");
            assert_eq!(through_helper, local, "one reading, whichever path made it");
            assert_eq!(through_helper.id, local.id, "the history continues across both paths");
        }
        let nvme = parse_disk("/dev/x", NVME, None).unwrap().to_wear().unwrap();
        assert_eq!(nvme.id, "EXAMPLE NVME 1TB EX123456");
        assert!(nvme.bytes_written.is_some(), "Written so far survives the helper path");
    }

    #[test]
    fn a_hard_disk_has_no_wear_reading_on_either_path() {
        let hdd = r#"{"smartctl":{"exit_status":0},"model_name":"EXAMPLE HDD","rotation_rate":7200,"smart_status":{"passed":true},"ata_smart_attributes":{"table":[]}}"#;
        assert!(parse_smartctl_ssd(hdd).is_none());
        assert!(parse_disk("/dev/y", hdd, None).unwrap().to_wear().is_none());
    }
}
