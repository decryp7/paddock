//! The runner settings the Pitcrew form edits: a typed view of a few keys of the runner's `config.yaml`, their
//! validation, and the minimal [`Edit`]s that turn one set of settings into another (ADR 0011).
//!
//! Every field is "set" or "unset" (`None` / empty): unset means the key is absent and the runner uses its own
//! default, so a user who never touched a setting leaves no trace of it in the file. Nothing here reads or writes
//! files: [`RunnerSettings::from_text`] parses text, [`edits`] and [`crate::yaml_patch::apply`] produce text.
//!
//! Limits follow the gitea-runner 5.x reference (`config.reference.yaml`): `fetch_timeout` above 60 s is capped by
//! the runner, a label is `name`, `name:host` or `name:docker://image`, `container.*` only matters to runners with
//! container labels.

use crate::runner_schema::{parse, parse_duration, parse_size, ConfigIssue};
use crate::yaml_patch::{Edit, Scalar};
use serde::{Deserialize, Serialize};
use serde_norway::Value;
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

/// Most jobs one runner may run at once in the form.
pub const MAX_CAPACITY: u32 = 64;
/// Most labels (the same limit as `pitcrew.toml`).
pub const MAX_LABELS: usize = crate::config::MAX_LABELS;
/// Most environment variables.
pub const MAX_ENVS: usize = 64;
/// Most `valid_volumes` entries.
pub const MAX_VOLUMES: usize = 64;

/// The runner's own log level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Most detail.
    Trace,
    /// Debugging detail.
    Debug,
    /// The default.
    Info,
    /// Problems that did not stop work.
    Warn,
    /// Failures only.
    Error,
}

impl LogLevel {
    /// The word the runner reads.
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }

    fn parse(s: &str) -> Option<LogLevel> {
        match s.trim().to_ascii_lowercase().as_str() {
            "trace" => Some(LogLevel::Trace),
            "debug" => Some(LogLevel::Debug),
            "info" => Some(LogLevel::Info),
            "warn" | "warning" => Some(LogLevel::Warn),
            // `fatal` and `panic` are accepted by the runner and mean error.
            "error" | "fatal" | "panic" => Some(LogLevel::Error),
            _ => None,
        }
    }
}

/// The settings of the form. `None` / empty: the key is absent and the runner's default applies.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RunnerSettings {
    /// `runner.capacity`: jobs at the same time.
    pub capacity: Option<u32>,
    /// `runner.labels`: which jobs the runner takes and how it runs them.
    pub labels: Vec<String>,
    /// `runner.timeout`: longest a job may run (`3h`).
    pub timeout: Option<String>,
    /// `runner.fetch_interval`: how often it asks the server for work (`2s`).
    pub fetch_interval: Option<String>,
    /// `runner.fetch_timeout`: how long one such request may take (`5s`, at most `60s`).
    pub fetch_timeout: Option<String>,
    /// `runner.envs`: extra environment variables for jobs.
    pub envs: BTreeMap<String, String>,
    /// `cache.enabled`.
    pub cache_enabled: Option<bool>,
    /// `cache.dir`.
    pub cache_dir: Option<String>,
    /// `cache.size_limit`: cap for the whole cache (`10GB`; `0` is no cap).
    pub cache_size_limit: Option<String>,
    /// `container.network`.
    pub container_network: Option<String>,
    /// `container.privileged`.
    pub container_privileged: Option<bool>,
    /// `container.options`: extra `docker run` options.
    pub container_options: Option<String>,
    /// `container.valid_volumes`: volumes and bind mounts jobs may use.
    pub container_valid_volumes: Vec<String>,
    /// `log.level`.
    pub log_level: Option<LogLevel>,
}

impl RunnerSettings {
    /// The settings in `text`, plus the dotted keys it holds in a shape the form cannot show (they are left as they
    /// are when the form saves).
    pub fn from_text(text: &str) -> Result<(RunnerSettings, Vec<String>), ConfigIssue> {
        parse(text).map(|v| RunnerSettings::from_value(&v))
    }

