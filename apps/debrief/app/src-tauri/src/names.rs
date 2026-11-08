//! Human-name matching in the shell (ADR 0053): each window holds the [`Names`] of its workspace, rebuilt when the
//! entity catalogue is built, and persisted (user corrections and old AI aliases) in the configuration folder.
//!
//! Thin by design: grouping, matching, ranking and questions are the engine's (`debrief_core::names`); this module
//! owns the files and the commands. No model is asked here: names are never sent to a model unless the user starts
//! it (rule 46).
//!
//! Commands (all validated here, the engine validates again): `names_accept`, `names_reject`, `names_undo`,
//! `names_remove_alias`; `find_values` (in `search_everything`) answers through [`WindowNames::search`].

use crate::error::{CmdError, ErrorCode};
use crate::windows::{CurrentWindow, WindowState, Windows};
use crate::CmdResult;
use debrief_core::find::Resolved;
use debrief_core::names::book::{NameBook, UserFact, MAX_TERM_CHARS};
use debrief_core::names::Names;
use debrief_core::ports::Store;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

/// File of the name book below the configuration folder.
pub const BOOK_FILE: &str = "names/book.json";
/// Longest group id accepted from the webview.
const MAX_ID_CHARS: usize = 256;
/// How often the import watcher asks whether the catalogue is built, and how long it keeps asking.
const WATCH_POLL: Duration = Duration::from_millis(500);
const WATCH_MAX: Duration = Duration::from_secs(10 * 60);

/// What the table was last built from: events and sources of the window.
type Signature = (u64, usize);

/// Whether [`WindowNames::ensure_fresh`] had to rebuild the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Built from the workspace as it is.
    Current,
    /// Rebuilt just now.
    Rebuilt,
    /// The catalogue is not built yet (or the store cannot say): the name layer stays as it was.
    NotBuilt,
}

#[derive(Default)]
struct Inner {
    names: Arc<Names>,
    file: Option<PathBuf>,
    seen: Mutex<Option<Signature>>,
}

/// A window's name layer. Cheap to clone.
#[derive(Clone, Default)]
pub struct WindowNames(Arc<Inner>);

impl WindowNames {
    /// The layer with its book read from `file` (a missing or damaged file gives an empty book and a log line).
    pub fn open(file: Option<PathBuf>) -> WindowNames {
        let names = Arc::new(Names::default());
        if let Some(f) = &file {
            let (book, note) = NameBook::load(f);
            if let Some(n) = note {
                log::warn!("{n}");
            }
            names.set_book(book);
        }
        WindowNames(Arc::new(Inner {
            names,
            file,
            ..Inner::default()
        }))
    }

    pub fn names(&self) -> &Arc<Names> {
        &self.0.names
    }

    /// Writes the book (user corrections and old AI aliases). A failure is logged: the session keeps working.
    pub fn save(&self) {
        let Some(file) = &self.0.file else { return };
        if let Some(dir) = file.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::warn!("the name book folder cannot be created: {e}");
                return;
            }
        }
        if let Err(e) = self.0.names.read(|b, _| b.save(file)) {
            log::warn!("the name book cannot be saved: {e}");
        }
    }

    /// Rebuilds the table when the workspace changed since the last build and its catalogue is built.
    pub fn ensure_fresh(&self, store: &dyn Store) -> Freshness {
        let Ok(summary) = store.summary() else { return Freshness::NotBuilt };
        let signature = (summary.total as u64, summary.sources.len());
        if *crate::sync::lock(&self.0.seen) == Some(signature) && self.0.names.is_built() {
            return Freshness::Current;
        }
        if self.0.names.refresh(store).is_err() || !self.0.names.is_built() {
            return Freshness::NotBuilt;
        }
        *crate::sync::lock(&self.0.seen) = Some(signature);
        Freshness::Rebuilt
    }

    /// The layer for the assistant, up to date.
    pub fn for_assistant(&self, store: Option<&dyn Store>) -> Arc<Names> {
        if let Some(s) = store {
            self.ensure_fresh(s);
        }
        self.0.names.clone()
    }

    /// The values and entities the words refer to (see [`Names::search`]).
    pub fn search(&self, store: &dyn Store, text: &str, limit: usize) -> CmdResult<(Resolved, Freshness)> {
        let fresh = self.ensure_fresh(store);
        let found = self
            .0
            .names
            .search(store, text, limit)
            .map_err(|e| CmdError::new(ErrorCode::Internal, format!("the values cannot be looked up: {e}")))?;
        Ok((found, fresh))
    }
}

