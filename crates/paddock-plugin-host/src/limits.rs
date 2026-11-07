//! The limits of running a plugin as registry rows (ADR 0019 rows 52 and 53), and the errors they cause.
//!
//! A frame over the connection's limit is `paddock_ipc::limit_rows::FRAME`; here are the plugin's own: the memory it is
//! given, the time a call has, the grace after a cancel and the time it has to start. Each constructor builds the
//! [`LimitExceeded`] a layer above keeps ([`crate::error::HostError::Limit`]).

use paddock_limits::{
    LimitExceeded, LimitId, LimitRow, Quantity, Registry, Remedy, RowKind, RowValue, Scope, Unit,
};
use std::time::Duration;

const fn row(id: &'static str, default: Quantity, min: Quantity, max: Quantity) -> LimitRow {
    LimitRow {
        id: LimitId::new(id),
        unit: match default {
            Quantity::Bytes(_) => Unit::Bytes,
            _ => Unit::Millis,
        },
        default,
        min,
        max,
        value: RowValue::ByRequest,
        kind: RowKind::Stop,
        selections: &[],
        remedy: Remedy::None,
    }
}

/// Memory a plugin may use: 1 GiB unless it asks for more or less, 64 MiB to 4 GiB whatever it asks.
pub const MEMORY: LimitRow = row(
    "plugin.memory",
    Quantity::Bytes(crate::grants::DEFAULT_MEMORY),
    Quantity::Bytes(crate::grants::MIN_MEMORY),
    Quantity::Bytes(crate::grants::MAX_MEMORY),
);
/// The time one call has before it is cancelled (30 s by default, the app's setting).
pub const CALL_DEADLINE: LimitRow = row(
    "plugin.call_deadline",
    Quantity::Millis(30_000),
    Quantity::Millis(1),
    Quantity::Millis(3_600_000),
);
/// The time a plugin has to end a call after the cancel (5 s by default).
pub const HANG_GRACE: LimitRow = row(
    "plugin.hang_grace",
    Quantity::Millis(5_000),
    Quantity::Millis(1),
    Quantity::Millis(600_000),
);
/// The time a plugin has to start and answer its first request (15 s by default).
pub const START: LimitRow = row(
    "plugin.start",
    Quantity::Millis(15_000),
    Quantity::Millis(1),
    Quantity::Millis(600_000),
);

static ROWS: [LimitRow; 4] = [MEMORY, CALL_DEADLINE, HANG_GRACE, START];

/// The rows of this crate.
pub const REGISTRY: Registry = Registry::new(&ROWS);

/// The registry that knows `id`: this crate's, or the IPC's for `ipc.*`.
#[must_use]
pub fn registry_of(id: LimitId) -> Registry {
    if id.as_str().starts_with("ipc.") {
        paddock_ipc::limit_rows::REGISTRY
    } else {
        REGISTRY
    }
}

fn millis(d: Duration) -> Quantity {
    Quantity::Millis(u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The plugin used more memory than `limit` bytes (`used` when known).
#[must_use]
pub fn memory(limit: u64, used: Option<u64>) -> LimitExceeded {
    let base = LimitExceeded::new(MEMORY.id, Quantity::Bytes(limit), Scope::Plugin);
    used.and_then(|u| base.reaching(Quantity::Bytes(u)).ok())
        .unwrap_or(base)
}

/// A call overran `limit` and the plugin ignored the cancel.
#[must_use]
pub fn call_deadline(limit: Duration) -> LimitExceeded {
    LimitExceeded::new(CALL_DEADLINE.id, millis(limit), Scope::Plugin)
}

/// The plugin did not end the call within `limit` of the cancel.
#[must_use]
pub fn hang_grace(limit: Duration) -> LimitExceeded {
    LimitExceeded::new(HANG_GRACE.id, millis(limit), Scope::Plugin)
}

/// The plugin did not start within `limit`.
#[must_use]
pub fn start(limit: Duration) -> LimitExceeded {
    LimitExceeded::new(START.id, millis(limit), Scope::Plugin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_testkit::limit_cases;

    limit_cases! { REGISTRY;
        memory_cap: "plugin.memory" => stop(
            got = memory(64 << 20, Some(70 << 20)),
            limit = Quantity::Bytes(64 << 20),
            setting = None
        ),
        call_deadline_case: "plugin.call_deadline" => stop(
            got = call_deadline(Duration::from_secs(30)),
            limit = Quantity::Millis(30_000),
            setting = None
        ),
        hang_grace_case: "plugin.hang_grace" => stop(
            got = hang_grace(Duration::from_secs(5)),
            limit = Quantity::Millis(5_000),
            setting = None
        ),
        start_window: "plugin.start" => stop(
            got = start(Duration::from_secs(15)),
            limit = Quantity::Millis(15_000),
            setting = None
        ),
    }

    #[test]
    fn the_registry_is_well_formed_and_finds_its_rows() {
        REGISTRY.check().unwrap();
        assert_eq!(registry_of(LimitId::new("plugin.start")).rows().len(), 4);
        assert_eq!(registry_of(LimitId::new("ipc.frame")).rows().len(), 1);
    }

    #[test]
    fn a_memory_limit_without_a_measured_value_says_only_the_limit() {
        assert_eq!(memory(1 << 30, None).reached, None);
        assert_eq!(
            memory(1 << 30, Some(2 << 30)).reached,
            Some(Quantity::Bytes(2 << 30))
        );
        // A duration too long for a u64 of milliseconds saturates.
        assert_eq!(
            call_deadline(Duration::MAX).limit,
            Quantity::Millis(u64::MAX)
        );
    }
}
