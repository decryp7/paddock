//! The plugin process' side: [`serve`] runs the one child loop (`paddock_ipc::child::serve`: bootstrap on stdin, the token
//! handshake, the stdin lifeline) and answers the app's [`PlatformRequest`]s with the plugin's contributions.
//! A panic in a contribution is contained by the connection (the app gets a `panic` failure and counts a fault).

use crate::extension::{ExtCtx, route};
use crate::plugin::{Contributions, Plugin, ViewCtx};
use crate::protocol::*;
use crate::tools::{Host, ToolCtx, ToolError, ToolTask};
use paddock_ipc::{CallCtx, Handler, IpcError, Json, Peer, RemoteError, codes, decode_request, encode_reply};
use paddock_view::{ViewAction, ViewDoc, ViewPatch};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// The role the app asks of a plugin process that runs elevated operations (`Bootstrap::role`): it runs only operations
/// marked [`crate::tools::OpSpec::elevated`], and the others run in the normal process.
pub const ELEVATED_ROLE: &str = "elevated";

/// How long a plugin waits for an answer to a report or a task announcement.
const REPORT_TIMEOUT: Option<Duration> = Some(Duration::from_secs(10));
/// How long a plugin waits for a read of a host file.
const READ_TIMEOUT: Option<Duration> = Some(Duration::from_secs(60));
/// How long a plugin waits for a piece of output to be written.
const OUTPUT_TIMEOUT: Option<Duration> = Some(Duration::from_secs(120));

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A plugin process' `main`: `serve(Arc::new(MyPlugin))`.
pub fn serve(plugin: Arc<dyn Plugin>) -> Result<(), IpcError> {
    paddock_ipc::child::serve(
        "the app",
        PLATFORM_PROTOCOL,
        move |boot| Arc::new(PluginHandler::new(plugin, boot.role == ELEVATED_ROLE)),
        |_, _| {},
    )
}

/// The [`Host`] of a plugin process: calls on the connection to the app.
pub struct PeerHost(pub Peer);

fn to_error(e: IpcError) -> ToolError {
    match e {
        IpcError::Remote(r) if r.code == DENIED => ToolError::Denied(r.message),
        IpcError::Remote(r) if r.code == codes::CANCELLED => ToolError::Cancelled,
        other => ToolError::Failed(other.to_string()),
    }
}

impl Host for PeerHost {
    fn read_file(&self, id: u64, offset: u64, len: u32) -> Result<Vec<u8>, ToolError> {
        self.0.request(&HostCall::ReadFile { id, offset, len }, READ_TIMEOUT).map_err(to_error)
    }

    fn report(&self, task: &str, progress: Option<f32>, note: &str) {
        let call = HostCall::Report {
            task: task.into(),
            progress,
            note: note.into(),
        };
        let _: Result<(), _> = self.0.request(&call, REPORT_TIMEOUT);
    }

    fn task_started(&self, tool: &str, task: &ToolTask) {
        let call = HostCall::TaskStarted {
            tool: tool.into(),
            task: task.clone(),
        };
        let _: Result<(), _> = self.0.request(&call, REPORT_TIMEOUT);
    }

    fn task_finished(&self, task: &str) {
        let _: Result<(), _> = self.0.request(&HostCall::TaskFinished { task: task.into() }, REPORT_TIMEOUT);
    }

    fn output(&self, name: &str, bytes: &[u8], last: bool) -> Result<Option<SavedOutput>, ToolError> {
        let call = HostCall::Output {
            name: name.into(),
            bytes: bytes.to_vec(),
            last,
        };
        self.0.request(&call, OUTPUT_TIMEOUT).map_err(to_error)
    }

    fn elevated(&self, tool: &str, op: &str, args: Value, timeout: Duration) -> Result<Value, ToolError> {
        let call = HostCall::Elevated {
            tool: tool.into(),
            op: op.into(),
            args: Json(args),
            timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
        };
        let wait = Some(timeout.saturating_add(Duration::from_secs(10)));
        let reply: Result<Json, Fail> = self.0.request(&call, wait).map_err(to_error)?;
        reply.map(Json::into_inner).map_err(|f| fail_to_error(&f))
    }
}

fn fail_to_error(f: &Fail) -> ToolError {
    match f.reason {
        Some(crate::tools::FailureReason::Denied) => ToolError::Denied(f.message.clone()),
        Some(crate::tools::FailureReason::Missing) => ToolError::Missing(f.message.clone()),
        None => ToolError::Failed(f.message.clone()),
    }
}

/// What a call gives the dispatcher: the app's services and whether the call was cancelled. [`CallCtx`] in a plugin
/// process; a stub in unit tests.
pub trait CallEnv {
    /// The app's services for this call.
    fn host(&self) -> Arc<dyn Host>;
    /// Whether the app cancelled the call.
    fn is_cancelled(&self) -> bool;
    /// A probe a tool can keep for the length of the call.
    fn cancel_probe(&self) -> Arc<dyn Fn() -> bool + Send + Sync>;
}

