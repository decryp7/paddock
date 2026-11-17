//! The machine's time zone as the assumed zone of timestamps that carry no offset: a time inside a skipped hour is not a
//! time, a repeated hour reads as its first occurrence. The only test of this binary that reads the machine's zone, which is
//! read from `TZ` once per process. Unix only (the module is declared under `cfg(unix)`): Windows takes its zone from the
//! system settings, not from `TZ`.

use debrief_plugin_api::time::{utc_epoch_ms, TimestampParser};

#[test]
fn a_skipped_hour_is_not_a_time_and_a_repeated_hour_reads_as_the_first() {
    // Central European time with the EU's summer time rules, as a POSIX rule so the test needs no zone database.
    let mut env = paddock_testkit::env_lock();
    env.set("TZ", "CET-1CEST,M3.5.0,M10.5.0/3");
    let p = TimestampParser::new(&[], "local", None).unwrap();
    let utc = |y, mo, d, h, mi| utc_epoch_ms(y, mo, d, h, mi, 0, 0);
    // 2026-03-29 02:30 never happened: the clocks went from 02:00 to 03:00.
    assert_eq!(p.parse("2026-03-29 02:30:00"), None);
    // 2026-10-25 02:30 happened twice (+02:00, then +01:00): the first.
    assert_eq!(p.parse("2026-10-25 02:30:00"), utc(2026, 10, 25, 0, 30));
    assert_eq!(p.parse("2026-10-25 03:30:00"), utc(2026, 10, 25, 2, 30));
    // Outside the changes: +02:00 in summer, +01:00 in winter.
    assert_eq!(p.parse("2026-07-01 12:00:00"), utc(2026, 7, 1, 10, 0));
    assert_eq!(p.parse("2026-01-01 12:00:00"), utc(2026, 1, 1, 11, 0));
    // The same through a user format.
    let f = TimestampParser::new(&["%d.%m.%Y %H:%M".to_string()], "local", None).unwrap();
    assert_eq!(f.parse("29.03.2026 02:30"), None);
    assert_eq!(f.parse("25.10.2026 02:30"), utc(2026, 10, 25, 0, 30));
}
