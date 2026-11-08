//! The bounds of a size scan as registry rows (ADR 0019 row 61). A scan that reaches one stops and marks its result `capped`
//! (the sizes are then lower bounds); these constructors are the typed form for the caller that reports why.

use paddock_limits::{LimitExceeded, LimitRow, Quantity, Registry, Remedy, RowValue, Scope};

const fn millis_of(d: std::time::Duration) -> Quantity {
    let ms = d.as_millis();
    Quantity::Millis(if ms > u64::MAX as u128 {
        u64::MAX
    } else {
        ms as u64
    })
}

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
    millis_of(crate::walk::MAX_SCAN_TIME),
    Quantity::Millis(1),
    millis_of(crate::walk::MAX_SCAN_TIME),
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
    LimitExceeded::new(TIME.id, millis_of(limit), Scope::Query)
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
    fn each_row_is_the_constant_it_limits() {
        assert_eq!(ENTRIES.default, Quantity::Count(crate::walk::MAX_ENTRIES));
        assert_eq!(
            TIME.default,
            Quantity::Millis(crate::walk::MAX_SCAN_TIME.as_millis() as u64)
        );
        assert_eq!(
            DEPTH.default,
            Quantity::Count(crate::walk::MAX_DEPTH as u64)
        );
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
