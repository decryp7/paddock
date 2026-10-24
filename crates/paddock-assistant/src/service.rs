//! The assistant service: conversations, one run per conversation on a worker thread, Stop, approvals. Commands return at
//! once (rule 10) and a run reports through [`EventSink`].

use crate::conversation::{
    ContextItem, Conversation, ConversationStore, ConversationSummary, MAX_CONTEXT_CHARS,
    MAX_CONTEXT_ITEMS, SaveError, StoredMessage,
};
use crate::event::{AssistantEvent, Envelope, ErrorKind, EventSink};
use crate::provider::{LlmProvider, ProviderError};
use crate::run::{Budgets, Gate, Hooks, Turn, run_turn};
use crate::tool::{ToolInfo, Tools};
use crate::types::Cancel;
use paddock_health::Clock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::UNIX_EPOCH;

/// Longest question, in characters.
pub const MAX_USER_CHARS: usize = 8000;
/// Most runs at once over all conversations.
pub const MAX_RUNS: usize = 4;

/// What the app tells the assistant about itself.
#[derive(Debug, Clone)]
pub struct AssistantConfig {
    /// The app's display name (in the needs-AI sentences).
    pub app: String,
    /// The system prompt: what the assistant is for and the app's rules.
    pub system: String,
    /// The limits of a turn.
    pub budgets: Budgets,
    /// How much of the conversation the model reads, in characters.
    pub history_chars: usize,
}

impl AssistantConfig {
    /// A config for `app` with this system prompt and the default limits.
    pub fn new(app: &str, system: &str) -> AssistantConfig {
        AssistantConfig {
            app: app.into(),
            system: system.into(),
            budgets: Budgets::default(),
            history_chars: 12_000,
        }
    }
}

/// A question.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SendRequest {
    /// The conversation to continue; none starts a new one.
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub conversation_id: Option<String>,
    /// What the user typed.
    pub text: String,
    /// The context chips still on when the user sent.
    #[serde(default)]
    pub context: Vec<ContextItem>,
    /// The user ticked "Allow actions": tools that write are offered (each call still needs approval).
    #[serde(default)]
    pub allow_write: bool,
}

/// The answer to a send: the turn runs on and reports through events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SendAck {
    /// The conversation (new or continued).
    pub conversation_id: String,
}

/// What the UI needs to know about the assistant besides the conversations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AssistantInfo {
    /// The provider's name ("Pitwall (qwen3-8b)").
    pub provider: String,
    /// The tools.
    pub tools: Vec<ToolInfo>,
    /// Some tool writes: the composer offers "Allow actions".
    pub has_write: bool,
}

/// Why a command was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantError {
    /// The argument is wrong; the text says how.
    Invalid(String),
    /// No such conversation.
    NotFound,
    /// The conversation is answering already, or too many runs are going.
    Busy,
    /// No such tool call waits for a decision.
    NotWaiting,
    /// The worker thread could not start, or a file could not be changed.
    Failed(String),
}

impl std::fmt::Display for AssistantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssistantError::Invalid(m) | AssistantError::Failed(m) => f.write_str(m),
            AssistantError::NotFound => f.write_str("That conversation no longer exists."),
            AssistantError::Busy => {
                f.write_str("The assistant is answering already. Wait for it or press Stop.")
            }
            AssistantError::NotWaiting => f.write_str("No action waits for a decision."),
        }
    }
}

impl std::error::Error for AssistantError {}

struct Run {
    cancel: Cancel,
    gate: Arc<Gate>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

/// The assistant of one app.
pub struct Assistant {
    provider: Arc<dyn LlmProvider>,
    tools: Tools,
    store: Arc<ConversationStore>,
    clock: Arc<dyn Clock>,
    cfg: AssistantConfig,
    runs: Mutex<HashMap<String, Arc<Run>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.into();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

impl Assistant {
    /// An assistant over `provider` with the app's `tools`, keeping conversations in `store`.
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        tools: Tools,
        store: Arc<ConversationStore>,
        clock: Arc<dyn Clock>,
        cfg: AssistantConfig,
    ) -> Arc<Assistant> {
        Arc::new(Assistant {
            provider,
            tools,
            store,
            clock,
            cfg,
            runs: Mutex::default(),
        })
    }

