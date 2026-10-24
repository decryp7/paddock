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
//! - Two ports keep the network out of the core: [`HttpPoster`] (real: [`EgressPoster`] over `paddock-net`: https only, reach
//!   limits, offline mode, certificates from the operating system, the proxy settings of `paddock-download`, connection reuse) and [`MailSender`] (real: [`LettreSender`], feature `smtp`).
//!   Tests use [`FakePoster`] / [`FakeMail`] or an in-process loopback server; nothing here reaches a real network in tests.
//! - Redirects are never followed (a redirect could leak the message or token to another host; it counts as a rejection).
//!
//! Dispatch blocks (rule 10): call it from a job or worker thread, never the UI or an IPC handler.

mod config;
mod control;
mod dispatch;
mod error;
mod fake;
mod http;
mod mail;
mod message;

pub use config::{
    ChannelConfig, ConfigError, MAX_CHANNELS, MAX_EVENTS, MAX_RECIPIENTS, Security, Settings,
    Transport, secret_name_of,
};
pub use control::{
    ChannelsControl, ChannelsError, ChannelsEvent, ChannelsView, Emit, ErrorCode, FileSettings,
    MAX_TESTS, MemorySettings, SettingsStore, TestPhase, TestStarted,
};
pub use dispatch::{
    DEFAULT_TIMEOUT, Delivery, Dispatcher, Phase, Progress, RetryPolicy, TEST_KIND,
};
pub use error::SendError;
pub use fake::{FakeMail, FakePoster};
pub use http::{
    EgressPoster, HttpPoster, MAX_HEADER_VALUE, PostRequest, PostResponse, channel_policy,
};
#[cfg(feature = "smtp")]
pub use mail::LettreSender;
pub use mail::{Login, MailRequest, MailSender};
pub use message::{MAX_BODY_BYTES, MAX_TITLE_CHARS, Message, Severity};

/// The event the channels adapter sends every update on, to every window (a [`ChannelsEvent`]).
pub const EVENT_NAME: &str = "paddock://channels";

paddock_commands::command_table! {
    /// The commands of the channels adapter (`paddock_channels::tauri::invoke_handler` serves them; the shell plugin folds them
    /// into its table). Every window may look; main windows and the Settings dialog change it; only the secret dialog's window
    /// (a DIALOG) and the Settings dialog store a secret.
    pub static COMMANDS;
    /// The settings and which secrets are stored.
    Query [WINDOW] => tauri::channels_get,
    /// Validates and saves the settings.
    Mutation [MAIN | DIALOG] => tauri::channels_save,
    /// Stores a channel's token or password (the secret dialog).
    Mutation [DIALOG] => tauri::channels_set_secret,
    /// Removes a channel's token or password.
    Mutation [MAIN | DIALOG] => tauri::channels_clear_secret,
    /// Starts a test send (returns at once; the outcome is an event).
    Mutation [MAIN | DIALOG] => tauri::channels_test,
}

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(test)]
mod generated;
