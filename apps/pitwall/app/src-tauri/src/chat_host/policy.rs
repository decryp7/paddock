//! Pitwall's approval policy (ADR 0023 Decision 8, the user's rule): a connected app's read-only tool asks the first time and
//! Always allow is remembered with the tool's fingerprint; an action asks every call (D2). The book is
//! `pitwall_core::approvals`; this keeps it in a file in the app's data folder (written atomically, the last good copy kept
//! as a backup; a damaged file falls back to the backup, then to an empty book, so nothing is ever allowed by accident).

use super::tool::ToolNames;
use paddock_assistant::{ApprovalChoice, ApprovalPolicy, CallInfo, Verdict};
use paddock_env::{Problem as FileProblem, VersionedFile};
use pitwall_core::approvals::{self, ApprovalBook, Remembered, ToolKind};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Mutex, PoisonError};

/// The file of the book, in the app's data folder.
pub const FILE: &str = "tool-approvals.json";

/// The policy of the Playground chat.
pub struct PitwallPolicy {
    book: Mutex<ApprovalBook>,
    file: Option<BookFile>,
    names: ToolNames,
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
}

/// The book as `paddock-env`'s `VersionedFile` keeps it: the entries, with the file's `version` added around them.
#[derive(Default, Serialize, Deserialize)]
struct BookDoc {
    #[serde(default)]
    entries: Vec<Remembered>,
}

/// The file of the book: owner-only, written atomically with the last good copy kept as `<file>.bak`, a damaged file moved aside
/// (and the backup used), a newer file left as it is and never overwritten, all by `VersionedFile` (rule 55, as `saved.rs`).
pub struct BookFile(VersionedFile<BookDoc>);

impl BookFile {
    /// The book file at `path`.
    pub fn new(path: &Path) -> BookFile {
        BookFile(
            VersionedFile::new(path, approvals::FORMAT_VERSION)
                .max_bytes(approvals::MAX_FILE_BYTES as u64)
                .private(),
        )
    }

    /// Reads the book: the file, else its backup, else an empty book. A newer file is kept as it is and the book is empty.
    pub fn load(&self) -> (ApprovalBook, Option<FileProblem>) {
        let loaded = self.0.load();
        (ApprovalBook::from_entries(loaded.value.entries), loaded.problem)
    }

    /// Writes the book atomically and keeps the previous good copy as the backup.
    pub fn save(&self, book: &ApprovalBook) -> paddock_env::Result<()> {
        self.0.save(&BookDoc { entries: book.entries() })
    }
}

impl PitwallPolicy {
    /// The policy over the book at `dir/`[`FILE`] (`None`: kept for this session only), naming tools through `names`.
    pub fn new(dir: Option<&Path>, names: ToolNames, clock: impl Fn() -> u64 + Send + Sync + 'static) -> PitwallPolicy {
        let file = dir.map(|d| BookFile::new(&d.join(FILE)));
        let book = match &file {
            Some(f) => {
                let (book, problem) = f.load();
                if let Some(e) = problem {
                    log::warn!(target: "pitwall_chat", "tool approvals: {e}; next step: approvals are asked again");
                }
                book
            }
            None => ApprovalBook::new(),
        };
        PitwallPolicy {
            book: Mutex::new(book),
            file,
            names,
            clock: Box::new(clock),
        }
    }

    /// Forgets one tool's approval: it is asked about again on its next call.
    pub fn revoke(&self, server: &str, tool: &str) {
        let mut book = self.book.lock().unwrap_or_else(PoisonError::into_inner);
        if book.revoke(server, tool) {
            if let Some(file) = &self.file {
                if let Err(e) = file.save(&book) {
                    log::warn!(target: "pitwall_chat", "tool approvals could not be saved: {e}; next step: check the data folder's permissions");
                }
            }
        }
    }

    /// Forgets every approval of a server (the user forgot the app).
    pub fn revoke_server(&self, server: &str) {
        let mut book = self.book.lock().unwrap_or_else(PoisonError::into_inner);
        if book.revoke_server(server) > 0 {
            if let Some(file) = &self.file {
                if let Err(e) = file.save(&book) {
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
        // What the server said, whole and as it said it (title included): the model's copy of the description is cleaned and
        // cut, so a change past the cut, or to the title alone, would not ask again.
        let (title, description) = self
            .names
            .raw_text_of(&call.tool.name)
            .unwrap_or_else(|| (None, call.tool.description.clone()));
        let fp = approvals::fingerprint(&call.tool.name, title.as_deref(), &description, &call.tool.input_schema);
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
        if let Some(file) = &self.file {
            if let Err(e) = file.save(&book) {
                log::warn!(target: "pitwall_chat", "tool approvals could not be saved: {e}; next step: check the data folder's permissions");
            }
        }
    }
}
