//! Pitwall's approval policy (ADR 0023 Decision 8, the user's rule): a connected app's read-only tool asks the first time and
//! Always allow is remembered with the tool's fingerprint; an action asks every call (D2). The book is
//! `pitwall_core::approvals`; this keeps it in a file in the app's data folder (written atomically, the last good copy kept
//! as a backup; a damaged file falls back to the backup, then to an empty book, so nothing is ever allowed by accident).

use super::tool::ToolNames;
use paddock_assistant::{ApprovalChoice, ApprovalPolicy, CallInfo, Verdict};
use pitwall_core::approvals::{self, ApprovalBook, BookError, ToolKind};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// The file of the book, in the app's data folder.
pub const FILE: &str = "tool-approvals.json";

/// The policy of the Playground chat.
pub struct PitwallPolicy {
    book: Mutex<ApprovalBook>,
    path: Option<PathBuf>,
    names: ToolNames,
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
}

fn backup(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

/// Reads the book: the file, else its backup, else an empty book. A newer file is kept as it is and the book is empty.
pub fn load(path: &Path) -> (ApprovalBook, Option<BookError>) {
    let read = |p: &Path| std::fs::read_to_string(p).ok().map(|t| ApprovalBook::from_json(&t));
    match read(path) {
        None => (ApprovalBook::new(), None),
        Some(Ok(book)) => (book, None),
        Some(Err(e)) => match read(&backup(path)) {
            Some(Ok(book)) if !matches!(e, BookError::Newer(_)) => (book, Some(e)),
            _ => (ApprovalBook::new(), Some(e)),
        },
    }
}

/// Writes the book atomically and keeps the previous good copy as the backup.
pub fn save(path: &Path, book: &ApprovalBook) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        std::fs::copy(path, backup(path))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, book.to_json())?;
    std::fs::rename(&tmp, path)
}

impl PitwallPolicy {
    /// The policy over the book at `dir/`[`FILE`] (`None`: kept for this session only), naming tools through `names`.
    pub fn new(dir: Option<&Path>, names: ToolNames, clock: impl Fn() -> u64 + Send + Sync + 'static) -> PitwallPolicy {
        let path = dir.map(|d| d.join(FILE));
        let book = match &path {
            Some(p) => {
                let (book, problem) = load(p);
                if let Some(e) = problem {
                    log::warn!(target: "pitwall_chat", "tool approvals: {e}; next step: approvals are asked again");
                }
                book
            }
            None => ApprovalBook::new(),
        };
        PitwallPolicy {
            book: Mutex::new(book),
            path,
            names,
            clock: Box::new(clock),
        }
    }

    /// Forgets one tool's approval: it is asked about again on its next call.
    pub fn revoke(&self, server: &str, tool: &str) {
        let mut book = self.book.lock().unwrap_or_else(PoisonError::into_inner);
        if book.revoke(server, tool) {
            if let Some(path) = &self.path {
                if let Err(e) = save(path, &book) {
                    log::warn!(target: "pitwall_chat", "tool approvals could not be saved: {e}; next step: check the data folder's permissions");
                }
            }
        }
    }

    /// Forgets every approval of a server (the user forgot the app).
    pub fn revoke_server(&self, server: &str) {
        let mut book = self.book.lock().unwrap_or_else(PoisonError::into_inner);
        if book.revoke_server(server) > 0 {
            if let Some(path) = &self.path {
                if let Err(e) = save(path, &book) {
                    log::warn!(target: "pitwall_chat", "tool approvals could not be saved: {e}; next step: check the data folder's permissions");
                }
            }
        }
    }

    /// The remembered approvals (Settings > Connected apps).
    pub fn remembered(&self) -> Vec<approvals::Remembered> {
        self.book.lock().unwrap_or_else(PoisonError::into_inner).entries()
    }

    fn key(&self, call: &CallInfo<'_>) -> (String, String, String, ToolKind) {
        let (server, tool) = self
            .names
            .of(&call.tool.name)
            .unwrap_or_else(|| (String::new(), call.tool.name.clone()));
        let fp = approvals::fingerprint(&call.tool.name, &call.tool.description, &call.tool.input_schema);
        let kind = if call.access.writes() {
            ToolKind::Action
        } else {
            ToolKind::ReadOnly
        };
        (server, tool, fp, kind)
    }
}

impl ApprovalPolicy for PitwallPolicy {
    fn check(&self, call: &CallInfo<'_>) -> Verdict {
        let (server, tool, fp, kind) = self.key(call);
        // A tool Pitwall did not register (no server) is never remembered: it asks like an action.
        if server.is_empty() {
            return Verdict::AskEveryTime;
        }
        match self
            .book
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .check(&server, &tool, &fp, kind)
        {
            approvals::Verdict::Remembered => Verdict::Remembered,
            approvals::Verdict::AskOrRemember => Verdict::AskOrRemember,
            approvals::Verdict::AskEveryTime => Verdict::AskEveryTime,
        }
    }

    fn record(&self, call: &CallInfo<'_>, choice: ApprovalChoice) {
        if choice != ApprovalChoice::AlwaysAllow {
            return;
        }
        let (server, tool, fp, _) = self.key(call);
        let mut book = self.book.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(e) = book.remember(&server, &tool, &fp, (self.clock)()) {
            log::warn!(target: "pitwall_chat", "tool approval not remembered: {e}");
            return;
        }
        if let Some(path) = &self.path {
            if let Err(e) = save(path, &book) {
                log::warn!(target: "pitwall_chat", "tool approvals could not be saved: {e}; next step: check the data folder's permissions");
            }
        }
    }
}
