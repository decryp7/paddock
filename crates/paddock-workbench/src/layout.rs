//! The workbench's layout store (plan W2a, W7): the dock's saved arrangements (per window, per perspective, named layouts)
//! as one versioned state file of `paddock-env` in the app's config folder, so they survive a reinstall of the webview's
//! storage and a crash (the session checkpoint of `paddock-startup` holds the UI state of a recovery reload; this holds
//! what the user arranged).
//!
//! The UI owns the content: each value is a JSON object it parses at its edge (the dock's versioned envelope). The store
//! only bounds it: a key is `[a-z0-9._:-]{1,64}`, a value is an object of at most [`MAX_VALUE_BYTES`], at most
//! [`MAX_KEYS`] keys and [`MAX_TOTAL_BYTES`] in all. A damaged or newer file gives an empty store and one [`Problem`] to
//! report; saves to a newer file are refused (`paddock-env`).

use paddock_env::{Problem, VersionedFile};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

/// The file's name in the config folder.
pub const FILE_NAME: &str = "workbench-layout.json";
/// The file format this build writes.
pub const FILE_VERSION: u32 = 1;
/// The longest key.
pub const MAX_KEY_LEN: usize = 64;
/// The largest value, serialized.
pub const MAX_VALUE_BYTES: usize = 1024 * 1024;
/// The most keys.
pub const MAX_KEYS: usize = 128;
/// The largest file, serialized (all values together).
pub const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;

/// What the file holds: the values by key.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Layouts {
    /// Saved values by key.
    #[serde(default)]
    pub values: BTreeMap<String, Map<String, Value>>,
}

/// Why a request was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    /// The key is not `[a-z0-9._:-]{1,64}`.
    BadKey,
    /// The value is not a JSON object.
    NotAnObject,
    /// The value is larger than [`MAX_VALUE_BYTES`].
    TooLarge,
    /// A new key over [`MAX_KEYS`], or the whole store over [`MAX_TOTAL_BYTES`].
    Full,
    /// The file could not be written (or saving is refused: a newer or unreadable file).
    Write(String),
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::BadKey => write!(
                f,
                "A layout name is 1 to {MAX_KEY_LEN} characters of a-z, 0-9, '.', '_', ':' and '-'."
            ),
            LayoutError::NotAnObject => f.write_str("A layout must be a JSON object."),
            LayoutError::TooLarge => write!(
                f,
                "The layout is larger than {} KiB.",
                MAX_VALUE_BYTES / 1024
            ),
            LayoutError::Full => f.write_str(
                "There are too many saved layouts. Delete some named layouts and try again.",
            ),
            LayoutError::Write(why) => write!(f, "The layout could not be saved: {why}"),
        }
    }
}

impl std::error::Error for LayoutError {}

/// Whether `key` is a valid key.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY_LEN
        && key.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b':' | b'-')
        })
}

fn size_of(value: &Map<String, Value>) -> usize {
    // A map of JSON values always serializes; the fallback only bounds what cannot happen.
    serde_json::to_vec(value).map_or(usize::MAX, |v| v.len())
}

/// The store: loaded once, changed in memory, saved on each change (atomic, with a backup).
pub struct LayoutStore {
    file: VersionedFile<Layouts>,
    state: Mutex<Layouts>,
    problem: Option<Problem>,
}

impl LayoutStore {
    /// Opens the store at `path` (usually `<config>/workbench-layout.json`). Never fails: a missing or damaged file gives an
    /// empty store and [`problem`](Self::problem) says why.
    pub fn open(path: impl Into<PathBuf>) -> LayoutStore {
        let file: VersionedFile<Layouts> =
            VersionedFile::new(path, FILE_VERSION).max_bytes((MAX_TOTAL_BYTES + 64 * 1024) as u64);
        let loaded = file.load();
        let mut value = loaded.value;
        // A file written by hand may break the bounds: keep what fits, in key order.
        value
            .values
            .retain(|k, v| valid_key(k) && size_of(v) <= MAX_VALUE_BYTES);
        while value.values.len() > MAX_KEYS {
            value.values.pop_last();
        }
        LayoutStore {
            file,
            state: Mutex::new(value),
            problem: loaded.problem,
        }
    }

    /// Why the file was not used as it was, if so (report it once).
    pub fn problem(&self) -> Option<&Problem> {
        self.problem.as_ref()
    }

    /// The value saved under `key`, if any.
    pub fn get(&self, key: &str) -> Result<Option<Map<String, Value>>, LayoutError> {
        if !valid_key(key) {
            return Err(LayoutError::BadKey);
        }
        Ok(self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values
            .get(key)
            .cloned())
    }

    /// The keys saved, in order (named layouts are listed by their keys).
    pub fn keys(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values
            .keys()
            .cloned()
            .collect()
    }

