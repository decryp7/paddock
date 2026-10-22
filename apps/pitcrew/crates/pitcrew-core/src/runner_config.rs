//! Editing a runner's `config.yaml` from Pitcrew, as the protocol sees it: read it, preview a change, name its
//! backups, and check afterwards that the runner declared the labels the file asks for (ADR 0011).
//!
//! Two ways to edit, one result:
//!
//! - **Form**: [`RunnerSettings`] in, the file's text out, changing only the keys that differ ([`crate::yaml_patch`]).
//! - **Raw**: the whole text typed in the Advanced editor.
//!
//! Either way [`preview`] returns the new text, a line diff against the file as it is now, every finding
//! ([`ConfigIssue`]) and whether the runner must restart to use it; saving is refused while there are errors.
//! The runner reads its configuration only when it starts, so every real change needs a restart, which the service
//! does after the current job (the controller's "Restarts after the current job"). A change that only touches
//! comments or layout needs none.
//!
//! Pure: text in, text out. Secrets in the file (`cache.external_secret`, S3 keys) are not interpreted here and
//! [`ConfigEdit`]'s `Debug` never prints the raw text.

use crate::checksum::Sha256Digest;
use crate::runner_schema::{check_text, has_errors, ConfigIssue};
use crate::runner_settings::{edits, label_name, validate, RunnerSettings};
use crate::text_diff::{diff, TextDiff};
use crate::time::{format_rfc3339, Timestamp};
use crate::yaml_patch;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Backups kept of one configuration file (the oldest are deleted when a new one is made).
pub const MAX_BACKUPS: usize = 10;

/// A change to the configuration file.
// The form variant is the larger one; edits are short-lived values, so boxing would only add noise.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ConfigEdit {
    /// The form's settings; keys not in the form stay as they are.
    Form {
        /// The settings the form shows.
        settings: RunnerSettings,
    },
    /// The whole file, as typed in the Advanced editor.
    Raw {
        /// The new text.
        text: String,
    },
}

impl std::fmt::Debug for ConfigEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigEdit::Form { .. } => f.write_str("ConfigEdit::Form(..)"),
            // The text may hold a secret (a cache secret, S3 keys); only its size is ever printed.
            ConfigEdit::Raw { text } => write!(f, "ConfigEdit::Raw({} bytes)", text.len()),
        }
    }
}

/// What the form shows for a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    /// The form's settings.
    pub settings: RunnerSettings,
    /// Keys the file has in a shape the form cannot show (they stay as they are).
    pub unreadable: Vec<String>,
    /// Findings about the file as it is now.
    pub issues: Vec<ConfigIssue>,
    /// The runner runs container jobs (show the container settings).
    pub container_runner: bool,
}

/// Reads a file for the form.
pub fn view(text: &str) -> ConfigView {
    match RunnerSettings::from_text(text) {
        Ok((settings, unreadable)) => {
            let mut issues = check_text(text);
            issues.extend(validate(&settings));
            ConfigView {
                container_runner: settings.is_container_runner(),
                settings,
                unreadable,
                issues,
            }
        }
        Err(issue) => ConfigView {
            settings: RunnerSettings::default(),
            unreadable: vec![],
            issues: vec![issue],
            container_runner: true,
        },
    }
}

/// What saving an edit would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// The file's new text (the current text when the edit could not be applied).
    pub text: String,
    /// The text differs from the current file.
    pub changed: bool,
    /// The runner must restart to use it.
    pub restart_needed: bool,
    /// The difference from the current file.
    pub diff: TextDiff,
    /// Findings; saving is refused while any is an error.
    pub issues: Vec<ConfigIssue>,
    /// The keys that differ in meaning, dotted.
    pub changed_keys: Vec<String>,
    /// The label names the new file asks the runner to declare (empty: the file sets none, so the registration's
    /// labels apply and nothing can be checked).
    pub labels: Vec<String>,
}

impl Preview {
    /// Whether saving is allowed.
    pub fn can_save(&self) -> bool {
        !has_errors(&self.issues)
    }
}

