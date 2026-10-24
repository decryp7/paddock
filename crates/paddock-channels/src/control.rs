//! The engine side of a Settings page for channels: load and save the settings, keep the secrets in the credential store, send a
//! test message as a background job and report its outcome as an event. Pure Rust; the Tauri adapter ([`crate::tauri`]) only
//! maps its commands and event onto this.
//!
//! Every method returns at once except [`ChannelsControl::get`] and [`ChannelsControl::save`] (a file and the credential store,
//! both quick and bounded); a test runs on its own thread and ends with a [`ChannelsEvent::Test`] (rule 10). Nothing here times
//! a running test out (rule 12); [`ChannelsControl::cancel`] stops them.

use crate::config::{ChannelConfig, Settings, secret_name_of};
use crate::dispatch::{Delivery, Dispatcher};
use crate::message::Message;
use crate::{HttpPoster, MailSender};
use paddock_env::VersionedFile;
use paddock_jobs::CancelToken;
use paddock_jobs::time::Clock;
use paddock_secrets::{Secret, Secrets};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Most tests that may run at once.
pub const MAX_TESTS: usize = 4;

/// Where the settings live. The app implements it over its own settings file; [`FileSettings`] is the shared one.
pub trait SettingsStore: Send + Sync {
    /// The saved settings (the defaults when there are none or the file is unusable).
    fn load(&self) -> Settings;
    /// Saves the settings; the message says what to fix.
    fn save(&self, settings: &Settings) -> Result<(), String>;
}

/// [`SettingsStore`] over a versioned JSON file (atomic writes, a damaged file kept aside, see `paddock-env`).
pub struct FileSettings(VersionedFile<Settings>);

impl FileSettings {
    /// The settings in `path` (for example `dirs.config().join("channels.json")`).
    pub fn new(path: impl Into<PathBuf>) -> FileSettings {
        FileSettings(VersionedFile::new(path, 1))
    }
}

impl SettingsStore for FileSettings {
    fn load(&self) -> Settings {
        self.0.load().value
    }

    fn save(&self, settings: &Settings) -> Result<(), String> {
        self.0
            .save(settings)
            .map_err(|e| format!("The notification settings could not be saved: {e}"))
    }
}

/// [`SettingsStore`] in memory, for tests.
#[derive(Default)]
pub struct MemorySettings {
    value: Mutex<Settings>,
    /// When set, [`SettingsStore::save`] fails with this message.
    pub fail: Mutex<Option<String>>,
}

impl SettingsStore for MemorySettings {
    fn load(&self) -> Settings {
        self.value
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn save(&self, settings: &Settings) -> Result<(), String> {
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

/// What the page shows: the settings, and for every channel whether its secret is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsView {
    /// The saved settings.
    pub settings: Settings,
    /// Per channel id: a secret is stored (only channels that need one are listed).
    pub secret_set: BTreeMap<String, bool>,
}

/// Why a command did not work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The settings or the secret are not acceptable; the message says what to fix.
    Invalid,
    /// The credential store or the settings file could not be used.
    StoreUnavailable,
    /// A test of this channel runs already, or too many run.
    Busy,
    /// Anything else.
    Failed,
}

/// A refused command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsError {
    /// The kind.
    pub code: ErrorCode,
    /// What to do, for the user.
    pub message: String,
    /// Whether trying again can help.
    pub retryable: bool,
}

impl ChannelsError {
    fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> ChannelsError {
        ChannelsError {
            code,
            message: message.into(),
            retryable,
        }
    }
}

/// What the engine reports on `paddock://channels`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChannelsEvent {
    /// A test send ended.
    Test {
        /// The id [`ChannelsControl::test`] returned.
        test_id: u64,
        /// The channel.
        channel_id: String,
        /// Delivered.
        ok: bool,
        /// Attempts made.
        attempts: u32,
        /// Why not, in words the user can act on.
        message: Option<String>,
    },
}

/// Sends events to the windows.
pub type Emit = Arc<dyn Fn(&ChannelsEvent) + Send + Sync>;

struct Tests {
    next: u64,
    running: HashMap<String, CancelToken>,
}

/// The channels of an app: settings, secrets and tests.
pub struct ChannelsControl {
    app: String,
    store: Arc<dyn SettingsStore>,
    secrets: Arc<dyn Secrets>,
    poster: Arc<dyn HttpPoster>,
    mailer: Arc<dyn MailSender>,
    clock: Arc<dyn Clock>,
    emit: Emit,
    tests: Mutex<Tests>,
    /// Serialises save, set_secret and clear_secret so a save and a secret change cannot interleave.
    write: Mutex<()>,
}

impl ChannelsControl {
    /// A control for the app called `app` (its display name, used in test messages).
    pub fn new(
        app: &str,
        store: Arc<dyn SettingsStore>,
        secrets: Arc<dyn Secrets>,
        poster: Arc<dyn HttpPoster>,
        mailer: Arc<dyn MailSender>,
        clock: Arc<dyn Clock>,
        emit: Emit,
    ) -> Arc<ChannelsControl> {
        Arc::new(ChannelsControl {
            app: app.to_string(),
            store,
            secrets,
            poster,
            mailer,
            clock,
            emit,
            tests: Mutex::new(Tests {
                next: 1,
                running: HashMap::new(),
            }),
            write: Mutex::new(()),
        })
    }

    fn tests(&self) -> MutexGuard<'_, Tests> {
        self.tests.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn store_error(what: &str, e: impl std::fmt::Display) -> ChannelsError {
        ChannelsError::new(
            ErrorCode::StoreUnavailable,
            format!("{what} ({e}). Unlock the credential store and try again."),
            true,
        )
    }

