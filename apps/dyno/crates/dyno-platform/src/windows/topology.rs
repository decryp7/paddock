//! Performance and efficiency cores on Windows, from `GetLogicalProcessorInformationEx` (`EfficiencyClass`: the
//! highest class is the performance cores). Read once; only the first processor group (up to 64 logical cores) is
//! mapped, which is every consumer and workstation CPU.

use crate::parse::cpu_topology::kinds_from_classes;
use dyno_core::metrics::CoreKind;
use windows::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};

/// The kind of each logical core of group 0, or empty when the CPU is uniform or the call fails.
pub fn core_kinds(total: usize) -> Vec<CoreKind> {
    let mut len: u32 = 0;
    // SAFETY: a null buffer with a zero length asks for the required size; `len` is valid for writes.
    let _ = unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len) };
    if len == 0 || len > 1 << 20 {
        return Vec::new();
    }
    let mut buf = vec![0u8; len as usize];
    // SAFETY: `buf` is `len` bytes, the size the OS asked for; the pointer is only read as records the OS wrote.
    let ok = unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, Some(buf.as_mut_ptr().cast()), &mut len) };
    if ok.is_err() {
        return Vec::new();
    }
    let mut classes = vec![None::<u32>; total.min(64)];
    let mut offset = 0usize;
    while offset + std::mem::size_of::<u32>() * 2 <= len as usize {
        // SAFETY: `offset` is inside the buffer the OS filled and each record starts at the previous record's end;
        // records are read unaligned because the buffer is a byte vector.
        let rec: SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX = unsafe { std::ptr::read_unaligned(buf.as_ptr().add(offset).cast()) };
        let size = rec.Size as usize;
        if size == 0 || offset + size > len as usize {
            break;
        }
        if rec.Relationship == RelationProcessorCore {
            // SAFETY: for RelationProcessorCore the union holds a PROCESSOR_RELATIONSHIP.
            let p = unsafe { rec.Anonymous.Processor };
            let class = u32::from(p.EfficiencyClass);
            if p.GroupCount >= 1 && p.GroupMask[0].Group == 0 {
                let mask = p.GroupMask[0].Mask as u64;
                for (i, slot) in classes.iter_mut().enumerate() {
                    if mask >> i & 1 == 1 {
                        *slot = Some(class);
                    }
                }
            }
        }
        offset += size;
    }
    if classes.iter().any(Option::is_none) || classes.len() != total {
        return Vec::new();
    }
    kinds_from_classes(&classes.into_iter().flatten().collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_reports_nothing_or_one_kind_per_core() {
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let k = core_kinds(n);
        assert!(k.is_empty() || k.len() == n);
    }
}
