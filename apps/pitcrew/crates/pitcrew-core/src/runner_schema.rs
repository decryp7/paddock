//! Checking a runner `config.yaml` as the Advanced editor shows it: does it parse, and does every key the
//! gitea-runner 5.x reference documents have the type the runner expects (ADR 0011).
//!
//! The key table below is `config.reference.yaml` of gitea-runner 5.2.0 (`gitea-runner generate-config` prints it;
//! checked 2026-10-22). Unknown keys are **warnings** (a newer runner may add keys, and a typo is still worth
//! pointing at, with a suggestion); a wrongly typed value is an **error**, because the runner would refuse to start.
//! Values written as `${NAME}` read the runner's environment when it loads the file, so they pass any type check.
//!
//! Pure: text in, [`ConfigIssue`]s out.

use serde::{Deserialize, Serialize};
use serde_norway::Value;
use std::time::Duration;

/// How bad a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// The file must not be saved like this.
    Error,
    /// Worth a look; the file can be saved.
    Warning,
}

/// One finding about a configuration file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigIssue {
    /// Error or warning.
    pub severity: Severity,
    /// The key it is about, dotted (`runner.capacity`); empty for the file as a whole.
    pub path: String,
    /// The line in the text the check ran on (1-based), when known.
    pub line: Option<u32>,
    /// What is wrong and what to do, for the user.
    pub message: String,
}

impl ConfigIssue {
    /// An error about `path`.
    pub fn error(path: impl Into<String>, message: impl Into<String>) -> Self {
        ConfigIssue {
            severity: Severity::Error,
            path: path.into(),
            line: None,
            message: message.into(),
        }
    }

    /// A warning about `path`.
    pub fn warning(path: impl Into<String>, message: impl Into<String>) -> Self {
        ConfigIssue {
            severity: Severity::Warning,
            path: path.into(),
            line: None,
            message: message.into(),
        }
    }
}

/// Whether any finding is an error.
pub fn has_errors(issues: &[ConfigIssue]) -> bool {
    issues.iter().any(|i| i.severity == Severity::Error)
}

/// What a key holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Str,
    Int,
    Bool,
    Duration,
    Size,
    List,
    Map,
    Section,
    Any,
}