    fn view(&self, settings: Settings) -> Result<ChannelsView, ChannelsError> {
        let mut secret_set = BTreeMap::new();
        for c in &settings.channels {
            if let Some(name) = c.secret_name() {
                let stored = self
                    .secrets
                    .get(&name)
                    .map_err(|e| Self::store_error("The stored secrets could not be read", e))?;
                secret_set.insert(c.id.clone(), stored.is_some());
            }
        }
        Ok(ChannelsView {
            settings,
            secret_set,
        })
    }

    /// The saved settings and which secrets are stored.
    pub fn get(&self) -> Result<ChannelsView, ChannelsError> {
        self.view(self.store.load())
    }

    /// The current dispatcher, for the app's alerts (`dispatcher().dispatch(&message, &cancel)` from a job).
    pub fn dispatcher(&self) -> Dispatcher {
        Dispatcher::new(
            self.store.load(),
            self.secrets.clone(),
            self.poster.clone(),
            self.mailer.clone(),
            self.clock.clone(),
        )
    }

    /// Sends `message` to the channels that want it (blocks: call it from a job).
    pub fn dispatch(&self, message: &Message, cancel: &CancelToken) -> Vec<Delivery> {
        self.dispatcher().dispatch(message, cancel)
    }

    /// Validates and saves the settings. The secrets of channels that were removed, or no longer need one, are deleted after the
    /// save succeeded (a failed save keeps everything as it was).
    pub fn save(&self, settings: Settings) -> Result<ChannelsView, ChannelsError> {
        settings
            .validate()
            .map_err(|e| ChannelsError::new(ErrorCode::Invalid, e.0, false))?;
        let _write = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let before = self.store.load();
        self.store
            .save(&settings)
            .map_err(|m| ChannelsError::new(ErrorCode::StoreUnavailable, m, true))?;
        for old in &before.channels {
            let still_needed = settings
                .channel(&old.id)
                .is_some_and(|c| c.secret_name().is_some());
            if !still_needed {
                // A secret that cannot be deleted now is deleted by the next save; the settings are already right.
                let _ = old.forget_secret(self.secrets.as_ref());
            }
        }
        self.view(settings)
    }

    /// Stores the token or password of channel `id` (called by the secret dialog's window). The channel need not be saved yet.
    pub fn set_secret(&self, id: &str, value: &str) -> Result<(), ChannelsError> {
        let name = secret_name_of(id).ok_or_else(|| {
            ChannelsError::new(
                ErrorCode::Invalid,
                "That channel id cannot have a secret.",
                false,
            )
        })?;
        let secret = Secret::new(value)
            .map_err(|e| ChannelsError::new(ErrorCode::Invalid, e.to_string(), false))?;
        let _write = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        self.secrets
            .set(&name, &secret)
            .map_err(|e| Self::store_error("The secret could not be saved", e))
    }

    /// Removes the secret of channel `id` (no error when there is none).
    pub fn clear_secret(&self, id: &str) -> Result<(), ChannelsError> {
        let name = secret_name_of(id).ok_or_else(|| {
            ChannelsError::new(
                ErrorCode::Invalid,
                "That channel id cannot have a secret.",
                false,
            )
        })?;
        let _write = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        self.secrets
            .delete(&name)
            .map_err(|e| Self::store_error("The secret could not be removed", e))
    }

    /// Starts a test send of `channel` (saved or not, enabled or not) with one attempt, returns its id at once and reports the
    /// outcome as a [`ChannelsEvent::Test`]. A channel is tested once at a time, and at most [`MAX_TESTS`] run at once.
    pub fn test(self: &Arc<Self>, channel: ChannelConfig) -> Result<u64, ChannelsError> {
        let (id, cancel) = {
            let mut t = self.tests();
            if t.running.contains_key(&channel.id) {
                return Err(ChannelsError::new(
                    ErrorCode::Busy,
                    "A test of this channel is running; wait for its result.",
                    true,
                ));
            }
            if t.running.len() >= MAX_TESTS {
                return Err(ChannelsError::new(
                    ErrorCode::Busy,
                    "Too many tests are running; wait for one to finish.",
                    true,
                ));
            }
            let id = t.next;
            t.next += 1;
            let cancel = CancelToken::new();
            t.running.insert(channel.id.clone(), cancel.clone());
            (id, cancel)
        };
        let me = self.clone();
        let spawned = std::thread::Builder::new()
            .name("channels-test".into())
            .spawn({
                let channel = channel.clone();
                move || {
                    let delivery = me.dispatcher().test_with(&channel, None, &me.app, &cancel);
                    me.tests().running.remove(&channel.id);
                    let (ok, message) = match &delivery.result {
                        Ok(()) => (true, None),
                        Err(e) => (false, Some(e.to_string())),
                    };
                    (me.emit)(&ChannelsEvent::Test {
                        test_id: id,
                        channel_id: channel.id.clone(),
                        ok,
                        attempts: delivery.attempts,
                        message,
                    });
                }
            });
        match spawned {
            Ok(_) => Ok(id),
            Err(e) => {
                self.tests().running.remove(&channel.id);
                Err(ChannelsError::new(
                    ErrorCode::Failed,
                    format!("The test could not be started ({e})."),
                    true,
                ))
            }
        }
    }

    /// Stops every running test (at app exit); each one still reports.
    pub fn cancel(&self) {
        for token in self.tests().running.values() {
            token.cancel();
        }
    }

    /// How many tests run now.
    pub fn running(&self) -> usize {
        self.tests().running.len()
    }
}
