//! The limits of running a plugin as registry rows (ADR 0019 rows 52 and 53), and the errors they cause.
//!
//! A frame over the connection's limit is `paddock_ipc::limit_rows::FRAME`; here are the plugin's own: the memory it is
//! given, the time a call has, the grace after a cancel and the time it has to start. Each constructor builds the
//! [`LimitExceeded`] a layer above keeps ([`crate::error::HostError::Limit`]).

use crate::config::{DEFAULT_CALL_TIMEOUT, DEFAULT_HANG_GRACE, DEFAULT_START_TIMEOUT};
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
/// The time one call has before it is cancelled (30 s by default, the app's setting). No maximum: the host takes any duration the
/// app sets, so the row names none.
pub const CALL_DEADLINE: LimitRow = row(
    "plugin.call_deadline",
    millis_of(DEFAULT_CALL_TIMEOUT),
    Quantity::Millis(1),
    Quantity::Millis(u64::MAX),
);
/// The time a plugin has to end a call after the cancel (5 s by default). No maximum, like the call deadline.
pub const HANG_GRACE: LimitRow = row(
    "plugin.hang_grace",
    millis_of(DEFAULT_HANG_GRACE),
    Quantity::Millis(1),
    Quantity::Millis(u64::MAX),
);
/// The time a plugin has to start and answer its first request (15 s by default). No maximum, like the call deadline.
pub const START: LimitRow = row(
    "plugin.start",
    millis_of(DEFAULT_START_TIMEOUT),
    Quantity::Millis(1),
    Quantity::Millis(u64::MAX),
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

const fn millis_of(d: Duration) -> Quantity {
    let ms = d.as_millis();
    Quantity::Millis(if ms > u64::MAX as u128 {
        u64::MAX
    } else {
        ms as u64
    })
}

fn millis(d: Duration) -> Quantity {
    millis_of(d)
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

    /// The registry builds its rows in constants, where no coverage is measured: build each unit's row once at run time.
    #[test]
    fn a_row_takes_its_unit_from_its_default() {
        let bytes = row(
            "plugin.example",
            Quantity::Bytes(2),
            Quantity::Bytes(1),
            Quantity::Bytes(3),
        );
        assert_eq!(
            (bytes.unit, bytes.default, bytes.min, bytes.max),
            (
                Unit::Bytes,
                Quantity::Bytes(2),
                Quantity::Bytes(1),
                Quantity::Bytes(3)
            )
        );
        let millis = row(
            "plugin.example",
            Quantity::Millis(2),
            Quantity::Millis(1),
            Quantity::Millis(3),
        );
        assert_eq!(millis.unit, Unit::Millis);
        assert_eq!(millis.id, LimitId::new("plugin.example"));
        assert_eq!(
            (millis.value, millis.kind, millis.remedy),
            (RowValue::ByRequest, RowKind::Stop, Remedy::None)
        );
        assert!(millis.selections.is_empty());
    }

    #[test]
    fn a_duration_in_milliseconds_saturates_at_the_largest_count() {
        use std::time::Duration;
        assert_eq!(
            millis_of(Duration::from_millis(1500)),
            Quantity::Millis(1500)
        );
        // More milliseconds than a `u64` holds (Duration::MAX truncates to u64::MAX by chance, so it would not tell).
        assert_eq!(
            millis_of(Duration::from_secs(u64::MAX / 500)),
            Quantity::Millis(u64::MAX)
        );
    }
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
    fn each_row_is_the_default_of_what_it_limits() {
        use crate::HostConfig;
        use paddock_guard::{Quarantine, QuarantinePolicy};
        use paddock_jobs::time::{Clock, FakeClock};
        use paddock_plugin_api::compat::HostInfo;
        use std::sync::Arc;
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::new());
        let quarantine = Arc::new(Quarantine::new(QuarantinePolicy::default(), clock.clone()));
        let cfg = HostConfig::new(
            HostInfo::new("TestApp"),
            clock,
            quarantine,
            std::env::temp_dir(),
        );
        assert_eq!(CALL_DEADLINE.default, millis(cfg.call_timeout));
        assert_eq!(HANG_GRACE.default, millis(cfg.hang_grace));
        assert_eq!(START.default, millis(cfg.start_timeout));
        assert_eq!(
            MEMORY.default,
            Quantity::Bytes(crate::grants::SandboxSpec::locked().memory_limit)
        );
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
