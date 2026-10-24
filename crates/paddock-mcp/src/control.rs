//! The engine side of the app's "AI access (MCP)" settings page: the saved settings, the tools and their exposure, the clients'
//! keys (issued once, persisted in the credential store, revocable), and the audit log. Pure Rust; the Tauri adapter
//! ([`crate::tauri`]) maps its commands and event onto this.
//!
//! Every method is quick and bounded (a small file, one credential entry): none needs a job (rule 10). Nothing here ever returns
//! a key except [`McpControl::issue_key`], once.

use crate::audit::{AuditEntry, AuditLog, Outcome};
use crate::keys::{KeyEntry, KeyStore};
use crate::registry::{Access, Policy};
use crate::server::Server;
use paddock_env::VersionedFile;
use paddock_secrets::{Name, Secret, Secrets};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

/// Most client keys one app keeps (the entries must fit one credential: a few hundred bytes each).
pub const MAX_KEYS: usize = 16;
/// Longest client name.
const MAX_CLIENT_CHARS: usize = 64;
/// Most tool names in the settings.
const MAX_NAMES: usize = 256;

/// What the user chose. Off by default (ADR 0007 rule 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct McpSettings {
    /// The master switch.
    pub enabled: bool,
    /// Whether resources are exposed.
    pub resources: bool,
    /// Read-only tools the user switched off.
    pub disabled_tools: Vec<String>,
    /// Action tools the user allowed, by name.
    pub allowed_actions: Vec<String>,
}

impl Default for McpSettings {
    fn default() -> Self {
        McpSettings {
            enabled: false,
            resources: true,
            disabled_tools: vec![],
            allowed_actions: vec![],
        }
    }
}

/// Where the settings live. The app implements it over its own settings file; [`FileSettings`] is the shared one.
pub trait SettingsStore: Send + Sync {
    /// The saved settings (the defaults when there are none or the file is unusable).
    fn load(&self) -> McpSettings;
    /// Saves the settings; the message says what to fix.
    fn save(&self, settings: &McpSettings) -> Result<(), String>;
}

/// [`SettingsStore`] over a versioned JSON file.
pub struct FileSettings(VersionedFile<McpSettings>);

impl FileSettings {
    /// The settings in `path` (for example `dirs.config().join("mcp.json")`).
    pub fn new(path: impl Into<PathBuf>) -> FileSettings {
        FileSettings(VersionedFile::new(path, 1))
    }
}

impl SettingsStore for FileSettings {
    fn load(&self) -> McpSettings {
        self.0.load().value
    }

    fn save(&self, settings: &McpSettings) -> Result<(), String> {
        self.0
            .save(settings)
            .map_err(|e| format!("The AI access settings could not be saved: {e}"))
    }
}

/// [`SettingsStore`] in memory, for tests.
#[derive(Default)]
pub struct MemorySettings {
    value: Mutex<McpSettings>,
    /// When set, [`SettingsStore::save`] fails with this message.
    pub fail: Mutex<Option<String>>,
}

impl SettingsStore for MemorySettings {
    fn load(&self) -> McpSettings {
        self.value
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn save(&self, settings: &McpSettings) -> Result<(), String> {
        if let Some(why) = self
            .fail
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            return Err(why);
        }
        *self.value.lock().unwrap_or_else(PoisonError::into_inner) = settings.clone();
        Ok(())
    }
}

/// A tool as the page lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    /// Its name.
    pub name: String,
    /// A name for people.
    pub title: Option<String>,
    /// What it does.
    pub description: String,
    /// `true` for an action tool (it changes something and needs its own permission).
    pub action: bool,
    /// Whether clients see it under the saved settings.
    pub exposed: bool,
}

/// A client key as the page lists it; never the key itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    /// The id to revoke by.
    #[cfg_attr(test, ts(type = "number"))]
    pub id: u64,
    /// The client it was issued to.
    pub client: String,
    /// When it was issued (epoch milliseconds).
    #[cfg_attr(test, ts(type = "number"))]
    pub created_ms: i64,
    /// Revoked keys stay in the list but no longer work.
    pub revoked: bool,
}

/// What the page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct McpView {
    /// The saved settings.
    pub settings: McpSettings,
    /// Every tool the app registered.
    pub tools: Vec<ToolInfo>,
    /// The clients' keys.
    pub keys: Vec<KeyInfo>,
    /// Most keys that can be issued.
    pub max_keys: u32,
}

/// The reply of `mcp_issue_key`: the key, shown once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct IssuedKey {
    /// The key's id (in the list).
    #[cfg_attr(test, ts(type = "number"))]
    pub id: u64,
    /// The client it belongs to.
    pub client: String,
    /// The key. The engine keeps only its hash; it cannot be shown again.
    pub key: String,
}