struct PeerEnv<'a>(&'a CallCtx);

impl CallEnv for PeerEnv<'_> {
    fn host(&self) -> Arc<dyn Host> {
        Arc::new(PeerHost(self.0.peer().clone()))
    }

    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }

    fn cancel_probe(&self) -> Arc<dyn Fn() -> bool + Send + Sync> {
        let probe = self.0.cancel_probe();
        Arc::new(move || probe.is_cancelled())
    }
}

/// Remembers which tool started which task, so `Stop { task }` reaches it.
struct Tracking {
    inner: Arc<dyn Host>,
    tasks: Arc<Mutex<HashMap<String, String>>>,
}

impl Host for Tracking {
    fn read_file(&self, id: u64, offset: u64, len: u32) -> Result<Vec<u8>, ToolError> {
        self.inner.read_file(id, offset, len)
    }
    fn report(&self, task: &str, progress: Option<f32>, note: &str) {
        self.inner.report(task, progress, note);
    }
    fn task_started(&self, tool: &str, task: &ToolTask) {
        lock(&self.tasks).insert(task.id.clone(), tool.into());
        self.inner.task_started(tool, task);
    }
    fn task_finished(&self, task: &str) {
        lock(&self.tasks).remove(task);
        self.inner.task_finished(task);
    }
    fn output(&self, name: &str, bytes: &[u8], last: bool) -> Result<Option<SavedOutput>, ToolError> {
        self.inner.output(name, bytes, last)
    }
    fn elevated(&self, tool: &str, op: &str, args: Value, timeout: Duration) -> Result<Value, ToolError> {
        self.inner.elevated(tool, op, args, timeout)
    }
}

/// Answers the app's requests with a plugin's contributions.
pub struct PluginHandler {
    plugin: Arc<dyn Plugin>,
    c: Contributions,
    registered: Result<Described, String>,
    elevated: bool,
    tasks: Arc<Mutex<HashMap<String, String>>>,
}

fn invalid(m: impl Into<String>) -> RemoteError {
    RemoteError::new(codes::INVALID, m)
}

fn internal(m: impl Into<String>) -> RemoteError {
    RemoteError::new(codes::INTERNAL, m)
}

/// A tool error as the call's result: cancellation is a failure of the call, the rest a typed [`Fail`] in the reply.
fn outcome<T: serde::Serialize>(result: Result<T, ToolError>) -> Result<Vec<u8>, RemoteError> {
    match result {
        Err(ToolError::Cancelled) => Err(CallCtx::cancelled_error()),
        Ok(v) => encode_reply(&Ok::<T, Fail>(v)),
        Err(e) => encode_reply(&Err::<T, Fail>(Fail {
            reason: e.reason(),
            message: e.to_string(),
        })),
    }
}

impl PluginHandler {
    /// Registers `plugin`'s contributions and checks them; `elevated` is the elevated role ([`ELEVATED_ROLE`]).
    pub fn new(plugin: Arc<dyn Plugin>, elevated: bool) -> PluginHandler {
        let mut c = Contributions::default();
        let registered = plugin
            .register(&mut c)
            .and_then(|()| c.check())
            .map(|()| c.describe(plugin.as_ref()))
            .map_err(|e| e.to_string());
        PluginHandler {
            plugin,
            c,
            registered,
            elevated,
            tasks: Arc::default(),
        }
    }

    /// The plugin.
    pub fn plugin(&self) -> &Arc<dyn Plugin> {
        &self.plugin
    }

    fn tool_ctx(&self, env: &dyn CallEnv, tool: &str) -> ToolCtx {
        let host = Arc::new(Tracking {
            inner: env.host(),
            tasks: self.tasks.clone(),
        });
        ToolCtx::new(tool, None, host).with_cancel(env.cancel_probe())
    }

