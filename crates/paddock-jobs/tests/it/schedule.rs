//! Schedules (plan X33): the maths ported from Spotter's `schedule.rs` (every N, times of day, cron, zones, daylight
//! saving, minimum interval, jitter, wire format), plus catch-up after sleep and pause windows, all on a `FakeClock`.

use paddock_jobs::FakeClock;
use paddock_jobs::schedule::pause::{MAX_WINDOWS, PauseWindow, PauseWindows};
use paddock_jobs::schedule::*;
use std::time::Duration;

fn ts(text: &str) -> UnixMs {
    UnixMs::parse_rfc3339(text).unwrap()
}

fn t(h: u8, m: u8) -> ClockTime {
    ClockTime::new(h, m).unwrap()
}

fn berlin() -> LocalZone {
    LocalZone {
        name: Some("Europe/Berlin".into()),
        offset_minutes: 60,
    }
}

fn timing(min: u32) -> Timing {
    Timing {
        min_interval_secs: min,
        failures: 0,
        jitter: 0.0,
    }
}

fn at_times(days: &[Weekday], times: &[ClockTime], zone: Option<&str>) -> Schedule {
    Schedule::Times {
        days: days.to_vec(),
        times: times.to_vec(),
        zone: zone.map(Into::into),
    }
}

fn cron(expr: &str) -> Schedule {
    Schedule::Cron {
        expr: expr.into(),
        zone: None,
    }
}

/// The preview's runs as wall-clock text in `zone`.
fn walls(p: &SchedulePreview, zone: &str) -> Vec<String> {
    let z = zone_named(zone).unwrap();
    p.next_runs.iter().map(|r| wall_clock(*r, &z)).collect()
}

#[test]
fn intervals_jitter_and_back_off() {
    assert_eq!(next_delay_secs(60, 0, 0.5), 60);
    assert_eq!(next_delay_secs(60, 0, 0.0), 54);
    assert_eq!(next_delay_secs(60, 0, 1.0), 66);
    assert_eq!(next_delay_secs(60, 1, 0.5), 120);
    assert_eq!(next_delay_secs(60, 3, 0.5), 480);
    assert_eq!(next_delay_secs(60, 40, 0.5), u64::from(MAX_BACKOFF_SECS));
    assert_eq!(
        next_delay_secs(MAX_INTERVAL_SECS, 5, 0.5),
        u64::from(MAX_INTERVAL_SECS),
        "long intervals cap at themselves"
    );
    assert_eq!(
        next_delay_secs(1, 0, 0.0),
        u64::from(MIN_INTERVAL_SECS),
        "never below the minimum"
    );
    assert_eq!(next_delay_secs(60, 0, f64::NAN), 60);
    assert_eq!(next_delay_secs(60, 0, 7.0), 66);
    assert_eq!(clamp_interval(u32::MAX), MAX_INTERVAL_SECS);
}

#[test]
fn every_mode_clamps_to_the_source_minimum() {
    let every = |secs| Schedule::every(secs);
    let now = ts("2026-10-22T08:00:00Z");
    let mid = |min| Timing {
        jitter: 0.5,
        ..timing(min)
    };
    let next = |s: &Schedule, min| next_check(s, &berlin(), now, mid(min)).unwrap();
    assert_eq!(next(&every(900), 60), now.plus_secs(900));
    assert_eq!(
        next(&every(30), 60),
        now.plus_secs(60),
        "below the site minimum"
    );
    assert_eq!(next(&every(1), 10), now.plus_secs(10));
    assert_eq!(
        next(&every(u32::MAX), 10),
        now.plus_secs(u64::from(MAX_INTERVAL_SECS))
    );
    let jittery = Timing {
        jitter: 1.0,
        ..timing(60)
    };
    assert_eq!(
        next_check(&every(1000), &berlin(), now, jittery).unwrap(),
        now.plus_secs(1100)
    );
    let failing = Timing {
        failures: 2,
        jitter: 0.5,
        ..timing(60)
    };
    assert_eq!(
        next_check(&every(100), &berlin(), now, failing).unwrap(),
        now.plus_secs(400)
    );
    let p = preview(&every(30), &berlin(), now, 60).unwrap();
    assert!(
        p.warnings[0].contains("at most every minute"),
        "{:?}",
        p.warnings
    );
    assert_eq!(p.next_runs[0], now.plus_secs(60));
    let p = preview(&every(6 * 3600), &berlin(), now, 60).unwrap();
    assert_eq!(
        (p.description.as_str(), p.next_runs.len(), p.warnings.len()),
        ("Every 6 hours", 5, 0)
    );
    assert_eq!(p.next_runs[4], now.plus_secs(30 * 3600));
    assert_eq!(p.zone, "Europe/Berlin");
}

