//! A fake host for plugin unit tests: it calls a plugin **in this process** through a real `paddock-ipc` connection pair
//! (same frames, same handshake, same limits as in the app), answers the calls the plugin makes back from memory, and
//! records them.

use paddock_ipc::{
    CallCtx, Child, ChildSpec, Client, FrameLimit, Handler, Host, IpcError, Json, Peer,
    PeerOptions, RemoteError, codes, decode_request, encode_reply,
};
use paddock_plugin_api::Plugin;
use paddock_plugin_api::extension::Contract;
use paddock_plugin_api::host_file::HostFileRef;
use paddock_plugin_api::protocol::{
    DENIED, Described, Fail, HostCall, McpBudget, PLATFORM_PROTOCOL, PlatformRequest, SavedOutput,
};
use paddock_plugin_api::serve::PluginHandler;
use paddock_plugin_api::tools::ToolTask;
use paddock_testkit::Clock;
use paddock_view::{ViewAction, ViewDoc, ViewPatch};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Why a call to the plugin under test did not give its result.
#[derive(Debug, Clone, PartialEq)]
pub enum CallFailure {
    /// The plugin answered a typed failure (`Fail`): it is fine.
    Fail(Fail),
    /// The plugin refused the request (`invalid`, `unimplemented`, `version`, `panic`, ...).
    Remote(RemoteError),
    /// The connection failed.
    Ipc(IpcError),
}

impl std::fmt::Display for CallFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallFailure::Fail(x) => f.write_str(&x.message),
            CallFailure::Remote(r) => write!(f, "{}: {}", r.code, r.message),
            CallFailure::Ipc(e) => e.fmt(f),
        }
    }
}

impl From<IpcError> for CallFailure {
    fn from(e: IpcError) -> Self {
        match e {
            IpcError::Remote(r) => CallFailure::Remote(r),
            other => CallFailure::Ipc(other),
        }
    }
}

type ElevatedFn = Box<dyn Fn(&str, &str, Value) -> Result<Value, Fail> + Send + Sync>;

/// What the plugin did to the host while it worked.
#[derive(Default)]
struct Recorded {
    files: HashMap<u64, Vec<u8>>,
    reports: Vec<(String, Option<f32>, String)>,
    started: Vec<(String, ToolTask)>,
    finished: Vec<String>,
    outputs: BTreeMap<String, Vec<u8>>,
    saved: BTreeMap<String, SavedOutput>,
    logs: Vec<String>,
    elevated_calls: Vec<(String, String)>,
}

#[derive(Default)]
struct Services {
    rec: Mutex<Recorded>,
    next: Mutex<u64>,
    elevated: Mutex<Option<ElevatedFn>>,
}

impl Handler for Services {
    fn call(&self, _ctx: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError> {
        match decode_request::<HostCall>(body)? {
            // The ids a test uses to make the app answer in the other ways it can.
            HostCall::ReadFile { id: 0, .. } => {
                Err(RemoteError::new(codes::CANCELLED, "the call was cancelled"))
            }
            HostCall::ReadFile { id: DISK_FAILS, .. } => {
                Err(RemoteError::new(codes::INTERNAL, "the disk failed"))
            }
            HostCall::ReadFile { id, offset, len } => {
                let rec = lock(&self.rec);
                let file = rec.files.get(&id).ok_or_else(|| {
                    RemoteError::new(DENIED, format!("file {id} was not opened for this plugin"))
                })?;
                let start = usize::try_from(offset)
                    .unwrap_or(usize::MAX)
                    .min(file.len());
                let end = start.saturating_add(len as usize).min(file.len());
                encode_reply(&file[start..end].to_vec())
            }
            HostCall::Report {
                task,
                progress,
                note,
            } => {
                lock(&self.rec).reports.push((task, progress, note));
                encode_reply(&())
            }
            HostCall::TaskStarted { tool, task } => {
                lock(&self.rec).started.push((tool, task));
                encode_reply(&())
            }
            HostCall::TaskFinished { task } => {
                lock(&self.rec).finished.push(task);
                encode_reply(&())
            }
            HostCall::Output { name, bytes, last } => {
                let mut rec = lock(&self.rec);
                rec.outputs
                    .entry(name.clone())
                    .or_default()
                    .extend_from_slice(&bytes);
                let saved = last.then(|| SavedOutput {
                    name: name.clone(),
                    bytes: rec.outputs[&name].len() as u64,
                });
                if let Some(s) = &saved {
                    rec.saved.insert(name, s.clone());
                }
                encode_reply(&saved)
            }
            HostCall::Elevated { tool, op, args, .. } => {
                lock(&self.rec)
                    .elevated_calls
                    .push((tool.clone(), op.clone()));
                let reply: Result<Json, Fail> = match &*lock(&self.elevated) {
                    Some(f) => f(&tool, &op, args.into_inner()).map(Json),
                    None => Err(Fail::new("the fake host runs no elevated operations")),
                };
                encode_reply(&reply)
            }
            HostCall::Log { lines, .. } => {
                lock(&self.rec).logs.extend(lines);
                encode_reply(&())
            }
        }
    }
}

/// A plugin under test and the app side of its connection.
pub struct FakeHost {
    host_side: Peer,
    // Kept so the plugin's side of the connection and the private socket live as long as the fake host.
    _plugin_side: Option<Peer>,
    _host: Option<Host>,
    // The started program of `launch`; its whole process tree ends when the fake host is dropped.
    _child: Option<Child>,
    services: Arc<Services>,
}

impl FakeHost {
    /// Connects `plugin` (running in this process) to a fake app. Calls wait on the system clock.
    pub fn start(plugin: Arc<dyn Plugin>) -> Result<FakeHost, IpcError> {
        FakeHost::start_with(plugin, FrameLimit::default(), None)
    }

