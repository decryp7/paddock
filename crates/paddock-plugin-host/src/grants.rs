//! Permission grants: what the user allowed each plugin (default deny), and the pure mapping from a manifest and its grants
//! to a [`SandboxSpec`].
//!
//! A manifest *asks* ([`paddock_plugin_package::Permission`]); only what the user *granted* and the manifest also asked for
//! is in force. Folders are named grants (`GrantRef`): the user maps each name to a folder through the app's dialog, and a
//! plugin never sees a raw path it was not given.

use paddock_plugin_api::tools::{PathAccess, PathKind};
use paddock_plugin_package::{GrantRef, HostPattern, Manifest, Permission, ProgramRef};
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
    pub fn effective<'a>(
        &'a self,
        manifest: &'a Manifest,
    ) -> impl Iterator<Item = &'a Permission> + 'a {
        manifest
            .permissions
            .iter()
            .map(|r| &r.permission)
            .filter(|p| self.permissions.contains(p))
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
        self.effective(manifest)
            .any(|p| matches!(p, Permission::Elevated(ops) if ops.iter().any(|o| o == op)))
    }

    /// The path rule for tool arguments: an existing path, resolved (links followed, so a link cannot lead out of a grant),
    /// inside a granted folder (a folder for [`PathKind::Dir`] may be the granted folder itself). Writing needs a write grant;
    /// a file to be created is judged by its folder.
    pub fn path_policy<'a>(
        &'a self,
        manifest: &'a Manifest,
    ) -> impl Fn(&str, PathKind, PathAccess) -> Result<PathBuf, String> + 'a {
        move |path, kind, access| {
            let resolved = resolve(
                Path::new(path),
                access == PathAccess::Write && kind == PathKind::File,
            )
            .map_err(|e| format!("\"{path}\" cannot be used ({e})"))?;
            let roots = if access == PathAccess::Write {
                self.writable(manifest)
            } else {
                self.readable(manifest)
            };
            let inside = roots
                .iter()
                .filter_map(|r| paddock_env::winpath::real_verbatim(r).ok())
                .any(|r| resolved.starts_with(&r));
            if inside {
                Ok(resolved)
            } else {
                Err(format!(
                    "\"{path}\" is not inside a folder you allowed this plugin to {}; grant a folder first",
                    if access == PathAccess::Write {
                        "write into"
                    } else {
                        "read"
                    }
                ))
            }
        }
    }
}

