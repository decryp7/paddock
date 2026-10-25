//! Pitcrew's configuration file (`pitcrew.toml`): versioned, migrated forward, and tolerant of damage.
//!
//! Loading never fails. A damaged file falls back to the last good copy (`pitcrew.toml.bak`, written by the service
//! before every save) and then to the defaults; one bad runner entry or section is skipped with a problem report
//! while the rest loads. A file written by a newer Pitcrew is read as far as it is understood and never overwritten.
//! The service keeps the damaged file aside for the user (see `docs/adr/0001-architecture.md`).

use crate::disk_guard::DiskGuardPolicy;
use crate::distribution::RunnerKind;
use crate::housekeeping::CleanupPolicy;
use crate::logs::RotationPolicy;
use crate::names::RunnerName;
use crate::pause::PausePolicy;
use crate::process_policy::{self, ProcessOverride, ProcessPolicy};
use crate::restart::RestartPolicy;
use crate::session::LoggedOffMode;
use crate::update::{UpdatePolicy, UpdateTimeouts};
use crate::watchdog::StallRule;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

/// The schema version this Pitcrew writes.
pub const SCHEMA_VERSION: u32 = 1;
/// Most runners one machine may have.
pub const MAX_RUNNERS: usize = 16;
/// Most labels per runner.
pub const MAX_LABELS: usize = 32;
/// Longest label.
pub const MAX_LABEL_LEN: usize = 128;
/// Largest configuration file Pitcrew reads.
pub const MAX_CONFIG_BYTES: usize = 1024 * 1024;

/// The whole configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// The schema version ([`SCHEMA_VERSION`] once loaded).
    pub schema_version: u32,
    /// Settings every runner inherits.
    #[serde(default)]
    pub defaults: Defaults,
    /// The managed runners.
    #[serde(default)]
    pub runners: Vec<RunnerConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            schema_version: SCHEMA_VERSION,
            defaults: Defaults::default(),
            runners: vec![],
        }
    }
}

/// Settings every runner inherits unless it overrides them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Defaults {
    /// The update policy.
    pub update: UpdatePolicy,
    /// How long update phases may take.
    pub update_timeouts: UpdateTimeouts,
    /// The stall rule.
    pub watchdog: StallRule,
    /// Restart backoff and crash-loop protection.
    pub restart: RestartPolicy,
    /// Log rotation.
    pub logs: RotationPolicy,
    /// Disk housekeeping.
    pub cleanup: CleanupPolicy,
    /// What happens to the processes a finished job leaves behind (ADR 0012).
    pub processes: ProcessPolicy,
    /// Drain runners before the disk is full, resume when space recovers (ADR 0013).
    pub disk_guard: DiskGuardPolicy,
    /// Pause windows and only-when-idle (ADR 0013).
    pub pause: PausePolicy,
}

/// Where a runner runs on Windows (macOS always runs runners in the user's session, see ADR 0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionMode {
    /// In the session of the user who is logged on at the console, with that user's rights: jobs that need a
    /// desktop (WebView2, UI tests) work. The runner runs only while the user is logged on.
    #[default]
    Interactive,
    /// In the service's session (session 0), without a desktop; runs before anyone logs on.
    Service,
}

/// One managed runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerConfig {
    /// Pitcrew's name for it.
    pub name: RunnerName,
    /// Which runner it is.
    pub kind: RunnerKind,
    /// Its folder: binary, registration (`.runner`), runner configuration, work folders and logs live here.
    pub dir: PathBuf,
    /// The runner's own configuration file, relative to `dir` (default `config.yaml`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_file: Option<PathBuf>,
    /// Labels used when registering.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Where it runs on Windows.
    #[serde(default)]
    pub session: SessionMode,
    /// What an `interactive` runner does while nobody is logged on.
    #[serde(default)]
    pub logged_off: LoggedOffMode,
    /// The server it is registered with (shown in the window and used for links; the registration itself is the
    /// runner's `.runner` file, which Pitcrew never reads).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_url: Option<String>,
    /// Whether Pitcrew keeps it running.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// A binary the user provides instead of the one Pitcrew downloads; never updated automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_path: Option<PathBuf>,
    /// Overrides the default update policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<UpdatePolicy>,
    /// Overrides the default stall rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watchdog: Option<StallRule>,
    /// Overrides the default disk housekeeping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<CleanupPolicy>,
    /// Overrides the default handling of leftover processes (its rules are tried first).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processes: Option<ProcessOverride>,
    /// Environment variables for the runner process (and so its jobs), over the login shell's `PATH` the service
    /// adds on macOS and Linux. Validated by [`crate::runner_env::validate_env`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// Longest server URL Pitcrew keeps.
