//! What a plugin implements: one trait per [`Kind`], assembled into a [`Plugin`] that [`crate::serve`] runs.
//! [`Plugin::answer`] is the whole dispatch (no IPC), so a plugin's tests call it directly.

use crate::manifest::{Kind, Manifest, ManifestError};
use crate::protocol::{
    code, Description, EndpointInfo, EndpointRequest, EndpointResponse, FilterVerdict, GenerateItem, Reply, Request, ToolInfo,
    CONTRACT_VERSION,
};
use pitwall_ipc::{Json, RemoteError};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

/// A typed error a plugin answers with (the call failed; the plugin is fine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginError {
    /// A code from [`crate::protocol::code`].
    pub code: i32,
    /// What went wrong, for the user (never secrets or request content).
    pub message: String,
}

impl PluginError {
    /// An error with `code`.
    pub fn new(code: i32, message: impl Into<String>) -> PluginError {
        PluginError {
            code,
            message: message.into(),
        }
    }

    /// The request is not valid ([`code::INVALID`]).
    pub fn invalid(message: impl Into<String>) -> PluginError {
        PluginError::new(code::INVALID, message)
    }

    /// Something the request names does not exist ([`code::NOT_FOUND`]).
    pub fn not_found(message: impl Into<String>) -> PluginError {
        PluginError::new(code::NOT_FOUND, message)
    }

    /// The plugin failed ([`code::FAILED`]).
    pub fn failed(message: impl Into<String>) -> PluginError {
        PluginError::new(code::FAILED, message)
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PluginError {}

impl From<PluginError> for RemoteError {
    fn from(e: PluginError) -> RemoteError {
        RemoteError::new(e.code, e.message)
    }
}

/// What the host tells every plugin about its environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginContext {
    /// The plugin's private data folder (the only folder it may write), from `PITWALL_PLUGIN_DATA`.
    pub data_dir: Option<PathBuf>,
}

impl PluginContext {
    /// The context of this process (read from its environment).
    pub fn from_env() -> PluginContext {
        PluginContext {
            data_dir: std::env::var_os(DATA_DIR_ENV).map(PathBuf::from).filter(|p| p.is_absolute()),
        }
    }
}

/// The environment variable that names a plugin's data folder.
pub const DATA_DIR_ENV: &str = "PITWALL_PLUGIN_DATA";

/// A filter: sees each request before it reaches the model, and each response after.
pub trait Filter: Send + Sync {
    /// Forwards the request (possibly changed) or rejects it.
    fn filter_request(&self, body: Value) -> Result<FilterVerdict, PluginError>;

    /// The response, possibly changed (default: unchanged).
    fn filter_response(&self, body: Value) -> Result<Value, PluginError> {
        Ok(body)
    }
}

/// Extra HTTP endpoints, served by Pitwall under `/plugins/<id>/...`.
pub trait Endpoints: Send + Sync {
    /// The endpoints served.
    fn endpoints(&self) -> Vec<EndpointInfo>;

    /// Answers one request.
    fn handle(&self, request: EndpointRequest) -> Result<EndpointResponse, PluginError>;
}

/// Tools a model or the user can invoke.
pub trait Tools: Send + Sync {
    /// The tools offered.
    fn tools(&self) -> Vec<ToolInfo>;

    /// Runs `tool` with `args`.
    fn invoke(&self, tool: &str, args: Value) -> Result<Value, PluginError>;
}

/// Where an engine sends generated text.
pub trait TokenSink {
    /// Sends one piece of text; fails when the host cancelled (stop generating then).
    fn text(&mut self, text: &str) -> Result<(), PluginError>;

    /// Whether the host cancelled the generation.
    fn cancelled(&self) -> bool;
}

/// An engine: loads models and generates text (minimal for now: one stream of text per request).
pub trait Engine: Send + Sync {
    /// Loads `model`; returns a handle for later requests.
    fn load(&self, model: &str, options: Value) -> Result<String, PluginError>;

