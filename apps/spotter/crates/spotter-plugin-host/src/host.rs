//! The plugin registry: finds installed plugins, asks each for its manifest, and runs list requests guarded. Every
//! plugin call goes through here, so a crash, hang or memory blow-up is attributed to its plugin, counts as a strike
//! ([`spotter_core::plugin_health`]) and never reaches the caller as a panic.

use crate::session::{self, Fault, Limits};
use serde::Deserialize;
use spotter_core::plugin_health::PluginHealth;
use spotter_core::release::Candidate;
use spotter_core::source::SourceError;
use spotter_core::time::Timestamp;
use spotter_plugin_api::{Config, Manifest, Request, Response, SourceKindSpec};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;

/// The file that describes an installed plugin, inside its folder.
pub const MANIFEST_FILE: &str = "plugin.toml";
/// Most plugins loaded.
pub const MAX_PLUGINS: usize = 64;

/// `plugin.toml`.
#[derive(Debug, Clone, Deserialize)]
struct PluginToml {
    id: String,
    executable: String,
    #[serde(default)]
    memory_limit_mb: Option<u64>,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// An installed plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    /// Its id (folder and `plugin.toml`).
    pub id: String,
    /// Its folder.
    pub dir: PathBuf,
    /// Its program (inside its folder).
    pub exe: PathBuf,
    /// Its limits.
    pub limits: Limits,
    /// What it offers, when it answered.
    pub manifest: Option<Manifest>,
    /// Why it cannot be used, when it cannot.
    pub problem: Option<String>,
}

/// A plugin as the plugin manager shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginStatus {
    /// The installation.
    pub installed: Installed,
    /// Switched off by the user.
    pub disabled: bool,
    /// Its health.
    pub health: PluginHealth,
}

/// The result of one guarded list call.
#[derive(Debug, Clone, PartialEq)]
pub struct ListOutcome {
    /// The candidates or why not.
    pub result: Result<Vec<Candidate>, SourceError>,
    /// A plugin problem to record (a strike), when there was one.
    pub issue: Option<String>,
    /// Whether this call quarantined the plugin.
    pub quarantined: bool,
}

/// The registry.
#[derive(Default)]
pub struct PluginHost {
    plugins: RwLock<Vec<Installed>>,
    disabled: RwLock<Vec<String>>,
    health: Mutex<BTreeMap<String, PluginHealth>>,
}

fn read<T>(l: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(PoisonError::into_inner)
}

