//! The set of an app's plugins: discovered from the plugins folder, one [`Supervisor`] each, with the operations the
//! Plugins view needs (list with status, install and update, enable, grant, remove, re-enable after a quarantine).

use crate::config::HostConfig;
use crate::grants::Grants;
use crate::installed::{Inspection, Installed, PluginsDir};
use crate::services::RunningTask;
use crate::status::Status;
use crate::supervisor::Supervisor;
use paddock_plugin_package::{PackageError, PermissionRequest, TrustPolicy};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// One line of the Plugins view.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// The plugin's id.
    pub id: String,
    /// Display name (the id when the manifest cannot be read).
    pub name: String,
    /// Version (empty when the manifest cannot be read).
    pub version: String,
    /// The status, with the reason for a refusal or quarantine.
    pub status: Status,
    /// What it asks for.
    pub requested: Vec<PermissionRequest>,
    /// What the user granted.
    pub grants: Grants,
    /// What it contributes (`tool:collector`, `view:findings`, `extension:debrief.import`).
    pub contributions: Vec<String>,
    /// Its running background tasks.
    pub tasks: Vec<RunningTask>,
}

/// An app's plugins.
pub struct PluginHost {
    dir: PluginsDir,
    cfg: Arc<HostConfig>,
    plugins: Mutex<BTreeMap<String, Supervisor>>,
    unreadable: Mutex<BTreeMap<String, String>>,
}

impl PluginHost {
    /// Discovers the plugins in `dir`. Nothing is started; one that cannot be read is listed as refused with the reason.
    pub fn load(dir: PluginsDir, cfg: Arc<HostConfig>) -> PluginHost {
        let host = PluginHost {
            dir,
            cfg,
            plugins: Mutex::default(),
            unreadable: Mutex::default(),
        };
        host.rescan();
        host
    }

    /// Reads the plugins folder again, keeping the supervisors of plugins that did not change.
    pub fn rescan(&self) {
        let mut plugins = lock(&self.plugins);
        let mut unreadable = lock(&self.unreadable);
        unreadable.clear();
        let mut seen = Vec::new();
        for entry in self.dir.list() {
            match entry {
                Ok(installed) => {
                    let id = installed.manifest.id.to_string();
                    seen.push(id.clone());
                    plugins
                        .entry(id)
                        .or_insert_with(|| Supervisor::new(self.cfg.clone(), installed));
                }
                Err((id, e)) => {
                    seen.push(id.clone());
                    plugins.remove(&id);
                    unreadable.insert(id, e.to_string());
                }
            }
        }
        plugins.retain(|id, sup| {
            let keep = seen.contains(id);
            if !keep {
                sup.stop();
            }
            keep
        });
    }

    /// The supervisor of `id`.
    pub fn get(&self, id: &str) -> Option<Supervisor> {
        lock(&self.plugins).get(id).cloned()
    }

    /// Every plugin for the Plugins view, by id.
    pub fn list(&self) -> Vec<PluginInfo> {
        let mut out: Vec<PluginInfo> = lock(&self.plugins)
            .values()
            .map(|s| {
                let m = s.manifest();
                PluginInfo {
                    id: s.id().into(),
                    name: m.name.clone(),
                    version: m.version.to_string(),
                    status: s.status(),
                    requested: m.permissions.clone(),
                    grants: s.grants(),
                    contributions: m.contribution_keys(),
                    tasks: s.tasks(),
                }
            })
            .collect();
        out.extend(lock(&self.unreadable).iter().map(|(id, why)| PluginInfo {
            id: id.clone(),
            name: id.clone(),
            version: String::new(),
            status: Status::Refused(why.clone()),
            requested: Vec::new(),
            grants: Grants::none(),
            contributions: Vec::new(),
            tasks: Vec::new(),
        }));
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// What installing the package at `path` would do.
    pub fn inspect(&self, path: &Path, trust: &TrustPolicy) -> Result<Inspection, PackageError> {
        self.dir.inspect(path, trust, &self.cfg.info)
    }

    /// Installs or updates from the package at `path` that the user reviewed as `digest`. The old process is stopped; the new
    /// program is verified again before its first start.
    pub fn install(
        &self,
        path: &Path,
        trust: &TrustPolicy,
        digest: &str,
        grants: Grants,
    ) -> Result<Installed, PackageError> {
        let installed = self
            .dir
            .install(path, trust, &self.cfg.info, digest, grants)?;
        let id = installed.manifest.id.to_string();
        if let Some(old) = lock(&self.plugins).remove(&id) {
            old.stop();
        }
        lock(&self.unreadable).remove(&id);
        lock(&self.plugins).insert(id, Supervisor::new(self.cfg.clone(), installed.clone()));
        Ok(installed)
    }

    /// Switches a plugin on or off (kept in its record).
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), PackageError> {
        self.dir.set_enabled(id, enabled)?;
        if let Some(s) = self.get(id) {
            s.set_enabled(enabled);
        }
        Ok(())
    }

    /// Changes what the user granted (kept in its record).
    pub fn set_grants(&self, id: &str, grants: Grants) -> Result<(), PackageError> {
        self.dir.set_grants(id, grants.clone())?;
        if let Some(s) = self.get(id) {
            s.set_grants(grants);
        }
        Ok(())
    }

    /// Re-enables a quarantined plugin.
    pub fn release(&self, id: &str) {
        if let Some(s) = self.get(id) {
            s.release();
        }
    }

    /// Removes a plugin: its process is stopped and its folders deleted (its data too unless `keep_data`).
    pub fn remove(&self, id: &str, keep_data: bool) -> Result<(), PackageError> {
        if let Some(s) = lock(&self.plugins).remove(id) {
            s.stop();
        }
        self.cfg.quarantine.release(id);
        self.dir.remove(id, keep_data)
    }

    /// Stops every plugin process (the app is closing).
    pub fn stop_all(&self) {
        for s in lock(&self.plugins).values() {
            s.stop();
        }
    }
}
