//! The queue status channel: a caller that wraps a command whose children may queue for a slot (the push gate's `./dev gen check` row)
//! names a file in `THROTTLE_STATUS_FILE`, and every `throttle` run below it appends one JSON line per event to it: queued, admitted,
//! done. The caller reads the file with [`read`] and so tells a child that waits for a slot ("waiting", never stuck) from one that is
//! silent while it runs, and splits the queue seconds from the run seconds, with no text of the run log parsed (ER-42: the one
//! declared format, written and read here, ER-22).
//!
//! A line is `{"event":"queued","id":..,"agent":..,"at_ms":..}`, `{"event":"admitted","id":..,"queued_secs":..,"at_ms":..}` or
//! `{"event":"done","id":..,"ran_secs":..,"code":..,"at_ms":..}`; `at_ms` is the wall clock in milliseconds since the epoch. A line
//! that cannot be understood is counted ([`Summary::unreadable`]), never guessed at, and a file that cannot be read is
//! "not measured" to its reader ([`read`] returns `None`): the caller then treats no step as waiting (ER-33).

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// The variable that names the status file: set by the caller for one row, read by every `throttle run` below it.
pub const STATUS_FILE_VAR: &str = "THROTTLE_STATUS_FILE";

/// One event of one run (`id` is the run's: its pid and agent name, so two runs never share one).
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The run asked for a slot and waits for it.
    Queued {
        /// The run.
        id: String,
        /// The agent name of the run, for a person reading the file.
        agent: String,
        /// When, milliseconds since the epoch.
        at_ms: u64,
    },
    /// The run holds a slot: its command starts now.
    Admitted {
        /// The run.
        id: String,
        /// Seconds from the request to the slot (queue and load gate), as the run measured them.
        queued_secs: f64,
        /// When, milliseconds since the epoch.
        at_ms: u64,
    },
    /// The run ended. `ran_secs` is 0 for a run that never got a slot (queue limit, queue stall, stop request).
    Done {
        /// The run.
        id: String,
        /// Seconds the command ran.
        ran_secs: f64,
        /// The exit code the run leaves with.
        code: i32,
        /// When, milliseconds since the epoch.
        at_ms: u64,
    },
}

impl Event {
    /// The one line of the file for this event (no newline).
    pub fn line(&self) -> String {
        match self {
            Event::Queued { id, agent, at_ms } => json!({"event": "queued", "id": id, "agent": agent, "at_ms": at_ms}),
            Event::Admitted { id, queued_secs, at_ms } => {
                json!({"event": "admitted", "id": id, "queued_secs": queued_secs, "at_ms": at_ms})
            }
            Event::Done { id, ran_secs, code, at_ms } => {
                json!({"event": "done", "id": id, "ran_secs": ran_secs, "code": code, "at_ms": at_ms})
            }
        }
        .to_string()
    }

    /// The event a line holds, or `None` for a line of no known shape.
    pub fn parse(line: &str) -> Option<Event> {
        let v: Value = serde_json::from_str(line).ok()?;
        let id = v["id"].as_str()?.to_string();
        let at_ms = v["at_ms"].as_u64()?;
        match v["event"].as_str()? {
            "queued" => Some(Event::Queued {
                id,
                agent: v["agent"].as_str().unwrap_or_default().to_string(),
                at_ms,
            }),
            "admitted" => Some(Event::Admitted {
                id,
                queued_secs: v["queued_secs"]
                    .as_f64()
                    .filter(|s| s.is_finite() && *s >= 0.0)?,
                at_ms,
            }),
            "done" => Some(Event::Done {
                id,
                ran_secs: v["ran_secs"]
                    .as_f64()
                    .filter(|s| s.is_finite() && *s >= 0.0)?,
                code: i32::try_from(v["code"].as_i64()?).ok()?,
                at_ms,
            }),
            _ => None,
        }
    }
}

/// Appends `event` to the status file `path`. Never fails and never panics: a status that cannot be written is skipped (the run goes
/// on; its reader then sees no event, which is "not measured", never "waiting"). One line is one `write`, so runs that append at the
/// same time do not tear each other's lines.
pub fn append(path: &Path, event: &Event) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(format!("{}\n", event.line()).as_bytes());
    }
}

/// What a status file says.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Summary {
    /// Seconds queued by the runs that left the queue (admitted, or ended without a slot).
    pub queued_closed_secs: f64,
    /// The runs that wait for a slot now: their id and the `at_ms` they queued.
    pub waiting: BTreeMap<String, u64>,
    /// Runs that were admitted and have not ended.
    pub running: usize,
    /// Lines that held a known event: 0 says no run below the row ever asked for a slot.
    pub events: usize,
    /// Lines that held no known event.
    pub unreadable: usize,
}

impl Summary {
    /// Whether some run waits for a slot now.
    pub fn is_waiting(&self) -> bool {
        !self.waiting.is_empty()
    }

