#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The disk map's wire contract with the UI. The Rust types serialise into `app/src/app/core/disk-map.wire.json`; the UI's
//! spec (`disk-map.contract.spec.ts`) holds the same values as TypeScript literals typed by `api.types.ts` and checks they
//! equal this file, so a change on one side without the other fails a test on both. To update the file after a deliberate
//! change, run this test with `DYNO_UPDATE_WIRE_FIXTURE=1` and commit the result together with the TypeScript.

use dyno_core::disk::index::{DiskIndex, ScanLimits, Sizes};
use dyno_core::disk::reason::UnreadableReason;
use dyno_core::disk::scan::{DiskScanStatus, DiskTarget, NotSaved, ScanFailure, ScanOrigin, ScanPhase, ScanProgress, ScanState};
use dyno_core::disk::volume::{NetworkProtocol, Volume, VolumeLocation};
use dyno_core::protocol::Request;
use dyno_core::time::Timestamp;
use serde_json::{json, to_value, Value};
use std::path::PathBuf;

fn volume(id: &str, location: VolumeLocation, whole: bool) -> Volume {
    Volume {
        id: id.into(),
        name: id.into(),
        mount: format!("/Volumes/{id}"),
        fs: "apfs".into(),
        location,
        capacity: Some(1000),
        free: Some(400),
        rotational: Some(false),
        read_only: false,
        reachable: true,
        whole_volume: whole,
        measured_at: None,
        freshness: Default::default(),
    }
}

fn status(scan: u64, state: ScanState, volume: Option<Volume>) -> DiskScanStatus {
    DiskScanStatus {
        scan,
        seq: 3,
        name: "Macintosh HD".into(),
        target: DiskTarget::Volume { id: "Macintosh HD".into() },
        volume,
        state,
        phase: ScanPhase::ReadingFolders,
        progress: ScanProgress {
            bytes: 600,
            items: 12,
            folders: 3,
            total_bytes: Some(600),
            unreadable: 1,
        },
        truncated: false,
        unreadable_first: vec!["/Users/alex/Library/Mail".into()],
        needs_full_disk_access: true,
        items_per_second: Some(1200),
        eta_seconds: None,
        hidden_bytes: Some(40),
        scanned_at: Some(Timestamp(1_700_000_100_000)),
        origin: ScanOrigin::Live,
        changed_bytes: None,
        replaces: None,
        not_saved: None,
    }
}

/// A finished scan read back from the cache, or a live one that could not be kept.
fn saved_status(scan: u64, changed_bytes: Option<i64>, replaces: Option<u64>, not_saved: Option<NotSaved>) -> DiskScanStatus {
    DiskScanStatus {
        origin: if not_saved.is_some() {
            ScanOrigin::Live
        } else {
            ScanOrigin::Saved {
                saved_at: Timestamp(1_700_000_200_000),
            }
        },
        changed_bytes,
        replaces,
        not_saved,
        ..status(scan, ScanState::Done, None)
    }
}

fn children() -> Value {
    let mut i = DiskIndex::new(
        "Macintosh HD",
        ScanLimits {
            max_nodes: 100,
            notable_bytes: 10,
            max_hard_links: 10,
        },
    );
    let root = i.root();
    let users = i.add_folder(root, "Users").unwrap();
    i.add_file(
        users,
        "disk-image.dmg",
        Sizes {
            on_disk: 500,
            logical: 480,
        },
        Timestamp(1_700_000_000_000),
    );
    i.add_file(root, "swap", Sizes::same(4), Timestamp(1_600_000_000_000));
    i.add_file(root, "vm.img", Sizes { on_disk: 100, logical: 90 }, Timestamp(1_650_000_000_000));
    i.add_mount_point(root, "Volumes");
    let locked = i.add_folder(root, "private").unwrap();
    i.mark_unreadable(locked, UnreadableReason::NeedsFullDiskAccess);
    i.finish_listing(users);
    to_value(i.children(root, 0, 10, 0.0).unwrap()).unwrap()
}

/// The same index as `children()`, as the nested answer.
fn rings() -> Value {
    let mut i = DiskIndex::new(
        "Macintosh HD",
        ScanLimits {
            max_nodes: 100,
            notable_bytes: 10,
            max_hard_links: 10,
        },
    );
    let root = i.root();
    let users = i.add_folder(root, "Users").unwrap();
    i.add_file(
        users,
        "disk-image.dmg",
        Sizes {
            on_disk: 500,
            logical: 480,
        },
        Timestamp(1_700_000_000_000),
    );
    i.add_file(users, "notes", Sizes::same(3), Timestamp(1_700_000_000_000));
    let locked = i.add_folder(root, "private").unwrap();
    i.mark_unreadable(locked, UnreadableReason::NeedsFullDiskAccess);
    i.finish_listing(users);
    to_value(i.rings(root, 2, 0.0, 10).unwrap()).unwrap()
}

