//! What the user configures: channels, their transports and the checks that run when settings are saved.

use crate::message::{Message, Severity};
use paddock_secrets::{Name, Secrets, SecretsError};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Most channels a [`Settings`] value may hold.
pub const MAX_CHANNELS: usize = 8;
/// Most event kinds one channel may subscribe to.
pub const MAX_EVENTS: usize = 64;
/// Most recipients of one e-mail channel.
pub const MAX_RECIPIENTS: usize = 10;
const MAX_TEXT: usize = 200;
const MAX_URL: usize = 2048;

/// How an SMTP connection is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Security {
    /// A plain connection upgraded with STARTTLS, which is required (usually port 587).
    #[default]
    StartTls,
    /// TLS from the first byte (usually port 465).
    Implicit,
    /// No encryption at all: only for a relay on a trusted network. The password would travel in the clear.
    None,
}

/// Where and how a channel delivers. Secrets are never part of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Transport {
    /// POST the message as JSON.
    Webhook {
        /// Target URL (https, or http for a service on the local network).
        url: String,
        /// Send the stored token as `Authorization: Bearer <token>`.
        #[serde(default)]
        auth: bool,
    },
    /// Publish to an ntfy topic.
    Ntfy {
        /// Server base URL.
        #[serde(default = "default_ntfy_server")]
        server: String,
        /// Topic name (`A-Za-z0-9_-`).
        topic: String,
        /// Send the stored access token as `Authorization: Bearer <token>`.
        #[serde(default)]
        auth: bool,
    },
    /// Send an e-mail through an SMTP relay.
    Email {
        /// SMTP host.
        host: String,
        /// SMTP port.
        port: u16,
        /// Connection security.
        #[serde(default)]
        security: Security,
        /// Login name; `None` for a relay that needs no login (then no password is used).
        #[serde(default)]
        username: Option<String>,
        /// Sender address.
        from: String,
        /// Recipient addresses.
        to: Vec<String>,
    },
}

fn default_ntfy_server() -> String {
    "https://ntfy.sh".to_string()
}

/// One configured channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConfig {
    /// Unique id within the settings (`A-Za-z0-9._-`, at most 64): also the credential store scope.
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
        self.enabled
            && msg.severity >= self.min_severity
            && self.events.iter().any(|e| e == &msg.kind)
    }

    /// The name this channel's token or password has in the credential store (scope `channel:<id>`, key `secret`), or `None` when the channel needs none (no `auth`, or an e-mail relay without a login). `None` too when the
    /// id is not usable, which [`Settings::validate`] reports.
    pub fn secret_name(&self) -> Option<Name> {
        match &self.transport {
            Transport::Webhook { auth: true, .. }
            | Transport::Ntfy { auth: true, .. }
            | Transport::Email {
                username: Some(_), ..
            } => secret_name_of(&self.id),
            _ => None,
        }
    }

    /// Removes the channel's secret (when the channel is deleted or no longer needs one).
    pub fn forget_secret(&self, secrets: &dyn Secrets) -> Result<(), SecretsError> {
        self.secret_name().map_or(Ok(()), |n| secrets.delete(&n))
    }

    /// The name to show for the channel in messages.
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }

    fn validate(&self) -> Result<(), String> {
        check_id(&self.id)?;
        check_text("Channel name", &self.name, 80)?;
        if self.events.len() > MAX_EVENTS {
            return Err(format!(
                "A channel can subscribe to at most {MAX_EVENTS} events"
            ));
        }
        for e in &self.events {
            check_text("Event kind", e, 64)?;
        }
        match &self.transport {
            Transport::Webhook { url, .. } => check_http_url("Webhook URL", url),
            Transport::Ntfy { server, topic, .. } => {
                check_http_url("ntfy server", server)?;
                if topic.is_empty()
                    || topic.len() > 64
                    || !topic
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                {
                    return Err(
                        "ntfy topic must be 1-64 characters of letters, digits, '_' or '-'".into(),
                    );
                }
                Ok(())
            }
            Transport::Email {
                host,
                port,
                username,
                from,
                to,
                ..
            } => {
                check_host(host)?;
                if *port == 0 {
                    return Err("SMTP port must not be 0".into());
                }
                if let Some(u) = username {
                    check_text("SMTP username", u, MAX_TEXT)?;
                    if u.trim().is_empty() {
                        return Err(
                            "SMTP username must not be empty; clear it for a relay without a login"
                                .into(),
                        );
                    }
                }
                check_address("Sender", from)?;
                if to.is_empty() {
                    return Err("Add at least one recipient".into());
                }
                if to.len() > MAX_RECIPIENTS {
                    return Err(format!("At most {MAX_RECIPIENTS} recipients are allowed"));
                }
                to.iter().try_for_each(|a| check_address("Recipient", a))
            }
        }
    }
}

