//! `manifest.toml`: what a plugin is (id, name, version, publisher), which platform protocol it was built for, what it
//! contributes (tools, settings, views, MCP tools and the app contract namespaces it speaks), what it may touch
//! (permissions, default deny) and which program it carries for each target. Parsed once into a [`Manifest`] whose values
//! are all valid ([`Manifest::check`] holds every rule, so a manifest received over the IPC is checked by the same code).
//!
//! The model types are plain serde types without `untagged`, `flatten`, internal tags or `skip_serializing_if`, so they
//! cross the postcard IPC and JSON unchanged; `manifest.toml` has its own friendlier shape (below) that
//! [`Manifest::parse`] converts. Unknown keys are **refused** (`deny_unknown_fields`, as in Debrief's package): a plugin
//! built against a newer manifest format gets "unknown field `x`" instead of silently losing a permission or a
//! contribution, and the `format` number is the way to add keys.
//!
//! ```toml
//! format = 1
//!
//! [plugin]
//! id = "com.example.kv-lines"
//! name = "Key-value lines"
//! version = "1.2.0"
//! publisher = "Example Ltd"
//! description = "Reads key=value logs."
//! protocol = "2.0"                   # the platform protocol (ADR 0001: PLATFORM_PROTOCOL)
//! binary = "kv-lines"                # bin/<target>/kv-lines[.exe]
//! targets = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"]
//! key = "3f2a9c0d1e7b4a55"           # the signing key's id; omitted for an unsigned package
//!
//! [[contributions]]
//! kind = "extension"                 # tool, settings, view, mcpTool or extension
//! ns = "debrief.import/3"            # the app contract and the minor version it speaks
//!
//! [[contributions]]
//! kind = "tool"
//! id = "collector"
//! actions = [{ id = "start", writes = true }, { id = "status" }]   # writes: true is badged and never exposed over MCP
//!
//! [[permissions]]
//! permission = "network"             # openedFilePaths, readFolders, writeFolders, runPrograms, network, elevated,
//! hosts = ["*.example.com"]          # mcpExpose, memory, cpu; each takes grants / programs / hosts / ops / mib / percent
//! reason = "Looks up host names."
//! ```

use crate::PackageError;
use crate::names::{GrantRef, HostPattern, Namespace, PluginId, ProgramRef, valid_ident};
use crate::version::ProtocolVersion;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The manifest format this crate reads and writes.
pub const FORMAT: u32 = 1;
/// Largest `manifest.toml`.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// Most targets one package may carry.
pub const MAX_TARGETS: usize = 16;
/// Most contributions one plugin may declare.
pub const MAX_CONTRIBUTIONS: usize = 256;
/// Most actions one tool or view may declare.
pub const MAX_ACTIONS: usize = 64;
/// Most entries in one permission's list (grants, programs, hosts, operations).
pub const MAX_LIST: usize = 64;
/// Most memory a plugin may ask for (MiB).
pub const MAX_MEMORY_MIB: u32 = 65_536;
/// Least memory a plugin may ask for (MiB).
pub const MIN_MEMORY_MIB: u32 = 16;
/// Longest display name, publisher or reason (characters).
pub const MAX_TEXT: usize = 120;
/// Longest description (characters).
pub const MAX_DESCRIPTION: usize = 2000;

/// An operation of a tool or an action of a view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    /// Its id, unique within the tool or view.
    pub id: String,
    /// It changes something (ADR 0002): the UI badges it, and it is never exposed over MCP.
    pub writes: bool,
}

/// What a plugin adds to the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Contribution {
    /// A tool with operations.
    Tool {
        /// The tool's id.
        id: String,
        /// Its operations.
        actions: Vec<Action>,
    },
    /// A settings section.
    Settings {
        /// The section's id.
        id: String,
    },
    /// A declarative view (ADR 0002) with actions.
    View {
        /// The view's id.
        id: String,
        /// Its actions.
        actions: Vec<Action>,
    },
    /// A read-only MCP tool (writes are UI/CLI only, so there is nothing to mark).
    McpTool {
        /// The tool's id.
        id: String,
    },
    /// An app contract the plugin speaks (`debrief.import/3`).
    Extension(Namespace),
}

impl Contribution {
    /// A stable key for the Plugins view and duplicate checks (`tool:collector`, `extension:debrief.import`).
    pub fn key(&self) -> String {
        match self {
            Contribution::Tool { id, .. } => format!("tool:{id}"),
            Contribution::Settings { id } => format!("settings:{id}"),
            Contribution::View { id, .. } => format!("view:{id}"),
            Contribution::McpTool { id } => format!("mcp-tool:{id}"),
            Contribution::Extension(ns) => format!("extension:{}", ns.ns()),
        }
    }

    fn actions(&self) -> &[Action] {
        match self {
            Contribution::Tool { actions, .. } | Contribution::View { actions, .. } => actions,
            _ => &[],
        }
    }
}

/// A right a plugin needs beyond computing on what the host hands it. The user grants each per plugin; the default is deny.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permission {
    /// Receive the paths of files the user opened (not just host-file ranges).
    OpenedFilePaths,
    /// Read folders the user chose, by grant name, never by raw path.
    ReadFolders(Vec<GrantRef>),
    /// Write into folders the user chose.
    WriteFolders(Vec<GrantRef>),
    /// Start these programs (by name).
    RunPrograms(Vec<ProgramRef>),
    /// Connect to these hosts only (loopback to Pitwall goes through the host, not the plugin).
    Network(Vec<HostPattern>),
    /// Run these named operations elevated (ADR 0017 section 7); nothing else runs elevated.
    Elevated(Vec<String>),
    /// Contribute MCP tools (read-only tools only).
    McpExpose,
    /// Memory limit the plugin needs, in MiB.
    Memory {
        /// MiB.
        mib: u32,
    },
    /// CPU share the plugin may use, in percent of the app's processor budget (1 to 100).
    Cpu {
        /// Percent.
        percent: u8,
    },
}