/// Applies `edit` to `current` without writing anything.
pub fn preview(current: &str, edit: &ConfigEdit) -> Preview {
    let mut issues = vec![];
    let text = match edit {
        ConfigEdit::Raw { text } => text.clone(),
        ConfigEdit::Form { settings } => match RunnerSettings::from_text(current) {
            Err(issue) => {
                issues.push(issue);
                current.to_string()
            }
            Ok((old, _)) => {
                issues.extend(validate(&settings.normalised()));
                match yaml_patch::apply(current, &edits(&old, settings)) {
                    Ok(t) => t,
                    Err(e) => {
                        issues.push(ConfigIssue::error(e.path.clone(), e.to_string()));
                        current.to_string()
                    }
                }
            }
        },
    };
    let mut found = check_text(&text);
    // The form's own checks run on the settings; in the raw editor they run on what the new text says.
    if matches!(edit, ConfigEdit::Raw { .. }) && !has_errors(&found) {
        if let Ok((s, _)) = RunnerSettings::from_text(&text) {
            found.extend(validate(&s));
        }
    }
    for f in found {
        if !issues.contains(&f) {
            issues.push(f);
        }
    }
    let (new_settings, _) = RunnerSettings::from_text(&text).unwrap_or_default();
    let changed_keys = changed_keys(current, &text);
    let restart_needed = text != current && meaning_differs(current, &text);
    Preview {
        changed: text != current,
        diff: diff(current, &text),
        restart_needed,
        labels: new_settings.labels.iter().map(|l| label_name(l).to_string()).collect(),
        changed_keys,
        issues,
        text,
    }
}

/// Whether the two texts mean something different to the runner (comments and layout do not count). A text that does
/// not parse always counts as different.
fn meaning_differs(a: &str, b: &str) -> bool {
    match (crate::runner_schema::parse(a), crate::runner_schema::parse(b)) {
        (Ok(x), Ok(y)) => normal(&x) != normal(&y),
        _ => true,
    }
}

fn normal(v: &serde_norway::Value) -> serde_norway::Value {
    match v {
        serde_norway::Value::Null => serde_norway::Value::Mapping(Default::default()),
        other => other.clone(),
    }
}

/// The dotted leaf keys whose values differ between two texts (empty when either does not parse).
pub fn changed_keys(a: &str, b: &str) -> Vec<String> {
    let (Ok(x), Ok(y)) = (crate::runner_schema::parse(a), crate::runner_schema::parse(b)) else {
        return vec![];
    };
    let mut out = BTreeSet::new();
    walk(&normal(&x), &normal(&y), "", &mut out);
    out.into_iter().take(64).collect()
}

fn walk(a: &serde_norway::Value, b: &serde_norway::Value, prefix: &str, out: &mut BTreeSet<String>) {
    use serde_norway::Value::Mapping;
    if let (Mapping(m), Mapping(n)) = (a, b) {
        let keys: BTreeSet<String> = m.keys().chain(n.keys()).filter_map(|k| k.as_str().map(str::to_string)).collect();
        for k in keys {
            let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
            match (m.get(k.as_str()), n.get(k.as_str())) {
                (Some(x), Some(y)) => walk(x, y, &path, out),
                _ => {
                    out.insert(path);
                }
            }
        }
    } else if a != b {
        out.insert(prefix.to_string());
    }
}

/// SHA-256 of the file's text, to notice that it changed on disk between reading and saving.
pub fn fingerprint(text: &str) -> String {
    Sha256Digest::of_bytes(text.as_bytes()).to_string()
}

/// A backup's file name: `config.yaml.bak-20261022-153045` (UTC).
pub fn backup_name(file_name: &str, at: Timestamp) -> String {
    let t = format_rfc3339(at);
    let stamp: String = t.chars().filter(char::is_ascii_digit).take(14).collect();
    format!("{file_name}.bak-{}-{}", &stamp[..8.min(stamp.len())], stamp.get(8..).unwrap_or(""))
}

/// Whether `candidate` is a backup Pitcrew made of `file_name`.
pub fn is_backup(file_name: &str, candidate: &str) -> bool {
    candidate
        .strip_prefix(file_name)
        .and_then(|r| r.strip_prefix(".bak-"))
        .is_some_and(|r| r.len() == 15 && r.as_bytes()[8] == b'-' && r.bytes().enumerate().all(|(i, b)| i == 8 || b.is_ascii_digit()))
}

/// The backups to delete so that at most `keep` remain (the oldest go first). Files that are not Pitcrew backups
/// (including a plain `config.yaml.bak`) are never listed.
pub fn backups_to_prune(file_name: &str, existing: &[String], keep: usize) -> Vec<String> {
    let mut mine: Vec<&String> = existing.iter().filter(|n| is_backup(file_name, n)).collect();
    mine.sort();
    let excess = mine.len().saturating_sub(keep);
    mine.into_iter().take(excess).cloned().collect()
}

