//! A timestamp with an offset followed straight by a comma (`2026/10/25 02:30:00 +02:00,message`, as in comma
//! separated message histories) keeps its offset: the offset is applied to the time and is not part of the message.
//! Before, the generic regex needed white space after the timestamp, so it backtracked, left the offset in the
//! message and read the local clock as UTC, with no problem reported.
//!
//! All data here is invented.

use debrief_core::{Profile, Workspace};
use std::path::PathBuf;

/// UTC milliseconds of `y-m-d h:m:s` (a helper that does not use the parser under test).
fn utc_ms(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32, ms: i64) -> i64 {
    chrono::NaiveDate::from_ymd_opt(y, mo, d)
        .unwrap()
        .and_hms_opt(h, mi, s)
        .unwrap()
        .and_utc()
        .timestamp_millis()
        + ms
}

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Imports `bytes` as `name` with the default profile; the events as (ts, message).
fn events(name: &str, bytes: &[u8]) -> Vec<(Option<i64>, String)> {
    let dir = Dir(std::env::temp_dir().join(format!("debrief-offset-comma-{}-{name}", std::process::id())));
    let _ = std::fs::remove_dir_all(&dir.0);
    std::fs::create_dir_all(&dir.0).unwrap();
    let path = dir.0.join(name);
    std::fs::write(&path, bytes).unwrap();
    let mut ws = Workspace::new(Profile::default_profile()).unwrap();
    ws.open(&[path]).unwrap();
    ws.records().iter().map(|r| (r.ts, r.message.to_string())).collect()
}

#[test]
fn the_offset_before_a_comma_is_applied_and_left_out_of_the_message() {
    // Europe's clocks went forward on 2026-03-29 at 01:00 UTC (+01:00 to +02:00) and back on 2026-10-25.
    let text = "\
2026/03/29 00:30:00 +01:00,Pump station started,ok
2026/03/29 03:30:00 +02:00,Pump station stopped,ok
2026/10/25 02:30:00 +02:00,Valve opened,ok
2026/10/25 02:30:00 +01:00,Valve closed,ok
";
    let got = events("history.csv", text.as_bytes());
    let want = [
        (utc_ms(2026, 3, 28, 23, 30, 0, 0), "Pump station started,ok"),
        (utc_ms(2026, 3, 29, 1, 30, 0, 0), "Pump station stopped,ok"),
        (utc_ms(2026, 10, 25, 0, 30, 0, 0), "Valve opened,ok"),
        (utc_ms(2026, 10, 25, 1, 30, 0, 0), "Valve closed,ok"),
    ];
    assert_eq!(got.len(), want.len(), "{got:?}");
    for ((ts, message), (want_ts, want_message)) in got.iter().zip(want) {
        assert_eq!(*ts, Some(want_ts), "{message}");
        assert_eq!(message, want_message);
    }
}

#[test]
fn the_offset_with_a_fraction_before_a_comma_is_applied() {
    let text = "\
2026/03/29 00:59:59.250 +01:00,Alarm raised,high
2026/03/29 03:00:00.500 +02:00,Alarm cleared,high
";
    let got = events("alarms.csv", text.as_bytes());
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(
        got[0],
        (Some(utc_ms(2026, 3, 28, 23, 59, 59, 250)), "Alarm raised,high".to_string())
    );
    assert_eq!(got[1], (Some(utc_ms(2026, 3, 29, 1, 0, 0, 500)), "Alarm cleared,high".to_string()));
}

#[test]
fn the_offset_is_applied_in_8_bit_content() {
    // Windows-1252 bytes: "é" is 0xE9. Dense enough for the encoding to be detected.
    let mut bytes = Vec::new();
    for (i, off) in ["+01:00", "+02:00"].iter().enumerate() {
        bytes.extend(format!("2026/03/29 0{}:30:00 {off},Temp", i * 3).as_bytes());
        bytes.push(0xE9);
        bytes.extend(b"rature \xE9lev\xE9e,\xE9t\xE9 \xE0 l'\xE9tude\r\n");
    }
    let got = events("history8.csv", &bytes);
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[0].0, Some(utc_ms(2026, 3, 28, 23, 30, 0, 0)));
    assert_eq!(got[1].0, Some(utc_ms(2026, 3, 29, 1, 30, 0, 0)));
    for (_, message) in &got {
        assert!(!message.contains("+0"), "the offset is not in the message: {message}");
        assert!(message.starts_with("Temp") && message.contains(','), "{message}");
    }
}

