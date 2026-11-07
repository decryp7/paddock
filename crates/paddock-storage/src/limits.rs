//! The bounds of a size scan as registry rows (ADR 0019 row 61). A scan that reaches one stops and marks its result `capped`
//! (the sizes are then lower bounds); these constructors are the typed form for the caller that reports why.

use paddock_limits::{LimitExceeded, LimitRow, Quantity, Registry, Remedy, RowValue, Scope};

/// Entries one scan reads (500,000).
pub const ENTRIES: LimitRow = LimitRow::stop(
    "storage.walk_entries",
    Quantity::Count(crate::walk::MAX_ENTRIES),
    Quantity::Count(1),
    Quantity::Count(crate::walk::MAX_ENTRIES),
    RowValue::Hard,
    Remedy::Narrow,
);
/// Time one scan may take (10 s).
pub const TIME: LimitRow = LimitRow::stop(
    "storage.walk_time",
    Quantity::Millis(10_000),
    Quantity::Millis(1),
    Quantity::Millis(10_000),
    RowValue::Hard,
    Remedy::Narrow,
);
/// Folder depth one scan follows (64).
pub const DEPTH: LimitRow = LimitRow::stop(
    "storage.walk_depth",
    Quantity::Count(crate::walk::MAX_DEPTH as u64),
    Quantity::Count(1),
    Quantity::Count(crate::walk::MAX_DEPTH as u64),
    RowValue::Hard,
    Remedy::Narrow,
);

static ROWS: [LimitRow; 3] = [ENTRIES, TIME, DEPTH];

/// The rows of this crate.
pub const REGISTRY: Registry = Registry::new(&ROWS);

/// The scan read `limit` entries and stopped.
#[must_use]
pub fn entries(limit: u64) -> LimitExceeded {
    LimitExceeded::new(ENTRIES.id, Quantity::Count(limit), Scope::Query)
}

/// The scan ran for `limit` and stopped.
#[must_use]
pub fn time(limit: std::time::Duration) -> LimitExceeded {
    LimitExceeded::new(
        TIME.id,
        Quantity::Millis(u64::try_from(limit.as_millis()).unwrap_or(u64::MAX)),
        Scope::Query,
    )
}

/// The scan reached `limit` folders deep and stopped.
#[must_use]
pub fn depth(limit: usize) -> LimitExceeded {
    LimitExceeded::new(DEPTH.id, Quantity::Count(limit as u64), Scope::Query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_testkit::limit_cases;

    limit_cases! { REGISTRY;
        entries_case: "storage.walk_entries" => stop(got = entries(crate::walk::MAX_ENTRIES), limit = Quantity::Count(crate::walk::MAX_ENTRIES), setting = None),
        time_case: "storage.walk_time" => stop(got = time(crate::walk::MAX_SCAN_TIME), limit = Quantity::Millis(10_000), setting = None),
        depth_case: "storage.walk_depth" => stop(got = depth(crate::walk::MAX_DEPTH), limit = Quantity::Count(crate::walk::MAX_DEPTH as u64), setting = None),
    }

    #[test]
    fn the_registry_is_well_formed_and_a_huge_time_saturates() {
        REGISTRY.check().unwrap();
        assert_eq!(
            time(std::time::Duration::MAX).limit,
            Quantity::Millis(u64::MAX)
        );
    }
}
