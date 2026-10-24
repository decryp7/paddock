//! What may be written to a log. The rule (Debrief's `activity::privacy`, now for every app): **never file contents**.
//! The default [`Redactor`] therefore
//!
//! - masks values of secret-like keys (`Token`, `ApiKey`, `Password`, `Authorization`, ...) as `***`,
//! - replaces values of content-like keys (`Content`, `Body`, `Line`, `Text`, `Prompt`, `Query`, ...) by a count
//!   (`<omitted: 120 characters>`), so the log still shows that something was there,
//! - cuts every other string to [`MAX_VALUE`] characters and the message to [`MAX_MESSAGE`].
//!
//! Callers decide what they log; this is the net under them. Apps add rules with [`Redactor::with_hook`] (and the report
//! writer of plan X2 can reuse the same trait). [`fingerprint`] is the way to show that two values were equal without
//! writing them.

use serde_json::Value;
use std::sync::Arc;

/// Longest string property (characters); longer ones are cut with `…`.
pub const MAX_VALUE: usize = 1000;
/// Longest message (characters).
pub const MAX_MESSAGE: usize = 4000;
/// Longest value of the properties the log itself adds (`Backtrace`, `Breadcrumbs`), in characters.
pub const MAX_TRUSTED_VALUE: usize = 16 * 1024;
/// What replaces a secret.
pub const SECRET: &str = "***";

/// Properties the log adds itself (after redaction) and the panic hook logs; they may be long.
pub(crate) const TRUSTED_KEYS: &[&str] = &["Backtrace", "Breadcrumbs"];

/// Decides what of an event reaches the file. Implementations must not panic.
pub trait Redact: Send + Sync + 'static {
    /// The value to write for property `key`, or `None` to leave the property out.
    fn field(&self, key: &str, value: Value) -> Option<Value>;
    /// The message to write.
    fn message(&self, message: &str) -> String;
}

/// A hook that sees a property first: `Some(Some(v))` writes `v`, `Some(None)` drops the property, `None` leaves it to the
/// default rules.
pub type Hook = Arc<dyn Fn(&str, &Value) -> Option<Option<Value>> + Send + Sync>;

/// The default [`Redact`]: the rules in the module docs plus the app's hooks.
#[derive(Clone, Default)]
pub struct Redactor {
    hooks: Vec<Hook>,
}

impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Redactor({} hooks)", self.hooks.len())
    }
}

impl Redactor {
    /// The default rules, no hooks.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a hook (see [`Hook`]); hooks run in the order added, the first answer wins.
    pub fn with_hook(
        mut self,
        hook: impl Fn(&str, &Value) -> Option<Option<Value>> + Send + Sync + 'static,
    ) -> Self {
        self.hooks.push(Arc::new(hook));
        self
    }
}

impl Redact for Redactor {
    fn field(&self, key: &str, value: Value) -> Option<Value> {
        for hook in &self.hooks {
            if let Some(answer) = hook(key, &value) {
                return answer;
            }
        }
        if is_secret_key(key) {
            return Some(Value::from(SECRET));
        }
        if is_content_key(key) {
            return Some(Value::from(omitted(&value)));
        }
        let max = if TRUSTED_KEYS.contains(&key) {
            MAX_TRUSTED_VALUE
        } else {
            MAX_VALUE
        };
        Some(match value {
            Value::String(s) => Value::from(cap(&s, max)),
            // A nested object or array is text for the log's purposes: bounded like a string.
            v @ (Value::Array(_) | Value::Object(_)) => {
                let text = v.to_string();
                if text.chars().count() > max {
                    Value::from(cap(&text, max))
                } else {
                    v
                }
            }
            v => v,
        })
    }

    fn message(&self, message: &str) -> String {
        cap(message, MAX_MESSAGE)
    }
}

/// 32-bit FNV-1a of the UTF-8 bytes as 8 lowercase hex digits: shows that two values were equal, not what they were (it is
/// not a cryptographic hash). The same function as Debrief's UI uses, so fingerprints match across the two.
pub fn fingerprint(text: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for b in text.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

/// Whether `key` names a secret whatever it contains: `ApiKey`, `serviceKey`, `api_key`, `Token`, `Password`,
/// `Authorization`, ... (but not `Keys` or `Keybinding`, which name things).
pub fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k == "auth"
        || k.ends_with("key")
        || k.contains("_key")
        || [
            "secret",
            "token",
            "password",
            "passwd",
            "credential",
            "cookie",
            "authorization",
            "apikey",
        ]
        .iter()
        .any(|s| k.contains(s))
}

