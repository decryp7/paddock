//! Sets and cancels a real wake through `pmset` on the machine it runs on: only for the local macOS test VM, as root (`pmset schedule`
//! needs it): `PADDOCK_VM=1` and `--ignored`. Never in CI, never on a developer's machine (ER-14). It checks that Dyno's tagged event is
//! the only one listed and cancelled, and that an event it did not make is left alone.
#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use dyno_core::wake::{WakeScheduler, WakeTag};
use dyno_platform::runner::{tool, SystemRunner, ToolRunner};
use dyno_platform::wakes::{pmset_date, system_offset_at, PmsetWakes};
use std::time::Duration;

fn sched() -> String {
    SystemRunner.run_tool(&tool("/usr/bin/pmset", &["-g", "sched"]), Duration::from_secs(15)).unwrap().stdout
}

#[test]
#[ignore = "sets a real wake of this machine: the macOS test VM only, as root (PADDOCK_VM=1)"]
fn dynos_wake_is_the_only_one_listed_and_cancelled() {
    if std::env::var("PADDOCK_VM").as_deref() != Ok("1") {
        eprintln!("skipped: PADDOCK_VM is not 1");
        return;
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let (foreign_at, dyno_at) = (now + 6 * 3600, now + 7 * 3600);
    let offset = |t: i64| system_offset_at(t);
    // A wake that Dyno did not make.
    let foreign = tool("/usr/bin/pmset", &["schedule", "wake", &pmset_date(foreign_at, offset(foreign_at))]);
    assert!(SystemRunner.run_tool(&foreign, Duration::from_secs(15)).unwrap().ok, "needs root");
    let wakes = PmsetWakes::new(Box::new(SystemRunner), Box::new(offset));
    let tag = WakeTag::parse("dyno-1").unwrap();
    wakes.schedule(&tag, dyno_at).unwrap();
    let text = sched();
    println!("--- after schedule ---\n{text}");
    assert!(text.contains("net.decryptology.dyno.dyno-1"), "Dyno's event is set");
    let listed = wakes.list().unwrap();
    assert_eq!(listed.len(), 1, "only Dyno's event is listed: {listed:?}");
    assert_eq!((listed[0].tag.as_str(), listed[0].at_utc), ("dyno-1", dyno_at));
    assert!(wakes.cancel(&tag).unwrap());
    let text = sched();
    println!("--- after cancel ---\n{text}");
    assert!(!text.contains("net.decryptology.dyno"), "Dyno's event is gone");
    assert!(text.contains(&pmset_date(foreign_at, offset(foreign_at))[9..]), "the event Dyno did not make is still there");
    assert!(wakes.list().unwrap().is_empty());
    // Clean up what this test made.
    let cancel = tool("/usr/bin/pmset", &["schedule", "cancel", "wake", &pmset_date(foreign_at, offset(foreign_at))]);
    let _ = SystemRunner.run_tool(&cancel, Duration::from_secs(15));
}
