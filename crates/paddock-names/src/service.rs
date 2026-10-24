//! The engine side of the palette's `names_search` command: the request and reply shapes (camelCase JSON, mirrored by the UI's
//! `api.types.ts`), argument validation in Rust (the webview is not trusted), and a [`NamesService`] that holds the current
//! catalogue and swaps a rebuilt one in whole (build new, then swap, rule 11).
//!
//! The app registers one query command that calls [`NamesService::search`]:
//!
//! ```text
//! Query [WINDOW] limit 4 * paddock_commands::KIB => names::names_search,
//! ```
//!
//! and rebuilds the catalogue in a job when its data changes, then calls [`NamesService::replace`]. The palette and the assistant
//! only ever call the command.

use crate::catalogue::{
    Catalogue, Clarification, Entry, MAX_MATCHES, MAX_TEXT_CHARS, Match, NamesError,
};
use crate::normalise::Style;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::{Arc, PoisonError, RwLock};

/// The command name.
pub const NAMES_SEARCH: &str = "names_search";

/// The arguments of `names_search`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NamesSearchRequest {
    /// What the user typed.
    pub query: String,
    /// Most matches wanted (1 to 50; absent means 10).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// The reply of `names_search`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamesSearchReply {
    /// The matches, best first, each with how and why.
    pub matches: Vec<Match>,
    /// A question to ask the user when the top matches tie.
    pub clarify: Option<Clarification>,
    /// How many names are searchable now (0 while the first catalogue is being built).
    pub searchable: usize,
}

/// A request the engine refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {
    /// The query is longer than [`MAX_TEXT_CHARS`] characters.
    QueryTooLong,
    /// The limit is 0 or over [`MAX_MATCHES`].
    BadLimit,
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RequestError::QueryTooLong => write!(
                f,
                "Type at most {MAX_TEXT_CHARS} characters to search for a name."
            ),
            RequestError::BadLimit => write!(f, "Ask for between 1 and {MAX_MATCHES} matches."),
        }
    }
}

impl std::error::Error for RequestError {}

/// The current catalogue of an app, replaced as a whole.
pub struct NamesService {
    current: RwLock<Arc<Catalogue>>,
}

impl Default for NamesService {
    fn default() -> Self {
        NamesService {
            current: RwLock::new(Arc::new(Catalogue::empty())),
        }
    }
}

impl NamesService {
    /// A service with nothing to search yet.
    pub fn new() -> NamesService {
        NamesService::default()
    }

    /// Builds a catalogue of the entries (outside any lock; call it from a job) and swaps it in. On an error the old catalogue
    /// stays.
    pub fn replace(&self, entries: Vec<Entry>, style: Style) -> Result<usize, NamesError> {
        let built = match style {
            Style::Path => Catalogue::new(entries)?,
            Style::Text => Catalogue::text(entries)?,
        };
        let n = built.len();
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(built);
        Ok(n)
    }

    /// Answers `names_search`. Reads one catalogue for the whole search, so a swap in between cannot mix two.
    pub fn search(&self, request: &NamesSearchRequest) -> Result<NamesSearchReply, RequestError> {
        if request.query.chars().count() > MAX_TEXT_CHARS {
            return Err(RequestError::QueryTooLong);
        }
        let limit = match request.limit {
            None => 10,
            Some(n) if (1..=MAX_MATCHES).contains(&n) => n,
            Some(_) => return Err(RequestError::BadLimit),
        };
        let catalogue = self
            .current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let found = catalogue.search(&request.query, limit);
        Ok(NamesSearchReply {
            matches: found.matches,
            clarify: found.clarify,
            searchable: catalogue.len(),
        })
    }
}
