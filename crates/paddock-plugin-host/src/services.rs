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
    fn run(
        &self,
        plugin: &str,
        tool: &str,
        op: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, Fail>;
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
        let ended: Vec<String> = std::mem::take(&mut *lock(&self.running))
            .into_keys()
            .collect();
        for task in ended {
            self.observer.finished(&self.plugin, &task);
        }
        self.files.close_all();
        self.outputs.discard_open();
    }

    /// Answers one call.
    pub fn serve(&self, call: HostCall) -> Result<Vec<u8>, RemoteError> {
        match call {
            HostCall::ReadFile { id, offset, len } => {
                let bytes = self.files.read(id, offset, len).map_err(file_refusal)?;
                encode_reply(&bytes)
            }
            HostCall::Report {
                task,
                progress,
                note,
            } => {
                let progress = progress
                    .filter(|p| p.is_finite())
                    .map(|p| p.clamp(0.0, 1.0));
                if let Some(t) = lock(&self.running).get_mut(&task) {
                    t.progress = progress;
                    t.note = cut_line(&note, MAX_LOG_LINE).to_string();
                }
                self.observer
                    .report(&self.plugin, &task, progress, cut_line(&note, MAX_LOG_LINE));
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
                let saved = self
                    .outputs
                    .piece(&name, &bytes, last)
                    .map_err(|m| RemoteError::new(codes::INVALID, m))?;
                encode_reply(&saved)
            }
            HostCall::Elevated {
                tool,
                op,
                args,
                timeout_ms,
            } => {
                let reply: Result<Json, Fail> = self
                    .elevated(&tool, &op, args.into_inner(), timeout_ms)
                    .map(Json);
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
            return Err(denied(format!(
                "plugin {} may not run \"{op}\" with administrator rights; allow it first",
                self.plugin
            )));
        }
        let Some(runner) = &self.elevated else {
            return Err(Fail {
                reason: Some(FailureReason::Missing),
                message: "this app cannot run operations with administrator rights".into(),
            });
        };
        runner.run(
            &self.plugin,
            tool,
            op,
            args,
            Duration::from_millis(timeout_ms).min(MAX_ELEVATED),
        )
    }
}

/// A failed read as the refusal the plugin sees: not allowed, or the file could not be read.
fn file_refusal(e: FileError) -> RemoteError {
    let code = if matches!(e, FileError::Denied(_)) {
        DENIED
    } else {
        codes::INTERNAL
    };
    RemoteError::new(code, e.to_string())
}