#[test]
fn descriptions_are_in_words() {
    assert_eq!(Schedule::every(900).describe(), "Every 15 minutes");
    assert_eq!(Schedule::every(3600).describe(), "Every hour");
    assert_eq!(Schedule::every(86_400).describe(), "Every day");
    assert_eq!(Schedule::every(2 * 86_400).describe(), "Every 2 days");
    assert_eq!(Schedule::every(60).describe(), "Every minute");
    assert_eq!(Schedule::every(90).describe(), "Every 90 seconds");
    let d = |days: &[Weekday], times: &[ClockTime], zone| at_times(days, times, zone).describe();
    assert_eq!(
        d(&Weekday::WEEKDAYS, &[t(14, 0), t(9, 0), t(9, 0)], None),
        "Weekdays at 09:00 and 14:00"
    );
    assert_eq!(
        d(&Weekday::ALL, &[t(7, 30)], Some("Asia/Tokyo")),
        "Every day at 07:30 (Asia/Tokyo)"
    );
    assert_eq!(
        d(&[Weekday::Sun, Weekday::Sat], &[t(8, 0)], None),
        "Weekends at 08:00"
    );
    assert_eq!(
        d(
            &[Weekday::Fri, Weekday::Mon, Weekday::Wed],
            &[t(6, 0), t(12, 0), t(18, 0)],
            None
        ),
        "Mon, Wed and Fri at 06:00, 12:00 and 18:00"
    );
    assert_eq!(d(&[Weekday::Tue], &[t(6, 0)], None), "Tue at 06:00");
    assert!(
        cron("0 9 * * 1-5").describe().contains("09:00"),
        "{}",
        cron("0 9 * * 1-5").describe()
    );
    let zoned = Schedule::Cron {
        expr: "0 9 * * *".into(),
        zone: Some("UTC".into()),
    };
    assert!(zoned.describe().ends_with("(UTC)"));
    assert_eq!(cron("nonsense").describe(), "Cron nonsense");
}

#[test]
fn at_times_runs_on_the_chosen_days_in_the_chosen_zone() {
    let s = at_times(&Weekday::WEEKDAYS, &[t(14, 0), t(9, 0)], None);
    // Thursday 2026-10-22 10:00 in Berlin (08:00Z): the next runs are 14:00, then Friday 09:00 and 14:00, then Monday.
    let p = preview(&s, &berlin(), ts("2026-10-22T08:00:00Z"), 60).unwrap();
    assert_eq!(
        walls(&p, "Europe/Berlin"),
        [
            "2026-10-22 14:00",
            "2026-10-23 09:00",
            "2026-10-23 14:00",
            "2026-10-26 09:00",
            "2026-10-26 14:00"
        ]
    );
    assert!(p.warnings.is_empty());
    // A chosen zone wins over the machine's.
    let tokyo = at_times(&Weekday::ALL, &[t(9, 0)], Some("Asia/Tokyo"));
    let p = preview(&tokyo, &berlin(), ts("2026-10-22T08:00:00Z"), 60).unwrap();
    assert_eq!(p.next_runs[0], ts("2026-10-23T00:00:00Z"));
    assert_eq!(p.zone, "Asia/Tokyo");
    // With only an offset (no zone name) the machine's offset is used.
    let offset = LocalZone {
        name: None,
        offset_minutes: -330,
    };
    let p = preview(
        &at_times(&Weekday::ALL, &[t(9, 0)], None),
        &offset,
        ts("2026-10-22T08:00:00Z"),
        60,
    )
    .unwrap();
    assert_eq!(
        (p.next_runs[0], p.zone.as_str()),
        (ts("2026-10-22T14:30:00Z"), "UTC-05:30")
    );
    let east = LocalZone {
        name: Some("Not/AZone".into()),
        offset_minutes: 120,
    };
    assert_eq!(zone_label(None, &east), "UTC+02:00");
    // A weekday that is only seven days away.
    let sundays = at_times(&[Weekday::Thu], &[t(8, 0)], Some("UTC"));
    assert_eq!(
        next_check(&sundays, &berlin(), ts("2026-10-22T08:00:00Z"), timing(60)).unwrap(),
        ts("2026-10-29T08:00:00Z")
    );
}

#[test]
fn at_times_follow_daylight_saving() {
    let daily = |h, m| at_times(&Weekday::ALL, &[t(h, m)], Some("Europe/Berlin"));
    let runs = |s: &Schedule, now: &str| {
        walls(
            &preview(s, &LocalZone::default(), ts(now), 60).unwrap(),
            "Europe/Berlin",
        )
    };
    // Spring forward 2026-03-29: 02:00 CET becomes 03:00 CEST, so 02:30 does not exist and runs once, shifted.
    let spring = runs(&daily(2, 30), "2026-03-27T12:00:00Z");
    assert_eq!(
        &spring[..3],
        ["2026-03-28 02:30", "2026-03-29 03:30", "2026-03-30 02:30"]
    );
    // 09:00 stays 09:00 on the wall clock across the change (an hour earlier in UTC).
    let p = preview(
        &daily(9, 0),
        &LocalZone::default(),
        ts("2026-03-27T12:00:00Z"),
        60,
    )
    .unwrap();
    assert_eq!(
        (p.next_runs[0], p.next_runs[1]),
        (ts("2026-03-28T08:00:00Z"), ts("2026-03-29T07:00:00Z"))
    );
    // Fall back 2026-10-25: 03:00 CEST becomes 02:00 CET, so 02:30 happens twice and runs once, the first time.
    let fall = daily(2, 30);
    let first = next_check(
        &fall,
        &LocalZone::default(),
        ts("2026-10-24T12:00:00Z"),
        timing(60),
    )
    .unwrap();
    assert_eq!(first, ts("2026-10-25T00:30:00Z"), "02:30 CEST");
    let after_first = next_check(&fall, &LocalZone::default(), first, timing(60)).unwrap();
    assert_eq!(
        after_first,
        ts("2026-10-26T01:30:00Z"),
        "the repeated 02:30 CET is skipped"
    );
    // The clocks-go-back day has 25 hours: 09:00 CET is 25 hours after 09:00 CEST the day before.
    let nine = daily(9, 0);
    let a = next_check(
        &nine,
        &LocalZone::default(),
        ts("2026-10-23T08:00:00Z"),
        timing(60),
    )
    .unwrap();
    let b = next_check(&nine, &LocalZone::default(), a, timing(60)).unwrap();
    assert_eq!((b.0 - a.0) / 3_600_000, 25);
}