pub const MAX_URL_LEN: usize = 512;

impl RunnerConfig {
    /// The update policy in effect (the runner's own, else the default).
    pub fn update_policy<'a>(&'a self, d: &'a Defaults) -> &'a UpdatePolicy {
        self.update.as_ref().unwrap_or(&d.update)
    }

    /// The stall rule in effect.
    pub fn stall_rule(&self, d: &Defaults) -> StallRule {
        self.watchdog.unwrap_or(d.watchdog)
    }

    /// The cleanup policy in effect.
    pub fn cleanup_policy(&self, d: &Defaults) -> CleanupPolicy {
        self.cleanup.unwrap_or(d.cleanup)
    }

    /// The leftover-process policy in effect (the runner's overrides over the defaults).
    pub fn process_policy(&self, d: &Defaults) -> ProcessPolicy {
        process_policy::effective(&d.processes, self.processes.as_ref())
    }
}

/// Checks a server URL typed by the user: HTTPS (or HTTP for internal servers), a host, no credentials, bounded.
pub fn check_server_url(url: &str) -> Result<String, String> {
    let u = url.trim().trim_end_matches('/');
    if u.len() > MAX_URL_LEN {
        return Err(format!("the server URL is longer than {MAX_URL_LEN} characters"));
    }
    let rest = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://"))
        .ok_or_else(|| "the server URL must start with https:// (or http:// for an internal server)".to_string())?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() {
        return Err("the server URL has no host name".into());
    }
    if host.contains('@') {
        return Err("the server URL must not contain a user name or password".into());
    }
    if u.chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '<' || c == '>')
    {
        return Err("the server URL contains spaces or characters that are not allowed".into());
    }
    Ok(u.to_string())
}

fn yes() -> bool {
    true
}

/// Something the user should know about the loaded configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConfigProblem {
    /// The file could not be read as a configuration; it was set aside.
    #[error("the configuration file is damaged ({reason}); it was set aside and {fallback}")]
    Damaged {
        /// What is wrong.
        reason: String,
        /// What was loaded instead.
        fallback: String,
    },
    /// The backup was damaged too.
    #[error("the backup configuration is damaged too ({reason}); the defaults are used")]
    BackupDamaged {
        /// What is wrong.
        reason: String,
    },
    /// A runner entry was skipped.
    #[error("runner entry {index} was skipped: {reason}")]
    RunnerSkipped {
        /// Its position in the file (1-based).
        index: usize,
        /// Why.
        reason: String,
    },
    /// A defaults section was replaced by the built-in defaults.
    #[error("the [defaults.{section}] section was ignored: {reason}")]
    SectionIgnored {
        /// The section.
        section: String,
        /// Why.
        reason: String,
    },
    /// The file was written by an older Pitcrew and was migrated.
    #[error("the configuration was migrated from schema version {from} to {to}")]
    Migrated {
        /// The old version.
        from: u32,
        /// The new version.
        to: u32,
    },
    /// The file was written by a newer Pitcrew; it is used as far as it is understood and not overwritten.
    #[error("the configuration was written by a newer Pitcrew (schema version {found}); changes cannot be saved until Pitcrew is updated")]
    Newer {
        /// The file's version.
        found: u32,
    },
}

/// Where the loaded configuration came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoadSource {
    /// The configuration file.
    File,
    /// The backup of the last good file.
    Backup,
    /// The built-in defaults (no file yet, or nothing readable).
    Defaults,
}

