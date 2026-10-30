#![allow(clippy::unwrap_used, clippy::expect_used)]

//! How long the Maintenance view's status takes (rule 48: measured, not assumed). Ignored by default; run it in release mode:
//!
//! ```sh
//! cargo test --release -p dyno-platform --test maintenance_bench -- --ignored --nocapture
//! ```
//!
//! The worst case: the most windows a user may set (14), all closed now, the next one opening almost a week later, so the
//! status has to look ahead the longest. Each zone lookup is a real one: the OS zone and a POSIX zone with daylight saving.

use dyno_core::extras::maintenance::{MaintenanceSettings, Weekday, Window, MAX_WINDOWS};
use dyno_core::time::{LocalZone, Timestamp, MINUTE};
use dyno_platform::zone::{SystemZone, TzZone};
use std::time::Instant;

/// 2026-10-26T10:05:00Z, a Monday, five minutes after every window closed.
const NOW: Timestamp = Timestamp(1_793_009_100_000);
const RUNS: u32 = 50;

fn settings() -> MaintenanceSettings {
    MaintenanceSettings {
        enabled: true,
        windows: (0..MAX_WINDOWS as u16)
            .map(|i| Window {
                days: vec![Weekday::Mon],
                start_minute: 9 * 60 + i,
                end_minute: 10 * 60,
            })
            .collect(),
    }
}

fn time(name: &str, zone: &dyn LocalZone) {
    let s = settings();
    let mut best = f64::MAX;
    let mut total = 0.0;
    let mut last = None;
    for _ in 0..RUNS {
        let t = Instant::now();
        let st = s.status(NOW, zone);
        let ms = t.elapsed().as_secs_f64() * 1_000.0;
        best = best.min(ms);
        total += ms;
        last = Some(st);
    }
    let st = last.unwrap();
    assert!(!st.open_now && st.next_opens_at.is_some());
    println!(
        "{name}: best {best:.3} ms, mean {:.3} ms over {RUNS} runs; opens again in {} minutes",
        total / f64::from(RUNS),
        st.next_opens_at.unwrap().since(NOW) / MINUTE
    );
}

#[test]
#[ignore = "a measurement; run it in release mode"]
fn maintenance_status_cost() {
    time("system zone", &SystemZone);
    time(
        "POSIX zone (CET with summer time)",
        &TzZone::posix("CET-1CEST,M3.5.0,M10.5.0/3").unwrap(),
    );
}