    /// The conversation store.
    pub fn store(&self) -> &Arc<ConversationStore> {
        &self.store
    }

    fn now_ms(&self) -> u64 {
        self.clock
            .wall()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }

    /// The provider's name and the tools.
    pub fn info(&self) -> AssistantInfo {
        AssistantInfo {
            provider: self.provider.display_name(),
            tools: self.tools.infos(),
            has_write: self.tools.has_write(),
        }
    }

    /// The conversations, newest first.
    pub fn conversations(&self) -> Vec<ConversationSummary> {
        self.store.summaries()
    }

    /// One conversation.
    pub fn load(&self, id: &str) -> Result<Conversation, AssistantError> {
        self.store.get(id).ok_or(AssistantError::NotFound)
    }

    /// Deletes a conversation (stopping its run first).
    pub fn delete(&self, id: &str) -> Result<(), AssistantError> {
        self.stop(id);
        match self.store.delete(id) {
            Ok(true) => Ok(()),
            Ok(false) => Err(AssistantError::NotFound),
            Err(SaveError(m)) => Err(AssistantError::Failed(m)),
        }
    }

    /// Stops the conversation's run; `false` when none runs. The run ends within a read slice (50 ms for Pitwall).
    pub fn stop(&self, id: &str) -> bool {
        let run = lock(&self.runs).get(id).cloned();
        run.is_some_and(|r| {
            r.cancel.cancel();
            r.gate.wake();
            true
        })
    }

    /// Answers the write waiting in the conversation's run.
    pub fn approve(&self, id: &str, call_id: &str, approved: bool) -> Result<(), AssistantError> {
        let run = lock(&self.runs)
            .get(id)
            .cloned()
            .ok_or(AssistantError::NotWaiting)?;
        if run.gate.decide(call_id, approved) {
            Ok(())
        } else {
            Err(AssistantError::NotWaiting)
        }
    }

    /// Stops every run and waits for the threads (the app is closing, or a test ends).
    pub fn shutdown(&self) {
        let runs: Vec<Arc<Run>> = lock(&self.runs).values().cloned().collect();
        for r in &runs {
            r.cancel.cancel();
            r.gate.wake();
        }
        for r in runs {
            if let Some(h) = lock(&r.handle).take() {
                let _ = h.join();
            }
        }
    }

    fn check(req: &SendRequest) -> Result<(String, Vec<ContextItem>), AssistantError> {
        let text = req.text.trim().to_string();
        if text.is_empty() {
            return Err(AssistantError::Invalid("Type a question first.".into()));
        }
        if text.chars().count() > MAX_USER_CHARS {
            return Err(AssistantError::Invalid(format!(
                "A question can have at most {MAX_USER_CHARS} characters."
            )));
        }
        if req.context.len() > MAX_CONTEXT_ITEMS {
            return Err(AssistantError::Invalid(format!(
                "At most {MAX_CONTEXT_ITEMS} context items can be attached."
            )));
        }
        let mut context = req.context.clone();
        for c in &mut context {
            if c.id.trim().is_empty() || c.label.trim().is_empty() {
                return Err(AssistantError::Invalid(
                    "A context item needs an id and a label.".into(),
                ));
            }
            c.text = cut(&c.text, MAX_CONTEXT_CHARS);
        }
        Ok((text, context))
    }