    /// The settings of a parsed document.
    pub fn from_value(root: &Value) -> (RunnerSettings, Vec<String>) {
        let mut unreadable = vec![];
        let mut r = Reader {
            root,
            unreadable: &mut unreadable,
        };
        let s = RunnerSettings {
            capacity: r.int("runner.capacity"),
            labels: r.list("runner.labels"),
            timeout: r.text("runner.timeout"),
            fetch_interval: r.text("runner.fetch_interval"),
            fetch_timeout: r.text("runner.fetch_timeout"),
            envs: r.map("runner.envs"),
            cache_enabled: r.boolean("cache.enabled"),
            cache_dir: r.text("cache.dir"),
            cache_size_limit: r.text("cache.size_limit"),
            container_network: r.text("container.network"),
            container_privileged: r.boolean("container.privileged"),
            container_options: r.text("container.options"),
            container_valid_volumes: r.list("container.valid_volumes"),
            log_level: r.text("log.level").and_then(|l| LogLevel::parse(&l)),
        };
        (s, unreadable)
    }

    /// The same settings with text trimmed, blank text turned into "unset" and empty list items dropped.
    pub fn normalised(&self) -> RunnerSettings {
        let t = |o: &Option<String>| o.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let l = |v: &[String]| v.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>();
        RunnerSettings {
            capacity: self.capacity,
            labels: l(&self.labels),
            timeout: t(&self.timeout),
            fetch_interval: t(&self.fetch_interval),
            fetch_timeout: t(&self.fetch_timeout),
            envs: self
                .envs
                .iter()
                .map(|(k, v)| (k.trim().to_string(), v.clone()))
                .filter(|(k, _)| !k.is_empty())
                .collect(),
            cache_enabled: self.cache_enabled,
            cache_dir: t(&self.cache_dir),
            cache_size_limit: t(&self.cache_size_limit),
            container_network: t(&self.container_network),
            container_privileged: self.container_privileged,
            container_options: t(&self.container_options),
            container_valid_volumes: l(&self.container_valid_volumes),
            log_level: self.log_level,
        }
    }

    /// Whether the runner runs jobs in containers: a label with no type (the runner's default is docker), `docker`
    /// or `docker://image`; also true when there are no labels (they come from the registration, which Pitcrew does
    /// not read), so the container settings stay reachable.
    pub fn is_container_runner(&self) -> bool {
        self.labels.is_empty() || self.labels.iter().any(|l| label_kind(l) == LabelKind::Container)
    }
}

/// Reads dotted keys out of a parsed document.
struct Reader<'a> {
    root: &'a Value,
    unreadable: &'a mut Vec<String>,
}

impl Reader<'_> {
    fn get(&self, path: &str) -> Option<&Value> {
        let mut v = self.root;
        for seg in path.split('.') {
            v = v.as_mapping()?.get(seg)?;
        }
        (!v.is_null()).then_some(v)
    }

    fn text(&mut self, path: &str) -> Option<String> {
        let v = self.get(path)?;
        match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => {
                self.unreadable.push(path.into());
                None
            }
        }
    }

    fn int(&mut self, path: &str) -> Option<u32> {
        let v = self.get(path)?;
        match v.as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(n) => Some(n),
            None => {
                self.unreadable.push(path.into());
                None
            }
        }
    }

    fn boolean(&mut self, path: &str) -> Option<bool> {
        let v = self.get(path)?;
        match v.as_bool() {
            Some(b) => Some(b),
            None => {
                self.unreadable.push(path.into());
                None
            }
        }
    }

    fn list(&mut self, path: &str) -> Vec<String> {
        let Some(v) = self.get(path) else { return vec![] };
        match v.as_sequence() {
            Some(items) if items.iter().all(|i| i.is_string() || i.is_number() || i.is_bool()) => items
                .iter()
                .filter_map(|i| match i {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    Value::Bool(b) => Some(b.to_string()),
                    _ => None,
                })
                .collect(),
            _ => {
                self.unreadable.push(path.into());
                vec![]
            }
        }
    }

    fn map(&mut self, path: &str) -> BTreeMap<String, String> {
        let Some(v) = self.get(path) else { return BTreeMap::new() };
        let Some(m) = v.as_mapping() else {
            self.unreadable.push(path.into());
            return BTreeMap::new();
        };
        let mut out = BTreeMap::new();
        for (k, v) in m {
            let key = match k {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => {
                    self.unreadable.push(path.into());
                    return BTreeMap::new();
                }
            };
            let val = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Null => String::new(),
                _ => {
                    self.unreadable.push(path.into());
                    return BTreeMap::new();
                }
            };
            out.insert(key, val);
        }
        out
    }
}