#[test]
fn cron_follows_daylight_saving() {
    let zoned = |expr: &str| Schedule::Cron {
        expr: expr.into(),
        zone: Some("Europe/Berlin".into()),
    };
    let go = |expr: &str, now: &str| {
        walls(
            &preview(&zoned(expr), &LocalZone::default(), ts(now), 60).unwrap(),
            "Europe/Berlin",
        )
    };
    // A time inside the spring gap runs once, at the first instant after it.
    let spring = go("30 2 * * *", "2026-03-27T12:00:00Z");
    assert_eq!(
        &spring[..3],
        ["2026-03-28 02:30", "2026-03-29 03:00", "2026-03-30 02:30"]
    );
    // A repeated time runs once.
    let fall = go("30 2 * * *", "2026-10-24T12:00:00Z");
    assert_eq!(
        &fall[..3],
        ["2026-10-25 02:30", "2026-10-26 02:30", "2026-10-27 02:30"]
    );
    let utc: Vec<_> = preview(
        &zoned("30 2 * * *"),
        &LocalZone::default(),
        ts("2026-10-24T12:00:00Z"),
        60,
    )
    .unwrap()
    .next_runs;
    assert_eq!(utc[0], ts("2026-10-25T00:30:00Z"));
    assert_eq!(utc[1], ts("2026-10-26T01:30:00Z"));
}

#[test]
fn cron_parses_five_or_six_fields_and_says_what_is_wrong() {
    let local = berlin();
    let now = ts("2026-10-22T08:00:00Z");
    let p = preview(&cron("0 9 * * 1-5"), &local, now, 60).unwrap();
    assert_eq!(walls(&p, "Europe/Berlin")[0], "2026-10-23 09:00");
    assert_eq!(walls(&p, "Europe/Berlin")[2], "2026-10-27 09:00");
    let with_seconds = preview(&cron("30 0 9 * * *"), &local, now, 60).unwrap();
    assert_eq!(with_seconds.next_runs[0], ts("2026-10-23T07:00:30Z"));
    assert!(preview(&cron("@daily"), &local, now, 60).is_ok());
    let err = |s: &Schedule| s.check().unwrap_err().join(" ");
    assert!(err(&cron("")).contains("Enter a cron expression"));
    assert!(err(&cron("   ")).contains("Enter a cron expression"));
    assert!(err(&cron("* * *")).contains("five fields") && err(&cron("* * *")).contains("has 3"));
    assert!(err(&cron("* * * * * * *")).contains("has 7"));
    assert!(
        err(&cron("61 * * * *")).contains("not a valid cron expression"),
        "{}",
        err(&cron("61 * * * *"))
    );
    assert!(err(&cron("a b c d e")).contains("not a valid cron expression"));
    assert!(err(&cron(&"*/5 ".repeat(100))).contains("too long"));
    assert!(err(&cron("0 0 30 2 *")).contains("never matches"));
    let bad_zone = Schedule::Cron {
        expr: "0 9 * * *".into(),
        zone: Some("Mars/Base".into()),
    };
    assert!(err(&bad_zone).contains("Unknown time zone \"Mars/Base\""));
    let both = Schedule::Cron {
        expr: "x".into(),
        zone: Some("Mars/Base".into()),
    };
    assert_eq!(
        both.check().unwrap_err().len(),
        2,
        "every problem is listed"
    );
    assert!(cron("0 9 * * *").check().is_ok());
}

#[test]
fn at_times_say_what_is_missing() {
    let err = |s: &Schedule| s.check().unwrap_err();
    assert_eq!(
        err(&at_times(&[], &[t(9, 0)], None)),
        ["Pick at least one day."]
    );
    assert_eq!(
        err(&at_times(&Weekday::ALL, &[], None)),
        ["Add at least one time of day."]
    );
    assert_eq!(err(&at_times(&[], &[], None)).len(), 2);
    let many: Vec<ClockTime> = (0..25).map(|m| t(1, m)).collect();
    assert!(err(&at_times(&Weekday::ALL, &many, None))[0].contains("at most 24"));
    for bad in ["", "Berlin", "../etc", &"a/".repeat(40)] {
        let zone = if bad.is_empty() {
            Some("   ")
        } else {
            Some(bad)
        };
        let r = at_times(&Weekday::ALL, &[t(9, 0)], zone).check();
        // An empty zone means "the machine's"; anything else must be a real IANA name.
        assert_eq!(r.is_err(), !bad.is_empty(), "{bad:?}");
    }
    assert!(Weekday::ALL.iter().all(|d| d.short().len() == 3));
}

