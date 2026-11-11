//! The pure core of the performance section: names, the ring of the slowest operations and the startup phases. Time is
//! always passed in (`now_ms`, milliseconds since the Unix epoch), so every rule here is a plain unit test.
//!
//! No user content, ever: an operation, phase or startup-phase name is an identifier (letters, digits and `. _ : -`, at most
//! [`MAX_NAME`] characters). Anything else (a path, a query text, a file name with spaces) is replaced by [`INVALID_NAME`]
//! at the door, and again when a ring file is read back, so a damaged or hand-edited file cannot carry content into a report.

use serde::{Deserialize, Serialize};

/// The slowest operations kept.
pub const RING_CAPACITY: usize = 50;
/// How long an operation stays in the ring (24 h), milliseconds.
pub const WINDOW_MS: u64 = 24 * 60 * 60 * 1000;
/// Startup phases kept.
pub const MAX_STARTUP_PHASES: usize = 64;
/// Longest name kept, characters.
pub const MAX_NAME: usize = 48;
/// Largest ring file read, bytes (50 entries are about 8 KB).
pub const MAX_RING_FILE: u64 = 64 * 1024;
/// What an unusable name becomes.
pub const INVALID_NAME: &str = "invalid-name";
/// The phase of a duration recorded without one.
pub const DEFAULT_PHASE: &str = "total";
/// The version of the ring file and of `performance.json`.
pub const SCHEMA: u32 = 1;

/// Whether `name` is an identifier this section accepts.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_NAME
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}

/// `name` when it is an identifier, else [`INVALID_NAME`].
pub fn clean_name(name: &str) -> String {
    if valid_name(name) {
        name.to_string()
    } else {
        INVALID_NAME.to_string()
    }
}

/// One slow operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slow {
    /// What ran (`import-files`, `query`, `page-load`, `job:export`).
    pub op: String,
    /// The phase that took the time (`scan`, `parse`; [`DEFAULT_PHASE`] when the caller timed the whole).
    pub phase: String,
    /// How long, milliseconds.
    pub duration_ms: u64,
    /// When it ended, milliseconds since the Unix epoch.
    pub at_ms: u64,
}

impl Slow {
    /// An entry with cleaned names.
    pub fn new(op: &str, phase: &str, duration_ms: u64, at_ms: u64) -> Self {
        Slow {
            op: clean_name(op),
            phase: clean_name(phase),
            duration_ms,
            at_ms,
        }
    }
}

/// One startup phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phase {
    /// The phase (`logging`, `settings`, `window-ready`).
    pub name: String,
    /// When it ended, milliseconds since the startup began.
    pub at_ms: u64,
    /// How long it took, milliseconds.
    pub duration_ms: u64,
}

/// Why a ring file was not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RingError {
    /// The file is larger than [`MAX_RING_FILE`].
    TooLarge(u64),
    /// The file is not a ring (not JSON, a field of the wrong type).
    Damaged(String),
    /// The file is of another version.
    Version(u32),
}

impl std::fmt::Display for RingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RingError::TooLarge(n) => write!(f, "it is {n} bytes, more than {MAX_RING_FILE}"),
            RingError::Damaged(why) => write!(f, "it is damaged ({why})"),
            RingError::Version(v) => {
                write!(f, "it is version {v}, this build reads version {SCHEMA}")
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RingFile {
    schema: u32,
    entries: Vec<Slow>,
}

/// The slowest operations of the last 24 hours, at most [`RING_CAPACITY`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ring {
    entries: Vec<Slow>,
}

impl Ring {
    /// An empty ring.
    pub fn new() -> Self {
        Ring::default()
    }

    /// Entries held (expired ones included until the next insert or snapshot).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is held.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// A duration that cannot enter the ring any more: the shortest held when it is full, else 0. A caller may skip a
    /// shorter duration without taking a lock (a hint: a stale value only costs one extra insert call).
    pub fn floor_ms(&self) -> u64 {
        if self.entries.len() < RING_CAPACITY {
            0
        } else {
            self.entries
                .iter()
                .map(|e| e.duration_ms)
                .min()
                .unwrap_or(0)
        }
    }

