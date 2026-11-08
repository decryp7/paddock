//! The one redactor of a report (plan X2): the union of the three designs the apps had.
//!
//! | layer | from | removes |
//! |---|---|---|
//! | [`redact_secrets`](crate::redact_secrets) | Pitcrew `redact.rs` + Debrief detectors | tokens, passwords, keys, JWTs, URL credentials, card numbers |
//! | [`Personal`] | Dyno `diagnostics.rs` | user names, home folders, profile paths, the machine's name, e-mail and IP addresses |
//! | field rules | `paddock-log`'s [`Redact`] | values of secret-like keys, file contents (content-like keys), over-long values |
//!
//! Every file of a report passes through here before it is counted, previewed or written; a section cannot add an
//! unredacted file ([`Content`](crate::Content) has no raw variant). Text goes through the first two layers; structured
//! data (settings, system facts) also through the field rules, key by key; a log line is parsed as CLEF JSON and each
//! string property redacted, so a replacement can never break the line.

use crate::personal::{Personal, RedactionCounts};
use crate::secrets::redact_secrets;
use paddock_env::Vars;
use paddock_log::{Redact, is_secret_key};
use regex::{Regex, RegexBuilder};
use serde_json::{Map, Value};
use std::sync::Arc;

/// Longest pattern an app may add (characters) and largest compiled size (bytes): the same limits as user regexes.
pub const MAX_PATTERN_LEN: usize = 500;
/// Largest compiled app pattern, bytes.
pub const MAX_PATTERN_BYTES: usize = 256 * 1024;
/// Most patterns an app may add.
pub const MAX_PATTERNS: usize = 32;

/// Why an app's pattern was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError(pub String);

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PatternError {}

/// The names a host answers to: as given and its first label (`Alexs-MBP.local` and `Alexs-MBP`).
fn host_name_forms(name: &str) -> Vec<String> {
    let name = name.trim();
    let first = name.split('.').next().unwrap_or(name);
    let mut forms = vec![name.to_string()];
    if first != name {
        forms.push(first.to_string());
    }
    forms
}

/// The machine's name from the OS (`gethostname`). A GUI app's environment has no `HOSTNAME`, so the variables alone miss it.
/// Windows keeps `COMPUTERNAME` in every process's environment, so only Unix needs the call.
fn os_host_name() -> Option<String> {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: `buf` is writable for its whole length, which is passed; the call writes at most that many bytes.
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
        if rc != 0 {
            return None;
        }
        let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        let name = String::from_utf8_lossy(&buf[..end]).into_owned();
        (!name.is_empty()).then_some(name)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Removes secrets, personal data and file contents from what goes into a report. Cheap to clone.
#[derive(Clone)]
pub struct Redactor {
    personal: Personal,
    fields: Arc<dyn Redact>,
    patterns: Vec<(Regex, String)>,
}

impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Redactor({} app patterns)", self.patterns.len())
    }
}

impl Default for Redactor {
    fn default() -> Self {
        Redactor::new()
    }
}

impl Redactor {
    /// The default rules (secrets, e-mail and IP addresses, unknown profile paths, `paddock-log`'s field rules), nothing
    /// machine-specific. Use [`Redactor::for_machine`] to add this machine's user, home folder and name.
    pub fn new() -> Self {
        Redactor {
            personal: Personal::default(),
            fields: Arc::new(paddock_log::Redactor::new()),
            patterns: Vec::new(),
        }
    }

    /// The default rules plus the user, home folder and machine name found in `vars` (`USER`/`USERNAME`, `HOME`/`USERPROFILE`,
    /// `HOSTNAME`/`COMPUTERNAME`/`HOST`); a variable that is not set is skipped.
    pub fn for_machine(vars: &dyn Vars) -> Self {
        Redactor::for_machine_named(vars, os_host_name().as_deref())
    }

    /// [`Redactor::for_machine`] with the machine's name given (tests; the name an app got from its own OS call).
    pub fn for_machine_named(vars: &dyn Vars, host_name: Option<&str>) -> Self {
        let get = |names: &[&str]| -> Vec<String> {
            names
                .iter()
                .filter_map(|n| vars.get(n))
                .map(|v| v.to_string_lossy().into_owned())
                .collect()
        };
        Redactor::new().with_personal(
            &get(&["USER", "USERNAME", "LOGNAME"]),
            &get(&["HOME", "USERPROFILE"]),
            &{
                let mut machines = get(&["HOSTNAME", "COMPUTERNAME", "HOST"]);
                machines.extend(host_name.into_iter().flat_map(host_name_forms));
                machines
            },
        )
    }