impl Permission {
    /// The permission's name in `manifest.toml` and the UI (`readFolders`).
    pub fn kind(&self) -> &'static str {
        match self {
            Permission::OpenedFilePaths => "openedFilePaths",
            Permission::ReadFolders(_) => "readFolders",
            Permission::WriteFolders(_) => "writeFolders",
            Permission::RunPrograms(_) => "runPrograms",
            Permission::Network(_) => "network",
            Permission::Elevated(_) => "elevated",
            Permission::McpExpose => "mcpExpose",
            Permission::Memory { .. } => "memory",
            Permission::Cpu { .. } => "cpu",
        }
    }

    /// What the user is asked to allow, in words.
    pub fn describe(&self) -> String {
        let list = |items: Vec<&str>| items.join(", ");
        match self {
            Permission::OpenedFilePaths => "read the files you open directly".into(),
            Permission::ReadFolders(g) => format!(
                "read (never write) the folders {}",
                list(g.iter().map(GrantRef::as_str).collect())
            ),
            Permission::WriteFolders(g) => format!(
                "write into the folders {}",
                list(g.iter().map(GrantRef::as_str).collect())
            ),
            Permission::RunPrograms(p) => format!(
                "run the programs {}",
                list(p.iter().map(ProgramRef::as_str).collect())
            ),
            Permission::Network(h) => format!(
                "connect to {}",
                list(h.iter().map(HostPattern::as_str).collect())
            ),
            Permission::Elevated(o) => format!(
                "run the operations {} with administrator rights",
                list(o.iter().map(String::as_str).collect())
            ),
            Permission::McpExpose => "offer read-only tools to AI assistants over MCP".into(),
            Permission::Memory { mib } => format!("use up to {mib} MiB of memory"),
            Permission::Cpu { percent } => format!("use up to {percent}% of the processor budget"),
        }
    }
}

/// A permission and why the plugin needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequest {
    /// The permission requested.
    pub permission: Permission,
    /// Why the plugin needs it, shown to the user when they decide.
    pub reason: String,
}

/// A valid plugin manifest: identity, protocol, contributions, permissions and programs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Reverse-DNS id, unique among plugins.
    pub id: PluginId,
    /// Display name.
    pub name: String,
    /// The plugin's version.
    pub version: semver::Version,
    /// Who publishes it, as shown to the user.
    pub publisher: String,
    /// What it does, as shown to the user before installing.
    pub description: String,
    /// The platform protocol its program was built for.
    pub protocol: ProtocolVersion,
    /// The program's file name without extension (`kv-lines`; `kv-lines.exe` on Windows targets).
    pub binary: String,
    /// The target triples it carries a program for, in the manifest's order.
    pub targets: Vec<String>,
    /// The id of the key that signs it ([`crate::PublicKey::id`]); `None` for an unsigned package.
    pub key: Option<String>,
    /// What it adds to the app.
    pub contributions: Vec<Contribution>,
    /// The rights it asks for, with the publisher's reasons.
    pub permissions: Vec<PermissionRequest>,
}