    /// Saves `value` under `key` (`None` deletes it) and writes the file. On a refused write the change is kept in memory
    /// for this run and the error says why.
    pub fn set(&self, key: &str, value: Option<Value>) -> Result<(), LayoutError> {
        if !valid_key(key) {
            return Err(LayoutError::BadKey);
        }
        let value = match value {
            None => None,
            Some(Value::Object(map)) => {
                if size_of(&map) > MAX_VALUE_BYTES {
                    return Err(LayoutError::TooLarge);
                }
                Some(map)
            }
            Some(_) => return Err(LayoutError::NotAnObject),
        };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut next = state.clone();
        match value {
            None => {
                next.values.remove(key);
            }
            Some(map) => {
                if !next.values.contains_key(key) && next.values.len() >= MAX_KEYS {
                    return Err(LayoutError::Full);
                }
                next.values.insert(key.to_owned(), map);
                if next.values.values().map(size_of).sum::<usize>() > MAX_TOTAL_BYTES {
                    return Err(LayoutError::Full);
                }
            }
        }
        let written = self.file.save(&next);
        *state = next;
        written.map_err(|e| LayoutError::Write(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store() -> (tempfile::TempDir, LayoutStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let s = LayoutStore::open(dir.path().join(FILE_NAME));
        (dir, s)
    }

    #[test]
    fn keys_are_bounded() {
        assert!(valid_key("main:perspective.understand"));
        assert!(valid_key(&"a".repeat(MAX_KEY_LEN)));
        assert!(!valid_key(""));
        assert!(!valid_key(&"a".repeat(MAX_KEY_LEN + 1)));
        assert!(!valid_key("Main"));
        assert!(!valid_key("a/b"));
        assert!(!valid_key("a b"));
    }

    #[test]
    fn saves_reads_deletes_and_survives_a_reopen() {
        let (dir, s) = store();
        assert_eq!(s.problem(), None);
        assert_eq!(s.get("main").unwrap(), None);
        s.set("main", Some(json!({ "version": 2, "grid": [1, 2] })))
            .unwrap();
        s.set("named:review", Some(json!({}))).unwrap();
        assert_eq!(s.keys(), vec!["main".to_owned(), "named:review".to_owned()]);
        let again = LayoutStore::open(dir.path().join(FILE_NAME));
        assert_eq!(
            again.get("main").unwrap(),
            Some(
                json!({ "version": 2, "grid": [1, 2] })
                    .as_object()
                    .unwrap()
                    .clone()
            )
        );
        again.set("named:review", None).unwrap();
        again.set("never", None).unwrap();
        assert_eq!(
            LayoutStore::open(dir.path().join(FILE_NAME)).keys(),
            vec!["main".to_owned()]
        );
    }

    #[test]
    fn refuses_bad_requests() {
        let (_dir, s) = store();
        assert_eq!(s.get("Bad Key"), Err(LayoutError::BadKey));
        assert_eq!(s.set("Bad Key", None), Err(LayoutError::BadKey));
        assert_eq!(
            s.set("main", Some(json!([1]))),
            Err(LayoutError::NotAnObject)
        );
        let big = "x".repeat(MAX_VALUE_BYTES);
        assert_eq!(
            s.set("main", Some(json!({ "big": big }))),
            Err(LayoutError::TooLarge)
        );
        for i in 0..MAX_KEYS {
            s.set(&format!("k{i}"), Some(json!({}))).unwrap();
        }
        assert_eq!(s.set("one-more", Some(json!({}))), Err(LayoutError::Full));
        // Replacing an existing key is fine at the limit.
        s.set("k0", Some(json!({ "a": 1 }))).unwrap();
        for e in [
            LayoutError::BadKey,
            LayoutError::NotAnObject,
            LayoutError::TooLarge,
            LayoutError::Full,
            LayoutError::Write("x".into()),
        ] {
            assert!(!e.to_string().is_empty());
        }
    }

    #[test]
    fn refuses_a_store_over_its_total() {
        let (_dir, s) = store();
        let half = "x".repeat(MAX_VALUE_BYTES - 64);
        for i in 0..(MAX_TOTAL_BYTES / MAX_VALUE_BYTES) {
            s.set(&format!("k{i}"), Some(json!({ "v": half }))).unwrap();
        }
        assert_eq!(
            s.set("last", Some(json!({ "v": half }))),
            Err(LayoutError::Full)
        );
        assert_eq!(s.get("last").unwrap(), None);
    }

    #[test]
    fn a_damaged_file_gives_an_empty_store_and_a_problem() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, "{ not json").unwrap();
        let s = LayoutStore::open(&path);
        assert!(matches!(s.problem(), Some(Problem::Damaged { .. })));
        assert!(s.keys().is_empty());
        s.set("main", Some(json!({}))).unwrap();
    }

    #[test]
    fn a_newer_file_is_left_alone_and_saves_are_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, r#"{ "version": 99, "values": { "main": {} } }"#).unwrap();
        let s = LayoutStore::open(&path);
        assert!(matches!(
            s.problem(),
            Some(Problem::Newer { found: 99, .. })
        ));
        assert!(matches!(
            s.set("main", Some(json!({}))),
            Err(LayoutError::Write(_))
        ));
        // Kept in memory for this run.
        assert!(s.get("main").unwrap().is_some());
        assert!(std::fs::read_to_string(&path).unwrap().contains("99"));
    }

    #[test]
    fn values_out_of_bounds_in_the_file_are_dropped() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(FILE_NAME);
        let mut values = Map::new();
        values.insert("Bad Key".into(), json!({}));
        for i in 0..(MAX_KEYS + 3) {
            values.insert(format!("k{i:04}"), json!({}));
        }
        std::fs::write(&path, json!({ "version": 1, "values": values }).to_string()).unwrap();
        let s = LayoutStore::open(&path);
        assert_eq!(s.keys().len(), MAX_KEYS);
        assert_eq!(s.keys().first().map(String::as_str), Some("k0000"));
    }
}