#[test]
fn too_frequent_schedules_are_clamped_with_a_warning() {
    let now = ts("2026-10-22T08:00:00Z");
    // Every minute against a five-minute minimum.
    let p = preview(&cron("* * * * *"), &berlin(), now, 300).unwrap();
    assert!(
        p.warnings[0].contains("every minute")
            && p.warnings[0].contains("clamped to at most once every 5 minutes"),
        "{:?}",
        p.warnings
    );
    let gaps: Vec<i64> = p
        .next_runs
        .windows(2)
        .map(|w| (w[1].0 - w[0].0) / 1000)
        .collect();
    assert_eq!(gaps, [300, 300, 300, 300]);
    // Every second (six fields).
    let p = preview(&cron("* * * * * *"), &berlin(), now, 60).unwrap();
    assert!(p.warnings[0].contains("every second"), "{:?}", p.warnings);
    assert!(p.next_runs.windows(2).all(|w| w[1].0 - w[0].0 >= 60_000));
    // Two times of day closer than the minimum.
    let close = at_times(&Weekday::ALL, &[t(9, 0), t(9, 2)], Some("UTC"));
    let p = preview(&close, &berlin(), now, 600).unwrap();
    assert_eq!(p.warnings.len(), 1);
    assert_eq!(
        wall_clock(p.next_runs[0], &zone_named("UTC").unwrap()),
        "2026-10-22 09:00"
    );
    assert_eq!(
        wall_clock(p.next_runs[1], &zone_named("UTC").unwrap()),
        "2026-10-23 09:00",
        "09:02 is skipped, not run late"
    );
    // And in the real schedule: the next check is at least the minimum after the last one.
    let after = ts("2026-10-22T08:00:10Z");
    let next = next_check(&cron("* * * * *"), &berlin(), after, timing(300)).unwrap();
    assert_eq!(next, ts("2026-10-22T08:06:00Z"));
    assert!(next.0 - after.0 >= 300_000);
    assert!(
        next.0 - after.0 < 360_000,
        "the first run at or after the minimum"
    );
    // A schedule that is fine has no warning.
    assert!(
        preview(&cron("0 * * * *"), &berlin(), now, 300)
            .unwrap()
            .warnings
            .is_empty()
    );
}

#[test]
fn jitter_is_deterministic_and_small() {
    let s = at_times(&Weekday::ALL, &[t(9, 0)], Some("UTC"));
    let now = ts("2026-10-22T08:00:00Z");
    let at = |jitter| {
        next_check(
            &s,
            &berlin(),
            now,
            Timing {
                jitter,
                ..timing(60)
            },
        )
        .unwrap()
    };
    assert_eq!(at(0.0), ts("2026-10-22T09:00:00Z"));
    assert_eq!(at(0.5), ts("2026-10-22T09:00:15Z"));
    assert_eq!(at(0.5), at(0.5), "same seed, same answer");
    assert_eq!(at(1.0), ts("2026-10-22T09:00:30Z"));
    assert_eq!(at(9.0), at(1.0), "out-of-range jitter is clamped");
    assert_eq!(at(f64::NAN), at(0.5));
    // Two watches on the same host get different times when their random numbers differ.
    assert_ne!(at(0.1), at(0.9));
}

#[test]
fn a_machine_that_slept_gets_one_catch_up_check() {
    let s = at_times(&Weekday::ALL, &[t(9, 0), t(10, 0), t(11, 0)], Some("UTC"));
    // The last check finished at 08:00; the laptop slept until 12:30, missing three runs.
    let woke = ts("2026-10-22T12:30:00Z");
    let next = next_check(&s, &berlin(), woke, timing(60)).unwrap();
    assert_eq!(
        next,
        ts("2026-10-23T09:00:00Z"),
        "the next check lies after the wake time: no burst of three"
    );
    let every = next_check(
        &Schedule::every(900),
        &berlin(),
        woke,
        Timing {
            jitter: 0.5,
            ..timing(60)
        },
    )
    .unwrap();
    assert_eq!(every, woke.plus_secs(900));
    let c = next_check(&cron("*/15 * * * *"), &berlin(), woke, timing(60)).unwrap();
    assert_eq!(c, ts("2026-10-22T12:45:00Z"));
}

