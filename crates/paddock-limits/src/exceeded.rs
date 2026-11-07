//! The typed error of a limit that stopped work.

use crate::id::{LimitId, Quantity, Scope};
use crate::row::{Registry, Remedy, RowValue};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A limit stopped the work. Every layer between the check and the person keeps this type (ADR 0019 decision 1); none
/// turns it into a `String`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    #[must_use]
    pub const fn reaching(mut self, reached: Quantity) -> Self {
        self.reached = Some(reached);
        self
    }

    /// The message, generated from the type and its registry row (ADR 0019 decision 3): what stopped, the observed value
    /// when known, the limit and its value, and what to do. For logs, the AI and as the fallback text; the UI's words are
    /// its own messages keyed by [`LimitId`]. A row missing from the registry still gives the first two parts.
    #[must_use]
    pub fn message(&self, registry: &Registry) -> String {
        let mut out = format!("The {} stopped: ", self.scope.word());
        if let Some(reached) = self.reached {
            out.push_str(&format!("it reached {reached}; "));
        }
        out.push_str(&format!("the limit ({}) is {}.", self.id, self.limit));
        let remedy = registry.get(self.id).map(|row| (row.remedy, row.value));
        match remedy {
            Some((Remedy::RaiseSetting { path }, _)) | Some((_, RowValue::Setting { path })) => {
                out.push_str(&format!(" Raise it in Settings \u{2192} {path}."));
            }
            Some((Remedy::ShowMore, _)) => out.push_str(" Show more."),
            Some((Remedy::Narrow, _)) => out.push_str(" Narrow the query or the input."),
            Some((Remedy::Retry, _)) => out.push_str(" Try again."),
            Some((Remedy::None, _)) | None => {}
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
