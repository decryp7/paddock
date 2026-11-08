//! Where each chat lives (ADR 0023 Decision 3 and its D3 note): saved on this computer ([`SavedChats`]), or, for a chat
//! marked "Don't save this chat", in memory only (`paddock-assistant`'s `ConversationStore::in_memory`), which leaves no file
//! behind even when the app is killed. The question that starts a chat says which (`SendRequest::save`); the default comes
//! from Settings > Chats ("Save new chats"). A chat can move either way later: turning "Don't save" on deletes its saved copy
//! (the UI asks first), turning it off saves it.

use super::saved::{SavedChats, Usage, CHATS_DIR};
use paddock_assistant::{
    Attachment, ChatMessage, Conversation, ConversationStore, ConversationSummary, Conversations, Overflow, SaveError, StoreLimits,
    StoredMessage, MAX_MESSAGES,
};
use paddock_env::VersionedFile;
use paddock_health::Clock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// The chats' settings, in the chats' folder (written only when the user changes them).
pub const PREFS_FILE: &str = "prefs.json";
/// Most chats kept in memory ("Don't save"): past it a new one is refused in words; none is ever dropped to make room.
pub const MAX_UNSAVED: usize = 200;
/// Most bytes of the unsaved chats' conversations in memory.
pub const MAX_UNSAVED_BYTES: u64 = 64 << 20;
/// Most bytes of the unsaved chats' attachments in memory.
pub const MAX_UNSAVED_ATTACHMENT_BYTES: u64 = 512 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Prefs {
    #[serde(default = "yes")]
    save_new: bool,
}

fn yes() -> bool {
    true
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { save_new: true }
    }
}

/// Settings > Chats: the default for new chats and what the saved ones take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStorage {
    /// New chats are saved unless the user says "Don't save this chat".
    pub save_new: bool,
    /// Saved chats.
    pub chats: usize,
    /// Their files, in bytes.
    pub bytes: u64,
    /// Their attachments, in bytes.
    pub attachment_bytes: u64,
    /// Chats kept until Pitwall closes ("Don't save").
    pub unsaved: usize,
}

/// Why a chat could not be moved or the chats' settings changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatsError {
    /// No such chat.
    NotFound,
    /// The words say why.
    Save(String),
}

impl std::fmt::Display for ChatsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatsError::NotFound => f.write_str("That chat no longer exists."),
            ChatsError::Save(m) => f.write_str(m),
        }
    }
}