/// Waits (in the background) for the window's catalogue to be built after an import, then rebuilds the name table. Ends when the window is gone, the table is current, or after `max`.
pub fn watch_catalogue<R: Runtime>(app: &AppHandle<R>, label: &str, poll: Duration, max: Duration) {
    let (app, label) = (app.clone(), label.to_string());
    let spawned = std::thread::Builder::new().name("debrief-names".into()).spawn(move || {
        let started = std::time::Instant::now();
        while started.elapsed() < max {
            let Some(win) = app.try_state::<Windows>().and_then(|w| w.get(&label)) else {
                return;
            };
            let Ok(store) = win.engine.store() else { return };
            match win.names.ensure_fresh(store.as_ref()) {
                Freshness::Rebuilt | Freshness::Current => return,
                Freshness::NotBuilt => {}
            }
            drop((win, store));
            std::thread::sleep(poll);
        }
    });
    if let Err(e) = spawned {
        log::warn!("the name watcher cannot start: {e}");
    }
}

/// An import finished in `label`: its name table is rebuilt once the catalogue is ready.
pub fn after_import<R: Runtime>(app: &AppHandle<R>, label: &str) {
    watch_catalogue(app, label, WATCH_POLL, WATCH_MAX);
}

// Commands ---------------------------------------------------------------------------------------------------------

/// What the user said about a match: the words they typed and the entity (`group` of a `ValueMatch`) they mean or not.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameFeedback {
    pub text: String,
    pub group: String,
    /// Remember it for every workspace, not only this one.
    #[serde(default)]
    pub global: bool,
}

/// An AI alias to remove from an entity.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveAlias {
    pub group: String,
    pub alias: String,
}

/// The fact that was recorded (its id undoes it), or none when the book already said so.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactReply {
    pub fact: Option<u64>,
}

/// What `names_undo` took back.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoReply {
    pub undone: Option<UserFact>,
}

/// What `names_remove_alias` did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedReply {
    pub removed: bool,
}

fn invalid(message: impl Into<String>) -> CmdError {
    CmdError::new(ErrorCode::Invalid, message)
}

/// A phrase from the webview: non-empty, bounded, no control characters.
fn phrase(what: &str, s: &str) -> CmdResult<String> {
    let t = s.trim();
    if t.is_empty() {
        return Err(invalid(format!("{what} is empty")));
    }
    if t.chars().count() > MAX_TERM_CHARS {
        return Err(invalid(format!("{what} is longer than {MAX_TERM_CHARS} characters")));
    }
    if t.chars().any(char::is_control) {
        return Err(invalid(format!("{what} holds control characters")));
    }
    Ok(t.to_string())
}

/// A group id the book knows.
fn known_group(names: &Names, id: &str) -> CmdResult<String> {
    if id.is_empty() || id.chars().count() > MAX_ID_CHARS {
        return Err(invalid("the name is not valid; search again"));
    }
    if names.read(|b, _| b.group(id).is_none()) {
        return Err(invalid("that name is no longer in the logs; search again"));
    }
    Ok(id.to_string())
}

fn feedback(
    state: &WindowState,
    request: &NameFeedback,
    apply: impl FnOnce(&mut NameBook, &str, &str, bool) -> Option<u64>,
) -> CmdResult<FactReply> {
    let text = phrase("the words", &request.text)?;
    let group = known_group(state.names.names(), &request.group)?;
    let fact = state.names.names().write(|b| apply(b, &text, &group, request.global));
    state.names.save();
    Ok(FactReply { fact })
}

/// "Yes, that is what I meant": the words now mean the entity (outranks everything, including the AI's aliases).
#[tauri::command]
pub async fn names_accept(state: CurrentWindow, request: NameFeedback) -> CmdResult<FactReply> {
    feedback(&state, &request, NameBook::accept)
}

/// "No, not that": the words never mean the entity again.
#[tauri::command]
pub async fn names_reject(state: CurrentWindow, request: NameFeedback) -> CmdResult<FactReply> {
    feedback(&state, &request, NameBook::reject)
}

/// Takes back the user's latest correction.
#[tauri::command]
pub async fn names_undo(state: CurrentWindow) -> CmdResult<UndoReply> {
    let undone = state.names.names().write(NameBook::undo);
    state.names.save();
    Ok(UndoReply { undone })
}