    /// Unloads a model.
    fn unload(&self, handle: &str) -> Result<(), PluginError>;

    /// Generates for `request`, sending text to `out`; returns a summary (finish reason, token counts).
    fn generate(&self, handle: &str, request: Value, out: &mut dyn TokenSink) -> Result<Value, PluginError>;
}

type ConfigureFn = dyn Fn(Value, &PluginContext) -> Result<(), PluginError> + Send + Sync;

/// A plugin: its manifest and its implementations, built with the methods below and run by [`crate::serve`].
#[derive(Clone)]
pub struct Plugin {
    manifest: Result<Manifest, ManifestError>,
    context: PluginContext,
    configure: Option<Arc<ConfigureFn>>,
    filter: Option<Arc<dyn Filter>>,
    endpoints: Option<Arc<dyn Endpoints>>,
    tools: Option<Arc<dyn Tools>>,
    engine: Option<Arc<dyn Engine>>,
}

impl std::fmt::Debug for Plugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plugin").field("manifest", &self.manifest).finish_non_exhaustive()
    }
}

impl Plugin {
    /// A plugin described by its manifest's text (embed the file: `include_str!("../pitwall-plugin.toml")`). An
    /// invalid manifest is reported by [`crate::serve`] and to every request.
    pub fn new(manifest_toml: &str) -> Plugin {
        Plugin {
            manifest: Manifest::parse(manifest_toml),
            context: PluginContext::from_env(),
            configure: None,
            filter: None,
            endpoints: None,
            tools: None,
            engine: None,
        }
    }

    /// Replaces the context (tests).
    pub fn with_context(mut self, context: PluginContext) -> Plugin {
        self.context = context;
        self
    }

    /// Receives the user's settings after every start.
    pub fn configure(mut self, f: impl Fn(Value, &PluginContext) -> Result<(), PluginError> + Send + Sync + 'static) -> Plugin {
        self.configure = Some(Arc::new(f));
        self
    }

    /// The plugin's filter (kind `filter`).
    pub fn filter(mut self, f: impl Filter + 'static) -> Plugin {
        self.filter = Some(Arc::new(f));
        self
    }

    /// The plugin's endpoints (kind `endpoint`).
    pub fn endpoints(mut self, e: impl Endpoints + 'static) -> Plugin {
        self.endpoints = Some(Arc::new(e));
        self
    }

    /// The plugin's tools (kind `tool`).
    pub fn tools(mut self, t: impl Tools + 'static) -> Plugin {
        self.tools = Some(Arc::new(t));
        self
    }

    /// The plugin's engine (kind `engine`).
    pub fn engine(mut self, e: impl Engine + 'static) -> Plugin {
        self.engine = Some(Arc::new(e));
        self
    }

    /// The kinds implemented.
    pub fn kinds(&self) -> Vec<Kind> {
        let mut k = Vec::new();
        if self.engine.is_some() {
            k.push(Kind::Engine);
        }
        if self.endpoints.is_some() {
            k.push(Kind::Endpoint);
        }
        if self.tools.is_some() {
            k.push(Kind::Tool);
        }
        if self.filter.is_some() {
            k.push(Kind::Filter);
        }
        k
    }

    /// The manifest, if it is valid and matches what the plugin implements.
    pub fn check(&self) -> Result<&Manifest, String> {
        let m = self.manifest.as_ref().map_err(|e| e.to_string())?;
        if m.contract != CONTRACT_VERSION {
            return Err(format!(
                "the manifest says contract {} but this plugin is built with pitwall-plugin-api for {CONTRACT_VERSION}",
                m.contract
            ));
        }
        let kinds = self.kinds();
        if m.kinds != kinds {
            let names = |k: &[Kind]| k.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ");
            return Err(format!(
                "the manifest lists kinds [{}] but the plugin implements [{}]",
                names(&m.kinds),
                names(&kinds)
            ));
        }
        Ok(m)
    }

