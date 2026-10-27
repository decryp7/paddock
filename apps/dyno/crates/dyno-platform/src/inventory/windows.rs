//! Windows: `Get-CimInstance Win32_PhysicalMemory` / `Win32_PhysicalMemoryArray` rows as JSON (no administrator).

use super::clean;
use dyno_core::inventory::{FormFactor, MemoryArray, MemoryKind, MemoryLayout, MemoryModule, Unavailable, MAX_MODULES};
use serde_json::Value;

/// The largest output parsed.
const MAX_BYTES: usize = 256 * 1024;

/// A JSON object or array of objects as a list of rows (PowerShell emits a bare object for a single row).
fn rows(text: &str) -> Vec<Value> {
    if text.len() > MAX_BYTES {
        return Vec::new();
    }
    match serde_json::from_str::<Value>(text.trim().trim_start_matches('\u{feff}')) {
        Ok(Value::Array(a)) => a.into_iter().filter(Value::is_object).collect(),
        Ok(v @ Value::Object(_)) => vec![v],
        _ => Vec::new(),
    }
}

/// A number that CIM may serialise as a number or a string.
fn num(v: &Value, key: &str) -> Option<u64> {
    match v.get(key)? {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).and_then(clean)
}

fn mts(v: &Value, key: &str) -> Option<u32> {
    num(v, key).filter(|n| *n > 0 && *n < 100_000).map(|n| n as u32)
}

/// Parses the two CIM outputs into a layout. `array_json` may be empty when that query failed. `total` is used when
/// no module could be read.
pub fn parse(modules_json: &str, array_json: &str, total: u64) -> MemoryLayout {
    let mut modules = Vec::new();
    for r in rows(modules_json).iter().take(MAX_MODULES) {
        let Some(capacity) = num(r, "Capacity").filter(|c| *c > 0) else {
            continue;
        };
        let slot = text(r, "DeviceLocator")
            .or_else(|| text(r, "BankLabel"))
            .unwrap_or_else(|| format!("Slot {}", modules.len() + 1));
        modules.push(MemoryModule {
            slot,
            bank: text(r, "BankLabel"),
            capacity_bytes: capacity,
            speed_mts: mts(r, "Speed"),
            configured_speed_mts: mts(r, "ConfiguredClockSpeed"),
            kind: num(r, "SMBIOSMemoryType").map_or(MemoryKind::Unknown, |c| MemoryKind::from_smbios(c as u32)),
            form_factor: num(r, "FormFactor").map_or(FormFactor::Unknown, |c| FormFactor::from_smbios(c as u32)),
            manufacturer: text(r, "Manufacturer"),
            part_number: text(r, "PartNumber"),
        });
    }
    if modules.is_empty() {
        return MemoryLayout::TotalOnly {
            total,
            reason: Unavailable::failed("Windows did not report any memory modules (common in virtual machines)."),
        };
    }
    let (mut slots, mut max) = (0u64, None);
    for r in rows(array_json) {
        slots += num(&r, "MemoryDevices").unwrap_or(0);
        // MaxCapacityEx is in KB and wins over MaxCapacity (also KB, but 32-bit).
        let kb = num(&r, "MaxCapacityEx").filter(|k| *k > 0).or_else(|| num(&r, "MaxCapacity"));
        max = kb.map(|k| k.saturating_mul(1024)).filter(|b| *b > 0).or(max);
    }
    MemoryLayout::Slots(
        MemoryArray {
            slots: slots.min(u64::from(u32::MAX)) as u32,
            max_capacity_bytes: max,
            modules,
        }
        .bounded(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULES: &str = r#"[
      {"BankLabel":"BANK 0","DeviceLocator":"ChannelA-DIMM0","Capacity":"8589934592","Speed":3200,"ConfiguredClockSpeed":2666,"SMBIOSMemoryType":26,"Manufacturer":"Acme Memory","PartNumber":"AC8G3200 ","FormFactor":8},
      {"BankLabel":"BANK 2","DeviceLocator":"ChannelB-DIMM0","Capacity":8589934592,"Speed":3200,"ConfiguredClockSpeed":2666,"SMBIOSMemoryType":26,"Manufacturer":"Unknown","PartNumber":"To Be Filled By O.E.M.","FormFactor":8}
    ]"#;
    const ARRAY: &str = r#"{"MemoryDevices":4,"MaxCapacity":67108864,"MaxCapacityEx":0}"#;

    #[test]
    fn two_sticks() {
        let MemoryLayout::Slots(a) = parse(MODULES, ARRAY, 1) else {
            panic!("slots")
        };
        assert_eq!((a.slots, a.max_capacity_bytes, a.modules.len()), (4, Some(64 << 30), 2));
        let m = &a.modules[0];
        assert_eq!(
            (m.slot.as_str(), m.bank.as_deref(), m.capacity_bytes),
            ("ChannelA-DIMM0", Some("BANK 0"), 8 << 30)
        );
        assert_eq!(
            (m.speed_mts, m.configured_speed_mts, m.kind, m.form_factor),
            (Some(3200), Some(2666), MemoryKind::Ddr4, FormFactor::Dimm)
        );
        assert_eq!(
            (m.manufacturer.as_deref(), m.part_number.as_deref()),
            (Some("Acme Memory"), Some("AC8G3200"))
        );
        assert_eq!(a.modules[1].manufacturer, None);
        assert_eq!(a.modules[1].part_number, None);
    }

    #[test]
    fn single_object_and_extended_capacity() {
        let one = r#"{"DeviceLocator":"DIMM1","Capacity":4294967296}"#;
        let arr = r#"[{"MemoryDevices":2,"MaxCapacity":0,"MaxCapacityEx":134217728}]"#;
        let MemoryLayout::Slots(a) = parse(one, arr, 1) else {
            panic!("slots")
        };
        assert_eq!(a.max_capacity_bytes, Some(128 << 30));
        assert_eq!(a.modules[0].kind, MemoryKind::Unknown);
        assert_eq!(a.modules[0].speed_mts, None);
        let MemoryLayout::Slots(b) = parse(r#"{"Capacity":1048576}"#, "", 1) else {
            panic!("slots")
        };
        assert_eq!((b.modules[0].slot.as_str(), b.slots, b.max_capacity_bytes), ("Slot 1", 0, None));
    }

    #[test]
    fn junk_never_panics() {
        for j in [
            "",
            "null",
            "[]",
            "{",
            "[1,2,3]",
            r#"{"Capacity":-5}"#,
            r#"{"Capacity":"x"}"#,
            r#"[{"Capacity":0}]"#,
            "\u{feff}[]",
        ] {
            assert!(matches!(parse(j, j, 7), MemoryLayout::TotalOnly { total: 7, .. }), "{j}");
        }
        let huge = format!("[{}]", "{\"Capacity\":1},".repeat(100_000));
        assert!(matches!(parse(&huge, "", 1), MemoryLayout::TotalOnly { .. }));
        let many = format!("[{}]", vec![r#"{"Capacity":1}"#; 200].join(","));
        let MemoryLayout::Slots(a) = parse(&many, "garbage", 1) else {
            panic!("slots")
        };
        assert_eq!(a.modules.len(), MAX_MODULES);
        let f = r#"{"Capacity":1.5e9,"Speed":1e9}"#;
        let MemoryLayout::Slots(a) = parse(f, "", 1) else { panic!("slots") };
        assert_eq!((a.modules[0].capacity_bytes, a.modules[0].speed_mts), (1_500_000_000, None));
    }
}
