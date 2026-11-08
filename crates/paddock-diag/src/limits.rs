//! The bounds of a diagnostic report as registry rows (ADR 0019 row 60). The report cuts what exceeds them and says so in
//! the file ([`crate::report`]'s truncation note); these constructors are the typed form for the caller that reports it.

use paddock_limits::{LimitExceeded, LimitRow, Quantity, Registry, Remedy, RowValue, Scope};

/// Files in one report (64).
pub const FILES: LimitRow = LimitRow::stop(
    "diag.files",
    Quantity::Count(crate::MAX_FILES as u64),
    Quantity::Count(1),
    Quantity::Count(crate::MAX_FILES as u64),
    RowValue::Hard,
    Remedy::None,
);
/// Bytes of one file in a report (4 MiB).
pub const FILE_BYTES: LimitRow = LimitRow::stop(
    "diag.file_bytes",
    Quantity::Bytes(crate::MAX_FILE_BYTES as u64),
    Quantity::Bytes(1),
    Quantity::Bytes(crate::MAX_FILE_BYTES as u64),
    RowValue::Hard,
    Remedy::None,
);
/// Bytes of the whole report (16 MiB).
pub const TOTAL_BYTES: LimitRow = LimitRow::stop(
    "diag.total_bytes",
    Quantity::Bytes(crate::MAX_TOTAL_BYTES as u64),
    Quantity::Bytes(1),
    Quantity::Bytes(crate::MAX_TOTAL_BYTES as u64),
    RowValue::Hard,
    Remedy::None,
);

static ROWS: [LimitRow; 3] = [FILES, FILE_BYTES, TOTAL_BYTES];

/// The rows of this crate.
pub const REGISTRY: Registry = Registry::new(&ROWS);

/// A report would hold more than `limit` files (`wanted` of them).
#[must_use]
pub fn files(limit: usize, wanted: usize) -> LimitExceeded {
    LimitExceeded::new(FILES.id, Quantity::Count(limit as u64), Scope::File)
        .reaching(Quantity::Count(wanted as u64))
        .unwrap_or(LimitExceeded::new(
            FILES.id,
            Quantity::Count(limit as u64),
            Scope::File,
        ))
}

/// One file of a report is longer than `limit` bytes (`len` of them).
#[must_use]
pub fn file_bytes(limit: usize, len: usize) -> LimitExceeded {
    LimitExceeded::new(FILE_BYTES.id, Quantity::Bytes(limit as u64), Scope::File)
        .reaching(Quantity::Bytes(len as u64))
        .unwrap_or(LimitExceeded::new(
            FILE_BYTES.id,
            Quantity::Bytes(limit as u64),
            Scope::File,
        ))
}

/// The report is longer than `limit` bytes in all.
#[must_use]
pub fn total_bytes(limit: usize) -> LimitExceeded {
    LimitExceeded::new(TOTAL_BYTES.id, Quantity::Bytes(limit as u64), Scope::File)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_testkit::limit_cases;

    limit_cases! { REGISTRY;
        too_many_files: "diag.files" => stop(got = files(crate::MAX_FILES, crate::MAX_FILES + 1), limit = Quantity::Count(crate::MAX_FILES as u64), setting = None),
        file_too_long: "diag.file_bytes" => stop(got = file_bytes(4 << 20, 5 << 20), limit = Quantity::Bytes(4 << 20), setting = None),
        report_too_long: "diag.total_bytes" => stop(got = total_bytes(16 << 20), limit = Quantity::Bytes(16 << 20), setting = None),
    }

    #[test]
    fn each_row_is_the_constant_it_limits() {
        assert_eq!(FILES.default, Quantity::Count(crate::MAX_FILES as u64));
        assert_eq!(
            FILE_BYTES.default,
            Quantity::Bytes(crate::MAX_FILE_BYTES as u64)
        );
        assert_eq!(
            TOTAL_BYTES.default,
            Quantity::Bytes(crate::MAX_TOTAL_BYTES as u64)
        );
    }

    #[test]
    fn the_registry_is_well_formed() {
        REGISTRY.check().unwrap();
    }
}