    /// Adds `entry` if it is among the slowest: while there is room it is kept; when full it replaces the shortest entry
    /// if it is longer (expired entries go first). Returns whether it was kept.
    pub fn insert(&mut self, entry: Slow, now_ms: u64) -> bool {
        if self.entries.len() >= RING_CAPACITY {
            self.entries
                .retain(|e| now_ms.saturating_sub(e.at_ms) <= WINDOW_MS);
        }
        if self.entries.len() < RING_CAPACITY {
            self.entries.push(entry);
            return true;
        }
        let Some((i, shortest)) = self
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.duration_ms)
        else {
            return false;
        };
        if entry.duration_ms > shortest.duration_ms {
            self.entries[i] = entry;
            true
        } else {
            false
        }
    }

    /// The entries of the last 24 hours, slowest first (ties: newest first).
    pub fn snapshot(&self, now_ms: u64) -> Vec<Slow> {
        let mut out: Vec<Slow> = self
            .entries
            .iter()
            .filter(|e| now_ms.saturating_sub(e.at_ms) <= WINDOW_MS)
            .cloned()
            .collect();
        out.sort_by(|a, b| {
            b.duration_ms
                .cmp(&a.duration_ms)
                .then(b.at_ms.cmp(&a.at_ms))
        });
        out
    }

    /// The ring file's bytes.
    pub fn to_bytes(&self, now_ms: u64) -> Vec<u8> {
        let file = RingFile {
            schema: SCHEMA,
            entries: self.snapshot(now_ms),
        };
        serde_json::to_vec(&file).unwrap_or_default()
    }

    /// Reads a ring file: names re-cleaned, expired and surplus entries dropped, future times clamped.
    pub fn parse(bytes: &[u8], now_ms: u64) -> Result<Ring, RingError> {
        if bytes.len() as u64 > MAX_RING_FILE {
            return Err(RingError::TooLarge(bytes.len() as u64));
        }
        let file: RingFile = serde_json::from_slice(bytes)
            .map_err(|e| RingError::Damaged(format!("line {}, column {}", e.line(), e.column())))?;
        if file.schema != SCHEMA {
            return Err(RingError::Version(file.schema));
        }
        let mut ring = Ring::new();
        for e in file.entries {
            let entry = Slow::new(&e.op, &e.phase, e.duration_ms, e.at_ms.min(now_ms));
            if now_ms.saturating_sub(entry.at_ms) <= WINDOW_MS {
                ring.insert(entry, now_ms);
            }
        }
        Ok(ring)
    }
}

/// The startup phases, in order, at most [`MAX_STARTUP_PHASES`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Startup {
    phases: Vec<Phase>,
}

impl Startup {
    /// No phases yet.
    pub fn new() -> Self {
        Startup::default()
    }

    /// Adds a phase; a phase past the limit is dropped (the first ones explain a slow start).
    pub fn push(&mut self, name: &str, at_ms: u64, duration_ms: u64) -> bool {
        if self.phases.len() >= MAX_STARTUP_PHASES {
            return false;
        }
        self.phases.push(Phase {
            name: clean_name(name),
            at_ms,
            duration_ms,
        });
        true
    }

