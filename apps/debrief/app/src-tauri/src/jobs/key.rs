//! The one builder of an import's or export's idempotency key (see [`JobSpec::key`]).
//!
//! A key is the window, the kind of work and a fingerprint of everything the result depends on: the sources or the target,
//! and the whole request minus the fields that only confirm it (`overwrite`: the user's answer to "replace the file?").
//! Building every key here keeps the class "a key forgets a field that changes the result" in one place, checked by one test per
//! keyed call site. The fingerprint is a hash, so neither the Jobs record nor a log line carries a path or a query.
//!
//! A request that cannot be fingerprinted (its serialization failed) gives no key: the job runs unkeyed (never merged),
//! which is the safe side.

use super::JobSpec;
use serde::Serialize;
use serde_json::Value;
use std::hash::{Hash, Hasher};

/// The request fields that only confirm a request; never part of a key.
pub(crate) const CONFIRMATION_ONLY: &[&str] = &["overwrite"];

/// A key being built.
pub(crate) struct JobKey {
    prefix: String,
    hasher: std::collections::hash_map::DefaultHasher,
}

impl JobKey {
    /// A key for `what` (the kind of work, e.g. "import") of `window` (`None`: the app).
    pub fn new(what: &str, window: Option<&str>) -> JobKey {
        JobKey {
            prefix: format!("{}|{what}", window.unwrap_or("app")),
            hasher: std::collections::hash_map::DefaultHasher::new(),
        }
    }

    /// Adds a text (a title, a path as text, an id).
    pub fn text(mut self, text: &str) -> JobKey {
        text.hash(&mut self.hasher);
        self
    }

    /// Adds a resolved path.
    pub fn path(self, path: &std::path::Path) -> JobKey {
        self.text(&path.to_string_lossy())
    }

    /// Adds paths as a set: their order does not matter.
    pub fn paths<S: AsRef<str>>(self, paths: &[S]) -> JobKey {
        let mut sorted: Vec<&str> = paths.iter().map(AsRef::as_ref).collect();
        sorted.sort_unstable();
        sorted.into_iter().fold(self, |k, p| k.text(p))
    }

    /// Adds a whole request except the top-level fields named in `skip` ([`CONFIRMATION_ONLY`]). `None` when it cannot be
    /// serialized.
    pub fn request<T: Serialize>(self, request: &T, skip: &[&str]) -> Option<JobKey> {
        let mut value = serde_json::to_value(request).ok()?;
        if let Value::Object(map) = &mut value {
            for field in skip {
                map.remove(*field);
            }
        }
        let mut canonical = String::new();
        canonical_json(&value, &mut canonical);
        Some(self.text(&canonical))
    }

    /// The finished key.
    pub fn finish(self) -> String {
        format!("{}|{:016x}", self.prefix, self.hasher.finish())
    }
}

/// `value` as text with every object's keys in order, so a map that serializes in another order is the same fingerprint.
fn canonical_json(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for k in keys {
                out.push_str(&Value::String(k.clone()).to_string());
                out.push(':');
                canonical_json(&map[k], out);
                out.push(',');
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for i in items {
                canonical_json(i, out);
                out.push(',');
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// `spec` keyed by `key`; unkeyed (never merged) when the request could not be fingerprinted.
pub(crate) fn keyed(spec: JobSpec, key: Option<JobKey>) -> JobSpec {
    match key {
        Some(k) => spec.keyed(k.finish()),
        None => spec,
    }
}

/// As [`keyed`], but only a still-queued job serves an equal request (see [`JobSpec::keyed_while_queued`]).
pub(crate) fn keyed_while_queued(spec: JobSpec, key: JobKey) -> JobSpec {
    spec.keyed_while_queued(key.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fp(v: Value, skip: &[&str]) -> String {
        JobKey::new("x", Some("main")).request(&v, skip).unwrap().finish()
    }

    #[test]
    fn a_confirmation_only_field_is_not_part_of_the_key_but_any_other_is() {
        let a = fp(json!({"path": "/a", "format": "clef", "overwrite": false}), CONFIRMATION_ONLY);
        let b = fp(json!({"path": "/a", "format": "clef", "overwrite": true}), CONFIRMATION_ONLY);
        let c = fp(json!({"path": "/a", "format": "json", "overwrite": false}), CONFIRMATION_ONLY);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn the_order_of_object_keys_and_of_paths_does_not_matter() {
        assert_eq!(
            fp(json!({"a": 1, "b": {"c": 1, "d": 2}}), &[]),
            fp(json!({"b": {"d": 2, "c": 1}, "a": 1}), &[])
        );
        let k = |p: &[&str]| JobKey::new("import", Some("main")).paths(p).finish();
        assert_eq!(k(&["/b", "/a"]), k(&["/a", "/b"]));
        assert_ne!(k(&["/a"]), k(&["/a", "/b"]));
    }

    #[test]
    fn the_window_and_the_kind_of_work_are_part_of_the_key() {
        let k = |what: &str, w: Option<&str>| JobKey::new(what, w).text("t").finish();
        assert_ne!(k("import", Some("main")), k("import", Some("w2")));
        assert_ne!(k("import", Some("main")), k("export", Some("main")));
        assert_ne!(k("import", None), k("import", Some("main")));
    }

    #[test]
    fn a_request_that_cannot_be_serialized_gives_an_unkeyed_spec() {
        struct Broken;
        impl Serialize for Broken {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("no"))
            }
        }
        let key = JobKey::new("x", None).request(&Broken, &[]);
        assert!(key.is_none());
        let spec = keyed(JobSpec::new(super::super::JobKind::Export, "e"), key);
        assert!(spec.key.is_none());
    }
}
