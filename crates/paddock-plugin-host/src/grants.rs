//! Permission grants: what the user allowed each plugin (default deny), and the pure mapping from a manifest and its grants
//! to a [`SandboxSpec`].
//!
//! A manifest *asks* ([`paddock_plugin_package::Permission`]); only what the user *granted* and the manifest also asked for
//! is in force. Folders are named grants (`GrantRef`): the user maps each name to a folder through the app's dialog, and a
//! plugin never sees a raw path it was not given.

use paddock_plugin_package::{GrantRef, HostPattern, Manifest, Permission, ProgramRef};
use paddock_plugin_api::tools::{PathAccess, PathKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What the user granted one plugin.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grants {
    /// The permissions allowed (the app shows each manifest request and the user ticks).
    pub permissions: Vec<Permission>,
    /// For each named grant, the folder the user chose for it.
    pub folders: BTreeMap<String, PathBuf>,
}

impl Grants {
    /// Nothing granted.
    pub fn none() -> Grants {
        Grants::default()
    }

    /// Allows `permission` (only in force when the manifest asks for it too).
    pub fn allow(mut self, permission: Permission) -> Grants {
        if !self.permissions.contains(&permission) {
            self.permissions.push(permission);
        }
        self
    }

    /// Maps the grant name `name` to the folder `path` the user chose.
    pub fn folder(mut self, name: &str, path: impl Into<PathBuf>) -> Grants {
        self.folders.insert(name.into(), path.into());
        self
    }

    /// The permissions in force: asked by the manifest **and** granted.
    pub fn effective<'a>(&'a self, manifest: &'a Manifest) -> impl Iterator<Item = &'a Permission> + 'a {
        manifest.permissions.iter().map(|r| &r.permission).filter(|p| self.permissions.contains(p))
    }

    fn folders_of<'a>(&'a self, names: &'a [GrantRef]) -> impl Iterator<Item = &'a PathBuf> + 'a {
        names.iter().filter_map(|n| self.folders.get(n.as_str()))
    }

    /// The folders the plugin may read (including those it may write into).
    pub fn readable(&self, manifest: &Manifest) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for p in self.effective(manifest) {
            if let Permission::ReadFolders(n) | Permission::WriteFolders(n) = p {
                out.extend(self.folders_of(n).cloned());
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// The folders the plugin may write into.
    pub fn writable(&self, manifest: &Manifest) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for p in self.effective(manifest) {
            if let Permission::WriteFolders(n) = p {
                out.extend(self.folders_of(n).cloned());
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// Whether the elevated operation `op` is allowed.
    pub fn may_elevate(&self, manifest: &Manifest, op: &str) -> bool {
        self.effective(manifest).any(|p| matches!(p, Permission::Elevated(ops) if ops.iter().any(|o| o == op)))
    }

    /// The path rule for tool arguments: an existing path, resolved (links followed, so a link cannot lead out of a grant),
    /// inside a granted folder (a folder for [`PathKind::Dir`] may be the granted folder itself). Writing needs a write grant;
    /// a file to be created is judged by its folder.
    pub fn path_policy<'a>(&'a self, manifest: &'a Manifest) -> impl Fn(&str, PathKind, PathAccess) -> Result<PathBuf, String> + 'a {
        move |path, kind, access| {
            let resolved = resolve(Path::new(path), access == PathAccess::Write && kind == PathKind::File)
                .map_err(|e| format!("\"{path}\" cannot be used ({e})"))?;
            let roots = if access == PathAccess::Write { self.writable(manifest) } else { self.readable(manifest) };
            let inside = roots.iter().filter_map(|r| std::fs::canonicalize(r).ok()).any(|r| resolved.starts_with(&r));
            if inside {
                Ok(resolved)
            } else {
                Err(format!(
                    "\"{path}\" is not inside a folder you allowed this plugin to {}; grant a folder first",
                    if access == PathAccess::Write { "write into" } else { "read" }
                ))
            }
        }
    }
}

/// Resolves `path` to an absolute path with links followed. A path that does not exist yet (`allow_missing`) is resolved by
/// its folder.
pub fn resolve(path: &Path, allow_missing: bool) -> std::io::Result<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(p) => Ok(p),
        Err(e) if allow_missing && e.kind() == std::io::ErrorKind::NotFound => {
            let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
                return Err(e);
            };
            Ok(std::fs::canonicalize(dir)?.join(name))
        }
        Err(e) => Err(e),
    }
}

