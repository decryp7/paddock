#![allow(clippy::unwrap_used, clippy::expect_used)]

//! "Is this normal?" learning across restarts and reboots (user report 2026-11-03: installed on Monday, after Tuesday's
//! boot the card still said "day 1 of 7"). The agent is restarted over the same database file each day, on a fake
//! clock, in a zone eight hours from UTC; the learning counts local calendar days since Dyno first saw the machine.

use dyno_agent::store::Store;
use dyno_core::baseline::{learning_status, BaselineSettings, Learning};
use dyno_core::time::{FixedZone, Timestamp, DAY, HOUR};

/// UTC+8: local midnight is 16:00 UTC the day before.
const ZONE: FixedZone = FixedZone(480);
/// Monday 2026-11-02 00:00 UTC.
const MON_UTC: i64 = 1_793_577_600_000;

/// Local wall time `day` days after Monday 2 November at `hour`:00, as an instant.
fn local(day: i64, hour: i64) -> Timestamp {
    Timestamp(MON_UTC + day * DAY + hour * HOUR - 8 * HOUR)
}

/// One agent run: opens the database, records the start, and says how far the learning is at `t`.
fn run_at(path: &std::path::Path, t: Timestamp) -> Learning {
    let (store, note) = Store::open(path, t).unwrap();
    assert!(note.is_none(), "{note:?}");
    let first = dyno_agent::baselines::on_start(&store, t);
    learning_status(&BaselineSettings::default(), first, t, &ZONE)
}

#[test]
fn learning_counts_local_days_across_restarts() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("dyno.db");
    // Installed Monday 2 November in the evening.
    let mon = run_at(&db, local(0, 20));
    assert_eq!(mon, learning(1, local(0, 20)), "Monday, the day of the install");
    // Laptop off overnight; booted Tuesday morning, less than 24 hours later.
    let tue = run_at(&db, local(1, 8));
    assert_eq!(tue, learning(2, local(0, 20)), "Tuesday after a boot is day 2, not day 1");
    // Off again; Wednesday just after midnight local time (still Tuesday in UTC).
    let wed = run_at(&db, local(2, 0));
    assert_eq!(wed, learning(3, local(0, 20)), "Wednesday is day 3 of 7");
    // The following Monday: Monday to Sunday are seven complete local days.
    assert_eq!(run_at(&db, local(6, 23)), learning(7, local(0, 20)));
    assert_eq!(run_at(&db, local(7, 0)), Learning::Ready { days: 7 });
}

#[test]
fn the_first_start_is_remembered_without_any_window_open() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("dyno.db");
    // An agent started at logon (Windows Run entry) with no app: the start itself is the first sighting.
    {
        let (store, _) = Store::open(&db, local(0, 9)).unwrap();
        assert_eq!(dyno_agent::baselines::on_start(&store, local(0, 9)), Some(local(0, 9)));
    }
    let (store, _) = Store::open(&db, local(3, 9)).unwrap();
    assert_eq!(store.first_seen(local(3, 9)).unwrap(), local(0, 9));
}

fn learning(day: u32, since: Timestamp) -> Learning {
    Learning::Learning {
        day,
        days_needed: 7,
        since: Some(since),
    }
}
