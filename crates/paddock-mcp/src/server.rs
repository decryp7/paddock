//! The server: one `handle` per line from a client, the same for every transport.

use crate::audit::{AuditEntry, AuditSink, Outcome, redact_arguments};
use crate::jsonrpc::{Id, Incoming, RpcError, code, error_line, parse, result_line};
use crate::page;
use crate::registry::{
    Access, CallCtx, Policy, RegisterError, ResourceDef, ToolDef, ToolError, ToolOutput,
};
use crate::types::{
    CallToolResult, Content, Implementation, InitializeResult, ListResourcesResult,
    ListToolsResult, ReadResourceResult, Resource, ResourceContents, ServerCapabilities, Tool,
    ToolAnnotations, negotiate,
};
use paddock_jobs::{CancelReason, CancelToken, Clock};
use paddock_log::Redact;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The server's bounds (ADR 0007 rule 4).
#[derive(Debug, Clone)]
pub struct Limits {
    /// Bytes of one incoming line; longer lines are answered with an error and skipped.
    pub max_line_bytes: usize,
    /// Entries per page of `tools/list` and `resources/list`.
    pub page_size: usize,
    /// The result text of a tool that does not set its own limit.
    pub default_result_bytes: usize,
    /// The most any tool or resource may return.
    pub hard_result_bytes: usize,
    /// The time limit of a tool that does not set its own.
    pub default_timeout: Duration,
    /// Calls that timed out but are still running; above this new calls are refused until they end.
    pub max_abandoned: usize,
    /// Tool calls of one connection that run at the same time; further calls wait until one ends.
    pub max_concurrent_calls: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_line_bytes: 1 << 20,
            page_size: 50,
            default_result_bytes: 64 << 10,
            hard_result_bytes: 1 << 20,
            default_timeout: Duration::from_secs(30),
            max_abandoned: 8,
            max_concurrent_calls: 4,
        }
    }
}

/// What wakes a call that waits for its tool: the answer, or a cancel from the client.
enum Msg {
    Done(std::thread::Result<Result<ToolOutput, ToolError>>),
    Cancelled,
}

/// A running call that `notifications/cancelled` can stop.
struct Inflight {
    token: CancelToken,
    wake: mpsc::Sender<Msg>,
}

/// One client connection's state. Shared between the threads of a connection (calls run concurrently), so it is all interior.
#[derive(Default)]
pub struct Session {
    client: Mutex<String>,
    initialized: AtomicBool,
    calls: Mutex<Calls>,
}

/// The calls of a session: the running ones, and the ids of recent cancels that arrived before their call started (a connection
/// reads the cancel while the call's worker thread is still starting). The second list is bounded.
#[derive(Default)]
struct Calls {
    running: HashMap<Id, Inflight>,
    early: std::collections::VecDeque<Id>,
}

const MAX_EARLY_CANCELS: usize = 64;

impl Session {
    /// A session for `client` (the name the transport knows it by; empty to take the name from `initialize`).
    pub fn new(client: &str) -> Session {
        Session {
            client: Mutex::new(client.into()),
            ..Session::default()
        }
    }