/// Memory a plugin gets when it asks for no more (1 GiB).
pub const DEFAULT_MEMORY: u64 = 1 << 30;
/// The least memory a plugin is given (64 MiB).
pub const MIN_MEMORY: u64 = 64 << 20;
/// The most memory a plugin can be given (4 GiB), whatever it asks.
pub const MAX_MEMORY: u64 = 4 << 30;

/// The restrictions a plugin process runs under, as data. `paddock-plugin-host` computes it (a pure function, a tested
/// table); **enforcing** the file, network and program parts is the OS sandbox of the app (AppContainer, landlock and
/// seccomp, `sandbox-exec`), which stays in the app for now and receives this value in the child's bootstrap
/// (`Bootstrap::extra`). What the host enforces itself: the memory cap (`paddock-proc`), the grant-checked file reads and
/// tool paths, the network and program lists as the only ones the app brokers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxSpec {
    /// Folders it may read.
    pub read: Vec<PathBuf>,
    /// Folders it may write into.
    pub write: Vec<PathBuf>,
    /// Hosts it may connect to (empty: no network).
    pub network: Vec<String>,
    /// Programs it may start by name (empty: none).
    pub programs: Vec<String>,
    /// Whether it may start child processes at all.
    pub children: bool,
    /// Run at low integrity (Windows) / the strictest profile elsewhere; off when it may write folders or start programs.
    pub low_integrity: bool,
    /// Memory cap in bytes.
    pub memory_limit: u64,
    /// Share of the processor budget in percent, when capped.
    pub cpu_percent: Option<u8>,
    /// Elevated operations the user allowed.
    pub elevated_ops: Vec<String>,
}

impl SandboxSpec {
    /// Nothing allowed: no files, no network, no programs, low integrity, the default memory cap.
    pub fn locked() -> SandboxSpec {
        SandboxSpec {
            read: Vec::new(),
            write: Vec::new(),
            network: Vec::new(),
            programs: Vec::new(),
            children: false,
            low_integrity: true,
            memory_limit: DEFAULT_MEMORY,
            cpu_percent: None,
            elevated_ops: Vec::new(),
        }
    }
}

/// The sandbox of `manifest`'s plugin under `grants`: [`SandboxSpec::locked`], widened only by what was asked **and**
/// granted. A grant name the user has not mapped to a folder adds nothing.
pub fn sandbox_for(manifest: &Manifest, grants: &Grants) -> SandboxSpec {
    let mut s = SandboxSpec::locked();
    for p in grants.effective(manifest) {
        match p {
            Permission::RunPrograms(programs) => {
                s.children = true;
                s.low_integrity = false;
                s.programs.extend(programs.iter().map(ProgramRef::to_string));
            }
            Permission::Network(hosts) => s.network.extend(hosts.iter().map(HostPattern::to_string)),
            Permission::Elevated(ops) => s.elevated_ops.extend(ops.iter().cloned()),
            Permission::Memory { mib } => s.memory_limit = (u64::from(*mib) << 20).clamp(MIN_MEMORY, MAX_MEMORY),
            Permission::Cpu { percent } => s.cpu_percent = Some((*percent).clamp(1, 100)).filter(|p| *p < 100),
            Permission::OpenedFilePaths | Permission::McpExpose | Permission::ReadFolders(_) | Permission::WriteFolders(_) => {}
        }
    }
    s.read = grants.readable(manifest);
    s.write = grants.writable(manifest);
    if !s.write.is_empty() {
        s.low_integrity = false;
    }
    s.network.sort();
    s.network.dedup();
    s.programs.sort();
    s.programs.dedup();
    s
}