impl Handler for HostServices {
    fn call(&self, _ctx: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError> {
        self.serve(decode_request(body)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_ipc::wire::decode;

    #[derive(Default)]
    struct Seen(Mutex<Vec<String>>);
    impl TaskObserver for Seen {
        fn started(&self, plugin: &str, tool: &str, task: &ToolTask) {
            lock(&self.0).push(format!("started {plugin} {tool} {}", task.id));
        }
        fn report(&self, plugin: &str, task: &str, progress: Option<f32>, note: &str) {
            lock(&self.0).push(format!("report {plugin} {task} {progress:?} {note}"));
        }
        fn finished(&self, plugin: &str, task: &str) {
            lock(&self.0).push(format!("finished {plugin} {task}"));
        }
    }

    struct Runner(Mutex<Vec<Duration>>);
    impl ElevatedRunner for Runner {
        fn run(
            &self,
            plugin: &str,
            tool: &str,
            op: &str,
            args: Value,
            timeout: Duration,
        ) -> Result<Value, Fail> {
            lock(&self.0).push(timeout);
            if op == "boom" {
                Err(Fail::new("boom"))
            } else {
                Ok(serde_json::json!({"plugin": plugin, "tool": tool, "args": args}))
            }
        }
    }

    const MANIFEST: &str = "format = 1\n[plugin]\nid = \"net.example.p\"\nname = \"P\"\nversion = \"1.0.0\"\npublisher = \"E\"\nprotocol = \"2.0\"\nbinary = \"p\"\ntargets = [\"x86_64-unknown-linux-gnu\"]\n[[permissions]]\npermission = \"elevated\"\nops = [\"collect\", \"boom\"]\nreason = \"r\"\n";

    fn services(
        grants: Grants,
        runner: Option<Arc<dyn ElevatedRunner>>,
    ) -> (HostServices, Arc<Seen>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let seen = Arc::new(Seen::default());
        let s = HostServices::new(
            Manifest::parse(MANIFEST).unwrap(),
            grants,
            HostFiles::new(),
            OutputSink::new(dir.path().join("out")),
            seen.clone(),
            runner,
        );
        (s, seen, dir)
    }

    fn all_granted() -> Grants {
        Grants::none().allow(
            Manifest::parse(MANIFEST).unwrap().permissions[0]
                .permission
                .clone(),
        )
    }

    fn task(id: &str) -> ToolTask {
        ToolTask {
            id: id.into(),
            label: id.into(),
            kind: "k".into(),
            stoppable: true,
        }
    }

    #[test]
    fn tasks_are_listed_reported_with_bounded_progress_and_ended() {
        let (s, seen, _dir) = services(Grants::none(), None);
        s.serve(HostCall::TaskStarted {
            tool: "t".into(),
            task: task("a"),
        })
        .unwrap();
        s.serve(HostCall::Report {
            task: "a".into(),
            progress: Some(7.0),
            note: "x".repeat(10_000),
        })
        .unwrap();
        s.serve(HostCall::Report {
            task: "a".into(),
            progress: Some(f32::NAN),
            note: "n".into(),
        })
        .unwrap();
        let t = &s.tasks()[0];
        assert_eq!(
            (
                t.tool.as_str(),
                t.task.id.as_str(),
                t.progress,
                t.note.as_str()
            ),
            ("t", "a", None, "n")
        );
        s.serve(HostCall::Report {
            task: "a".into(),
            progress: Some(-1.0),
            note: "n".into(),
        })
        .unwrap();
        assert_eq!(s.tasks()[0].progress, Some(0.0));
        // A report for a task nobody announced reaches the observer and changes nothing.
        s.serve(HostCall::Report {
            task: "ghost".into(),
            progress: None,
            note: String::new(),
        })
        .unwrap();
        s.serve(HostCall::TaskFinished { task: "a".into() })
            .unwrap();
        assert!(s.tasks().is_empty());
        let log = lock(&seen.0).clone();
        assert_eq!(log[0], "started net.example.p t a");
        assert!(log[1].starts_with("report net.example.p a Some(1.0) xxx") && log[1].len() < 4200);
        assert_eq!(log[2], "report net.example.p a None n");
        assert_eq!(log.last().unwrap(), "finished net.example.p a");
    }

    #[test]
    fn the_end_of_the_process_ends_its_tasks_files_and_unfinished_outputs() {
        let (s, seen, dir) = services(Grants::none(), None);
        s.serve(HostCall::TaskStarted {
            tool: "t".into(),
            task: task("a"),
        })
        .unwrap();
        s.serve(HostCall::TaskStarted {
            tool: "t".into(),
            task: task("b"),
        })
        .unwrap();
        let f = dir.path().join("f");
        std::fs::write(&f, "x").unwrap();
        let file = s.files.open_chosen(&f).unwrap();
        s.serve(HostCall::Output {
            name: "o.bin".into(),
            bytes: vec![1],
            last: false,
        })
        .unwrap();
        s.process_ended();
        assert!(s.tasks().is_empty());
        assert!(
            matches!(s.serve(HostCall::ReadFile { id: file.id, offset: 0, len: 1 }), Err(e) if e.code == DENIED)
        );
        assert_eq!(s.outputs.open_count(), 0);
        let log = lock(&seen.0).clone();
        assert!(
            log.contains(&"finished net.example.p a".to_string())
                && log.contains(&"finished net.example.p b".to_string())
        );
    }

    #[test]
    fn reads_outputs_and_logs_are_served_and_checked() {
        let (s, _seen, dir) = services(Grants::none(), None);
        let f = dir.path().join("f");
        std::fs::write(&f, "hello").unwrap();
        let file = s.files.open_chosen(&f).unwrap();
        let bytes: Vec<u8> = decode(
            &s.serve(HostCall::ReadFile {
                id: file.id,
                offset: 1,
                len: 3,
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(bytes, b"ell");
        let denied = s
            .serve(HostCall::ReadFile {
                id: 99,
                offset: 0,
                len: 1,
            })
            .unwrap_err();
        assert_eq!(denied.code, DENIED);
        assert_eq!(
            file_refusal(FileError::Io("disk".into())).code,
            codes::INTERNAL
        );
        let saved: Option<paddock_plugin_api::protocol::SavedOutput> = decode(
            &s.serve(HostCall::Output {
                name: "o.bin".into(),
                bytes: vec![1, 2],
                last: true,
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(saved.unwrap().bytes, 2);
        assert_eq!(
            s.serve(HostCall::Output {
                name: "../x".into(),
                bytes: vec![],
                last: true
            })
            .unwrap_err()
            .code,
            codes::INVALID
        );
        let lines: Vec<String> = (0..MAX_LOG_BATCH + 3)
            .map(|i| format!("line {i}"))
            .collect();
        s.serve(HostCall::Log { lines, dropped: 4 }).unwrap();
        s.serve(HostCall::Log {
            lines: vec!["é".repeat(MAX_LOG_LINE)],
            dropped: 0,
        })
        .unwrap();
    }

    #[test]
    fn elevated_operations_run_only_by_the_name_the_user_allowed() {
        let runner = Arc::new(Runner(Mutex::default()));
        let (s, _seen, _dir) = services(all_granted(), Some(runner.clone()));
        let call = |op: &str, timeout_ms: u64| {
            let reply = s
                .serve(HostCall::Elevated {
                    tool: "t".into(),
                    op: op.into(),
                    args: Json(serde_json::json!({"k": 1})),
                    timeout_ms,
                })
                .unwrap();
            decode::<Result<Json, Fail>>(&reply).unwrap()
        };
        assert_eq!(
            call("collect", 5000).unwrap().into_inner(),
            serde_json::json!({"plugin": "net.example.p", "tool": "t", "args": {"k": 1}})
        );
        assert_eq!(call("boom", 1).unwrap_err().message, "boom");
        let denied = call("other", 1).unwrap_err();
        assert_eq!(denied.reason, Some(FailureReason::Denied));
        assert!(
            denied
                .message
                .contains("may not run \"other\" with administrator rights")
        );
        // The wait is capped.
        let _ = call("collect", u64::MAX);
        assert_eq!(
            lock(&runner.0).clone(),
            [
                Duration::from_secs(5),
                Duration::from_millis(1),
                MAX_ELEVATED
            ]
        );
        // Grants change at run time.
        s.set_grants(Grants::none());
        assert_eq!(
            call("collect", 1).unwrap_err().reason,
            Some(FailureReason::Denied)
        );
        // Allowed but the app cannot elevate.
        let (bare, _seen, _dir) = services(all_granted(), None);
        let reply = bare
            .serve(HostCall::Elevated {
                tool: "t".into(),
                op: "collect".into(),
                args: Json(serde_json::json!({})),
                timeout_ms: 1,
            })
            .unwrap();
        let missing = decode::<Result<Json, Fail>>(&reply).unwrap().unwrap_err();
        assert_eq!(missing.reason, Some(FailureReason::Missing));
    }

    #[test]
    fn the_task_observer_that_ignores_everything_is_harmless() {
        let n = NoTasks;
        n.started("p", "t", &task("a"));
        n.report("p", "a", None, "");
        n.finished("p", "a");
    }
}