/// `config.reference.yaml` of gitea-runner 5.2.0, key by key.
const KEYS: &[(&str, Kind)] = &[
    ("log", Kind::Section),
    ("log.level", Kind::Str),
    ("log.job", Kind::Section),
    ("log.job.dir", Kind::Str),
    ("log.job.retention", Kind::Duration),
    ("log.job.max_size", Kind::Size),
    ("runner", Kind::Section),
    ("runner.file", Kind::Str),
    ("runner.capacity", Kind::Int),
    ("runner.envs", Kind::Map),
    ("runner.env_file", Kind::Str),
    ("runner.timeout", Kind::Duration),
    ("runner.shutdown_timeout", Kind::Duration),
    ("runner.insecure", Kind::Bool),
    ("runner.client_cert_file", Kind::Str),
    ("runner.client_key_file", Kind::Str),
    ("runner.fetch_timeout", Kind::Duration),
    ("runner.fetch_interval", Kind::Duration),
    ("runner.fetch_interval_max", Kind::Duration),
    ("runner.workdir_cleanup_age", Kind::Duration),
    ("runner.idle_cleanup_interval", Kind::Duration),
    ("runner.log_report_interval", Kind::Duration),
    ("runner.log_report_max_latency", Kind::Duration),
    ("runner.log_report_batch_size", Kind::Int),
    ("runner.log_report_level", Kind::Str),
    ("runner.state_report_interval", Kind::Duration),
    ("runner.report_close_timeout", Kind::Duration),
    ("runner.github_mirror", Kind::Str),
    ("runner.extra_headers", Kind::Map),
    ("runner.action_shallow_clone", Kind::Bool),
    ("runner.set_act_env", Kind::Bool),
    ("runner.patch_actions", Kind::Bool),
    ("runner.labels", Kind::List),
    ("runner.allocate_pty", Kind::Bool),
    ("runner.default_image", Kind::Str),
    ("runner.tool_cache_mode", Kind::Str),
    ("runner.post_task_script", Kind::Str),
    ("runner.post_task_script_timeout", Kind::Duration),
    ("runner.hooks", Kind::Section),
    ("runner.hooks.job_started", Kind::Str),
    ("runner.hooks.job_completed", Kind::Str),
    ("cache", Kind::Section),
    ("cache.enabled", Kind::Bool),
    ("cache.dir", Kind::Str),
    ("cache.host", Kind::Str),
    ("cache.port", Kind::Int),
    ("cache.external_server", Kind::Str),
    ("cache.external_secret", Kind::Str),
    ("cache.external_secret_file", Kind::Str),
    ("cache.s3", Kind::Section),
    ("cache.s3.endpoint", Kind::Str),
    ("cache.s3.region", Kind::Str),
    ("cache.s3.bucket", Kind::Str),
    ("cache.s3.prefix", Kind::Str),
    ("cache.s3.path_style", Kind::Bool),
    ("cache.s3.access_key_file", Kind::Str),
    ("cache.s3.secret_key_file", Kind::Str),
    ("cache.s3.access_key", Kind::Str),
    ("cache.s3.secret_key", Kind::Str),
    ("cache.offline_mode", Kind::Bool),
    ("cache.v2", Kind::Bool),
    ("cache.retention", Kind::Duration),
    ("cache.repo_size_limit", Kind::Size),
    ("cache.size_limit", Kind::Size),
    ("cache.sweep_interval", Kind::Duration),
    ("container", Kind::Section),
    ("container.network", Kind::Str),
    ("container.network_mode", Kind::Str),
    ("container.network_create_options", Kind::Section),
    ("container.network_create_options.enable_ipv4", Kind::Bool),
    ("container.network_create_options.enable_ipv6", Kind::Bool),
    ("container.privileged", Kind::Bool),
    ("container.options", Kind::Str),
    ("container.workdir_parent", Kind::Str),
    ("container.valid_volumes", Kind::List),
    ("container.docker_host", Kind::Str),
    ("container.force_pull", Kind::Bool),
    ("container.force_rebuild", Kind::Bool),
    ("container.require_docker", Kind::Bool),
    ("container.docker_timeout", Kind::Duration),
    ("container.bind_workdir", Kind::Bool),
    ("container.service_ready_timeout", Kind::Duration),
    ("container.sweep", Kind::Bool),
    ("host", Kind::Section),
    ("host.workdir_parent", Kind::Str),
    ("kubernetes", Kind::Section),
    ("kubernetes.kubeconfig", Kind::Str),
    ("kubernetes.namespace", Kind::Str),
    ("kubernetes.pod_template", Kind::Any),
    ("kubernetes.pod_templates", Kind::Any),
    ("health_check", Kind::Section),
    ("health_check.enabled", Kind::Bool),
    ("health_check.min_free_disk_space_mb", Kind::Int),
    ("health_check.script", Kind::Str),
    ("health_check.interval", Kind::Duration),
    ("health_check.timeout", Kind::Duration),
    ("metrics", Kind::Section),
    ("metrics.enabled", Kind::Bool),
    ("metrics.addr", Kind::Str),
    ("metrics.readiness_grace", Kind::Duration),
];

/// Largest configuration file the editor works with.
pub const MAX_CONFIG_TEXT: usize = 256 * 1024;

