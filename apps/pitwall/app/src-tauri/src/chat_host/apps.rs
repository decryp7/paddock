//! Settings > Connected apps (ADR 0023 Decision 7): the family apps whose MCP tools the chat may use, the key the user pasted
//! for each (kept by `paddock-secrets`, never shown again and never logged), the connection over the app's loopback listener
//! (`paddock_mcp::client::connect_loopback`, the family's one handshake), and the assistant's tool set built from what is
//! connected. Listing is on connect and on Refresh, never per turn (Dyno sends no `list_changed`).

use super::policy::PitwallPolicy;
use super::tool::{tool_set, tools_of, ToolNames};
use paddock_assistant::{Assistant, AssistantTool};
use paddock_mcp::client::{connect_loopback, Connection, ConnectionConfig};
use paddock_secrets::{Name, Secret, Secrets};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// The family apps the chat can connect to: id (its `paddock-env` folder name) and title. Recce joins when it serves MCP.
pub const KNOWN: &[(&str, &str)] = &[("dyno", "Dyno")];
/// Longest key accepted.
pub const MAX_KEY_LEN: usize = 256;

/// One tool of a connected app, as Settings lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTool {
    /// The tool's name on that app.
    pub name: String,
    /// Its name for people.
    pub title: String,
    /// It only looks (asked once, then remembered when allowed); otherwise an action, asked every call.
    pub read_only: bool,
    /// The user said Always allow for it (Settings can ask again).
    pub remembered: bool,
}

/// A family app as Settings > Connected apps shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedApp {
    /// `dyno`.
    pub id: String,
    /// `Dyno`.
    pub title: String,
    /// A key is saved for it.
    pub key_saved: bool,
    /// Connected now.
    pub connected: bool,
    /// Why the last connect failed, in the app's words with the next step; `None` when it did not.
    pub problem: Option<String>,
    /// The app refused the saved key: Refresh cannot help, only a new key.
    pub key_refused: bool,
    /// Its tools, when connected.
    pub tools: Vec<AppTool>,
    /// How many of its tools are always allowed.
    pub remembered: usize,
}

/// Why a command about a connected app was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppsError {
    /// Not one of [`KNOWN`].
    Unknown(String),
    /// The pasted text is not a key.
    BadKey,
    /// The key could not be saved or read; the text says why.
    Store(String),
}

impl std::fmt::Display for AppsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppsError::Unknown(id) => write!(f, "Pitwall does not know an app called {id}."),
            AppsError::BadKey => {
                f.write_str("That is not an AI access key. Copy the key Dyno shows in Settings > AI access (it starts with pmk_).")
            }
            AppsError::Store(why) => write!(f, "The key could not be kept: {why}."),
        }
    }
}

impl std::error::Error for AppsError {}

/// The key as typed: trimmed, `pmk_`, no spaces, at most [`MAX_KEY_LEN`].
pub fn parse_key(text: &str) -> Result<String, AppsError> {
    let key = text.trim();
    if key.len() > 4 && key.len() <= MAX_KEY_LEN && key.starts_with("pmk_") && !key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        Ok(key.to_string())
    } else {
        Err(AppsError::BadKey)
    }
}

/// Where an app's listener file is, by app id.
type Discovery = Box<dyn Fn(&str) -> Option<PathBuf> + Send + Sync>;

struct Live {
    _conn: Arc<Connection>,
    tools: Vec<Arc<dyn AssistantTool>>,
}

/// The connected apps of this app process.
pub struct ConnectedApps {
    secrets: Arc<dyn Secrets>,
    discovery: Discovery,
    assistant: Arc<Assistant>,
    policy: Arc<PitwallPolicy>,
    names: ToolNames,
    live: Mutex<BTreeMap<String, Live>>,
    problems: Mutex<BTreeMap<String, (String, bool)>>,
    changed: Mutex<Option<OnChange>>,
}

/// Told every apps' views after each change.
type OnChange = Box<dyn Fn(&[ConnectedApp]) + Send + Sync>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn title_of(id: &str) -> Result<&'static str, AppsError> {
    KNOWN
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, t)| *t)
        .ok_or_else(|| AppsError::Unknown(id.into()))
}

fn key_name(id: &str) -> Result<Name, AppsError> {
    Name::new("mcp", id).map_err(|e| AppsError::Store(e.to_string()))
}

impl ConnectedApps {
    /// The apps over `secrets`; `discovery` names an app's listener file (`paddock_env` folders in the app, a temp folder
    /// in tests).
    pub fn new(
        secrets: Arc<dyn Secrets>,
        discovery: impl Fn(&str) -> Option<PathBuf> + Send + Sync + 'static,
        assistant: Arc<Assistant>,
        policy: Arc<PitwallPolicy>,
        names: ToolNames,
    ) -> ConnectedApps {
        ConnectedApps {
            secrets,
            discovery: Box::new(discovery),
            assistant,
            policy,
            names,
            live: Mutex::default(),
            problems: Mutex::default(),
            changed: Mutex::default(),
        }
    }

    /// Calls `f` with every app's view after each change (connect, a new key, forget): the app emits it to every window.
    pub fn on_change(&self, f: impl Fn(&[ConnectedApp]) + Send + Sync + 'static) {
        *lock(&self.changed) = Some(Box::new(f));
    }

    /// Every known app.
    pub fn views(&self) -> Vec<ConnectedApp> {
        KNOWN.iter().map(|(id, _)| self.view(id)).collect()
    }

