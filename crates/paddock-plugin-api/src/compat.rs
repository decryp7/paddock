//! Compatibility between an app and a plugin (ADR 0001, "Versioning and compatibility"): the platform protocol and each
//! extension namespace follow the same rule as [`ProtocolVersion::accepts`] (same major, and the plugin's minor is one
//! the host knows), and a refusal says who must update.
//!
//! An extension namespace carries its major version in its name (`debrief.import.v2`): another major is another
//! namespace, which a host either serves or does not know. Its minor is declared by both sides.

use crate::protocol::{Described, PLATFORM_PROTOCOL, ProtocolVersion};
use paddock_ipc::Skew;
use paddock_plugin_package::{Contribution, Manifest, Permission};
use std::collections::BTreeSet;
use std::fmt;

/// What an app tells the platform about itself, to judge plugins and word refusals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    /// The app's name for messages ("Debrief").
    pub app: String,
    /// The platform protocol it serves ([`PLATFORM_PROTOCOL`] unless it serves less).
    pub protocol: ProtocolVersion,
    /// The app versions that introduced each platform protocol minor ("2.1" with "Debrief 1.4"), for "needs 1.4 or newer".
    pub protocol_since: Vec<(ProtocolVersion, String)>,
    /// The namespaces the app serves.
    pub namespaces: Vec<NamespaceSupport>,
}

/// One extension namespace an app serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceSupport {
    /// The namespace, major version included in its name.
    pub ns: String,
    /// The newest minor the app knows.
    pub minor: u16,
    /// The app version that introduced each minor (`(2, "1.4")`), for "needs 1.4 or newer".
    pub since: Vec<(u16, String)>,
}

impl NamespaceSupport {
    /// A namespace the app serves up to `minor`, without release notes.
    pub fn new(ns: impl Into<String>, minor: u16) -> NamespaceSupport {
        NamespaceSupport {
            ns: ns.into(),
            minor,
            since: Vec::new(),
        }
    }

    /// Records that `app_version` introduced `minor`.
    pub fn since(mut self, minor: u16, app_version: impl Into<String>) -> NamespaceSupport {
        self.since.push((minor, app_version.into()));
        self
    }
}

impl HostInfo {
    /// An app serving [`PLATFORM_PROTOCOL`] and no extension.
    pub fn new(app: impl Into<String>) -> HostInfo {
        HostInfo {
            app: app.into(),
            protocol: PLATFORM_PROTOCOL,
            protocol_since: Vec::new(),
            namespaces: Vec::new(),
        }
    }

    /// Serves `ns`.
    pub fn serving(mut self, ns: NamespaceSupport) -> HostInfo {
        self.namespaces.push(ns);
        self
    }

    /// Whether the plugin `plugin`, built for `built_for`, can be served.
    pub fn check_protocol(&self, plugin: &str, built_for: ProtocolVersion) -> Result<(), Refusal> {
        let Some(skew) = self.protocol.skew(built_for) else {
            return Ok(());
        };
        if skew == Skew::ChildBehind {
            return Err(if built_for.major == 1 {
                Refusal::Legacy {
                    plugin: plugin.into(),
                    built_for,
                    app: self.app.clone(),
                    host: self.protocol,
                }
            } else {
                Refusal::PluginTooOld {
                    plugin: plugin.into(),
                    built_for,
                    app: self.app.clone(),
                    host: self.protocol,
                }
            });
        }
        let needs = ProtocolVersion::introduced_in(
            self.protocol_since.iter().map(|(v, a)| (*v, a.as_str())),
            built_for,
        )
        .map(str::to_owned);
        Err(Refusal::AppTooOld {
            plugin: plugin.into(),
            app: self.app.clone(),
            needs,
            what: format!(
                "protocol {built_for}, this {} speaks {}",
                self.app, self.protocol
            ),
        })
    }

    /// Whether the app can serve the plugin `plugin` speaking `ns` at `minor`.
    pub fn check_extension(&self, plugin: &str, ns: &str, minor: u16) -> Result<(), Refusal> {
        let Some(known) = self.namespaces.iter().find(|n| n.ns == ns) else {
            return Err(Refusal::AppTooOld {
                plugin: plugin.into(),
                app: self.app.clone(),
                needs: None,
                what: format!(
                    "the contract \"{ns}\", which this {} does not serve",
                    self.app
                ),
            });
        };
        // A contract's minor follows the protocol rule ([`ProtocolVersion::accepts`]); its major is in its name.
        let minor_of = |m: u16| ProtocolVersion::new(0, m);
        if minor_of(known.minor).accepts(minor_of(minor)) {
            return Ok(());
        }
        let needs = ProtocolVersion::introduced_in(
            known.since.iter().map(|(m, v)| (minor_of(*m), v.as_str())),
            minor_of(minor),
        )
        .map(str::to_owned);
        Err(Refusal::AppTooOld {
            plugin: plugin.into(),
            app: self.app.clone(),
            needs,
            what: format!(
                "\"{ns}\" version {minor}, this {} serves up to {}",
                self.app, known.minor
            ),
        })
    }
}

