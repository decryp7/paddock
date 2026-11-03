//! A user's list file or a subscription's answer is untrusted text: parsing, deduplicating and building the index must never
//! panic, whatever the bytes (ADR 0031 amendment 1).
#![no_main]

use dyno_core::intel::{AllowInput, IntelIndex, ListInput};
use dyno_core::intel_text::parse_list;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let parsed = parse_list(&text, 10_000);
    let list = ListInput {
        id: "f".into(),
        name: "f".into(),
        enabled: true,
        entries: parsed.entries,
    };
    if let Ok(ix) = IntelIndex::build(&[list], &AllowInput::default()) {
        for ip in ["8.8.8.8", "10.0.0.1", "2606:4700::1111", "::ffff:8.8.8.8"] {
            let _ = ix.check(&ip.parse().unwrap(), Some("a.example"));
        }
    }
});