fn sample() -> Value {
    let internal = volume("Macintosh HD", VolumeLocation::Internal, true);
    let usb = volume("Photos", VolumeLocation::External { bus: Some("USB".into()) }, true);
    let share = volume(
        "Media",
        VolumeLocation::Network {
            protocol: NetworkProtocol::WebDav,
            host: "nas.local".into(),
        },
        false,
    );
    let failed = ScanState::Failed {
        failure: ScanFailure::VolumeUnreachable {
            detail: "the share stopped answering".into(),
        },
    };
    json!({
        "requests": {
            "diskVolumes": to_value(Request::DiskVolumes).unwrap(),
            "startVolume": to_value(Request::StartDiskScan { target: DiskTarget::Volume { id: "Macintosh HD".into() }, replaces: None }).unwrap(),
            "rescanVolume": to_value(Request::StartDiskScan { target: DiskTarget::Volume { id: "Macintosh HD".into() }, replaces: Some(7) }).unwrap(),
            "startFolder": to_value(Request::StartDiskScan { target: DiskTarget::Folder { path: "/Users/alex".into() }, replaces: None }).unwrap(),
            "forgetSaved": to_value(Request::ForgetSavedDiskMap { scan: 7 }).unwrap(),
            "forgetAllSaved": to_value(Request::ForgetSavedDiskMaps).unwrap(),
            "cancel": to_value(Request::CancelDiskScan { scan: 7 }).unwrap(),
            "scans": to_value(Request::DiskScans).unwrap(),
            "children": to_value(Request::DiskChildren { scan: 7, node: Some(4), offset: 0, limit: 48, min_fraction: 0.004 }).unwrap(),
            "childrenOfRoot": to_value(Request::DiskChildren { scan: 7, node: None, offset: 48, limit: 48, min_fraction: 0.0 }).unwrap(),
            "path": to_value(Request::DiskPath { scan: 7, node: 4 }).unwrap(),
            "retry": to_value(Request::RetryDiskScan { scan: 7 }).unwrap(),
            "rings": to_value(Request::DiskRings { scan: 7, node: Some(4), rings: 3, min_share: 0.01, max_nodes: 500 }).unwrap(),
            "ringsOfRoot": to_value(Request::DiskRings { scan: 7, node: None, rings: 6, min_share: 0.0, max_nodes: 2000 }).unwrap(),
            "openFullDiskAccess": to_value(Request::OpenFullDiskAccessSettings).unwrap(),
            "reveal": to_value(Request::DiskReveal { scan: 7, node: 4 }).unwrap(),
        },
        "volumes": [to_value(&internal).unwrap(), to_value(&usb).unwrap(), to_value(&share).unwrap()],
        "statuses": [
            to_value(status(7, ScanState::Running, Some(internal.clone()))).unwrap(),
            to_value(status(7, ScanState::Done, None)).unwrap(),
            to_value(status(7, ScanState::Cancelled, None)).unwrap(),
            to_value(status(7, failed, None)).unwrap(),
            to_value(status(7, ScanState::Failed { failure: ScanFailure::CannotStart { reason: "no".into() } }, None)).unwrap(),
            to_value(status(7, ScanState::Failed { failure: ScanFailure::Internal { detail: "bug".into() } }, None)).unwrap(),
            to_value(saved_status(8, Some(-120), None, None)).unwrap(),
            to_value(saved_status(9, None, Some(8), Some(NotSaved::TooBig))).unwrap(),
            to_value(saved_status(10, Some(4096), None, Some(NotSaved::NoSpace))).unwrap(),
            to_value(saved_status(11, None, None, Some(NotSaved::Failed))).unwrap(),
        ],
        "settings": to_value(dyno_core::disk::settings::DiskSettings::default()).unwrap(),
        "rings": rings(),
        "phases": [to_value(ScanPhase::ReadingVolumeFacts).unwrap(), to_value(ScanPhase::ReadingFolders).unwrap()],
        "children": children(),
    })
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("app")
        .join("src")
        .join("app")
        .join("core")
        .join("disk-map.wire.json")
}

#[test]
fn the_wire_fixture_matches_the_rust_types() {
    let now = sample();
    let path = fixture_path();
    if std::env::var_os("DYNO_UPDATE_WIRE_FIXTURE").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&now).unwrap() + "\n").unwrap();
    }
    let on_disk: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("the fixture exists")).unwrap();
    assert_eq!(
        on_disk, now,
        "run with DYNO_UPDATE_WIRE_FIXTURE=1 and update disk-map.contract.spec.ts to match"
    );
}