/// Compares what a plugin says it contributes (`Describe`) with the manifest the app verified. Everything must match:
/// a plugin that contributes more than it declared, or declares what it does not contribute, is refused, so the grants and
/// the badges the user saw are the truth.
pub fn check_described(manifest: &Manifest, d: &Described) -> Result<(), Refusal> {
    let plugin = manifest.id.as_str();
    let mismatch = |what: String| {
        Err(Refusal::Mismatch {
            plugin: plugin.into(),
            what,
        })
    };
    if d.id != plugin {
        return mismatch(format!("it calls itself \"{}\"", d.id));
    }
    if d.version != manifest.version.to_string() {
        return mismatch(format!(
            "it is version {}, the manifest says {}",
            d.version, manifest.version
        ));
    }
    if d.protocol != manifest.protocol {
        return mismatch(format!(
            "it speaks protocol {}, the manifest says {}",
            d.protocol, manifest.protocol
        ));
    }
    let (mut tools, mut views, mut mcp, mut exts) =
        (Vec::new(), Vec::new(), BTreeSet::new(), BTreeSet::new());
    let mut settings = None;
    for c in &manifest.contributions {
        match c {
            Contribution::Tool { id, actions } => tools.push((
                id.as_str(),
                actions
                    .iter()
                    .map(|a| a.id.as_str())
                    .collect::<BTreeSet<_>>(),
            )),
            Contribution::View { id, actions } => {
                views.push((id.as_str(), actions.iter().any(|a| a.writes)))
            }
            Contribution::McpTool { id } => {
                mcp.insert(id.as_str());
            }
            Contribution::Settings { id } => settings = Some(id.as_str()),
            Contribution::Extension(n) => {
                exts.insert((n.ns(), n.minor));
            }
        }
    }
    tools.sort();
    views.sort();
    let mut said_tools: Vec<(&str, BTreeSet<&str>)> = d
        .tools
        .iter()
        .map(|t| {
            (
                t.id.as_str(),
                t.ops.iter().map(|o| o.name.as_str()).collect(),
            )
        })
        .collect();
    said_tools.sort();
    if said_tools != tools {
        return mismatch(format!(
            "its tools ({}) are not the manifest's ({})",
            said_tools
                .iter()
                .map(|t| t.0)
                .collect::<Vec<_>>()
                .join(", "),
            tools.iter().map(|t| t.0).collect::<Vec<_>>().join(", ")
        ));
    }
    let mut said_views: Vec<(&str, bool)> =
        d.views.iter().map(|v| (v.id.as_str(), v.writes)).collect();
    said_views.sort();
    if said_views != views {
        return mismatch("its views, or which of them write, are not the manifest's".into());
    }
    let said_mcp: BTreeSet<&str> = d.mcp_tools.iter().map(|t| t.name.as_str()).collect();
    if said_mcp != mcp {
        return mismatch("its MCP tools are not the manifest's".into());
    }
    if !mcp.is_empty()
        && !manifest
            .permissions
            .iter()
            .any(|p| p.permission == Permission::McpExpose)
    {
        return mismatch("it offers MCP tools without asking for the mcpExpose permission".into());
    }
    if d.settings.as_ref().map(|s| s.id.as_str()) != settings {
        return mismatch("its settings section is not the manifest's".into());
    }
    let said_exts: BTreeSet<(&str, u16)> = d
        .extensions
        .iter()
        .map(|e| (e.ns.as_str(), e.minor))
        .collect();
    if said_exts != exts {
        return mismatch("the contracts it speaks are not the manifest's".into());
    }
    Ok(())
}

