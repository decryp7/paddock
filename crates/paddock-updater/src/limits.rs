//! The limits of an update as registry rows (ADR 0019 row 58), and the typed view of the error they cause.

use crate::{DEFAULT_MAX_DOWNLOAD_BYTES, UpdateError};
use paddock_limits::{LimitExceeded, LimitRow, Quantity, Registry, Remedy, RowValue, Scope};

/// The most bytes an update may be (512 MiB).
pub const DOWNLOAD: LimitRow = LimitRow::stop(
    "updater.download_bytes",
    Quantity::Bytes(DEFAULT_MAX_DOWNLOAD_BYTES),
    Quantity::Bytes(1),
    Quantity::Bytes(u64::MAX),
    RowValue::ByRequest,
    Remedy::None,
);

static ROWS: [LimitRow; 1] = [DOWNLOAD];

/// The rows of this crate.
pub const REGISTRY: Registry = Registry::new(&ROWS);

impl UpdateError {
    /// The limit this error is, when it is one.
    #[must_use]
    pub fn limit(&self) -> Option<LimitExceeded> {
        match *self {
            UpdateError::TooLarge { limit } => Some(LimitExceeded::new(
                DOWNLOAD.id,
                Quantity::Bytes(limit),
                Scope::File,
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
        update_over_the_limit: "updater.download_bytes" => stop(
            got = UpdateError::TooLarge { limit: DEFAULT_MAX_DOWNLOAD_BYTES }.limit().unwrap(),
            limit = Quantity::Bytes(DEFAULT_MAX_DOWNLOAD_BYTES),
            setting = None
        ),
    }

    #[test]
    fn the_registry_is_well_formed() {
        REGISTRY.check().unwrap();
        assert_eq!(
            UpdateError::DiskFull {
                detail: String::new()
            }
            .limit(),
            None
        );
    }
}