    /// The phases in order.
    pub fn phases(&self) -> &[Phase] {
        &self.phases
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 10 * WINDOW_MS;

    fn slow(op: &str, ms: u64, age: u64) -> Slow {
        Slow::new(op, "scan", ms, NOW - age)
    }

    #[test]
    fn names_are_identifiers_and_anything_else_is_replaced() {
        for ok in ["import-files", "job:export", "page.load_2", "a"] {
            assert!(valid_name(ok), "{ok}");
            assert_eq!(clean_name(ok), ok);
        }
        let long = "x".repeat(MAX_NAME + 1);
        for bad in [
            "",
            "C:\\Users\\bob\\notes.txt",
            "/home/bob/x",
            "select * from t",
            "bob@example.com",
            "naïve",
            long.as_str(),
        ] {
            assert!(!valid_name(bad), "{bad}");
            assert_eq!(clean_name(bad), INVALID_NAME);
        }
        assert!(valid_name(&"x".repeat(MAX_NAME)));
    }

    #[test]
    fn a_new_entry_has_cleaned_names() {
        let e = Slow::new("open /home/bob/a.log", "ok", 5, 6);
        assert_eq!(
            (e.op.as_str(), e.phase.as_str(), e.duration_ms, e.at_ms),
            (INVALID_NAME, "ok", 5, 6)
        );
    }

    #[test]
    fn the_ring_keeps_the_slowest_up_to_its_capacity() {
        let mut ring = Ring::new();
        assert!(ring.is_empty());
        assert_eq!(ring.floor_ms(), 0);
        for i in 1..=RING_CAPACITY as u64 {
            assert!(ring.insert(slow("op", i * 10, 0), NOW));
        }
        assert_eq!(ring.len(), RING_CAPACITY);
        assert_eq!(ring.floor_ms(), 10);
        assert!(
            !ring.insert(slow("fast", 10, 0), NOW),
            "not longer than the shortest"
        );
        assert!(!ring.insert(slow("fast", 5, 0), NOW));
        assert!(ring.insert(slow("slower", 15, 0), NOW));
        assert_eq!(ring.len(), RING_CAPACITY, "bounded");
        assert_eq!(ring.floor_ms(), 15);
        let snap = ring.snapshot(NOW);
        assert_eq!(
            snap.first().map(|e| e.duration_ms),
            Some(RING_CAPACITY as u64 * 10)
        );
        assert_eq!(snap.last().map(|e| e.duration_ms), Some(15));
        assert!(
            snap.windows(2)
                .all(|w| w[0].duration_ms >= w[1].duration_ms)
        );
    }

    #[test]
    fn entries_older_than_a_day_leave_first_and_never_show() {
        let mut ring = Ring::new();
        for _ in 0..RING_CAPACITY {
            ring.insert(slow("old", 9_000, WINDOW_MS + 1), NOW);
        }
        assert!(
            ring.snapshot(NOW).is_empty(),
            "expired entries are not shown"
        );
        assert!(
            ring.insert(slow("new", 1, 0), NOW),
            "a full ring of expired entries makes room"
        );
        assert_eq!(ring.len(), 1);
        assert_eq!(ring.snapshot(NOW)[0].op, "new");
    }

    #[test]
    fn ties_list_the_newest_first() {
        let mut ring = Ring::new();
        ring.insert(slow("a", 100, 50), NOW);
        ring.insert(slow("b", 100, 10), NOW);
        let ops: Vec<String> = ring.snapshot(NOW).into_iter().map(|e| e.op).collect();
        assert_eq!(ops, ["b", "a"]);
    }

    #[test]
    fn the_file_round_trips() {
        let mut ring = Ring::new();
        ring.insert(slow("a", 100, 5), NOW);
        ring.insert(slow("b", 300, 7), NOW);
        let back = Ring::parse(&ring.to_bytes(NOW), NOW).unwrap();
        assert_eq!(back.snapshot(NOW), ring.snapshot(NOW));
    }

    #[test]
    fn a_damaged_file_is_an_error_not_a_panic() {
        for bytes in [
            &b""[..],
            b"not json",
            b"{\"schema\":1}",
            b"{\"schema\":1,\"entries\":[{\"op\":5}]}",
            b"\xff\xfe\x00",
            b"[1,2",
        ] {
            assert!(
                matches!(Ring::parse(bytes, NOW), Err(RingError::Damaged(_))),
                "{bytes:?}"
            );
        }
        let other = br#"{"schema":9,"entries":[]}"#;
        assert_eq!(Ring::parse(other, NOW), Err(RingError::Version(9)));
        let big = vec![b' '; MAX_RING_FILE as usize + 1];
        assert_eq!(
            Ring::parse(&big, NOW),
            Err(RingError::TooLarge(MAX_RING_FILE + 1))
        );
        for e in [
            RingError::TooLarge(9),
            RingError::Damaged("x".into()),
            RingError::Version(3),
        ] {
            assert!(!e.to_string().is_empty());
        }
    }

    #[test]
    fn a_file_with_content_in_it_is_cleaned_expired_trimmed_and_clamped() {
        let mut entries = String::new();
        for i in 0..(RING_CAPACITY + 20) {
            entries.push_str(&format!(
                r#"{{"op":"o{i}","phase":"p","durationMs":{},"atMs":{NOW}}},"#,
                i + 1
            ));
        }
        let text = format!(
            r#"{{"schema":1,"entries":[{entries}
            {{"op":"C:\\Users\\bob\\secret.txt","phase":"my password is x","durationMs":99999,"atMs":{NOW}}},
            {{"op":"expired","phase":"p","durationMs":99999,"atMs":0}},
            {{"op":"future","phase":"p","durationMs":99998,"atMs":{}}}]}}"#,
            NOW + WINDOW_MS * 5
        );
        let ring = Ring::parse(text.as_bytes(), NOW).unwrap();
        assert_eq!(ring.len(), RING_CAPACITY);
        let snap = ring.snapshot(NOW);
        assert_eq!(
            (snap[0].op.as_str(), snap[0].phase.as_str()),
            (INVALID_NAME, INVALID_NAME)
        );
        assert_eq!(snap[1].op, "future");
        assert_eq!(snap[1].at_ms, NOW, "a time in the future is clamped to now");
        assert!(snap.iter().all(|e| e.op != "expired"
            && !e.op.contains("secret")
            && !e.phase.contains("password")));
    }

    #[test]
    fn startup_phases_keep_order_are_bounded_and_cleaned() {
        let mut s = Startup::new();
        assert!(s.push("logging", 10, 10));
        assert!(s.push("/home/bob", 30, 20));
        for i in 2..MAX_STARTUP_PHASES {
            assert!(s.push("p", i as u64, 1));
        }
        assert!(!s.push("late", 999, 1), "past the limit");
        assert_eq!(s.phases().len(), MAX_STARTUP_PHASES);
        assert_eq!(s.phases()[0].name, "logging");
        assert_eq!(s.phases()[1].name, INVALID_NAME);
    }
}