    fn view(&self, id: &str) -> ConnectedApp {
        let live = lock(&self.live);
        let allowed: Vec<String> = self
            .policy
            .remembered()
            .into_iter()
            .filter(|r| r.server == id)
            .map(|r| r.tool)
            .collect();
        let tools = live
            .get(id)
            .map(|l| {
                l.tools
                    .iter()
                    .filter_map(|t| {
                        self.names.of(&t.spec().name).map(|(_, name)| AppTool {
                            remembered: allowed.contains(&name),
                            name,
                            title: t.title().unwrap_or_default(),
                            read_only: !t.access().writes(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let problem = lock(&self.problems).get(id).cloned();
        ConnectedApp {
            id: id.into(),
            title: title_of(id).unwrap_or(id).into(),
            key_saved: key_name(id).ok().and_then(|n| self.secrets.get(&n).ok().flatten()).is_some(),
            connected: live.contains_key(id),
            key_refused: problem.as_ref().is_some_and(|p| p.1),
            problem: problem.map(|p| p.0),
            tools,
            remembered: self.policy.remembered().iter().filter(|r| r.server == id).count(),
        }
    }

    /// Keeps a pasted key and connects with it. A bad key is refused before anything is kept.
    pub fn set_key(&self, id: &str, text: &str) -> Result<ConnectedApp, AppsError> {
        title_of(id)?;
        let key = parse_key(text)?;
        let secret = Secret::new(key).map_err(|e| AppsError::Store(e.to_string()))?;
        self.secrets
            .set(&key_name(id)?, &secret)
            .map_err(|e| AppsError::Store(e.to_string()))?;
        log::info!(target: "pitwall_chat", "connected apps: a key was saved for {id}");
        self.connect(id)
    }

    /// Connects (again) with the saved key and lists the app's tools; a failure is kept as the app's problem, in words.
    pub fn connect(&self, id: &str) -> Result<ConnectedApp, AppsError> {
        let title = title_of(id)?;
        lock(&self.live).remove(id);
        // The failure in words, and whether the app refused the key (then only a new key helps).
        let outcome = (|| -> Result<Live, (String, bool)> {
            let plain = |e: String| (e, false);
            let key = self
                .secrets
                .get(&key_name(id).map_err(|e| plain(e.to_string()))?)
                .map_err(|e| plain(e.to_string()))?
                .ok_or_else(|| {
                    plain(format!(
                        "No key is saved for {title}. Issue one in {title}'s Settings > AI access and paste it here."
                    ))
                })?;
            let file = (self.discovery)(id).ok_or_else(|| plain(format!("Pitwall cannot find {title}'s folders.")))?;
            let link = connect_loopback(&file, key.expose()).map_err(|e| {
                let refused = matches!(e, paddock_mcp::client::ConnectError::KeyRefused { .. });
                (e.to_string(), refused)
            })?;
            let conn = Arc::new(link.into_connection(ConnectionConfig::default()));
            conn.initialize("Pitwall", env!("CARGO_PKG_VERSION"))
                .map_err(|e| plain(e.to_string()))?;
            let defs = conn.list_tools().map_err(|e| plain(e.to_string()))?;
            let tools = tools_of(id, title, &defs, true, &conn, &self.names);
            Ok(Live { _conn: conn, tools })
        })();
        match outcome {
            Ok(live) => {
                log::info!(target: "pitwall_chat", "connected apps: {id} connected, {} tools", live.tools.len());
                lock(&self.problems).remove(id);
                lock(&self.live).insert(id.into(), live);
            }
            Err((why, refused)) => {
                log::warn!(target: "pitwall_chat", "connected apps: {id} not connected, cause: {why}");
                lock(&self.problems).insert(id.into(), (why, refused));
            }
        }
        self.publish();
        Ok(self.view(id))
    }

    /// Connects every app that has a key (at start and when the chat opens); failures are each app's problem.
    pub fn connect_saved(&self) {
        for (id, _) in KNOWN {
            if self.view(id).key_saved {
                let _ = self.connect(id);
            }
        }
    }

    /// Asks again for one tool: its Always allow is forgotten.
    pub fn revoke(&self, id: &str, tool: &str) -> Result<ConnectedApp, AppsError> {
        title_of(id)?;
        self.policy.revoke(id, tool);
        log::info!(target: "pitwall_chat", "connected apps: {id} {tool} asks again");
        self.publish();
        Ok(self.view(id))
    }

    /// Forgets an app: its connection, its key and its remembered approvals.
    pub fn forget(&self, id: &str) -> Result<ConnectedApp, AppsError> {
        title_of(id)?;
        lock(&self.live).remove(id);
        lock(&self.problems).remove(id);
        self.secrets.delete(&key_name(id)?).map_err(|e| AppsError::Store(e.to_string()))?;
        self.policy.revoke_server(id);
        log::info!(target: "pitwall_chat", "connected apps: {id} forgotten");
        self.publish();
        Ok(self.view(id))
    }

    /// The assistant's tools: every connected app's.
    fn publish(&self) {
        let tools = lock(&self.live).values().flat_map(|l| l.tools.iter().cloned()).collect();
        self.assistant.set_tools(tool_set(tools));
        let views = self.views();
        if let Some(f) = lock(&self.changed).as_ref() {
            f(&views);
        }
    }
}
