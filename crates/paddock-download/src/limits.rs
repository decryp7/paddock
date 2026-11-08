//! The limits of a download as registry rows (ADR 0019 row 57), and the typed view of the error they cause.

use crate::{DEFAULT_FETCH_BYTES, DEFAULT_MAX_BYTES, DownloadError};
use paddock_limits::{LimitExceeded, LimitRow, Quantity, Registry, Remedy, RowValue, Scope};

/// The most bytes one download may be (64 GiB, [`Config::max_bytes`](crate::Config::max_bytes), unless the request sets its own).
pub const SIZE: LimitRow = LimitRow::stop(
    "download.size",
    Quantity::Bytes(DEFAULT_MAX_BYTES),
    Quantity::Bytes(1),
    Quantity::Bytes(u64::MAX),
    RowValue::ByRequest,
    Remedy::Narrow,
);
/// The most body bytes a small GET reads (16 MiB, [`Fetch::max_bytes`](crate::Fetch::max_bytes), unless the caller sets its own).
pub const FETCH_SIZE: LimitRow = LimitRow::stop(
    "download.fetch_size",
    Quantity::Bytes(DEFAULT_FETCH_BYTES),
    Quantity::Bytes(1),
    Quantity::Bytes(u64::MAX),
    RowValue::ByRequest,
    Remedy::Narrow,
);

static ROWS: [LimitRow; 2] = [SIZE, FETCH_SIZE];

/// The rows of this crate.
pub const REGISTRY: Registry = Registry::new(&ROWS);

impl DownloadError {
    /// The limit this error is, when it is one: `reached` is the size the server announced or the bytes read.
    #[must_use]
    pub fn limit(&self) -> Option<LimitExceeded> {
        match *self {
            DownloadError::TooLarge { size, limit } => {
                LimitExceeded::new(SIZE.id, Quantity::Bytes(limit), Scope::File)
                    .reaching(Quantity::Bytes(size))
                    .ok()
            }
            DownloadError::ReplyTooLarge { size, limit } => {
                LimitExceeded::new(FETCH_SIZE.id, Quantity::Bytes(limit), Scope::Query)
                    .reaching(Quantity::Bytes(size))
                    .ok()
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, Fetch};
    use paddock_testkit::limit_cases;

    limit_cases! { REGISTRY;
        download_over_the_limit: "download.size" => stop(
            got = DownloadError::TooLarge { size: 600 << 20, limit: 512 << 20 }.limit().unwrap(),
            limit = Quantity::Bytes(512 << 20),
            setting = None
        ),
        fetch_over_the_limit: "download.fetch_size" => stop(
            got = DownloadError::ReplyTooLarge { size: 20 << 20, limit: 16 << 20 }.limit().unwrap(),
            limit = Quantity::Bytes(16 << 20),
            setting = None
        ),
    }

    #[test]
    fn each_row_is_the_default_of_what_it_limits() {
        assert_eq!(SIZE.default, Quantity::Bytes(Config::default().max_bytes));
        assert_eq!(
            FETCH_SIZE.default,
            Quantity::Bytes(Fetch::default().max_bytes)
        );
    }

    #[test]
    fn an_error_that_is_not_a_size_limit_is_no_limit() {
        for err in [
            DownloadError::DiskFull,
            DownloadError::Throttled { host: "h".into() },
            DownloadError::SizeMismatch {
                expected: 2,
                actual: 1,
            },
        ] {
            assert!(err.limit().is_none(), "{err:?}");
        }
    }

    #[test]
    fn the_registry_is_well_formed() {
        REGISTRY.check().unwrap();
    }
}
