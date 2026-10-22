//! [`PluginHost`]: the server's one door to plugins. It discovers installed plugins, applies the user's settings,
//! and offers an async API whose every call is guarded ([`crate::instance`]): a plugin that crashes, hangs, floods
//! or exhausts its memory fails that call with a [`PluginError`] attributed to it, and never takes the server down.

use crate::config::{FilterFailure, HostConfig, PluginSettings};
use crate::discover::{discover, Problem};
use crate::error::{ErrorKind, PluginError, Rejection};
use crate::events::{HostEvent, EVENT_BUFFER};
use crate::instance::{Effective, EngineModel, GenerateEvent, Instance, PluginStatus};
use pitwall_plugin_api::{EndpointRequest, EndpointResponse, FilterVerdict, Json, Kind, Reply, Request};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio::sync::{broadcast, mpsc};

fn read<T>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(|p| p.into_inner())
}

fn write<T>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(|p| p.into_inner())
}

/// Runs blocking plugin work off the async runtime's worker threads.
async fn blocking<T: Send + 'static>(plugin: &str, f: impl FnOnce() -> Result<T, PluginError> + Send + 'static) -> Result<T, PluginError> {
    match tokio::task::spawn_blocking(f).await {
        Ok(r) => r,
        Err(e) => Err(PluginError::new(plugin, ErrorKind::Crashed, format!("the call to plugin \"{plugin}\" failed in Pitwall ({e})"))),
    }
}

/// A stream of generated text from an engine plugin. Dropping it cancels the generation.
pub struct GenerateStream {
    rx: mpsc::Receiver<Result<GenerateEvent, PluginError>>,
}

impl GenerateStream {
    /// The next piece of text, the summary at the end, or the failure; `None` after the end.
    pub async fn next(&mut self) -> Option<Result<GenerateEvent, PluginError>> {
        self.rx.recv().await
    }
}

/// The plugin host (see the module docs). Cheap to share behind an `Arc`; all methods take `&self`.
pub struct PluginHost {
    config: Arc<HostConfig>,
    settings: RwLock<PluginSettings>,
    plugins: RwLock<BTreeMap<String, Arc<Instance>>>,
    problems: RwLock<Vec<Problem>>,
    events: broadcast::Sender<HostEvent>,
}

impl PluginHost {
    /// A host with no plugins yet; call [`PluginHost::rescan`] to discover them.
    pub fn new(config: HostConfig, settings: PluginSettings) -> PluginHost {
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        PluginHost {
            config: Arc::new(config),
            settings: RwLock::new(settings),
            plugins: RwLock::new(BTreeMap::new()),
            problems: RwLock::new(Vec::new()),
            events,
        }
    }

    fn effective(&self, id: &str) -> Effective {
        let s = read(&self.settings);
        Effective {
            enabled: s.enabled(id),
            network: s.network_granted(id),
            config: s.config_of(id),
        }
    }

    /// (Re)discovers the installed plugins: new ones are added, removed or changed ones are stopped and replaced,
    /// unchanged ones keep running. Returns the folders that are not usable plugins. Reads the file system (call it
    /// at start-up or from a blocking task).
    pub fn rescan(&self) -> Vec<Problem> {
        let found = discover(&self.config);
        let mut next = BTreeMap::new();
        {
            let current = read(&self.plugins);
            for d in found.plugins {
                let id = d.manifest.id.clone();
                let instance = match current.get(&id) {
                    Some(existing) if existing.found == d => existing.clone(),
                    _ => Arc::new(Instance::new(d, self.config.clone(), self.effective(&id), self.events.clone())),
                };
                next.insert(id, instance);
            }
        }
        let old = std::mem::replace(&mut *write(&self.plugins), next);
        let kept = read(&self.plugins);
        for (id, instance) in old {
            if !kept.get(&id).is_some_and(|k| Arc::ptr_eq(k, &instance)) {
                instance.stop();
            }
        }
        drop(kept);
        *write(&self.problems) = found.problems.clone();
        found.problems
    }