/// A call in the audit log, as the viewer shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AuditRow {
    /// When (epoch milliseconds).
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: i64,
    /// Who asked.
    pub client: String,
    /// The tool.
    pub tool: String,
    /// The arguments, redacted.
    #[cfg_attr(test, ts(type = "unknown"))]
    pub arguments: Value,
    /// Bytes of the result.
    #[cfg_attr(test, ts(type = "number"))]
    pub result_bytes: u64,
    /// How long it took.
    #[cfg_attr(test, ts(type = "number"))]
    pub duration_ms: u64,
    /// How it ended.
    pub outcome: Outcome,
}

/// Why a command did not work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "McpErrorCode"))]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The input is not acceptable; the message says what to fix.
    Invalid,
    /// The credential store or the settings file could not be used.
    StoreUnavailable,
    /// Anything else.
    Failed,
}

/// A refused command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct McpError {
    /// The kind.
    pub code: ErrorCode,
    /// What to do, for the user.
    pub message: String,
    /// Whether trying again can help.
    pub retryable: bool,
}

impl McpError {
    fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> McpError {
        McpError {
            code,
            message: message.into(),
            retryable,
        }
    }
}

/// What is sent to every window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum McpEvent {
    /// A call was recorded or the log was cleared; `count` is how many entries it holds.
    Audit {
        /// Entries now in the log.
        count: u32,
    },
}

/// How the control tells the adapter about events.
pub type Emit = Arc<dyn Fn(&McpEvent) + Send + Sync>;

/// An [`AuditLog`] that also tells the windows when a call is recorded.
struct Announcing {
    log: Arc<AuditLog>,
    emit: Emit,
}

impl crate::audit::AuditSink for Announcing {
    fn record(&self, entry: AuditEntry) {
        self.log.record(entry);
        (self.emit)(&McpEvent::Audit {
            count: u32::try_from(self.log.entries().len()).unwrap_or(u32::MAX),
        });
    }
}

/// The AI-access settings of an app.
pub struct McpControl {
    server: Arc<Server>,
    audit: Arc<AuditLog>,
    keys: Arc<KeyStore>,
    store: Arc<dyn SettingsStore>,
    secrets: Arc<dyn Secrets>,
    emit: Emit,
    now_ms: Arc<dyn Fn() -> i64 + Send + Sync>,
    write: Mutex<()>,
}

fn keys_name() -> Result<Name, McpError> {
    Name::new("mcp", "keys").map_err(|e| McpError::new(ErrorCode::Failed, e.to_string(), false))
}

impl McpControl {
    /// The sink to give [`Server::new`] so that the windows hear of every recorded call: records into `log` and emits. Build the
    /// server with it, then the control with that server and the same `log`.
    pub fn announcing_sink(log: Arc<AuditLog>, emit: Emit) -> Arc<dyn crate::audit::AuditSink> {
        Arc::new(Announcing { log, emit })
    }

    /// The control: loads the saved settings into the server's policy and the saved keys into the key store.
    pub fn new(
        server: Arc<Server>,
        audit: Arc<AuditLog>,
        keys: Arc<KeyStore>,
        store: Arc<dyn SettingsStore>,
        secrets: Arc<dyn Secrets>,
        emit: Emit,
        now_ms: Arc<dyn Fn() -> i64 + Send + Sync>,
    ) -> McpControl {
        let control = McpControl {
            server,
            audit,
            keys,
            store,
            secrets,
            emit,
            now_ms,
            write: Mutex::new(()),
        };
        control.apply(&control.store.load());
        control.load_keys();
        control
    }

    fn load_keys(&self) {
        // An unreadable or damaged key entry means no keys work until new ones are issued: failing closed.
        let entries = keys_name()
            .ok()
            .and_then(|n| self.secrets.get(&n).ok().flatten())
            .and_then(|s| serde_json::from_str::<Vec<KeyEntry>>(s.expose()).ok())
            .unwrap_or_default();
        self.keys.import(entries);
    }

    fn persist_keys(&self) -> Result<(), McpError> {
        let text = serde_json::to_string(&self.keys.export())
            .map_err(|e| McpError::new(ErrorCode::Failed, e.to_string(), false))?;
        let secret = Secret::new(text)
            .map_err(|e| McpError::new(ErrorCode::Invalid, e.to_string(), false))?;
        self.secrets.set(&keys_name()?, &secret).map_err(|e| {
            McpError::new(
                ErrorCode::StoreUnavailable,
                format!("The client keys could not be saved: {e}. Check the credential store."),
                true,
            )
        })
    }

    fn apply(&self, s: &McpSettings) {
        let off: BTreeSet<&String> = s.disabled_tools.iter().collect();
        let tools: BTreeSet<String> = self
            .server
            .tool_infos()
            .into_iter()
            .filter(|t| !off.contains(&t.name))
            .map(|t| t.name)
            .collect();
        self.server.set_policy(Policy {
            enabled: s.enabled,
            tools: Some(tools),
            actions: s.allowed_actions.iter().cloned().collect(),
            resources: s.resources,
        });
    }