/// The result of [`load`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    /// The configuration to use.
    pub config: Config,
    /// What the user should know.
    pub problems: Vec<ConfigProblem>,
    /// Where it came from.
    pub source: LoadSource,
    /// The primary file must be set aside (it is damaged).
    pub set_aside_primary: bool,
    /// The file should be written back (it was missing, migrated, repaired or restored).
    pub needs_save: bool,
    /// Never write the file (it is from a newer Pitcrew).
    pub read_only: bool,
}

/// Loads the configuration from the primary file's text (`None`: no file) and the backup's.
pub fn load(primary: Option<&str>, backup: Option<&str>) -> Loaded {
    let Some(text) = primary else {
        return Loaded {
            config: Config::default(),
            problems: vec![],
            source: LoadSource::Defaults,
            set_aside_primary: false,
            needs_save: true,
            read_only: false,
        };
    };
    match parse(text) {
        Ok(mut loaded) => {
            loaded.needs_save |= !loaded.problems.is_empty() && !loaded.read_only;
            loaded
        }
        Err(reason) => {
            let from_backup = backup.map(parse);
            let (config, mut problems, source, read_only, fallback) = match from_backup {
                Some(Ok(b)) => (b.config, b.problems, LoadSource::Backup, b.read_only, "the last good copy is used"),
                Some(Err(why)) => (
                    Config::default(),
                    vec![ConfigProblem::BackupDamaged { reason: why }],
                    LoadSource::Defaults,
                    false,
                    "the defaults are used",
                ),
                None => (Config::default(), vec![], LoadSource::Defaults, false, "the defaults are used"),
            };
            problems.insert(
                0,
                ConfigProblem::Damaged {
                    reason,
                    fallback: fallback.into(),
                },
            );
            Loaded {
                config,
                problems,
                source,
                set_aside_primary: true,
                needs_save: !read_only,
                read_only,
            }
        }
    }
}

/// Serialises a configuration for saving.
pub fn to_toml(config: &Config) -> Result<String, String> {
    toml::to_string_pretty(config).map_err(|e| e.to_string())
}

/// Parses one file. `Err` only when the file as a whole is unusable.
fn parse(text: &str) -> Result<Loaded, String> {
    if text.len() > MAX_CONFIG_BYTES {
        return Err(format!("it is larger than {} KB", MAX_CONFIG_BYTES / 1024));
    }
    let mut table: toml::Table = toml::from_str(text).map_err(|e| first_line(&e.to_string()))?;
    let found = match table.get("schema_version") {
        None => 0,
        Some(toml::Value::Integer(n)) => u32::try_from(*n).map_err(|_| format!("schema_version {n} is not a version"))?,
        Some(_) => return Err("schema_version is not a number".into()),
    };
    let mut problems = vec![];
    let read_only = found > SCHEMA_VERSION;
    if read_only {
        problems.push(ConfigProblem::Newer { found });
    } else if found < SCHEMA_VERSION {
        table = migrate(table, found);
        problems.push(ConfigProblem::Migrated {
            from: found,
            to: SCHEMA_VERSION,
        });
    }

    let defaults = parse_defaults(table.remove("defaults"), &mut problems);
    let runners = parse_runners(table.remove("runners"), &mut problems);
    let config = Config {
        schema_version: SCHEMA_VERSION,
        defaults,
        runners,
    };
    let migrated = found < SCHEMA_VERSION;
    Ok(Loaded {
        config,
        problems,
        source: LoadSource::File,
        set_aside_primary: false,
        needs_save: migrated,
        read_only,
    })
}

/// Brings an older file up to [`SCHEMA_VERSION`], one version at a time.
fn migrate(mut table: toml::Table, from: u32) -> toml::Table {
    let mut version = from;
    while version < SCHEMA_VERSION {
        if version == 0 {
            table = migrate_0_to_1(table);
        }
        version += 1;
    }
    table.insert("schema_version".into(), toml::Value::Integer(i64::from(SCHEMA_VERSION)));
    table
}