    /// Seconds spent queued up to `now_ms`: the closed waits and every wait still going (runs that wait together count once each, as
    /// the row's children run one after another).
    pub fn queued_secs(&self, now_ms: u64) -> f64 {
        let open: f64 = self
            .waiting
            .values()
            .map(|&at| now_ms.saturating_sub(at) as f64 / 1000.0)
            .sum();
        self.queued_closed_secs + open
    }

    /// The summary of the events in `text` (the file's content), in order.
    pub fn of(text: &str) -> Summary {
        let mut s = Summary::default();
        let mut running: std::collections::BTreeSet<String> = Default::default();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let parsed = Event::parse(line);
            s.events += usize::from(parsed.is_some());
            match parsed {
                Some(Event::Queued { id, at_ms, .. }) => {
                    s.waiting.insert(id, at_ms);
                }
                Some(Event::Admitted {
                    id, queued_secs, ..
                }) => {
                    s.waiting.remove(&id);
                    s.queued_closed_secs += queued_secs;
                    running.insert(id);
                }
                Some(Event::Done { id, at_ms, .. }) => {
                    // A run that never got a slot ends while queued: its wait ends here.
                    if let Some(at) = s.waiting.remove(&id) {
                        s.queued_closed_secs += at_ms.saturating_sub(at) as f64 / 1000.0;
                    }
                    running.remove(&id);
                }
                None => s.unreadable += 1,
            }
        }
        s.running = running.len();
        s
    }
}

/// The summary of the status file `path`; `None` when it cannot be read (it is not there yet when no run below the row has queued).
pub fn read(path: &Path) -> Option<Summary> {
    std::fs::read_to_string(path).ok().map(|t| Summary::of(&t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queued(id: &str, at_ms: u64) -> Event {
        Event::Queued {
            id: id.into(),
            agent: "gen".into(),
            at_ms,
        }
    }

    #[test]
    fn every_event_survives_its_line() {
        for e in [
            queued("7-gen", 1_000),
            Event::Admitted {
                id: "7-gen".into(),
                queued_secs: 12.5,
                at_ms: 13_500,
            },
            Event::Done {
                id: "7-gen".into(),
                ran_secs: 3.25,
                code: 0,
                at_ms: 17_000,
            },
            Event::Done {
                id: "7-gen".into(),
                ran_secs: 0.0,
                code: 79,
                at_ms: 17_000,
            },
        ] {
            assert_eq!(Event::parse(&e.line()), Some(e.clone()), "{}", e.line());
        }
        for bad in [
            "",
            "queued",
            "{}",
            r#"{"event":"queued"}"#,
            r#"{"event":"x","id":"a","at_ms":1}"#,
            r#"{"event":"admitted","id":"a","at_ms":1,"queued_secs":-1}"#,
        ] {
            assert_eq!(Event::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_run_is_waiting_from_queued_until_admitted_and_its_wait_is_counted() {
        let lines = |evs: &[Event]| evs.iter().map(|e| e.line() + "\n").collect::<String>();
        let q = Summary::of(&lines(&[queued("a", 10_000)]));
        assert!(q.is_waiting() && q.running == 0);
        assert_eq!(
            q.queued_secs(70_000),
            60.0,
            "the open wait counts up to now"
        );
        let up = Summary::of(&lines(&[
            queued("a", 10_000),
            Event::Admitted {
                id: "a".into(),
                queued_secs: 42.0,
                at_ms: 52_000,
            },
        ]));
        assert!(!up.is_waiting() && up.running == 1);
        assert_eq!(up.queued_secs(900_000), 42.0, "a closed wait stops growing");
        let done = Summary::of(&lines(&[
            queued("a", 10_000),
            Event::Admitted {
                id: "a".into(),
                queued_secs: 42.0,
                at_ms: 52_000,
            },
            Event::Done {
                id: "a".into(),
                ran_secs: 5.0,
                code: 0,
                at_ms: 57_000,
            },
            queued("b", 60_000),
            Event::Done {
                id: "b".into(),
                ran_secs: 0.0,
                code: 79,
                at_ms: 65_000,
            },
        ]));
        assert!(!done.is_waiting() && done.running == 0);
        assert_eq!(
            done.queued_secs(1_000_000),
            47.0,
            "42 s admitted plus 5 s of a run that never got a slot"
        );
    }

    #[test]
    fn a_line_of_no_known_shape_is_counted_and_never_makes_a_run_wait() {
        let s = Summary::of("not json\n{\"event\":\"queued\"}\n");
        assert_eq!((s.unreadable, s.is_waiting()), (2, false));
    }

    #[test]
    fn append_and_read_go_through_a_real_file_and_a_missing_one_is_not_measured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a").join("row.queue.jsonl");
        assert_eq!(read(&path), None, "no file: not measured");
        append(&path, &queued("a", 1));
        append(
            &path,
            &Event::Admitted {
                id: "a".into(),
                queued_secs: 2.0,
                at_ms: 2_001,
            },
        );
        let s = read(&path).expect("read");
        assert_eq!(
            (s.is_waiting(), s.running, s.queued_secs(9_999_999)),
            (false, 1, 2.0)
        );
    }
}
