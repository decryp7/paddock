#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Sets a real wake timer on the machine it runs on and checks that Windows registered it and that it fires: only for the local Windows
//! test VM, as `paddock-admin` (`powercfg /waketimers` needs it): `PADDOCK_VM=1` and `--ignored`. Never in CI, never on a developer's
//! machine (ER-14). Prints what `powercfg /waketimers` says, for the lesson and the runbook.
#![cfg(windows)]

use dyno_core::wake::{WakeScheduler, WakeTag};
use dyno_platform::windows::wake_timer::WakeTimers;
use std::time::{Duration, Instant};

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn waketimers() -> String {
    let out = dyno_platform::process::command("powercfg").arg("/waketimers").output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Whether this machine has any sleep state a timer could wake it from. A VM with none (Fusion's Windows 11 Arm guest) never lists a
/// wake timer in `powercfg /waketimers`, however the timer was set.
fn can_sleep() -> bool {
    let out = dyno_platform::process::command("powercfg").arg("/a").output().unwrap();
    String::from_utf8_lossy(&out.stdout).contains("The following sleep states are available")
}

#[test]
#[ignore = "sets a real wake timer of this machine: the Windows test VM only, as an administrator (PADDOCK_VM=1)"]
fn a_wake_timer_is_registered_cancelled_and_fires() {
    if std::env::var("PADDOCK_VM").as_deref() != Ok("1") {
        eprintln!("skipped: PADDOCK_VM is not 1");
        return;
    }
    let exe = std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().into_owned();
    let timers = WakeTimers::default();
    let tag = WakeTag::parse("dyno-1").unwrap();
    let sleeps = can_sleep();
    println!("sleep states available: {sleeps}");
    println!("--- before ---\n{}", waketimers());
    timers.schedule(&tag, now_secs() + 600).unwrap();
    let text = waketimers();
    println!("--- after schedule (10 min) ---\n{text}");
    if sleeps {
        assert!(text.contains(&exe), "Windows lists a wake timer owned by this program ({exe})");
    } else {
        println!("no sleep state here: Windows lists no wake timers, so only Dyno's own bookkeeping is checked");
    }
    assert_eq!(timers.list().unwrap().len(), 1);
    assert!(timers.cancel(&tag).unwrap());
    let text = waketimers();
    println!("--- after cancel ---\n{text}");
    assert!(!text.contains(&exe), "the timer is gone after Cancel");
    assert!(timers.list().unwrap().is_empty());
    // A short one fires: its thread wakes, the entry leaves the list and the timer leaves Windows' list.
    timers.schedule(&tag, now_secs() + 20).unwrap();
    if sleeps {
        assert!(waketimers().contains(&exe));
    }
    let began = Instant::now();
    while !timers.list().unwrap().is_empty() {
        assert!(began.elapsed() < Duration::from_secs(120), "the timer never fired");
        std::thread::yield_now();
    }
    println!("fired after {:?}", began.elapsed());
    assert!(!waketimers().contains(&exe), "a fired timer is gone from Windows' list");
}