/// The TOML shape of `manifest.toml`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    format: u32,
    plugin: RawPlugin,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    contributions: Vec<RawContribution>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    permissions: Vec<RawPermission>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPlugin {
    id: String,
    name: String,
    version: String,
    publisher: String,
    #[serde(default)]
    description: String,
    protocol: String,
    binary: String,
    targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContribution {
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ns: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<RawAction>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAction {
    id: String,
    #[serde(default)]
    writes: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPermission {
    permission: String,
    reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    grants: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    programs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hosts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    ops: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mib: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    percent: Option<u8>,
}

fn bad(m: impl Into<String>) -> PackageError {
    PackageError::InvalidManifest(m.into())
}

/// Whether `s` has 1–`max` characters, none of them control characters, and is not blank.
fn text_ok(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
}

/// Whether `t` looks like a target triple (`x86_64-pc-windows-msvc`): 3–64 characters from `a–z 0–9 _ . -`, at least two
/// dashes.
pub fn valid_target(t: &str) -> bool {
    (3..=64).contains(&t.len())
        && t.matches('-').count() >= 2
        && t.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_.-".contains(c))
}

/// Whether `b` is a valid program name: 1–64 characters from `a–z A–Z 0–9 _ -`.
pub fn valid_binary(b: &str) -> bool {
    (1..=64).contains(&b.len())
        && b.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
}

/// Whether a key id is 16 lowercase hexadecimal digits.
pub fn valid_key_id(k: &str) -> bool {
    k.len() == 16
        && k.chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

fn names<T, F: Fn(String) -> Result<T, String>>(
    items: &[String],
    make: F,
) -> Result<Vec<T>, PackageError> {
    items.iter().map(|s| make(s.clone()).map_err(bad)).collect()
}

impl RawPermission {
    fn into_permission(self) -> Result<PermissionRequest, PackageError> {
        let kind = self.permission.as_str();
        let present = [
            ("grants", !self.grants.is_empty()),
            ("programs", !self.programs.is_empty()),
            ("hosts", !self.hosts.is_empty()),
            ("ops", !self.ops.is_empty()),
            ("mib", self.mib.is_some()),
            ("percent", self.percent.is_some()),
        ];
        let wants = match kind {
            "readFolders" | "writeFolders" => "grants",
            "runPrograms" => "programs",
            "network" => "hosts",
            "elevated" => "ops",
            "memory" => "mib",
            "cpu" => "percent",
            "openedFilePaths" | "mcpExpose" => "",
            other => {
                return Err(bad(format!(
                    "permission \"{other}\" is not known: use openedFilePaths, readFolders, writeFolders, runPrograms, network, elevated, mcpExpose, memory or cpu"
                )));
            }
        };
        for (field, given) in present {
            if given && field != wants {
                return Err(bad(if wants.is_empty() {
                    format!("permission {kind} takes no `{field}`")
                } else {
                    format!("permission {kind} takes `{wants}`, not `{field}`")
                }));
            }
            if !given && field == wants {
                return Err(bad(format!("permission {kind} needs `{wants}`")));
            }
        }
        let permission = match kind {
            "readFolders" => Permission::ReadFolders(names(&self.grants, GrantRef::new)?),
            "writeFolders" => Permission::WriteFolders(names(&self.grants, GrantRef::new)?),
            "runPrograms" => Permission::RunPrograms(names(&self.programs, ProgramRef::new)?),
            "network" => Permission::Network(names(&self.hosts, HostPattern::new)?),
            "elevated" => Permission::Elevated(self.ops.clone()),
            "memory" => Permission::Memory {
                mib: self.mib.unwrap_or_default(),
            },
            "cpu" => Permission::Cpu {
                percent: self.percent.unwrap_or_default(),
            },
            "openedFilePaths" => Permission::OpenedFilePaths,
            _ => Permission::McpExpose,
        };
        Ok(PermissionRequest {
            permission,
            reason: self.reason,
        })
    }

    fn from_permission(r: &PermissionRequest) -> RawPermission {
        let strings = |v: Vec<&str>| v.into_iter().map(String::from).collect::<Vec<_>>();
        let mut raw = RawPermission {
            permission: r.permission.kind().into(),
            reason: r.reason.clone(),
            grants: vec![],
            programs: vec![],
            hosts: vec![],
            ops: vec![],
            mib: None,
            percent: None,
        };
        match &r.permission {
            Permission::ReadFolders(g) | Permission::WriteFolders(g) => {
                raw.grants = strings(g.iter().map(GrantRef::as_str).collect())
            }
            Permission::RunPrograms(p) => {
                raw.programs = strings(p.iter().map(ProgramRef::as_str).collect())
            }
            Permission::Network(h) => {
                raw.hosts = strings(h.iter().map(HostPattern::as_str).collect())
            }
            Permission::Elevated(o) => raw.ops = o.clone(),
            Permission::Memory { mib } => raw.mib = Some(*mib),
            Permission::Cpu { percent } => raw.percent = Some(*percent),
            Permission::OpenedFilePaths | Permission::McpExpose => {}
        }
        raw
    }
}

impl RawContribution {
    fn into_contribution(self) -> Result<Contribution, PackageError> {
        let kind = self.kind.clone();
        let id = |c: &RawContribution| {
            c.id.clone()
                .ok_or_else(|| bad(format!("contribution kind \"{kind}\" needs an `id`")))
        };
        let no = |field: &str, given: bool| {
            if given {
                Err(bad(format!(
                    "contribution kind \"{kind}\" takes no `{field}`"
                )))
            } else {
                Ok(())
            }
        };
        let actions = |c: &RawContribution| -> Vec<Action> {
            c.actions
                .iter()
                .map(|a| Action {
                    id: a.id.clone(),
                    writes: a.writes,
                })
                .collect()
        };
        match kind.as_str() {
            "tool" | "view" => {
                no("ns", self.ns.is_some())?;
                let (id, actions) = (id(&self)?, actions(&self));
                Ok(if kind == "tool" {
                    Contribution::Tool { id, actions }
                } else {
                    Contribution::View { id, actions }
                })
            }
            "settings" | "mcpTool" => {
                no("ns", self.ns.is_some())?;
                no("actions", !self.actions.is_empty())?;
                let id = id(&self)?;
                Ok(if kind == "settings" {
                    Contribution::Settings { id }
                } else {
                    Contribution::McpTool { id }
                })
            }
            "extension" => {
                no("id", self.id.is_some())?;
                no("actions", !self.actions.is_empty())?;
                let ns = self.ns.as_deref().ok_or_else(|| {
                    bad("contribution kind \"extension\" needs an `ns` like debrief.import/3")
                })?;
                Ok(Contribution::Extension(ns.parse().map_err(bad)?))
            }
            other => Err(bad(format!(
                "contribution kind \"{other}\" is not known: use tool, settings, view, mcpTool or extension"
            ))),
        }
    }

    fn from_contribution(c: &Contribution) -> RawContribution {
        let raw = |kind: &str, id: Option<&String>, ns: Option<String>| RawContribution {
            kind: kind.into(),
            id: id.cloned(),
            ns,
            actions: c
                .actions()
                .iter()
                .map(|a| RawAction {
                    id: a.id.clone(),
                    writes: a.writes,
                })
                .collect(),
        };
        match c {
            Contribution::Tool { id, .. } => raw("tool", Some(id), None),
            Contribution::View { id, .. } => raw("view", Some(id), None),
            Contribution::Settings { id } => raw("settings", Some(id), None),
            Contribution::McpTool { id } => raw("mcpTool", Some(id), None),
            Contribution::Extension(ns) => raw("extension", None, Some(ns.to_string())),
        }
    }
}

impl Manifest {
    /// Parses and validates `manifest.toml` (size, shape, every rule of [`Manifest::check`]).
    pub fn parse(text: &str) -> Result<Manifest, PackageError> {
        if text.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(PackageError::TooLarge {
                what: "manifest.toml".into(),
                limit: MAX_MANIFEST_BYTES,
            });
        }
        let raw: Raw = toml::from_str(text).map_err(|e| bad(e.message().to_string()))?;
        if raw.format != FORMAT {
            return Err(bad(format!(
                "format {} is not supported (this version reads format {FORMAT}); rebuild the package with a current cargo-paddock-plugin",
                raw.format
            )));
        }
        let p = raw.plugin;
        let id = PluginId::new(p.id).map_err(bad)?;
        let version = semver::Version::parse(&p.version)
            .map_err(|e| bad(format!("plugin.version \"{}\": {e}", p.version)))?;
        let protocol: ProtocolVersion = p
            .protocol
            .parse()
            .map_err(|e: String| bad(format!("plugin.protocol: {e}")))?;
        if raw.contributions.len() > MAX_CONTRIBUTIONS {
            return Err(bad(format!(
                "at most {MAX_CONTRIBUTIONS} contributions can be declared"
            )));
        }
        let contributions = raw
            .contributions
            .into_iter()
            .map(RawContribution::into_contribution)
            .collect::<Result<Vec<_>, _>>()?;
        if raw.permissions.len() > 16 {
            return Err(bad(
                "too many permissions: each of the nine kinds may be listed once",
            ));
        }
        let permissions = raw
            .permissions
            .into_iter()
            .map(RawPermission::into_permission)
            .collect::<Result<Vec<_>, _>>()?;
        let manifest = Manifest {
            id,
            name: p.name,
            version,
            publisher: p.publisher,
            description: p.description,
            protocol,
            binary: p.binary,
            targets: p.targets,
            key: p.key,
            contributions,
            permissions,
        };
        manifest.check()?;
        Ok(manifest)
    }

    /// Checks every rule the host relies on: texts, targets, key id, contribution ids and actions, permission lists and
    /// ranges. [`Manifest::parse`] calls it; a host calls it on a manifest it received over the IPC.
    pub fn check(&self) -> Result<(), PackageError> {
        if !text_ok(&self.name, 80) {
            return Err(bad("plugin.name must have 1-80 characters"));
        }
        if !text_ok(&self.publisher, MAX_TEXT) {
            return Err(bad(format!(
                "plugin.publisher must have 1-{MAX_TEXT} characters"
            )));
        }
        if self.description.chars().count() > MAX_DESCRIPTION
            || self
                .description
                .chars()
                .any(|c| c.is_control() && c != '\n')
        {
            return Err(bad(format!(
                "plugin.description may have at most {MAX_DESCRIPTION} characters"
            )));
        }
        if !valid_binary(&self.binary) {
            return Err(bad(
                "plugin.binary must have 1-64 characters from A-Z, a-z, 0-9, '_' and '-' (no extension)",
            ));
        }
        if self.targets.is_empty() || self.targets.len() > MAX_TARGETS {
            return Err(bad(format!(
                "plugin.targets must list 1-{MAX_TARGETS} target triples"
            )));
        }
        let mut seen = HashSet::new();
        for t in &self.targets {
            if !valid_target(t) {
                return Err(bad(format!(
                    "plugin.targets: \"{t}\" is not a target triple"
                )));
            }
            if !seen.insert(t.as_str()) {
                return Err(bad(format!("plugin.targets lists \"{t}\" twice")));
            }
        }
        if let Some(k) = &self.key
            && !valid_key_id(k)
        {
            return Err(bad(format!(
                "plugin.key \"{k}\" is not a key id (16 hexadecimal digits)"
            )));
        }
        self.check_contributions()?;
        self.check_permissions()
    }

    fn check_contributions(&self) -> Result<(), PackageError> {
        if self.contributions.len() > MAX_CONTRIBUTIONS {
            return Err(bad(format!(
                "at most {MAX_CONTRIBUTIONS} contributions can be declared"
            )));
        }
        let mut seen = HashSet::new();
        for c in &self.contributions {
            let key = c.key();
            let id = match c {
                Contribution::Tool { id, .. }
                | Contribution::Settings { id }
                | Contribution::View { id, .. }
                | Contribution::McpTool { id } => Some(id),
                Contribution::Extension(_) => None,
            };
            if let Some(id) = id
                && !valid_ident(id)
            {
                return Err(bad(format!(
                    "contribution id \"{}\" is not valid: use 1-64 characters from A-Z, a-z, 0-9, '.', '-', '_'",
                    id.chars().take(40).collect::<String>()
                )));
            }
            if !seen.insert(key.clone()) {
                return Err(bad(format!("it contributes \"{key}\" twice")));
            }
            if c.actions().len() > MAX_ACTIONS {
                return Err(bad(format!(
                    "\"{key}\" has more than {MAX_ACTIONS} actions"
                )));
            }
            let mut actions = HashSet::new();
            for a in c.actions() {
                if !valid_ident(&a.id) || !actions.insert(a.id.as_str()) {
                    return Err(bad(format!(
                        "\"{key}\": action \"{}\" is not a unique id of 1-64 characters from A-Z, a-z, 0-9, '.', '-', '_'",
                        a.id.chars().take(40).collect::<String>()
                    )));
                }
            }
        }
        Ok(())
    }

    fn check_permissions(&self) -> Result<(), PackageError> {
        let mut kinds = HashSet::new();
        for r in &self.permissions {
            let kind = r.permission.kind();
            if !kinds.insert(kind) {
                return Err(bad(format!("permission {kind} is listed twice")));
            }
            if !text_ok(&r.reason, 500) {
                return Err(bad(format!(
                    "permission {kind} needs a reason of 1-500 characters"
                )));
            }
            let (len, unique) = match &r.permission {
                Permission::ReadFolders(g) | Permission::WriteFolders(g) => (g.len(), unique(g)),
                Permission::RunPrograms(p) => (p.len(), unique(p)),
                Permission::Network(h) => (h.len(), unique(h)),
                Permission::Elevated(o) => {
                    if let Some(o) = o.iter().find(|o| !valid_ident(o)) {
                        return Err(bad(format!(
                            "permission elevated: operation \"{}\" is not valid: use 1-64 characters from A-Z, a-z, 0-9, '.', '-', '_'",
                            o.chars().take(40).collect::<String>()
                        )));
                    }
                    (o.len(), unique(o))
                }
                Permission::Memory { mib } => {
                    if !(MIN_MEMORY_MIB..=MAX_MEMORY_MIB).contains(mib) {
                        return Err(bad(format!(
                            "permission memory: mib must be {MIN_MEMORY_MIB}-{MAX_MEMORY_MIB}"
                        )));
                    }
                    continue;
                }
                Permission::Cpu { percent } => {
                    if !(1..=100).contains(percent) {
                        return Err(bad("permission cpu: percent must be 1-100"));
                    }
                    continue;
                }
                Permission::OpenedFilePaths | Permission::McpExpose => continue,
            };
            if len == 0 || len > MAX_LIST {
                return Err(bad(format!(
                    "permission {kind} must list 1-{MAX_LIST} entries"
                )));
            }
            if !unique {
                return Err(bad(format!("permission {kind} lists an entry twice")));
            }
        }
        Ok(())
    }

    /// The manifest as `manifest.toml` text ([`Manifest::parse`] reads it back unchanged). Deterministic.
    pub fn to_toml(&self) -> String {
        let raw = Raw {
            format: FORMAT,
            plugin: RawPlugin {
                id: self.id.to_string(),
                name: self.name.clone(),
                version: self.version.to_string(),
                publisher: self.publisher.clone(),
                description: self.description.clone(),
                protocol: self.protocol.to_string(),
                binary: self.binary.clone(),
                targets: self.targets.clone(),
                key: self.key.clone(),
            },
            contributions: self
                .contributions
                .iter()
                .map(RawContribution::from_contribution)
                .collect(),
            permissions: self
                .permissions
                .iter()
                .map(RawPermission::from_permission)
                .collect(),
        };
        // Plain strings, numbers and arrays of tables always serialise.
        toml::to_string(&raw).unwrap_or_default()
    }

    /// The program's path in the package for `target` (`bin/<target>/<binary>[.exe]`).
    pub fn binary_path(&self, target: &str) -> String {
        format!("bin/{target}/{}", self.binary_file(target))
    }

    /// The program's file name for `target` (with `.exe` on Windows targets).
    pub fn binary_file(&self, target: &str) -> String {
        if target.contains("windows") {
            format!("{}.exe", self.binary)
        } else {
            self.binary.clone()
        }
    }

    /// The permission of `kind` (`network`), when it asks for it.
    pub fn permission(&self, kind: &str) -> Option<&PermissionRequest> {
        self.permissions
            .iter()
            .find(|r| r.permission.kind() == kind)
    }

    /// The app contracts it speaks, in order.
    pub fn namespaces(&self) -> impl Iterator<Item = &Namespace> {
        self.contributions.iter().filter_map(|c| match c {
            Contribution::Extension(ns) => Some(ns),
            _ => None,
        })
    }

    /// The write-capable actions as (contribution key, action id): what the UI badges and MCP never exposes.
    pub fn write_actions(&self) -> Vec<(String, String)> {
        self.contributions
            .iter()
            .flat_map(|c| {
                c.actions()
                    .iter()
                    .filter(|a| a.writes)
                    .map(|a| (c.key(), a.id.clone()))
            })
            .collect()
    }

    /// What the plugin contributes, as the Plugins view lists it (`tool:collector`, `extension:debrief.import`, ...).
    pub fn contribution_keys(&self) -> Vec<String> {
        self.contributions.iter().map(Contribution::key).collect()
    }

    /// Whether `host_name` (speaking platform protocol `host` and the contracts `supported`) can serve the plugin. The error
    /// says who must update: the platform protocol needs the same major and a known minor, and each contract namespace the
    /// plugin speaks must be one the app has, with a minor the app knows.
    pub fn check_compatible(
        &self,
        host: ProtocolVersion,
        supported: &[Namespace],
        host_name: &str,
    ) -> Result<(), PackageError> {
        let label = format!("plugin \"{}\"", self.id);
        if let Some(why) = host.mismatch(self.protocol, &label, host_name) {
            return Err(PackageError::Incompatible(why));
        }
        for ns in self.namespaces() {
            let Some(known) = supported.iter().find(|s| s.ns() == ns.ns()) else {
                return Err(PackageError::Incompatible(format!(
                    "{label} speaks \"{}\", a contract this {host_name} does not have; it was built for another app or a newer {host_name}",
                    ns.ns()
                )));
            };
            let (have, want) = (
                ProtocolVersion::new(0, known.minor),
                ProtocolVersion::new(0, ns.minor),
            );
            if !have.accepts(want) {
                return Err(PackageError::Incompatible(format!(
                    "{label} needs {host_name} to speak {} or newer (it speaks {known}); update {host_name}",
                    ns
                )));
            }
        }
        Ok(())
    }
}

fn unique<T: std::hash::Hash + Eq>(items: &[T]) -> bool {
    let mut seen = HashSet::new();
    items.iter().all(|i| seen.insert(i))
}

/// The target triple of this build: the program a package must carry to run here.
pub fn host_target() -> &'static str {
    target_for(std::env::consts::OS, std::env::consts::ARCH)
}

/// The target triple for an operating system and architecture (`std::env::consts::{OS, ARCH}` names).
pub fn target_for(os: &str, arch: &str) -> &'static str {
    match (os, arch) {
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        _ => "unknown-unknown-unknown",
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const SAMPLE: &str = r#"format = 1

[plugin]
id = "com.example.kv-lines"
name = "Key-value lines"
version = "1.2.0"
publisher = "Example Ltd"
description = "Reads key=value logs."
protocol = "2.0"
binary = "kv-lines"
targets = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"]

[[contributions]]
kind = "extension"
ns = "debrief.import/3"

[[contributions]]
kind = "tool"
id = "collector"
actions = [{ id = "start", writes = true }, { id = "status" }]

[[contributions]]
kind = "settings"
id = "main"

[[contributions]]
kind = "view"
id = "overview"
actions = [{ id = "refresh" }]

[[contributions]]
kind = "mcpTool"
id = "lookup"

[[permissions]]
permission = "network"
hosts = ["*.example.com", "example.org"]
reason = "Looks up host names."
"#;

    fn parse(text: &str) -> Result<Manifest, PackageError> {
        Manifest::parse(text)
    }

    #[test]
    fn a_valid_manifest_parses_and_roundtrips() {
        let m = parse(SAMPLE).unwrap();
        assert_eq!(m.id.as_str(), "com.example.kv-lines");
        assert_eq!(m.version, semver::Version::new(1, 2, 0));
        assert_eq!(m.protocol, ProtocolVersion::new(2, 0));
        assert_eq!(m.contributions.len(), 5);
        assert_eq!(
            m.namespaces().map(ToString::to_string).collect::<Vec<_>>(),
            ["debrief.import/3"]
        );
        assert_eq!(
            m.write_actions(),
            [("tool:collector".to_string(), "start".to_string())]
        );
        assert_eq!(
            m.contribution_keys(),
            [
                "extension:debrief.import",
                "tool:collector",
                "settings:main",
                "view:overview",
                "mcp-tool:lookup"
            ]
        );
        assert!(m.permission("network").is_some() && m.permission("cpu").is_none());
        assert_eq!(
            m.binary_path("x86_64-pc-windows-msvc"),
            "bin/x86_64-pc-windows-msvc/kv-lines.exe"
        );
        assert_eq!(
            m.binary_path("x86_64-unknown-linux-gnu"),
            "bin/x86_64-unknown-linux-gnu/kv-lines"
        );
        assert_eq!(parse(&m.to_toml()).unwrap(), m);
        let mut signed = m.clone();
        signed.key = Some("0123456789abcdef".into());
        assert_eq!(parse(&signed.to_toml()).unwrap(), signed);
        assert!(valid_target(host_target()));
        assert_eq!(target_for("windows", "x86_64"), "x86_64-pc-windows-msvc");
        assert_eq!(target_for("windows", "aarch64"), "aarch64-pc-windows-msvc");
        assert_eq!(target_for("linux", "x86_64"), "x86_64-unknown-linux-gnu");
        assert_eq!(target_for("linux", "aarch64"), "aarch64-unknown-linux-gnu");
        assert_eq!(target_for("macos", "aarch64"), "aarch64-apple-darwin");
        assert_eq!(target_for("macos", "x86_64"), "x86_64-apple-darwin");
        assert!(valid_target(target_for("plan9", "mips")));
    }

    fn every_permission() -> Vec<PermissionRequest> {
        let ask = |permission| PermissionRequest {
            permission,
            reason: "because".into(),
        };
        vec![
            ask(Permission::OpenedFilePaths),
            ask(Permission::ReadFolders(vec![
                GrantRef::new("symbols").unwrap(),
            ])),
            ask(Permission::WriteFolders(vec![
                GrantRef::new("out").unwrap(),
                GrantRef::new("tmp").unwrap(),
            ])),
            ask(Permission::RunPrograms(vec![
                ProgramRef::new("relog").unwrap(),
            ])),
            ask(Permission::Network(vec![
                HostPattern::new("example.com").unwrap(),
            ])),
            ask(Permission::Elevated(vec!["collector.start".into()])),
            ask(Permission::McpExpose),
            ask(Permission::Memory { mib: 512 }),
            ask(Permission::Cpu { percent: 50 }),
        ]
    }

    #[test]
    fn every_permission_kind_roundtrips_through_toml_postcard_and_json() {
        let mut m = parse(SAMPLE).unwrap();
        m.permissions = every_permission();
        m.check().unwrap();
        let back = parse(&m.to_toml()).unwrap();
        assert_eq!(back, m);
        let bytes = postcard::to_stdvec(&m).unwrap();
        assert_eq!(postcard::from_bytes::<Manifest>(&bytes).unwrap(), m);
        assert_eq!(
            serde_json::from_str::<Manifest>(&serde_json::to_string(&m).unwrap()).unwrap(),
            m
        );
        let kinds: Vec<_> = m.permissions.iter().map(|r| r.permission.kind()).collect();
        assert_eq!(kinds.len(), 9);
        assert!(
            m.permissions
                .iter()
                .all(|r| !r.permission.describe().is_empty())
        );
        assert!(m.permissions[1].permission.describe().contains("symbols"));
    }

    #[test]
    fn invalid_manifests_say_what_to_fix() {
        let cases = [
            ("format = 1", "format = 2", "format 2"),
            ("\"com.example.kv-lines\"", "\"nodots\"", "plugin id"),
            ("\"com.example.kv-lines\"", "\"../evil.x\"", "plugin id"),
            ("name = \"Key-value lines\"", "name = \"\"", "plugin.name"),
            ("version = \"1.2.0\"", "version = \"one\"", "plugin.version"),
            ("protocol = \"2.0\"", "protocol = \"x\"", "plugin.protocol"),
            (
                "binary = \"kv-lines\"",
                "binary = \"../kv\"",
                "plugin.binary",
            ),
            (
                "binary = \"kv-lines\"",
                "binary = \"kv.exe\"",
                "plugin.binary",
            ),
            (
                "targets = [\"x86_64-pc-windows-msvc\", \"x86_64-unknown-linux-gnu\"]",
                "targets = []",
                "plugin.targets",
            ),
            (
                "targets = [\"x86_64-pc-windows-msvc\", \"x86_64-unknown-linux-gnu\"]",
                "targets = [\"x86_64-pc-windows-msvc\", \"x86_64-pc-windows-msvc\"]",
                "twice",
            ),
            (
                "targets = [\"x86_64-pc-windows-msvc\", \"x86_64-unknown-linux-gnu\"]",
                "targets = [\"../x/y\"]",
                "target triple",
            ),
            (
                "publisher = \"Example Ltd\"",
                "publisher = \"Example Ltd\"\nkey = \"XYZ\"",
                "plugin.key",
            ),
            (
                "publisher = \"Example Ltd\"",
                "publisher = \"\"",
                "plugin.publisher",
            ),
            (
                "description = \"Reads key=value logs.\"",
                "description = \"a\\u0007b\"",
                "plugin.description",
            ),
            (
                "publisher = \"Example Ltd\"",
                "publisher = \"Example Ltd\"\nextra = 1",
                "unknown field",
            ),
            (
                "kind = \"extension\"",
                "kind = \"widget\"",
                "contribution kind \"widget\"",
            ),
            (
                "ns = \"debrief.import/3\"",
                "ns = \"debrief.import\"",
                "namespace/minor",
            ),
            ("ns = \"debrief.import/3\"", "id = \"x\"", "takes no `id`"),
            (
                "kind = \"settings\"\nid = \"main\"",
                "kind = \"settings\"",
                "needs an `id`",
            ),
            (
                "kind = \"mcpTool\"\nid = \"lookup\"",
                "kind = \"mcpTool\"\nid = \"lookup\"\nactions = [{ id = \"x\" }]",
                "takes no `actions`",
            ),
            (
                "kind = \"tool\"\nid = \"collector\"",
                "kind = \"tool\"\nid = \"collector\"\nns = \"a/1\"",
                "takes no `ns`",
            ),
            (
                "kind = \"extension\"\nns = \"debrief.import/3\"",
                "kind = \"extension\"",
                "needs an `ns`",
            ),
            ("id = \"collector\"", "id = \"../c\"", "contribution id"),
            ("{ id = \"status\" }", "{ id = \"start\" }", "unique id"),
            ("{ id = \"status\" }", "{ id = \"a b\" }", "unique id"),
            (
                "permission = \"network\"",
                "permission = \"teleport\"",
                "not known",
            ),
            (
                "reason = \"Looks up host names.\"",
                "reason = \"\"",
                "reason",
            ),
            (
                "hosts = [\"*.example.com\", \"example.org\"]",
                "hosts = [\"http://x\"]",
                "host pattern",
            ),
            (
                "hosts = [\"*.example.com\", \"example.org\"]",
                "hosts = [\"a.com\", \"a.com\"]",
                "twice",
            ),
            (
                "hosts = [\"*.example.com\", \"example.org\"]",
                "grants = [\"x\"]",
                "takes `hosts`, not `grants`",
            ),
            (
                "hosts = [\"*.example.com\", \"example.org\"]",
                "",
                "needs `hosts`",
            ),
        ];
        for (from, to, says) in cases {
            assert!(SAMPLE.contains(from), "{from}");
            let e = parse(&SAMPLE.replacen(from, to, 1)).unwrap_err();
            assert_eq!(e.code(), "invalidManifest", "{to}: {e}");
            assert!(e.to_string().contains(says), "{to}: {e}");
        }
        let twice = format!(
            "{SAMPLE}\n[[permissions]]\npermission = \"network\"\nhosts = [\"a.com\"]\nreason = \"again\"\n"
        );
        assert!(parse(&twice).unwrap_err().to_string().contains("twice"));
        let dup = format!("{SAMPLE}\n[[contributions]]\nkind = \"settings\"\nid = \"main\"\n");
        assert!(
            parse(&dup)
                .unwrap_err()
                .to_string()
                .contains("settings:main")
        );
        assert!(parse("not toml [").is_err());
        assert!(parse(&format!("{SAMPLE}\n[[permissions]]\npermission = \"mcpExpose\"\nhosts = [\"a.com\"]\nreason = \"x\"\n"))
            .unwrap_err()
            .to_string()
            .contains("takes no `hosts`"));
    }

    #[test]
    fn permission_ranges_and_lists_are_enforced() {
        let with =
            |permission: &str| format!("{SAMPLE}\n[[permissions]]\n{permission}\nreason = \"r\"\n");
        for (ok, text) in [
            (true, "permission = \"memory\"\nmib = 16"),
            (true, "permission = \"memory\"\nmib = 65536"),
            (false, "permission = \"memory\"\nmib = 15"),
            (false, "permission = \"memory\"\nmib = 65537"),
            (true, "permission = \"cpu\"\npercent = 100"),
            (false, "permission = \"cpu\"\npercent = 0"),
            (false, "permission = \"cpu\"\npercent = 101"),
            (false, "permission = \"cpu\"\npercent = 300"),
            (
                true,
                "permission = \"elevated\"\nops = [\"collector.start\"]",
            ),
            (false, "permission = \"elevated\"\nops = [\"a b\"]"),
            (false, "permission = \"readFolders\"\ngrants = [\"/etc\"]"),
            (
                false,
                "permission = \"runPrograms\"\nprograms = [\"C:\\\\x.exe\"]",
            ),
            (false, "permission = \"cpu\"\nmib = 3"),
        ] {
            assert_eq!(
                parse(&with(text)).is_ok(),
                ok,
                "{text}: {:?}",
                parse(&with(text)).err()
            );
        }
        let many: Vec<String> = (0..=MAX_LIST).map(|i| format!("\"g{i}\"")).collect();
        let e = parse(&with(&format!(
            "permission = \"readFolders\"\ngrants = [{}]",
            many.join(",")
        )))
        .unwrap_err();
        assert!(e.to_string().contains("1-64 entries"), "{e}");
        // A manifest received over the IPC is checked by the same rules.
        let mut m = parse(SAMPLE).unwrap();
        m.permissions.clear();
        m.permissions.push(PermissionRequest {
            permission: Permission::Network(vec![]),
            reason: "x".into(),
        });
        assert!(m.check().unwrap_err().to_string().contains("1-64 entries"));
    }

    #[test]
    fn counts_and_sizes_are_bounded() {
        let huge = format!("{SAMPLE}#{}", "x".repeat(MAX_MANIFEST_BYTES as usize));
        assert_eq!(parse(&huge).unwrap_err().code(), "tooLarge");
        let many: String = (0..=MAX_CONTRIBUTIONS)
            .map(|i| format!("[[contributions]]\nkind = \"settings\"\nid = \"s{i}\"\n"))
            .collect();
        let text = SAMPLE.replacen("[[contributions]]", &format!("{many}[[contributions]]"), 1);
        assert!(text.len() as u64 <= MAX_MANIFEST_BYTES);
        assert!(
            parse(&text)
                .unwrap_err()
                .to_string()
                .contains("contributions")
        );
        let actions: Vec<String> = (0..=MAX_ACTIONS)
            .map(|i| format!("{{ id = \"a{i}\" }}"))
            .collect();
        let text = SAMPLE.replacen(
            "actions = [{ id = \"refresh\" }]",
            &format!("actions = [{}]", actions.join(",")),
            1,
        );
        assert!(
            parse(&text)
                .unwrap_err()
                .to_string()
                .contains("more than 64 actions")
        );
        let perms: String = (0..17)
            .map(|_| "[[permissions]]\npermission = \"mcpExpose\"\nreason = \"r\"\n".to_string())
            .collect();
        assert!(
            parse(&format!("{SAMPLE}{perms}"))
                .unwrap_err()
                .to_string()
                .contains("too many permissions")
        );
        // Checked on the model too (a manifest from the IPC).
        let mut m = parse(SAMPLE).unwrap();
        m.contributions = (0..=MAX_CONTRIBUTIONS)
            .map(|i| Contribution::Settings {
                id: format!("s{i}"),
            })
            .collect();
        assert!(m.check().is_err());
        let mut m = parse(SAMPLE).unwrap();
        m.name = "x".repeat(81);
        assert!(m.check().is_err());
    }

    #[test]
    fn compatibility_names_who_must_update() {
        let m = parse(SAMPLE).unwrap();
        let host = ProtocolVersion::new(2, 1);
        let ns = |s: &str| s.parse::<Namespace>().unwrap();
        assert!(
            m.check_compatible(host, &[ns("debrief.import/3")], "Debrief")
                .is_ok()
        );
        assert!(
            m.check_compatible(host, &[ns("debrief.import/5"), ns("other/1")], "Debrief")
                .is_ok(),
            "the host runs older minors"
        );
        let e = m
            .check_compatible(host, &[ns("debrief.import/2")], "Debrief")
            .unwrap_err();
        assert_eq!(e.code(), "incompatible");
        assert!(
            e.to_string().contains("debrief.import/3 or newer")
                && e.to_string().contains("update Debrief"),
            "{e}"
        );
        let e = m
            .check_compatible(host, &[ns("pacenotes.index/1")], "Pacenotes")
            .unwrap_err();
        assert!(e.to_string().contains("does not have"), "{e}");
        let e = m
            .check_compatible(ProtocolVersion::new(3, 0), &[], "Debrief")
            .unwrap_err();
        assert!(
            e.to_string().contains("older contract")
                && e.to_string().contains("plugin \"com.example.kv-lines\""),
            "{e}"
        );
        let e = m
            .check_compatible(ProtocolVersion::new(1, 9), &[], "Debrief")
            .unwrap_err();
        assert!(e.to_string().contains("update Debrief"), "{e}");
    }
}