/// Version 0 (preview builds, unversioned) had one runner as a `[runner]` table with `dir` and `labels`, Gitea's
/// runner only, in the interactive session (the layout of the PowerShell set-up Pitcrew replaces).
fn migrate_0_to_1(mut table: toml::Table) -> toml::Table {
    if let Some(toml::Value::Table(mut old)) = table.remove("runner") {
        old.entry("name").or_insert_with(|| toml::Value::String("default".into()));
        old.entry("kind").or_insert_with(|| toml::Value::String("gitea-runner".into()));
        let mut runners = match table.remove("runners") {
            Some(toml::Value::Array(a)) => a,
            _ => vec![],
        };
        runners.insert(0, toml::Value::Table(old));
        table.insert("runners".into(), toml::Value::Array(runners));
    }
    table
}

fn parse_defaults(value: Option<toml::Value>, problems: &mut Vec<ConfigProblem>) -> Defaults {
    let mut d = Defaults::default();
    let Some(value) = value else { return d };
    let toml::Value::Table(mut t) = value else {
        problems.push(ConfigProblem::SectionIgnored {
            section: "*".into(),
            reason: "[defaults] is not a table".into(),
        });
        return d;
    };
    fn section<T: serde::de::DeserializeOwned>(t: &mut toml::Table, name: &str, into: &mut T, problems: &mut Vec<ConfigProblem>) {
        if let Some(v) = t.remove(name) {
            match v.try_into::<T>() {
                Ok(parsed) => *into = parsed,
                Err(e) => problems.push(ConfigProblem::SectionIgnored {
                    section: name.into(),
                    reason: first_line(&e.to_string()),
                }),
            }
        }
    }
    section(&mut t, "update", &mut d.update, problems);
    section(&mut t, "update_timeouts", &mut d.update_timeouts, problems);
    section(&mut t, "watchdog", &mut d.watchdog, problems);
    section(&mut t, "restart", &mut d.restart, problems);
    section(&mut t, "logs", &mut d.logs, problems);
    section(&mut t, "cleanup", &mut d.cleanup, problems);
    section(&mut t, "processes", &mut d.processes, problems);
    if let Err(reason) = process_policy::validate(&d.processes) {
        problems.push(ConfigProblem::SectionIgnored {
            section: "processes".into(),
            reason,
        });
        d.processes = ProcessPolicy::default();
    }
    section(&mut t, "disk_guard", &mut d.disk_guard, problems);
    if let Err(reason) = d.disk_guard.validate() {
        problems.push(ConfigProblem::SectionIgnored {
            section: "disk_guard".into(),
            reason,
        });
        d.disk_guard = DiskGuardPolicy::default();
    }
    section(&mut t, "pause", &mut d.pause, problems);
    if let Err(reason) = d.pause.validate() {
        problems.push(ConfigProblem::SectionIgnored {
            section: "pause".into(),
            reason,
        });
        d.pause = PausePolicy::default();
    }
    d
}

fn parse_runners(value: Option<toml::Value>, problems: &mut Vec<ConfigProblem>) -> Vec<RunnerConfig> {
    let entries = match value {
        None => return vec![],
        Some(toml::Value::Array(a)) => a,
        Some(_) => {
            problems.push(ConfigProblem::RunnerSkipped {
                index: 0,
                reason: "runners is not a list of [[runners]] tables".into(),
            });
            return vec![];
        }
    };
    let mut names = HashSet::new();
    let mut out = vec![];
    for (i, entry) in entries.into_iter().enumerate() {
        let index = i + 1;
        let runner = match entry.try_into::<RunnerConfig>() {
            Ok(r) => r,
            Err(e) => {
                problems.push(ConfigProblem::RunnerSkipped {
                    index,
                    reason: first_line(&e.to_string()),
                });
                continue;
            }
        };
        if let Err(reason) = check_runner(&runner) {
            problems.push(ConfigProblem::RunnerSkipped { index, reason });
            continue;
        }
        if !names.insert(runner.name.clone()) {
            problems.push(ConfigProblem::RunnerSkipped {
                index,
                reason: format!("another runner is already named {}", runner.name),
            });
            continue;
        }
        if out.len() == MAX_RUNNERS {
            problems.push(ConfigProblem::RunnerSkipped {
                index,
                reason: format!("Pitcrew manages at most {MAX_RUNNERS} runners"),
            });
            continue;
        }
        out.push(runner);
    }
    out
}