    /// [`FakeHost::start`] with a frame limit (a small one makes an oversized frame cheap to cause) and, when given, the
    /// clock the app side's call timeouts run on.
    pub fn start_with(
        plugin: Arc<dyn Plugin>,
        limit: FrameLimit,
        clock: Option<Arc<dyn Clock>>,
    ) -> Result<FakeHost, IpcError> {
        FakeHost::start_handler(Arc::new(PluginHandler::new(plugin, false)), limit, clock)
    }

    /// [`FakeHost::start_with`] for any handler on the plugin side: how the conformance checks themselves are tested against
    /// a plugin that misbehaves.
    pub fn start_handler(
        handler: Arc<dyn Handler>,
        limit: FrameLimit,
        clock: Option<Arc<dyn Clock>>,
    ) -> Result<FakeHost, IpcError> {
        let mut host = Host::listen(limit, PLATFORM_PROTOCOL).map_err(IpcError::from)?;
        let (endpoint, token) = (host.endpoint().to_string(), host.token().clone());
        let connect = std::thread::spawn(move || {
            Client::connect(&endpoint, &token, limit, PLATFORM_PROTOCOL)
        });
        let host_conn = host.accept(Duration::from_secs(10))?;
        let plugin_conn = connect
            .join()
            .map_err(|_| IpcError::Closed("the plugin side panicked".into()))??;
        let services = Arc::new(Services::default());
        let mut opts = PeerOptions::new("plugin under test");
        if let Some(c) = clock {
            opts.clock = c;
        }
        let host_side = Peer::start(host_conn, services.clone(), opts)?;
        let plugin_side = Peer::start(plugin_conn, handler, PeerOptions::new("the app"))?;
        Ok(FakeHost {
            host_side,
            _plugin_side: Some(plugin_side),
            _host: Some(host),
            _child: None,
            services,
        })
    }

    /// Starts the plugin **program** `program` as the app would (same launcher, bootstrap and handshake as the real host,
    /// speaking the platform protocol), and answers its calls back from memory like [`FakeHost::start`]. `args` are passed to
    /// the program. The program's process tree ends when the fake host is dropped.
    ///
    /// # Errors
    ///
    /// When the program cannot be started, exits before it connects, or speaks another protocol (the text says who must
    /// update).
    pub fn launch(program: &Path, args: &[&str]) -> Result<FakeHost, IpcError> {
        let services = Arc::new(Services::default());
        let mut spec = ChildSpec::new(program, "plugin under test");
        spec.protocol = PLATFORM_PROTOCOL;
        spec.host_name = "the fake host".into();
        spec.start_timeout = Duration::from_secs(30);
        spec.args = args.iter().map(Into::into).collect();
        let child = paddock_ipc::launch(&spec, services.clone())?;
        Ok(FakeHost {
            host_side: child.peer().clone(),
            _plugin_side: None,
            _host: None,
            _child: Some(child),
            services,
        })
    }

    /// The app side of the connection.
    pub fn peer(&self) -> &Peer {
        &self.host_side
    }

    /// Whether the connection is closed (the plugin side ended or broke the protocol).
    pub fn is_closed(&self) -> bool {
        self.host_side.is_closed()
    }

    /// Opens a file for the plugin with this content.
    pub fn add_file(&self, content: impl Into<Vec<u8>>) -> HostFileRef {
        let content = content.into();
        let mut next = lock(&self.services.next);
        *next += 1;
        let r = HostFileRef {
            id: *next,
            len: content.len() as u64,
        };
        lock(&self.services.rec).files.insert(r.id, content);
        r
    }

    /// Makes the plugin's elevated calls run `f`.
    pub fn on_elevated(
        &self,
        f: impl Fn(&str, &str, Value) -> Result<Value, Fail> + Send + Sync + 'static,
    ) {
        *lock(&self.services.elevated) = Some(Box::new(f));
    }

    /// Sends a request and returns the reply bytes.
    pub fn raw(&self, request: &PlatformRequest) -> Result<Vec<u8>, IpcError> {
        let body = paddock_ipc::wire::encode(request).map_err(IpcError::Protocol)?;
        self.host_side.call(body, Some(Duration::from_secs(60)))
    }