/// Why a plugin cannot be served, worded for the user. The status of the plugin in the app is
/// `Refused(reason)` with this text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Built for protocol 1 (Debrief's own, before the platform): the app's compatibility shim may serve it
    /// (ADR 0003 phase 3), otherwise it is refused with this text.
    Legacy {
        /// The plugin's id.
        plugin: String,
        /// What it was built for.
        built_for: ProtocolVersion,
        /// The app.
        app: String,
        /// What the app serves.
        host: ProtocolVersion,
    },
    /// Built for an older major version of the platform than the app serves.
    PluginTooOld {
        /// The plugin's id.
        plugin: String,
        /// What it was built for.
        built_for: ProtocolVersion,
        /// The app.
        app: String,
        /// What the app serves.
        host: ProtocolVersion,
    },
    /// Needs more than the app has: the app must update.
    AppTooOld {
        /// The plugin's id.
        plugin: String,
        /// The app.
        app: String,
        /// The app version that has it, when known.
        needs: Option<String>,
        /// What exactly is missing.
        what: String,
    },
    /// The plugin contributes something its manifest does not declare (or declares something it does not contribute).
    Mismatch {
        /// The plugin's id.
        plugin: String,
        /// What differs.
        what: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Legacy {
                plugin,
                built_for,
                app,
                host,
            } => write!(
                f,
                "plugin {plugin} was built for an older contract (protocol {built_for}; this {app} speaks {host}); update it"
            ),
            Refusal::PluginTooOld {
                plugin,
                built_for,
                app,
                host,
            } => write!(
                f,
                "plugin {plugin} was built for an older contract (protocol {built_for}; this {app} speaks {host}); update it"
            ),
            Refusal::AppTooOld {
                plugin,
                app,
                needs: Some(v),
                ..
            } => {
                write!(f, "plugin {plugin} needs {app} {v} or newer")
            }
            Refusal::AppTooOld {
                plugin,
                app,
                needs: None,
                what,
            } => {
                write!(
                    f,
                    "plugin {plugin} needs a newer {app}: it uses {what}; update {app}"
                )
            }
            Refusal::Mismatch { plugin, what } => {
                write!(
                    f,
                    "plugin {plugin} does not match its manifest ({what}); reinstall or update it"
                )
            }
        }
    }
}

impl std::error::Error for Refusal {}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> HostInfo {
        let mut h = HostInfo::new("Debrief")
            .serving(NamespaceSupport::new("debrief.import.v1", 2).since(2, "1.4"));
        h.protocol = ProtocolVersion::new(2, 1);
        h.protocol_since = vec![(ProtocolVersion::new(2, 1), "1.3".into())];
        h
    }

    #[test]
    fn the_protocol_rule_is_same_major_and_a_known_minor() {
        let h = host();
        assert_eq!(h.check_protocol("p", ProtocolVersion::new(2, 0)), Ok(()));
        assert_eq!(h.check_protocol("p", ProtocolVersion::new(2, 1)), Ok(()));
        let newer = h
            .check_protocol("p", ProtocolVersion::new(2, 2))
            .unwrap_err();
        assert_eq!(
            newer.to_string(),
            "plugin p needs a newer Debrief: it uses protocol 2.2, this Debrief speaks 2.1; update Debrief"
        );
        let mut h = host();
        h.protocol = ProtocolVersion::new(2, 0);
        h.protocol_since = vec![
            (ProtocolVersion::new(2, 0), "1.0".into()),
            (ProtocolVersion::new(2, 3), "1.9".into()),
        ];
        let named = h
            .check_protocol("p", ProtocolVersion::new(2, 3))
            .unwrap_err();
        assert_eq!(named.to_string(), "plugin p needs Debrief 1.9 or newer");
        let future_major = h
            .check_protocol("p", ProtocolVersion::new(3, 0))
            .unwrap_err();
        assert!(matches!(future_major, Refusal::AppTooOld { .. }));
    }

    #[test]
    fn protocol_one_is_legacy_and_older_majors_say_update_the_plugin() {
        let h = host();
        let legacy = h
            .check_protocol("p", ProtocolVersion::new(1, 1))
            .unwrap_err();
        assert!(matches!(legacy, Refusal::Legacy { .. }));
        assert_eq!(
            legacy.to_string(),
            "plugin p was built for an older contract (protocol 1.1; this Debrief speaks 2.1); update it"
        );
        let mut h3 = host();
        h3.protocol = ProtocolVersion::new(3, 0);
        let old = h3
            .check_protocol("p", ProtocolVersion::new(2, 0))
            .unwrap_err();
        assert!(matches!(old, Refusal::PluginTooOld { .. }));
        assert!(old.to_string().ends_with("update it"));
    }

    #[test]
    fn a_namespace_is_served_up_to_its_minor() {
        let h = host();
        assert_eq!(h.check_extension("p", "debrief.import.v1", 2), Ok(()));
        let newer = h.check_extension("p", "debrief.import.v1", 3).unwrap_err();
        assert_eq!(
            newer.to_string(),
            "plugin p needs a newer Debrief: it uses \"debrief.import.v1\" version 3, this Debrief serves up to 2; update Debrief"
        );
        let mut h = host();
        h.namespaces[0].minor = 1;
        h.namespaces[0].since.push((2, "1.4".into()));
        assert_eq!(
            h.check_extension("p", "debrief.import.v1", 2)
                .unwrap_err()
                .to_string(),
            "plugin p needs Debrief 1.4 or newer"
        );
        let unknown = h.check_extension("p", "other.v1", 0).unwrap_err();
        assert!(unknown.to_string().contains("does not serve"));
    }

    #[test]
    fn a_mismatch_with_the_manifest_says_what_differs() {
        let m = Refusal::Mismatch {
            plugin: "p".into(),
            what: "tool \"x\" is not declared".into(),
        };
        assert_eq!(
            m.to_string(),
            "plugin p does not match its manifest (tool \"x\" is not declared); reinstall or update it"
        );
    }
}