    /// Adds user names, home folders and machine names to remove (empty and one-letter entries are ignored).
    pub fn with_personal(
        mut self,
        users: &[String],
        homes: &[String],
        machines: &[String],
    ) -> Self {
        let keep_local = self.personal.keeps_local_addresses();
        self.personal = Personal::new(users, homes, machines);
        if keep_local {
            self.personal = self.personal.keep_local_addresses();
        }
        self
    }

    /// Keeps loopback, unspecified and private (RFC 1918) addresses, which show how this machine is set up and say nothing
    /// about its owner (see [`Personal::keep_local_addresses`]). Off by default: every address is replaced. Only for a report
    /// the user reads and saves; never for text that leaves the machine (a search seed). Works before or after
    /// [`Redactor::with_personal`].
    pub fn keep_local_addresses(mut self) -> Self {
        self.personal = self.personal.keep_local_addresses();
        self
    }

    /// Replaces the field rules (default: `paddock-log`'s [`Redactor`](paddock_log::Redactor)); an app uses the same rules
    /// here as in its log (`LogConfig::with_redactor`).
    pub fn with_field_rules(mut self, rules: impl Redact) -> Self {
        self.fields = Arc::new(rules);
        self
    }

    /// An app's own pattern: every match is replaced by `replacement` (Pitcrew adds its runner UUIDs this way). At most
    /// [`MAX_PATTERNS`], each up to [`MAX_PATTERN_LEN`] characters and [`MAX_PATTERN_BYTES`] compiled.
    pub fn with_pattern(mut self, pattern: &str, replacement: &str) -> Result<Self, PatternError> {
        if self.patterns.len() >= MAX_PATTERNS {
            return Err(PatternError(format!(
                "At most {MAX_PATTERNS} redaction patterns are allowed."
            )));
        }
        if pattern.chars().count() > MAX_PATTERN_LEN {
            return Err(PatternError(format!(
                "A redaction pattern may have at most {MAX_PATTERN_LEN} characters."
            )));
        }
        let re = RegexBuilder::new(pattern)
            .size_limit(MAX_PATTERN_BYTES)
            .build()
            .map_err(|e| PatternError(format!("The redaction pattern is not valid: {e}")))?;
        self.patterns.push((re, replacement.to_string()));
        Ok(self)
    }

    /// Redacts free text: the app's patterns, secrets, then personal data.
    pub fn text(&self, text: &str) -> (String, RedactionCounts) {
        let mut counts = RedactionCounts::default();
        let mut out = text.to_string();
        for (re, with) in &self.patterns {
            let n = re.find_iter(&out).count();
            if n > 0 {
                out = re.replace_all(&out, with.as_str()).into_owned();
                counts.secrets += n as u32;
            }
        }
        let (s, n) = redact_secrets(&out);
        counts.secrets += n;
        let (s, personal) = self.personal.apply(&s);
        counts.add(&personal);
        (s, counts)
    }

    /// Redacts structured data key by key: `paddock-log`'s field rules (a secret key's value becomes `***`, a content key's
    /// value a size, long values are cut), then [`Redactor::text`] on every string. Nested objects and arrays are walked.
    pub fn json(&self, value: &Value) -> (Value, RedactionCounts) {
        let mut counts = RedactionCounts::default();
        let out = self.walk(value, &mut counts);
        (out, counts)
    }

    /// Redacts the strings of structured data with [`Redactor::text`], at any depth, and nothing else: keys, numbers and
    /// the shape stay, and the field rules of [`Redactor::json`] (masked secret keys, omitted contents, cut long values) are
    /// not applied. For data whose keys are fixed names and whose values are free text (a report's breadcrumbs, processes
    /// or snapshot details).
    pub fn text_in_json(&self, value: &Value) -> (Value, RedactionCounts) {
        let mut counts = RedactionCounts::default();
        let out = self.text_walk(value, &mut counts);
        (out, counts)
    }

