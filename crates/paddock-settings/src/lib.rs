//! Settings for the Paddock apps: the section schema (sections, typed fields, validation, owners) and the versioned
//! settings file that falls back to defaults when it is missing or corrupt.
//!
//! Rule 21 (tunables are settings), ADR 0012 §3; plan S10. Taken from Debrief's `debrief_plugin_api::settings` (schema
//! and validation), `config_store.rs` (versioned, atomic files) and the generic half of `settings.rs` (the service),
//! without Debrief's sections, plugins or groups.
//!
//! - [`schema`]: [`SettingsSection`], [`SettingField`], [`FieldKind`] (bool, int, float, text, enum, path, duration,
//!   size, string list) and [`SettingsSection::check`], which refuses a defective schema at registration.
//! - [`values`]: [`validate`] and [`sanitize`] turn untrusted JSON into [`ValidValues`] (parse, don't validate), with one
//!   [`FieldError`] per refused field; [`ValidValues::decode`] gives the app its own typed struct.
//! - [`file`](mod@file): the versioned `settings.json`, read once (missing, damaged, newer or unreadable: defaults and one
//!   [`LoadProblem`]; damaged and newer files are kept aside) and written atomically.
//! - [`service`]: [`Settings`], the registry of the shell's and the app's sections ([`Configurable`]); every change is
//!   validated, applied, saved and announced to a [`ChangeSink`] in one place.
//! - [`shell`]: the shell's own sections ([`shell::appearance`]).
//!
//! The core is pure Rust; the adapter (`tauri`: managed state, `paddock://settings` to every window, the commands
//! `settings_overview`, `settings_set` and `settings_reset`, the start-up problem notice) lives behind the `tauri`
//! feature.
//!
//! ```
//! use paddock_settings::{FieldKind, SectionOwner, SettingField, Settings, SettingsSection};
//! use serde_json::json;
//! use std::sync::Arc;
//!
//! let settings = Settings::in_memory();
//! let section = SettingsSection::new("pw.models", "Models", SectionOwner::App).field(
//!     SettingField::new("parallel", "Parallel downloads", FieldKind::Int { min: 1, max: 8, step: 1, unit: None }, 4),
//! );
//! settings.register(Arc::new(section)).unwrap();
//! let refused = settings.set("pw.models", &json!({ "parallel": 20 })).unwrap();
//! assert!(!refused.applied);
//! assert_eq!(refused.errors[0].message, "Must be between 1 and 8.");
//! assert!(settings.set("pw.models", &json!({ "parallel": 2 })).unwrap().applied);
//! assert_eq!(settings.values("pw.models").unwrap().int("parallel"), Some(2));
//! ```

pub mod file;
pub mod format;
pub mod schema;
pub mod service;
pub mod shell;
pub mod values;

pub use file::{
    FILE_NAME, FORMAT_VERSION, LoadProblem, LoadProblemKind, MAX_FILE_BYTES, SettingsFile,
};
pub use format::format_ms;
pub use schema::{
    ByteKind, EnumOption, FieldKind, MAX_DESCRIPTION_LEN, MAX_FIELDS, MAX_ID_LEN, MAX_ITEM_LEN,
    MAX_LABEL_LEN, MAX_LIST_ITEMS, MAX_OPTIONS, MAX_PATH_LEN, MAX_PATTERN_LEN, MAX_TEXT_LEN,
    SchemaError, SectionOwner, SettingField, SettingsSection, valid_id,
};
pub use service::{
    ApplyError, ChangeSink, Configurable, FnSection, RegisterError, SectionView, SetResult,
    Settings, SettingsChanged, SettingsCommandError, SettingsError, SettingsErrorCode,
    SettingsOverview, section,
};
pub use values::{FieldError, ValidValues, sanitize, validate, validate_field};

/// The event every change is sent on to every window (the Tauri adapter's `EVENT`).
pub const EVENT_NAME: &str = "paddock://settings";

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(test)]
mod generated;
