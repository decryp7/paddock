//! Installed plugins: where they live, installing and updating from a `.paddockplugin` package, removing, enabling and
//! granting, and the check that an installed program is still the one that was verified.
//!
//! ```text
//! <plugins>/<id>/install.json                  what was installed and what the user granted (InstallRecord)
//! <plugins>/<id>/v-<version>-<digest8>/bin/<program>, manifest.toml
//! <plugins>/<id>/data/                         the plugin's private folder, kept across updates
//! ```
//!
//! An install or update writes a new version folder next to the old one and then switches `install.json` (a
//! [`paddock_env::VersionedFile`]: atomic, with a backup, a damaged file never crashes). A running process keeps its
//! program until it restarts; [`PluginsDir::cleanup`] removes folders nothing refers to. Everything read back from disk is
//! checked: a damaged record or a changed manifest makes its plugin unavailable with a reason, and a changed program is
//! [`PackageError::Tampered`], which the supervisor treats as a quarantine-grade fault.

use crate::grants::Grants;
use paddock_env::{AppDirs, VersionedFile};
use paddock_jobs::time::Clock;
use paddock_plugin_api::compat::{HostInfo, protocol_of};
use paddock_plugin_package::archive::verify_installed;
use paddock_plugin_package::keys::{from_hex, to_hex};
use paddock_plugin_package::manifest::host_target;
use paddock_plugin_package::{Manifest, Package, PackageError, Permission, PermissionRequest, SignatureStatus, Trust, TrustPolicy};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// The record file's name in a plugin's folder.
pub const RECORD_FILE: &str = "install.json";
/// The record's format version.
pub const RECORD_FORMAT: u32 = 1;

/// What was installed and what the user granted: `install.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRecord {
    /// The plugin's id.
    pub id: String,
    /// The installed version.
    pub version: String,
    /// The signature status when it was installed (`builtin`, `trusted`, `unknownKey`, `unsigned`).
    pub signature: String,
    /// The trust level it was installed at (`builtin`, `signed`, `unsigned`).
    pub trust: String,
    /// The publisher's name for a signed package.
    pub signer: Option<String>,
    /// The warning the user accepted for an unsigned package.
    pub warning: Option<String>,
    /// The package digest the user reviewed.
    pub digest: String,
    /// The version folder.
    pub folder: String,
    /// The program's file name in `bin/`.
    pub program: String,
    /// The program's SHA-256, hex.
    pub sha256: String,
    /// What the manifest asked for when it was installed.
    pub requested: Vec<PermissionRequest>,
    /// What the user granted.
    pub grants: Grants,
    /// Whether the user has it switched on.
    pub enabled: bool,
    /// When it was installed (Unix milliseconds on the injected clock).
    pub installed_at_ms: i64,
}

/// The file's content: a record, or nothing yet.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct RecordFile {
    record: Option<InstallRecord>,
}

/// An installed plugin whose record, manifest and folder passed the checks.
#[derive(Debug, Clone)]
pub struct Installed {
    /// The record.
    pub record: InstallRecord,
    /// The manifest read back from the version folder.
    pub manifest: Manifest,
    /// The program to start.
    pub program: PathBuf,
    /// The plugin's private data folder.
    pub data_dir: PathBuf,
    /// The program's SHA-256.
    pub sha256: [u8; 32],
}

/// What a package is and would do, shown before the user installs it.
#[derive(Debug, Clone)]
pub struct Inspection {
    /// The package's manifest.
    pub manifest: Manifest,
    /// The package digest; install must be given the same one.
    pub digest: String,
    /// Who signed it.
    pub signature: SignatureStatus,
    /// Whether the trust policy allows installing it, and at which level.
    pub trust: Result<Trust, PackageError>,
    /// Whether this app can serve it (protocol, contracts, a program for this computer).
    pub runnable: Result<(), PackageError>,
    /// The installed version, when this is an update.
    pub installed: Option<InstallRecord>,
    /// Permissions an update asks for that the installed version did not.
    pub added_permissions: Vec<Permission>,
    /// Permissions the installed version had that the update drops.
    pub removed_permissions: Vec<Permission>,
}

