//! A folder watch end to end: one watch tracks several products, each with its own latest version, "new" state and
//! notification; files of one build join into one release; extraction previews run in Rust. Folders are temporary.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::release::LatestBy;
use spotter_core::source::SourceConfig;
use spotter_core::time::Timestamp;
use spotter_core::tracking::ProductSetting;
use spotter_core::watch::WatchSpec;
use spotter_engine::clock::ManualClock;
use spotter_engine::notifier::RecordingNotifier;
use spotter_engine::secrets::MemorySecrets;
use spotter_engine::{Engine, EngineConfig, Services};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

const T0: i64 = 1_800_000_000_000;
const ACME: &str =
    r"Acme-(?<id>[A-Z]+)-R(?<major>\d+)\.(?<minor>\d+)\.(?<patch>\d+)T(?<build>\d+)(?:_(?<variant>x64|arm64))?(?:-(?<channel>beta|rc\d*))?";

struct Fixture {
    engine: Engine,
    notifier: Arc<RecordingNotifier>,
    clock: Arc<ManualClock>,
    drops: tempfile::TempDir,
    _data: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let notifier = Arc::new(RecordingNotifier::default());
    let clock = Arc::new(ManualClock::new(Timestamp(T0)));
    let mut svc = Services::new(Arc::new(MemorySecrets::new()));
    svc.notifier = notifier.clone();
    svc.clock = clock.clone();
    let engine = Engine::open(EngineConfig::new(data.path(), "test"), svc).unwrap();
    Fixture {
        engine,
        notifier,
        clock,
        drops: tempfile::tempdir().unwrap(),
        _data: data,
    }
}

fn put(dir: &Path, rel: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, rel.as_bytes()).unwrap();
}

fn spec(dir: &Path, tweak: impl FnOnce(&mut WatchSpec)) -> WatchSpec {
    let mut s = WatchSpec {
        id: String::new(),
        name: "Acme Installer".into(),
        source: SourceConfig::new(
            "folder",
            &[
                ("path", &dir.to_string_lossy()),
                ("settleSeconds", "0"),
                ("relistEvery", "1"),
                ("extensions", "iso,exe,zip"),
            ],
        ),
        versions: "custom".into(),
        version_pattern: Some(ACME.into()),
        include_prereleases: false,
        name_filter: String::new(),
        interval_secs: 60,
        notify: Default::default(),
        actions: Vec::new(),
        channels: Vec::new(),
        version_filter: Default::default(),
        allow_local: false,
        paused: false,
        tracking: Default::default(),
    };
    tweak(&mut s);
    s
}

async fn check(f: &Fixture, id: &str) {
    f.clock.advance_secs(120);
    f.engine.check_one(id).await.unwrap();
}