/// Removes an AI-suggested alias (it is not suggested again by the same model).
#[tauri::command]
pub async fn names_remove_alias(state: CurrentWindow, request: RemoveAlias) -> CmdResult<RemovedReply> {
    let alias = phrase("the alias", &request.alias)?;
    let group = known_group(state.names.names(), &request.group)?;
    let removed = state.names.names().write(|b| b.remove_ai_alias(&group, &alias));
    if removed {
        state.names.save();
    }
    Ok(RemovedReply { removed })
}

#[cfg(test)]
mod tests {
    use super::*;
    use debrief_core::names::group::NameGroup;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("debrief-it-names-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn with_group(win: &WindowNames, spelling: &str, count: u64) {
        use debrief_core::catalogue::{Candidate, Scope};
        use debrief_core::entities::EntityKind;
        use debrief_core::names::group::group;
        let c = Candidate {
            scope: Scope::Entity(EntityKind::Executable),
            name: spelling.into(),
            count,
            first_ts: None,
            last_ts: None,
            seen_in: vec![],
        };
        let groups: Vec<NameGroup> = group(&[c], &debrief_core::names::Vocabulary::builtin());
        win.names().write(|b| b.rebuild(groups));
    }

    #[test]
    fn the_book_survives_a_restart_and_tolerates_bad_files() {
        let dir = temp("persist");
        let file = dir.join(BOOK_FILE);
        let win = WindowNames::open(Some(file.clone()));
        with_group(&win, "graphicbuilder.exe", 9);
        assert!(win
            .names()
            .write(|b| b.accept("gfx", "executable|graphic builder", false))
            .is_some());
        win.save();
        let again = WindowNames::open(Some(file.clone()));
        assert_eq!(again.names().read(|b, _| b.facts().len()), 1);
        // A damaged file gives an empty book, not a failure.
        std::fs::write(&file, b"{ not json").unwrap();
        assert_eq!(WindowNames::open(Some(file.clone())).names().read(|b, _| b.facts().len()), 0);
        // A missing file and no file at all too.
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(WindowNames::open(Some(file)).names().read(|b, _| b.groups().len()), 0);
        WindowNames::open(None).save();
    }

    #[test]
    fn webview_arguments_are_validated() {
        assert!(phrase("x", "  ").is_err());
        assert!(phrase("x", &"a".repeat(MAX_TERM_CHARS + 1)).is_err());
        assert!(phrase("x", "a\u{0}b").is_err());
        assert_eq!(phrase("x", "  gfx ").unwrap(), "gfx");
        let win = WindowNames::open(None);
        with_group(&win, "graphicbuilder.exe", 9);
        assert!(known_group(win.names(), "executable|graphic builder").is_ok());
        for bad in ["", "nope", &"g".repeat(MAX_ID_CHARS + 1)] {
            assert_eq!(known_group(win.names(), bad).unwrap_err().code, ErrorCode::Invalid);
        }
    }

    #[test]
    fn accept_reject_undo_and_remove_alias_change_the_book_and_save_it() {
        let dir = temp("feedback");
        let file = dir.join(BOOK_FILE);
        let win = WindowNames::open(Some(file.clone()));
        with_group(&win, "graphicbuilder.exe", 9);
        let mut state = crate::windows::tests::window_state("names-fb", debrief_core::PluginRegistry::new());
        state.names = win.clone();
        let req = |text: &str, group: &str| NameFeedback {
            text: text.into(),
            group: group.into(),
            global: false,
        };
        let id = "executable|graphic builder";
        assert!(feedback(&state, &req("gfx", id), NameBook::accept).unwrap().fact.is_some());
        assert!(feedback(&state, &req("paint", id), NameBook::reject).unwrap().fact.is_some());
        assert_eq!(win.names().read(|b, _| b.facts().len()), 2);
        assert_eq!(WindowNames::open(Some(file)).names().read(|b, _| b.facts().len()), 2, "saved");
        assert_eq!(
            feedback(&state, &req("", id), NameBook::accept).unwrap_err().code,
            ErrorCode::Invalid
        );
        assert_eq!(
            feedback(&state, &req("gfx", "nope"), NameBook::accept).unwrap_err().code,
            ErrorCode::Invalid
        );
        assert!(win.names().write(NameBook::undo).is_some());
        assert_eq!(win.names().read(|b, _| b.facts().len()), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
