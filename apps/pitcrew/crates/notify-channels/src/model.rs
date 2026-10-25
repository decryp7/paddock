//! Message, configuration and validation types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Most channels a [`Settings`] value may hold.
pub const MAX_CHANNELS: usize = 8;
const MAX_TEXT: usize = 200;
const MAX_URL: usize = 2048;
const MAX_EVENTS: usize = 64;
const MAX_RECIPIENTS: usize = 10;

/// How important a message is. Ordered: `Info < Warning < Error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Informational.
    #[default]
    Info,
    /// Needs attention soon.
    Warning,
    /// Needs attention now.
    Error,
}

/// A notification to deliver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// Short headline (email subject, ntfy title).
    pub title: String,
    /// Details.
    pub body: String,
    /// Importance.
    pub severity: Severity,
    /// Event kind id such as `diskLow`; channels subscribe to kinds.
    pub kind: String,
    /// Who raised it, for example a runner name.
    pub source: String,
}

/// Read access to secrets (passwords, tokens). The app implements this over the OS keychain.
pub trait SecretStore {
    /// The secret stored under `key`, if any.
    fn get(&self, key: &str) -> Option<String>;
}

/// In-memory [`SecretStore`] for tests. Its `Debug` output hides the values.
#[derive(Default, Clone)]
pub struct MemorySecrets(HashMap<String, String>);

impl MemorySecrets {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces an entry.
    pub fn with(mut self, key: &str, value: &str) -> Self {
        self.0.insert(key.to_string(), value.to_string());
        self
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
}

impl fmt::Debug for MemorySecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MemorySecrets({} entries, values hidden)", self.0.len())
    }
}

/// Why a send failed. Messages never contain secrets.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    /// The channel names a secret the store does not have.
    #[error("secret '{0}' is not in the keychain; save it in the channel settings")]
    MissingSecret(String),
    /// The configuration cannot be used (bad address, bad URL).
    #[error("invalid channel configuration: {0}")]
    Invalid(String),
    /// The server could not be reached.
    #[error("could not connect: {0}")]
    Connect(String),
    /// The server did not answer in time.
    #[error("the server did not answer in time")]
    Timeout,
    /// The HTTP server answered with a non-2xx status (redirects count as rejections).
    #[error("the server rejected the message (HTTP {0})")]
    Rejected(u16),
    /// The mail server refused or broke the conversation.
    #[error("mail delivery failed: {0}")]
    Smtp(String),
    /// The channel implementation panicked; the app keeps running.
    #[error("the channel failed unexpectedly")]
    Panicked,
}

/// How an SMTP connection is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tls {
    /// Plain connection upgraded with STARTTLS (usually port 587).
    StartTls,
    /// TLS from the first byte (usually port 465).
    Implicit,
}

fn default_ntfy_server() -> String {
    "https://ntfy.sh".to_string()
}

/// Where and how a channel delivers. Secrets are referenced by keychain entry name only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Transport {
    /// POST the message as JSON.
    Webhook {
        /// Target URL (http or https).
        url: String,
        /// Keychain entry sent as `Authorization: Bearer <secret>`, if any.
        #[serde(default)]
        secret_key: Option<String>,
    },
    /// Publish to an ntfy topic.
    Ntfy {
        /// Server base URL.
        #[serde(default = "default_ntfy_server")]
        server: String,
        /// Topic name (`A-Za-z0-9_-`).
        topic: String,
        /// Keychain entry with an access token, if any.
        #[serde(default)]
        secret_key: Option<String>,
    },
    /// Send an email through an SMTP relay.
    Email {
        /// SMTP host.
        host: String,
        /// SMTP port.
        port: u16,
        /// Connection security.
        tls: Tls,
        /// Login name.
        username: String,
        /// Keychain entry holding the password.
        secret_key: String,
        /// Sender address.
        from: String,
        /// Recipient addresses.
        to: Vec<String>,
    },
}

/// One configured channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConfig {
    /// Unique id within the settings.
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Off by default.
    #[serde(default)]
    pub enabled: bool,
    /// Event kinds this channel receives; empty means none.
    #[serde(default)]
    pub events: Vec<String>,
    /// Messages below this severity are skipped.
    #[serde(default)]
    pub min_severity: Severity,
    /// Delivery details.
    pub transport: Transport,
}