#[tokio::test]
async fn each_product_has_its_own_latest_new_state_and_notification() {
    let f = fixture();
    put(f.drops.path(), "Acme-PRO-R3.09.00T40.iso");
    put(f.drops.path(), "Acme-QA-R1.00.00T05.zip");
    put(f.drops.path(), "readme.txt");
    let saved = f.engine.save_watch(spec(f.drops.path(), |_| {}), &BTreeMap::new()).unwrap();
    f.engine.check_one(&saved.id).await.unwrap();
    assert!(f.notifier.notifications().is_empty(), "the first check is every product's baseline");
    let d = f.engine.get_watch(&saved.id).unwrap();
    assert_eq!((d.summary.product_count, d.products.len()), (2, 2));

    // PRO gets a new build as an ISO and an installer; QA is untouched.
    put(f.drops.path(), "Acme-PRO-R3.10.00T32.iso");
    put(f.drops.path(), "Acme-PRO-R3.10.00T32_x64.exe");
    put(f.drops.path(), "Acme-PRO-R3.10.00T32_x64.exe.sha256");
    check(&f, &saved.id).await;
    let n = f.notifier.notifications();
    assert_eq!(n.len(), 1, "one release, however many files");
    assert_eq!(n[0].title, "Acme Installer · PRO R3.10.00T32");
    let d = f.engine.get_watch(&saved.id).unwrap();
    let pro = d.products.iter().find(|p| p.id == "PRO").unwrap();
    let latest = pro.latest.as_ref().unwrap();
    assert_eq!(latest.artifacts.len(), 2);
    assert_eq!(latest.artifacts.iter().filter(|a| a.checksum_file.is_some()).count(), 1);
    assert!(pro.unseen && !d.products.iter().find(|p| p.id == "QA").unwrap().unseen);
    assert!(d.summary.unseen);

    // A late artifact joins the release without a second notification.
    put(f.drops.path(), "Acme-PRO-R3.10.00T32_arm64.exe");
    check(&f, &saved.id).await;
    assert_eq!(f.notifier.notifications().len(), 1);
    let d = f.engine.get_watch(&saved.id).unwrap();
    assert_eq!(
        d.products
            .iter()
            .find(|p| p.id == "PRO")
            .unwrap()
            .latest
            .as_ref()
            .unwrap()
            .artifacts
            .len(),
        3
    );

    // Acknowledging one product leaves the others alone.
    put(f.drops.path(), "Acme-QA-R1.01.00T01.zip");
    check(&f, &saved.id).await;
    assert_eq!(f.notifier.notifications().len(), 2);
    f.engine.acknowledge_product(&saved.id, "PRO").unwrap();
    let d = f.engine.get_watch(&saved.id).unwrap();
    assert!(!d.products.iter().find(|p| p.id == "PRO").unwrap().unseen && d.products.iter().find(|p| p.id == "QA").unwrap().unseen);
}

#[tokio::test]
async fn a_new_product_is_quiet_unless_asked_for_and_products_can_be_restricted_renamed_and_muted() {
    let f = fixture();
    put(f.drops.path(), "Acme-PRO-R3.09.00T40.iso");
    let saved = f
        .engine
        .save_watch(
            spec(f.drops.path(), |s| {
                s.tracking.products.settings.insert(
                    "PRO".into(),
                    ProductSetting {
                        name: "Acme Installer Pro".into(),
                        ..ProductSetting::default()
                    },
                );
            }),
            &BTreeMap::new(),
        )
        .unwrap();
    f.engine.check_one(&saved.id).await.unwrap();
    put(f.drops.path(), "Acme-LITE-R1.00.00T01.zip");
    check(&f, &saved.id).await;
    assert!(f.notifier.notifications().is_empty(), "a new product appearing is off by default");
    assert_eq!(f.engine.get_watch(&saved.id).unwrap().products.len(), 2);
    put(f.drops.path(), "Acme-PRO-R3.10.00T01.iso");
    check(&f, &saved.id).await;
    assert_eq!(
        f.notifier.notifications()[0].title,
        "Acme Installer Pro R3.10.00T01",
        "a renamed product replaces the id"
    );

    // Restricted to PRO, muted LITE, announcing new products.
    let f = fixture();
    put(f.drops.path(), "Acme-PRO-R3.09.00T40.iso");
    let saved = f
        .engine
        .save_watch(
            spec(f.drops.path(), |s| {
                s.tracking.products.only = vec!["PRO".into(), "NEW".into()];
                s.tracking.products.announce_new = true;
            }),
            &BTreeMap::new(),
        )
        .unwrap();
    f.engine.check_one(&saved.id).await.unwrap();
    put(f.drops.path(), "Acme-LITE-R1.00.00T01.zip");
    put(f.drops.path(), "Acme-NEW-R1.00.00T01.zip");
    check(&f, &saved.id).await;
    let n = f.notifier.notifications();
    assert_eq!(n.len(), 1, "LITE is not tracked");
    assert_eq!(n[0].title, "Acme Installer · NEW R1.00.00T01");
    assert!(n[0].body.contains("new product"), "{}", n[0].body);
    assert_eq!(f.engine.get_watch(&saved.id).unwrap().products.len(), 2);
}

