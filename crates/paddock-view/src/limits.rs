//! The limits of a view document as registry rows (ADR 0019 row 59), and the typed view of the errors they cause.

use crate::{MAX_DEPTH, MAX_ENCODED, MAX_INLINE_ROWS, MAX_NODES, MAX_STRING, ViewError};
use paddock_limits::{LimitExceeded, LimitRow, Quantity, Registry, Remedy, RowValue, Scope};

const fn hard(id: &'static str, default: Quantity) -> LimitRow {
    LimitRow::stop(id, default, default, default, RowValue::Hard, Remedy::None)
}

/// The encoded document, in bytes (1 MiB).
pub const ENCODED: LimitRow = hard("view.encoded_bytes", Quantity::Bytes(MAX_ENCODED as u64));
/// Nodes in one document (20,000).
pub const NODES: LimitRow = hard("view.nodes", Quantity::Count(MAX_NODES as u64));
/// Inline rows of one table (2,000; more is `Paged`).
pub const INLINE_ROWS: LimitRow = hard("view.inline_rows", Quantity::Count(MAX_INLINE_ROWS as u64));
/// One string, in bytes (8 KiB).
pub const STRING: LimitRow = hard("view.string_bytes", Quantity::Bytes(MAX_STRING as u64));
/// Nesting depth (16).
pub const DEPTH: LimitRow = hard("view.depth", Quantity::Count(MAX_DEPTH as u64));

static ROWS: [LimitRow; 5] = [ENCODED, NODES, INLINE_ROWS, STRING, DEPTH];

/// The rows of this crate.
pub const REGISTRY: Registry = Registry::new(&ROWS);

impl ViewError {
    /// The limit this error is, when it is one (`reached` where the error knows it). A plugin's document is the query.
    #[must_use]
    pub fn limit(&self) -> Option<LimitExceeded> {
        let make = |row: &LimitRow, reached: Option<Quantity>| {
            let base = LimitExceeded::new(row.id, row.default, Scope::Plugin);
            reached.and_then(|r| base.reaching(r).ok()).unwrap_or(base)
        };
        match *self {
            ViewError::TooLarge { bytes } => {
                Some(make(&ENCODED, Some(Quantity::Bytes(bytes as u64))))
            }
            ViewError::TooManyNodes { .. } => Some(make(&NODES, None)),
            ViewError::TooManyRows { rows, .. } => {
                Some(make(&INLINE_ROWS, Some(Quantity::Count(rows as u64))))
            }
            ViewError::StringTooLong { len, .. } => {
                Some(make(&STRING, Some(Quantity::Bytes(len as u64))))
            }
            ViewError::TooDeep { .. } => Some(make(&DEPTH, None)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_testkit::limit_cases;

    fn at(path: &str) -> String {
        path.to_owned()
    }

    limit_cases! { REGISTRY;
        encoded_over_the_limit: "view.encoded_bytes" => stop(got = ViewError::TooLarge { bytes: MAX_ENCODED + 1 }.limit().unwrap(), limit = Quantity::Bytes(MAX_ENCODED as u64), setting = None),
        too_many_nodes: "view.nodes" => stop(got = ViewError::TooManyNodes { path: at("root") }.limit().unwrap(), limit = Quantity::Count(MAX_NODES as u64), setting = None),
        too_many_rows: "view.inline_rows" => stop(got = ViewError::TooManyRows { path: at("root.rows"), rows: MAX_INLINE_ROWS + 1 }.limit().unwrap(), limit = Quantity::Count(MAX_INLINE_ROWS as u64), setting = None),
        string_too_long: "view.string_bytes" => stop(got = ViewError::StringTooLong { path: at("root.text"), len: MAX_STRING + 1 }.limit().unwrap(), limit = Quantity::Bytes(MAX_STRING as u64), setting = None),
        too_deep: "view.depth" => stop(got = ViewError::TooDeep { path: at("root") }.limit().unwrap(), limit = Quantity::Count(MAX_DEPTH as u64), setting = None),
    }

    #[test]
    fn the_registry_is_well_formed_and_a_schema_error_is_no_limit() {
        REGISTRY.check().unwrap();
        assert_eq!(
            ViewError::UnsupportedSchema {
                found: 9,
                supported: 1
            }
            .limit(),
            None
        );
    }
}