    /// Applies the user's decisions; plugins whose settings changed are restarted on their next use.
    pub fn apply_settings(&self, settings: PluginSettings) {
        *write(&self.settings) = settings;
        let plugins: Vec<Arc<Instance>> = read(&self.plugins).values().cloned().collect();
        for p in plugins {
            p.apply(self.effective(p.id()));
        }
    }

    /// Every discovered plugin's status.
    pub fn plugins(&self) -> Vec<PluginStatus> {
        read(&self.plugins).values().map(|p| p.status()).collect()
    }

    /// The folders of the last scan that are not usable plugins.
    pub fn problems(&self) -> Vec<Problem> {
        read(&self.problems).clone()
    }

    /// Events (starts, failures, quarantines, stops) from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<HostEvent> {
        self.events.subscribe()
    }

    /// Lifts a plugin's quarantine (the user asked to try again).
    pub fn reset(&self, plugin: &str) -> Result<(), PluginError> {
        self.get(plugin)?.reset();
        Ok(())
    }

    /// The last lines a running plugin wrote to standard error (diagnostics only; never logged by default).
    pub fn diagnostics(&self, plugin: &str) -> Result<Vec<String>, PluginError> {
        Ok(self.get(plugin)?.stderr_tail())
    }

    /// Stops every plugin process (they start again on their next use).
    pub fn stop_all(&self) {
        for p in read(&self.plugins).values() {
            p.stop();
        }
    }

    fn get(&self, plugin: &str) -> Result<Arc<Instance>, PluginError> {
        read(&self.plugins)
            .get(plugin)
            .cloned()
            .ok_or_else(|| PluginError::new(plugin, ErrorKind::NotFound, format!("no plugin \"{plugin}\" is installed")))
    }

    fn of_kind(&self, plugin: &str, kind: Kind) -> Result<Arc<Instance>, PluginError> {
        let p = self.get(plugin)?;
        if !p.found.manifest.has(kind) {
            return Err(PluginError::new(
                plugin,
                ErrorKind::Unsupported,
                format!("plugin \"{plugin}\" is not of kind {kind}"),
            ));
        }
        Ok(p)
    }

    /// The enabled filter plugins, in id order.
    fn filters(&self) -> Vec<Arc<Instance>> {
        let settings = read(&self.settings);
        read(&self.plugins)
            .values()
            .filter(|p| p.found.manifest.has(Kind::Filter) && settings.enabled(p.id()))
            .cloned()
            .collect()
    }

    fn filter_failed(&self, e: PluginError) -> Option<Rejection> {
        match self.config.filter_failure {
            FilterFailure::Skip => None,
            FilterFailure::Reject => Some(Rejection {
                status: 503,
                message: format!("the request could not be filtered: {}", e.message),
                plugin: e.plugin,
            }),
        }
    }

    /// Runs a request body (OpenAI-compatible JSON) through every enabled filter plugin, in id order. `Ok` is the
    /// body to send to the model; `Err` is a refusal for the client (a filter rejected it, or a filter failed and
    /// [`HostConfig::filter_failure`] is [`FilterFailure::Reject`]).
    pub async fn filter_request(&self, mut body: Value) -> Result<Value, Rejection> {
        let timeout = self.config.timeouts.filter;
        for p in self.filters() {
            let id = p.id().to_string();
            let request = Request::FilterRequest { body: Json(body.clone()) };
            let result = blocking(&id, {
                let p = p.clone();
                move || match p.call(&request, timeout)? {
                    Reply::FilteredRequest(v) => Ok(v),
                    other => Err(p.unexpected(p.generation(), "FilterRequest", &other)),
                }
            })
            .await;
            match result {
                Ok(FilterVerdict::Forward(next)) => body = next.into_inner(),
                Ok(FilterVerdict::Reject { status, message }) => {
                    return Err(Rejection {
                        plugin: id,
                        status: status.clamp(400, 599),
                        message,
                    })
                }
                Err(e) => {
                    if let Some(r) = self.filter_failed(e) {
                        return Err(r);
                    }
                }
            }
        }
        Ok(body)
    }

    /// Runs a response body through every enabled filter plugin, in id order (same failure rule as
    /// [`PluginHost::filter_request`]).
    pub async fn filter_response(&self, mut body: Value) -> Result<Value, Rejection> {
        let timeout = self.config.timeouts.filter;
        for p in self.filters() {
            let id = p.id().to_string();
            let request = Request::FilterResponse { body: Json(body.clone()) };
            let result = blocking(&id, {
                let p = p.clone();
                move || match p.call(&request, timeout)? {
                    Reply::FilteredResponse(v) => Ok(v.into_inner()),
                    other => Err(p.unexpected(p.generation(), "FilterResponse", &other)),
                }
            })
            .await;
            match result {
                Ok(next) => body = next,
                Err(e) => {
                    if let Some(r) = self.filter_failed(e) {
                        return Err(r);
                    }
                }
            }
        }
        Ok(body)
    }

    /// Calls one of `plugin`'s endpoints (the server routes `/plugins/<id>/<path>` here).
    pub async fn call_endpoint(&self, plugin: &str, request: EndpointRequest) -> Result<EndpointResponse, PluginError> {
        let p = self.of_kind(plugin, Kind::Endpoint)?;
        let timeout = self.config.timeouts.endpoint;
        blocking(plugin, move || match p.call(&Request::Endpoint(request), timeout)? {
            Reply::Endpoint(r) => Ok(r),
            other => Err(p.unexpected(p.generation(), "Endpoint", &other)),
        })
        .await
    }

    /// Invokes one of `plugin`'s tools with `args` (a JSON object); returns the tool's JSON result.
    pub async fn invoke_tool(&self, plugin: &str, tool: &str, args: Value) -> Result<Value, PluginError> {
        let p = self.of_kind(plugin, Kind::Tool)?;
        let timeout = self.config.timeouts.tool;
        let request = Request::InvokeTool {
            tool: tool.to_string(),
            args: Json(args),
        };
        blocking(plugin, move || match p.call(&request, timeout)? {
            Reply::Tool(v) => Ok(v.into_inner()),
            other => Err(p.unexpected(p.generation(), "InvokeTool", &other)),
        })
        .await
    }

    /// Starts `plugin` if needed and returns its status (with its tools and endpoints).
    pub async fn start(&self, plugin: &str) -> Result<PluginStatus, PluginError> {
        let p = self.get(plugin)?;
        let timeout = self.config.timeouts.start;
        blocking(plugin, move || match p.call(&Request::Describe, timeout)? {
            Reply::Described(_) => Ok(p.status()),
            other => Err(p.unexpected(p.generation(), "Describe", &other)),
        })
        .await
    }

    /// Engine: loads `model` into `plugin`.
    pub async fn engine_load(&self, plugin: &str, model: &str, options: Value) -> Result<EngineModel, PluginError> {
        let p = self.of_kind(plugin, Kind::Engine)?;
        let timeout = self.config.timeouts.engine_load;
        let request = Request::EngineLoad {
            model: model.to_string(),
            options: Json(options),
        };
        blocking(plugin, move || p.engine_load(&request, timeout)).await
    }

    /// Engine: unloads a model.
    pub async fn engine_unload(&self, model: &EngineModel) -> Result<(), PluginError> {
        let p = self.of_kind(&model.plugin, Kind::Engine)?;
        let timeout = self.config.timeouts.engine_unload;
        let m = model.clone();
        blocking(&model.plugin, move || p.engine_unload(&m, timeout)).await
    }

    /// Engine: generates for `request` (OpenAI-compatible JSON) with a loaded model; text arrives on the stream.
    pub fn engine_generate(&self, model: &EngineModel, request: Value) -> Result<GenerateStream, PluginError> {
        let p = self.of_kind(&model.plugin, Kind::Engine)?;
        let (tx, rx) = mpsc::channel(32);
        let m = model.clone();
        std::thread::Builder::new()
            .name("pitwall-plugin-generate".into())
            .spawn(move || p.engine_generate(&m, request, tx))
            .map_err(|e| PluginError::new(&model.plugin, ErrorKind::Spawn, format!("cannot start the generation: {e}")))?;
        Ok(GenerateStream { rx })
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        self.stop_all();
    }
}
