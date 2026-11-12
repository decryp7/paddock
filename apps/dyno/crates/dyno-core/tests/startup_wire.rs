#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The wire shape of `StartupOverview` and `StartupLocation` after ADR 0033, WP-4 (still protocol 8): `signer_read` and `coverage` were
//! added to the overview, and `UserRunOnceKey`, `MachineRunOnceKey` and `PackagedApp` were appended after `LoginItem`. Two checks:
//! - the JSON the webview gets equals `app/src/app/core/startup.wire.json`, which `startup.contract.spec.ts` checks against
//!   `api.types.ts` (update with `DYNO_UPDATE_WIRE_FIXTURE=1`, and the spec with it);
//! - the postcard bytes are pinned, so a later reorder is caught (a deliberate change is a new `PROTOCOL_VERSION`).

use dyno_core::protocol::Reply;
use dyno_core::read_via::ReadVia;
use dyno_core::startup::{ImpactRating, StartupEntry, StartupImpact, StartupLocation, StartupOverview, StartupState};
use serde_json::Value;
use std::path::PathBuf;

fn entry(id: &str, name: &str, location: StartupLocation) -> StartupEntry {
    StartupEntry {
        id: id.into(),
        name: name.into(),
        command: "\"C:\\Tools\\a.exe\" --x".into(),
        exe: Some("C:\\Tools\\a.exe".into()),
        location,
        state: StartupState::Enabled,
        publisher: None,
        impact: StartupImpact {
            cpu_secs: 0.0,
            disk_bytes: 0,
            rating: ImpactRating::NotMeasured,
        },
    }
}

fn overview() -> StartupOverview {
    StartupOverview {
        entries: vec![
            entry("reg:HKCU:runonce:64:a", "a", StartupLocation::UserRunOnceKey),
            entry("reg:HKLM:runonce:64:b", "b", StartupLocation::MachineRunOnceKey),
            entry("appx:Fam!Task", "c", StartupLocation::PackagedApp),
        ],
        total: 3,
        os_count: 0,
        signed_count: 0,
        unavailable: vec!["The all-users Run key (32-bit) could not be read: access denied (error 5).".into()],
        btm_via: ReadVia::Local,
        signer_read: false,
        coverage: Some("Lists Run entries.".into()),
    }
}

#[test]
fn the_json_fixture_matches_the_rust_type() {
    let now = serde_json::to_value(overview()).unwrap();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/startup.wire.json");
    if std::env::var_os("DYNO_UPDATE_WIRE_FIXTURE").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&now).unwrap() + "\n").unwrap();
    }
    let on_disk: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("the fixture exists")).unwrap();
    assert_eq!(
        on_disk, now,
        "run with DYNO_UPDATE_WIRE_FIXTURE=1 and update startup.contract.spec.ts to match"
    );
}

#[test]
fn a_startup_overview_has_these_postcard_bytes() {
    let bytes = postcard::to_stdvec(&Reply::StartupOverview(Box::new(overview()))).unwrap();
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, EXPECTED, "a changed layout is a new PROTOCOL_VERSION");
    let back: Reply = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(back, Reply::StartupOverview(Box::new(overview())));
}

/// Reply::StartupOverview (2b), three entries (id, name, command, exe as Some, location 08/09/0a, state 00, no publisher, impact zero
/// and NotMeasured), total 3, os 0, signed 0, one unavailable sentence, btm_via Local (00), signer_read false (00), coverage Some.
const EXPECTED: &str = "2b03157265673a484b43553a72756e6f6e63653a36343a6101611422433a5c546f6f6c735c612e65786522202d2d78010e433a5c546f6f6c735c612e657865080000000000000000157265673a484b4c4d3a72756e6f6e63653a36343a6201621422433a5c546f6f6c735c612e65786522202d2d78010e433a5c546f6f6c735c612e6578650900000000000000000d617070783a46616d215461736b01631422433a5c546f6f6c735c612e65786522202d2d78010e433a5c546f6f6c735c612e6578650a0000000000000000030000014a54686520616c6c2d75736572732052756e206b6579202833322d6269742920636f756c64206e6f7420626520726561643a206163636573732064656e69656420286572726f722035292e010001124c697374732052756e20656e74726965732e";