    fn describe(&self, m: &Manifest) -> Description {
        Description {
            id: m.id.clone(),
            version: m.version.clone(),
            contract: CONTRACT_VERSION,
            kinds: self.kinds(),
            tools: self.tools.as_ref().map(|t| t.tools()).unwrap_or_default(),
            endpoints: self.endpoints.as_ref().map(|e| e.endpoints()).unwrap_or_default(),
        }
    }

    /// Answers one request; `out` receives a generation's items. The whole dispatch of [`crate::serve`].
    pub fn answer(&self, request: Request, out: &mut dyn ItemSink) -> Result<Reply, PluginError> {
        let m = self.check().map_err(PluginError::failed)?;
        let unsupported = |k: Kind| PluginError::new(code::UNSUPPORTED, format!("this plugin is not of kind {k}"));
        match request {
            Request::Describe => Ok(Reply::Described(self.describe(m))),
            Request::Configure { settings } => {
                if let Some(f) = &self.configure {
                    f(settings.into_inner(), &self.context)?;
                }
                Ok(Reply::Configured)
            }
            Request::FilterRequest { body } => {
                let f = self.filter.as_ref().ok_or_else(|| unsupported(Kind::Filter))?;
                f.filter_request(body.into_inner()).map(Reply::FilteredRequest)
            }
            Request::FilterResponse { body } => {
                let f = self.filter.as_ref().ok_or_else(|| unsupported(Kind::Filter))?;
                f.filter_response(body.into_inner()).map(|v| Reply::FilteredResponse(Json(v)))
            }
            Request::Endpoint(req) => {
                let e = self.endpoints.as_ref().ok_or_else(|| unsupported(Kind::Endpoint))?;
                e.handle(req).map(Reply::Endpoint)
            }
            Request::InvokeTool { tool, args } => {
                let t = self.tools.as_ref().ok_or_else(|| unsupported(Kind::Tool))?;
                t.invoke(&tool, args.into_inner()).map(|v| Reply::Tool(Json(v)))
            }
            Request::EngineLoad { model, options } => {
                let e = self.engine.as_ref().ok_or_else(|| unsupported(Kind::Engine))?;
                e.load(&model, options.into_inner()).map(|handle| Reply::Loaded { handle })
            }
            Request::EngineUnload { handle } => {
                let e = self.engine.as_ref().ok_or_else(|| unsupported(Kind::Engine))?;
                e.unload(&handle).map(|()| Reply::Unloaded)
            }
            Request::EngineGenerate { handle, request } => {
                let e = self.engine.as_ref().ok_or_else(|| unsupported(Kind::Engine))?;
                let mut sink = Tokens(out);
                e.generate(&handle, request.into_inner(), &mut sink).map(|v| Reply::Generated(Json(v)))
            }
        }
    }
}

/// Where [`Plugin::answer`] sends the items of a streamed reply.
pub trait ItemSink {
    /// Sends one item (waits for the host's credit); fails when the host cancelled.
    fn item(&mut self, item: GenerateItem) -> Result<(), PluginError>;

    /// Whether the host cancelled the call.
    fn cancelled(&self) -> bool;
}

/// An [`ItemSink`] that collects items (for tests and for requests that do not stream).
#[derive(Debug, Default)]
pub struct Collect(pub Vec<GenerateItem>);

impl ItemSink for Collect {
    fn item(&mut self, item: GenerateItem) -> Result<(), PluginError> {
        self.0.push(item);
        Ok(())
    }

    fn cancelled(&self) -> bool {
        false
    }
}

/// An engine's [`TokenSink`] over the call's [`ItemSink`].
struct Tokens<'a>(&'a mut dyn ItemSink);