#[tokio::test]
async fn the_wizard_preview_reads_names_in_rust() {
    let f = fixture();
    put(f.drops.path(), "Acme-PRO-R3.10.00T32.iso");
    put(f.drops.path(), "Acme-PRO-R3.10.00T32_x64.exe");
    put(f.drops.path(), "Acme-QA-R1.00.00T05.zip");
    put(f.drops.path(), "Acme-QA-R1.01.00T01-beta.zip");
    put(f.drops.path(), "notes.zip");
    let p = f.engine.test_source(&spec(f.drops.path(), |_| {}), &BTreeMap::new()).await;
    assert!(p.error.is_none() && p.problems.is_empty(), "{p:?}");
    let x = p.extraction.unwrap();
    let scan = x.scan.unwrap();
    assert_eq!((scan.scanned, scan.matched), (5, 4));
    let pro = x.products.iter().find(|p| p.id.as_deref() == Some("PRO")).unwrap();
    assert_eq!(pro.versions.len(), 1);
    assert!(pro.versions[0].latest && pro.versions[0].artifacts.len() == 2);
    let skipped: Vec<_> = x
        .rows
        .iter()
        .filter_map(|r| r.ignored.as_deref().map(|w| (r.file_name.as_str(), w)))
        .collect();
    assert!(
        skipped.iter().any(|(n, w)| *n == "notes.zip" && w.contains("no match")),
        "{skipped:?}"
    );
    assert!(
        skipped.iter().any(|(n, w)| n.contains("beta") && w.contains("pre-release")),
        "{skipped:?}"
    );
    // A bad pattern is a message, and nothing is scanned.
    let bad = f
        .engine
        .test_source(&spec(f.drops.path(), |s| s.version_pattern = Some("(".into())), &BTreeMap::new())
        .await;
    assert!(bad.problems[0].contains("not a valid regular expression"), "{:?}", bad.problems);
    // The Test field, the generated pattern and the shipped patterns.
    let t = f.engine.test_pattern("custom", Some(ACME), false, "Acme-QA-R4.01.02T7_arm64.exe");
    assert_eq!(t.read.unwrap().variant, "arm64");
    let schemes = f.engine.version_schemes();
    assert!(schemes.iter().any(|s| s.id == "letters-build"));
    let t = f.engine.test_pattern("letters-build", None, false, "Setup-R1.04.00T12.exe");
    assert_eq!(t.read.unwrap().version, "R1.04.00T12");
}

#[tokio::test]
async fn latest_by_modified_follows_the_time_and_an_in_place_overwrite_is_reported() {
    let f = fixture();
    put(f.drops.path(), "setup.exe");
    let make = |s: &mut WatchSpec| {
        s.version_pattern = Some(r"^(?<id>setup)\.exe$".into());
        s.tracking.latest_by = LatestBy::Modified;
    };
    let saved = f.engine.save_watch(spec(f.drops.path(), make), &BTreeMap::new()).unwrap();
    f.engine.check_one(&saved.id).await.unwrap();
    assert!(f.notifier.notifications().is_empty());
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(f.drops.path().join("setup.exe"), b"a different build, same name").unwrap();
    check(&f, &saved.id).await;
    let n = f.notifier.notifications();
    assert_eq!(n.len(), 1);
    assert!(n[0].title.starts_with("Acme Installer · setup setup.exe"), "{}", n[0].title);
    let d = f.engine.get_watch(&saved.id).unwrap();
    assert!(d.products[0].latest.as_ref().unwrap().updated_in_place);
    // The preview warns about modified times.
    let p = f.engine.test_source(&spec(f.drops.path(), make), &BTreeMap::new()).await;
    assert!(p.extraction.unwrap().warnings[0].contains("re-copied old build"));
}

#[tokio::test]
async fn too_many_files_is_a_status_with_a_next_step() {
    let f = fixture();
    for i in 0..120 {
        put(f.drops.path(), &format!("junk-{i}.zip"));
    }
    let mut s = spec(f.drops.path(), |_| {});
    s.source.values.insert("maxEntries".into(), "100".into());
    let saved = f.engine.save_watch(s, &BTreeMap::new()).unwrap();
    f.engine.check_one(&saved.id).await.unwrap();
    let d = f.engine.get_watch(&saved.id).unwrap();
    let msg = format!("{:?}", d.summary.status);
    assert!(msg.contains("Too many files") && msg.contains("Narrow"), "{msg}");
}
