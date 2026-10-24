//! Identifier search for the Paddock apps (plan X18, rule 20): the user types "grafic bilder" and finds
//! `CompanyA.Graphic.Builder.exe`; the pure parts of Debrief's `names/` and Dyno's counter search, without Debrief's entity
//! catalogue, AI aliases or context ranking (those stay in the app and layer on top).
//!
//! - [`segment`] / [`Vocabulary`]: word segmentation of run-together names (`graphicbuilder` is `graphic builder`) from an
//!   embedded word list plus the data's own words.
//! - [`normalise`]: a name as lower-case words, in a [`Style`]: [`Style::Path`] drops the path, extension, vendor prefix,
//!   version and architecture; [`Style::Text`] drops nothing.
//! - [`group`]: spellings of one name become one [`Group`] with variants and counts.
//! - [`Catalogue`]: [`Entry`]s (an id, spellings, other fields, aliases, a weight) searched by [`Catalogue::search`], which
//!   returns ranked [`Match`]es that say [`How`] and why they matched, and a [`Clarification`] on a true tie.
//! - [`edit_distance_capped`]: the typo measure.
//!
//! Everything is pure, deterministic and bounded; nothing here reads a file or the clock. The palette and the assistant reach it
//! through an engine command (the engine is the source of truth).

mod catalogue;
mod distance;
mod group;
mod index;
mod normalise;
mod segment;
mod service;

pub use catalogue::{
    Catalogue, Clarification, Entry, How, MAX_ALIASES, MAX_ENTRIES, MAX_FIELDS, MAX_MATCHES,
    MAX_NAME_CHARS, MAX_NAMES, MAX_OPTIONS, MAX_TEXT_CHARS, Match, NamesError, Search, TIE_GAP,
};
pub use distance::{allowed_typos, edit_distance_capped};
pub use group::{Candidate, Group, MAX_SPELLINGS, Spelling, group, vocabulary_from};
pub use normalise::{
    NormName, Style, file_name, normalise, normalise_as, squash, strip_extension, vendor_prefixes,
};
pub use segment::{MAX_SEGMENTS, MAX_TOKEN_CHARS, Vocabulary, pieces, segment, split_raw};
pub use service::{NAMES_SEARCH, NamesSearchReply, NamesSearchRequest, NamesService, RequestError};