/// Resolves `path` to an absolute path with links followed. A path that does not exist yet (`allow_missing`) is resolved by
/// its folder.
pub fn resolve(path: &Path, allow_missing: bool) -> std::io::Result<PathBuf> {
    match paddock_env::winpath::real_verbatim(path) {
        Ok(p) => Ok(p),
        Err(e) if allow_missing && e.kind() == std::io::ErrorKind::NotFound => {
            let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
                return Err(e);
            };
            Ok(paddock_env::winpath::real_verbatim(dir)?.join(name))
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
                s.programs
                    .extend(programs.iter().map(ProgramRef::to_string));
            }
            Permission::Network(hosts) => {
                s.network.extend(hosts.iter().map(HostPattern::to_string))
            }
            Permission::Elevated(ops) => s.elevated_ops.extend(ops.iter().cloned()),
            Permission::Memory { mib } => {
                s.memory_limit = (u64::from(*mib) << 20).clamp(MIN_MEMORY, MAX_MEMORY)
            }
            Permission::Cpu { percent } => {
                s.cpu_percent = Some((*percent).clamp(1, 100)).filter(|p| *p < 100)
            }
            Permission::OpenedFilePaths
            | Permission::McpExpose
            | Permission::ReadFolders(_)
            | Permission::WriteFolders(_) => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(permissions: &str) -> Manifest {
        Manifest::parse(&format!(
            "format = 1\n[plugin]\nid = \"net.example.p\"\nname = \"P\"\nversion = \"1.0.0\"\npublisher = \"E\"\nprotocol = \"2.0\"\nbinary = \"p\"\ntargets = [\"x86_64-unknown-linux-gnu\"]\n{permissions}"
        ))
        .unwrap()
    }

    const ALL: &str = r#"
[[permissions]]
permission = "readFolders"
grants = ["docs"]
reason = "r"
[[permissions]]
permission = "writeFolders"
grants = ["out", "unmapped"]
reason = "r"
[[permissions]]
permission = "runPrograms"
programs = ["relog"]
reason = "r"
[[permissions]]
permission = "network"
hosts = ["b.example.com", "a.example.com"]
reason = "r"
[[permissions]]
permission = "elevated"
ops = ["collect"]
reason = "r"
[[permissions]]
permission = "memory"
mib = 65536
reason = "r"
[[permissions]]
permission = "cpu"
percent = 40
reason = "r"
[[permissions]]
permission = "openedFilePaths"
reason = "r"
"#;

    fn granted_all(m: &Manifest) -> Grants {
        let mut g = Grants::none().folder("docs", "/docs").folder("out", "/out");
        for r in &m.permissions {
            g = g.allow(r.permission.clone());
        }
        g
    }

    #[test]
    fn nothing_granted_is_the_locked_sandbox() {
        let m = manifest(ALL);
        let s = sandbox_for(&m, &Grants::none());
        assert_eq!(s, SandboxSpec::locked());
        assert!(
            s.low_integrity
                && !s.children
                && s.read.is_empty()
                && s.write.is_empty()
                && s.network.is_empty()
        );
        assert_eq!(s.memory_limit, DEFAULT_MEMORY);
    }

    #[test]
    fn what_is_asked_and_granted_widens_the_sandbox_and_only_that() {
        let m = manifest(ALL);
        let s = sandbox_for(&m, &granted_all(&m));
        assert_eq!(s.read, [PathBuf::from("/docs"), PathBuf::from("/out")]);
        assert_eq!(s.write, [PathBuf::from("/out")]);
        assert_eq!(
            (s.programs.clone(), s.network.clone()),
            (
                vec!["relog".to_string()],
                vec!["a.example.com".to_string(), "b.example.com".to_string()]
            )
        );
        assert_eq!(s.elevated_ops, ["collect"]);
        assert!(s.children && !s.low_integrity);
        // 65536 MiB is clamped to the most a plugin can have.
        assert_eq!(s.memory_limit, MAX_MEMORY);
        assert_eq!(s.cpu_percent, Some(40));
        // Granted but not asked for: nothing changes.
        let none = manifest("");
        assert_eq!(sandbox_for(&none, &granted_all(&m)), SandboxSpec::locked());
        // Asked but not granted: still locked, per permission.
        let only_net = Grants::none().allow(m.permissions[3].permission.clone());
        let n = sandbox_for(&m, &only_net);
        assert_eq!(n.network.len(), 2);
        assert!(n.programs.is_empty() && n.read.is_empty() && n.low_integrity);
    }

    #[test]
    fn limits_are_clamped_and_a_full_cpu_share_is_no_cap() {
        let m = manifest(
            "[[permissions]]\npermission = \"memory\"\nmib = 16\nreason = \"r\"\n[[permissions]]\npermission = \"cpu\"\npercent = 100\nreason = \"r\"\n",
        );
        let s = sandbox_for(&m, &granted_all(&m));
        assert_eq!((s.memory_limit, s.cpu_percent), (MIN_MEMORY, None));
        let low = manifest("[[permissions]]\npermission = \"cpu\"\npercent = 1\nreason = \"r\"\n");
        assert_eq!(sandbox_for(&low, &granted_all(&low)).cpu_percent, Some(1));
    }

    #[test]
    fn an_unmapped_grant_name_adds_no_folder_and_write_clears_low_integrity() {
        let m = manifest(
            "[[permissions]]\npermission = \"writeFolders\"\ngrants = [\"out\", \"unmapped\"]\nreason = \"r\"\n",
        );
        let g = Grants::none()
            .allow(m.permissions[0].permission.clone())
            .folder("out", "/out");
        let s = sandbox_for(&m, &g);
        assert_eq!(s.write, [PathBuf::from("/out")]);
        assert!(!s.low_integrity && !s.children);
        let unmapped = Grants::none().allow(m.permissions[0].permission.clone());
        assert!(
            sandbox_for(&m, &unmapped).write.is_empty() && sandbox_for(&m, &unmapped).low_integrity
        );
    }

    #[test]
    fn grants_report_elevation_by_name_and_do_not_duplicate() {
        let m = manifest(ALL);
        let g = granted_all(&m);
        assert!(g.may_elevate(&m, "collect") && !g.may_elevate(&m, "other"));
        assert!(!Grants::none().may_elevate(&m, "collect"));
        let twice = Grants::none()
            .allow(Permission::McpExpose)
            .allow(Permission::McpExpose);
        assert_eq!(twice.permissions.len(), 1);
        let back: Grants = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
        assert_eq!(back, g);
    }

    #[test]
    fn tool_paths_must_be_inside_a_granted_folder_after_links_are_followed() {
        let dir = tempfile::tempdir().unwrap();
        let (docs, out, secret) = (
            dir.path().join("docs"),
            dir.path().join("out"),
            dir.path().join("secret"),
        );
        for d in [&docs, &out, &secret] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(docs.join("a.txt"), "a").unwrap();
        std::fs::write(secret.join("s.txt"), "s").unwrap();
        let m = manifest(ALL);
        let g = Grants::none()
            .folder("docs", &docs)
            .folder("out", &out)
            .allow(m.permissions[0].permission.clone())
            .allow(m.permissions[1].permission.clone());
        let policy = g.path_policy(&m);
        let ok = policy(
            docs.join("a.txt").to_str().unwrap(),
            PathKind::File,
            PathAccess::Read,
        )
        .unwrap();
        assert_eq!(
            ok,
            paddock_env::winpath::real_verbatim(&docs.join("a.txt")).unwrap()
        );
        // A folder itself is inside its grant; a file in a folder that is only readable is not writable.
        policy(docs.to_str().unwrap(), PathKind::Dir, PathAccess::Read).unwrap();
        let no_write = policy(
            docs.join("a.txt").to_str().unwrap(),
            PathKind::File,
            PathAccess::Write,
        )
        .unwrap_err();
        assert!(no_write.contains("allowed this plugin to write into"));
        // A file to create is judged by its folder.
        let created = policy(
            out.join("new.txt").to_str().unwrap(),
            PathKind::File,
            PathAccess::Write,
        )
        .unwrap();
        assert!(created.ends_with("new.txt"));
        // Outside the grants, missing, and a missing folder.
        let outside = policy(
            secret.join("s.txt").to_str().unwrap(),
            PathKind::File,
            PathAccess::Read,
        )
        .unwrap_err();
        assert!(outside.contains("not inside a folder you allowed this plugin to read"));
        assert!(
            policy(
                docs.join("missing.txt").to_str().unwrap(),
                PathKind::File,
                PathAccess::Read
            )
            .unwrap_err()
            .contains("cannot be used")
        );
        assert!(
            policy(
                out.join("nodir/new.txt").to_str().unwrap(),
                PathKind::File,
                PathAccess::Write
            )
            .unwrap_err()
            .contains("cannot be used")
        );
        assert!(policy("/", PathKind::File, PathAccess::Write).is_err());
        // `..` does not get out.
        let dotdot = format!("{}/../secret/s.txt", docs.display());
        assert!(policy(&dotdot, PathKind::File, PathAccess::Read).is_err());
        // A link inside a grant that leads out of it is followed, so it leads out.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&secret, docs.join("link")).unwrap();
            let via_link = docs.join("link").join("s.txt");
            assert!(policy(via_link.to_str().unwrap(), PathKind::File, PathAccess::Read).is_err());
        }
    }

    #[test]
    fn resolve_handles_missing_files_by_their_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve(&dir.path().join("x"), false).is_err());
        assert!(resolve(&dir.path().join("x"), true).unwrap().ends_with("x"));
        assert!(resolve(Path::new("/"), true).is_ok());
        assert!(resolve(Path::new(""), true).is_err());
    }
}
