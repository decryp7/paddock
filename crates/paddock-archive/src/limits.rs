//! The limits of unpacking an archive (ADR 0019 row 55): the registry, and the typed error a limit gives.
//!
//! The caller sets the values ([`crate::Limits`]), so none of these rows is a Setting here; an app that exposes one maps it
//! in its own registry. A limit that stops an unpack is a [`paddock_limits::LimitExceeded`] inside [`ArchiveError::Limit`],
//! and the job it ran in keeps it ([`paddock_jobs::JobError::Limit`]).

use crate::ArchiveError;
use paddock_limits::{
    LimitExceeded, LimitId, LimitRow, Quantity, Registry, Remedy, RowKind, RowValue, Scope, Unit,
};

const fn row(id: &'static str, unit: Unit, default: Quantity, max: Quantity) -> LimitRow {
    let min = match unit {
        Unit::Bytes => Quantity::Bytes(1),
        Unit::Ratio => Quantity::Ratio(1),
        _ => Quantity::Count(1),
    };
    LimitRow {
        id: LimitId::new(id),
        unit,
        default,
        min,
        max,
        value: RowValue::ByRequest,
        kind: RowKind::Stop,
        selections: &[],
        remedy: Remedy::Narrow,
    }
}

/// Entries in one archive.
pub const ENTRIES: LimitRow = row(
    "archive.entries",
    Unit::Count,
    Quantity::Count(10_000),
    Quantity::Count(u64::MAX),
);
/// Bytes of one file.
pub const FILE_BYTES: LimitRow = row(
    "archive.file_bytes",
    Unit::Bytes,
    Quantity::Bytes(4 << 30),
    Quantity::Bytes(u64::MAX),
);
/// Bytes of everything unpacked.
pub const TOTAL_BYTES: LimitRow = row(
    "archive.total_bytes",
    Unit::Bytes,
    Quantity::Bytes(8 << 30),
    Quantity::Bytes(u64::MAX),
);
/// How much larger the output may be than the archive.
pub const RATIO: LimitRow = row(
    "archive.ratio",
    Unit::Ratio,
    Quantity::Ratio(200),
    Quantity::Ratio(u32::MAX),
);
/// Folders deep.
pub const DEPTH: LimitRow = row(
    "archive.depth",
    Unit::Count,
    Quantity::Count(32),
    Quantity::Count(u64::MAX),
);
/// Characters of a whole path.
pub const PATH_LENGTH: LimitRow = row(
    "archive.path_length",
    Unit::Count,
    Quantity::Count(1024),
    Quantity::Count(u64::MAX),
);

static ROWS: [LimitRow; 6] = [ENTRIES, FILE_BYTES, TOTAL_BYTES, RATIO, DEPTH, PATH_LENGTH];

/// The rows of this crate.
pub const REGISTRY: Registry = Registry::new(&ROWS);

fn exceeded(row: &LimitRow, limit: Quantity) -> ArchiveError {
    ArchiveError::Limit(LimitExceeded::new(row.id, limit, Scope::File))
}

pub(crate) fn entries(limit: u64) -> ArchiveError {
    exceeded(&ENTRIES, Quantity::Count(limit))
}

pub(crate) fn file_bytes(limit: u64) -> ArchiveError {
    exceeded(&FILE_BYTES, Quantity::Bytes(limit))
}

pub(crate) fn total_bytes(limit: u64) -> ArchiveError {
    exceeded(&TOTAL_BYTES, Quantity::Bytes(limit))
}

pub(crate) fn ratio(limit: u64) -> ArchiveError {
    exceeded(
        &RATIO,
        Quantity::Ratio(u32::try_from(limit).unwrap_or(u32::MAX)),
    )
}

pub(crate) fn depth(limit: u64) -> ArchiveError {
    exceeded(&DEPTH, Quantity::Count(limit))
}

pub(crate) fn path_length(limit: u64) -> ArchiveError {
    exceeded(&PATH_LENGTH, Quantity::Count(limit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_testkit::limit_cases;

    fn limit(e: ArchiveError) -> LimitExceeded {
        match e {
            ArchiveError::Limit(l) => l,
            other => panic!("not a limit: {other:?}"),
        }
    }

    limit_cases! { REGISTRY;
        too_many_entries: "archive.entries" => stop(got = limit(entries(10_000)), limit = Quantity::Count(10_000), setting = None),
        a_file_over_the_limit: "archive.file_bytes" => stop(got = limit(file_bytes(4 << 30)), limit = Quantity::Bytes(4 << 30), setting = None),
        everything_over_the_limit: "archive.total_bytes" => stop(got = limit(total_bytes(8 << 30)), limit = Quantity::Bytes(8 << 30), setting = None),
        an_expansion_over_the_ratio: "archive.ratio" => stop(got = limit(ratio(200)), limit = Quantity::Ratio(200), setting = None),
        folders_too_deep: "archive.depth" => stop(got = limit(depth(32)), limit = Quantity::Count(32), setting = None),
        a_path_too_long: "archive.path_length" => stop(got = limit(path_length(1024)), limit = Quantity::Count(1024), setting = None),
    }

    #[test]
    fn the_registry_is_well_formed() {
        REGISTRY.check().unwrap();
    }
}
