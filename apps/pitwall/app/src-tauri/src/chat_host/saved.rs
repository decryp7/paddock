//! The chat's saved conversations (ADR 0023 Decision 3, WP-3b): one versioned file per conversation in the app's data folder
//! (`chats/<id>.json`, owner-only, written atomically with a backup by `paddock-env`'s `VersionedFile`), and attachments
//! beside them by SHA-256 (`chats/attachments/<sha256>`). One file per conversation: a save costs one conversation, not the
//! whole history.
//!
//! - **Damage** is tolerated: a damaged file falls back to its backup, or is kept aside (`<id>.json.corrupt-<time>`) and left
//!   out of the list, and the user is told once ([`Conversations::take_problem`]).
//! - **Newer** files (a later Pitwall wrote them) are left as they are and not listed; nothing migrates them.
//! - **Limits** (Decision 3): [`MAX_SAVED`] conversations, [`MAX_CHAT_BYTES`] per file, [`MAX_ATTACHMENT_BYTES`] of
//!   attachments. At a limit the new item is refused with words that say what to delete; the conversation is kept for this
//!   session and nothing is ever deleted by the store itself.
//! - Not in the server, not in any log (only counts and ids are logged), left out of Report a problem (`report.rs` reads
//!   named files only; a test plants a marker here).
//!
//! The folder is read on first use, not at start. Bodies are cached up to [`CACHE_BYTES`]; the rest is read from its file.

use paddock_assistant::{
    conversation_title, history_of, Attachment, ChatMessage, Conversation, ConversationSummary, Conversations, SaveError, StoredMessage,
};
use paddock_env::{Problem as FileProblem, VersionedFile};
use paddock_health::Clock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::UNIX_EPOCH;

/// The chats' folder in the data folder.
pub const CHATS_DIR: &str = "chats";
/// The attachments' folder in the chats' folder.
pub const ATTACHMENTS_DIR: &str = "attachments";
/// A conversation file's format.
pub const FORMAT_VERSION: u32 = 1;
/// Most saved conversations (Decision 3).
pub const MAX_SAVED: usize = 1000;
/// Largest conversation file (Decision 3).
pub const MAX_CHAT_BYTES: u64 = 16 << 20;
/// Most bytes of saved attachments (Decision 3).
pub const MAX_ATTACHMENT_BYTES: u64 = 2 << 30;
/// Most bytes of conversation bodies kept in memory besides those that could not be saved.
pub const CACHE_BYTES: u64 = 64 << 20;
/// Most files of the chats' folder looked at when it is read (a folder of other things is not walked without end).
const MAX_SCANNED: usize = 4 * MAX_SAVED;
/// The size `VersionedFile` adds around a value (the version field, the closing newline).
const FILE_OVERHEAD: u64 = 64;

/// The bounds of a [`SavedChats`]; the defaults are Decision 3's. Tests use small ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedLimits {
    /// Most saved conversations.
    pub conversations: usize,
    /// Largest conversation file, in bytes.
    pub file_bytes: u64,
    /// Most bytes of attachments.
    pub attachment_bytes: u64,
}

impl Default for SavedLimits {
    fn default() -> Self {
        SavedLimits {
            conversations: MAX_SAVED,
            file_bytes: MAX_CHAT_BYTES,
            attachment_bytes: MAX_ATTACHMENT_BYTES,
        }
    }
}

/// What the saved chats take on disk (Settings > Chats).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Saved conversations.
    pub chats: usize,
    /// Their files, in bytes.
    pub bytes: u64,
    /// The attachments, in bytes.
    pub attachment_bytes: u64,
}

/// A conversation file: the conversation, or nothing (a file's defaults).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ChatFile {
    #[serde(default)]
    conversation: Option<Conversation>,
}

struct Entry {
    summary: ConversationSummary,
    /// The attachments it refers to (an attachment file goes when no saved conversation refers to it any more).
    refs: BTreeSet<String>,
    /// Its file's size, or what it would be.
    bytes: u64,
    /// Cached, or not written: then this is the only copy, kept for the session.
    body: Option<Conversation>,
    /// The file does not hold all of it (a limit refused the last save, or the disk did).
    unsaved: bool,
    /// It has a file (it counts against [`MAX_SAVED`]).
    has_file: bool,
    /// The last use, for the cache.
    used: u64,
}

