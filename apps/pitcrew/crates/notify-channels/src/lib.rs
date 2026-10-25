//! Notification channels (webhook, ntfy, email) for the suite's apps.
//!
//! This module is self-contained and meant to be copied into an app (Spotter, Dyno, Debrief, Pitcrew). To wire it:
//!
//! 1. Keep a [`Settings`] value in the app's own settings file. It is off by default (no channels) and never holds
//!    secrets: each channel only names a keychain entry in `secret_key`.
//! 2. Implement [`SecretStore`] over the OS keychain and hand it to the dispatcher.
//! 3. Call [`Dispatcher::dispatch`] with a [`Message`] from a background thread (it blocks up to the timeout per
//!    channel). Call [`Settings::validate`] when the user saves the settings and show its message.

mod dispatch;
mod http;
mod model;
mod smtp;

pub use dispatch::{Channel, Dispatcher};
pub use model::{ChannelConfig, MemorySecrets, Message, SecretStore, SendError, Settings, Severity, Tls, Transport, MAX_CHANNELS};

#[cfg(test)]
mod tests;