impl ChannelConfig {
    /// Whether this channel should receive `msg`.
    pub fn wants(&self, msg: &Message) -> bool {
        self.enabled && msg.severity >= self.min_severity && self.events.iter().any(|e| e == &msg.kind)
    }
}

/// All notification settings of an app. The default has no channels, so nothing is ever sent unless configured.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Configured channels (at most [`MAX_CHANNELS`]).
    #[serde(default)]
    pub channels: Vec<ChannelConfig>,
}

fn check_len(what: &str, value: &str, max: usize) -> Result<(), String> {
    if value.chars().count() > max {
        return Err(format!("{what} is too long (at most {max} characters)"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{what} contains control characters"));
    }
    Ok(())
}

fn check_http_url(what: &str, url: &str) -> Result<(), String> {
    check_len(what, url, MAX_URL)?;
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| format!("{what} must start with https:// or http://"))?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.starts_with(':') || host.contains(char::is_whitespace) || host.contains('@') {
        return Err(format!("{what} needs a valid host name"));
    }
    Ok(())
}

fn check_host(host: &str) -> Result<(), String> {
    check_len("SMTP host", host, 255)?;
    if host.is_empty() || host.contains(|c: char| c.is_whitespace() || c == '/' || c == '@') {
        return Err("SMTP host must be a host name or address".to_string());
    }
    Ok(())
}

fn check_address(what: &str, addr: &str) -> Result<(), String> {
    check_len(what, addr, 254)?;
    let ok = match addr.split_once('@') {
        Some((local, domain)) => !local.is_empty() && domain.contains('.') && !domain.contains('@') && !addr.contains(char::is_whitespace),
        None => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("{what} '{addr}' is not a valid email address"))
    }
}

fn check_secret_key(key: &str) -> Result<(), String> {
    check_len("Secret name", key, 128)?;
    if key.is_empty() {
        return Err("Secret name must not be empty".to_string());
    }
    Ok(())
}

impl ChannelConfig {
    fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() {
            return Err("Channel id must not be empty".to_string());
        }
        check_len("Channel id", &self.id, 64)?;
        check_len("Channel name", &self.name, 80)?;
        if self.events.len() > MAX_EVENTS {
            return Err(format!("A channel can subscribe to at most {MAX_EVENTS} events"));
        }
        for e in &self.events {
            check_len("Event kind", e, 64)?;
        }
        match &self.transport {
            Transport::Webhook { url, secret_key } => {
                check_http_url("Webhook URL", url)?;
                secret_key.as_deref().map_or(Ok(()), check_secret_key)
            }
            Transport::Ntfy { server, topic, secret_key } => {
                check_http_url("ntfy server", server)?;
                if topic.is_empty() || topic.len() > 64 || !topic.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                    return Err("ntfy topic must be 1-64 characters of letters, digits, '_' or '-'".to_string());
                }
                secret_key.as_deref().map_or(Ok(()), check_secret_key)
            }
            Transport::Email {
                host,
                port,
                username,
                secret_key,
                from,
                to,
                ..
            } => {
                check_host(host)?;
                if *port == 0 {
                    return Err("SMTP port must not be 0".to_string());
                }
                check_len("SMTP username", username, MAX_TEXT)?;
                check_secret_key(secret_key)?;
                check_address("Sender", from)?;
                if to.is_empty() {
                    return Err("Add at least one recipient".to_string());
                }
                if to.len() > MAX_RECIPIENTS {
                    return Err(format!("At most {MAX_RECIPIENTS} recipients are allowed"));
                }
                to.iter().try_for_each(|a| check_address("Recipient", a))
            }
        }
    }
}

impl Settings {
    /// Checks the settings and returns a message telling the user what to fix.
    pub fn validate(&self) -> Result<(), String> {
        if self.channels.len() > MAX_CHANNELS {
            return Err(format!("At most {MAX_CHANNELS} notification channels are allowed"));
        }
        for (i, c) in self.channels.iter().enumerate() {
            if self.channels[..i].iter().any(|p| p.id == c.id) {
                return Err(format!("Channel id '{}' is used twice", c.id));
            }
            c.validate().map_err(|e| {
                let label = if c.name.is_empty() { &c.id } else { &c.name };
                format!("{label}: {e}")
            })?;
        }
        Ok(())
    }
}
