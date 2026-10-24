//! Test doubles shared by this crate's unit tests.

use crate::protocol::SavedOutput;
use crate::tools::{Host, ToolError, ToolTask};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A [`Host`] over bytes in memory that records every call.
#[derive(Default)]
pub(crate) struct Memory {
    pub(crate) content: Vec<u8>,
    /// Answers reads with two bytes more than asked for.
    pub(crate) overlong: bool,
    /// Never says where an output was saved.
    pub(crate) silent: bool,
    pub(crate) log: Mutex<Vec<String>>,
}

impl Memory {
    pub(crate) fn of(content: &[u8]) -> Arc<Memory> {
        Arc::new(Memory {
            content: content.to_vec(),
            ..Memory::default()
        })
    }

    pub(crate) fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn note(&self, line: String) {
        self.log.lock().unwrap().push(line);
    }
}

impl Host for Memory {
    fn read_file(&self, id: u64, offset: u64, len: u32) -> Result<Vec<u8>, ToolError> {
        self.note(format!("read {id} {offset} {len}"));
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.content.len());
        let end = start.saturating_add(len as usize).min(self.content.len());
        let mut out = self.content[start..end].to_vec();
        if self.overlong {
            out.extend_from_slice(&[0, 0]);
        }
        Ok(out)
    }

    fn report(&self, task: &str, progress: Option<f32>, note: &str) {
        self.note(format!("report {task} {progress:?} {note}"));
    }

    fn task_started(&self, tool: &str, task: &ToolTask) {
        self.note(format!("started {tool} {}", task.id));
    }

    fn task_finished(&self, task: &str) {
        self.note(format!("finished {task}"));
    }

    fn output(
        &self,
        name: &str,
        bytes: &[u8],
        last: bool,
    ) -> Result<Option<SavedOutput>, ToolError> {
        self.note(format!("output {name} {} {last}", bytes.len()));
        Ok((last && !self.silent).then(|| SavedOutput {
            name: name.into(),
            bytes: 0,
        }))
    }

    fn log(&self, lines: &[String]) {
        self.note(format!("log {}", lines.join("|")));
    }

    fn elevated(&self, tool: &str, op: &str, args: Value, _: Duration) -> Result<Value, ToolError> {
        self.note(format!("elevated {tool} {op}"));
        Ok(args)
    }
}