#[test]
fn nearby_shapes_still_read_their_offset() {
    // The space form: unchanged.
    let space = events(
        "space.log",
        b"2026/10/25 02:30:00 +02:00 Valve opened\n2026/10/25 02:31:00.250 +02:00 Valve closed\n",
    );
    assert_eq!(
        space,
        [
            (Some(utc_ms(2026, 10, 25, 0, 30, 0, 0)), "Valve opened".to_string()),
            (Some(utc_ms(2026, 10, 25, 0, 31, 0, 250)), "Valve closed".to_string())
        ]
    );
    // ISO with an offset, a space or a T.
    let iso = events(
        "iso.log",
        b"2026-10-25 02:30:00 +02:00 Valve opened\n2026-10-25T02:31:00+02:00 Valve closed\n",
    );
    assert_eq!(iso[0], (Some(utc_ms(2026, 10, 25, 0, 30, 0, 0)), "Valve opened".to_string()));
    assert_eq!(iso[1], (Some(utc_ms(2026, 10, 25, 0, 31, 0, 0)), "Valve closed".to_string()));
    // Slash dates without an offset are still read as UTC.
    let slash = events("slash.log", b"2026/10/25 02:30:00 Valve opened\n");
    assert_eq!(slash, [(Some(utc_ms(2026, 10, 25, 2, 30, 0, 0)), "Valve opened".to_string())]);
    // The `date,time+offset` CSV (the log4net CSV parser).
    let csv = events(
        "app.csv",
        b"2026/10/25,02:30:00.013+02:00,INFO,1000,OrderService,4120,Service starting\n2026/10/25,02:30:01.000+01:00,WARN,1001,OrderService,4120,Slow call\n",
    );
    assert_eq!(csv[0].0, Some(utc_ms(2026, 10, 25, 0, 30, 0, 13)));
    assert_eq!(csv[1].0, Some(utc_ms(2026, 10, 25, 1, 30, 1, 0)));
    assert_eq!(csv[0].1, "Service starting");
}

/// The importer that read `bytes` named `name`.
fn importer_of(name: &str, bytes: &[u8]) -> String {
    let dir = Dir(std::env::temp_dir().join(format!("debrief-offset-comma-imp-{}-{name}", std::process::id())));
    let _ = std::fs::remove_dir_all(&dir.0);
    std::fs::create_dir_all(&dir.0).unwrap();
    let path = dir.0.join(name);
    std::fs::write(&path, bytes).unwrap();
    let mut ws = Workspace::new(Profile::default_profile()).unwrap();
    ws.open(&[path]).unwrap();
    ws.sources()[0].importer.clone()
}

#[test]
fn a_headerless_csv_with_an_offset_straight_before_a_comma_stays_a_csv() {
    // A timestamp column that ends in `Z` or in an offset glued to the time, then more columns: a CSV, whose columns
    // the text importer would lose by reading the rest as one message.
    let z = "\
2026-10-25T10:00:00Z,ERROR,OrderService,Payment failed
2026-10-25T10:00:01Z,INFO,OrderService,Retry scheduled
2026-10-25T10:00:02Z,INFO,OrderService,Payment accepted
";
    assert_eq!(importer_of("orders-z.csv", z.as_bytes()), "csv");
    let glued = "\
2026-10-25T10:00:00+02:00,ERROR,OrderService,Payment failed
2026-10-25T10:00:01+02:00,INFO,OrderService,Retry scheduled
";
    assert_eq!(importer_of("orders-offset.csv", glued.as_bytes()), "csv");
    // The shape the offset fix is for (a space before the offset) still reads as text.
    let spaced = "2026/03/29 00:30:00 +01:00,Pump station started,ok\n2026/03/29 03:30:00 +02:00,Pump station stopped,ok\n";
    assert_eq!(importer_of("pumps.csv", spaced.as_bytes()), "text");
}