/// What a label asks of the runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelKind {
    /// Runs on the machine itself (`name:host`).
    Host,
    /// Runs in a container (`name`, `name:docker`, `name:docker://image`).
    Container,
    /// Another type the runner may support (`lxc://`, `kubernetes`).
    Other,
}

/// The kind of a label.
pub fn label_kind(label: &str) -> LabelKind {
    match label.split_once(':').map(|(_, schema)| schema.trim()) {
        None => LabelKind::Container,
        Some("host") => LabelKind::Host,
        Some(s) if s == "docker" || s.starts_with("docker://") => LabelKind::Container,
        Some(_) => LabelKind::Other,
    }
}

/// The name part of a label (before the first colon).
pub fn label_name(label: &str) -> &str {
    label.split(':').next().unwrap_or(label)
}

/// Checks one label; `Err` is a message for the user.
pub fn check_label(label: &str) -> Result<(), String> {
    if label.is_empty() {
        return Err("a label is empty".into());
    }
    if label.len() > crate::config::MAX_LABEL_LEN {
        return Err(format!("`{label}` is longer than {} characters", crate::config::MAX_LABEL_LEN));
    }
    if label.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '\'') {
        return Err(format!("`{label}` contains spaces, quotes or control characters"));
    }
    let name = label_name(label);
    if name.is_empty() {
        return Err(format!("`{label}` has no name before the colon"));
    }
    if let Some((_, schema)) = label.split_once(':') {
        if schema.is_empty() || schema == "docker://" {
            return Err(format!(
                "`{label}` has nothing after the colon: use `{name}:host` or `{name}:docker://image`"
            ));
        }
    }
    Ok(())
}

fn push(issues: &mut Vec<ConfigIssue>, i: ConfigIssue) {
    issues.push(i);
}

