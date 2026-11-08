//! The Tauri adapter of `paddock-assistant` (built with the `tauri` feature; ADR 0012 §3).
//!
//! - [`manage`] puts the app's [`Assistant`] in the app state; every event of a run goes to every window as [`EVENT`]
//!   (`paddock://assistant`, an [`Envelope`](crate::Envelope)).
//! - [`COMMANDS`] is the adapter's command table (`<area>_<verb>`, ADR 0012): `assistant_send`, `assistant_stop`,
//!   `assistant_approve_tool`, `assistant_conversations`, `assistant_load`, `assistant_delete`, `assistant_info` and `assistant_context`. They are
//!   async and return at once; the turn itself runs on its own worker thread and answers through events (rule 10), the file
//!   work on the blocking pool. [`invoke_handler`] is their handler without a guard (for a shell plugin that folds
//!   [`COMMANDS`] into its table); [`handler`] guards it; [`route`] puts it in front of the app's own.
//!
//! ```ignore
//! tauri::Builder::default()
//!     .setup(|app| { paddock_assistant::tauri::manage(app.handle(), assistant); Ok(()) })
//!     .invoke_handler(paddock_assistant::tauri::route(paddock_assistant::tauri::handler(), app_handler))
//! ```

// `CommandError` is the family's one command error (plan X28); its size is its business, not this adapter's.
#![allow(clippy::result_large_err)]

use crate::approval::ApprovalChoice;
use crate::context::ContextUse;
use crate::conversation::{Conversation, ConversationSummary};
use crate::event::{EVENT as EVENT_NAME, Envelope, EventSink};
use crate::service::{Assistant, AssistantError, AssistantInfo, SendAck, SendRequest};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager, Runtime};
use paddock_commands::CommandError;
use paddock_commands::tauri::Guard;
use std::sync::Arc;

/// The event every window hears while a run goes on.
pub const EVENT: &str = EVENT_NAME;

/// Sends every event to all webviews. A failed emit (the app is closing) is dropped.
pub struct Emit<R: Runtime>(AppHandle<R>);

impl<R: Runtime> Emit<R> {
    /// A sink over `app`.
    pub fn new(app: AppHandle<R>) -> Self {
        Emit(app)
    }
}

impl<R: Runtime> EventSink for Emit<R> {
    fn emit(&self, envelope: &Envelope) {
        let _ = self.0.emit(EVENT, envelope);
    }
}

/// What an app puts around the event sink of every turn (a log line per tool call, say).
pub type SinkWrap = Arc<dyn Fn(Arc<dyn EventSink>) -> Arc<dyn EventSink> + Send + Sync>;

/// The managed state the commands read.
pub struct Managed(Arc<Assistant>, Option<SinkWrap>);

/// Manages `assistant`. A second call keeps the first and returns it.
pub fn manage<R: Runtime>(app: &AppHandle<R>, assistant: Arc<Assistant>) -> Arc<Assistant> {
    manage_with(app, assistant, None)
}

/// [`manage`], with `wrap` put around each turn's sink: every event still reaches the windows through what `wrap` returns.
pub fn manage_with<R: Runtime>(
    app: &AppHandle<R>,
    assistant: Arc<Assistant>,
    wrap: Option<SinkWrap>,
) -> Arc<Assistant> {
    if let Some(m) = app.try_state::<Managed>() {
        return m.0.clone();
    }
    app.manage(Managed(assistant.clone(), wrap));
    assistant
}

fn error(e: AssistantError) -> CommandError {
    let message = e.to_string();
    match e {
        AssistantError::Invalid(_) => CommandError::invalid(message),
        AssistantError::NotFound => CommandError::not_found(message),
        AssistantError::Busy => CommandError::busy(message),
        AssistantError::NotWaiting => CommandError::not_found(message),
        AssistantError::NotOffered => CommandError::invalid(message),
        AssistantError::Failed(_) => CommandError::failed(message),
    }
}

fn assistant_of<R: Runtime>(app: &AppHandle<R>) -> Result<Arc<Assistant>, CommandError> {
    app.try_state::<Managed>()
        .map(|m| m.0.clone())
        .ok_or_else(|| CommandError::unavailable("The assistant is not set up in this app."))
}

/// Runs `work` (it may write the conversation file) on the blocking pool.
async fn blocking<T: Send + 'static>(
    assistant: Arc<Assistant>,
    work: impl FnOnce(&Arc<Assistant>) -> Result<T, AssistantError> + Send + 'static,
) -> Result<T, CommandError> {
    match ::tauri::async_runtime::spawn_blocking(move || work(&assistant)).await {
        Ok(result) => result.map_err(error),
        Err(e) => Err(CommandError::internal(format!(
            "The assistant failed unexpectedly: {e}"
        ))),
    }
}

