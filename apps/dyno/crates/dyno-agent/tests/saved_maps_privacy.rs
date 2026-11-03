#![allow(clippy::unwrap_used, clippy::expect_used)]

//! ADR 0029 section 6: the agent's log of saving, loading, unloading, evicting, a damaged saved map and forgetting carries ids,
//! kinds, counts, sizes and outcomes, and never a path or a name from the scanned folders. One test, because the log is the
//! process's: it installs a capturing logger, runs a whole agent over folders with invented, distinctive names, restarts it, and
//! checks every line the run wrote.

use dyno_agent::cli::{ask, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::disk::scan::{DiskScanStatus, DiskTarget};
use dyno_core::protocol::{Reply, Request};
use dyno_platform::fake::{process, Fake};
use std::sync::Mutex;

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

struct Kv(String);

impl<'kvs> log::kv::VisitSource<'kvs> for Kv {
    fn visit_pair(&mut self, key: log::kv::Key<'kvs>, value: log::kv::Value<'kvs>) -> Result<(), log::kv::Error> {
        self.0.push_str(&format!(" {key}={value}"));
        Ok(())
    }
}

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, r: &log::Record) {
        let mut kv = Kv(String::new());
        let _ = r.key_values().visit(&mut kv);
        LINES.lock().unwrap().push(format!("{} {}{}", r.level(), r.args(), kv.0));
    }

    fn flush(&self) {}
}

fn start(dirs: &Dirs) -> Running {
    let fake = Fake::new();
    fake.add_process(process(100, "calm", 1.0));
    Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap()
}

fn scans(running: &Running, dirs: &Dirs) -> Vec<DiskScanStatus> {
    let peer = connect(dirs).unwrap().0;
    let Reply::DiskScans(s) = ask(&peer, Request::DiskScans).unwrap() else {
        panic!("scans")
    };
    drop(peer);
    let _ = running;
    s
}

fn level(dirs: &Dirs, scan: u64) -> bool {
    let peer = connect(dirs).unwrap().0;
    ask(
        &peer,
        Request::DiskChildren {
            scan,
            node: None,
            offset: 0,
            limit: 10,
            min_fraction: 0.0,
        },
    )
    .is_ok()
}

#[test]
fn the_log_of_a_whole_run_over_saved_maps_names_no_path_and_no_folder() {
    log::set_logger(&Capture).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let work = std::fs::canonicalize(tmp.path()).unwrap().join("__PRIVATE_work__");

    let mut running = start(&dirs);
    // More folders than the memory holds (3) and than the cache keeps (16): maps are saved, unloaded and evicted.
    for n in 0..18 {
        let folder = work.join(format!("__PRIVATE_folder_{n}__"));
        std::fs::create_dir_all(folder.join(format!("__PRIVATE_inner_{n}__"))).unwrap();
        std::fs::write(folder.join("__PRIVATE_taxes__.txt"), vec![1u8; 10]).unwrap();
        let peer = connect(&dirs).unwrap().0;
        let Reply::DiskScan(s) = ask(
            &peer,
            Request::StartDiskScan {
                target: DiskTarget::Folder {
                    path: folder.to_string_lossy().into_owned(),
                },
                replaces: None,
            },
        )
        .unwrap() else {
            panic!("a scan")
        };
        running.shared.disk.wait_finished(s.scan).unwrap();
        // The save is written before the next scan starts: eviction order is then the order of the scans. (The pass hands its
        // map to the cache writer as it ends; the test waits for the row, not for a time.)
        let cache = running.shared.cache.as_ref().unwrap();
        let key = format!("folder:{}", folder.to_string_lossy());
        while !cache.list().iter().any(|r| r.params.target_key == key) {
            std::thread::yield_now();
        }
    }
    // A map that was unloaded to make room answers again: it is loaded.
    let first_run = scans(&running, &dirs);
    assert!(first_run.len() >= 16, "the saved maps and the scans of this run are listed");
    for s in &first_run {
        level(&dirs, s.scan);
    }

    // A restart offers the saved maps; asking for a level loads each.
    running.stop();
    running = start(&dirs);
    let offered = scans(&running, &dirs);
    assert_eq!(offered.len(), 16, "one saved map per target, 16 targets");
    assert!(offered
        .iter()
        .all(|s| matches!(s.origin, dyno_core::disk::scan::ScanOrigin::Saved { .. })));
    // One is damaged on disk: it answers "scan again" and is dropped alone.
    {
        let raw = rusqlite::Connection::open(dirs.cache_db()).unwrap();
        raw.execute(
            "UPDATE disk_chunks SET data = X'01FFFF' WHERE saved = (SELECT id FROM disk_saved ORDER BY id LIMIT 1)",
            [],
        )
        .unwrap();
    }
    let answered = offered.iter().filter(|s| level(&dirs, s.scan)).count();
    assert_eq!(answered, 15, "the damaged one is dropped, the others load");

    // Forgetting every saved map.
    let peer = connect(&dirs).unwrap().0;
    assert!(matches!(ask(&peer, Request::ForgetSavedDiskMaps).unwrap(), Reply::Ok));
    drop(peer);
    running.stop();

    let lines = LINES.lock().unwrap().clone();
    let all = lines.join("\n");
    let secret_dir = tmp.path().to_string_lossy().into_owned();
    for needle in ["__PRIVATE_", secret_dir.as_str(), "taxes"] {
        let leaks: Vec<&String> = lines.iter().filter(|l| l.contains(needle)).collect();
        assert!(leaks.is_empty(), "the log names {needle:?}: {leaks:?}");
    }
    // The run is not silent: each step left its line, with ids and sizes.
    for expected in [
        "Disk map saved",
        "Saved disk map loaded",
        "Disk map unloaded from memory",
        "Saved disk maps offered",
        "could not be loaded and is dropped",
        "All saved disk maps forgotten",
    ] {
        assert!(all.contains(expected), "no line says {expected:?}:\n{all}");
    }
    assert!(
        all.contains("PackedBytes=") && all.contains("Nodes=") && all.contains("Kind=folder"),
        "sizes and the kind are logged"
    );
}