    fn view(&self) -> McpView {
        let settings = self.store.load();
        McpView {
            tools: self
                .server
                .tool_infos()
                .into_iter()
                .map(|t| ToolInfo {
                    name: t.name,
                    title: t.title,
                    description: t.description,
                    action: t.access == Access::Action,
                    exposed: t.exposed,
                })
                .collect(),
            keys: self
                .keys
                .export()
                .into_iter()
                .map(|k| KeyInfo {
                    id: k.id,
                    client: k.client,
                    created_ms: k.created_ms,
                    revoked: k.revoked,
                })
                .collect(),
            max_keys: u32::try_from(MAX_KEYS).unwrap_or(u32::MAX),
            settings,
        }
    }

    /// The saved settings, the tools and the keys.
    pub fn get(&self) -> McpView {
        self.view()
    }

    /// Validates and saves the settings, and applies them to the running server at once.
    pub fn save(&self, settings: McpSettings) -> Result<McpView, McpError> {
        let known: BTreeSet<String> = self
            .server
            .tool_infos()
            .into_iter()
            .map(|t| t.name)
            .collect();
        if settings.disabled_tools.len() > MAX_NAMES || settings.allowed_actions.len() > MAX_NAMES {
            return Err(McpError::new(
                ErrorCode::Invalid,
                "Too many tools are listed.",
                false,
            ));
        }
        let actions: BTreeSet<String> = self
            .server
            .tool_infos()
            .into_iter()
            .filter(|t| t.access == Access::Action)
            .map(|t| t.name)
            .collect();
        if let Some(bad) = settings.disabled_tools.iter().find(|n| !known.contains(*n)) {
            return Err(McpError::new(
                ErrorCode::Invalid,
                format!("There is no tool named {bad}. Reload the page and try again."),
                false,
            ));
        }
        if let Some(bad) = settings
            .allowed_actions
            .iter()
            .find(|n| !actions.contains(*n))
        {
            return Err(McpError::new(
                ErrorCode::Invalid,
                format!("{bad} is not an action tool, so it needs no permission."),
                false,
            ));
        }
        let _write = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        self.store
            .save(&settings)
            .map_err(|m| McpError::new(ErrorCode::StoreUnavailable, m, true))?;
        self.apply(&settings);
        Ok(self.view())
    }

    /// Issues a key for `client` and returns it; the engine keeps only its hash. Persists the key list in the credential store; if
    /// that fails the key is not issued.
    pub fn issue_key(&self, client: &str) -> Result<IssuedKey, McpError> {
        let client = client.trim();
        if client.is_empty()
            || client.chars().count() > MAX_CLIENT_CHARS
            || client.chars().any(char::is_control)
        {
            return Err(McpError::new(
                ErrorCode::Invalid,
                format!(
                    "Name the client in 1 to {MAX_CLIENT_CHARS} characters, for example \"Pitwall\"."
                ),
                false,
            ));
        }
        let _write = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let before = self.keys.export();
        if before.iter().filter(|k| !k.revoked).count() >= MAX_KEYS {
            return Err(McpError::new(
                ErrorCode::Invalid,
                format!("{MAX_KEYS} keys are in use. Revoke one you no longer need first."),
                false,
            ));
        }
        let (id, key) = self
            .keys
            .issue(client, (self.now_ms)())
            .map_err(|e| McpError::new(ErrorCode::Failed, e, true))?;
        if let Err(e) = self.persist_keys() {
            self.keys.import(before);
            return Err(e);
        }
        Ok(IssuedKey {
            id,
            client: client.to_string(),
            key,
        })
    }

    /// Revokes the key `id` (no error when it is already revoked).
    pub fn revoke_key(&self, id: u64) -> Result<McpView, McpError> {
        let _write = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let before = self.keys.export();
        if !self.keys.revoke(id) {
            return Err(McpError::new(
                ErrorCode::Invalid,
                "That key does not exist any more. Reload the page.",
                false,
            ));
        }
        if let Err(e) = self.persist_keys() {
            self.keys.import(before);
            return Err(e);
        }
        Ok(self.view())
    }

    /// The recent calls, newest first.
    pub fn audit(&self) -> Vec<AuditRow> {
        self.audit
            .entries()
            .into_iter()
            .rev()
            .map(|e| AuditRow {
                at_ms: e.at_ms,
                client: e.client,
                tool: e.tool,
                arguments: e.arguments,
                result_bytes: e.result_bytes as u64,
                duration_ms: e.duration_ms,
                outcome: e.outcome,
            })
            .collect()
    }

    /// Clears the audit log and tells the windows.
    pub fn clear_audit(&self) {
        self.audit.clear();
        (self.emit)(&McpEvent::Audit { count: 0 });
    }

    /// The server, for tests of the adapter.
    #[cfg(test)]
    pub(crate) fn server_for_tests(&self) -> &Server {
        &self.server
    }

    /// The key store, to give to [`crate::serve_loopback`].
    pub fn keys(&self) -> Arc<KeyStore> {
        self.keys.clone()
    }
}
