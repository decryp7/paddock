//! The typed error of a limit that stopped work.

use crate::id::{LimitId, Quantity, Scope, SettingPath, Unit};
use crate::row::{Registry, Remedy, RowValue};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The observed value of a limit is in another unit than the limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitMismatch {
    /// The limit's unit.
    pub limit: Unit,
    /// The observed value's unit.
    pub reached: Unit,
}

impl fmt::Display for UnitMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the limit is in {:?} but the observed value is in {:?}",
            self.limit, self.reached
        )
    }
}

impl std::error::Error for UnitMismatch {}

/// The words a Settings schema uses for a path, so a message can say "Settings -> Performance -> Plugin memory" instead of a
/// dotted id. The app implements it over its schema; this crate does not depend on the Settings crate.
pub trait SettingLabels {
    /// The section's and the field's labels, or `None` when the schema does not know the path.
    fn label(&self, path: SettingPath) -> Option<(&str, &str)>;
}

/// A limit stopped the work. Every layer between the check and the person keeps this type (ADR 0019 decision 1); none
/// turns it into a `String`. `limit` and `reached` are in the same unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LimitExceeded {
    /// Which limit.
    pub id: LimitId,
    /// Its value when it was hit.
    pub limit: Quantity,
    /// The observed value, when known.
    pub reached: Option<Quantity>,
    /// What it applied to.
    pub scope: Scope,
}

/// What `LimitExceeded` reads from the wire; its own `Deserialize` checks the units.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LimitExceededRepr {
    id: LimitId,
    limit: Quantity,
    reached: Option<Quantity>,
    scope: Scope,
}

impl<'de> Deserialize<'de> for LimitExceeded {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = LimitExceededRepr::deserialize(d)?;
        let base = Self::new(r.id, r.limit, r.scope);
        match r.reached {
            Some(q) => base.reaching(q).map_err(serde::de::Error::custom),
            None => Ok(base),
        }
    }
}

impl LimitExceeded {
    /// A limit hit with an unknown observed value.
    #[must_use]
    pub const fn new(id: LimitId, limit: Quantity, scope: Scope) -> Self {
        Self {
            id,
            limit,
            reached: None,
            scope,
        }
    }

    /// The same, with the value that was reached.
    ///
    /// # Errors
    /// [`UnitMismatch`] when `reached` is not in the limit's unit.
    pub fn reaching(mut self, reached: Quantity) -> Result<Self, UnitMismatch> {
        if !self.limit.unit_eq(reached) {
            return Err(UnitMismatch {
                limit: self.limit.unit(),
                reached: reached.unit(),
            });
        }
        self.reached = Some(reached);
        Ok(self)
    }

    /// The Setting that raises this limit, from its registry row; the structured field a log line carries instead of the
    /// words of the message.
    #[must_use]
    pub fn setting_path(&self, registry: &Registry) -> Option<SettingPath> {
        match registry.get(self.id) {
            Some(row) => match (row.remedy, row.value) {
                (Remedy::RaiseSetting { path }, _) | (_, RowValue::Setting { path }) => Some(path),
                _ => None,
            },
            None => None,
        }
    }

    /// The message, generated from the type and its registry row (ADR 0019 decision 3): what stopped, the observed value
    /// (rounded: it was measured) when known, the limit and its exact value, and what to do. The Setting is named by its
    /// dotted path; [`LimitExceeded::message_with`] uses the schema's labels. A row missing from the registry still gives
    /// the first parts.
    #[must_use]
    pub fn message(&self, registry: &Registry) -> String {
        self.compose(registry, None)
    }

    /// [`LimitExceeded::message`] with the Settings schema's words for the path ("Performance -> Plugin memory"); a path
    /// the schema does not know falls back to the dotted path.
    #[must_use]
    pub fn message_with(&self, registry: &Registry, labels: &dyn SettingLabels) -> String {
        self.compose(registry, Some(labels))
    }

    fn compose(&self, registry: &Registry, labels: Option<&dyn SettingLabels>) -> String {
        // The id is for logs; a message built with the schema's words is for a person and shows the value instead.
        let limit_is = match labels {
            Some(_) => format!("the limit is {}.", self.limit),
            None => format!("the limit ({}) is {}.", self.id, self.limit),
        };
        let mut out = format!("The {} stopped: ", self.scope.word());
        if let Some(reached) = self.reached {
            out.push_str(&format!("it reached {}; ", reached.approx()));
        }
        out.push_str(&limit_is);
        if let Some(path) = self.setting_path(registry) {
            let words = labels.and_then(|l| l.label(path)).map_or_else(
                || path.to_string(),
                |(section, field)| format!("{section} \u{2192} {field}"),
            );
            out.push_str(&format!(" Raise it in Settings \u{2192} {words}."));
            return out;
        }
        match registry.get(self.id).map(|row| row.remedy) {
            Some(Remedy::ShowMore) => out.push_str(" Show more."),
            Some(Remedy::Narrow) => out.push_str(" Narrow the query or the input."),
            Some(Remedy::Retry) => out.push_str(" Try again."),
            _ => {}
        }
        out
    }
}

impl fmt::Display for LimitExceeded {
    /// The message without a registry (no remedy sentence).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message(&Registry::new(&[])))
    }
}

impl std::error::Error for LimitExceeded {}