/// Starts a turn and returns at once; the answer arrives as events.
#[::tauri::command]
pub async fn assistant_send<R: Runtime>(
    app: AppHandle<R>,
    request: SendRequest,
) -> Result<SendAck, CommandError> {
    let emit: Arc<dyn EventSink> = Arc::new(Emit::new(app.clone()));
    let sink = match app.try_state::<Managed>().and_then(|m| m.1.clone()) {
        Some(wrap) => wrap(emit),
        None => emit,
    };
    blocking(assistant_of(&app)?, move |a| a.send(request, sink)).await
}

/// Stops the conversation's run; false when none runs.
#[::tauri::command]
pub async fn assistant_stop<R: Runtime>(
    app: AppHandle<R>,
    conversation_id: String,
) -> Result<bool, CommandError> {
    Ok(assistant_of(&app)?.stop(&conversation_id))
}

/// Answers the call waiting in the conversation's run with the user's choice (`allowOnce`, `alwaysAllow`, `deny`).
#[::tauri::command]
pub async fn assistant_approve_tool<R: Runtime>(
    app: AppHandle<R>,
    conversation_id: String,
    call_id: String,
    choice: ApprovalChoice,
) -> Result<(), CommandError> {
    assistant_of(&app)?
        .approve(&conversation_id, &call_id, choice)
        .map_err(error)
}

/// The conversations, newest first.
#[::tauri::command]
pub async fn assistant_conversations<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<ConversationSummary>, CommandError> {
    Ok(assistant_of(&app)?.conversations())
}

/// One conversation with its messages.
#[::tauri::command]
pub async fn assistant_load<R: Runtime>(
    app: AppHandle<R>,
    conversation_id: String,
) -> Result<Conversation, CommandError> {
    assistant_of(&app)?.load(&conversation_id).map_err(error)
}

/// Deletes a conversation.
#[::tauri::command]
pub async fn assistant_delete<R: Runtime>(
    app: AppHandle<R>,
    conversation_id: String,
) -> Result<(), CommandError> {
    blocking(assistant_of(&app)?, move |a| a.delete(&conversation_id)).await
}

/// The provider's name and the tools.
#[::tauri::command]
pub async fn assistant_info<R: Runtime>(app: AppHandle<R>) -> Result<AssistantInfo, CommandError> {
    Ok(assistant_of(&app)?.info())
}

/// How full the model's context is for a conversation (none: a new chat), for the composer's meter (ADR 0023 WP-4b).
#[::tauri::command]
pub async fn assistant_context<R: Runtime>(
    app: AppHandle<R>,
    conversation_id: Option<String>,
) -> Result<ContextUse, CommandError> {
    // The estimate reads the conversation's history (a saved store may read its file): off the command threads.
    blocking(assistant_of(&app)?, move |a| {
        Ok(a.context(conversation_id.as_deref()))
    })
    .await
}

paddock_commands::command_table! {
    /// The commands of the assistant adapter. Windows and pop-outs only: no dialog reads conversations.
    pub static COMMANDS, macro generated_handler;
    Mutation [MAIN | POPOUT] limit paddock_commands::MIB => assistant_send,
    Mutation [MAIN | POPOUT] => assistant_stop,
    Mutation [MAIN | POPOUT] => assistant_approve_tool,
    Query [MAIN | POPOUT] => assistant_conversations,
    Query [MAIN | POPOUT] => assistant_load,
    Mutation [MAIN | POPOUT] => assistant_delete,
    Query [MAIN | POPOUT] => assistant_info,
    Query [MAIN | POPOUT] => assistant_context,
}

/// The invoke handler of [`COMMANDS`] without a guard (for a guard of the app's or the shell plugin's own).
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    generated_handler!()
}

/// The guarded invoke handler of [`COMMANDS`].
pub fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    Guard::new(&COMMANDS).wrap(generated_handler!())
}

/// One invoke handler of two: the commands of [`COMMANDS`] go to `assistant`, everything else to `other` (the app's).
pub fn route<R: Runtime>(
    assistant: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
    other: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke: Invoke<R>| {
        if COMMANDS.spec(invoke.message.command()).is_some() {
            assistant(invoke)
        } else {
            other(invoke)
        }
    }
}