impl Inspection {
    /// Whether it can be installed now.
    pub fn installable(&self) -> Result<(), PackageError> {
        self.trust.as_ref().map_err(Clone::clone)?;
        self.runnable.clone()
    }
}

fn plain(rel: &str) -> bool {
    let p = Path::new(rel);
    !rel.is_empty() && p.components().count() == 1 && matches!(p.components().next(), Some(Component::Normal(_)))
}

fn io(e: std::io::Error) -> PackageError {
    PackageError::Io(e.to_string())
}

/// The folder of installed plugins.
#[derive(Clone)]
pub struct PluginsDir {
    root: PathBuf,
    clock: Arc<dyn Clock>,
}

impl PluginsDir {
    /// Plugins under `root`, time stamps from `clock`.
    pub fn new(root: impl Into<PathBuf>, clock: Arc<dyn Clock>) -> PluginsDir {
        PluginsDir { root: root.into(), clock }
    }

    /// `<data folder of the app>/plugins`.
    pub fn of(dirs: &AppDirs, clock: Arc<dyn Clock>) -> PluginsDir {
        PluginsDir::new(dirs.data().join("plugins"), clock)
    }

    /// The root folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn plugin_dir(&self, id: &str) -> Result<PathBuf, PackageError> {
        if paddock_plugin_package::PluginId::new(id).is_err() {
            return Err(PackageError::InvalidManifest(format!("\"{id}\" is not a plugin id")));
        }
        Ok(self.root.join(id))
    }

    fn file(&self, id: &str) -> Result<VersionedFile<RecordFile>, PackageError> {
        Ok(VersionedFile::new(self.plugin_dir(id)?.join(RECORD_FILE), RECORD_FORMAT).private())
    }

    /// The record of `id`, if it is installed. A damaged record is an error that says to reinstall.
    pub fn record(&self, id: &str) -> Result<Option<InstallRecord>, PackageError> {
        let loaded = self.file(id)?.load();
        if let Some(problem) = loaded.problem {
            return Err(PackageError::Io(format!("the record of {id} is not usable ({problem}); reinstall it")));
        }
        match loaded.value.record {
            Some(r) if r.id == id => Ok(Some(r)),
            Some(_) => Err(PackageError::Tampered(format!("the record in the folder of {id} belongs to another plugin"))),
            None => Ok(None),
        }
    }

    fn save(&self, record: &InstallRecord) -> Result<(), PackageError> {
        let dir = self.plugin_dir(&record.id)?;
        std::fs::create_dir_all(&dir).map_err(io)?;
        self.file(&record.id)?.save(&RecordFile { record: Some(record.clone()) })?;
        Ok(())
    }

    /// Reads `id` back: the record, the manifest in its version folder (it must still ask for what the user saw), the program
    /// path.
    pub fn load(&self, id: &str) -> Result<Option<Installed>, PackageError> {
        let Some(record) = self.record(id)? else { return Ok(None) };
        if !plain(&record.folder) || !plain(&record.program) {
            return Err(PackageError::Tampered(format!("the record of {id} names paths outside its folder")));
        }
        let dir = self.plugin_dir(id)?;
        let version_dir = dir.join(&record.folder);
        let text = std::fs::read_to_string(version_dir.join("manifest.toml"))
            .map_err(|e| PackageError::MissingFile(format!("manifest.toml of {id} ({e})")))?;
        let manifest = Manifest::parse(&text)?;
        if manifest.id.as_str() != id || manifest.version.to_string() != record.version || manifest.permissions != record.requested {
            return Err(PackageError::Tampered(format!("the manifest of {id} is not the one that was installed")));
        }
        let sha256 = from_hex::<32>(&record.sha256).ok_or_else(|| PackageError::Tampered(format!("the record of {id} has no valid hash")))?;
        Ok(Some(Installed {
            program: version_dir.join("bin").join(&record.program),
            data_dir: dir.join("data"),
            sha256,
            manifest,
            record,
        }))
    }

    /// Every installed plugin, by id; one that cannot be read is listed with the reason, never skipped.
    pub fn list(&self) -> Vec<Result<Installed, (String, PackageError)>> {
        let Ok(entries) = std::fs::read_dir(&self.root) else { return Vec::new() };
        let mut ids: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().join(RECORD_FILE).is_file())
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter(|id| paddock_plugin_package::PluginId::new(id.as_str()).is_ok())
            .collect();
        ids.sort();
        ids.into_iter()
            .filter_map(|id| match self.load(&id) {
                Ok(Some(i)) => Some(Ok(i)),
                Ok(None) => None,
                Err(e) => Some(Err((id, e))),
            })
            .collect()
    }

    /// Whether the program of `plugin` is still the one that was verified at install. A changed or replaced program is
    /// [`PackageError::Tampered`] (a quarantine-grade fault), a deleted one [`PackageError::MissingFile`].
    pub fn verify(&self, plugin: &Installed) -> Result<(), PackageError> {
        verify_installed(&plugin.program, &plugin.sha256)
    }

    /// Reads and verifies the package at `path` and says what installing it would do.
    pub fn inspect(&self, path: &Path, trust: &TrustPolicy, host: &HostInfo) -> Result<Inspection, PackageError> {
        let pkg = Package::open(path)?;
        let m = &pkg.manifest;
        let installed = self.record(m.id.as_str()).ok().flatten();
        let asked: Vec<Permission> = m.permissions.iter().map(|p| p.permission.clone()).collect();
        let before: Vec<Permission> = installed.as_ref().map(|r| r.requested.iter().map(|p| p.permission.clone()).collect()).unwrap_or_default();
        let label = m.id.to_string();
        let runnable = host
            .check_protocol(&label, protocol_of(m.protocol))
            .and_then(|()| m.namespaces().try_for_each(|n| host.check_extension(&label, n.ns(), n.minor)))
            .map_err(|r| PackageError::Incompatible(r.to_string()))
            .and_then(|()| match pkg.program(host_target()) {
                Some(_) => Ok(()),
                None => Err(PackageError::WrongTarget {
                    target: host_target().into(),
                    supported: m.targets.join(", "),
                }),
            });
        Ok(Inspection {
            digest: pkg.digest(),
            signature: trust.status(&pkg.signature),
            trust: trust.decide(&pkg.signature),
            runnable,
            added_permissions: asked.iter().filter(|p| !before.contains(p)).cloned().collect(),
            removed_permissions: before.iter().filter(|p| installed.is_some() && !asked.contains(p)).cloned().collect(),
            installed,
            manifest: pkg.manifest,
        })
    }

    /// Installs (or updates to) the package at `path`, which the user reviewed as `digest`, with `grants` (only what the
    /// manifest asks for stays in force). The program is copied out and checked against the package's checksum; the record
    /// is switched last.
    pub fn install(
        &self,
        path: &Path,
        trust: &TrustPolicy,
        host: &HostInfo,
        digest: &str,
        grants: Grants,
    ) -> Result<Installed, PackageError> {
        let inspection = self.inspect(path, trust, host)?;
        let changed = || PackageError::Tampered("the package changed after you reviewed it; review it again".into());
        if inspection.digest != digest {
            return Err(changed());
        }
        let trust_level = inspection.trust.clone().and_then(|t| inspection.runnable.clone().map(|()| t))?;
        let pkg = Package::open(path)?;
        if pkg.digest() != digest {
            return Err(changed());
        }
        let m = &pkg.manifest;
        let dir = self.plugin_dir(m.id.as_str())?;
        let folder = format!("v-{}-{}", m.version, &digest[..8.min(digest.len())]);
        let version_dir = dir.join(&folder);
        let staging = dir.join(format!(".staging-{}", self.clock.wall_ms()));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(staging.join("bin")).map_err(io)?;
        std::fs::create_dir_all(dir.join("data")).map_err(io)?;
        let built = (|| -> Result<InstallRecord, PackageError> {
            let program = m.binary_file(host_target());
            let sha = pkg.extract(path, host_target(), &staging.join("bin").join(&program))?;
            std::fs::write(staging.join("manifest.toml"), m.to_toml()).map_err(io)?;
            if version_dir.exists() {
                std::fs::remove_dir_all(&version_dir).map_err(io)?;
            }
            std::fs::rename(&staging, &version_dir).map_err(io)?;
            let (signature, signer) = match &inspection.signature {
                SignatureStatus::Builtin(n) | SignatureStatus::Trusted(n) => (inspection.signature.code(), Some(n.clone())),
                SignatureStatus::UnknownKey(id) => (inspection.signature.code(), Some(id.clone())),
                SignatureStatus::Unsigned => (inspection.signature.code(), None),
            };
            let granted = Grants {
                permissions: grants.permissions.iter().filter(|p| m.permissions.iter().any(|r| &r.permission == *p)).cloned().collect(),
                folders: grants.folders.clone(),
            };
            Ok(InstallRecord {
                id: m.id.to_string(),
                version: m.version.to_string(),
                signature: signature.into(),
                trust: trust_level.code().into(),
                signer,
                warning: match &trust_level {
                    Trust::Unsigned { warning } => Some(warning.clone()),
                    _ => None,
                },
                digest: digest.into(),
                folder: folder.clone(),
                program,
                sha256: to_hex(&sha),
                requested: m.permissions.clone(),
                grants: granted,
                enabled: inspection.installed.as_ref().is_none_or(|p| p.enabled),
                installed_at_ms: self.clock.wall_ms(),
            })
        })();
        let record = match built {
            Ok(r) => r,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(e);
            }
        };
        self.save(&record)?;
        self.cleanup(m.id.as_str());
        self.load(m.id.as_str())?.ok_or_else(|| PackageError::Io("the plugin was installed but cannot be read back".into()))
    }

    /// Switches a plugin on or off.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), PackageError> {
        let mut record = self.record(id)?.ok_or_else(|| PackageError::MissingFile(format!("plugin {id} is not installed")))?;
        record.enabled = enabled;
        self.save(&record)
    }

    /// Replaces what the user granted (only what the manifest asks for stays in force when it is used).
    pub fn set_grants(&self, id: &str, grants: Grants) -> Result<(), PackageError> {
        let mut record = self.record(id)?.ok_or_else(|| PackageError::MissingFile(format!("plugin {id} is not installed")))?;
        record.grants = grants;
        self.save(&record)
    }

    /// Removes a plugin and its folders. Its data folder goes too unless `keep_data`.
    pub fn remove(&self, id: &str, keep_data: bool) -> Result<(), PackageError> {
        let dir = self.plugin_dir(id)?;
        if !dir.exists() {
            return Ok(());
        }
        if !keep_data {
            return std::fs::remove_dir_all(&dir).map_err(io);
        }
        for entry in std::fs::read_dir(&dir).map_err(io)?.flatten() {
            if entry.file_name() != "data" {
                let p = entry.path();
                if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) }.map_err(io)?;
            }
        }
        Ok(())
    }

    /// Removes version folders and staging leftovers nothing refers to.
    pub fn cleanup(&self, id: &str) {
        let (Ok(dir), Ok(Some(record))) = (self.plugin_dir(id), self.record(id)) else { return };
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let stale = (name.starts_with("v-") && name != record.folder) || name.starts_with(".staging-");
            if stale {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}
