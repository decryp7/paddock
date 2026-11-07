//! The quarantine as registry rows (ADR 0019 row 54): a plugin that fails `max` times within `window` is switched off. The
//! status is the quarantine's own; these constructors give the limit as a type for the caller that explains it.

use crate::quarantine::QuarantinePolicy;
use paddock_limits::{LimitExceeded, LimitRow, Quantity, Registry, Remedy, RowValue, Scope};

/// Failures within the window that start a quarantine (3).
pub const FAILURES: LimitRow = LimitRow::stop(
    "guard.quarantine_failures",
    Quantity::Count(3),
    Quantity::Count(1),
    Quantity::Count(u64::MAX),
    RowValue::ByRequest,
    Remedy::None,
);
/// How far back failures count (600 s).
pub const WINDOW: LimitRow = LimitRow::stop(
    "guard.quarantine_window",
    Quantity::Millis(600_000),
    Quantity::Millis(1),
    Quantity::Millis(u64::MAX),
    RowValue::ByRequest,
    Remedy::None,
);

static ROWS: [LimitRow; 2] = [FAILURES, WINDOW];

/// The rows of this crate.
pub const REGISTRY: Registry = Registry::new(&ROWS);

/// The failures that switched it off: `policy.max` within `policy.window`.
#[must_use]
pub fn quarantine(policy: &QuarantinePolicy, failures: u32) -> LimitExceeded {
    let base = LimitExceeded::new(
        FAILURES.id,
        Quantity::Count(u64::from(policy.max)),
        Scope::Plugin,
    );
    base.reaching(Quantity::Count(u64::from(failures)))
        .unwrap_or(base)
}

/// The window the failures were counted in.
#[must_use]
pub fn window(policy: &QuarantinePolicy) -> LimitExceeded {
    LimitExceeded::new(
        WINDOW.id,
        Quantity::Millis(u64::try_from(policy.window.as_millis()).unwrap_or(u64::MAX)),
        Scope::Plugin,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_testkit::limit_cases;

    limit_cases! { REGISTRY;
        failures_case: "guard.quarantine_failures" => stop(got = quarantine(&QuarantinePolicy::default(), 3), limit = Quantity::Count(3), setting = None),
        window_case: "guard.quarantine_window" => stop(got = window(&QuarantinePolicy::default()), limit = Quantity::Millis(600_000), setting = None),
    }

    #[test]
    fn each_row_is_the_default_of_the_policy() {
        let p = QuarantinePolicy::default();
        assert_eq!(FAILURES.default, Quantity::Count(u64::from(p.max)));
        assert_eq!(
            WINDOW.default,
            Quantity::Millis(p.window.as_millis() as u64)
        );
    }

    #[test]
    fn the_registry_is_well_formed() {
        REGISTRY.check().unwrap();
    }
}
