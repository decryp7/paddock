//! The bytes of a downloaded location database come from the network: opening, checking (including `Reader::verify`) and looking
//! up must never panic or run away on any input, and a gzip wrapper must not inflate past its cap (ADR 0031 §3.1).
#![no_main]

use dyno_agent::geo_db::{DbKind, GeoReader, inflate_capped, open_checked};
use dyno_core::geo::GeoLookup;
use libfuzzer_sys::fuzz_target;

const NOW: u64 = 1_800_000_000;

fuzz_target!(|data: &[u8]| {
    // As a download: the gzip layer with a small cap.
    let _ = inflate_capped(data, 1 << 20);
    // As the unpacked file, for both kinds.
    for kind in [DbKind::Country, DbKind::Asn] {
        if let Ok((reader, info)) = open_checked(data.to_vec(), kind, NOW, true) {
            let g = GeoReader::new(Some((reader, info)), None);
            for ip in ["8.8.8.8", "2606:4700::1111", "192.0.2.1", "::ffff:1.2.3.4"] {
                let _ = g.lookup(&ip.parse().unwrap());
            }
        }
    }
});