impl TokenSink for Tokens<'_> {
    fn text(&mut self, text: &str) -> Result<(), PluginError> {
        self.0.item(GenerateItem::Text(text.to_string()))
    }

    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MANIFEST: &str = "id='x'\nname='X'\nversion='1.2.3'\ncontract='1.0'\nkinds=['engine','endpoint','tool','filter']\nexecutable='x'";

    struct Upper;
    impl Filter for Upper {
        fn filter_request(&self, body: Value) -> Result<FilterVerdict, PluginError> {
            if body.get("reject").is_some() {
                return Ok(FilterVerdict::reject(403, "rejected"));
            }
            Ok(FilterVerdict::forward(json!({"seen": body})))
        }
    }

    struct Echo;
    impl Endpoints for Echo {
        fn endpoints(&self) -> Vec<EndpointInfo> {
            vec![EndpointInfo {
                method: "GET".into(),
                path: "/echo".into(),
                description: "echo".into(),
            }]
        }
        fn handle(&self, r: EndpointRequest) -> Result<EndpointResponse, PluginError> {
            Ok(EndpointResponse::text(200, r.path))
        }
    }

    struct Add;
    impl Tools for Add {
        fn tools(&self) -> Vec<ToolInfo> {
            vec![ToolInfo {
                name: "add".into(),
                description: "adds".into(),
                parameters: json!({"type": "object"}).into(),
            }]
        }
        fn invoke(&self, tool: &str, args: Value) -> Result<Value, PluginError> {
            match tool {
                "add" => Ok(json!(args["a"].as_i64().unwrap_or(0) + args["b"].as_i64().unwrap_or(0))),
                _ => Err(PluginError::not_found(format!("no tool {tool}"))),
            }
        }
    }

    struct Words;
    impl Engine for Words {
        fn load(&self, model: &str, _: Value) -> Result<String, PluginError> {
            Ok(format!("h-{model}"))
        }
        fn unload(&self, _: &str) -> Result<(), PluginError> {
            Ok(())
        }
        fn generate(&self, _: &str, request: Value, out: &mut dyn TokenSink) -> Result<Value, PluginError> {
            for w in request["prompt"].as_str().unwrap_or("").split(' ') {
                if out.cancelled() {
                    break;
                }
                out.text(w)?;
            }
            Ok(json!({"finish": "stop"}))
        }
    }

    fn all() -> Plugin {
        Plugin::new(MANIFEST)
            .filter(Upper)
            .endpoints(Echo)
            .tools(Add)
            .engine(Words)
            .configure(|s, ctx| match s.get("fail") {
                Some(_) => Err(PluginError::invalid(format!("bad settings ({:?})", ctx.data_dir))),
                None => Ok(()),
            })
    }

    fn ask(p: &Plugin, r: Request) -> Result<Reply, PluginError> {
        p.answer(r, &mut Collect::default())
    }

    #[test]
    fn every_kind_is_dispatched() {
        let p = all();
        assert!(format!("{p:?}").contains("Plugin"));
        match ask(&p, Request::Describe).unwrap() {
            Reply::Described(d) => {
                assert_eq!((d.id.as_str(), d.version.as_str()), ("x", "1.2.3"));
                assert_eq!(d.kinds, vec![Kind::Engine, Kind::Endpoint, Kind::Tool, Kind::Filter]);
                assert_eq!(d.tools.len(), 1);
                assert_eq!(d.endpoints[0].path, "/echo");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(ask(&p, Request::Configure { settings: json!({}).into() }).unwrap(), Reply::Configured);
        assert_eq!(ask(&p, Request::Configure { settings: json!({"fail": 1}).into() }).unwrap_err().code, code::INVALID);
        assert_eq!(
            ask(&p, Request::FilterRequest { body: json!({"a": 1}).into() }).unwrap(),
            Reply::FilteredRequest(FilterVerdict::forward(json!({"seen": {"a": 1}})))
        );
        assert!(matches!(
            ask(&p, Request::FilterRequest { body: json!({"reject": 1}).into() }).unwrap(),
            Reply::FilteredRequest(FilterVerdict::Reject { status: 403, .. })
        ));
        assert_eq!(
            ask(&p, Request::FilterResponse { body: json!({"r": 1}).into() }).unwrap(),
            Reply::FilteredResponse(json!({"r": 1}).into())
        );
        let req = EndpointRequest {
            method: "GET".into(),
            path: "/echo".into(),
            query: String::new(),
            content_type: None,
            body: Vec::new(),
        };
        assert_eq!(ask(&p, Request::Endpoint(req)).unwrap(), Reply::Endpoint(EndpointResponse::text(200, "/echo")));
        assert_eq!(
            ask(
                &p,
                Request::InvokeTool {
                    tool: "add".into(),
                    args: json!({"a": 2, "b": 3}).into()
                }
            )
            .unwrap(),
            Reply::Tool(json!(5).into())
        );
        let missing = ask(
            &p,
            Request::InvokeTool {
                tool: "nope".into(),
                args: json!({}).into(),
            },
        );
        assert_eq!(missing.unwrap_err().code, code::NOT_FOUND);
        assert_eq!(
            ask(
                &p,
                Request::EngineLoad {
                    model: "m".into(),
                    options: json!({}).into()
                }
            )
            .unwrap(),
            Reply::Loaded { handle: "h-m".into() }
        );
        assert_eq!(ask(&p, Request::EngineUnload { handle: "h".into() }).unwrap(), Reply::Unloaded);
        let mut items = Collect::default();
        let done = p
            .answer(
                Request::EngineGenerate {
                    handle: "h".into(),
                    request: json!({"prompt": "a b c"}).into(),
                },
                &mut items,
            )
            .unwrap();
        assert_eq!(done, Reply::Generated(json!({"finish": "stop"}).into()));
        assert_eq!(items.0.len(), 3);
    }

    #[test]
    fn missing_kinds_and_bad_manifests_are_errors() {
        let only_filter = "id='x'\nname='X'\nversion='1'\ncontract='1.0'\nkinds=['filter']\nexecutable='x'";
        let p = Plugin::new(only_filter).filter(Upper);
        assert!(p.check().is_ok());
        for r in [
            Request::InvokeTool {
                tool: "t".into(),
                args: json!({}).into(),
            },
            Request::EngineUnload { handle: "h".into() },
            Request::EngineLoad {
                model: "m".into(),
                options: json!({}).into(),
            },
            Request::EngineGenerate {
                handle: "h".into(),
                request: json!({}).into(),
            },
            Request::Endpoint(EndpointRequest {
                method: "GET".into(),
                path: "/".into(),
                query: String::new(),
                content_type: None,
                body: Vec::new(),
            }),
        ] {
            assert_eq!(ask(&p, r).unwrap_err().code, code::UNSUPPORTED);
        }
        assert!(ask(&p, Request::Configure { settings: json!({}).into() }).is_ok(), "no configure hook is fine");
        // The manifest must list exactly what is implemented.
        let lying = Plugin::new(only_filter).tools(Add);
        assert!(lying.check().unwrap_err().contains("implements [tool]"));
        assert!(ask(&lying, Request::Describe).is_err());
        let none = Plugin::new(only_filter);
        assert!(none.check().is_err());
        assert!(Plugin::new("garbage").check().unwrap_err().contains("pitwall-plugin.toml"));
        let old = Plugin::new(&only_filter.replace("1.0", "0.9")).filter(Upper);
        assert!(old.check().unwrap_err().contains("contract 0.9"));
        // Errors become typed remote errors.
        let e: RemoteError = PluginError::failed("boom").into();
        assert_eq!((e.code, e.message.as_str()), (code::FAILED, "boom"));
        assert_eq!(PluginError::invalid("i").to_string(), "i");
        let ctx = PluginContext::default();
        assert!(Plugin::new(only_filter).with_context(ctx.clone()).context == ctx);
        assert!(PluginContext::from_env().data_dir.is_none_or(|d| d.is_absolute()));
    }
}