    /// Answers one request.
    pub fn dispatch(&self, env: &dyn CallEnv, req: PlatformRequest) -> Result<Vec<u8>, RemoteError> {
        let described = self.registered.as_ref().map_err(|e| internal(e.clone()))?;
        match req {
            PlatformRequest::Describe => encode_reply(&Json(serde_json::to_value(described).map_err(|e| internal(e.to_string()))?)),
            PlatformRequest::Extension { ns, minor, body } => {
                let cancelled = || env.is_cancelled();
                let ctx = ExtCtx {
                    cancelled: &cancelled,
                    host: env.host(),
                };
                route(&self.c.extensions, &described.id, &ns, minor, &body, &ctx)
            }
            _ if self.elevated && !matches!(req, PlatformRequest::Invoke { .. } | PlatformRequest::Stop { .. }) => {
                Err(invalid("the elevated process only runs elevated tool operations"))
            }
            PlatformRequest::ApplySettings { json } => {
                let (Some(s), Some(section)) = (&self.c.settings, &described.settings) else {
                    return Err(invalid("this plugin has no settings"));
                };
                let valid = paddock_settings::validate(section, &json).map_err(|errors| {
                    invalid(errors.iter().map(|e| format!("{}: {}", e.key, e.message)).collect::<Vec<_>>().join("; "))
                })?;
                s.apply(&valid).map_err(|e| internal(e.to_string()))?;
                encode_reply(&())
            }
            PlatformRequest::Invoke { tool, op, args } => {
                let (info, t) = self.c.tools.iter().find(|(i, _)| i.id == tool).ok_or_else(|| invalid(format!("no tool \"{tool}\"")))?;
                let spec = info
                    .ops
                    .iter()
                    .find(|o| o.name == op)
                    .ok_or_else(|| invalid(format!("tool \"{tool}\" has no operation \"{op}\"")))?;
                if spec.elevated != self.elevated {
                    return Err(invalid(if spec.elevated {
                        format!("\"{tool} {op}\" runs in the plugin's elevated process")
                    } else {
                        format!("\"{tool} {op}\" does not run in the elevated process")
                    }));
                }
                // The app validated the arguments (and the paths); check the shape again.
                spec.args.validate(&args).map_err(|e| invalid(e.to_string()))?;
                let ctx = self.tool_ctx(env, &tool);
                outcome(t.invoke(&op, args.into_inner(), &ctx).map(Json))
            }
            PlatformRequest::Stop { task } => {
                let owner = lock(&self.tasks).get(&task).cloned();
                let Some((_, t)) = owner.and_then(|tool| self.c.tools.iter().find(|(i, _)| i.id == tool)) else {
                    return outcome::<()>(Err(ToolError::Missing(format!("no running task \"{task}\" to stop"))));
                };
                let ctx = self.tool_ctx(env, t.id());
                outcome(t.stop(&task, &ctx))
            }
            PlatformRequest::RenderView { view, ctx } => {
                let v = self.view(&view)?;
                let vctx = ViewCtx {
                    args: ctx.into_inner(),
                    tool: self.tool_ctx(env, &view),
                };
                outcome(v.render(&vctx).and_then(|doc| checked(paddock_view::validate(&doc), serde_json::to_value(&doc))).map(Json))
            }
            PlatformRequest::ViewAction { view, action, args } => {
                let v = self.view(&view)?;
                let vctx = ViewCtx {
                    args: Value::Null,
                    tool: self.tool_ctx(env, &view),
                };
                let action = ViewAction { view, action, args };
                paddock_view::validate_action(&action).map_err(|e| invalid(e.to_string()))?;
                outcome(
                    v.action(&vctx, &action)
                        .and_then(|patch| checked(check_patch(&patch), serde_json::to_value(&patch)))
                        .map(Json),
                )
            }
            PlatformRequest::McpTool { tool, args, budget } => {
                let (info, t) = self.c.mcp.iter().find(|(i, _)| i.name == tool).ok_or_else(|| invalid(format!("no MCP tool \"{tool}\"")))?;
                info.args.validate(&args).map_err(|e| invalid(e.to_string()))?;
                let ctx = self.tool_ctx(env, &tool);
                let answer = t.call(args.into_inner(), budget, &ctx).and_then(|v| {
                    let size = v.to_string().len() as u64;
                    if size > budget.max_bytes {
                        Err(ToolError::Failed(format!("the answer is {size} bytes; the budget is {}", budget.max_bytes)))
                    } else {
                        Ok(v)
                    }
                });
                outcome(answer.map(Json))
            }
        }
    }

    fn view(&self, id: &str) -> Result<&Arc<dyn crate::plugin::View>, RemoteError> {
        self.c.views.iter().find(|(i, _)| i.id == id).map(|(_, v)| v).ok_or_else(|| invalid(format!("no view \"{id}\"")))
    }
}

/// A patch the app would apply: a whole document must validate; a replacement node is validated inside an empty document.
fn check_patch(patch: &ViewPatch) -> Result<(), paddock_view::ViewError> {
    match patch {
        ViewPatch::Doc(doc) => paddock_view::validate(doc),
        ViewPatch::Replace { node, .. } => paddock_view::validate(&ViewDoc {
            schema: paddock_view::SCHEMA,
            title: String::new(),
            root: node.clone(),
            etag: 0,
        }),
    }
}

/// A validated document or patch as JSON; a document the platform would refuse never leaves the plugin.
fn checked<E: std::fmt::Display>(valid: Result<(), E>, json: Result<Value, serde_json::Error>) -> Result<Value, ToolError> {
    valid.map_err(|e| ToolError::Failed(format!("the view is not valid ({e})")))?;
    json.map_err(ToolError::from)
}

impl Handler for PluginHandler {
    fn call(&self, ctx: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError> {
        let req: PlatformRequest = decode_request(body)?;
        self.dispatch(&PeerEnv(ctx), req)
    }
}
