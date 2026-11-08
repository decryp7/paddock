//! The limits of an outgoing request as registry rows (ADR 0019 row 56), and the typed view of the errors they cause.

use crate::{DEFAULT_REDIRECTS, MAX_REDIRECTS, NetError};
use paddock_limits::{LimitExceeded, LimitRow, Quantity, Registry, Remedy, RowValue, Scope};

/// The most body bytes read from an answer (8 MiB unless the policy says otherwise).
pub const BODY: LimitRow = LimitRow::stop(
    "net.body_bytes",
    Quantity::Bytes(8 << 20),
    Quantity::Bytes(1),
    Quantity::Bytes(u64::MAX),
    RowValue::ByRequest,
    Remedy::Narrow,
);
/// The most redirects followed (5 unless the policy says otherwise, at most 10).
pub const REDIRECTS: LimitRow = LimitRow::stop(
    "net.redirects",
    Quantity::Count(DEFAULT_REDIRECTS as u64),
    Quantity::Count(0),
    Quantity::Count(MAX_REDIRECTS as u64),
    RowValue::ByRequest,
    Remedy::None,
);

static ROWS: [LimitRow; 2] = [BODY, REDIRECTS];

/// The rows of this crate.
pub const REGISTRY: Registry = Registry::new(&ROWS);

impl NetError {
    /// The limit this error is, when it is one.
    #[must_use]
    pub fn limit(&self) -> Option<LimitExceeded> {
        match self {
            NetError::TooLarge(limit) => Some(LimitExceeded::new(
                BODY.id,
                Quantity::Bytes(*limit),
                Scope::Query,
            )),
            NetError::TooManyRedirects(allowed) => Some(LimitExceeded::new(
                REDIRECTS.id,
                Quantity::Count(u64::from(*allowed)),
                Scope::Query,
            )),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_testkit::limit_cases;

    limit_cases! { REGISTRY;
        body_over_the_limit: "net.body_bytes" => stop(got = NetError::TooLarge(8 << 20).limit().unwrap(), limit = Quantity::Bytes(8 << 20), setting = None),
        too_many_redirects: "net.redirects" => stop(got = NetError::TooManyRedirects(DEFAULT_REDIRECTS).limit().unwrap(), limit = Quantity::Count(5), setting = None),
    }

    #[test]
    fn the_body_row_is_the_default_of_the_policy() {
        assert_eq!(
            BODY.default,
            Quantity::Bytes(crate::Policy::default().max_body)
        );
    }

    #[test]
    fn a_lowered_policy_names_its_own_number_and_the_row_is_the_default() {
        assert_eq!(
            NetError::TooManyRedirects(3).limit().unwrap().limit,
            Quantity::Count(3)
        );
        assert_eq!(
            REDIRECTS.default,
            Quantity::Count(u64::from(crate::Policy::default().max_redirects))
        );
        assert_eq!(REDIRECTS.max, Quantity::Count(u64::from(MAX_REDIRECTS)));
    }

    #[test]
    fn the_registry_is_well_formed_and_other_errors_are_no_limit() {
        REGISTRY.check().unwrap();
        assert_eq!(NetError::Cancelled.limit(), None);
    }
}
