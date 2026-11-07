//! The limits of a download as registry rows (ADR 0019 row 57), and the typed view of the error they cause.

use crate::DownloadError;
use paddock_limits::{LimitExceeded, LimitRow, Quantity, Registry, Remedy, RowValue, Scope};

/// The most bytes one download may be (512 MiB unless the caller says otherwise).
pub const SIZE: LimitRow = LimitRow::stop(
    "download.size",
    Quantity::Bytes(512 << 20),
    Quantity::Bytes(1),
    Quantity::Bytes(u64::MAX),
    RowValue::ByRequest,
    Remedy::Narrow,
);

static ROWS: [LimitRow; 1] = [SIZE];

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
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_testkit::limit_cases;

    limit_cases! { REGISTRY;
        download_over_the_limit: "download.size" => stop(
            got = DownloadError::TooLarge { size: 600 << 20, limit: 512 << 20 }.limit().unwrap(),
            limit = Quantity::Bytes(512 << 20),
            setting = None
        ),
    }

    #[test]
    fn the_registry_is_well_formed() {
        REGISTRY.check().unwrap();
    }
}
