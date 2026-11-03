#![allow(clippy::unwrap_used, clippy::expect_used)]

//! "Is this normal?" learning across restarts and reboots (user report 2026-11-03: installed on Monday, after Tuesday's
//! boot the card still said "day 1 of 7"). The agent is restarted over the same database file each day, on a fake
//! clock, in a zone eight hours from UTC; the learning counts local calendar days since Dyno first saw the machine.

use dyno_agent::store::Store;
use dyno_core::baseline::{learning_status, BaselineSettings, Learning};
use dyno_core::time::{to_local, FixedZone, LocalZone, Timestamp, DAY, HOUR};

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
    let first = local(0, 20);
    // Installed Monday 2 November in the evening.
    assert_eq!(run_at(&db, first), learning(1, first), "Monday, the day of the install");
    // Laptop off overnight; booted Tuesday morning, less than 24 hours later.
    assert_eq!(
        run_at(&db, local(1, 8)),
        learning(2, first),
        "Tuesday after a boot is day 2, not day 1"
    );
    // Off again; Wednesday just after midnight local time (still Tuesday in UTC).
    assert_eq!(run_at(&db, local(2, 0)), learning(3, first), "Wednesday is day 3 of 7");
    assert_eq!(run_at(&db, local(6, 23)), learning(7, first));
}

#[test]
fn an_evening_install_waits_for_the_full_hours_on_the_last_day() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("dyno.db");
    let first = local(0, 20);
    run_at(&db, first);
    // Sunday is day 7; the next Monday the seven local days are complete, but only 6 days 4 hours have passed: the eighth
    // calendar day says so (day 8, past the period) until 20:00, so no day label lasts longer than a local day.
    assert_eq!(run_at(&db, local(6, 12)), learning(7, first));
    assert_eq!(run_at(&db, local(7, 0)), learning(8, first));
    assert_eq!(run_at(&db, local(7, 19)), learning(8, first));
    assert_eq!(run_at(&db, local(7, 20)), Learning::Ready { days: 7 });
}

#[test]
fn a_morning_install_waits_until_the_same_morning_a_week_later() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("dyno.db");
    let first = local(0, 8);
    assert_eq!(run_at(&db, first), learning(1, first));
    assert_eq!(run_at(&db, local(0, 23)), learning(1, first));
    assert_eq!(run_at(&db, local(6, 12)), learning(7, first));
    assert_eq!(
        run_at(&db, local(7, 7)),
        learning(8, first),
        "a week minus an hour, on the eighth calendar day"
    );
    assert_eq!(run_at(&db, local(7, 8)), Learning::Ready { days: 7 });
}

/// Central European time around the end of summer time 2026: UTC+2 until Sunday 25 October 01:00 UTC, UTC+1 after.
#[derive(Debug)]
struct Cet;
/// Sunday 2026-10-25 01:00 UTC.
const CET_SWITCH: i64 = MON_UTC - 8 * DAY + HOUR;
impl LocalZone for Cet {
    fn offset_minutes(&self, at: Timestamp) -> i32 {
        if at.0 < CET_SWITCH {
            120
        } else {
            60
        }
    }
}

#[test]
fn a_clock_change_moves_the_ready_time_by_the_hour_it_moved() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("dyno.db");
    // Saturday 24 October 20:00 local (UTC+2) = 18:00 UTC; the clocks go back on Sunday.
    let first = Timestamp(MON_UTC - 9 * DAY + 18 * HOUR);
    let status = |t: Timestamp| {
        let (store, _) = Store::open(&db, t).unwrap();
        let f = dyno_agent::baselines::on_start(&store, t);
        learning_status(&BaselineSettings::default(), f, t, &Cet)
    };
    assert!(matches!(status(first), Learning::Learning { day: 1, .. }));
    // Seven times 24 hours later is Saturday 31 October 18:00 UTC, 19:00 local (UTC+1), not 20:00.
    let ready = first.plus(7 * DAY);
    assert_eq!(to_local(ready, &Cet).rem_euclid(DAY), 19 * HOUR);
    // Saturday 31 October 00:30 local: seven local days complete, hours not yet.
    let sat = Timestamp(ready.0 - 19 * HOUR + 30 * dyno_core::time::MINUTE);
    assert_eq!(
        status(sat),
        Learning::Learning {
            day: 8,
            days_needed: 7,
            since: Some(first),
            ready_at: Some(ready)
        }
    );
    assert!(matches!(status(ready.minus(1)), Learning::Learning { day: 8, .. }));
    assert_eq!(status(ready), Learning::Ready { days: 7 });
}

#[test]
fn a_first_seen_time_in_the_future_or_unreadable_starts_from_now() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("dyno.db");
    let t = local(3, 9);
    let (store, _) = Store::open(&db, t).unwrap();
    // A clock that ran days ahead once (a wrong RTC, a bad sync) must not hold the learning back for those days.
    store.set_meta("firstSeen", &local(9, 9).millis().to_string()).unwrap();
    assert_eq!(dyno_agent::baselines::on_start(&store, t), Some(t));
    assert_eq!(store.first_seen(t).unwrap(), t, "rewritten, so it only ever moves earlier");
    // Less than a day ahead is ordinary clock jitter between runs: kept.
    store.set_meta("firstSeen", &t.plus(HOUR).millis().to_string()).unwrap();
    assert_eq!(dyno_agent::baselines::on_start(&store, t), Some(t.plus(HOUR)));
    store.set_meta("firstSeen", "not a time").unwrap();
    assert_eq!(dyno_agent::baselines::on_start(&store, t), Some(t));
    assert_eq!(store.first_seen(t).unwrap(), t);
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

#[test]
fn a_week_that_ends_with_the_clocks_set_back_waits_for_the_last_local_midnight() {
    // Monday 19 October 00:30 local (UTC+2) = Sunday 18 October 22:30 UTC.
    let first = Timestamp(CET_SWITCH - 6 * DAY - 2 * HOUR - 30 * dyno_core::time::MINUTE);
    let s = BaselineSettings::default();
    // 7 times 24 hours later is Sunday 25 October 23:30 local (UTC+1): the seventh day is not over yet.
    let hours = first.plus(7 * DAY);
    assert_eq!(to_local(hours, &Cet).rem_euclid(DAY), 23 * HOUR + 30 * dyno_core::time::MINUTE);
    let midnight = hours.plus(30 * dyno_core::time::MINUTE);
    assert_eq!(
        learning_status(&s, Some(first), hours, &Cet),
        Learning::Learning {
            day: 7,
            days_needed: 7,
            since: Some(first),
            ready_at: Some(midnight)
        }
    );
    assert_eq!(learning_status(&s, Some(first), midnight, &Cet), Learning::Ready { days: 7 });
}

/// Learning on `day`, first seen at `since`: ready a full week of hours after it.
fn learning(day: u32, since: Timestamp) -> Learning {
    Learning::Learning {
        day,
        days_needed: 7,
        since: Some(since),
        ready_at: Some(since.plus(7 * DAY)),
    }
}