#[derive(Default)]
struct State {
    loaded: bool,
    entries: BTreeMap<String, Entry>,
    problem: Option<String>,
    counter: u64,
    tick: u64,
    attachment_bytes: u64,
}

/// The chat's saved conversations.
pub struct SavedChats {
    dir: PathBuf,
    clock: Arc<dyn Clock>,
    limits: SavedLimits,
    state: Mutex<State>,
}

/// Whether `id` can name a file: 1 to 64 ASCII letters, digits and dashes (ids come from the store, but also from the UI).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Whether `sha` is a SHA-256 in lowercase hex.
fn valid_sha(sha: &str) -> bool {
    sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn encoded_len(file: &ChatFile) -> u64 {
    serde_json::to_vec_pretty(file).map_or(u64::MAX, |v| v.len() as u64 + FILE_OVERHEAD)
}

fn summary_of(c: &Conversation) -> ConversationSummary {
    ConversationSummary {
        id: c.id.clone(),
        title: c.title.clone(),
        updated_ms: c.updated_ms,
        message_count: c.messages.len(),
    }
}

fn refs_of(c: &Conversation) -> BTreeSet<String> {
    c.messages
        .iter()
        .flat_map(|m| m.attachments.iter().map(|a| Attachment::sha256(a).to_string()))
        .collect()
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl SavedChats {
    /// The saved chats in `dir` (`<data>/chats`). Nothing is read or written until first use.
    pub fn open(dir: impl Into<PathBuf>, clock: Arc<dyn Clock>) -> SavedChats {
        Self::open_with(dir, clock, SavedLimits::default())
    }

    /// [`open`](Self::open) with other bounds than Decision 3's.
    pub fn open_with(dir: impl Into<PathBuf>, clock: Arc<dyn Clock>, limits: SavedLimits) -> SavedChats {
        SavedChats {
            dir: dir.into(),
            clock,
            limits,
            state: Mutex::default(),
        }
    }

    /// The folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file(&self, id: &str) -> VersionedFile<ChatFile> {
        VersionedFile::new(self.dir.join(format!("{id}.json")), FORMAT_VERSION)
            .max_bytes(self.limits.file_bytes)
            .private()
    }

    fn attachments(&self) -> PathBuf {
        self.dir.join(ATTACHMENTS_DIR)
    }

    fn now_ms(&self) -> u64 {
        self.clock
            .wall()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }

    /// The state, reading the folder on first use.
    fn state(&self) -> MutexGuard<'_, State> {
        let mut state = lock(&self.state);
        if !state.loaded {
            state.loaded = true;
            self.scan(&mut state);
        }
        state
    }

    fn scan(&self, state: &mut State) {
        let started = std::time::Instant::now();
        let (mut damaged, mut newer, mut unreadable, mut skipped) = (0usize, 0usize, 0usize, false);
        let names: Vec<String> = match std::fs::read_dir(&self.dir) {
            Ok(list) => list
                .filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter(|n| n.strip_suffix(".json").is_some_and(valid_id))
                .take(MAX_SCANNED)
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => {
                log::warn!(target: "pitwall_chat", "saved chats: the folder cannot be read ({e}); next step: check its permissions");
                state.problem = Some(format!(
                    "Pitwall cannot read its saved chats ({e}). New chats are kept until Pitwall closes."
                ));
                return;
            }
        };
        for name in names {
            let Some(id) = name.strip_suffix(".json") else { continue };
            if state.entries.len() >= self.limits.conversations {
                skipped = true;
                break;
            }
            let loaded = self.file(id).load();
            match (&loaded.problem, loaded.value.conversation) {
                (Some(FileProblem::Newer { .. }), _) => newer += 1,
                (Some(FileProblem::Unreadable { .. } | FileProblem::MissingMigration { .. }), _) => unreadable += 1,
                (problem, Some(c)) if c.id == id && !c.messages.is_empty() => {
                    if problem.is_some() {
                        damaged += 1;
                    }
                    let bytes = encoded_len(&ChatFile {
                        conversation: Some(c.clone()),
                    });
                    state.entries.insert(
                        id.to_string(),
                        Entry {
                            summary: summary_of(&c),
                            refs: refs_of(&c),
                            bytes,
                            body: None,
                            unsaved: false,
                            has_file: true,
                            used: 0,
                        },
                    );
                }
                // Damaged with no good backup (kept aside by VersionedFile), or a file that is not one of ours.
                _ => damaged += 1,
            }
        }
        state.attachment_bytes = std::fs::read_dir(self.attachments())
            .map(|list| {
                list.filter_map(Result::ok)
                    .take(MAX_SCANNED * 16)
                    .filter_map(|e| e.metadata().ok())
                    .filter(|m| m.is_file())
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0);
        log::info!(
            target: "pitwall_chat",
            "saved chats: read {} in {} ms (damaged {damaged}, newer {newer}, unreadable {unreadable}), attachments {} bytes",
            state.entries.len(),
            started.elapsed().as_millis(),
            state.attachment_bytes
        );
        let mut parts = Vec::new();
        if damaged > 0 {
            parts.push(format!(
                "{} could not be read and {} kept aside in the chats folder",
                plural(damaged, "chat", "chats"),
                if damaged == 1 { "was" } else { "were" }
            ));
        }
        if newer > 0 {
            parts.push(format!(
                "{} saved by a newer Pitwall {} left as {} (update Pitwall to open {})",
                plural(newer, "chat", "chats"),
                if newer == 1 { "is" } else { "are" },
                if newer == 1 { "it is" } else { "they are" },
                if newer == 1 { "it" } else { "them" }
            ));
        }
        if unreadable > 0 {
            parts.push(format!("{} could not be opened", plural(unreadable, "chat", "chats")));
        }
        if skipped {
            parts.push(format!("only the first {} chats are listed", self.limits.conversations));
        }
        if !parts.is_empty() {
            state.problem = Some(format!("Saved chats: {}.", parts.join("; ")));
        }
    }

    /// The conversation's body: cached, or read from its file.
    fn body(&self, state: &mut State, id: &str) -> Option<Conversation> {
        state.tick += 1;
        let tick = state.tick;
        let entry = state.entries.get_mut(id)?;
        entry.used = tick;
        if let Some(c) = &entry.body {
            return Some(c.clone());
        }
        let loaded = self.file(id).load();
        let c = loaded.value.conversation.filter(|c| c.id == id && !c.messages.is_empty());
        let Some(c) = c else {
            log::warn!(target: "pitwall_chat", "saved chats: {id} could no longer be read");
            state.entries.remove(id);
            state.problem = Some("A saved chat could no longer be read; it was kept aside in the chats folder.".into());
            return None;
        };
        entry.body = Some(c.clone());
        self.trim_cache(state);
        Some(c)
    }

    /// Keeps the cached bodies within [`CACHE_BYTES`] (the least recently used go; one never saved stays).
    fn trim_cache(&self, state: &mut State) {
        loop {
            let cached: u64 = state
                .entries
                .values()
                .filter(|e| e.body.is_some() && !e.unsaved)
                .map(|e| e.bytes)
                .sum();
            if cached <= CACHE_BYTES {
                return;
            }
            let oldest = state
                .entries
                .iter()
                .filter(|(_, e)| e.body.is_some() && !e.unsaved)
                .min_by_key(|(_, e)| e.used)
                .map(|(id, _)| id.clone());
            match oldest.and_then(|id| state.entries.get_mut(&id)) {
                Some(e) => e.body = None,
                None => return,
            }
        }
    }

    fn with_files(state: &State) -> usize {
        state.entries.values().filter(|e| e.has_file).count()
    }

    /// Writes `c` as its file within the limits; on refusal the conversation is kept for this session (`unsaved`).
    fn write(&self, state: &mut State, c: Conversation, new: bool) -> Result<(), SaveError> {
        let id = c.id.clone();
        let file = ChatFile { conversation: Some(c) };
        let bytes = encoded_len(&file);
        let Some(c) = file.conversation.clone() else { return Ok(()) };
        let had_file = state.entries.get(&id).is_some_and(|e| e.has_file);
        let refused = if !had_file && Self::with_files(state) >= self.limits.conversations {
            Some(format!(
                "Pitwall keeps at most {} saved chats. Delete chats you no longer need (Settings > Chats), then this one is saved \
                 again with its next answer; until then it is kept until Pitwall closes.",
                self.limits.conversations
            ))
        } else if bytes > self.limits.file_bytes {
            Some(format!(
                "this chat is larger than {} MiB, so its newest messages are kept only until Pitwall closes. Start a new chat to \
                 keep saving.",
                self.limits.file_bytes >> 20
            ))
        } else {
            None
        };
        let result = match refused {
            Some(words) => Err(SaveError(words)),
            None => {
                if let Err(e) = std::fs::create_dir_all(&self.dir) {
                    Err(SaveError(format!("the chats folder cannot be made ({e}).")))
                } else {
                    self.file(&id).save(&file).map_err(|e| SaveError(e.to_string()))
                }
            }
        };
        if let Err(e) = &result {
            log::warn!(target: "pitwall_chat", "saved chats: {id} not saved ({} bytes): {}", bytes, e.0);
        } else if new {
            log::info!(target: "pitwall_chat", "saved chats: {id} saved");
        }
        state.tick += 1;
        state.entries.insert(
            id,
            Entry {
                summary: summary_of(&c),
                refs: refs_of(&c),
                bytes,
                body: Some(c),
                unsaved: result.is_err(),
                has_file: had_file || result.is_ok(),
                used: state.tick,
            },
        );
        self.trim_cache(state);
        result
    }

    /// Whether `id` is here, and whether its file holds all of it (`Some(false)`: a limit kept it for this session only).
    pub fn is_on_disk(&self, id: &str) -> Option<bool> {
        self.state().entries.get(id).map(|e| !e.unsaved)
    }

    /// Takes in a conversation from the session's store with its id (the user turned "Don't save" off). Refused, with
    /// nothing kept, when the id is here already or a limit stops it.
    pub fn adopt(&self, conversation: Conversation) -> Result<(), SaveError> {
        if !valid_id(&conversation.id) || conversation.messages.is_empty() {
            return Err(SaveError("that chat cannot be saved.".into()));
        }
        let mut state = self.state();
        if state.entries.contains_key(&conversation.id) {
            return Err(SaveError("that chat is saved already.".into()));
        }
        let id = conversation.id.clone();
        let mut c = conversation;
        c.messages = c.messages.into_iter().map(StoredMessage::bounded).collect();
        let result = self.write(&mut state, c, true);
        if result.is_err() {
            state.entries.remove(&id);
        }
        result
    }

    /// Keeps `bytes` as an attachment and returns its SHA-256 (the same bytes are kept once). Refused, in words, past
    /// [`MAX_ATTACHMENT_BYTES`].
    pub fn put_attachment(&self, bytes: &[u8]) -> Result<String, SaveError> {
        let sha = hex(&Sha256::digest(bytes));
        let path = self.attachments().join(&sha);
        let mut state = self.state();
        if path.is_file() {
            return Ok(sha);
        }
        if state.attachment_bytes.saturating_add(bytes.len() as u64) > self.limits.attachment_bytes {
            return Err(SaveError(format!(
                "Pitwall keeps at most {} GiB of chat attachments. Delete chats you no longer need (Settings > Chats), then attach \
                 it again.",
                self.limits.attachment_bytes >> 30
            )));
        }
        std::fs::create_dir_all(self.attachments())
            .and_then(|()| paddock_env::write_atomic(&path, bytes, true).map_err(std::io::Error::other))
            .map_err(|e| SaveError(format!("the attachment cannot be kept ({e}).")))?;
        state.attachment_bytes += bytes.len() as u64;
        Ok(sha)
    }

    /// An attachment's bytes by its SHA-256.
    pub fn attachment(&self, sha: &str) -> Option<Vec<u8>> {
        valid_sha(sha).then(|| std::fs::read(self.attachments().join(sha)).ok()).flatten()
    }

    /// Removes the attachment files `gone` that no saved conversation refers to any more.
    fn drop_orphans(&self, state: &mut State, gone: &BTreeSet<String>) {
        for sha in gone {
            if !valid_sha(sha) || state.entries.values().any(|e| e.refs.contains(sha)) {
                continue;
            }
            let path = self.attachments().join(sha);
            if let Ok(meta) = std::fs::metadata(&path) {
                if std::fs::remove_file(&path).is_ok() {
                    state.attachment_bytes = state.attachment_bytes.saturating_sub(meta.len());
                }
            }
        }
    }

    /// Deletes every saved chat and attachment: every file of the chats' folder but `keep` (the chats' settings), including
    /// those a newer Pitwall wrote and those kept aside as damaged. Returns how many chats were listed.
    pub fn delete_all(&self, keep: &[&str]) -> Result<usize, SaveError> {
        let mut state = self.state();
        let count = state.entries.len();
        let mut failed = 0usize;
        for dir in [self.attachments(), self.dir.clone()] {
            let Ok(list) = std::fs::read_dir(&dir) else { continue };
            for e in list.filter_map(Result::ok) {
                let name = e.file_name().to_string_lossy().into_owned();
                if keep.iter().any(|k| name == *k || name == format!("{k}.bak")) || !e.file_type().is_ok_and(|t| t.is_file()) {
                    continue;
                }
                if std::fs::remove_file(e.path()).is_err() {
                    failed += 1;
                }
            }
        }
        let _ = std::fs::remove_dir(self.attachments());
        state.entries.clear();
        state.attachment_bytes = 0;
        log::info!(target: "pitwall_chat", "saved chats: all deleted ({count} listed, {failed} files could not be removed)");
        if failed > 0 {
            return Err(SaveError(format!(
                "{} could not be removed from the chats folder. Close what uses it and try again.",
                plural(failed, "file", "files")
            )));
        }
        Ok(count)
    }

    /// What the saved chats take on disk.
    pub fn usage(&self) -> Usage {
        let state = self.state();
        let saved = state.entries.values().filter(|e| e.has_file);
        Usage {
            chats: saved.clone().count(),
            bytes: saved.map(|e| e.bytes).sum(),
            attachment_bytes: state.attachment_bytes,
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Conversations for SavedChats {
    fn summaries(&self) -> Vec<ConversationSummary> {
        let mut list: Vec<ConversationSummary> = self.state().entries.values().map(|e| e.summary.clone()).collect();
        list.sort_by_key(|s| std::cmp::Reverse(s.updated_ms));
        list
    }

    fn get(&self, id: &str) -> Option<Conversation> {
        let mut state = self.state();
        self.body(&mut state, id)
    }

    fn create(&self, first: StoredMessage) -> (String, String, Result<(), SaveError>) {
        let now = self.now_ms();
        let mut state = self.state();
        let id = loop {
            state.counter += 1;
            let id = format!("s{now:x}-{}", state.counter);
            if !state.entries.contains_key(&id) && !self.dir.join(format!("{id}.json")).exists() {
                break id;
            }
        };
        let first = first.bounded();
        let title = conversation_title(&first.text);
        let c = Conversation {
            id: id.clone(),
            title: title.clone(),
            created_ms: now,
            updated_ms: now,
            messages: vec![first],
        };
        let saved = self.write(&mut state, c, true);
        (id, title, saved)
    }

    fn append(&self, id: &str, messages: Vec<StoredMessage>) -> Option<Result<(), SaveError>> {
        let now = self.now_ms();
        let mut state = self.state();
        let mut c = self.body(&mut state, id)?;
        c.messages.extend(messages.into_iter().map(StoredMessage::bounded));
        c.updated_ms = now;
        Some(self.write(&mut state, c, false))
    }

    fn delete(&self, id: &str) -> Result<bool, SaveError> {
        if !valid_id(id) {
            return Ok(false);
        }
        let mut state = self.state();
        let Some(entry) = state.entries.remove(id) else { return Ok(false) };
        let file = self.dir.join(format!("{id}.json"));
        let backup = self.dir.join(format!("{id}.json.bak"));
        for path in [&file, &backup] {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    log::warn!(target: "pitwall_chat", "saved chats: {id} could not be deleted: {e}");
                    state.entries.insert(id.to_string(), entry);
                    return Err(SaveError(format!("the chat's file could not be deleted ({e}).")));
                }
            }
        }
        self.drop_orphans(&mut state, &entry.refs);
        log::info!(target: "pitwall_chat", "saved chats: {id} deleted");
        Ok(true)
    }

    fn history(&self, id: &str, history_chars: usize) -> Vec<ChatMessage> {
        self.get(id).map_or_else(Vec::new, |c| history_of(&c, history_chars))
    }

    fn take_problem(&self) -> Option<String> {
        self.state().problem.take()
    }
}
