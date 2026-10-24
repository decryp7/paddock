//! Outbound notification channels for the Paddock apps (plan X17): webhook, ntfy and e-mail, from Pitcrew's
//! `notify-channels` and Spotter's `mail.rs`.
//!
//! - [`Settings`] / [`ChannelConfig`] / [`Transport`]: what the user configures (off by default, at most [`MAX_CHANNELS`]),
//!   checked once by [`Settings::validate`] with a message that says what to fix. Settings never hold a secret: the token or
//!   password is a `paddock-secrets` entry named by [`ChannelConfig::secret_name`] (scope `channel:<id>`, decision D6).
//! - [`Message`]: title, body, [`Severity`], kind and source; sent bounded ([`MAX_TITLE_CHARS`], [`MAX_BODY_BYTES`]).
//! - [`Dispatcher`]: sends a message to every enabled channel that wants it, in parallel (one thread per channel, results in
//!   channel order), retrying transient failures (no connection, timeout, 429, 5xx, a 4xx mail reply) with the family's
//!   back-off on an injected [`Clock`](paddock_jobs::time::Clock); [`Dispatcher::test`] and [`Dispatcher::test_with`] send one
//!   test message without retries, the second for a channel not saved yet.
//! - Two ports keep the network out of the core: [`HttpPoster`] (real: [`UreqPoster`], feature `http`, certificates from the
//!   operating system and the proxy settings of `paddock-download`) and [`MailSender`] (real: [`LettreSender`], feature `smtp`).
//!   Tests use [`FakePoster`] / [`FakeMail`] or an in-process loopback server; nothing here reaches a real network in tests.
//! - Redirects are never followed (a redirect could leak the message or token to another host; it counts as a rejection).
//!
//! Dispatch blocks (rule 10): call it from a job or worker thread, never the UI or an IPC handler.

mod config;
mod dispatch;
mod error;
mod fake;
mod http;
mod mail;
mod message;

pub use config::{
    ChannelConfig, ConfigError, MAX_CHANNELS, MAX_EVENTS, MAX_RECIPIENTS, Security, Settings,
    Transport,
};
pub use dispatch::{DEFAULT_TIMEOUT, Delivery, Dispatcher, RetryPolicy, TEST_KIND};
pub use error::SendError;
pub use fake::{FakeMail, FakePoster};
#[cfg(feature = "http")]
pub use http::UreqPoster;
pub use http::{HttpPoster, MAX_HEADER_VALUE, PostRequest, PostResponse};
#[cfg(feature = "smtp")]
pub use mail::LettreSender;
pub use mail::{Login, MailRequest, MailSender};
pub use message::{MAX_BODY_BYTES, MAX_TITLE_CHARS, Message, Severity};
