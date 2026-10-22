//! What Pitcrew does with the processes a job leaves behind (ADR 0012): the settings in `pitcrew.toml`.
//!
//! Cleanup of leftovers is **on by default**: a process that a finished job started and that is still alive is
//! stopped after a "keep for" time, while the runner is idle. Some leftovers are meant to stay (a Gradle daemon makes
//! the next build fast), so rules give a process its own time, or "never stop":
//!
//! ```toml
//! [defaults.processes]
//! cleanup = true                          # turn the whole thing off with false
//! keep = { amount = 10, unit = "minutes" } # the default time after the job ended
//! stop_timeout_secs = 15                   # asked nicely first, then ended after this many seconds
//!
//! [[defaults.processes.rules]]
//! pattern = "*GradleDaemon*"               # matches the name, the path or the command line
//! keep = { amount = 3, unit = "hours" }
//!
//! [[defaults.processes.rules]]
//! pattern = "adb"
//! keep = "never"                           # never stop
//! ```
//!
//! A runner can override all of it with `[runners.processes]` (its own `cleanup`, `keep` and `rules`, which are tried
//! before the defaults' rules). Everything here is pure.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Most rules in one list.
pub const MAX_RULES: usize = 64;
/// Longest rule pattern.
pub const MAX_PATTERN_LEN: usize = 256;
/// Longest keep time, in days.
pub const MAX_KEEP_DAYS: u32 = 30;
/// Bounds of the stop timeout, in seconds.
pub const STOP_TIMEOUT_RANGE: std::ops::RangeInclusive<u32> = 1..=300;

/// A unit of a keep time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeUnit {
    /// Minutes.
    Minutes,
    /// Hours.
    Hours,
    /// Days.
    Days,
}

impl TimeUnit {
    fn seconds(self) -> u64 {
        match self {
            TimeUnit::Minutes => 60,
            TimeUnit::Hours => 3600,
            TimeUnit::Days => 86_400,
        }
    }

    fn word(self) -> &'static str {
        match self {
            TimeUnit::Minutes => "min",
            TimeUnit::Hours => "h",
            TimeUnit::Days => "d",
        }
    }
}

/// How long to keep a leftover after its job ended: a number and a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeepFor {
    /// How many units (at least 1).
    pub amount: u32,
    /// The unit.
    pub unit: TimeUnit,
}

impl KeepFor {
    /// `n` minutes.
    pub const fn minutes(n: u32) -> Self {
        KeepFor {
            amount: n,
            unit: TimeUnit::Minutes,
        }
    }

    /// `n` hours.
    pub const fn hours(n: u32) -> Self {
        KeepFor {
            amount: n,
            unit: TimeUnit::Hours,
        }
    }

    /// The time span.
    pub fn duration(self) -> Duration {
        Duration::from_secs(u64::from(self.amount) * self.unit.seconds())
    }

    /// `10 min`, `3 h`, `2 d`.
    pub fn label(self) -> String {
        format!("{} {}", self.amount, self.unit.word())
    }

    /// Checks the amount: 1 up to [`MAX_KEEP_DAYS`] days.
    pub fn validate(self) -> Result<(), String> {
        if self.amount == 0 {
            return Err("the keep time must be at least 1".into());
        }
        if self.duration() > Duration::from_secs(u64::from(MAX_KEEP_DAYS) * 86_400) {
            return Err(format!("the keep time is at most {MAX_KEEP_DAYS} days"));
        }
        Ok(())
    }
}

/// What a rule does to the processes it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// Leave them alone.
    Never,
    /// Stop them this long after their job ended.
    For(KeepFor),
}

/// The file/JSON form of [`Keep`]: the word `"never"` or `{ amount, unit }`.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum KeepRepr {
    Word(NeverWord),
    Time(KeepFor),
}

#[derive(Serialize, Deserialize)]
enum NeverWord {
    #[serde(rename = "never")]
    Never,
}

impl From<KeepRepr> for Keep {
    fn from(r: KeepRepr) -> Self {
        match r {
            KeepRepr::Word(NeverWord::Never) => Keep::Never,
            KeepRepr::Time(k) => Keep::For(k),
        }
    }
}

impl From<Keep> for KeepRepr {
    fn from(k: Keep) -> Self {
        match k {
            Keep::Never => KeepRepr::Word(NeverWord::Never),
            Keep::For(k) => KeepRepr::Time(k),
        }
    }
}

