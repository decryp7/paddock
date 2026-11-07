//! Limits that explain themselves (family ADR 0019) and bounded replies that say what they left out (Debrief ADR 0065).
//!
//! A leaf crate: serde, and ts-rs behind the `ts` feature. API design: `docs/plans/debrief-1-2-api.md` section 4.6.
//!
//! - [`LimitId`], [`Quantity`] (a value with its unit), [`Scope`], [`SettingPath`]: the vocabulary.
//! - [`LimitRow`] and [`Registry`]: one row per limit (unit, default, bounds, Setting path or hard, kind, remedy);
//!   [`Registry::check`] validates a registry.
//! - [`LimitExceeded`]: the typed error a limit that stopped work carries up every layer; [`LimitExceeded::message`]
//!   generates the text from the type and its registry row, never from a string at the call site.
//! - [`Bounded`], [`Cut`], [`Total`], [`Selection`], [`Next`], [`FieldPath`], [`CutRow`]: a reply plus what was left out of
//!   it. Fields are private; the constructors enforce the arithmetic and deserializing runs the same checks.
//! - [`Limit`], [`Page`], [`Buckets`]: the request-side types of ADR 0018.
//!
//! `limit_cases!` (one test per limit) lives in `paddock-testkit`.
//!
//! **Wire format:** JSON or another self-describing format. The cut variants are internally tagged, which postcard cannot
//! decode; a postcard boundary carries the cuts as its own typed field.

mod bounded;
mod exceeded;
mod fieldpath;
mod id;
mod request;
mod row;

pub use bounded::{
    Bounded, BoundedError, BoundedWire, Cut, CutRow, CutWire, First, Last, LimitOutOfRange,
    MAX_CUTS, Maybe, Next, Order, RowLimit, Sample, Selection, SelectionMarker, SortKey, TopBy,
    Total,
};
pub use exceeded::{LimitExceeded, SettingLabels, UnitMismatch};
pub use fieldpath::{FieldPath, MAX_FIELD_PATH};
pub use id::{BadKey, LimitId, Quantity, Scope, SettingPath, Unit};
pub use request::{Buckets, Limit, Page, RequestError};
pub use row::{
    BOUNDED_CUTS, BUILTIN, LimitRow, Registry, RegistryError, Remedy, RowKind, RowValue,
    SelectionKind,
};

#[cfg(all(test, feature = "ts"))]
mod ts_tests;
