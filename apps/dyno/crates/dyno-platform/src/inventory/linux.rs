//! Linux: the SMBIOS table at `/sys/firmware/dmi/tables/DMI` (root-only on most systems). Type 16 is the memory
//! array, type 17 a memory device.

use super::clean;
use dyno_core::inventory::{FormFactor, MemoryArray, MemoryKind, MemoryLayout, MemoryModule, Unavailable, MAX_MODULES};

/// The most table bytes read.
pub const MAX_TABLE_BYTES: usize = 1 << 20;
/// The most structures walked.
const MAX_STRUCTURES: usize = 4096;

struct Structure<'a> {
    kind: u8,
    data: &'a [u8],
    strings: Vec<&'a [u8]>,
}

impl Structure<'_> {
    fn u8(&self, o: usize) -> Option<u8> {
        self.data.get(o).copied()
    }
    fn u16(&self, o: usize) -> Option<u16> {
        Some(u16::from_le_bytes(self.data.get(o..o + 2)?.try_into().ok()?))
    }
    fn u32(&self, o: usize) -> Option<u32> {
        Some(u32::from_le_bytes(self.data.get(o..o + 4)?.try_into().ok()?))
    }
    fn u64(&self, o: usize) -> Option<u64> {
        Some(u64::from_le_bytes(self.data.get(o..o + 8)?.try_into().ok()?))
    }
    /// The string whose 1-based index is stored at `o`.
    fn string(&self, o: usize) -> Option<String> {
        let i = usize::from(self.u8(o)?);
        let raw = self.strings.get(i.checked_sub(1)?)?;
        clean(&String::from_utf8_lossy(raw))
    }
}

/// Walks the structure table; stops quietly at the first malformed structure.
fn structures(bytes: &[u8]) -> Vec<Structure<'_>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while out.len() < MAX_STRUCTURES {
        let Some(head) = bytes.get(pos..pos + 4) else { break };
        let (kind, len) = (head[0], usize::from(head[1]));
        if len < 4 || kind == 127 {
            break;
        }
        let Some(data) = bytes.get(pos..pos + len) else { break };
        // The string set follows: NUL-terminated strings, ended by an empty one.
        let mut p = pos + len;
        let mut strings = Vec::new();
        loop {
            let Some(rest) = bytes.get(p..) else { return out };
            let Some(end) = rest.iter().position(|b| *b == 0) else { return out };
            if end == 0 {
                p += 1 + usize::from(strings.is_empty()); // an empty set is two NULs
                break;
            }
            strings.push(&rest[..end]);
            p += end + 1;
        }
        out.push(Structure { kind, data, strings });
        pos = p;
    }
    out
}

fn device(s: &Structure<'_>) -> Option<MemoryModule> {
    let size = match s.u16(0x0c)? {
        0 | 0xffff => return None, // empty slot / unknown
        0x7fff => u64::from(s.u32(0x1c)? & 0x7fff_ffff) << 20,
        v if v & 0x8000 != 0 => u64::from(v & 0x7fff) << 10,
        v => u64::from(v) << 20,
    };
    let mts = |v: Option<u16>| v.filter(|n| *n > 0 && *n != 0xffff).map(u32::from);
    Some(MemoryModule {
        slot: s.string(0x10).unwrap_or_else(|| "Slot".into()),
        bank: s.string(0x11),
        capacity_bytes: size,
        speed_mts: mts(s.u16(0x15)),
        configured_speed_mts: mts(s.u16(0x20)),
        kind: s.u8(0x12).map_or(MemoryKind::Unknown, |c| MemoryKind::from_smbios(u32::from(c))),
        form_factor: s.u8(0x0e).map_or(FormFactor::Unknown, |c| FormFactor::from_smbios(u32::from(c))),
        manufacturer: s.string(0x17),
        part_number: s.string(0x1a),
    })
}