    /// Starts a turn on a worker thread and returns at once; progress and the answer arrive through `sink`.
    pub fn send(
        self: &Arc<Self>,
        req: SendRequest,
        sink: Arc<dyn EventSink>,
    ) -> Result<SendAck, AssistantError> {
        let (text, context) = Self::check(&req)?;
        let user = StoredMessage::user(text, context, self.now_ms());
        let mut runs = lock(&self.runs);
        if runs.len() >= MAX_RUNS {
            return Err(AssistantError::Busy);
        }
        let (id, title, saved) = match req.conversation_id {
            Some(id) => {
                if runs.contains_key(&id) {
                    return Err(AssistantError::Busy);
                }
                let title = self.store.get(&id).ok_or(AssistantError::NotFound)?.title;
                let saved = self
                    .store
                    .append(&id, vec![user])
                    .ok_or(AssistantError::NotFound)?;
                (id, title, saved)
            }
            None => self.store.create(user),
        };
        let run = Arc::new(Run {
            cancel: Cancel::new(),
            gate: Arc::new(Gate::default()),
            handle: Mutex::default(),
        });
        let me = self.clone();
        let (thread_run, thread_id) = (run.clone(), id.clone());
        let allow_write = req.allow_write;
        let handle = std::thread::Builder::new()
            .name("paddock-assistant".into())
            .spawn(move || {
                me.work(
                    &thread_id,
                    &title,
                    saved,
                    allow_write,
                    &thread_run,
                    sink.as_ref(),
                )
            })
            .map_err(|e| AssistantError::Failed(format!("The assistant could not start: {e}")))?;
        *lock(&run.handle) = Some(handle);
        runs.insert(id.clone(), run);
        Ok(SendAck {
            conversation_id: id,
        })
    }

    fn work(
        &self,
        id: &str,
        title: &str,
        saved: Result<(), SaveError>,
        allow_write: bool,
        run: &Run,
        sink: &dyn EventSink,
    ) {
        let mut seq = 0u64;
        let mut told = false;
        let mut say = |event: AssistantEvent| {
            seq += 1;
            sink.emit(&Envelope {
                conversation_id: id.into(),
                seq,
                event,
            });
        };
        say(AssistantEvent::Started {
            title: title.into(),
        });
        if let Err(e) = &saved {
            told = true;
            say(AssistantEvent::Notice {
                message: e.to_string(),
            });
        }
        let history = self.store.history(id, self.cfg.history_chars);
        let turn = Turn {
            provider: self.provider.as_ref(),
            tools: &self.tools,
            clock: self.clock.as_ref(),
            budgets: self.cfg.budgets,
            system: &self.cfg.system,
            history,
            allow_write,
            cancel: &run.cancel,
            gate: &run.gate,
        };
        let mut kept: Vec<StoredMessage> = Vec::new();
        let mut keep = |m: StoredMessage| kept.push(m);
        // Events are delivered as they happen; messages are saved at the end of the turn (and are lost only if the process
        // dies mid-turn, which keeps the question: it was saved first).
        let now = || self.now_ms();
        let result = {
            let mut forward = |e: AssistantEvent| say(e);
            let mut hooks = Hooks {
                emit: &mut forward,
                keep: &mut keep,
            };
            run_turn(&turn, &mut hooks, &now)
        };
        if let Some(Err(e)) = self.store.append(id, kept)
            && !told
        {
            say(AssistantEvent::Notice {
                message: e.to_string(),
            });
        }
        // The conversation is free again before the last event, so a window that reacts to it can send at once.
        lock(&self.runs).remove(id);
        match result {
            Ok(end) => say(AssistantEvent::Done {
                text: end.text,
                usage: end.usage,
                end: end.end,
                message: end.message,
            }),
            Err(ProviderError::Unavailable(state)) => say(AssistantEvent::Error {
                kind: ErrorKind::NeedsAi,
                message: state.message(&self.cfg.app),
                needs_ai: Some(state),
            }),
            Err(e) => say(AssistantEvent::Error {
                kind: ErrorKind::Failed,
                message: e.to_string(),
                needs_ai: None,
            }),
        }
    }
}