fn write<T>(l: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(PoisonError::into_inner)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn valid_id(id: &str) -> bool {
    (1..=40).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Reads one plugin folder (without starting the plugin).
pub fn inspect(dir: &Path) -> Installed {
    let mut p = Installed {
        id: dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        dir: dir.to_path_buf(),
        exe: PathBuf::new(),
        limits: Limits::default(),
        manifest: None,
        problem: None,
    };
    let text = match std::fs::read_to_string(dir.join(MANIFEST_FILE)) {
        Ok(t) if t.len() <= 64 << 10 => t,
        Ok(_) => return fail(p, "plugin.toml is too large"),
        Err(e) => return fail(p, &format!("plugin.toml could not be read ({e})")),
    };
    let spec: PluginToml = match toml::from_str(&text) {
        Ok(s) => s,
        Err(e) => return fail(p, &format!("plugin.toml is not valid ({})", e.message())),
    };
    if !valid_id(&spec.id) {
        return fail(p, "its id must be 1 to 40 of a-z, 0-9 and -");
    }
    p.id = spec.id;
    if let Some(mb) = spec.memory_limit_mb {
        p.limits.memory_bytes = mb.clamp(16, 4096) << 20;
    }
    if let Some(s) = spec.timeout_secs {
        p.limits.request = Duration::from_secs(s.clamp(1, 600));
    }
    let mut exe = dir.join(&spec.executable);
    if cfg!(windows) && exe.extension().is_none() {
        exe.set_extension("exe");
    }
    // The program must be inside the plugin's folder (no `..`, no links out of it).
    let (Ok(real_dir), Ok(real_exe)) = (dir.canonicalize(), exe.canonicalize()) else {
        p.exe = exe;
        return fail(p, "its program was not found");
    };
    p.exe = real_exe.clone();
    if !real_exe.starts_with(&real_dir) || !real_exe.is_file() {
        return fail(p, "its program must be a file inside the plugin's folder");
    }
    p
}

fn fail(mut p: Installed, why: &str) -> Installed {
    p.problem = Some(format!("The plugin in {} cannot be used: {why}.", p.dir.display()));
    p
}

impl PluginHost {
    /// An empty registry.
    pub fn new() -> PluginHost {
        PluginHost::default()
    }

    /// Finds the plugins in `dir` (one folder each) and asks each for its manifest. Replaces what was loaded before.
    /// A missing folder means no plugins.
    pub fn scan(&self, dir: &Path) {
        let mut found = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            dirs.sort();
            for d in dirs.into_iter().take(MAX_PLUGINS) {
                let mut p = inspect(&d);
                if p.problem.is_none() && !found.iter().any(|f: &Installed| f.id == p.id) {
                    self.describe(&mut p);
                }
                found.push(p);
            }
        }
        *write(&self.plugins) = found;
    }

    fn describe(&self, p: &mut Installed) {
        let limits = Limits {
            request: Duration::from_secs(10),
            ..p.limits
        };
        match session::run(&p.exe, &p.id, &[Request::Describe], limits) {
            Ok(r) => match r.into_iter().next() {
                Some(Response::Described(m)) if m.id == p.id && m.kinds.iter().all(|k| valid_id(&k.id)) => p.manifest = Some(m),
                Some(Response::Described(m)) => {
                    p.problem = Some(format!(
                        "The plugin \"{}\" describes itself as \"{}\" or offers a kind with an invalid id; its plugin.toml and its manifest must agree.",
                        p.id, m.id
                    ))
                }
                _ => p.problem = Some(format!("The plugin \"{}\" did not describe itself.", p.id)),
            },
            Err(f) => {
                p.problem = Some(format!("The plugin \"{}\" {f}.", p.id));
                if !matches!(f, Fault::Start(_)) {
                    lock(&self.health).entry(p.id.clone()).or_default().strike(now(), f.to_string());
                }
            }
        }
    }

    /// Sets the plugins the user switched off.
    pub fn set_disabled(&self, ids: Vec<String>) {
        *write(&self.disabled) = ids;
    }

    /// Loads saved health (quarantine survives restarts).
    pub fn restore_health(&self, health: BTreeMap<String, PluginHealth>) {
        *lock(&self.health) = health;
    }

    /// The health to save.
    pub fn health(&self) -> BTreeMap<String, PluginHealth> {
        lock(&self.health).clone()
    }

    /// Lifts a quarantine.
    pub fn reset(&self, id: &str) {
        lock(&self.health).remove(id);
    }

    /// Every installed plugin with its state.
    pub fn statuses(&self) -> Vec<PluginStatus> {
        let disabled = read(&self.disabled);
        let health = lock(&self.health);
        read(&self.plugins)
            .iter()
            .map(|p| PluginStatus {
                installed: p.clone(),
                disabled: disabled.contains(&p.id),
                health: health.get(&p.id).cloned().unwrap_or_default(),
            })
            .collect()
    }

    /// The kinds of every usable, enabled plugin, with their full ids (`<plugin>/<kind>`) and plugin names.
    pub fn kinds(&self) -> Vec<(String, String, SourceKindSpec)> {
        let disabled = read(&self.disabled);
        read(&self.plugins)
            .iter()
            .filter(|p| !disabled.contains(&p.id))
            .filter_map(|p| p.manifest.as_ref().map(|m| (p, m)))
            .flat_map(|(p, m)| {
                m.kinds
                    .iter()
                    .map(move |k| (format!("{}/{}", p.id, k.id), m.name.clone(), k.clone()))
            })
            .collect()
    }

    /// One kind by full id (also for a quarantined plugin, so its watches stay valid and show the quarantine).
    pub fn kind(&self, id: &str) -> Option<SourceKindSpec> {
        self.kinds().into_iter().find(|(full, _, _)| full == id).map(|(_, _, k)| k)
    }

    /// Lists a plugin source, guarded.
    pub fn list(&self, plugin: &str, kind: &str, config: Config, limit: usize, now: Timestamp) -> ListOutcome {
        let found = read(&self.plugins).iter().find(|p| p.id == plugin && p.manifest.is_some()).cloned();
        let Some(p) = found else {
            return ListOutcome {
                result: Err(SourceError::InvalidConfig(format!(
                    "The plugin \"{plugin}\" is not installed or could not be loaded."
                ))),
                issue: None,
                quarantined: false,
            };
        };
        if read(&self.disabled).contains(&p.id) {
            return outcome_err(SourceError::Plugin(format!("The plugin \"{plugin}\" is switched off.")));
        }
        if lock(&self.health).get(plugin).is_some_and(|h| h.quarantined) {
            return outcome_err(SourceError::Plugin(format!(
                "The plugin \"{plugin}\" is quarantined after repeated failures. Re-enable it in Plugins when it is fixed."
            )));
        }
        let request = Request::List {
            kind: kind.into(),
            config,
            limit: u32::try_from(limit).unwrap_or(u32::MAX),
        };
        match session::run(&p.exe, plugin, &[request], p.limits) {
            Ok(r) => match r.into_iter().next() {
                Some(Response::Listed(releases)) => outcome(Ok(releases.into_iter().take(limit).map(Candidate::from).collect())),
                Some(Response::Failed(e)) => outcome(Err(SourceError::from(e))),
                _ => self.strike(plugin, now, "answered a list request with something else".into()),
            },
            Err(f) => self.strike(plugin, now, f.to_string()),
        }
    }

    fn strike(&self, plugin: &str, now: Timestamp, problem: String) -> ListOutcome {
        let quarantined = lock(&self.health)
            .entry(plugin.to_string())
            .or_default()
            .strike(now, problem.clone());
        let message = format!("The plugin \"{plugin}\" {problem}.");
        ListOutcome {
            result: Err(SourceError::Plugin(message.clone())),
            issue: Some(message),
            quarantined,
        }
    }
}

fn outcome(result: Result<Vec<Candidate>, SourceError>) -> ListOutcome {
    ListOutcome {
        result,
        issue: None,
        quarantined: false,
    }
}

fn outcome_err(e: SourceError) -> ListOutcome {
    outcome(Err(e))
}

fn now() -> Timestamp {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    Timestamp(i64::try_from(ms).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &Path, text: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(MANIFEST_FILE), text).unwrap();
    }

    #[test]
    fn broken_installations_are_reported_not_started() {
        let root = tempfile::tempdir().unwrap();
        let cases = [
            ("no-manifest", None),
            ("bad-toml", Some("id = ")),
            ("bad-id", Some("id = 'Bad Id'\nexecutable = 'x'")),
            ("missing-exe", Some("id = 'missing-exe'\nexecutable = 'nope'")),
            ("escape", Some("id = 'escape'\nexecutable = '../outside'")),
            ("too-large", Some(&*format!("id = 'x'\n#{}", "x".repeat(70 << 10)))),
        ];
        for (name, manifest) in cases {
            let d = root.path().join(name);
            std::fs::create_dir_all(&d).unwrap();
            if let Some(m) = manifest {
                write_manifest(&d, m);
            }
        }
        std::fs::write(root.path().join("outside.exe"), b"x").unwrap();
        std::fs::write(root.path().join("outside"), b"x").unwrap();
        let host = PluginHost::new();
        host.scan(root.path());
        let statuses = host.statuses();
        assert_eq!(statuses.len(), 6);
        assert!(
            statuses
                .iter()
                .all(|s| s.installed.problem.is_some() && s.installed.manifest.is_none()),
            "{statuses:#?}"
        );
        assert!(host.kinds().is_empty());
        let escape = statuses.iter().find(|s| s.installed.id == "escape").unwrap();
        assert!(escape.installed.problem.as_ref().unwrap().contains("inside the plugin's folder"));
        let out = host.list("escape", "k", Config::new(), 10, Timestamp(0));
        assert!(matches!(out.result, Err(SourceError::InvalidConfig(_))) && out.issue.is_none());
        host.scan(&root.path().join("does-not-exist"));
        assert!(host.statuses().is_empty());
    }

    #[test]
    fn limits_come_from_the_manifest() {
        let root = tempfile::tempdir().unwrap();
        let d = root.path().join("p");
        write_manifest(&d, "id = 'p'\nexecutable = 'p'\nmemory_limit_mb = 1\ntimeout_secs = 9999");
        let p = inspect(&d);
        assert_eq!((p.limits.memory_bytes, p.limits.request), (16 << 20, Duration::from_secs(600)));
    }

    #[test]
    fn health_survives_and_resets() {
        let host = PluginHost::new();
        let h = PluginHealth {
            quarantined: true,
            ..PluginHealth::default()
        };
        host.restore_health([("p".to_string(), h)].into());
        assert!(host.health()["p"].quarantined);
        host.reset("p");
        assert!(host.health().is_empty());
        host.set_disabled(vec!["p".into()]);
        assert_eq!(host.kind("p/k"), None);
    }
}