/// Parses the raw table into a layout; `total` is the OS-reported memory used when no device is found.
pub fn parse(bytes: &[u8], total: u64) -> MemoryLayout {
    let bytes = &bytes[..bytes.len().min(MAX_TABLE_BYTES)];
    let all = structures(bytes);
    let mut slots = 0u32;
    let mut max = None;
    let mut modules = Vec::new();
    for s in &all {
        match s.kind {
            16 => {
                slots = slots.saturating_add(u32::from(s.u16(0x0d).unwrap_or(0)));
                let kb = match s.u32(0x07) {
                    Some(0x8000_0000) => s.u64(0x0f).map(|b| b >> 10),
                    other => other.map(u64::from),
                };
                max = kb.map(|k| k.saturating_mul(1024)).filter(|b| *b > 0).or(max);
            }
            17 if modules.len() < MAX_MODULES => modules.extend(device(s)),
            _ => {}
        }
    }
    if modules.is_empty() {
        return MemoryLayout::TotalOnly {
            total,
            reason: Unavailable::failed("The firmware lists no memory modules (common in virtual machines)."),
        };
    }
    MemoryLayout::Slots(
        MemoryArray {
            slots,
            max_capacity_bytes: max,
            modules,
        }
        .bounded(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structure(kind: u8, len: usize, set: &dyn Fn(&mut Vec<u8>), strings: &[&str]) -> Vec<u8> {
        let mut d = vec![0u8; len];
        d[0] = kind;
        d[1] = len as u8;
        set(&mut d);
        for s in strings {
            d.extend_from_slice(s.as_bytes());
            d.push(0);
        }
        if strings.is_empty() {
            d.push(0);
        }
        d.push(0);
        d
    }

    fn put16(d: &mut [u8], o: usize, v: u16) {
        d[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn table() -> Vec<u8> {
        let mut t = structure(
            16,
            23,
            &|d| {
                d[0x07..0x0b].copy_from_slice(&(64u32 << 20).to_le_bytes());
                put16(d, 0x0d, 4);
            },
            &[],
        );
        t.extend(structure(
            17,
            0x28,
            &|d| {
                put16(d, 0x0c, 8192);
                d[0x0e] = 8;
                d[0x10] = 1;
                d[0x11] = 2;
                d[0x12] = 26;
                put16(d, 0x15, 3200);
                d[0x17] = 3;
                d[0x1a] = 4;
                put16(d, 0x20, 2666);
            },
            &["ChannelA-DIMM0", "BANK 0", "Acme", "AC-1"],
        ));
        t.extend(structure(17, 0x28, &|d| put16(d, 0x0c, 0), &["ChannelB-DIMM0"]));
        t.extend(structure(127, 4, &|_| {}, &[]));
        t
    }

    #[test]
    fn parses_array_and_devices() {
        let MemoryLayout::Slots(a) = parse(&table(), 1) else {
            panic!("slots")
        };
        assert_eq!((a.slots, a.max_capacity_bytes, a.modules.len()), (4, Some(64 << 30), 1));
        let m = &a.modules[0];
        assert_eq!(
            (m.slot.as_str(), m.bank.as_deref(), m.capacity_bytes),
            ("ChannelA-DIMM0", Some("BANK 0"), 8 << 30)
        );
        assert_eq!(
            (m.speed_mts, m.configured_speed_mts, m.kind, m.form_factor),
            (Some(3200), Some(2666), MemoryKind::Ddr4, FormFactor::Dimm)
        );
        assert_eq!((m.manufacturer.as_deref(), m.part_number.as_deref()), (Some("Acme"), Some("AC-1")));
    }

    #[test]
    fn extended_and_kb_sizes() {
        let mk = |sz: u16, ext: u32| {
            structure(
                17,
                0x28,
                &|d| {
                    put16(d, 0x0c, sz);
                    d[0x1c..0x20].copy_from_slice(&ext.to_le_bytes());
                },
                &[],
            )
        };
        let MemoryLayout::Slots(a) = parse(&mk(0x7fff, 65536), 1) else {
            panic!("slots")
        };
        assert_eq!(a.modules[0].capacity_bytes, 64 << 30);
        let MemoryLayout::Slots(a) = parse(&mk(0x8000 | 512, 0), 1) else {
            panic!("slots")
        };
        assert_eq!(a.modules[0].capacity_bytes, 512 << 10);
        assert_eq!(a.modules[0].slot, "Slot");
        // Extended maximum capacity.
        let mut t = structure(
            16,
            23,
            &|d| {
                d[0x07..0x0b].copy_from_slice(&0x8000_0000u32.to_le_bytes());
                d[0x0f..0x17].copy_from_slice(&(256u64 << 30).to_le_bytes());
            },
            &[],
        );
        t.extend(mk(1024, 0));
        let MemoryLayout::Slots(a) = parse(&t, 1) else { panic!("slots") };
        assert_eq!(a.max_capacity_bytes, Some(256 << 30));
    }

    #[test]
    fn junk_never_panics() {
        assert!(matches!(parse(&[], 5), MemoryLayout::TotalOnly { total: 5, .. }));
        let t = table();
        for cut in 0..t.len() {
            let _ = parse(&t[..cut], 1);
        }
        for junk in [
            &[17u8, 2, 0, 0][..],
            &[17, 40, 0, 0, 1, 2][..],
            &[0xff; 64][..],
            &[17, 4, 0, 0, b'a', b'b'][..],
        ] {
            assert!(matches!(parse(junk, 1), MemoryLayout::TotalOnly { .. }));
        }
        let mut many = Vec::new();
        for _ in 0..MAX_STRUCTURES + 10 {
            many.extend(structure(1, 4, &|_| {}, &[]));
        }
        assert!(matches!(parse(&many, 1), MemoryLayout::TotalOnly { .. }));
    }
}