/// Parses `text` as YAML. A syntax error is returned as an issue with its line.
pub fn parse(text: &str) -> Result<Value, ConfigIssue> {
    if text.len() > MAX_CONFIG_TEXT {
        return Err(ConfigIssue::error(
            "",
            format!("the file is larger than {} KB, which Pitcrew does not edit", MAX_CONFIG_TEXT / 1024),
        ));
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_norway::from_str::<Value>(text).map_err(|e| {
        let mut issue = ConfigIssue::error("", format!("this is not valid YAML: {}", clean_yaml_error(&e.to_string())));
        issue.line = e.location().and_then(|l| u32::try_from(l.line()).ok());
        issue
    })
}

/// The parser's message without its position suffix (the line is reported separately).
fn clean_yaml_error(msg: &str) -> String {
    let m = msg.split(" at line ").next().unwrap_or(msg);
    m.trim().to_string()
}

/// Checks a parsed document against the reference: types are errors, unknown keys are warnings.
pub fn check(value: &Value) -> Vec<ConfigIssue> {
    let mut out = vec![];
    match value {
        Value::Null => {}
        Value::Mapping(_) => walk(value, "", &mut out),
        _ => out.push(ConfigIssue::error(
            "",
            "the file must be a list of `section:` blocks (a YAML mapping) at the top level",
        )),
    }
    out
}

fn kind_of(path: &str) -> Option<Kind> {
    KEYS.iter().find(|(p, _)| *p == path).map(|(_, k)| *k)
}

fn walk(value: &Value, prefix: &str, out: &mut Vec<ConfigIssue>) {
    let Value::Mapping(map) = value else { return };
    let siblings: Vec<&str> = KEYS
        .iter()
        .map(|(p, _)| *p)
        .filter(|p| match prefix {
            "" => !p.contains('.'),
            _ => p
                .strip_prefix(prefix)
                .and_then(|r| r.strip_prefix('.'))
                .is_some_and(|r| !r.contains('.')),
        })
        .map(|p| p.rsplit('.').next().unwrap_or(p))
        .collect();
    for (k, v) in map {
        let Some(key) = scalar_text(k) else {
            out.push(ConfigIssue::error(prefix, "a key is not plain text"));
            continue;
        };
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match kind_of(&path) {
            None => {
                // Free-form below `kubernetes.pod_template*`; elsewhere an unknown key.
                if !siblings.is_empty() || prefix.is_empty() {
                    let hint = closest(&key, &siblings)
                        .map(|s| format!("; did you mean `{s}`?"))
                        .unwrap_or_default();
                    out.push(ConfigIssue::warning(
                        path,
                        format!("`{key}` is not a key of gitea-runner 5.x{hint} The runner ignores keys it does not know"),
                    ));
                }
            }
            Some(kind) => {
                if let Some(msg) = type_error(kind, v) {
                    out.push(ConfigIssue::error(path.clone(), msg));
                } else if kind == Kind::Section {
                    walk(v, &path, out);
                }
            }
        }
    }
}

fn scalar_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn is_env_ref(v: &Value) -> bool {
    matches!(v, Value::String(s) if s.contains("${"))
}

fn type_error(kind: Kind, v: &Value) -> Option<String> {
    if matches!(v, Value::Null) || is_env_ref(v) {
        return None;
    }
    let v = match v {
        Value::Tagged(t) => &t.value,
        other => other,
    };
    match kind {
        Kind::Str => (!matches!(v, Value::String(_) | Value::Number(_) | Value::Bool(_))).then(|| "this must be text".to_string()),
        Kind::Int => match v {
            Value::Number(n) if n.is_i64() || n.is_u64() => None,
            _ => Some("this must be a whole number".into()),
        },
        Kind::Bool => (!matches!(v, Value::Bool(_))).then(|| "this must be true or false".to_string()),
        Kind::Duration => match v {
            Value::String(s) if parse_duration(s).is_some() => None,
            Value::Number(n) if n.as_i64() == Some(0) => None,
            _ => Some("this must be a duration such as 30s, 5m or 3h (a number and a unit: ns, us, ms, s, m, h)".into()),
        },
        Kind::Size => match v {
            Value::String(s) if parse_size(s).is_some() => None,
            Value::Number(n) if n.as_u64().is_some() => None,
            _ => Some("this must be a size such as 512MB, 10GB or 1TiB, or a number of bytes".into()),
        },
        Kind::List => match v {
            Value::Sequence(items) if items.iter().all(|i| scalar_text(i).is_some() || matches!(i, Value::Null)) => None,
            Value::Sequence(_) => Some("every item of this list must be text".into()),
            _ => Some("this must be a list (one `- item` per line)".into()),
        },
        Kind::Map => match v {
            Value::Mapping(m) if m.values().all(|i| scalar_text(i).is_some() || matches!(i, Value::Null)) => None,
            Value::Mapping(_) => Some("every value of this block must be text".into()),
            _ => Some("this must be a block of `NAME: value` lines".into()),
        },
        Kind::Section => (!matches!(v, Value::Mapping(_))).then(|| "this must be a block of indented `key: value` lines".to_string()),
        Kind::Any => None,
    }
}

/// The closest of `candidates` to `key` when it is a likely typo (edit distance 1 to 3, scaled to the length).
fn closest<'a>(key: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let limit = (key.len() / 3).clamp(1, 3);
    candidates
        .iter()
        .map(|c| (edit_distance(key, c), *c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// A Go duration (`90s`, `1h30m`, `1.5h`, `0`): units ns, us, µs, ms, s, m, h; no sign, no spaces.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s == "0" {
        return Some(Duration::ZERO);
    }
    if s.is_empty() || s.len() > 64 {
        return None;
    }
    let mut rest = s;
    let mut total_ns = 0f64;
    while !rest.is_empty() {
        let num_end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
        if num_end == 0 {
            return None;
        }
        let n: f64 = rest[..num_end].parse().ok()?;
        rest = &rest[num_end..];
        let unit_end = rest.find(|c: char| c.is_ascii_digit() || c == '.').unwrap_or(rest.len());
        let unit = match &rest[..unit_end] {
            "ns" => 1.0,
            "us" | "µs" | "μs" => 1e3,
            "ms" => 1e6,
            "s" => 1e9,
            "m" => 60e9,
            "h" => 3600e9,
            _ => return None,
        };
        rest = &rest[unit_end..];
        total_ns += n * unit;
        if total_ns > 1e18 {
            return None;
        }
    }
    Some(Duration::from_nanos(total_ns as u64))
}

/// A size (`512MB`, `10 GB`, `1TiB`, `2048`): binary multiples either way, as the runner reads them.
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    if split == 0 {
        return None;
    }
    let n: f64 = s[..split].parse().ok()?;
    let mult: f64 = match s[split..].trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "k" | "kb" | "kib" => 1024.0,
        "m" | "mb" | "mib" => 1024.0 * 1024.0,
        "g" | "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tb" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    let bytes = n * mult;
    (bytes.is_finite() && bytes < 1.8e19).then_some(bytes as u64)
}