fn check_runner(r: &RunnerConfig) -> Result<(), String> {
    if !is_absolute_anywhere(&r.dir.to_string_lossy()) {
        return Err(format!("dir {:?} must be an absolute path", r.dir));
    }
    if let Some(cf) = &r.config_file {
        let s = cf.to_string_lossy();
        if is_absolute_anywhere(&s) || s.split(['/', '\\']).any(|part| part == "..") {
            return Err(format!("config_file {cf:?} must be a path inside the runner's dir"));
        }
    }
    if let Some(u) = &r.server_url {
        check_server_url(u)?;
    }
    crate::runner_env::validate_env(&r.env).map_err(|e| e.to_string())?;
    if let Some(o) = &r.processes {
        process_policy::validate_override(o)?;
    }
    if r.labels.len() > MAX_LABELS {
        return Err(format!("it has more than {MAX_LABELS} labels"));
    }
    for l in &r.labels {
        if l.is_empty() || l.len() > MAX_LABEL_LEN || l.contains(',') || l.chars().any(char::is_whitespace) {
            return Err(format!("label {l:?} must be 1-{MAX_LABEL_LEN} characters without spaces or commas"));
        }
    }
    Ok(())
}

/// Absolute on Windows (`C:\…`, `\\server\…`) or Unix (`/…`), whichever host reads the file.
fn is_absolute_anywhere(p: &str) -> bool {
    let b = p.as_bytes();
    p.starts_with('/')
        || p.starts_with("\\\\")
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

fn first_line(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::UpdateMode;

    const GOOD: &str = r#"
schema_version = 1

[defaults.update]
mode = "notify-only"
check_every_hours = 12

[defaults.watchdog]
threshold = 4
window_secs = 600

[[runners]]
name = "build-pc"
kind = "gitea-runner"
dir = 'C:\ProgramData\Pitcrew\runners\build-pc'
labels = ["windows-latest", "windows:host"]

[[runners]]
name = "mac"
kind = "gitea-runner"
dir = "/Users/ci/pitcrew/mac"
session = "service"
enabled = false
[runners.update]
mode = "manual"
pinned = "5.1.0"
"#;

    #[test]
    fn a_runner_env_table_loads_round_trips_and_bad_ones_skip_the_runner() {
        let text = "schema_version = 1\n[[runners]]\nname = \"a\"\nkind = \"gitea-runner\"\ndir = \"/r/a\"\n[runners.env]\nPATH = \"/x/bin\"\nNODE_ENV = \"ci\"\n";
        let l = load(Some(text), None);
        assert_eq!(l.problems, vec![]);
        assert_eq!(l.config.runners[0].env.get("PATH").map(String::as_str), Some("/x/bin"));
        let again = load(Some(&to_toml(&l.config).unwrap()), None);
        assert_eq!(again.config.runners[0].env, l.config.runners[0].env);
        let bad = text.replace("NODE_ENV", "1BAD");
        let l = load(Some(&bad), None);
        assert!(l.config.runners.is_empty());
        assert!(l.problems.iter().any(|p| p.to_string().contains("1BAD")), "{:?}", l.problems);
    }

    #[test]
    fn a_missing_file_gives_defaults_to_save() {
        let l = load(None, None);
        assert_eq!(l.config, Config::default());
        assert_eq!(l.source, LoadSource::Defaults);
        assert!(l.needs_save && l.problems.is_empty() && !l.set_aside_primary);
    }

    #[test]
    fn a_good_file_loads_without_problems() {
        let l = load(Some(GOOD), None);
        assert_eq!(l.problems, vec![]);
        assert_eq!(l.source, LoadSource::File);
        assert!(!l.needs_save && !l.read_only);
        assert_eq!(l.config.defaults.update.mode, UpdateMode::NotifyOnly);
        assert_eq!(l.config.defaults.update.check_every_hours, 12);
        assert_eq!(
            l.config.defaults.watchdog,
            StallRule {
                threshold: 4,
                window_secs: 600
            }
        );
        assert_eq!(l.config.defaults.restart, RestartPolicy::default());
        let [a, b] = l.config.runners.as_slice() else {
            panic!("two runners")
        };
        assert_eq!(
            (a.name.as_str(), a.session, a.enabled),
            ("build-pc", SessionMode::Interactive, true)
        );
        assert_eq!(a.labels.len(), 2);
        assert_eq!((b.session, b.enabled), (SessionMode::Service, false));
        assert_eq!(b.update.as_ref().map(|u| u.mode), Some(UpdateMode::Manual));
    }

    #[test]
    fn saving_and_loading_round_trips() {
        let l = load(Some(GOOD), None);
        let text = to_toml(&l.config).unwrap();
        let again = load(Some(&text), None);
        assert_eq!(again.config, l.config);
        assert!(again.problems.is_empty());
    }

    #[test]
    fn version_0_keeps_runners_listed_next_to_the_old_table() {
        let v0 = r#"
[runner]
dir = "/srv/a"
[[runners]]
name = "b"
kind = "gitea-runner"
dir = "/srv/b"
"#;
        let l = load(Some(v0), None);
        assert_eq!(
            l.config.runners.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["default", "b"]
        );
    }

    #[test]
    fn config_files_outside_the_runner_folder_are_refused() {
        let text = r#"
schema_version = 1
[[runners]]
name = "a"
kind = "gitea-runner"
dir = "/srv/a"
config_file = "/etc/x.yaml"
"#;
        assert!(load(Some(text), None).config.runners.is_empty());
        let ok = text.replace("/etc/x.yaml", "conf/runner.yaml");
        assert_eq!(load(Some(&ok), None).config.runners.len(), 1);
    }

    #[test]
    fn version_0_is_migrated() {
        let v0 = "[runner]\ndir = 'D:\\ci\\runner'\nlabels = [\"windows-latest\"]\n";
        let l = load(Some(v0), None);
        assert_eq!(l.problems, vec![ConfigProblem::Migrated { from: 0, to: 1 }]);
        assert!(l.needs_save);
        let r = &l.config.runners[0];
        assert_eq!(
            (r.name.as_str(), r.kind, r.session),
            ("default", RunnerKind::GiteaRunner, SessionMode::Interactive)
        );
        assert_eq!(r.dir, PathBuf::from("D:\\ci\\runner"));
        assert_eq!(l.config.schema_version, SCHEMA_VERSION);
        // An empty version-0 file is just the defaults.
        assert!(load(Some(""), None).config.runners.is_empty());
    }

    #[test]
    fn a_damaged_file_falls_back_to_the_backup_then_the_defaults() {
        let l = load(Some("schema_version = 1\n[[runners]\n"), Some(GOOD));
        assert_eq!(l.source, LoadSource::Backup);
        assert!(l.set_aside_primary && l.needs_save);
        assert!(matches!(&l.problems[0], ConfigProblem::Damaged { fallback, .. } if fallback.contains("last good")));
        assert_eq!(l.config.runners.len(), 2);

        let l = load(Some("\u{0}garbage"), Some("also = = bad"));
        assert_eq!(l.source, LoadSource::Defaults);
        assert!(matches!(
            l.problems.as_slice(),
            [ConfigProblem::Damaged { .. }, ConfigProblem::BackupDamaged { .. }]
        ));

        let l = load(Some("schema_version = \"one\""), None);
        assert_eq!(l.source, LoadSource::Defaults);
        assert!(l.problems[0].to_string().contains("schema_version is not a number"));
        assert!(load(Some("schema_version = -1"), None).set_aside_primary);
        assert!(load(Some(&"#".repeat(MAX_CONFIG_BYTES + 1)), None).set_aside_primary);
    }

    #[test]
    fn bad_entries_are_skipped_and_the_rest_loads() {
        let text = r#"
schema_version = 1
[defaults]
watchdog = "often"
[defaults.update]
mode = "sometimes"
[[runners]]
name = "Bad Name"
kind = "gitea-runner"
dir = "/x"
[[runners]]
name = "ok"
kind = "gitea-runner"
dir = "/srv/ok"
[[runners]]
name = "ok"
kind = "gitea-runner"
dir = "/srv/dup"
[[runners]]
name = "relative"
kind = "gitea-runner"
dir = "runners/x"
[[runners]]
name = "escape"
kind = "gitea-runner"
dir = "/srv/e"
config_file = "../../etc/passwd"
[[runners]]
name = "labels"
kind = "gitea-runner"
dir = "/srv/l"
labels = ["a b"]
[[runners]]
name = "unknown-kind"
kind = "jenkins"
dir = "/srv/j"
"#;
        let l = load(Some(text), None);
        assert_eq!(l.config.runners.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["ok"]);
        let skipped: Vec<usize> = l
            .problems
            .iter()
            .filter_map(|p| {
                if let ConfigProblem::RunnerSkipped { index, .. } = p {
                    Some(*index)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(skipped, vec![1, 3, 4, 5, 6, 7]);
        let ignored: Vec<&str> = l
            .problems
            .iter()
            .filter_map(|p| {
                if let ConfigProblem::SectionIgnored { section, .. } = p {
                    Some(section.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(ignored, vec!["update", "watchdog"]);
        assert_eq!(l.config.defaults, Defaults::default());
        assert!(l.needs_save, "repaired files are written back");
        assert!(!l.set_aside_primary);
    }

    #[test]
    fn limits_are_enforced() {
        let mut text = String::from("schema_version = 1\n");
        for i in 0..(MAX_RUNNERS + 2) {
            text.push_str(&format!(
                "[[runners]]\nname = \"r{i}\"\nkind = \"gitea-runner\"\ndir = \"/srv/r{i}\"\n"
            ));
        }
        let l = load(Some(&text), None);
        assert_eq!(l.config.runners.len(), MAX_RUNNERS);
        assert_eq!(l.problems.len(), 2);
        let many = (0..=MAX_LABELS).map(|i| format!("\"l{i}\"")).collect::<Vec<_>>().join(",");
        let l = load(
            Some(&format!(
                "schema_version = 1\n[[runners]]\nname = \"x\"\nkind = \"gitea-runner\"\ndir = \"/x\"\nlabels = [{many}]\n"
            )),
            None,
        );
        assert!(l.config.runners.is_empty());
        let l = load(Some("schema_version = 1\nrunners = 5\ndefaults = 3\n"), None);
        assert_eq!(l.problems.len(), 2);
    }

    #[test]
    fn a_newer_file_is_used_but_never_overwritten() {
        let text = GOOD.replace("schema_version = 1", "schema_version = 7\nfuture_key = true");
        let l = load(Some(&text), None);
        assert_eq!(l.problems, vec![ConfigProblem::Newer { found: 7 }]);
        assert!(l.read_only && !l.needs_save);
        assert_eq!(l.config.runners.len(), 2);
        let via_backup = load(Some("broken ="), Some(&text));
        assert!(via_backup.read_only && !via_backup.needs_save);
    }

    #[test]
    fn server_urls_are_checked() {
        assert_eq!(check_server_url(" https://git.example.com/ ").unwrap(), "https://git.example.com");
        assert!(check_server_url("http://10.0.0.5:3000").is_ok());
        for bad in [
            "git.example.com",
            "https://",
            "https://u:p@git.example.com",
            "https://a b",
            &format!("https://{}", "a".repeat(600)),
        ] {
            assert!(check_server_url(bad).is_err(), "{bad}");
        }
        let text = "schema_version = 1
[[runners]]
name = \"a\"
kind = \"gitea-runner\"
dir = \"/r/a\"
server_url = \"ftp://x\"
";
        let l = load(Some(text), None);
        assert!(l.config.runners.is_empty());
    }

    #[test]
    fn effective_policies_fall_back_to_the_defaults() {
        let text = "schema_version = 1
[defaults.cleanup]
cache_cap_mb = 5
[[runners]]
name = \"a\"
kind = \"gitea-runner\"
dir = \"/r/a\"
logged_off = \"service-account\"
[runners.cleanup]
automatic = false
";
        let l = load(Some(text), None);
        assert!(l.problems.is_empty(), "{:?}", l.problems);
        let r = &l.config.runners[0];
        let d = &l.config.defaults;
        assert_eq!(d.cleanup.cache_cap_mb, 5);
        assert!(!r.cleanup_policy(d).automatic);
        assert_eq!(r.stall_rule(d), d.watchdog);
        assert_eq!(r.update_policy(d), &d.update);
        assert_eq!(r.logged_off, LoggedOffMode::ServiceAccount);
    }

    #[test]
    fn absolute_paths_are_recognised_for_both_platforms() {
        for ok in ["/srv/x", "C:\\x", "c:/x", "\\\\server\\share"] {
            assert!(is_absolute_anywhere(ok), "{ok}");
        }
        for bad in ["x", "C:", "C:x", ".\\x", ""] {
            assert!(!is_absolute_anywhere(bad), "{bad}");
        }
    }

    #[test]
    fn process_settings_load_override_and_are_validated() {
        let l = load(
            Some(
                r#"
schema_version = 1
[defaults.processes]
cleanup = false
keep = { amount = 2, unit = "hours" }
[[defaults.processes.rules]]
pattern = "adb"
keep = "never"
[[runners]]
name = "a"
kind = "gitea-runner"
dir = "/r/a"
[runners.processes]
cleanup = true
[[runners.processes.rules]]
pattern = "java"
keep = { amount = 1, unit = "days" }
[[runners]]
name = "b"
kind = "gitea-runner"
dir = "/r/b"
"#,
            ),
            None,
        );
        assert!(l.problems.is_empty(), "{:?}", l.problems);
        let d = &l.config.defaults;
        assert!(!d.processes.cleanup);
        let a = l.config.runners[0].process_policy(d);
        assert!(a.cleanup && a.keep == crate::process_policy::KeepFor::hours(2));
        assert_eq!(a.rules[0].pattern, "java", "the runner's rules come first");
        assert!(!l.config.runners[1].process_policy(d).cleanup);
        // The shipped defaults are on.
        assert!(Config::default().defaults.processes.cleanup);
        // They survive a save.
        let again = load(Some(&to_toml(&l.config).unwrap()), None);
        assert_eq!(again.config, l.config);
        // Invalid values are reported: the section is replaced, a runner is skipped.
        let bad = load(
            Some("schema_version = 1\n[defaults.processes]\nkeep = { amount = 0, unit = \"hours\" }\n"),
            None,
        );
        assert!(matches!(bad.problems[0], ConfigProblem::SectionIgnored { .. }));
        assert!(bad.config.defaults.processes.cleanup);
        let bad = load(
            Some("schema_version = 1\n[[runners]]\nname = \"a\"\nkind = \"gitea-runner\"\ndir = \"/r\"\n[runners.processes]\nkeep = { amount = 0, unit = \"hours\" }\n"),
            None,
        );
        assert!(matches!(bad.problems[0], ConfigProblem::RunnerSkipped { .. }));
    }

    #[test]
    fn disk_guard_and_pause_settings_load_and_bad_ones_fall_back() {
        let text = "schema_version = 1\n[defaults.disk_guard]\ndrain_below_mb = 2048\nresume_above_mb = 3072\nreserve_mb = 256\n[defaults.pause]\nmode = \"schedule\"\n[[defaults.pause.windows]]\nstart = \"22:00\"\nend = \"06:00\"\n";
        let l = load(Some(text), None);
        assert!(l.problems.is_empty(), "{:?}", l.problems);
        assert_eq!(l.config.defaults.disk_guard.drain_below_mb, 2048);
        assert_eq!(l.config.defaults.pause.windows.len(), 1);
        let back = load(Some(&to_toml(&l.config).unwrap()), None);
        assert_eq!(back.config.defaults.pause, l.config.defaults.pause);
        let bad =
            "schema_version = 1\n[defaults.disk_guard]\ndrain_below_mb = 100\nresume_above_mb = 50\n[defaults.pause]\nquiet_minutes = 0\n";
        let l = load(Some(bad), None);
        assert_eq!(l.problems.len(), 2, "{:?}", l.problems);
        assert_eq!(l.config.defaults.disk_guard, DiskGuardPolicy::default());
        assert_eq!(l.config.defaults.pause, PausePolicy::default());
    }
}