#[test]
fn schedules_use_a_tagged_wire_format() {
    let json =
        serde_json::to_string(&at_times(&[Weekday::Mon, Weekday::Fri], &[t(9, 5)], None)).unwrap();
    assert_eq!(
        json,
        r#"{"mode":"times","days":["mon","fri"],"times":["09:05"],"zone":null}"#
    );
    let back: Schedule = serde_json::from_str(r#"{"mode":"cron","expr":"0 9 * * *"}"#).unwrap();
    assert_eq!(back, cron("0 9 * * *"));
    assert_eq!(
        serde_json::to_string(&Schedule::every(60)).unwrap(),
        r#"{"mode":"every","secs":60}"#
    );
    assert!(serde_json::from_str::<Schedule>(r#"{"mode":"hourly"}"#).is_err());
    assert_eq!(Schedule::default(), Schedule::every(300));
}

#[test]
fn calendar_helpers_cover_their_edges() {
    assert!(parse_cron("0 9 * * *").is_ok());
    assert!(zone_named("UTC").is_ok());
    assert!(zone_named(" Europe/Berlin ").is_ok());
    let far = LocalZone {
        name: None,
        offset_minutes: 100_000,
    };
    assert!(
        resolve_zone(None, &far).is_ok(),
        "an absurd offset is clamped, not an error"
    );

    assert_eq!(
        wall_clock(UnixMs(i64::MAX), &zone_named("UTC").unwrap()),
        ""
    );
}

/// A fake clock whose calendar time is the given RFC 3339 instant.
fn clock_at(text: &str) -> FakeClock {
    FakeClock::at_wall_ms(ts(text).0)
}

fn now_of(clock: &FakeClock) -> UnixMs {
    UnixMs::from_clock(clock)
}

fn utc_times(times: &[ClockTime]) -> Schedule {
    at_times(&Weekday::ALL, times, Some("UTC"))
}

#[test]
fn the_clock_gives_the_instant_and_text_parses_strictly() {
    let clock = clock_at("2026-10-22T08:00:00Z");
    assert_eq!(now_of(&clock), ts("2026-10-22T08:00:00Z"));
    clock.advance(Duration::from_secs(90));
    assert_eq!(
        now_of(&clock),
        ts("2026-10-22T08:01:30Z"),
        "the instant follows the clock, never the wall"
    );
    assert_eq!(
        UnixMs::parse_rfc3339("2026-10-22T10:00:00+02:00"),
        Some(ts("2026-10-22T08:00:00Z"))
    );
    for bad in ["", "   ", "yesterday", &"9".repeat(80)] {
        assert_eq!(UnixMs::parse_rfc3339(bad), None, "{bad:?}");
    }
    // Before 1970 reads as 1970.
    assert_eq!(
        UnixMs::from_clock(&FakeClock::at_wall(
            std::time::UNIX_EPOCH - Duration::from_secs(5)
        )),
        UnixMs(0)
    );
    assert_eq!(UnixMs(i64::MAX).plus_secs(5), UnixMs(i64::MAX));
}

#[test]
fn times_of_day_read_and_write_hh_mm() {
    assert_eq!(ClockTime::new(9, 5).unwrap().minutes(), 545);
    assert_eq!(ClockTime::from_minutes(1439), ClockTime::new(23, 59));
    assert_eq!(
        (
            ClockTime::new(24, 0),
            ClockTime::new(0, 60),
            ClockTime::from_minutes(1440)
        ),
        (None, None, None)
    );
    let json = serde_json::to_string(&t(7, 3)).unwrap();
    assert_eq!(json, "\"07:03\"");
    assert_eq!(
        serde_json::from_str::<ClockTime>(" \"7:03\" ").unwrap(),
        t(7, 3)
    );
    for bad in [
        "\"0903\"",
        "\"09:3\"",
        "\":30\"",
        "\"ab:cd\"",
        "\"24:00\"",
        "\"09:60\"",
    ] {
        let e = serde_json::from_str::<ClockTime>(bad)
            .unwrap_err()
            .to_string();
        assert!(e.contains("not a time of day"), "{bad}: {e}");
    }
}

#[test]
fn a_laptop_that_slept_catches_up_by_policy() {
    // Runs at 09:00, 10:00 and 11:00 UTC; the last run finished at 08:30 and the laptop woke at 12:30.
    let s = utc_times(&[t(9, 0), t(10, 0), t(11, 0)]);
    let clock = clock_at("2026-10-22T08:30:00Z");
    let last = now_of(&clock);
    clock.advance(Duration::from_secs(4 * 3600));
    let woke = now_of(&clock);
    assert_eq!(woke, ts("2026-10-22T12:30:00Z"));
    let go = |policy| due(&s, &berlin(), last, woke, policy).unwrap();
    let skip = go(CatchUp::Skip);
    assert_eq!(
        (skip.missed, skip.run_now, skip.next),
        (3, 0, ts("2026-10-23T09:00:00Z"))
    );
    let once = go(CatchUp::Once);
    assert_eq!((once.missed, once.run_now), (3, 1), "one run for all three");
    assert_eq!(go(CatchUp::default()), once);
    assert_eq!(go(CatchUp::All { max: 2 }).run_now, 2);
    assert_eq!(go(CatchUp::All { max: 10 }).run_now, 3);
    assert_eq!(go(CatchUp::All { max: 0 }).run_now, 0);
    // Nothing missed: nothing to run, and the next run is still ahead.
    let on_time = due(
        &s,
        &berlin(),
        ts("2026-10-22T09:00:00Z"),
        ts("2026-10-22T09:30:00Z"),
        CatchUp::Once,
    )
    .unwrap();
    assert_eq!(
        (on_time.missed, on_time.run_now, on_time.next),
        (0, 0, ts("2026-10-22T10:00:00Z"))
    );
}

#[test]
fn catch_up_for_intervals_counts_whole_intervals() {
    let every = Schedule::every(900);
    let d = |elapsed_secs: u64, policy| {
        let last = ts("2026-10-22T08:00:00Z");
        due(
            &every,
            &berlin(),
            last,
            last.plus_secs(elapsed_secs),
            policy,
        )
        .unwrap()
    };
    assert_eq!(
        (d(899, CatchUp::Once).missed, d(900, CatchUp::Once).missed),
        (0, 1)
    );
    let slept = d(3 * 3600 + 100, CatchUp::All { max: 5 });
    assert_eq!((slept.missed, slept.run_now), (12, 5));
    assert_eq!(
        slept.next,
        ts("2026-10-22T08:00:00Z").plus_secs(3 * 3600 + 100 + 900),
        "counted from the wake time"
    );
    // A clock set back before the last run counts as nothing missed; a huge gap is capped.
    let back = due(
        &every,
        &berlin(),
        ts("2026-10-22T08:00:00Z"),
        ts("2026-10-22T07:00:00Z"),
        CatchUp::Once,
    )
    .unwrap();
    assert_eq!(back.missed, 0);
    let years = due(
        &every,
        &berlin(),
        UnixMs(0),
        ts("2026-10-22T08:00:00Z"),
        CatchUp::All { max: u32::MAX },
    )
    .unwrap();
    assert_eq!((years.missed, years.run_now), (MAX_MISSED, MAX_MISSED));
    // An interval below the minimum counts as the minimum.
    let fast = due(
        &Schedule::every(1),
        &berlin(),
        UnixMs(0),
        UnixMs(35_000),
        CatchUp::Once,
    )
    .unwrap();
    assert_eq!(fast.missed, 3);
}

#[test]
fn catch_up_counts_daylight_saving_days_once() {
    let daily_berlin = at_times(&Weekday::ALL, &[t(2, 30)], Some("Europe/Berlin"));
    // Asleep from Saturday 2026-03-28 03:00 to Monday noon, across the spring-forward night: 2 runs (Sun 03:30, Mon 02:30).
    let last = ts("2026-03-28T02:00:00Z");
    let woke = ts("2026-03-30T10:00:00Z");
    let spring = due(
        &daily_berlin,
        &berlin(),
        last,
        woke,
        CatchUp::All { max: 10 },
    )
    .unwrap();
    assert_eq!((spring.missed, spring.run_now), (2, 2));
    // Across the fall-back night 2026-10-25 (02:30 happens twice): Sunday's run counts once.
    let fall = due(
        &daily_berlin,
        &berlin(),
        ts("2026-10-24T12:00:00Z"),
        ts("2026-10-25T12:00:00Z"),
        CatchUp::All { max: 10 },
    )
    .unwrap();
    assert_eq!(fall.missed, 1);
    assert_eq!(fall.next, ts("2026-10-26T01:30:00Z"));
    let c = Schedule::Cron {
        expr: "30 2 * * *".into(),
        zone: Some("Europe/Berlin".into()),
    };
    assert_eq!(
        due(
            &c,
            &berlin(),
            ts("2026-10-24T12:00:00Z"),
            ts("2026-10-26T12:00:00Z"),
            CatchUp::Once
        )
        .unwrap()
        .missed,
        2
    );
    let e = due(
        &cron("30 0 30 2 *"),
        &berlin(),
        UnixMs(0),
        UnixMs(1),
        CatchUp::Once,
    )
    .unwrap_err();
    assert!(e[0].contains("never matches"));
}

#[test]
fn due_follows_the_fake_clock_through_a_sleep() {
    let s = utc_times(&[t(9, 0)]);
    let clock = clock_at("2026-10-22T09:00:00Z");
    let mut last = now_of(&clock);
    for day in 1..=3 {
        clock.advance(Duration::from_secs(86_400));
        let d = due(&s, &berlin(), last, now_of(&clock), CatchUp::Once).unwrap();
        assert_eq!((d.missed, d.run_now), (1, 1), "day {day}");
        last = now_of(&clock);
    }
    // Three days of sleep: one run, not three.
    clock.advance(Duration::from_secs(3 * 86_400));
    let d = due(&s, &berlin(), last, now_of(&clock), CatchUp::Once).unwrap();
    assert_eq!((d.missed, d.run_now), (3, 1));
}

#[test]
fn due_and_catch_up_use_a_tagged_wire_format() {
    assert_eq!(
        serde_json::to_string(&CatchUp::Skip).unwrap(),
        r#"{"policy":"skip"}"#
    );
    assert_eq!(
        serde_json::to_string(&CatchUp::All { max: 3 }).unwrap(),
        r#"{"policy":"all","max":3}"#
    );
    assert_eq!(
        serde_json::from_str::<CatchUp>(r#"{"policy":"once"}"#).unwrap(),
        CatchUp::Once
    );
    let d = Due {
        missed: 2,
        run_now: 1,
        next: UnixMs(5),
    };
    assert_eq!(
        serde_json::to_string(&d).unwrap(),
        r#"{"missed":2,"runNow":1,"next":5}"#
    );
    let p = preview(
        &Schedule::every(3600),
        &berlin(),
        ts("2026-10-22T08:00:00Z"),
        60,
    )
    .unwrap();
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["description"], "Every hour");
    assert_eq!(json["nextRuns"].as_array().unwrap().len(), 5);
    assert_eq!(serde_json::from_value::<SchedulePreview>(json).unwrap(), p);
}

fn night() -> PauseWindows {
    PauseWindows {
        windows: vec![PauseWindow::daily(t(22, 0), t(6, 0))],
        zone: Some("Europe/Berlin".into()),
    }
}

#[test]
fn a_window_spanning_midnight_is_open_on_both_sides() {
    let w = night();
    let holds = |text: &str| w.holds(&LocalZone::default(), ts(text)).unwrap();
    // Berlin is UTC+2 in October (summer time) before the 25th.
    assert!(!holds("2026-10-22T19:59:00Z"), "21:59");
    assert!(holds("2026-10-22T20:00:00Z"), "22:00");
    assert!(holds("2026-10-23T03:59:00Z"), "05:59 next morning");
    assert!(
        !holds("2026-10-23T04:00:00Z"),
        "06:00 is closed (end is exclusive)"
    );
    let clock = clock_at("2026-10-22T20:00:00Z");
    assert_eq!(
        w.resume_at(&LocalZone::default(), now_of(&clock)).unwrap(),
        Some(ts("2026-10-23T04:00:00Z"))
    );
    clock.advance(Duration::from_secs(8 * 3600));
    assert_eq!(
        w.resume_at(&LocalZone::default(), now_of(&clock)).unwrap(),
        None,
        "already closed"
    );
    assert_eq!(
        w.next_open(&LocalZone::default(), now_of(&clock)).unwrap(),
        Some(ts("2026-10-23T20:00:00Z"))
    );
    assert_eq!(
        w.next_open(&LocalZone::default(), ts("2026-10-22T21:00:00Z"))
            .unwrap(),
        None,
        "open now"
    );
}

#[test]
fn windows_belong_to_the_day_they_start_on() {
    // Friday 22:00 to 06:00: open Saturday morning, not Friday morning.
    let w = PauseWindows {
        windows: vec![PauseWindow {
            days: vec![Weekday::Fri],
            start: t(22, 0),
            end: t(6, 0),
        }],
        zone: Some("UTC".into()),
    };
    let holds = |text: &str| w.holds(&LocalZone::default(), ts(text)).unwrap();
    assert!(!holds("2026-10-23T05:00:00Z"), "Friday 05:00");
    assert!(holds("2026-10-23T23:00:00Z"), "Friday 23:00");
    assert!(holds("2026-10-24T05:00:00Z"), "Saturday 05:00");
    assert!(!holds("2026-10-24T23:00:00Z"), "Saturday 23:00");
    assert_eq!(
        w.next_open(&LocalZone::default(), ts("2026-10-22T12:00:00Z"))
            .unwrap(),
        Some(ts("2026-10-23T22:00:00Z"))
    );
    // A window inside one day, on weekdays.
    let lunch = PauseWindows {
        windows: vec![PauseWindow {
            days: Weekday::WEEKDAYS.to_vec(),
            start: t(12, 0),
            end: t(13, 0),
        }],
        zone: Some("UTC".into()),
    };
    assert!(
        lunch
            .holds(&LocalZone::default(), ts("2026-10-22T12:30:00Z"))
            .unwrap()
    );
    assert!(
        !lunch
            .holds(&LocalZone::default(), ts("2026-10-24T12:30:00Z"))
            .unwrap(),
        "Saturday"
    );
    assert_eq!(
        lunch
            .next_open(&LocalZone::default(), ts("2026-10-23T13:00:00Z"))
            .unwrap(),
        Some(ts("2026-10-26T12:00:00Z"))
    );
}

#[test]
fn windows_follow_daylight_saving() {
    // 22:00 to 06:00 in Berlin on the night the clocks go back (2026-10-25): the window is nine hours long, not eight.
    let w = night();
    let local = LocalZone::default();
    let opens = ts("2026-10-24T20:00:00Z"); // 22:00 CEST
    assert!(w.holds(&local, opens).unwrap());
    assert_eq!(
        w.resume_at(&local, opens).unwrap(),
        Some(ts("2026-10-25T05:00:00Z")),
        "06:00 CET"
    );
    // And on the night they go forward (2026-03-29) it is seven hours.
    assert_eq!(
        w.resume_at(&local, ts("2026-03-28T21:00:00Z")).unwrap(),
        Some(ts("2026-03-29T04:00:00Z")),
        "06:00 CEST"
    );
    // A window ending inside the spring gap (02:30 does not exist) closes at the shifted time.
    let gap = PauseWindows {
        windows: vec![PauseWindow::daily(t(1, 0), t(2, 30))],
        zone: Some("Europe/Berlin".into()),
    };
    assert_eq!(
        gap.resume_at(&local, ts("2026-03-29T00:30:00Z")).unwrap(),
        Some(ts("2026-03-29T01:30:00Z")),
        "03:30 CEST"
    );
}

#[test]
fn overlapping_and_all_day_windows_resume_at_the_last_end() {
    let overlap = PauseWindows {
        windows: vec![
            PauseWindow::daily(t(8, 0), t(10, 0)),
            PauseWindow::daily(t(9, 0), t(12, 0)),
        ],
        zone: Some("UTC".into()),
    };
    let local = LocalZone::default();
    assert_eq!(
        overlap
            .resume_at(&local, ts("2026-10-22T08:30:00Z"))
            .unwrap(),
        Some(ts("2026-10-22T12:00:00Z"))
    );
    // Back-to-back windows chain; windows that cover the whole day never close.
    let chain = PauseWindows {
        windows: vec![
            PauseWindow::daily(t(8, 0), t(10, 0)),
            PauseWindow::daily(t(10, 0), t(11, 0)),
        ],
        zone: Some("UTC".into()),
    };
    assert_eq!(
        chain.resume_at(&local, ts("2026-10-22T08:30:00Z")).unwrap(),
        Some(ts("2026-10-22T11:00:00Z"))
    );
    let all_day = PauseWindows {
        windows: vec![
            PauseWindow::daily(t(0, 0), t(12, 0)),
            PauseWindow::daily(t(12, 0), t(0, 0)),
        ],
        zone: Some("UTC".into()),
    };
    assert!(all_day.holds(&local, ts("2026-10-22T03:00:00Z")).unwrap());
    assert_eq!(
        all_day
            .resume_at(&local, ts("2026-10-22T03:00:00Z"))
            .unwrap(),
        None,
        "never closes"
    );
    assert_eq!(
        PauseWindows::default()
            .resume_at(&local, UnixMs(0))
            .unwrap(),
        None
    );
    assert_eq!(
        PauseWindows::default()
            .next_open(&local, UnixMs(0))
            .unwrap(),
        None,
        "no windows: never opens"
    );
    assert!(!PauseWindows::default().holds(&local, UnixMs(0)).unwrap());
    // Out-of-range instants are closed, not a panic.
    assert!(!night().holds(&local, UnixMs(i64::MAX)).unwrap());
}

#[test]
fn a_set_of_windows_says_what_to_fix() {
    let local = LocalZone::default();
    assert!(night().check().is_ok());
    let empty = PauseWindows {
        windows: vec![PauseWindow::daily(t(9, 0), t(9, 0))],
        zone: None,
    };
    assert!(empty.check().unwrap_err()[0].contains("differs"));
    let many = PauseWindows {
        windows: vec![PauseWindow::daily(t(1, 0), t(2, 0)); MAX_WINDOWS + 1],
        zone: None,
    };
    assert!(many.check().unwrap_err()[0].contains("at most 14"));
    let bad = PauseWindows {
        windows: vec![],
        zone: Some("Mars/Base".into()),
    };
    assert!(bad.holds(&local, UnixMs(0)).unwrap_err()[0].contains("Unknown time zone"));
    let long = PauseWindows {
        windows: vec![],
        zone: Some("a".repeat(100)),
    };
    assert!(long.check().unwrap_err()[0].contains("too long"));
    assert!(bad.resume_at(&local, UnixMs(0)).is_err() && bad.next_open(&local, UnixMs(0)).is_err());
    // The machine's own zone (an offset only) works too.
    let own = PauseWindows {
        windows: vec![PauseWindow::daily(t(22, 0), t(6, 0))],
        zone: None,
    };
    let east = LocalZone {
        name: None,
        offset_minutes: 120,
    };
    assert!(own.holds(&east, ts("2026-10-22T20:30:00Z")).unwrap());
    let json = serde_json::to_string(&night()).unwrap();
    assert_eq!(
        json,
        r#"{"windows":[{"days":[],"start":"22:00","end":"06:00"}],"zone":"Europe/Berlin"}"#
    );
    assert_eq!(
        serde_json::from_str::<PauseWindows>("{}").unwrap(),
        PauseWindows::default()
    );
}

#[test]
fn a_window_with_equal_ends_is_empty_and_calendar_schedules_back_off() {
    let empty = PauseWindow::daily(t(9, 0), t(9, 0));
    assert!(!empty.contains(Weekday::Mon, t(9, 0)));
    assert!(!empty.contains(Weekday::Mon, t(0, 0)));
    // After failures a calendar schedule waits at least the backed-off delay before its next time.
    let s = utc_times(&[t(9, 0), t(9, 30), t(10, 0), t(10, 30)]);
    let failing = Timing {
        failures: 2,
        jitter: 0.5,
        ..timing(60)
    };
    // 60 s doubled twice is 240 s: the run at 09:00 is too soon, 09:30 is the first allowed, plus half of the 30 s jitter.
    let at = ts("2026-10-22T08:59:00Z");
    assert_eq!(
        next_check(&s, &berlin(), at, failing).unwrap(),
        ts("2026-10-22T09:30:15Z")
    );
}