    fn ask<T: DeserializeOwned>(&self, request: &PlatformRequest) -> Result<T, CallFailure> {
        let bytes = self.raw(request)?;
        paddock_ipc::wire::decode(&bytes).map_err(|m| CallFailure::Ipc(IpcError::Protocol(m)))
    }

    fn json(&self, request: &PlatformRequest) -> Result<Value, CallFailure> {
        match self.ask::<Result<Json, Fail>>(request)? {
            Ok(j) => Ok(j.into_inner()),
            Err(f) => Err(CallFailure::Fail(f)),
        }
    }

    /// What the plugin says about itself.
    pub fn describe(&self) -> Result<Described, CallFailure> {
        let json: Json = self.ask(&PlatformRequest::Describe)?;
        serde_json::from_value(json.into_inner())
            .map_err(|e| CallFailure::Ipc(IpcError::Protocol(e.to_string())))
    }

    /// Gives the plugin its settings (already validated by the app in real life; the plugin validates again).
    pub fn apply_settings(&self, values: Value) -> Result<(), CallFailure> {
        self.ask(&PlatformRequest::ApplySettings { json: Json(values) })
    }

    /// Runs a tool operation.
    pub fn invoke(&self, tool: &str, op: &str, args: Value) -> Result<Value, CallFailure> {
        self.json(&PlatformRequest::Invoke {
            tool: tool.into(),
            op: op.into(),
            args: Json(args),
        })
    }

    /// Renders a view.
    pub fn render_view(&self, view: &str, ctx: Value) -> Result<ViewDoc, CallFailure> {
        let v = self.json(&PlatformRequest::RenderView {
            view: view.into(),
            ctx: Json(ctx),
        })?;
        serde_json::from_value(v).map_err(|e| CallFailure::Ipc(IpcError::Protocol(e.to_string())))
    }

    /// Delivers a view action.
    pub fn view_action(&self, action: &ViewAction) -> Result<ViewPatch, CallFailure> {
        let v = self.json(&PlatformRequest::ViewAction {
            view: action.view.clone(),
            action: action.action.clone(),
            args: action.args.clone(),
        })?;
        serde_json::from_value(v).map_err(|e| CallFailure::Ipc(IpcError::Protocol(e.to_string())))
    }

    /// Calls an MCP tool.
    pub fn mcp_tool(
        &self,
        tool: &str,
        args: Value,
        budget: McpBudget,
    ) -> Result<Value, CallFailure> {
        self.json(&PlatformRequest::McpTool {
            tool: tool.into(),
            args: Json(args),
            budget,
        })
    }

    /// Stops a background task.
    pub fn stop_task(&self, task: &str) -> Result<(), CallFailure> {
        match self.ask::<Result<(), Fail>>(&PlatformRequest::Stop { task: task.into() })? {
            Ok(()) => Ok(()),
            Err(f) => Err(CallFailure::Fail(f)),
        }
    }

    /// Calls the contract `C` at `minor`.
    pub fn extension<C: Contract>(
        &self,
        minor: u16,
        request: &C::Request,
    ) -> Result<C::Reply, CallFailure> {
        let body = paddock_ipc::wire::encode(request)
            .map_err(|m| CallFailure::Ipc(IpcError::Protocol(m)))?;
        let bytes = self.raw(&PlatformRequest::Extension {
            ns: C::NS.into(),
            minor,
            body,
        })?;
        paddock_ipc::wire::decode(&bytes).map_err(|m| CallFailure::Ipc(IpcError::Protocol(m)))
    }

    /// Progress reports the plugin made: `(task, progress, note)`.
    pub fn reports(&self) -> Vec<(String, Option<f32>, String)> {
        lock(&self.services.rec).reports.clone()
    }

    /// Tasks the plugin announced, with the tool that started them.
    pub fn tasks_started(&self) -> Vec<(String, ToolTask)> {
        lock(&self.services.rec).started.clone()
    }

    /// Ids of tasks the plugin ended.
    pub fn tasks_finished(&self) -> Vec<String> {
        lock(&self.services.rec).finished.clone()
    }

    /// The bytes of an output file the plugin wrote (finished or not).
    pub fn output(&self, name: &str) -> Option<Vec<u8>> {
        lock(&self.services.rec).outputs.get(name).cloned()
    }

    /// Where the fake host "saved" a finished output file.
    pub fn saved_output(&self, name: &str) -> Option<SavedOutput> {
        lock(&self.services.rec).saved.get(name).cloned()
    }

    /// Log lines the plugin sent.
    pub fn logs(&self) -> Vec<String> {
        lock(&self.services.rec).logs.clone()
    }

    /// The elevated operations the plugin asked for: `(tool, op)`.
    pub fn elevated_calls(&self) -> Vec<(String, String)> {
        lock(&self.services.rec).elevated_calls.clone()
    }
}

/// A file id the fake host answers with a disk failure (the largest id a tool argument can name).
pub const DISK_FAILS: u64 = i64::MAX as u64;

/// The refusal code a request the plugin does not know comes back with.
pub const INVALID: &str = codes::INVALID;
