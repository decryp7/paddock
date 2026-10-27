//! Channel trait and the dispatcher that routes a message to the channels that want it.

use crate::http::{NtfyChannel, WebhookChannel};
use crate::model::{ChannelConfig, Message, SecretStore, SendError, Settings, Transport};
use crate::smtp::EmailChannel;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Duration;

/// A delivery mechanism for messages.
pub trait Channel {
    /// The id of the configured channel.
    fn id(&self) -> &str;
    /// Delivers `msg`, reading any secret from `secrets`.
    fn send(&self, msg: &Message, secrets: &dyn SecretStore) -> Result<(), SendError>;
}

/// Default network timeout per channel.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

fn build(cfg: &ChannelConfig, timeout: Duration) -> Box<dyn Channel> {
    match &cfg.transport {
        Transport::Webhook { url, secret_key } => Box::new(WebhookChannel::new(&cfg.id, url, secret_key.clone(), timeout)),
        Transport::Ntfy { server, topic, secret_key } => Box::new(NtfyChannel::new(&cfg.id, server, topic, secret_key.clone(), timeout)),
        Transport::Email { .. } => Box::new(EmailChannel::new(cfg, timeout)),
    }
}

/// Sends messages to the configured channels.
#[derive(Debug, Clone)]
pub struct Dispatcher {
    settings: Settings,
    timeout: Duration,
}

impl Dispatcher {
    /// A dispatcher with the default 15 second timeout per channel.
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Overrides the per-channel timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sends `msg` to every enabled channel subscribed to its kind whose minimum severity it reaches. Returns one
    /// result per attempted channel; never panics. Blocks, so call it from a background thread.
    pub fn dispatch(&self, msg: &Message, secrets: &dyn SecretStore) -> Vec<(String, Result<(), SendError>)> {
        self.settings
            .channels
            .iter()
            .filter(|c| c.wants(msg))
            .map(|c| {
                let result =
                    catch_unwind(AssertUnwindSafe(|| build(c, self.timeout).send(msg, secrets))).unwrap_or(Err(SendError::Panicked));
                (c.id.clone(), result)
            })
            .collect()
    }
}