impl Serialize for Keep {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        KeepRepr::from(*self).serialize(s)
    }
}

impl<'de> Deserialize<'de> for Keep {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        KeepRepr::deserialize(d).map(Keep::from)
    }
}

/// A per-process rule: processes whose name, path or command line match `pattern` get their own keep time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessRule {
    /// A name or path, with `*` (anything) and `?` (one character) allowed; matched without regard to case.
    /// Without wildcards it must equal the process name or its full path; with wildcards it may match the name, the
    /// path or the whole command line.
    pub pattern: String,
    /// What to do with matches.
    pub keep: Keep,
}

/// The cleanup settings in effect for a runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcessPolicy {
    /// Stop leftovers automatically (on by default).
    pub cleanup: bool,
    /// How long to keep a leftover no rule matches.
    pub keep: KeepFor,
    /// How long a process gets to stop on request before it is ended, in seconds.
    pub stop_timeout_secs: u32,
    /// Rules, first match wins.
    pub rules: Vec<ProcessRule>,
}

impl Default for ProcessPolicy {
    fn default() -> Self {
        ProcessPolicy {
            cleanup: true,
            keep: KeepFor::minutes(10),
            stop_timeout_secs: 15,
            rules: default_rules(),
        }
    }
}

/// The rules Pitcrew ships: long-lived helper processes that make the next job faster stay for a while.
pub fn default_rules() -> Vec<ProcessRule> {
    let rule = |pattern: &str, keep: Keep| ProcessRule {
        pattern: pattern.into(),
        keep,
    };
    vec![
        // The Gradle daemon is reused by the next build; idle daemons stop themselves after 3 hours anyway.
        rule("*GradleDaemon*", Keep::For(KeepFor::hours(3))),
        // The Kotlin compile daemon: the same story.
        rule("*KotlinCompileDaemon*", Keep::For(KeepFor::hours(3))),
        // Android emulators and QEMU virtual machines are slow to boot; keep them for a short while.
        rule("qemu-system*", Keep::For(KeepFor::minutes(30))),
        rule("*/emulator/emulator*", Keep::For(KeepFor::minutes(30))),
        // The adb server is tiny and shared by every device tool.
        rule("adb", Keep::Never),
    ]
}

/// A runner's own settings: anything left out follows the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcessOverride {
    /// Turns cleanup on or off for this runner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<bool>,
    /// Its own default keep time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep: Option<KeepFor>,
    /// Its own rules, tried before the defaults'.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<ProcessRule>,
}

/// The policy a runner follows: its overrides over the defaults.
pub fn effective(defaults: &ProcessPolicy, own: Option<&ProcessOverride>) -> ProcessPolicy {
    let Some(o) = own else { return defaults.clone() };
    ProcessPolicy {
        cleanup: o.cleanup.unwrap_or(defaults.cleanup),
        keep: o.keep.unwrap_or(defaults.keep),
        stop_timeout_secs: defaults.stop_timeout_secs,
        rules: o.rules.iter().chain(&defaults.rules).cloned().collect(),
    }
}

/// Checks a list of rules.
pub fn validate_rules(rules: &[ProcessRule]) -> Result<(), String> {
    if rules.len() > MAX_RULES {
        return Err(format!("at most {MAX_RULES} process rules"));
    }
    for r in rules {
        let p = r.pattern.trim();
        if p.is_empty() || p.len() > MAX_PATTERN_LEN || p.chars().any(char::is_control) {
            return Err(format!(
                "a process rule's pattern must be 1-{MAX_PATTERN_LEN} characters without line breaks"
            ));
        }
        if p.chars().all(|c| c == '*' || c == '?') {
            return Err(format!("the pattern `{p}` would match every process; name what it should match"));
        }
        if let Keep::For(k) = r.keep {
            k.validate().map_err(|e| format!("rule `{p}`: {e}"))?;
        }
    }
    Ok(())
}

/// Checks a whole policy.
pub fn validate(p: &ProcessPolicy) -> Result<(), String> {
    p.keep.validate()?;
    if !STOP_TIMEOUT_RANGE.contains(&p.stop_timeout_secs) {
        return Err(format!(
            "the stop timeout is {} to {} seconds",
            STOP_TIMEOUT_RANGE.start(),
            STOP_TIMEOUT_RANGE.end()
        ));
    }
    validate_rules(&p.rules)
}

