//! Captures the `log` records of the process, for tests that check what the code logs (ER-27): its `EventId`, level,
//! properties and message. Install once per test binary; the records of all tests that run at the same time are in one list,
//! so a test looks for the record with its own values.

use serde_json::{Map, Value};
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

/// One captured record.
#[derive(Debug, Clone)]
pub struct Record {
    pub level: log::Level,
    pub message: String,
    /// The record's key/values (`EventId`, `Ms`, ...), typed as the log file would write them.
    pub props: Map<String, Value>,
}

impl Record {
    /// The `EventId` property, or "".
    pub fn event(&self) -> &str {
        self.props.get("EventId").and_then(Value::as_str).unwrap_or("")
    }
}

/// The most records kept (the oldest are dropped).
const KEEP: usize = 20_000;

static RECORDS: Mutex<Vec<Record>> = Mutex::new(Vec::new());
static INSTALL: Once = Once::new();

/// The record's key/values as JSON (booleans and numbers keep their type).
fn props_of(source: &dyn log::kv::Source) -> Map<String, Value> {
    struct Props(Map<String, Value>);
    impl<'kvs> log::kv::VisitSource<'kvs> for Props {
        fn visit_pair(&mut self, key: log::kv::Key<'kvs>, value: log::kv::Value<'kvs>) -> Result<(), log::kv::Error> {
            let v = if let Some(b) = value.to_bool() {
                Value::from(b)
            } else if let Some(i) = value.to_i64() {
                Value::from(i)
            } else if let Some(f) = value.to_f64() {
                Value::from(f)
            } else {
                Value::from(value.to_string())
            };
            self.0.insert(key.as_str().to_string(), v);
            Ok(())
        }
    }
    let mut props = Props(Map::new());
    let _ = source.visit(&mut props);
    props.0
}

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let rec = Record {
            level: record.level(),
            message: record.args().to_string(),
            props: props_of(record.key_values()),
        };
        let mut all = RECORDS.lock().unwrap_or_else(|p| p.into_inner());
        if all.len() >= KEEP {
            all.drain(..KEEP / 2);
        }
        all.push(rec);
    }

    fn flush(&self) {}
}

/// Starts capturing at debug level (once; a second call does nothing).
pub fn install() {
    INSTALL.call_once(|| {
        if log::set_logger(&Capture).is_ok() {
            log::set_max_level(log::LevelFilter::Debug);
        }
    });
}

/// The records so far with `event`, oldest first.
pub fn records(event: &str) -> Vec<Record> {
    RECORDS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .filter(|r| r.event() == event)
        .cloned()
        .collect()
}

/// Every record so far, oldest first.
pub fn all() -> Vec<Record> {
    RECORDS.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// The first record with `event` that `wanted` accepts, waiting up to `within` for it (background loops write theirs
/// after the call that caused them returned). `None` when none came.
pub fn wait_for(event: &str, within: Duration, wanted: impl Fn(&Record) -> bool) -> Option<Record> {
    let deadline = Instant::now() + within;
    loop {
        if let Some(r) = records(event).into_iter().find(|r| wanted(r)) {
            return Some(r);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_are_found_by_event_and_a_late_one_is_waited_for() {
        install();
        log::info!(EventId = "testlog.found", N = 3u64; "found");
        let r = wait_for("testlog.found", Duration::from_secs(5), |r| r.props["N"] == 3).expect("the record");
        assert_eq!(
            (r.event(), r.level, r.message.as_str()),
            ("testlog.found", log::Level::Info, "found")
        );
        assert!(records("testlog.nothing").is_empty());
        assert!(wait_for("testlog.nothing", Duration::from_millis(50), |_| true).is_none());
        // One that arrives while waiting.
        let late = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(100));
            log::info!(EventId = "testlog.late"; "late");
        });
        assert!(wait_for("testlog.late", Duration::from_secs(10), |_| true).is_some());
        late.join().unwrap();
        // A record without an EventId has the empty event.
        log::info!("plain");
        assert!(records("").iter().any(|r| r.message == "plain"));
    }

    #[test]
    fn the_list_keeps_the_newest_records_when_it_is_full() {
        install();
        for i in 0..(KEEP as u64 + 10) {
            log::debug!(EventId = "testlog.flood", I = i; "flood");
        }
        let flood = records("testlog.flood");
        assert!(flood.len() <= KEEP, "{}", flood.len());
        assert_eq!(flood.last().unwrap().props["I"], KEEP as u64 + 9, "the newest is kept");
    }
}
