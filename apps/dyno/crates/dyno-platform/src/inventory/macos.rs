//! macOS: `system_profiler SPMemoryDataType -json`. Intel Macs list slots and modules; Apple silicon reports one
//! unified memory entry.

use super::clean;
use dyno_core::inventory::{FormFactor, MemoryArray, MemoryKind, MemoryLayout, MemoryModule, Unavailable, MAX_MODULES};
use serde_json::Value;

/// The largest output parsed.
const MAX_BYTES: usize = 512 * 1024;

/// "8 GB" / "16384 MB" / "512 KB" as bytes.
pub fn parse_size(s: &str) -> Option<u64> {
    let mut it = s.split_whitespace();
    let n: f64 = it.next()?.replace(',', ".").parse().ok()?;
    let mult = match it.next()?.to_ascii_uppercase().as_str() {
        "GB" => 1u64 << 30,
        "MB" => 1 << 20,
        "KB" => 1 << 10,
        "TB" => 1 << 40,
        _ => return None,
    };
    (n.is_finite() && n > 0.0).then_some((n * mult as f64) as u64)
}

/// "2667 MHz" as MT/s.
fn parse_speed(s: &str) -> Option<u32> {
    let n: u32 = s.split_whitespace().next()?.parse().ok()?;
    (n > 0 && n < 100_000).then_some(n)
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Parses the JSON into a layout; `total` is the OS-reported memory used for unified memory and for failures.
pub fn parse(json: &str, total: u64) -> MemoryLayout {
    let fail = |why: &str| MemoryLayout::TotalOnly {
        total,
        reason: Unavailable::failed(why),
    };
    if json.len() > MAX_BYTES {
        return fail("The memory report was too large to read.");
    }
    let Ok(root) = serde_json::from_str::<Value>(json) else {
        return fail("The memory report could not be understood.");
    };
    let Some(top) = root.get("SPMemoryDataType").and_then(Value::as_array).and_then(|a| a.first()) else {
        return fail("This Mac reported no memory details.");
    };
    // Apple silicon: the entry itself carries the type, manufacturer and size.
    if let Some(items) = top.get("SPMemoryDataType").and_then(Value::as_array) {
        let mut slots = 0u32;
        let mut modules = Vec::new();
        for it in items.iter().take(MAX_MODULES) {
            slots += 1;
            let size = str_of(it, "dimm_size").and_then(parse_size);
            let Some(capacity) = size else { continue }; // "Empty"
            let slot = str_of(it, "_name").and_then(clean).unwrap_or_else(|| format!("Slot {slots}"));
            let (bank, slot) = match slot.split_once('/') {
                Some((s, b)) => (clean(b), s.trim().to_string()),
                None => (None, slot),
            };
            modules.push(MemoryModule {
                slot,
                bank,
                capacity_bytes: capacity,
                speed_mts: str_of(it, "dimm_speed").and_then(parse_speed),
                configured_speed_mts: None,
                kind: str_of(it, "dimm_type").map_or(MemoryKind::Unknown, MemoryKind::from_name),
                form_factor: FormFactor::Unknown,
                manufacturer: str_of(it, "dimm_manufacturer").and_then(clean),
                part_number: str_of(it, "dimm_part_number").and_then(clean),
            });
        }
        return if modules.is_empty() {
            fail("This Mac reported no memory modules.")
        } else {
            MemoryLayout::Slots(
                MemoryArray {
                    slots,
                    max_capacity_bytes: None,
                    modules,
                }
                .bounded(),
            )
        };
    }
    let size = top
        .get("SPMemoryDataType")
        .and_then(Value::as_str)
        .and_then(parse_size)
        .unwrap_or(total);
    MemoryLayout::unified(
        size,
        str_of(top, "dimm_type").map_or(MemoryKind::Unknown, MemoryKind::from_name),
        str_of(top, "dimm_manufacturer").and_then(clean),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTEL: &str = r#"{"SPMemoryDataType":[{"_name":"memory","SPMemoryDataType":[
      {"_name":"DIMM0/BANK 0","dimm_manufacturer":"Acme","dimm_part_number":"AC-1","dimm_size":"8 GB","dimm_speed":"2667 MHz","dimm_status":"ok","dimm_type":"DDR4"},
      {"_name":"DIMM1/BANK 1","dimm_size":"Empty","dimm_type":"Empty"}]}]}"#;
    const SILICON: &str =
        r#"{"SPMemoryDataType":[{"_name":"memory","dimm_manufacturer":"Acme","dimm_type":"LPDDR5","SPMemoryDataType":"16 GB"}]}"#;

    #[test]
    fn intel_slots() {
        let MemoryLayout::Slots(a) = parse(INTEL, 1) else { panic!("slots") };
        assert_eq!((a.slots, a.modules.len()), (2, 1));
        let m = &a.modules[0];
        assert_eq!(
            (m.slot.as_str(), m.bank.as_deref(), m.capacity_bytes, m.speed_mts, m.kind),
            ("DIMM0", Some("BANK 0"), 8 << 30, Some(2667), MemoryKind::Ddr4)
        );
        assert_eq!(m.manufacturer.as_deref(), Some("Acme"));
        assert_eq!(a.free_slots(), 1);
    }

    #[test]
    fn apple_silicon_is_unified() {
        match parse(SILICON, 1) {
            MemoryLayout::Unified {
                total,
                kind,
                manufacturer,
                upgradeable,
            } => {
                assert_eq!(
                    (total, kind, manufacturer.as_deref(), upgradeable),
                    (16 << 30, MemoryKind::Lpddr5, Some("Acme"), false)
                );
            }
            other => panic!("{other:?}"),
        }
        // No size in the entry: fall back to the OS total.
        assert_eq!(parse(r#"{"SPMemoryDataType":[{"_name":"memory"}]}"#, 9).total(), 9);
    }

    #[test]
    fn junk_and_sizes() {
        for j in [
            "",
            "{",
            "[]",
            "{}",
            r#"{"SPMemoryDataType":[]}"#,
            r#"{"SPMemoryDataType":[{"SPMemoryDataType":[]}]}"#,
            r#"{"SPMemoryDataType":[{"SPMemoryDataType":[{"dimm_size":"x"}]}]}"#,
        ] {
            assert!(matches!(parse(j, 3), MemoryLayout::TotalOnly { total: 3, .. }), "{j}");
        }
        assert!(matches!(parse(&" ".repeat(MAX_BYTES + 1), 3), MemoryLayout::TotalOnly { .. }));
        assert_eq!(parse_size("16384 MB"), Some(16 << 30));
        assert_eq!(parse_size("512 KB"), Some(512 << 10));
        assert_eq!(parse_size("1 TB"), Some(1 << 40));
        assert_eq!(parse_size("1,5 GB"), Some(3 << 29));
        assert_eq!(parse_size("8"), None);
        assert_eq!(parse_size("8 XB"), None);
        assert_eq!(parse_size("-8 GB"), None);
        assert_eq!(parse_size("nan GB"), None);
        assert_eq!(parse_speed("0 MHz"), None);
        assert_eq!(parse_speed("fast"), None);
        let no_name = r#"{"SPMemoryDataType":[{"SPMemoryDataType":[{"dimm_size":"4 GB"}]}]}"#;
        let MemoryLayout::Slots(a) = parse(no_name, 1) else {
            panic!("slots")
        };
        assert_eq!(a.modules[0].slot, "Slot 1");
    }
}
