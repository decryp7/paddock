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
//! `limit_case!` (one test per limit) lives in `paddock-testkit`.

mod bounded;
mod exceeded;
mod id;
mod request;
mod row;

pub use bounded::{
    Bounded, BoundedError, BoundedWire, Cut, CutRow, CutWire, FieldPath, First, Last, MAX_CUTS,
    MAX_FIELD_PATH, Maybe, Next, Order, Sample, Selection, SortKey, TopBy, Total,
};
pub use exceeded::LimitExceeded;
pub use id::{LimitId, Quantity, Scope, SettingPath, Unit};
pub use request::{Buckets, Limit, Page, RequestError};
pub use row::{
    BOUNDED_CUTS, BUILTIN, LimitRow, Registry, RegistryError, Remedy, RowKind, RowValue,
    SelectionKind,
};

#[cfg(all(test, feature = "ts"))]
mod ts_tests;
