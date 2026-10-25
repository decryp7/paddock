//! Shared test rig: a scripted provider, a channel sink and a few tools. No sleeps: waits are channel and condition waits.
#![allow(dead_code)]

use paddock_assistant::*;
use paddock_health::{Clock, FakeClock};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub enum Step {
    /// Streams the deltas and answers.
    Reply(Vec<&'static str>, ChatResponse),
    /// Fails after streaming the deltas.
    Fail(Vec<&'static str>, ProviderError),
    /// Runs this, then answers (a test changes the world while the turn runs).
    Hook(Box<dyn Fn() + Send>, ChatResponse),
    /// Streams one delta, then waits for the cancel flag and answers cancelled with the partial text.
    Block,
}

pub fn text(t: &str) -> ChatResponse {
    ChatResponse {
        text: t.into(),
        tool_calls: vec![],
        stop_reason: StopReason::EndTurn,
        usage: Some(Usage {
            input_tokens: 10,
            output_tokens: 5,
        }),
    }
}

pub fn calls(t: &str, calls: Vec<ToolCall>) -> ChatResponse {
    ChatResponse {
        text: t.into(),
        tool_calls: calls,
        stop_reason: StopReason::ToolUse,
        usage: Some(Usage {
            input_tokens: 10,
            output_tokens: 5,
        }),
    }
}

pub fn call(id: &str, name: &str, input: Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        input,
    }
}

#[derive(Default)]
pub struct Scripted {
    steps: Mutex<VecDeque<Step>>,
    pub seen: Mutex<Vec<ChatRequest>>,
    /// When a blocked generation saw the stop.
    pub unblocked: Mutex<Option<std::time::Instant>>,
}

impl Scripted {
    pub fn new(steps: Vec<Step>) -> Arc<Scripted> {
        Arc::new(Scripted {
            steps: Mutex::new(steps.into()),
            seen: Mutex::default(),
            unblocked: Mutex::default(),
        })
    }

    pub fn tool_names(&self, request: usize) -> Vec<String> {
        self.seen.lock().unwrap()[request]
            .tools
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }
}

impl LlmProvider for Scripted {
    fn id(&self) -> &str {
        "scripted"
    }
    fn display_name(&self) -> String {
        "Scripted".into()
    }
    fn context_length(&self) -> usize {
        8192
    }
    fn chat(
        &self,
        req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProviderError> {
        self.seen.lock().unwrap().push(req.clone());
        let step = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("the script ran out of steps");
        match step {
            Step::Reply(deltas, response) => {
                deltas.into_iter().for_each(on_delta);
                Ok(response)
            }
            Step::Fail(deltas, e) => {
                deltas.into_iter().for_each(on_delta);
                Err(e)
            }
            Step::Hook(hook, response) => {
                hook();
                Ok(response)
            }
            Step::Block => {
                on_delta("partial");
                req.cancel.wait();
                *self.unblocked.lock().unwrap() = Some(std::time::Instant::now());
                Ok(ChatResponse {
                    text: "partial".into(),
                    tool_calls: vec![],
                    stop_reason: StopReason::Cancelled,
                    usage: None,
                })
            }
        }
    }
}

pub struct ChannelSink(pub Mutex<Sender<Envelope>>);

impl EventSink for ChannelSink {
    fn emit(&self, e: &Envelope) {
        let _ = self.0.lock().unwrap().send(e.clone());
    }
}

pub fn sink() -> (Arc<ChannelSink>, Receiver<Envelope>) {
    let (tx, rx) = channel();
    (Arc::new(ChannelSink(Mutex::new(tx))), rx)
}

/// Events up to and including the first terminal one (`Done` or `Error`).
pub fn until_end(rx: &Receiver<Envelope>) -> Vec<AssistantEvent> {
    let mut out = vec![];
    loop {
        let e = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("the turn never ended")
            .event;
        let end = matches!(
            e,
            AssistantEvent::Done { .. } | AssistantEvent::Error { .. }
        );
        out.push(e);
        if end {
            return out;
        }
    }
}

/// Waits for an event matching `want`; returns everything seen up to it.
pub fn until(
    rx: &Receiver<Envelope>,
    want: impl Fn(&AssistantEvent) -> bool,
) -> Vec<AssistantEvent> {
    let mut out = vec![];
    loop {
        let e = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("the event never came")
            .event;
        let hit = want(&e);
        out.push(e);
        if hit {
            return out;
        }
    }
}

pub fn done(events: &[AssistantEvent]) -> (String, EndReason, Option<String>) {
    match events.last() {
        Some(AssistantEvent::Done {
            text, end, message, ..
        }) => (text.clone(), *end, message.clone()),
        other => panic!("not done: {other:?}"),
    }
}

/// A read-only tool that counts its calls.
pub struct Echo(pub Arc<AtomicUsize>);

impl AssistantTool for Echo {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "echo".into(),
            description: "Echoes".into(),
            input_schema: json!({"type": "object", "required": ["word"]}),
        }
    }
    fn budget(&self) -> ToolBudget {
        ToolBudget {
            max_calls: 2,
            max_output_chars: 20,
        }
    }
    fn call(&self, input: &Value, _: &Cancel) -> ToolOutput {
        self.0.fetch_add(1, Ordering::SeqCst);
        ToolOutput::ok(format!(
            "echo {} and a very long tail that gets cut",
            input["word"]
        ))
        .with_data(json!({"word": input["word"]}))
        .with_summary("echoed")
    }
}