/// Checks `text` completely: syntax, then types and unknown keys, with line numbers where the key can be found.
pub fn check_text(text: &str) -> Vec<ConfigIssue> {
    let value = match parse(text) {
        Ok(v) => v,
        Err(issue) => return vec![issue],
    };
    let mut issues = check(&value);
    for i in &mut issues {
        if i.line.is_none() && !i.path.is_empty() {
            let path: Vec<String> = i.path.split('.').map(str::to_string).collect();
            i.line = crate::yaml_patch::line_of(text, &path).and_then(|l| u32::try_from(l + 1).ok());
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errs(text: &str) -> Vec<(Severity, String)> {
        check_text(text).into_iter().map(|i| (i.severity, i.path)).collect()
    }

    #[test]
    fn the_reference_shape_is_accepted() {
        let text = "log:\n  level: info\nrunner:\n  capacity: 2\n  timeout: 3h\n  fetch_interval: 2s\n  envs:\n    A: b\n  labels:\n    - \"a:host\"\ncache:\n  enabled: true\n  size_limit: 10GB\ncontainer:\n  valid_volumes: []\n  privileged: false\nhost:\n  workdir_parent:\n";
        assert!(check_text(text).is_empty(), "{:?}", check_text(text));
        assert!(check_text("").is_empty());
        assert!(check_text("# only a comment\n").is_empty());
    }

    #[test]
    fn syntax_errors_say_where() {
        let issues = check_text("runner:\n  capacity: [1\n");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
        assert!(
            issues[0].line.is_some() && issues[0].message.starts_with("this is not valid YAML"),
            "{issues:?}"
        );
        // Duplicate keys are a syntax error for the runner too.
        assert!(has_errors(&check_text("a: 1\na: 2\n")));
        let big = format!("# {}\n", "x".repeat(MAX_CONFIG_TEXT));
        assert!(check_text(&big)[0].message.contains("larger than"));
    }

    #[test]
    fn wrong_types_are_errors_with_a_line() {
        let issues =
            check_text("runner:\n  capacity: many\n  timeout: soon\n  insecure: maybe\n  labels: a\n  envs: [x]\n  fetch_interval: 5\n");
        let paths: Vec<_> = issues.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "runner.capacity",
                "runner.timeout",
                "runner.insecure",
                "runner.labels",
                "runner.envs",
                "runner.fetch_interval"
            ]
        );
        assert!(issues.iter().all(|i| i.severity == Severity::Error));
        assert_eq!(issues[0].line, Some(2));
        assert!(issues[1].message.contains("30s, 5m or 3h"));
        // Sections must be blocks; sizes must be sizes; list items must be text.
        assert!(has_errors(&check_text("runner: 5\n")));
        assert!(has_errors(&check_text("cache:\n  size_limit: lots\n")));
        assert!(has_errors(&check_text("runner:\n  labels:\n    - [a]\n")));
        assert!(has_errors(&check_text("runner:\n  envs:\n    A: [b]\n")));
        assert!(has_errors(&check_text("- a\n- b\n")));
        assert!(has_errors(&check_text("log:\n  level: [a]\n")));
        // A zero duration and a plain number of bytes are fine; so are null values.
        assert!(check_text("runner:\n  shutdown_timeout: 0\ncache:\n  size_limit: 2048\n  retention:\n").is_empty());
    }

    #[test]
    fn environment_references_pass_any_type() {
        assert!(check_text("runner:\n  capacity: ${CAPACITY}\n  insecure: \"${INSECURE}\"\n  timeout: ${T}\n").is_empty());
    }

    #[test]
    fn unknown_keys_are_warnings_with_a_suggestion() {
        let issues = check_text("runner:\n  captacity: 2\n  timeuot: 1h\nwhatever: 1\ncache:\n  enabld: true\n");
        assert_eq!(issues.len(), 4);
        assert!(issues.iter().all(|i| i.severity == Severity::Warning));
        assert!(issues[0].message.contains("did you mean `capacity`?"), "{}", issues[0].message);
        assert!(issues[1].message.contains("did you mean `timeout`?"));
        assert!(!issues[2].message.contains("did you mean"), "{}", issues[2].message);
        assert!(issues[3].message.contains("did you mean `enabled`?"));
        assert_eq!(issues[0].line, Some(2));
        // Free-form kubernetes templates are not checked.
        assert!(check_text("kubernetes:\n  pod_template:\n    spec:\n      anything: [1, 2]\n").is_empty());
    }

    #[test]
    fn every_documented_key_is_in_the_table_once() {
        let mut seen = std::collections::HashSet::new();
        for (p, _) in KEYS {
            assert!(seen.insert(*p), "{p} twice");
            if let Some((parent, _)) = p.rsplit_once('.') {
                assert_eq!(kind_of(parent), Some(Kind::Section), "{p}'s parent must be a section");
            }
        }
    }

    #[test]
    fn go_durations() {
        let d = |s| parse_duration(s).map(|d| d.as_millis());
        assert_eq!(d("0"), Some(0));
        assert_eq!(d("3h"), Some(3 * 3600 * 1000));
        assert_eq!(d("1h30m"), Some(90 * 60 * 1000));
        assert_eq!(d("1.5s"), Some(1500));
        assert_eq!(d("250ms"), Some(250));
        assert_eq!(d("5µs"), Some(0));
        for bad in [
            "",
            "3",
            "h",
            "3 h",
            "-3h",
            "3d",
            "1h-",
            "1..5s",
            &"1h".repeat(40),
            "99999999999999h",
        ] {
            assert_eq!(d(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("10GB"), Some(10 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("512 mb"), Some(512 * 1024 * 1024));
        assert_eq!(parse_size("1TiB"), Some(1024u64.pow(4)));
        assert_eq!(parse_size("2048"), Some(2048));
        assert_eq!(parse_size("1.5k"), Some(1536));
        assert_eq!(parse_size("0"), Some(0));
        for bad in ["", "GB", "10 apples", "-1", "99999999999999999999TB"] {
            assert_eq!(parse_size(bad), None, "{bad:?}");
        }
        let _ = errs("");
    }

    #[test]
    fn odd_keys_and_tags_do_not_trip_the_check() {
        let i = check_text("? [a, b]\n: 1\n1: x\ntrue: y\nrunner:\n  capacity: !!int 3\n");
        assert!(i.iter().any(|i| i.message.contains("not plain text")), "{i:?}");
        assert!(
            i.iter().filter(|i| i.severity == Severity::Warning).count() >= 2,
            "numbers and booleans are keys too: {i:?}"
        );
        assert!(!has_errors(&check_text("runner:\n  capacity: !custom 3\n")));
    }

    #[test]
    fn typos_are_matched_loosely() {
        assert_eq!(closest("captacity", &["capacity", "timeout"]), Some("capacity"));
        assert_eq!(closest("zzzzzz", &["capacity"]), None);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }
}