/// Whether `key` names the content of a file, a record, a prompt or a query (exact names, ignoring case and `_`).
pub fn is_content_key(key: &str) -> bool {
    let k: String = key
        .chars()
        .filter(|c| *c != '_' && *c != '-')
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(
        k.as_str(),
        "content"
            | "contents"
            | "filecontent"
            | "filecontents"
            | "body"
            | "line"
            | "lines"
            | "text"
            | "payload"
            | "prompt"
            | "answer"
            | "query"
            | "ql"
            | "record"
            | "records"
            | "snippet"
            | "stdin"
            | "data"
    )
}

fn omitted(value: &Value) -> String {
    match value {
        Value::String(s) => format!("<omitted: {} characters>", s.chars().count()),
        _ => "<omitted>".to_string(),
    }
}

pub(crate) fn cap(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut s: String = text.chars().take(max).collect();
    s.push('…');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn secrets_are_masked_whatever_they_hold() {
        let r = Redactor::new();
        for key in [
            "ApiKey",
            "serviceKey",
            "api_key",
            "Token",
            "Password",
            "Authorization",
            "auth",
            "Set-Cookie",
        ] {
            assert_eq!(r.field(key, json!("hunter2")), Some(json!("***")), "{key}");
        }
        for key in ["Keys", "Keybinding", "Rows"] {
            assert_eq!(r.field(key, json!("v")), Some(json!("v")), "{key}");
        }
    }

    #[test]
    fn content_is_replaced_by_its_size() {
        let r = Redactor::new();
        assert_eq!(
            r.field("Content", json!("secret file body")),
            Some(json!("<omitted: 16 characters>"))
        );
        assert_eq!(
            r.field("file_contents", json!("abc")),
            Some(json!("<omitted: 3 characters>"))
        );
        assert_eq!(r.field("Query", json!({"a": 1})), Some(json!("<omitted>")));
        assert_eq!(
            r.field("QueryFingerprint", json!("0a1b2c3d")),
            Some(json!("0a1b2c3d"))
        );
    }

    #[test]
    fn long_values_are_cut_but_the_logs_own_long_fields_survive() {
        let r = Redactor::new();
        let long = "x".repeat(5000);
        let cut = r.field("Note", json!(long)).unwrap();
        assert_eq!(cut.as_str().unwrap().chars().count(), MAX_VALUE + 1);
        assert_eq!(
            r.field("Backtrace", json!("y".repeat(5000)))
                .unwrap()
                .as_str()
                .unwrap()
                .len(),
            5000
        );
        assert!(
            r.field("Backtrace", json!("y".repeat(20000)))
                .unwrap()
                .as_str()
                .unwrap()
                .ends_with('…')
        );
        let nested = r.field("Items", json!(["z".repeat(3000)])).unwrap();
        assert!(
            nested.is_string(),
            "an oversized array becomes bounded text"
        );
        assert_eq!(r.field("Items", json!([1, 2])), Some(json!([1, 2])));
        assert_eq!(
            r.message(&"m".repeat(9000)).chars().count(),
            MAX_MESSAGE + 1
        );
        assert_eq!(r.message("short"), "short");
    }

    #[test]
    fn hooks_go_first_and_can_drop_a_property() {
        let r = Redactor::new()
            .with_hook(|k, _| (k == "Path").then(|| Some(json!("<path>"))))
            .with_hook(|k, _| (k == "Internal").then_some(None))
            .with_hook(|k, v| (k == "Text" && v == "ok").then(|| Some(json!("kept"))));
        assert_eq!(r.field("Path", json!("/home/me/x")), Some(json!("<path>")));
        assert_eq!(r.field("Internal", json!(1)), None);
        assert_eq!(r.field("Text", json!("ok")), Some(json!("kept")));
        assert_eq!(
            r.field("Text", json!("no")),
            Some(json!("<omitted: 2 characters>"))
        );
        assert!(format!("{r:?}").contains("3 hooks"));
    }

    #[test]
    fn fingerprints_are_stable() {
        assert_eq!(fingerprint(""), "811c9dc5");
        assert_eq!(fingerprint("a"), "e40c292c");
        assert_eq!(fingerprint("abc"), fingerprint("abc"));
        assert_ne!(fingerprint("abc"), fingerprint("abd"));
    }
}
