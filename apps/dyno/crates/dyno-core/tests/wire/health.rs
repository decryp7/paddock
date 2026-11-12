#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The wire shape of `HealthReport` (protocol 8: it gained `not_checked`). Three checks:
//! - the JSON the webview gets equals `app/src/app/core/health.wire.json`, which `health.contract.spec.ts` checks against
//!   `api.types.ts` (update with `DYNO_UPDATE_WIRE_FIXTURE=1`, and the spec with it);
//! - the postcard bytes are pinned, so a later field reorder is caught (a deliberate change is a new `PROTOCOL_VERSION`);
//! - a report written by a protocol-7 peer (no `not_checked`) is refused by the protocol-8 types instead of read wrongly.

use dyno_core::health::{Grade, HealthReport, NotChecked, NotCheckedKind};
use dyno_core::protocol::Reply;
use dyno_core::time::Timestamp;
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;

fn report() -> HealthReport {
    let gap = |what: &[&str], why: &str, kind| NotChecked {
        what: what.iter().map(|w| (*w).to_string()).collect(),
        why: why.into(),
        kind,
    };
    HealthReport {
        at: Timestamp(1),
        score: 100,
        grade: Grade::Good,
        performance_score: 100,
        capped_by: None,
        reasons: vec![],
        not_checked: vec![
            gap(&["Disk health"], "Needs administrator rights", NotCheckedKind::CouldNotRead),
            gap(&["Pending updates"], "Dyno does not check for updates", NotCheckedKind::NotReadHere),
            gap(
                &["Restart needed", "Startup items"],
                "Dyno does not check this yet",
                NotCheckedKind::NotReadYet,
            ),
        ],
    }
}

#[test]
fn the_json_fixture_matches_the_rust_type() {
    let now = serde_json::to_value(report()).unwrap();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/health.wire.json");
    if std::env::var_os("DYNO_UPDATE_WIRE_FIXTURE").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&now).unwrap() + "\n").unwrap();
    }
    let on_disk: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("the fixture exists")).unwrap();
    assert_eq!(
        on_disk, now,
        "run with DYNO_UPDATE_WIRE_FIXTURE=1 and update health.contract.spec.ts to match"
    );
}

#[test]
fn a_health_report_has_these_postcard_bytes() {
    let bytes = postcard::to_stdvec(&Reply::Health(Box::new(report()))).unwrap();
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, EXPECTED, "a changed layout is a new PROTOCOL_VERSION");
    let back: Reply = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(back, Reply::Health(Box::new(report())));
}

/// `HealthReport` as protocol 7 wrote it.
#[derive(Serialize)]
struct HealthReportV7 {
    at: Timestamp,
    score: u32,
    grade: Grade,
    performance_score: u32,
    capped_by: Option<dyno_core::health::Severity>,
    reasons: Vec<dyno_core::health::HealthReason>,
}

#[test]
fn a_report_written_by_protocol_7_is_refused_not_read_wrongly() {
    let old = HealthReportV7 {
        at: Timestamp(1),
        score: 100,
        grade: Grade::Good,
        performance_score: 100,
        capped_by: None,
        reasons: vec![],
    };
    // `Reply::Health` is a variant of the enum; reuse its tag from the current encoding, then the old body.
    let tag = postcard::to_stdvec(&Reply::Health(Box::new(report()))).unwrap()[0];
    let mut bytes = vec![tag];
    bytes.extend(postcard::to_stdvec(&old).unwrap());
    assert!(postcard::from_bytes::<Reply>(&bytes).is_err(), "the missing list must be an error");
}

/// Reply::Health (0b), at 1, score 100, Good, performance 100, no cap, no reasons, then three entries: what (count, strings), why,
/// kind (0 could not read, 1 not read here, 2 not read yet).
const EXPECTED: &str = "0b02640064000003010b4469736b206865616c74681a4e656564732061646d696e6973747261746f722072696768747300010f50656e64696e6720757064617465731f44796e6f20646f6573206e6f7420636865636b20666f72207570646174657301020e52657374617274206e65656465640d53746172747570206974656d731c44796e6f20646f6573206e6f7420636865636b20746869732079657402";
