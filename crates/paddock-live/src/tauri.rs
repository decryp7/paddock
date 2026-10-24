//! The Tauri adapter of `paddock-live` (feature `tauri`).
//!
//! - [`register`] gives a [`Hub`] a channel name: a thread forwards every frame to all webviews as the event
//!   `paddock://live/<channel>` ([`Channel::event`]) until the hub is closed. Frames arrive in order; a window folds them
//!   with `@decryp7/paddock-ui/live`.
//! - [`COMMANDS`] is the adapter's command table: `live_snapshot(channel)` answers the whole state as a snapshot frame, for a
//!   window that opens, reloads or saw a gap. It is async and returns at once (rule 10). [`handler`], [`handler_with_grants`],
//!   [`invoke_handler`] and [`route`] are used as in `paddock_jobs::tauri`.
//!
//! ```ignore
//! tauri::Builder::default()
//!     .setup(|app| {
//!         let hub = paddock_live::Hub::new(Overview::default());
//!         paddock_live::tauri::register(app.handle(), "overview", &hub)?;
//!         // ... a thread runs `paddock_live::run(&hub, &mut source, &clock, default_policy(), &cancel)`
//!         Ok(())
//!     })
//!     .invoke_handler(paddock_live::tauri::route(
//!         paddock_live::tauri::handler(),
//!         paddock_commands::tauri::Guard::new(&APP_COMMANDS).wrap(app_handler!()),
//!     ))
//! ```

use crate::channel::{Channel, ChannelError};
use crate::hub::{Hub, LiveState};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager, Runtime};
use paddock_commands::DialogGrant;
use paddock_commands::tauri::Guard;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

trait Erased: Send + Sync {
    fn snapshot_json(&self) -> Result<Value, serde_json::Error>;
}

impl<S: LiveState> Erased for Hub<S> {
    fn snapshot_json(&self) -> Result<Value, serde_json::Error> {
        serde_json::to_value(self.snapshot())
    }
}

/// The feeds of the app as the command sees them: managed Tauri state.
#[derive(Default)]
pub struct Feeds(Mutex<HashMap<Channel, Arc<dyn Erased>>>);

impl Feeds {
    fn get(&self, channel: &Channel) -> Option<Arc<dyn Erased>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(channel)
            .cloned()
    }

    fn insert(&self, channel: Channel, feed: Arc<dyn Erased>) -> bool {
        let mut map = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if map.contains_key(&channel) {
            return false;
        }
        map.insert(channel, feed);
        true
    }
}

/// Why a feed could not be registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterError {
    /// The channel name is not valid.
    Channel(ChannelError),
    /// Another feed has the name.
    Duplicate,
    /// The forwarding thread could not start.
    Thread,
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Channel(e) => e.fmt(f),
            Self::Duplicate => f.write_str("a live feed with this name is already registered"),
            Self::Thread => f.write_str("the live feed's forwarding thread could not start"),
        }
    }
}

impl std::error::Error for RegisterError {}

/// Puts `hub` on `channel`: managed for `live_snapshot`, and a thread named `paddock-live-<channel>` emits its frames until the
/// hub is closed (then the thread ends; join the handle at shutdown after [`Hub::close`]).
///
/// # Errors
/// The name is not a [`Channel`], is taken, or the thread did not start.
pub fn register<R: Runtime, S: LiveState>(
    app: &AppHandle<R>,
    channel: &str,
    hub: &Hub<S>,
) -> Result<JoinHandle<()>, RegisterError> {
    let channel = Channel::new(channel).map_err(RegisterError::Channel)?;
    if app.try_state::<Feeds>().is_none() {
        app.manage(Feeds::default());
    }
    let feeds = app.state::<Feeds>();
    let event = channel.event();
    // Subscribed before it is visible to the command, so the first frame the window sees is never newer than the snapshot it asks for.
    let sub = hub.subscribe();
    if !feeds.insert(channel.clone(), Arc::new(hub.clone())) {
        return Err(RegisterError::Duplicate);
    }
    let app = app.clone();
    std::thread::Builder::new()
        .name(format!("paddock-live-{}", channel.as_str()))
        .spawn(move || {
            while let Some(frame) = sub.next() {
                // A failed emit (the app is shutting down) is dropped: the next frame or a snapshot catches a window up.
                let _ = app.emit(&event, &frame);
            }
        })
        .map_err(|_| RegisterError::Thread)
}

/// Why a live command could not answer. On the wire `{ code, message, retryable }`, like a command refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandError {
    /// No feed was registered ([`register`]): a bug in the app.
    NotSetUp,
    /// The channel name is not valid.
    BadChannel,
    /// No feed has this name.
    UnknownChannel,
    /// The state could not be encoded.
    Encode,
}

impl CommandError {
    /// The stable code on the wire.
    pub fn code(self) -> &'static str {
        match self {
            Self::NotSetUp => "notSetUp",
            Self::BadChannel => "badChannel",
            Self::UnknownChannel => "unknownChannel",
            Self::Encode => "encode",
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotSetUp => "The live feed is not set up in this app",
            Self::BadChannel => "That is not a live channel name",
            Self::UnknownChannel => "There is no live feed with that name",
            Self::Encode => "The live state could not be encoded",
        })
    }
}

impl std::error::Error for CommandError {}

impl Serialize for CommandError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("CommandError", 3)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.serialize_field("retryable", &false)?;
        st.end()
    }
}

/// The whole state of feed `channel` as a snapshot frame (a window that opens, reloads or saw a gap in the sequence).
#[::tauri::command]
pub async fn live_snapshot<R: Runtime>(
    app: AppHandle<R>,
    channel: String,
) -> Result<Value, CommandError> {
    let feeds = app.try_state::<Feeds>().ok_or(CommandError::NotSetUp)?;
    let channel = Channel::new(&channel).map_err(|_| CommandError::BadChannel)?;
    feeds
        .get(&channel)
        .ok_or(CommandError::UnknownChannel)?
        .snapshot_json()
        .map_err(|_| CommandError::Encode)
}

paddock_commands::command_table! {
    /// The commands of the live adapter: callable from every window class (a dialog only when its contract names them).
    pub static COMMANDS, macro generated_handler;
    Query [WINDOW] => live_snapshot,
}

/// The invoke handler of [`COMMANDS`] without a guard: for a guard of the app's or the shell plugin's own.
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    generated_handler!()
}

/// The guarded invoke handler of [`COMMANDS`]. Dialog windows may call nothing of it; use [`handler_with_grants`] to let the
/// dialogs whose contracts name `live_snapshot` call it.
pub fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    Guard::new(&COMMANDS).wrap(generated_handler!())
}

/// As [`handler`], with the per-dialog contracts (ADR 0009): `lookup(app, label)` gives the grant of dialog `label`.
pub fn handler_with_grants<R: Runtime, G: DialogGrant + 'static>(
    lookup: impl Fn(&AppHandle<R>, &str) -> Option<G> + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    Guard::new(&COMMANDS)
        .dialog_grants(lookup)
        .wrap(generated_handler!())
}

/// One invoke handler of two: the commands of [`COMMANDS`] go to `live`, everything else to `other` (the app's).
pub fn route<R: Runtime>(
    live: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
    other: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke: Invoke<R>| {
        if COMMANDS.spec(invoke.message.command()).is_some() {
            live(invoke)
        } else {
            other(invoke)
        }
    }
}