/// Checks the settings; errors block saving, warnings do not.
pub fn validate(s: &RunnerSettings) -> Vec<ConfigIssue> {
    let mut out = vec![];
    if let Some(c) = s.capacity {
        if !(1..=MAX_CAPACITY).contains(&c) {
            push(
                &mut out,
                ConfigIssue::error(
                    "runner.capacity",
                    format!("capacity is how many jobs run at once: use a number from 1 to {MAX_CAPACITY}"),
                ),
            );
        } else if c > 4 && s.is_container_runner() {
            push(
                &mut out,
                ConfigIssue::warning(
                    "runner.capacity",
                    "with a high capacity every container job takes a network from Docker's address pool, which can run out; set a container network or keep the capacity low",
                ),
            );
        }
    }
    check_duration(
        &mut out,
        "runner.timeout",
        s.timeout.as_deref(),
        Duration::from_secs(60),
        Duration::from_secs(7 * 86_400),
        "the job timeout",
    );
    check_duration(
        &mut out,
        "runner.fetch_interval",
        s.fetch_interval.as_deref(),
        Duration::from_secs(1),
        Duration::from_secs(3600),
        "the fetch interval",
    );
    check_duration(
        &mut out,
        "runner.fetch_timeout",
        s.fetch_timeout.as_deref(),
        Duration::from_secs(1),
        Duration::from_secs(60),
        "the fetch timeout",
    );
    if let Some(sz) = s.cache_size_limit.as_deref() {
        if parse_size(sz).is_none() {
            push(
                &mut out,
                ConfigIssue::error(
                    "cache.size_limit",
                    "the cache size must look like 10GB, 512MB or 1TiB (0 is no limit)",
                ),
            );
        }
    }
    if s.cache_dir
        .as_deref()
        .is_some_and(|d| d.len() > 1024 || d.chars().any(char::is_control))
    {
        push(
            &mut out,
            ConfigIssue::error(
                "cache.dir",
                "the cache folder must be a path of at most 1024 characters without line breaks",
            ),
        );
    }
    // Labels.
    if s.labels.len() > MAX_LABELS {
        push(
            &mut out,
            ConfigIssue::error("runner.labels", format!("at most {MAX_LABELS} labels")),
        );
    }
    let mut names = HashSet::new();
    for l in &s.labels {
        if let Err(m) = check_label(l) {
            push(&mut out, ConfigIssue::error("runner.labels", m));
        } else if !names.insert(label_name(l).to_string()) {
            push(
                &mut out,
                ConfigIssue::error("runner.labels", format!("the label name `{}` is used twice", label_name(l))),
            );
        } else if label_kind(l) == LabelKind::Other {
            push(
                &mut out,
                ConfigIssue::warning(
                    "runner.labels",
                    format!("`{l}` is not a host or docker label; the runner must support its type"),
                ),
            );
        }
    }
    // Environment.
    if s.envs.len() > MAX_ENVS {
        push(&mut out, ConfigIssue::error("runner.envs", format!("at most {MAX_ENVS} variables")));
    }
    for (k, v) in &s.envs {
        let mut chars = k.chars();
        let ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && k.len() <= 64
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok {
            push(
                &mut out,
                ConfigIssue::error(
                    "runner.envs",
                    format!("`{k}` is not a variable name (letters, digits and _, not starting with a digit, at most 64)"),
                ),
            );
        } else if v.len() > 4096 || v.chars().any(char::is_control) {
            push(
                &mut out,
                ConfigIssue::error(
                    "runner.envs",
                    format!("the value of {k} must be at most 4096 characters without line breaks"),
                ),
            );
        }
    }
    // Containers.
    let container_set = s.container_network.is_some()
        || s.container_privileged.is_some()
        || s.container_options.is_some()
        || !s.container_valid_volumes.is_empty();
    if container_set && !s.labels.is_empty() && !s.is_container_runner() {
        push(
            &mut out,
            ConfigIssue::warning(
                "container",
                "none of the labels runs jobs in a container, so the container settings have no effect",
            ),
        );
    }
    if let Some(n) = s.container_network.as_deref() {
        if n.len() > 128 || n.chars().any(|c| c.is_whitespace() || c.is_control()) {
            push(
                &mut out,
                ConfigIssue::error(
                    "container.network",
                    "the network is `host`, `bridge` or the name of a Docker network (no spaces)",
                ),
            );
        }
    }
    if let Some(o) = s.container_options.as_deref() {
        if o.len() > 2048 || o.chars().any(char::is_control) {
            push(
                &mut out,
                ConfigIssue::error("container.options", "the options must be at most 2048 characters on one line"),
            );
        } else if o.split_whitespace().any(|w| w == "--privileged") {
            push(
                &mut out,
                ConfigIssue::warning("container.options", "use the Privileged switch instead of `--privileged` here"),
            );
        }
    }
    if s.container_privileged == Some(true) {
        push(
            &mut out,
            ConfigIssue::warning(
                "container.privileged",
                "privileged containers can reach the host's devices and kernel; turn this on only for jobs that need Docker-in-Docker",
            ),
        );
    }
    if s.container_valid_volumes.len() > MAX_VOLUMES {
        push(
            &mut out,
            ConfigIssue::error("container.valid_volumes", format!("at most {MAX_VOLUMES} entries")),
        );
    }
    for v in &s.container_valid_volumes {
        if v.len() > 512 || v.chars().any(char::is_control) {
            push(
                &mut out,
                ConfigIssue::error(
                    "container.valid_volumes",
                    format!("`{}` is not a volume pattern", v.chars().take(40).collect::<String>()),
                ),
            );
        } else if v == "**" {
            push(
                &mut out,
                ConfigIssue::warning("container.valid_volumes", "`**` lets jobs mount any folder of this machine"),
            );
        }
    }
    out
}

fn check_duration(out: &mut Vec<ConfigIssue>, path: &str, v: Option<&str>, min: Duration, max: Duration, what: &str) {
    let Some(v) = v else { return };
    match parse_duration(v) {
        None => push(out, ConfigIssue::error(path, format!("{what} must look like 30s, 5m or 3h"))),
        Some(d) if d < min || d > max => push(
            out,
            ConfigIssue::error(path, format!("{what} must be between {} and {}", show(min), show(max))),
        ),
        Some(_) => {}
    }
}

fn show(d: Duration) -> String {
    let s = d.as_secs();
    if s.is_multiple_of(86_400) {
        format!("{} d", s / 86_400)
    } else if s.is_multiple_of(3600) {
        format!("{} h", s / 3600)
    } else if s.is_multiple_of(60) {
        format!("{} min", s / 60)
    } else {
        format!("{s} s")
    }
}

fn path(p: &str) -> Vec<String> {
    p.split('.').map(str::to_string).collect()
}

