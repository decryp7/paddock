//! Webhook and ntfy channels over HTTP(S).

use crate::dispatch::Channel;
use crate::model::{Message, SecretStore, SendError, Severity};
use std::time::Duration;

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .http_status_as_error(false)
        // A redirect could leak the message or token to another host, so none are followed (they count as rejections).
        .max_redirects(0)
        .build()
        .into()
}

fn secret(secrets: &dyn SecretStore, key: &Option<String>) -> Result<Option<String>, SendError> {
    match key {
        None => Ok(None),
        Some(k) => secrets.get(k).map(Some).ok_or_else(|| SendError::MissingSecret(k.clone())),
    }
}

fn map_err(e: ureq::Error) -> SendError {
    match e {
        ureq::Error::Timeout(_) => SendError::Timeout,
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => SendError::Timeout,
        ureq::Error::BadUri(_) | ureq::Error::Http(_) | ureq::Error::InvalidProxyUrl => SendError::Invalid("bad address".to_string()),
        other => SendError::Connect(other.to_string()),
    }
}

fn finish(resp: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<(), SendError> {
    let status = resp.map_err(map_err)?.status().as_u16();
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(SendError::Rejected(status))
    }
}

/// Header values must be a single line.
fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// POSTs the message as JSON.
pub(crate) struct WebhookChannel {
    id: String,
    url: String,
    secret_key: Option<String>,
    timeout: Duration,
}

impl WebhookChannel {
    pub(crate) fn new(id: &str, url: &str, secret_key: Option<String>, timeout: Duration) -> Self {
        Self {
            id: id.to_string(),
            url: url.to_string(),
            secret_key,
            timeout,
        }
    }
}

impl Channel for WebhookChannel {
    fn id(&self) -> &str {
        &self.id
    }

    fn send(&self, msg: &Message, secrets: &dyn SecretStore) -> Result<(), SendError> {
        let token = secret(secrets, &self.secret_key)?;
        let body = serde_json::to_string(msg).map_err(|e| SendError::Invalid(e.to_string()))?;
        let mut req = agent(self.timeout).post(&self.url).header("Content-Type", "application/json");
        if let Some(t) = token {
            req = req.header("Authorization", &format!("Bearer {}", one_line(&t)));
        }
        finish(req.send(body))
    }
}

/// Publishes to an ntfy topic.
pub(crate) struct NtfyChannel {
    id: String,
    url: String,
    secret_key: Option<String>,
    timeout: Duration,
}

impl NtfyChannel {
    pub(crate) fn new(id: &str, server: &str, topic: &str, secret_key: Option<String>, timeout: Duration) -> Self {
        Self {
            id: id.to_string(),
            url: format!("{}/{}", server.trim_end_matches('/'), topic),
            secret_key,
            timeout,
        }
    }
}

fn priority(s: Severity) -> &'static str {
    match s {
        Severity::Info => "3",
        Severity::Warning => "4",
        Severity::Error => "5",
    }
}

fn tags(s: Severity) -> &'static str {
    match s {
        Severity::Info => "information_source",
        Severity::Warning => "warning",
        Severity::Error => "rotating_light",
    }
}

impl Channel for NtfyChannel {
    fn id(&self) -> &str {
        &self.id
    }

    fn send(&self, msg: &Message, secrets: &dyn SecretStore) -> Result<(), SendError> {
        let token = secret(secrets, &self.secret_key)?;
        let mut req = agent(self.timeout)
            .post(&self.url)
            .header("Title", &one_line(&msg.title))
            .header("Priority", priority(msg.severity))
            .header("Tags", tags(msg.severity));
        if let Some(t) = token {
            req = req.header("Authorization", &format!("Bearer {}", one_line(&t)));
        }
        finish(req.send(msg.body.as_str()))
    }
}