impl From<SaveError> for ChatsError {
    fn from(e: SaveError) -> Self {
        ChatsError::Save(e.to_string())
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn refs(c: &Conversation) -> Vec<String> {
    c.messages
        .iter()
        .flat_map(|m| m.attachments.iter().map(|a| Attachment::sha256(a).to_string()))
        .collect()
}

/// The chat's conversations: saved ones and those kept in memory, behind one port.
pub struct ChatStores {
    saved: SavedChats,
    memory: ConversationStore,
    unsaved: StoreLimits,
    prefs: VersionedFile<Prefs>,
    save_new: Mutex<Option<bool>>,
    memory_attachments: Mutex<HashMap<String, Arc<Vec<u8>>>>,
}

impl ChatStores {
    /// The chats of the data folder `data` (`<data>/chats`); nothing is read or written until first use.
    pub fn new(data: &Path, clock: Arc<dyn Clock>) -> ChatStores {
        Self::with(SavedChats::open(data.join(CHATS_DIR), clock.clone()), clock)
    }

    /// Over a given saved store (tests use small limits).
    pub fn with(saved: SavedChats, clock: Arc<dyn Clock>) -> ChatStores {
        Self::with_unsaved_limits(
            saved,
            clock,
            StoreLimits {
                conversations: MAX_UNSAVED,
                file_bytes: MAX_UNSAVED_BYTES,
            },
        )
    }

    /// Over a given saved store, with other bounds for the chats that are not saved (tests).
    pub fn with_unsaved_limits(saved: SavedChats, clock: Arc<dyn Clock>, unsaved: StoreLimits) -> ChatStores {
        let prefs = VersionedFile::new(saved.dir().join(PREFS_FILE), 1).private();
        ChatStores {
            saved,
            memory: ConversationStore::in_memory_with(clock, unsaved).with_overflow(Overflow::Refuse),
            unsaved,
            prefs,
            save_new: Mutex::default(),
            memory_attachments: Mutex::default(),
        }
    }

    /// The saved chats.
    pub fn saved(&self) -> &SavedChats {
        &self.saved
    }

    /// Whether new chats are saved (Settings > Chats, "Save new chats"; on by default).
    pub fn save_new(&self) -> bool {
        *lock(&self.save_new).get_or_insert_with(|| self.prefs.load().value.save_new)
    }

    /// Changes the default for new chats.
    pub fn set_save_new(&self, on: bool) -> Result<(), ChatsError> {
        let mut current = lock(&self.save_new);
        std::fs::create_dir_all(self.saved.dir()).map_err(|e| ChatsError::Save(format!("The setting cannot be kept ({e}).")))?;
        self.prefs
            .save(&Prefs { save_new: on })
            .map_err(|e| ChatsError::Save(format!("The setting cannot be kept ({e}).")))?;
        *current = Some(on);
        log::info!(target: "pitwall_chat", "chats: new chats are {}", if on { "saved" } else { "not saved" });
        Ok(())
    }

    /// Whether the chat `id` is saved (`None`: there is no such chat).
    pub fn is_saved(&self, id: &str) -> Option<bool> {
        if self.memory.get(id).is_some() {
            return Some(false);
        }
        self.saved.is_on_disk(id).map(|_| true)
    }

    /// Saves the chat `id`, or keeps it in memory only and deletes its saved copy and the attachments only it used. The UI
    /// asks before the delete. Returns the chat's new state.
    pub fn set_saved(&self, id: &str, save: bool) -> Result<bool, ChatsError> {
        let saved_now = self.is_saved(id).ok_or(ChatsError::NotFound)?;
        if saved_now == save {
            return Ok(save);
        }
        if save {
            let c = self.memory.get(id).ok_or(ChatsError::NotFound)?;
            for sha in refs(&c) {
                let bytes = lock(&self.memory_attachments).get(&sha).cloned();
                if let Some(bytes) = bytes {
                    self.saved.put_attachment(&bytes)?;
                }
            }
            self.saved.adopt(c)?;
            let _ = self.memory.delete(id);
        } else {
            let c = self.saved.get(id).ok_or(ChatsError::NotFound)?;
            for sha in refs(&c) {
                if let Some(bytes) = self.saved.attachment(&sha) {
                    self.keep_in_memory(sha, bytes)?;
                }
            }
            self.memory.adopt(c).map_err(|e| self.unsaved_full(&e))?;
            if let Err(e) = self.saved.delete(id) {
                let _ = self.memory.delete(id);
                return Err(e.into());
            }
        }
        log::info!(target: "pitwall_chat", "chats: {id} is {}", if save { "saved" } else { "no longer saved" });
        Ok(save)
    }

    /// A refusal of the chats that are not saved, in Pitwall's words.
    fn unsaved_full(&self, why: &SaveError) -> SaveError {
        log::info!(target: "pitwall_chat", "chats: a chat that is not saved was refused ({})", why.0);
        SaveError(format!(
            "Pitwall keeps at most {} unsaved chats ({} MiB) and never drops one to make room. Save or delete an unsaved chat, \
             then ask again.",
            self.unsaved.conversations,
            self.unsaved.file_bytes >> 20,
        ))
    }

    fn keep_in_memory(&self, sha: String, bytes: Vec<u8>) -> Result<(), ChatsError> {
        let mut kept = lock(&self.memory_attachments);
        let total: u64 = kept.values().map(|b| b.len() as u64).sum();
        if !kept.contains_key(&sha) && total + bytes.len() as u64 > MAX_UNSAVED_ATTACHMENT_BYTES {
            return Err(ChatsError::Save(format!(
                "Chats that are not saved can hold at most {} MiB of attachments until Pitwall closes. Save this chat, or remove \
                 attachments from others.",
                MAX_UNSAVED_ATTACHMENT_BYTES >> 20
            )));
        }
        kept.insert(sha, Arc::new(bytes));
        Ok(())
    }

    /// Keeps an attachment for a chat that is saved (on disk) or not (in memory); returns its SHA-256.
    pub fn put_attachment(&self, save: bool, bytes: &[u8]) -> Result<String, ChatsError> {
        if save {
            return Ok(self.saved.put_attachment(bytes)?);
        }
        let sha: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
        self.keep_in_memory(sha.clone(), bytes.to_vec())?;
        Ok(sha)
    }

    /// Keeps a file the user just attached in memory until its question is sent (ADR 0023 WP-4b): the chat may still become
    /// "Don't save", or the file be removed before sending, so nothing is written yet. A saved chat's question writes it
    /// ([`Conversations::create_saved`], [`Conversations::append`]). Returns its SHA-256.
    pub fn stage_attachment(&self, bytes: &[u8]) -> Result<String, ChatsError> {
        self.put_attachment(false, bytes)
    }

    /// Writes the staged attachments `messages` refer to beside the saved chats (those already on disk are kept once).
    fn save_staged(&self, messages: &[StoredMessage]) -> Result<(), SaveError> {
        for a in messages.iter().flat_map(|m| m.attachments.iter()) {
            let sha = Attachment::sha256(a);
            let staged = lock(&self.memory_attachments).get(sha).cloned();
            if let Some(bytes) = staged {
                self.saved.put_attachment(&bytes)?;
            }
        }
        Ok(())
    }

    /// An attachment's bytes by its SHA-256, wherever it is kept.
    pub fn attachment(&self, sha: &str) -> Option<Vec<u8>> {
        let kept = lock(&self.memory_attachments).get(sha).cloned();
        kept.map(|b| b.as_ref().clone()).or_else(|| self.saved.attachment(sha))
    }

    /// Deletes every chat, saved or not, and every attachment; the chats' settings stay. Returns how many chats went.
    pub fn delete_all(&self) -> Result<usize, ChatsError> {
        let unsaved: Vec<String> = self.memory.summaries().into_iter().map(|s| s.id).collect();
        for id in &unsaved {
            let _ = self.memory.delete(id);
        }
        lock(&self.memory_attachments).clear();
        Ok(unsaved.len() + self.saved.delete_all(&[PREFS_FILE])?)
    }

    /// Settings > Chats.
    pub fn storage(&self) -> ChatStorage {
        let Usage {
            chats,
            bytes,
            attachment_bytes,
        } = self.saved.usage();
        ChatStorage {
            save_new: self.save_new(),
            chats,
            bytes,
            attachment_bytes,
            unsaved: self.memory.summaries().len(),
        }
    }

    fn holder(&self, id: &str) -> &dyn Conversations {
        if self.memory.get(id).is_some() {
            &self.memory
        } else {
            &self.saved
        }
    }
}

impl Conversations for ChatStores {
    fn summaries(&self) -> Vec<ConversationSummary> {
        let mut list = self.saved.summaries();
        list.extend(self.memory.summaries());
        list.sort_by_key(|s| std::cmp::Reverse(s.updated_ms));
        list
    }

    fn get(&self, id: &str) -> Option<Conversation> {
        self.memory.get(id).or_else(|| self.saved.get(id))
    }

    fn create(&self, first: StoredMessage) -> (String, String, Result<(), SaveError>) {
        self.create_saved(first, self.save_new())
    }

    fn create_saved(&self, first: StoredMessage, save: bool) -> (String, String, Result<(), SaveError>) {
        if save {
            let staged = self.save_staged(std::slice::from_ref(&first));
            let (id, title, kept) = self.saved.create(first);
            // The chat is kept either way; a file that could not be written is said once, with the rest.
            (id, title, staged.and(kept))
        } else {
            self.memory.create(first)
        }
    }

    fn append(&self, id: &str, messages: Vec<StoredMessage>) -> Option<Result<(), SaveError>> {
        let staged = if self.memory.get(id).is_none() && self.saved.is_on_disk(id).is_some() {
            self.save_staged(&messages)
        } else {
            Ok(())
        };
        self.holder(id).append(id, messages).map(|kept| staged.and(kept))
    }

    fn delete(&self, id: &str) -> Result<bool, SaveError> {
        if self.memory.delete(id)? {
            return Ok(true);
        }
        self.saved.delete(id)
    }

    fn history(&self, id: &str, history_chars: usize) -> Vec<ChatMessage> {
        self.holder(id).history(id, history_chars)
    }

    fn take_problem(&self) -> Option<String> {
        self.saved.take_problem()
    }

    fn admits(&self, conversation_id: Option<&str>, save: bool) -> Result<(), SaveError> {
        let unsaved = match conversation_id {
            None => !save,
            Some(id) => self.memory.get(id).is_some(),
        };
        if !unsaved {
            // The saved store refuses past its limits in words and keeps the chat for the session: never a drop.
            return Ok(());
        }
        let long = conversation_id
            .and_then(|id| self.memory.get(id))
            .is_some_and(|c| c.messages.len() >= MAX_MESSAGES);
        self.memory.admits(conversation_id, false).map_err(|e| {
            if long {
                SaveError(format!(
                    "This chat holds as many messages as a chat can ({MAX_MESSAGES}). Start a new chat to go on."
                ))
            } else {
                self.unsaved_full(&e)
            }
        })
    }
}