/// The edits that turn `old` into `new`: one per setting that changed, nothing for the rest.
pub fn edits(old: &RunnerSettings, new: &RunnerSettings) -> Vec<Edit> {
    let (old, new) = (old.normalised(), new.normalised());
    let mut out = vec![];
    let int = |out: &mut Vec<Edit>, p: &str, a: Option<u32>, b: Option<u32>| {
        if a != b {
            out.push(match b {
                Some(n) => Edit::Set {
                    path: path(p),
                    value: Scalar::Int(i64::from(n)),
                },
                None => Edit::Remove { path: path(p) },
            });
        }
    };
    let text = |out: &mut Vec<Edit>, p: &str, a: &Option<String>, b: &Option<String>| {
        if a != b {
            out.push(match b {
                Some(s) => Edit::Set {
                    path: path(p),
                    value: Scalar::Str(s.clone()),
                },
                None => Edit::Remove { path: path(p) },
            });
        }
    };
    let boolean = |out: &mut Vec<Edit>, p: &str, a: Option<bool>, b: Option<bool>| {
        if a != b {
            out.push(match b {
                Some(v) => Edit::Set {
                    path: path(p),
                    value: Scalar::Bool(v),
                },
                None => Edit::Remove { path: path(p) },
            });
        }
    };
    let list = |out: &mut Vec<Edit>, p: &str, a: &[String], b: &[String]| {
        if a != b {
            out.push(if b.is_empty() {
                Edit::Remove { path: path(p) }
            } else {
                Edit::SetList {
                    path: path(p),
                    items: b.to_vec(),
                }
            });
        }
    };
    int(&mut out, "runner.capacity", old.capacity, new.capacity);
    list(&mut out, "runner.labels", &old.labels, &new.labels);
    text(&mut out, "runner.timeout", &old.timeout, &new.timeout);
    text(&mut out, "runner.fetch_interval", &old.fetch_interval, &new.fetch_interval);
    text(&mut out, "runner.fetch_timeout", &old.fetch_timeout, &new.fetch_timeout);
    if old.envs != new.envs {
        out.push(if new.envs.is_empty() {
            Edit::Remove { path: path("runner.envs") }
        } else {
            Edit::SetMap {
                path: path("runner.envs"),
                entries: new.envs.clone().into_iter().collect(),
            }
        });
    }
    boolean(&mut out, "cache.enabled", old.cache_enabled, new.cache_enabled);
    text(&mut out, "cache.dir", &old.cache_dir, &new.cache_dir);
    text(&mut out, "cache.size_limit", &old.cache_size_limit, &new.cache_size_limit);
    text(&mut out, "container.network", &old.container_network, &new.container_network);
    boolean(&mut out, "container.privileged", old.container_privileged, new.container_privileged);
    text(&mut out, "container.options", &old.container_options, &new.container_options);
    list(
        &mut out,
        "container.valid_volumes",
        &old.container_valid_volumes,
        &new.container_valid_volumes,
    );
    if old.log_level != new.log_level {
        out.push(match new.log_level {
            Some(l) => Edit::Set {
                path: path("log.level"),
                value: Scalar::Str(l.as_str().into()),
            },
            None => Edit::Remove { path: path("log.level") },
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml_patch::apply;

    const FILE: &str = "\
log:
  level: debug # chatty
runner:
  capacity: 2
  timeout: 3h
  envs:
    CI: \"true\"
  labels:
    - macos:host
    - ubuntu-latest:docker://img
cache:
  enabled: true
  size_limit: 10GB
container:
  network: host
  privileged: false
  options: --add-host=a:host-gateway
  valid_volumes:
    - data
";

    fn read(text: &str) -> RunnerSettings {
        RunnerSettings::from_text(text).unwrap().0
    }

    #[test]
    fn the_form_reads_the_documented_keys() {
        let s = read(FILE);
        assert_eq!(s.capacity, Some(2));
        assert_eq!(s.timeout.as_deref(), Some("3h"));
        assert_eq!(s.labels, vec!["macos:host", "ubuntu-latest:docker://img"]);
        assert_eq!(s.envs.get("CI").map(String::as_str), Some("true"));
        assert_eq!(s.cache_enabled, Some(true));
        assert_eq!(s.cache_size_limit.as_deref(), Some("10GB"));
        assert_eq!(s.container_network.as_deref(), Some("host"));
        assert_eq!(s.container_privileged, Some(false));
        assert_eq!(s.container_valid_volumes, vec!["data"]);
        assert_eq!(s.log_level, Some(LogLevel::Debug));
        assert_eq!(s.fetch_interval, None);
        assert!(read("").labels.is_empty());
        assert!(RunnerSettings::from_text("a: [1").is_err());
    }

    #[test]
    fn keys_in_a_shape_the_form_cannot_show_are_reported_not_lost() {
        let (s, unreadable) = RunnerSettings::from_text(
            "runner:\n  capacity: many\n  labels: x\n  envs:\n    A: [1]\n  timeout:\n    nested: 1\ncache:\n  enabled: maybe\n",
        )
        .unwrap();
        assert_eq!(s, RunnerSettings::default());
        assert_eq!(
            unreadable,
            vec!["runner.capacity", "runner.labels", "runner.timeout", "runner.envs", "cache.enabled"]
        );
        // Unrelated shapes: a section that is not a block, numbers and null values.
        let (s, un) =
            RunnerSettings::from_text("runner:\n  envs:\n    1: 2\n    B:\n  fetch_interval: 5\n  labels: [1, true]\nlog: 3\n").unwrap();
        assert_eq!(s.envs.get("B").map(String::as_str), Some(""));
        assert_eq!(s.envs.get("1").map(String::as_str), Some("2"));
        assert_eq!(s.fetch_interval.as_deref(), Some("5"));
        assert_eq!(s.labels, vec!["1", "true"]);
        assert!(un.is_empty());
        let (s, un) = RunnerSettings::from_text("runner:\n  envs:\n    ? [a]\n    : b\n").unwrap();
        assert!(s.envs.is_empty() && un == vec!["runner.envs"]);
        assert_eq!(
            RunnerSettings::from_text("log:\n  level: fatal\n").unwrap().0.log_level,
            Some(LogLevel::Error)
        );
        assert_eq!(RunnerSettings::from_text("log:\n  level: loud\n").unwrap().0.log_level, None);
    }

    #[test]
    fn nothing_changed_means_no_edits_and_no_change_to_the_file() {
        let s = read(FILE);
        assert!(edits(&s, &s).is_empty());
        let mut padded = s.clone();
        padded.timeout = Some("  3h ".into());
        padded.labels.push("  ".into());
        assert!(edits(&s, &padded).is_empty(), "white space is not a change");
        assert_eq!(apply(FILE, &edits(&s, &s)).unwrap(), FILE);
    }

    #[test]
    fn changed_settings_change_only_their_lines() {
        let old = read(FILE);
        let mut new = old.clone();
        new.capacity = Some(3);
        new.fetch_interval = Some("5s".into());
        new.labels = vec!["macos:host".into(), "linux:host".into()];
        new.envs.insert("NODE_ENV".into(), "ci".into());
        new.cache_enabled = Some(false);
        new.container_valid_volumes.clear();
        new.container_options = None;
        new.log_level = Some(LogLevel::Info);
        let out = apply(FILE, &edits(&old, &new)).unwrap();
        assert!(out.contains("  level: info # chatty\n"), "{out}");
        assert!(out.contains("  capacity: 3\n  timeout: 3h\n"));
        assert!(
            out.contains("    - macos:host\n    - \"linux:host\"\n") || out.contains("    - macos:host\n    - linux:host\n"),
            "{out}"
        );
        assert!(out.contains("    CI: \"true\"\n    NODE_ENV: ci\n"));
        assert!(out.contains("  enabled: false\n"));
        assert!(!out.contains("valid_volumes") && !out.contains("add-host"));
        assert!(out.contains("fetch_interval: 5s"));
        // Reading it back gives what was asked for.
        let back = read(&out);
        assert_eq!(back.normalised(), new.normalised());
        // Removing every setting removes the keys and keeps the sections.
        let out = apply(FILE, &edits(&old, &RunnerSettings::default())).unwrap();
        assert_eq!(read(&out), RunnerSettings::default());
        assert!(out.contains("runner:\n") && out.contains("cache:\n"));
    }

    fn msgs(s: &RunnerSettings) -> Vec<(bool, String)> {
        validate(s)
            .into_iter()
            .map(|i| (i.severity == crate::runner_schema::Severity::Error, i.path))
            .collect()
    }

    #[test]
    fn good_settings_pass() {
        assert!(validate(&read(FILE))
            .iter()
            .all(|i| i.severity == crate::runner_schema::Severity::Warning));
        assert!(validate(&RunnerSettings::default()).is_empty());
    }

    #[test]
    fn capacity_and_durations_have_clear_bounds() {
        let mut s = RunnerSettings {
            capacity: Some(0),
            ..Default::default()
        };
        assert_eq!(msgs(&s), vec![(true, "runner.capacity".into())]);
        s.capacity = Some(MAX_CAPACITY + 1);
        assert_eq!(msgs(&s), vec![(true, "runner.capacity".into())]);
        s.capacity = Some(8);
        assert_eq!(
            msgs(&s),
            vec![(false, "runner.capacity".into())],
            "high capacity of container jobs is a warning"
        );
        s.labels = vec!["a:host".into()];
        assert!(msgs(&s).is_empty());
        let s = RunnerSettings {
            timeout: Some("10s".into()),
            fetch_interval: Some("0s".into()),
            fetch_timeout: Some("90s".into()),
            ..Default::default()
        };
        let m = validate(&s);
        assert_eq!(m.len(), 3);
        assert!(m[0].message.contains("between 1 min and 7 d"), "{}", m[0].message);
        assert!(m[2].message.contains("between 1 s and 1 min"), "{}", m[2].message);
        let s = RunnerSettings {
            timeout: Some("soon".into()),
            ..Default::default()
        };
        assert!(validate(&s)[0].message.contains("30s, 5m or 3h"));
        let s = RunnerSettings {
            timeout: Some("90m".into()),
            fetch_timeout: Some("45s".into()),
            fetch_interval: Some("1h".into()),
            ..Default::default()
        };
        assert!(validate(&s).is_empty());
        assert_eq!(show(Duration::from_secs(7200)), "2 h");
        assert_eq!(show(Duration::from_secs(90)), "90 s");
    }

    #[test]
    fn labels_are_checked_one_by_one() {
        let s = RunnerSettings {
            labels: vec![
                "ok:host".into(),
                "ok:docker://i".into(),
                "bad label".into(),
                "x:".into(),
                ":host".into(),
                "y:docker://".into(),
                "z:lxc://a".into(),
                "".into(),
            ],
            ..Default::default()
        };
        let m = validate(&s);
        let text: Vec<_> = m.iter().map(|i| i.message.as_str()).collect();
        assert!(text.iter().any(|t| t.contains("`ok` is used twice")), "{text:?}");
        assert!(text.iter().any(|t| t.contains("spaces")));
        assert!(text.iter().any(|t| t.contains("`x:` has nothing after the colon")));
        assert!(text.iter().any(|t| t.contains("no name before the colon")));
        assert!(text.iter().any(|t| t.contains("`y:docker://` has nothing")));
        assert!(text.iter().any(|t| t.contains("not a host or docker label")));
        assert!(text.iter().any(|t| t.contains("is empty")));
        assert!(check_label(&"a".repeat(200)).is_err());
        let many = RunnerSettings {
            labels: (0..=MAX_LABELS).map(|n| format!("l{n}:host")).collect(),
            ..Default::default()
        };
        assert!(validate(&many).iter().any(|i| i.message.contains("at most")));
        assert_eq!(label_kind("a"), LabelKind::Container);
        assert_eq!(label_kind("a:docker"), LabelKind::Container);
        assert_eq!(label_kind("a:host"), LabelKind::Host);
        assert_eq!(label_kind("a:lxc://x"), LabelKind::Other);
        assert_eq!(label_name("a:b:c"), "a");
    }

    #[test]
    fn envs_cache_and_container_fields_are_checked() {
        let mut s = RunnerSettings::default();
        s.envs.insert("1BAD".into(), "x".into());
        s.envs.insert("GOOD".into(), "line\nbreak".into());
        s.cache_size_limit = Some("lots".into());
        s.cache_dir = Some("a\nb".into());
        s.container_network = Some("my net".into());
        s.container_options = Some("--privileged -v a:b".into());
        s.container_privileged = Some(true);
        s.container_valid_volumes = vec!["**".into(), "x\ny".into()];
        let m = validate(&s);
        let paths: Vec<_> = m
            .iter()
            .map(|i| (i.severity == crate::runner_schema::Severity::Error, i.path.as_str()))
            .collect();
        for want in [
            (true, "runner.envs"),
            (true, "cache.size_limit"),
            (true, "cache.dir"),
            (true, "container.network"),
            (false, "container.options"),
            (false, "container.privileged"),
            (false, "container.valid_volumes"),
            (true, "container.valid_volumes"),
        ] {
            assert!(paths.contains(&want), "{want:?} in {paths:?}");
        }
        s.container_options = Some("a\nb".into());
        assert!(msgs(&s).contains(&(true, "container.options".into())));
        let many = RunnerSettings {
            envs: (0..=MAX_ENVS).map(|n| (format!("V{n}"), "x".into())).collect(),
            container_valid_volumes: (0..=MAX_VOLUMES).map(|n| format!("v{n}")).collect(),
            ..Default::default()
        };
        assert_eq!(validate(&many).len(), 2);
        // Container settings on a host-only runner do nothing.
        let host = RunnerSettings {
            labels: vec!["mac:host".into()],
            container_network: Some("host".into()),
            ..Default::default()
        };
        assert_eq!(msgs(&host), vec![(false, "container".into())]);
        assert!(!host.is_container_runner());
        assert!(RunnerSettings::default().is_container_runner());
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use crate::yaml_patch::apply;
    use proptest::prelude::*;

    fn settings() -> impl Strategy<Value = RunnerSettings> {
        let text = || proptest::option::of("[a-zA-Z0-9][a-zA-Z0-9 ._/:-]{0,12}[a-zA-Z0-9]");
        (
            proptest::option::of(1u32..64),
            proptest::collection::vec("[a-z][a-z0-9-]{0,8}(:host|:docker://[a-z]{1,6})?", 0..4),
            (text(), text(), text()),
            proptest::collection::btree_map("[A-Z][A-Z0-9_]{0,6}", "[a-z0-9 ]{0,8}", 0..4),
            (proptest::option::of(any::<bool>()), text(), text()),
            (
                text(),
                proptest::option::of(any::<bool>()),
                text(),
                proptest::collection::vec("[a-z/*]{1,8}", 0..3),
            ),
            proptest::option::of(prop_oneof![
                Just(LogLevel::Trace),
                Just(LogLevel::Debug),
                Just(LogLevel::Info),
                Just(LogLevel::Warn),
                Just(LogLevel::Error)
            ]),
        )
            .prop_map(
                |(capacity, labels, (timeout, fi, ft), envs, (ce, cd, cs), (net, privileged, opts, vols), log_level)| RunnerSettings {
                    capacity,
                    labels,
                    timeout,
                    fetch_interval: fi,
                    fetch_timeout: ft,
                    envs: envs.into_iter().map(|(k, v)| (k, v.trim().to_string())).collect(),
                    cache_enabled: ce,
                    cache_dir: cd,
                    cache_size_limit: cs,
                    container_network: net,
                    container_privileged: privileged,
                    container_options: opts,
                    container_valid_volumes: vols,
                    log_level,
                },
            )
    }

    const BASE: &str = "# my runner\nlog:\n  level: warn # quiet\nrunner:\n  capacity: 4\n  # keep this note\n  timeout: 1h\n  labels:\n    - old:host\ncache:\n  dir: /tmp/cache\n";

    proptest! {
        #[test]
        fn what_the_form_saves_is_what_it_reads_back(target in settings()) {
            let (old, _) = RunnerSettings::from_text(BASE).unwrap();
            let out = apply(BASE, &edits(&old, &target)).unwrap();
            let (back, unreadable) = RunnerSettings::from_text(&out).unwrap();
            prop_assert!(unreadable.is_empty());
            prop_assert_eq!(back.normalised(), target.normalised());
            // Saving again changes nothing.
            prop_assert_eq!(apply(&out, &edits(&back, &target)).unwrap(), out.clone());
            // The comments of the original are still there.
            prop_assert!(out.contains("# my runner"));
            if target.normalised().timeout.is_some() {
                prop_assert!(out.contains("# keep this note"));
            }
        }

        #[test]
        fn validation_never_panics(s in settings(), extra in "(?s).{0,40}") {
            let mut s = s;
            s.timeout = Some(extra.clone());
            s.container_options = Some(extra.clone());
            s.labels.push(extra);
            let _ = validate(&s);
        }
    }
}