    /// Who the client is.
    pub fn client(&self) -> String {
        self.client
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn calls(&self) -> MutexGuard<'_, Calls> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// The server. Cheap to share (`Arc<Server>`); tools and policy can change while it serves.
pub struct Server {
    info: Implementation,
    instructions: Option<String>,
    tools: RwLock<BTreeMap<String, ToolDef>>,
    resources: RwLock<BTreeMap<String, ResourceDef>>,
    policy: RwLock<Policy>,
    limits: Limits,
    clock: Arc<dyn Clock>,
    audit: Arc<dyn AuditSink>,
    redactor: Arc<dyn Redact>,
    abandoned: Arc<AtomicUsize>,
    /// Signalled each time an abandoned call's thread has ended and been uncounted.
    abandoned_ended: Arc<(Mutex<()>, Condvar)>,
}

fn read<T>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn write<T>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Cuts `text` to `max` bytes at a character boundary.
fn cut(text: &str, max: usize) -> &str {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

impl Server {
    /// A server named `name` at `version`. It starts switched off ([`Policy::default`]); the app turns it on from its settings.
    pub fn new(
        name: &str,
        version: &str,
        clock: Arc<dyn Clock>,
        audit: Arc<dyn AuditSink>,
        redactor: Arc<dyn Redact>,
    ) -> Server {
        Server {
            info: Implementation {
                name: name.into(),
                version: version.into(),
                title: None,
            },
            instructions: None,
            tools: RwLock::default(),
            resources: RwLock::default(),
            policy: RwLock::default(),
            limits: Limits::default(),
            clock,
            audit,
            redactor,
            abandoned: Arc::new(AtomicUsize::new(0)),
            abandoned_ended: Arc::new((Mutex::new(()), Condvar::new())),
        }
    }

    /// How to use this server, for the model (shown in `initialize`).
    pub fn with_instructions(mut self, text: &str) -> Server {
        self.instructions = Some(text.into());
        self
    }

    /// Other bounds.
    pub fn with_limits(mut self, limits: Limits) -> Server {
        self.limits = limits;
        self
    }

    /// The bounds in force.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Registers (or replaces) a tool. Plugin tools come through here too, wrapped by the app in its guarded adapter.
    pub fn add_tool(&self, tool: ToolDef) -> Result<(), RegisterError> {
        tool.validate()?;
        write(&self.tools).insert(tool.name.clone(), tool);
        Ok(())
    }

    /// Removes a tool (a plugin was unloaded).
    pub fn remove_tool(&self, name: &str) -> bool {
        write(&self.tools).remove(name).is_some()
    }

    /// Every registered tool with whether the current policy shows it (for the app's settings page).
    pub fn tool_infos(&self) -> Vec<crate::registry::ToolInfoRaw> {
        let policy = read(&self.policy);
        read(&self.tools)
            .values()
            .map(|t| crate::registry::ToolInfoRaw {
                name: t.name.clone(),
                title: t.title.clone(),
                description: t.description.clone(),
                access: t.access,
                exposed: policy.enabled && policy.exposes(t),
            })
            .collect()
    }

    /// Registers (or replaces) a resource.
    pub fn add_resource(&self, resource: ResourceDef) -> Result<(), RegisterError> {
        resource.validate()?;
        write(&self.resources).insert(resource.uri.clone(), resource);
        Ok(())
    }

    /// Replaces the policy (the user changed the MCP settings). Takes effect on the next message.
    pub fn set_policy(&self, policy: Policy) {
        *write(&self.policy) = policy;
    }

    /// The policy in force.
    pub fn policy(&self) -> Policy {
        read(&self.policy).clone()
    }

    /// Handles one line from a client; returns the line to send back, if any. Callable from several threads at once for the same
    /// session (that is how a connection runs calls concurrently); a call cancelled by the client gets no answer, as the
    /// specification says.
    pub fn handle(&self, session: &Session, line: &str) -> Option<String> {
        match parse(line) {
            Err((id, e)) => Some(error_line(id.as_ref(), &e)),
            Ok(Incoming::Response) => None,
            Ok(Incoming::Notification { method, params }) => {
                if method == "notifications/cancelled" {
                    self.cancel_call(session, &params);
                }
                None
            }
            Ok(Incoming::Request { id, method, params }) => {
                match self.request(session, &id, &method, &params) {
                    Reply::Done(Ok(v)) => Some(result_line(&id, v)),
                    Reply::Done(Err(e)) => Some(error_line(Some(&id), &e)),
                    Reply::Silent => None,
                }
            }
        }
    }

    /// Whether `line` is a tool call, which a connection may run on a worker thread while it keeps reading.
    pub(crate) fn defers(line: &str) -> bool {
        matches!(parse(line), Ok(Incoming::Request { method, .. }) if method == "tools/call")
    }

    fn cancel_call(&self, session: &Session, params: &Map<String, Value>) {
        let Some(id) = params
            .get("requestId")
            .and_then(|v| serde_json::from_value::<Id>(v.clone()).ok())
        else {
            return;
        };
        let mut calls = session.calls();
        if let Some(call) = calls.running.get(&id) {
            call.token.cancel_with(CancelReason::User);
            let _ = call.wake.send(Msg::Cancelled);
        } else {
            if calls.early.len() == MAX_EARLY_CANCELS {
                calls.early.pop_front();
            }
            calls.early.push_back(id);
        }
    }

    /// The error for a line that was too long to read.
    pub fn too_long(&self) -> String {
        error_line(
            None,
            &RpcError::new(
                code::INVALID_REQUEST,
                format!(
                    "the message is longer than {} bytes",
                    self.limits.max_line_bytes
                ),
            ),
        )
    }

    fn request(
        &self,
        session: &Session,
        id: &Id,
        method: &str,
        params: &Map<String, Value>,
    ) -> Reply {
        if method == "tools/call"
            && read(&self.policy).enabled
            && session.initialized.load(Ordering::SeqCst)
        {
            return self.call_tool(session, id, params);
        }
        Reply::Done(self.simple_request(session, method, params))
    }

    fn simple_request(
        &self,
        session: &Session,
        method: &str,
        params: &Map<String, Value>,
    ) -> Result<Value, RpcError> {
        if !read(&self.policy).enabled {
            return Err(RpcError::new(
                code::INVALID_REQUEST,
                format!(
                    "MCP is switched off in {}. Turn it on in the app's settings.",
                    self.info.name
                ),
            ));
        }
        if method == "ping" {
            return Ok(json!({}));
        }
        if method == "initialize" {
            return Ok(self.initialize(session, params));
        }
        if !session.initialized.load(Ordering::SeqCst) {
            return Err(RpcError::new(
                code::INVALID_REQUEST,
                "send initialize first",
            ));
        }
        match method {
            "tools/list" => self.list_tools(params),
            "resources/list" => self.list_resources(params),
            "resources/read" => self.read_resource(params),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            other => Err(RpcError::new(
                code::METHOD_NOT_FOUND,
                format!("this server does not implement {other}"),
            )),
        }
    }

    fn initialize(&self, session: &Session, params: &Map<String, Value>) -> Value {
        session.initialized.store(true, Ordering::SeqCst);
        if let Some(name) = params
            .get("clientInfo")
            .and_then(|c| c.get("name"))
            .and_then(Value::as_str)
        {
            let mut client = session
                .client
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if client.is_empty() {
                *client = name.chars().take(64).collect();
            }
        }
        let tools = !read(&self.tools).is_empty();
        let resources = !read(&self.resources).is_empty();
        let caps = ServerCapabilities {
            tools: tools.then(|| Map::from_iter([("listChanged".to_string(), json!(false))])),
            resources: resources.then(|| {
                Map::from_iter([
                    ("subscribe".to_string(), json!(false)),
                    ("listChanged".to_string(), json!(false)),
                ])
            }),
        };
        let result = InitializeResult {
            protocol_version: negotiate(params.get("protocolVersion").and_then(Value::as_str))
                .to_string(),
            capabilities: caps,
            server_info: self.info.clone(),
            instructions: self.instructions.clone(),
        };
        serde_json::to_value(result).unwrap_or(Value::Null)
    }

    fn cursor_param(params: &Map<String, Value>) -> Result<Option<&str>, RpcError> {
        match params.get("cursor") {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            Some(_) => Err(RpcError::new(
                code::INVALID_PARAMS,
                "cursor must be a string",
            )),
        }
    }

    fn list_tools(&self, params: &Map<String, Value>) -> Result<Value, RpcError> {
        let policy = read(&self.policy);
        let listed: Vec<Tool> = read(&self.tools)
            .values()
            .filter(|t| policy.exposes(t))
            .map(|t| Tool {
                name: t.name.clone(),
                title: t.title.clone(),
                description: t.description.clone(),
                input_schema: t.effective_schema(),
                annotations: Some(ToolAnnotations {
                    read_only_hint: Some(t.access == Access::ReadOnly),
                    destructive_hint: (t.access == Access::Action).then_some(true),
                    open_world_hint: Some(false),
                }),
            })
            .collect();
        let (tools, next_cursor) =
            page::slice(&listed, Self::cursor_param(params)?, self.limits.page_size)
                .map_err(|m| RpcError::new(code::INVALID_PARAMS, m))?;
        Ok(serde_json::to_value(ListToolsResult {
            tools: tools.to_vec(),
            next_cursor,
        })
        .unwrap_or(Value::Null))
    }

    fn list_resources(&self, params: &Map<String, Value>) -> Result<Value, RpcError> {
        let all: Vec<Resource> = if read(&self.policy).resources {
            read(&self.resources)
                .values()
                .map(|r| Resource {
                    uri: r.uri.clone(),
                    name: r.name.clone(),
                    description: r.description.clone(),
                    mime_type: r.mime_type.clone(),
                })
                .collect()
        } else {
            vec![]
        };
        let (resources, next_cursor) =
            page::slice(&all, Self::cursor_param(params)?, self.limits.page_size)
                .map_err(|m| RpcError::new(code::INVALID_PARAMS, m))?;
        Ok(serde_json::to_value(ListResourcesResult {
            resources: resources.to_vec(),
            next_cursor,
        })
        .unwrap_or(Value::Null))
    }

    fn read_resource(&self, params: &Map<String, Value>) -> Result<Value, RpcError> {
        let uri = params
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::new(code::INVALID_PARAMS, "uri is required"))?;
        let def = read(&self.resources)
            .get(uri)
            .cloned()
            .filter(|_| read(&self.policy).resources);
        let Some(def) = def else {
            return Err(RpcError::new(
                code::RESOURCE_NOT_FOUND,
                format!("Resource not found: {uri}"),
            ));
        };
        let text = (def.read)().map_err(|e| RpcError::new(code::INTERNAL_ERROR, e.0))?;
        let text = cut(&text, self.limits.default_result_bytes).to_string();
        let result = ReadResourceResult {
            contents: vec![ResourceContents {
                uri: def.uri.clone(),
                mime_type: def.mime_type.clone(),
                text,
            }],
        };
        Ok(serde_json::to_value(result).unwrap_or(Value::Null))
    }

    fn call_tool(&self, session: &Session, id: &Id, params: &Map<String, Value>) -> Reply {
        let err = |code, m: &str| Reply::Done(Err(RpcError::new(code, m)));
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return err(code::INVALID_PARAMS, "name is required");
        };
        let started = self.clock.monotonic();
        let args = match params.get("arguments") {
            None => Value::Object(Map::new()),
            Some(v @ Value::Object(_)) => v.clone(),
            Some(_) => return err(code::INVALID_PARAMS, "arguments must be an object"),
        };
        let client = session.client();
        let audit = |outcome: Outcome, bytes: usize| {
            let wall = self
                .clock
                .wall()
                .duration_since(UNIX_EPOCH)
                .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
                .unwrap_or(0);
            let duration_ms =
                u64::try_from(self.clock.monotonic().saturating_sub(started).as_millis())
                    .unwrap_or(u64::MAX);
            log::info!(target: "paddock_mcp", "tool {name} by {client} ended {outcome:?} after {duration_ms} ms, {bytes} bytes");
            self.audit.record(AuditEntry {
                at_ms: wall,
                client: client.clone(),
                tool: name.to_string(),
                arguments: redact_arguments(self.redactor.as_ref(), &args, 0),
                result_bytes: bytes,
                duration_ms,
                outcome,
            });
        };
        let tool = read(&self.tools)
            .get(name)
            .cloned()
            .filter(|t| read(&self.policy).exposes(t));
        let Some(tool) = tool else {
            audit(Outcome::Refused, 0);
            return err(code::INVALID_PARAMS, &format!("Unknown tool: {name}"));
        };
        let fail = |outcome: Outcome, text: String| {
            audit(outcome, text.len());
            Reply::Done(Ok(
                serde_json::to_value(CallToolResult::error(text)).unwrap_or(Value::Null)
            ))
        };
        if let Err(m) = tool.check_args(&args) {
            return fail(
                Outcome::ToolError,
                format!("Invalid arguments for {name}: {m}."),
            );
        }
        if self.abandoned.load(Ordering::SeqCst) >= self.limits.max_abandoned {
            return fail(
                Outcome::Refused,
                "The server is busy finishing earlier slow calls. Try again in a moment.".into(),
            );
        }
        let mut cursor = None;
        let mut handler_args = args.clone();
        if tool.paged
            && let Some(obj) = handler_args.as_object_mut()
        {
            cursor = obj
                .remove("cursor")
                .and_then(|c| c.as_str().map(str::to_string));
        }
        let ctx = CallCtx {
            cancel: CancelToken::new(),
            client: client.clone(),
            cursor,
        };
        let timeout = tool.timeout.unwrap_or(self.limits.default_timeout);
        let (tx, rx) = mpsc::channel();
        {
            let mut calls = session.calls();
            if let Some(i) = calls.early.iter().position(|e| e == id) {
                calls.early.remove(i);
                ctx.cancel.cancel_with(CancelReason::User);
                let _ = tx.send(Msg::Cancelled);
            }
            calls.running.insert(
                id.clone(),
                Inflight {
                    token: ctx.cancel.clone(),
                    wake: tx.clone(),
                },
            );
        }
        let outcome = self.run(&tool, ctx, handler_args, timeout, tx, rx);
        session.calls().running.remove(id);
        match outcome {
            Run::Done(Ok(out)) => {
                let cap = tool
                    .max_result_bytes
                    .unwrap_or(self.limits.default_result_bytes)
                    .min(self.limits.hard_result_bytes);
                let result = shape(out, cap);
                let bytes = result.text().len();
                audit(Outcome::Ok, bytes);
                Reply::Done(Ok(serde_json::to_value(result).unwrap_or(Value::Null)))
            }
            Run::Done(Err(ToolError(m))) => fail(Outcome::ToolError, m),
            Run::TimedOut => fail(
                Outcome::TimedOut,
                format!(
                    "{name} took longer than {} s and was stopped. Ask for less (a narrower query, a shorter range).",
                    timeout.as_secs().max(1)
                ),
            ),
            Run::Panicked => fail(
                Outcome::Panicked,
                format!(
                    "{name} failed inside {}. This is a bug in the app, not in your request.",
                    self.info.name
                ),
            ),
            Run::Cancelled => {
                audit(Outcome::Cancelled, 0);
                Reply::Silent
            }
        }
    }

    /// Runs the handler on its own thread so a slow or panicking tool cannot stall or take down the server. A call that passes its
    /// time limit, or that the client cancels, is cancelled through its token and abandoned; it counts against `max_abandoned`
    /// until it really ends.
    fn run(
        &self,
        tool: &ToolDef,
        ctx: CallCtx,
        args: Value,
        timeout: Duration,
        tx: mpsc::Sender<Msg>,
        rx: mpsc::Receiver<Msg>,
    ) -> Run {
        let state = Arc::new(AtomicU8::new(RUNNING));
        let (handler, cancel, abandoned, ended, st) = (
            tool.handler.clone(),
            ctx.cancel.clone(),
            self.abandoned.clone(),
            self.abandoned_ended.clone(),
            state.clone(),
        );
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(&ctx, &args)));
            let _ = tx.send(Msg::Done(r));
            if st
                .compare_exchange(RUNNING, FINISHED, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                abandoned.fetch_sub(1, Ordering::SeqCst);
                let _guard = ended
                    .0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                ended.1.notify_all();
            }
        });
        wait(&rx, &state, &cancel, &self.abandoned, timeout)
    }

    /// Blocks until every abandoned call (one past its time limit or cancelled by the client) has really ended, so the server takes
    /// calls again. Returns at once when none is running; the wait is on the end of the calls, not on a time.
    pub fn wait_for_abandoned_calls(&self) {
        let (m, cv) = &*self.abandoned_ended;
        let mut guard = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        while self.abandoned.load(Ordering::SeqCst) > 0 {
            guard = cv
                .wait(guard)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// The calendar time the server's clock reports.
    pub fn now(&self) -> SystemTime {
        self.clock.wall()
    }
}

/// Waits for a running call to end, be cancelled by the client, or pass its time limit. `state` is shared with the handler's
/// thread, which sets it to `FINISHED` when it ends; a call stopped first is marked `ABANDONED` and counted in `abandoned` until
/// its thread ends.
fn wait(
    rx: &mpsc::Receiver<Msg>,
    state: &AtomicU8,
    cancel: &CancelToken,
    abandoned: &AtomicUsize,
    timeout: Duration,
) -> Run {
    let stop = match rx.recv_timeout(timeout) {
        Ok(m) => match settled(m) {
            Some(done) => return done,
            None => Run::Cancelled,
        },
        Err(_) => Run::TimedOut,
    };
    abandoned.fetch_add(1, Ordering::SeqCst);
    if state
        .compare_exchange(RUNNING, ABANDONED, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        cancel.cancel_with(CancelReason::Deadline);
        return stop;
    }
    // It finished in the instant between the stop and now. A call past its time limit takes the answer; a call the client
    // cancelled stays unanswered, because the cancel was read first (MCP: the receiver does not answer a cancelled request).
    abandoned.fetch_sub(1, Ordering::SeqCst);
    match stop {
        Run::Cancelled => Run::Cancelled,
        _ => rx.iter().find_map(settled).unwrap_or(Run::Panicked),
    }
}

const RUNNING: u8 = 0;
const FINISHED: u8 = 1;
const ABANDONED: u8 = 2;

enum Run {
    Done(Result<ToolOutput, ToolError>),
    TimedOut,
    Panicked,
    Cancelled,
}

/// The end of a call from the tool's side: its answer, a panic, or `None` for a cancel (not an end).
fn settled(m: Msg) -> Option<Run> {
    match m {
        Msg::Done(Ok(r)) => Some(Run::Done(r)),
        Msg::Done(Err(_)) => Some(Run::Panicked),
        Msg::Cancelled => None,
    }
}

/// What a request comes to: an answer, or none (the client cancelled it).
enum Reply {
    Done(Result<Value, RpcError>),
    Silent,
}

/// A tool's output as a result: the text cut at `cap` bytes, a note when it was cut or there is a next page, the structured data
/// dropped when it alone is over the cap.
fn shape(out: ToolOutput, cap: usize) -> CallToolResult {
    let mut text = out.text;
    let mut meta = Map::new();
    if text.len() > cap {
        text = format!(
            "{}\n\n[truncated at {cap} bytes; ask for less, or use a narrower query]",
            cut(&text, cap)
        );
        meta.insert("paddock/truncated".into(), Value::Bool(true));
    }
    if let Some(c) = &out.next_cursor {
        text.push_str(&format!(
            "\n\n[more results: call again with cursor \"{c}\"]"
        ));
        meta.insert("paddock/nextCursor".into(), Value::String(c.clone()));
    }
    let structured = out.structured.filter(|v| v.to_string().len() <= cap);
    CallToolResult {
        content: vec![Content::Text { text }],
        structured_content: structured,
        is_error: false,
        meta: (!meta.is_empty()).then_some(meta),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_cut_on_a_character_boundary() {
        assert_eq!(cut("héllo", 2), "h");
        assert_eq!(cut("héllo", 3), "hé");
        assert_eq!(cut("abc", 10), "abc");
    }

    #[test]
    fn shaping_marks_truncation_pages_and_drops_oversized_structure() {
        let big = ToolOutput {
            text: "x".repeat(50),
            structured: Some(json!({"k": "y".repeat(50)})),
            next_cursor: Some("p2".into()),
        };
        let r = shape(big, 20);
        assert!(r.text().contains("[truncated at 20 bytes") && r.text().contains("cursor \"p2\""));
        assert!(r.structured_content.is_none());
        let m = r.meta.unwrap();
        assert_eq!(
            (
                m["paddock/truncated"].as_bool(),
                m["paddock/nextCursor"].as_str()
            ),
            (Some(true), Some("p2"))
        );
        let small = shape(
            ToolOutput {
                text: "ok".into(),
                structured: Some(json!({"a": 1})),
                next_cursor: None,
            },
            100,
        );
        assert_eq!(
            (
                small.text(),
                small.meta.is_none(),
                small.structured_content.is_some()
            ),
            ("ok", true, true)
        );
    }

    /// The interleaving behind CI run 6487's flake: the client's cancel is queued, then the tool finishes (and marks itself
    /// finished) before the waiting call looks at the state. The cancel came first, so the call stays unanswered.
    #[test]
    fn a_call_cancelled_before_its_tool_finished_stays_cancelled() {
        let (tx, rx) = mpsc::channel();
        tx.send(Msg::Cancelled).unwrap();
        tx.send(Msg::Done(Ok(Ok(ToolOutput::text("late")))))
            .unwrap();
        let (state, abandoned) = (AtomicU8::new(FINISHED), AtomicUsize::new(0));
        let run = wait(
            &rx,
            &state,
            &CancelToken::new(),
            &abandoned,
            Duration::from_secs(60),
        );
        assert!(
            matches!(run, Run::Cancelled),
            "a call the client cancelled got its tool's answer"
        );
        assert_eq!(
            abandoned.load(Ordering::SeqCst),
            0,
            "a finished call is not abandoned"
        );
    }

    #[test]
    fn a_call_that_times_out_as_its_tool_ends_without_an_answer_is_a_panic() {
        let (tx, rx) = mpsc::channel();
        drop(tx);
        let (state, abandoned) = (AtomicU8::new(FINISHED), AtomicUsize::new(0));
        let run = wait(&rx, &state, &CancelToken::new(), &abandoned, Duration::ZERO);
        assert!(
            matches!(run, Run::Panicked),
            "a tool that ended without an answer counts as a panic"
        );
        assert_eq!(abandoned.load(Ordering::SeqCst), 0);
    }
}