/// The credential store name of the token or password of channel `id` (scope `channel:<id>`, key `secret`); `None` when the id
/// does not make a valid name. Used where only the id is known (the secret dialog).
pub fn secret_name_of(id: &str) -> Option<Name> {
    Name::new(&format!("channel:{id}"), "secret").ok()
}

/// All notification settings of an app. The default has no channels, so nothing is ever sent unless configured.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Configured channels (at most [`MAX_CHANNELS`]).
    #[serde(default)]
    pub channels: Vec<ChannelConfig>,
}

/// A setting the user must fix; the message says which and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

impl Settings {
    /// Checks the settings (call it when the user saves them) and names the first thing to fix.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.channels.len() > MAX_CHANNELS {
            return Err(ConfigError(format!(
                "At most {MAX_CHANNELS} notification channels are allowed"
            )));
        }
        for (i, c) in self.channels.iter().enumerate() {
            if self.channels[..i].iter().any(|p| p.id == c.id) {
                return Err(ConfigError(format!("Channel id '{}' is used twice", c.id)));
            }
            c.validate()
                .map_err(|e| ConfigError(format!("{}: {e}", c.label())))?;
        }
        Ok(())
    }

    /// The channel with this id.
    pub fn channel(&self, id: &str) -> Option<&ChannelConfig> {
        self.channels.iter().find(|c| c.id == id)
    }
}

fn check_text(what: &str, value: &str, max: usize) -> Result<(), String> {
    if value.chars().count() > max {
        return Err(format!("{what} is too long (at most {max} characters)"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{what} contains control characters"));
    }
    Ok(())
}

fn check_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(
            "Channel id must be 1-64 characters of letters, digits, '.', '_' or '-'".into(),
        );
    }
    Ok(())
}

fn check_http_url(what: &str, url: &str) -> Result<(), String> {
    check_text(what, url, MAX_URL)?;
    let (plain, rest) = match url.strip_prefix("https://") {
        Some(rest) => (false, rest),
        None => match url.strip_prefix("http://") {
            Some(rest) => (true, rest),
            None => {
                return Err(format!(
                    "{what} must start with https:// (http:// is only for this computer)"
                ));
            }
        },
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.starts_with(':') || host.contains(char::is_whitespace) {
        return Err(format!("{what} needs a valid host name"));
    }
    if host.contains('@') {
        return Err(format!(
            "{what} must not contain a login; use the token field"
        ));
    }
    // A bracketed IPv6 literal keeps its colons; any other host loses its port.
    let name = if host.starts_with('[') {
        host.split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    if plain && !paddock_net::host_is_loopback(name) {
        return Err(format!(
            "{what} must use https:// (plain http is only for this computer)"
        ));
    }
    Ok(())
}

fn check_host(host: &str) -> Result<(), String> {
    check_text("SMTP host", host, 255)?;
    if host.is_empty() || host.contains(|c: char| c.is_whitespace() || c == '/' || c == '@') {
        return Err("SMTP host must be a host name or address".into());
    }
    Ok(())
}

/// Whether `addr` looks like `name@domain.tld` (one `@`, a dot in the domain, no spaces); the mail library parses it for real.
pub(crate) fn is_address(addr: &str) -> bool {
    let addr = addr.trim();
    match addr.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !domain.contains('@')
                && !addr.contains(char::is_whitespace)
                && addr.len() <= 254
        }
        None => false,
    }
}

fn check_address(what: &str, addr: &str) -> Result<(), String> {
    check_text(what, addr, 254)?;
    if is_address(addr) {
        Ok(())
    } else {
        Err(format!("{what} '{addr}' is not a valid email address"))
    }
}