/// A mismatch between the labels the file asks for and the ones the runner declared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelMismatch {
    /// In the file, not declared.
    pub missing: Vec<String>,
    /// Declared, not in the file.
    pub unexpected: Vec<String>,
}

/// Compares label names (a label's type after the colon is not declared to the server); order and repeats do not
/// matter. `None` when they agree, or when nothing is expected.
pub fn labels_mismatch(expected: &[String], declared: &[String]) -> Option<LabelMismatch> {
    if expected.is_empty() {
        return None;
    }
    let want: BTreeSet<&str> = expected.iter().map(|l| label_name(l)).collect();
    let got: BTreeSet<&str> = declared.iter().map(|l| label_name(l)).collect();
    if want == got {
        return None;
    }
    Some(LabelMismatch {
        missing: want.difference(&got).map(|s| (*s).to_string()).collect(),
        unexpected: got.difference(&want).map(|s| (*s).to_string()).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_schema::Severity;

    const FILE: &str = "# my runner\nrunner:\n  capacity: 1 # one at a time\n  labels:\n    - \"macos:host\"\ncache:\n  enabled: true\n";

    fn form(f: impl FnOnce(&mut RunnerSettings)) -> ConfigEdit {
        let (mut s, _) = RunnerSettings::from_text(FILE).unwrap();
        f(&mut s);
        ConfigEdit::Form { settings: s }
    }

    #[test]
    fn viewing_a_file_gives_settings_and_findings() {
        let v = view(FILE);
        assert_eq!(v.settings.capacity, Some(1));
        assert!(v.issues.is_empty() && v.unreadable.is_empty());
        assert!(!v.container_runner, "only host labels");
        let bad = view("runner:\n  capacity: 0\n  captacity: 1\n");
        assert_eq!(bad.issues.len(), 2);
        let broken = view("runner: [");
        assert_eq!(broken.settings, RunnerSettings::default());
        assert_eq!(broken.issues[0].severity, Severity::Error);
        assert!(broken.container_runner);
    }

    #[test]
    fn a_form_edit_changes_only_its_keys_and_asks_for_a_restart() {
        let p = preview(FILE, &form(|s| s.capacity = Some(3)));
        assert!(p.can_save() && p.changed && p.restart_needed);
        assert_eq!(p.text, FILE.replace("capacity: 1 #", "capacity: 3 #"));
        assert_eq!(p.changed_keys, vec!["runner.capacity"]);
        assert_eq!((p.diff.added, p.diff.removed), (1, 1));
        assert_eq!(p.labels, vec!["macos"]);
    }

    #[test]
    fn nothing_changed_means_no_restart() {
        let p = preview(FILE, &form(|_| {}));
        assert!(!p.changed && !p.restart_needed && p.can_save());
        assert!(!p.diff.is_changed());
        assert!(p.changed_keys.is_empty());
    }

    #[test]
    fn a_comment_only_raw_edit_needs_no_restart() {
        let text = FILE.replace("# my runner", "# my build runner, edited");
        let p = preview(FILE, &ConfigEdit::Raw { text: text.clone() });
        assert!(p.changed && !p.restart_needed && p.can_save());
        assert!(p.changed_keys.is_empty());
        // A different layout of the same data too.
        let p = preview(
            "a: 1\nb: 2\n",
            &ConfigEdit::Raw {
                text: "b: 2\na: 1\n".into(),
            },
        );
        assert!(p.changed && !p.restart_needed);
        // An empty file and a file of only comments mean the same.
        let p = preview(
            "",
            &ConfigEdit::Raw {
                text: "# nothing\n".into(),
            },
        );
        assert!(p.changed && !p.restart_needed);
    }

    #[test]
    fn raw_edits_are_checked_for_syntax_types_and_values() {
        let syntax = preview(FILE, &ConfigEdit::Raw { text: "runner: [".into() });
        assert!(!syntax.can_save() && syntax.restart_needed);
        assert!(syntax.issues[0].message.contains("not valid YAML"));
        let ty = preview(
            FILE,
            &ConfigEdit::Raw {
                text: "runner:\n  capacity: many\n".into(),
            },
        );
        assert!(!ty.can_save());
        let value = preview(
            FILE,
            &ConfigEdit::Raw {
                text: "runner:\n  capacity: 99\n".into(),
            },
        );
        assert!(!value.can_save() && value.issues[0].path == "runner.capacity");
        let typo = preview(
            FILE,
            &ConfigEdit::Raw {
                text: "runner:\n  captacity: 2\n".into(),
            },
        );
        assert!(typo.can_save(), "unknown keys only warn");
        assert_eq!(typo.issues[0].severity, Severity::Warning);
        assert_eq!(
            typo.changed_keys,
            vec!["cache", "runner.capacity", "runner.captacity", "runner.labels"]
        );
    }

    #[test]
    fn a_form_edit_with_a_bad_value_is_refused_and_a_broken_file_is_reported() {
        let p = preview(FILE, &form(|s| s.timeout = Some("soon".into())));
        assert!(!p.can_save());
        assert!(p.issues.iter().any(|i| i.path == "runner.timeout"));
        let broken = preview("runner: [", &form(|s| s.capacity = Some(2)));
        assert!(!broken.can_save());
        assert_eq!(broken.text, "runner: [");
        // A file the patcher refuses to touch says so and stays as it is.
        let odd = "runner:\n  capacity: |\n    text\n";
        let p = preview(
            odd,
            &ConfigEdit::Form {
                settings: RunnerSettings {
                    capacity: Some(2),
                    ..Default::default()
                },
            },
        );
        assert!(!p.can_save() && p.text == odd);
        assert!(p.issues.iter().any(|i| i.message.contains("Advanced editor")));
    }

    #[test]
    fn debug_never_prints_the_text() {
        let e = ConfigEdit::Raw {
            text: "cache:\n  external_secret: hunter2hunter2\n".into(),
        };
        let d = format!("{e:?}");
        assert!(!d.contains("hunter2") && d.contains("bytes"));
        assert_eq!(
            format!(
                "{:?}",
                ConfigEdit::Form {
                    settings: RunnerSettings::default()
                }
            ),
            "ConfigEdit::Form(..)"
        );
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.starts_with(r#"{"mode":"raw","text":"#));
        let back: ConfigEdit = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
        let f: ConfigEdit = serde_json::from_str(r#"{"mode":"form","settings":{"capacity":2}}"#).unwrap();
        assert!(matches!(f, ConfigEdit::Form { settings } if settings.capacity == Some(2)));
    }

    #[test]
    fn backups_are_named_by_time_and_pruned_oldest_first() {
        let at = Timestamp(1_792_683_045_000); // 2026-10-22 15:30:45 UTC
        let name = backup_name("config.yaml", at);
        assert_eq!(name, "config.yaml.bak-20261022-153045");
        assert!(is_backup("config.yaml", &name));
        assert!(!is_backup("config.yaml", "config.yaml.bak"), "the runner's own .bak is not ours");
        assert!(!is_backup("config.yaml", "config.yaml.bak-2026"));
        assert!(!is_backup("config.yaml", "other.yaml.bak-20261022-153045"));
        assert!(!is_backup("config.yaml", "config.yaml.bak-2026x022-153045"));
        let names: Vec<String> = (1..=14)
            .map(|d| format!("config.yaml.bak-202610{d:02}-000000"))
            .chain(["config.yaml".to_string(), "config.yaml.bak".to_string(), "notes.txt".to_string()])
            .collect();
        let prune = backups_to_prune("config.yaml", &names, MAX_BACKUPS);
        assert_eq!(prune.len(), 4);
        assert_eq!(prune[0], "config.yaml.bak-20261001-000000");
        assert_eq!(prune[3], "config.yaml.bak-20261004-000000");
        assert!(backups_to_prune("config.yaml", &names, 20).is_empty());
        assert_eq!(backups_to_prune("config.yaml", &names, 0).len(), 14);
    }

    #[test]
    fn labels_are_compared_by_name() {
        let s = |v: &[&str]| v.iter().map(|x| (*x).to_string()).collect::<Vec<_>>();
        assert_eq!(labels_mismatch(&s(&["a", "b"]), &s(&["b", "a"])), None);
        assert_eq!(labels_mismatch(&s(&["a:host", "b:docker://x"]), &s(&["a", "b"])), None);
        assert_eq!(labels_mismatch(&s(&[]), &s(&["a"])), None, "nothing to check");
        let m = labels_mismatch(&s(&["a", "c"]), &s(&["a", "b"])).unwrap();
        assert_eq!((m.missing, m.unexpected), (s(&["c"]), s(&["b"])));
        let m = labels_mismatch(&s(&["a"]), &[]).unwrap();
        assert_eq!(m.missing, s(&["a"]));
    }

    #[test]
    fn the_fingerprint_changes_with_the_text() {
        assert_eq!(fingerprint("a"), fingerprint("a"));
        assert_ne!(fingerprint("a"), fingerprint("b"));
        assert_eq!(fingerprint("a").len(), 64);
    }
}