/// Checks a runner's overrides.
pub fn validate_override(o: &ProcessOverride) -> Result<(), String> {
    if let Some(k) = o.keep {
        k.validate()?;
    }
    validate_rules(&o.rules)
}

/// Whether `text` matches `pattern`: `*` is any run of characters (also across `/` and `\`), `?` one character;
/// without regard to case. Linear time in the text for any pattern (no backtracking blow-up).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// The first rule that matches a process with this name, path and command line.
pub fn rule_for<'a>(rules: &'a [ProcessRule], name: &str, exe: Option<&str>, command: &str) -> Option<&'a ProcessRule> {
    rules.iter().find(|r| {
        let p = r.pattern.trim();
        if p.contains(['*', '?']) {
            glob_match(p, name) || exe.is_some_and(|e| glob_match(p, e)) || glob_match(p, command)
        } else {
            p.eq_ignore_ascii_case(name) || exe.is_some_and(|e| p.eq_ignore_ascii_case(e))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_on_with_ten_minutes_and_commented_rules() {
        let p = ProcessPolicy::default();
        assert!(p.cleanup);
        assert_eq!(p.keep, KeepFor::minutes(10));
        assert_eq!(p.keep.duration(), Duration::from_secs(600));
        assert!(validate(&p).is_ok());
        let by = |pattern: &str| p.rules.iter().find(|r| r.pattern == pattern).map(|r| r.keep);
        assert_eq!(by("*GradleDaemon*"), Some(Keep::For(KeepFor::hours(3))));
        assert_eq!(by("*KotlinCompileDaemon*"), Some(Keep::For(KeepFor::hours(3))));
        assert_eq!(by("qemu-system*"), Some(Keep::For(KeepFor::minutes(30))));
        assert_eq!(by("adb"), Some(Keep::Never));
    }

    #[test]
    fn the_toml_form_is_the_documented_one() {
        let text = r#"
            cleanup = false
            keep = { amount = 2, unit = "hours" }
            [[rules]]
            pattern = "*GradleDaemon*"
            keep = { amount = 1, unit = "days" }
            [[rules]]
            pattern = "adb"
            keep = "never"
        "#;
        let p: ProcessPolicy = toml::from_str(text).unwrap();
        assert!(!p.cleanup);
        assert_eq!(
            p.keep,
            KeepFor {
                amount: 2,
                unit: TimeUnit::Hours
            }
        );
        assert_eq!(
            p.rules[0].keep,
            Keep::For(KeepFor {
                amount: 1,
                unit: TimeUnit::Days
            })
        );
        assert_eq!(p.rules[1].keep, Keep::Never);
        assert_eq!(p.stop_timeout_secs, 15, "left-out keys take the defaults");
        let back: ProcessPolicy = toml::from_str(&toml::to_string_pretty(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        let json = serde_json::to_string(&p.rules[1]).unwrap();
        assert_eq!(json, r#"{"pattern":"adb","keep":"never"}"#);
        let back: ProcessRule = serde_json::from_str(&serde_json::to_string(&p.rules[0]).unwrap()).unwrap();
        assert_eq!(back, p.rules[0]);
        assert!(toml::from_str::<ProcessPolicy>("keep = \"soon\"").is_err());
        assert!(toml::from_str::<ProcessPolicy>("[[rules]]\npattern = \"x\"\nkeep = \"forever\"").is_err());
    }

    #[test]
    fn a_runner_overrides_the_defaults_and_its_rules_come_first() {
        let defaults = ProcessPolicy::default();
        assert_eq!(effective(&defaults, None), defaults);
        let own = ProcessOverride {
            cleanup: Some(false),
            keep: Some(KeepFor::hours(1)),
            rules: vec![ProcessRule {
                pattern: "*GradleDaemon*".into(),
                keep: Keep::Never,
            }],
        };
        let e = effective(&defaults, Some(&own));
        assert!(!e.cleanup);
        assert_eq!(e.keep, KeepFor::hours(1));
        assert_eq!(e.rules.len(), defaults.rules.len() + 1);
        let hit = rule_for(
            &e.rules,
            "java",
            None,
            "java -cp x org.gradle.launcher.daemon.bootstrap.GradleDaemon 8.5",
        )
        .unwrap();
        assert_eq!(hit.keep, Keep::Never, "the runner's rule wins");
        let none = effective(&defaults, Some(&ProcessOverride::default()));
        assert_eq!(none, defaults);
        let o: ProcessOverride = toml::from_str("").unwrap();
        assert_eq!(o, ProcessOverride::default());
        assert_eq!(toml::to_string(&o).unwrap().trim(), "");
    }

    #[test]
    fn globs() {
        for (p, t, want) in [
            (
                "*GradleDaemon*",
                "java -cp a org.gradle.launcher.daemon.bootstrap.GradleDaemon 8",
                true,
            ),
            ("qemu-system*", "QEMU-SYSTEM-aarch64", true),
            ("qemu-system*", "xqemu-system", false),
            ("adb", "adb", true),
            ("a?c", "abc", true),
            ("a?c", "ac", false),
            ("*/emulator/emulator*", "/Users/x/Library/Android/sdk/emulator/emulator64", true),
            ("*/emulator/emulator*", "C:\\sdk\\emulator\\emulator", false),
            ("*", "", true),
            ("", "", true),
            ("", "a", false),
            ("**x", "ax", true),
            ("a*b*c", "a-b-c", true),
            ("a*b*c", "a-c-b", false),
        ] {
            assert_eq!(glob_match(p, t), want, "{p:?} vs {t:?}");
        }
        // No exponential blow-up.
        assert!(!glob_match(&format!("{}b", "a*".repeat(60)), &"a".repeat(200)));
    }

    #[test]
    fn rules_match_name_path_or_command() {
        let rules = default_rules();
        assert_eq!(rule_for(&rules, "adb", None, "adb -L tcp:5037").map(|r| r.keep), Some(Keep::Never));
        assert_eq!(rule_for(&rules, "x", Some("ADB"), "").map(|r| r.keep), Some(Keep::Never));
        assert_eq!(rule_for(&rules, "adbd", None, "").map(|r| r.keep), None, "no wildcard: exact only");
        assert_eq!(
            rule_for(&rules, "emulator64", Some("/sdk/emulator/emulator64-arm"), "").map(|r| r.keep),
            Some(Keep::For(KeepFor::minutes(30)))
        );
        assert_eq!(rule_for(&rules, "node", Some("/usr/bin/node"), "node build.js"), None);
    }

    #[test]
    fn validation_gives_clear_messages() {
        let mut p = ProcessPolicy {
        keep: KeepFor { amount: 0, unit: TimeUnit::Minutes },
        ..ProcessPolicy::default()
        };
        assert!(validate(&p).unwrap_err().contains("at least 1"));
        p.keep = KeepFor {
            amount: 31,
            unit: TimeUnit::Days,
        };
        assert!(validate(&p).unwrap_err().contains("at most 30 days"));
        p.keep = KeepFor {
            amount: 30,
            unit: TimeUnit::Days,
        };
        assert!(validate(&p).is_ok());
        p.stop_timeout_secs = 0;
        assert!(validate(&p).unwrap_err().contains("stop timeout"));
        p.stop_timeout_secs = 15;
        let bad = |pattern: &str, keep| {
            validate_rules(&[ProcessRule {
                pattern: pattern.into(),
                keep,
            }])
        };
        assert!(bad("", Keep::Never).is_err());
        assert!(bad("  ", Keep::Never).is_err());
        assert!(bad("*", Keep::Never).unwrap_err().contains("every process"));
        assert!(bad("a\nb", Keep::Never).is_err());
        assert!(bad(&"x".repeat(300), Keep::Never).is_err());
        assert!(bad("x", Keep::For(KeepFor::minutes(0))).unwrap_err().starts_with("rule `x`"));
        assert!(validate_rules(&vec![
            ProcessRule {
                pattern: "x".into(),
                keep: Keep::Never
            };
            MAX_RULES + 1
        ])
        .is_err());
        assert!(validate_override(&ProcessOverride {
            keep: Some(KeepFor::minutes(0)),
            ..Default::default()
        })
        .is_err());
        assert!(validate_override(&ProcessOverride::default()).is_ok());
        assert_eq!(
            KeepFor {
                amount: 2,
                unit: TimeUnit::Days
            }
            .label(),
            "2 d"
        );
        assert_eq!(KeepFor::minutes(5).label(), "5 min");
        assert_eq!(KeepFor::hours(3).label(), "3 h");
    }
}