    fn text_walk(&self, value: &Value, counts: &mut RedactionCounts) -> Value {
        match value {
            Value::String(_) => self.string_leaf(value.clone(), counts),
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| self.text_walk(v, counts)).collect())
            }
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), self.text_walk(v, counts)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    fn walk(&self, value: &Value, counts: &mut RedactionCounts) -> Value {
        match value {
            Value::Object(map) => {
                let mut out = Map::new();
                for (key, v) in map {
                    if is_secret_name(key) && !paddock_log::is_secret_key(key) {
                        counts.fields += 1;
                        out.insert(key.clone(), Value::from(paddock_log::SECRET));
                        continue;
                    }
                    let nested = matches!(v, Value::Object(_) | Value::Array(_));
                    // A container under an ordinary key is walked; under a secret or content key the rules take it whole.
                    let walked = if nested
                        && !paddock_log::is_secret_key(key)
                        && !paddock_log::is_content_key(key)
                    {
                        self.walk(v, counts)
                    } else {
                        v.clone()
                    };
                    match self.fields.field(key, walked.clone()) {
                        None => counts.fields += 1,
                        Some(kept) => {
                            if kept != walked {
                                counts.fields += 1;
                            }
                            out.insert(
                                key.clone(),
                                if nested {
                                    kept
                                } else {
                                    self.string_leaf(kept, counts)
                                },
                            );
                        }
                    }
                }
                Value::Object(out)
            }
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| self.walk(v, counts)).collect())
            }
            other => self.string_leaf(other.clone(), counts),
        }
    }

    fn string_leaf(&self, value: Value, counts: &mut RedactionCounts) -> Value {
        match value {
            Value::String(s) => {
                let (t, c) = self.text(&s);
                counts.add(&c);
                Value::String(t)
            }
            other => other,
        }
    }

    /// Redacts one line of a log. A CLEF JSON line is parsed and each string property redacted (a secret-named property is
    /// masked); the log already applied its field rules when it wrote the line, so content keys are not touched again
    /// (an app hook may have allowed them on purpose). A line that is not JSON is redacted as text.
    pub fn log_line(&self, line: &str) -> (String, RedactionCounts) {
        let mut counts = RedactionCounts::default();
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(line) {
            let out = self.log_object(map, &mut counts);
            return (Value::Object(out).to_string(), counts);
        }
        let (t, c) = self.text(line);
        (t, c)
    }

    /// A CLEF object: a secret-named property is masked whatever its value (string, array or object), every other string is
    /// redacted as text, and containers are walked to any depth.
    fn log_object(
        &self,
        map: Map<String, Value>,
        counts: &mut RedactionCounts,
    ) -> Map<String, Value> {
        let mut out = Map::new();
        for (key, v) in map {
            let v = if is_secret_name(&key)
                && v.as_str() != Some(paddock_log::SECRET)
                && !v.is_null()
            {
                counts.fields += 1;
                Value::from(paddock_log::SECRET)
            } else {
                self.log_value(v, counts)
            };
            out.insert(key, v);
        }
        out
    }

    fn log_value(&self, v: Value, counts: &mut RedactionCounts) -> Value {
        match v {
            Value::String(s) => {
                let (t, c) = self.text(&s);
                counts.add(&c);
                Value::String(t)
            }
            Value::Object(m) => Value::Object(self.log_object(m, counts)),
            Value::Array(items) => Value::Array(
                items
                    .into_iter()
                    .map(|i| self.log_value(i, counts))
                    .collect(),
            ),
            other => other,
        }
    }
}

