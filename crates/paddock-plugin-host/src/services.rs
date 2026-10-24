//! The host side of [`HostCall`]: what the app does when a plugin calls back while it works. Every call is checked: a file
//! is read only if it was registered for this plugin, output goes only into the folder the app chose, an elevated operation
//! runs only if the user allowed that name, a report is bounded.

use crate::files::{FileError, HostFiles};
use crate::grants::Grants;
use crate::output::OutputSink;
use paddock_ipc::{CallCtx, Handler, Json, RemoteError, codes, decode_request, encode_reply};
use paddock_plugin_api::protocol::{DENIED, Fail, HostCall, MAX_LOG_BATCH, MAX_LOG_LINE, cut_line};
use paddock_plugin_api::tools::{FailureReason, ToolTask};
use paddock_plugin_package::Manifest;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// The longest an elevated operation may be waited for.
pub const MAX_ELEVATED: Duration = Duration::from_secs(3600);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Receives a plugin's background work (the tray, the status bar, a jobs pane).
pub trait TaskObserver: Send + Sync {
    /// A task started.
    fn started(&self, plugin: &str, tool: &str, task: &ToolTask);
    /// Progress (`None`: unknown) and what it is doing.
    fn report(&self, plugin: &str, task: &str, progress: Option<f32>, note: &str);
    /// A task ended.
    fn finished(&self, plugin: &str, task: &str);
}

/// A [`TaskObserver`] that ignores everything.
pub struct NoTasks;

impl TaskObserver for NoTasks {
    fn started(&self, _: &str, _: &str, _: &ToolTask) {}
    fn report(&self, _: &str, _: &str, _: Option<f32>, _: &str) {}
    fn finished(&self, _: &str, _: &str) {}
}

/// Runs a named elevated operation (the app owns elevation: the OS prompt and the elevated process).
pub trait ElevatedRunner: Send + Sync {
    /// Runs operation `op` of `tool` of `plugin` elevated and answers its result.
    fn run(&self, plugin: &str, tool: &str, op: &str, args: Value, timeout: Duration) -> Result<Value, Fail>;
}

/// A task a plugin announced and has not ended.
#[derive(Debug, Clone, PartialEq)]
pub struct RunningTask {
    /// The tool that started it.
    pub tool: String,
    /// The task.
    pub task: ToolTask,
    /// Its last progress (0 to 1).
    pub progress: Option<f32>,
    /// What it last said it was doing.
    pub note: String,
}

/// Serves one plugin's [`HostCall`]s.
pub struct HostServices {
    plugin: String,
    manifest: Manifest,
    grants: Mutex<Grants>,
    files: HostFiles,
    outputs: OutputSink,
    observer: Arc<dyn TaskObserver>,
    elevated: Option<Arc<dyn ElevatedRunner>>,
    running: Mutex<BTreeMap<String, RunningTask>>,
}

impl HostServices {
    /// Services for the plugin of `manifest` under `grants`.
    pub fn new(
        manifest: Manifest,
        grants: Grants,
        files: HostFiles,
        outputs: OutputSink,
        observer: Arc<dyn TaskObserver>,
        elevated: Option<Arc<dyn ElevatedRunner>>,
    ) -> HostServices {
        HostServices {
            plugin: manifest.id.to_string(),
            manifest,
            grants: Mutex::new(grants),
            files,
            outputs,
            observer,
            elevated,
            running: Mutex::default(),
        }
    }

    /// Replaces the grants in force (the user changed them).
    pub fn set_grants(&self, grants: Grants) {
        *lock(&self.grants) = grants;
    }

    /// The tasks the plugin announced and has not ended.
    pub fn tasks(&self) -> Vec<RunningTask> {
        lock(&self.running).values().cloned().collect()
    }

    /// Forgets the plugin's tasks, open files and unfinished outputs (its process ended).
    pub fn process_ended(&self) {
        let ended: Vec<String> = std::mem::take(&mut *lock(&self.running)).into_keys().collect();
        for task in ended {
            self.observer.finished(&self.plugin, &task);
        }
        self.files.close_all();
        self.outputs.discard_open();
    }

    /// Answers one call.
    pub fn serve(&self, call: HostCall) -> Result<Vec<u8>, RemoteError> {
        match call {
            HostCall::ReadFile { id, offset, len } => match self.files.read(id, offset, len) {
                Ok(bytes) => encode_reply(&bytes),
                Err(FileError::Denied(m)) => Err(RemoteError::new(DENIED, m)),
                Err(FileError::Io(m)) => Err(RemoteError::new(codes::INTERNAL, m)),
            },
            HostCall::Report { task, progress, note } => {
                let progress = progress.filter(|p| p.is_finite()).map(|p| p.clamp(0.0, 1.0));
                if let Some(t) = lock(&self.running).get_mut(&task) {
                    t.progress = progress;
                    t.note = cut_line(&note, MAX_LOG_LINE).to_string();
                }
                self.observer.report(&self.plugin, &task, progress, cut_line(&note, MAX_LOG_LINE));
                encode_reply(&())
            }
            HostCall::TaskStarted { tool, task } => {
                lock(&self.running).insert(
                    task.id.clone(),
                    RunningTask {
                        tool: tool.clone(),
                        task: task.clone(),
                        progress: None,
                        note: String::new(),
                    },
                );
                self.observer.started(&self.plugin, &tool, &task);
                encode_reply(&())
            }
            HostCall::TaskFinished { task } => {
                lock(&self.running).remove(&task);
                self.observer.finished(&self.plugin, &task);
                encode_reply(&())
            }
            HostCall::Output { name, bytes, last } => {
                let saved = self.outputs.piece(&name, &bytes, last).map_err(|m| RemoteError::new(codes::INVALID, m))?;
                encode_reply(&saved)
            }
            HostCall::Elevated { tool, op, args, timeout_ms } => {
                let reply: Result<Json, Fail> = self.elevated(&tool, &op, args.into_inner(), timeout_ms).map(Json);
                encode_reply(&reply)
            }
            HostCall::Log { lines, dropped } => {
                for line in lines.iter().take(MAX_LOG_BATCH) {
                    log::info!(target: "paddock_plugin", "{}: {}", self.plugin, cut_line(line, MAX_LOG_LINE));
                }
                if dropped > 0 || lines.len() > MAX_LOG_BATCH {
                    log::info!(target: "paddock_plugin", "{}: some log lines were dropped", self.plugin);
                }
                encode_reply(&())
            }
        }
    }

    fn elevated(&self, tool: &str, op: &str, args: Value, timeout_ms: u64) -> Result<Value, Fail> {
        let denied = |message: String| Fail {
            reason: Some(FailureReason::Denied),
            message,
        };
        if !lock(&self.grants).may_elevate(&self.manifest, op) {
            return Err(denied(format!("plugin {} may not run \"{op}\" with administrator rights; allow it first", self.plugin)));
        }
        let Some(runner) = &self.elevated else {
            return Err(Fail {
                reason: Some(FailureReason::Missing),
                message: "this app cannot run operations with administrator rights".into(),
            });
        };
        runner.run(&self.plugin, tool, op, args, Duration::from_millis(timeout_ms).min(MAX_ELEVATED))
    }
}

impl Handler for HostServices {
    fn call(&self, _ctx: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError> {
        self.serve(decode_request(body)?)
    }
}