/// A tool that writes what cannot be undone.
pub struct Erase(pub Arc<AtomicUsize>);

impl AssistantTool for Erase {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "erase".into(),
            description: "Erases".into(),
            input_schema: json!({"type": "object"}),
        }
    }
    fn access(&self) -> Access {
        Access::Destructive
    }
    fn describe(&self, _: &Value) -> String {
        "Erase everything".into()
    }
    fn call(&self, _: &Value, _: &Cancel) -> ToolOutput {
        self.0.fetch_add(1, Ordering::SeqCst);
        ToolOutput::ok("erased")
    }
}

/// A tool that writes something the user can undo.
pub struct Note(pub Arc<AtomicUsize>);

impl AssistantTool for Note {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "note".into(),
            description: "Adds a note".into(),
            input_schema: json!({"type": "object"}),
        }
    }
    fn access(&self) -> Access {
        Access::Write
    }
    fn describe(&self, _: &Value) -> String {
        "Add a note".into()
    }
    fn call(&self, _: &Value, _: &Cancel) -> ToolOutput {
        self.0.fetch_add(1, Ordering::SeqCst);
        ToolOutput::ok("noted")
    }
}

/// A tool that panics.
pub struct Boom;

impl AssistantTool for Boom {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "boom".into(),
            description: "Panics".into(),
            input_schema: json!({"type": "object"}),
        }
    }
    fn call(&self, _: &Value, _: &Cancel) -> ToolOutput {
        panic!("the tool blew up");
    }
}

/// A tool that moves the fake clock (time passes while it works).
pub struct Slow(pub FakeClock);

impl AssistantTool for Slow {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "slow".into(),
            description: "Takes long".into(),
            input_schema: json!({"type": "object"}),
        }
    }
    fn call(&self, _: &Value, _: &Cancel) -> ToolOutput {
        self.0.advance(Duration::from_secs(1000));
        ToolOutput::ok("done")
    }
}

pub struct Rig {
    pub assistant: Arc<Assistant>,
    pub provider: Arc<Scripted>,
    pub clock: FakeClock,
    pub dir: tempfile::TempDir,
    pub rx: Receiver<Envelope>,
    pub sink: Arc<ChannelSink>,
}

pub fn rig_with(
    steps: Vec<Step>,
    tools: Vec<Arc<dyn AssistantTool>>,
    budgets: impl FnOnce(&mut Budgets),
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    rig_in(dir, steps, tools, budgets)
}

pub fn rig_in(
    dir: tempfile::TempDir,
    steps: Vec<Step>,
    tools: Vec<Arc<dyn AssistantTool>>,
    budgets: impl FnOnce(&mut Budgets),
) -> Rig {
    let clock = FakeClock::at_wall_ms(1_700_000_000_000);
    let provider = Scripted::new(steps);
    let store = Arc::new(ConversationStore::open(
        dir.path().join("assistant.json"),
        Arc::new(clock.clone()),
    ));
    let mut cfg = AssistantConfig::new("Demo", "You are the demo assistant.");
    budgets(&mut cfg.budgets);
    let assistant = Assistant::new(
        provider.clone(),
        Tools::new(tools).unwrap(),
        store,
        Arc::new(clock.clone()) as Arc<dyn Clock>,
        cfg,
    );
    let (sink, rx) = sink();
    Rig {
        assistant,
        provider,
        clock,
        dir,
        rx,
        sink,
    }
}

pub fn rig(steps: Vec<Step>) -> Rig {
    rig_with(steps, vec![], |_| {})
}

pub fn ask(text: &str) -> SendRequest {
    SendRequest {
        conversation_id: None,
        text: text.into(),
        context: vec![],
        allow_write: false,
    }
}

impl Rig {
    pub fn send(&self, req: SendRequest) -> Result<SendAck, AssistantError> {
        self.assistant.send(req, self.sink.clone())
    }

    /// Sends and waits for the turn to end.
    pub fn turn(&self, req: SendRequest) -> (String, Vec<AssistantEvent>) {
        let ack = self.send(req).unwrap();
        (ack.conversation_id, until_end(&self.rx))
    }
}

/// A tool that stops the turn while it runs (the user pressed Stop).
pub struct Halt;

impl AssistantTool for Halt {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "halt".into(),
            description: "Stops".into(),
            input_schema: json!({"type": "object"}),
        }
    }
    fn call(&self, _: &Value, cancel: &Cancel) -> ToolOutput {
        cancel.cancel();
        ToolOutput::ok("halted")
    }
}