/// A key whose value is a secret: `paddock-log`'s rule plus the short names it leaves out on purpose (`pass`, `pw`, `sig`).
fn is_secret_name(key: &str) -> bool {
    is_secret_key(key)
        || ["pass", "pw", "sig", "signature"]
            .iter()
            .any(|k| key.eq_ignore_ascii_case(k))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_env::MapVars;
    use serde_json::json;

    fn machine() -> Redactor {
        Redactor::for_machine(
            &MapVars::new()
                .with("USER", "alex")
                .with("HOME", "/Users/alex")
                .with("HOSTNAME", "ALEX-PC"),
        )
    }

    #[test]
    fn text_goes_through_secrets_then_personal_data() {
        let (s, c) = machine().text("alex at ALEX-PC opened /Users/alex/x with token=abc123 from 10.0.0.7 mail alex@example.com");
        assert_eq!(
            s,
            "<user> at <machine> opened ~/x with token=[redacted] from <ip> mail <email>"
        );
        assert_eq!(
            (
                c.secrets,
                c.user_names,
                c.machine_names,
                c.home_paths,
                c.ip_addresses,
                c.emails
            ),
            (1, 1, 1, 1, 1, 1)
        );
        assert_eq!(c.total(), 6);
    }

    #[test]
    fn a_secret_inside_a_path_is_removed_whole() {
        let (s, _) = Redactor::new().text("GET https://bob:hunter2@host/Users/alex/a");
        assert_eq!(s, "GET https://[redacted]@host/Users/<user>/a");
    }

    #[test]
    fn structured_data_follows_the_logs_field_rules() {
        let settings = json!({
            "theme": "dark",
            "apiKey": "sk-123456",
            "body": "line one\nline two",
            "nested": { "password": "x", "path": "/Users/alex/Documents", "list": ["a@b.example", 3, true] },
            "long": "x".repeat(2000),
        });
        let (out, c) = machine().json(&settings);
        assert_eq!(out["theme"], "dark");
        assert_eq!(out["apiKey"], "***");
        assert_eq!(out["body"], "<omitted: 17 characters>");
        assert_eq!(out["nested"]["password"], "***");
        assert_eq!(out["nested"]["path"], "~/Documents");
        assert_eq!(out["nested"]["list"], json!(["<email>", 3, true]));
        assert_eq!(
            out["long"].as_str().map(|s| s.chars().count()),
            Some(paddock_log::MAX_VALUE + 1)
        );
        // Masked and cut properties are counted: apiKey, body, password and long.
        assert_eq!(c.fields, 4);
        assert!(c.home_paths >= 1 && c.emails == 1);
    }

    #[test]
    fn a_rule_that_drops_a_property_removes_it_and_is_counted() {
        let rules =
            paddock_log::Redactor::new().with_hook(|key, _| (key == "internal").then_some(None));
        let (out, c) = Redactor::new()
            .with_field_rules(rules)
            .json(&json!({ "internal": 1, "kept": 2 }));
        assert_eq!(out, json!({ "kept": 2 }));
        assert_eq!(c.fields, 1);
    }

    #[test]
    fn a_log_line_stays_one_valid_json_line() {
        let line = json!({
            "@t": "2026-10-24T10:00:00Z", "@l": "Error", "@m": "failed for alex in C:\\Users\\alex\\x",
            "Token": "abc", "Path": "/home/carol/y", "Count": 3, "Line": "<omitted: 20 characters>",
        })
        .to_string();
        let (out, c) = machine().log_line(&line);
        let back: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            back["@m"], "failed for <user> in C:~\\x",
            "the machine's own home folder becomes ~"
        );
        assert_eq!(back["Token"], "***");
        assert_eq!(back["Path"], "/home/<user>/y");
        assert_eq!(back["Count"], 3);
        // Already omitted content is not described a second time.
        assert_eq!(back["Line"], "<omitted: 20 characters>");
        assert!(!out.contains('\n'));
        assert!(c.fields >= 1 && c.home_paths >= 1);
        // Not JSON: plain text rules.
        let (t, _) = machine().log_line("panic at /Users/alex/src/main.rs:3");
        assert_eq!(t, "panic at ~/src/main.rs:3");
        // Not an object: text rules too.
        assert_eq!(Redactor::new().log_line("[1,2]").0, "[1,2]");
    }

    #[test]
    fn app_patterns_are_limited_and_applied_first() {
        let r = Redactor::new()
            .with_pattern(r"runner-[0-9a-f]{8}", "<runner>")
            .unwrap();
        let (s, c) = r.text("runner-0a1b2c3d started");
        assert_eq!((s.as_str(), c.secrets), ("<runner> started", 1));
        assert!(
            Redactor::new()
                .with_pattern("(", "x")
                .unwrap_err()
                .to_string()
                .contains("not valid")
        );
        assert!(
            Redactor::new()
                .with_pattern(&"a".repeat(MAX_PATTERN_LEN + 1), "x")
                .is_err()
        );
        assert!(
            Redactor::new()
                .with_pattern("(?:a{1000}){1000}", "x")
                .is_err(),
            "compiled size is limited"
        );
        let mut r = Redactor::new();
        for i in 0..MAX_PATTERNS {
            r = r.with_pattern(&format!("p{i}"), "x").unwrap();
        }
        assert!(
            r.with_pattern("one-more", "x")
                .unwrap_err()
                .to_string()
                .contains("At most")
        );
    }

    #[test]
    fn the_default_redactor_has_the_default_rules() {
        let (s, c) = Redactor::default().text("token=abc and 10.0.0.1");
        assert_eq!(
            (s.as_str(), c.secrets, c.ip_addresses),
            ("token=[redacted] and <ip>", 1, 1)
        );
    }

    #[test]
    fn machine_variables_that_are_missing_or_tiny_are_skipped() {
        let r = Redactor::for_machine(&MapVars::new().with("USER", "x"));
        assert_eq!(r.text("x marks").0, "x marks");
        assert_eq!(format!("{r:?}"), "Redactor(0 app patterns)");
    }

    #[test]
    fn a_log_line_redacts_nested_arrays_and_objects() {
        let line = json!({
            "@m": "x",
            "Peers": [{ "Host": "ALEX-PC", "Path": "C:\\Users\\carol\\a.evtx" }, ["/home/dave/z", 7]],
            "Deep": { "A": { "B": [{ "Mail": "bob@example.org", "Ip": "203.0.113.9", "Local": "127.0.0.1" }] } },
            "Meta": { "apiKey": ["x"] },
        })
        .to_string();
        let (out, c) = machine().log_line(&line);
        let back: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(back["Peers"][0]["Host"], "<machine>");
        assert_eq!(back["Peers"][0]["Path"], "C:\\Users\\<user>\\a.evtx");
        assert_eq!(back["Peers"][1][0], "/home/<user>/z");
        assert_eq!(back["Peers"][1][1], 7);
        assert_eq!(back["Deep"]["A"]["B"][0]["Mail"], "<email>");
        assert_eq!(back["Deep"]["A"]["B"][0]["Ip"], "<ip>");
        assert_eq!(
            back["Deep"]["A"]["B"][0]["Local"], "<ip>",
            "by default local addresses go too"
        );
        assert_eq!(back["Meta"]["apiKey"], "***");
        assert!(
            c.machine_names == 1 && c.profile_paths == 2 && c.emails == 1 && c.ip_addresses == 2,
            "{c:?}"
        );
    }

    #[test]
    fn keeping_local_addresses_is_opt_in_and_survives_with_personal() {
        let text = "from 10.0.0.7 and 127.0.0.1 via 203.0.113.9 over fd7a:115c:a1e0::1 alex";
        assert_eq!(
            Redactor::new().text(text).0,
            "from <ip> and <ip> via <ip> over <ip> alex"
        );
        let kept = "from 10.0.0.7 and 127.0.0.1 via <ip> over <ip> <user>";
        // Before or after the machine's names are given, the choice stands.
        let a = Redactor::new()
            .keep_local_addresses()
            .with_personal(&["alex".into()], &[], &[]);
        let b = machine().keep_local_addresses();
        assert_eq!(a.text(text).0, kept);
        assert_eq!(b.text(text).0, kept);
        // A log line gets the same rule.
        let line = json!({"@m": "x", "Ip": ["10.0.0.7", "203.0.113.9"]}).to_string();
        let back: Value = serde_json::from_str(&a.log_line(&line).0).unwrap();
        assert_eq!(back["Ip"], json!(["10.0.0.7", "<ip>"]));
    }

    #[test]
    fn text_in_json_redacts_values_only_and_keeps_keys_and_shape() {
        let v = json!({
            "/Users/alex/a": "opened /Users/alex/a",
            "token": "token=abc123",
            "list": [1, true, null, "bob@example.com", { "k": "203.0.113.9" }],
            "n": 3,
        });
        let (out, c) = machine().text_in_json(&v);
        assert_eq!(
            out["/Users/alex/a"], "opened ~/a",
            "the key is kept, the value redacted"
        );
        assert_eq!(
            out["token"], "token=[redacted]",
            "no field rules: the key is not masked"
        );
        assert_eq!(
            out["list"],
            json!([1, true, null, "<email>", { "k": "<ip>" }])
        );
        assert_eq!(out["n"], 3);
        assert_eq!(
            (c.home_paths, c.secrets, c.emails, c.ip_addresses),
            (1, 1, 1, 1)
        );
        // Two keys that would collapse once redacted stay two.
        let (out, _) = machine().text_in_json(&json!({"/Users/alex/x": 1, "/Users/alex/y": 2}));
        assert_eq!(out.as_object().map(|m| m.len()), Some(2));
    }
}
