//! Any bytes, with any row bounds, read as a saved disk map: a typed error or an index, never a panic or a large allocation.
#![no_main]

use dyno_core::disk::index::{DiskIndex, RecordBounds, RECORD_FORMAT};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The bounds claim the stream's own length and the node count its header names, the case that gets furthest.
    let nodes = data.get(8..12).map_or(1, |b| u64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])));
    let bounds = RecordBounds {
        format: RECORD_FORMAT,
        nodes,
        stored_bytes: data.len() as u64,
    };
    let _ = DiskIndex::from_records(data, bounds);
});
