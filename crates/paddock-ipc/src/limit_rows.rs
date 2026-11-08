//! The limits of the IPC as registry rows (ADR 0019 row 51), and the typed view of the errors they cause.
//!
//! The frame size is the one limit a caller can hit: [`IpcError::TooLarge`] (our request) and [`IpcError::ReplyTooLarge`]
//! (the other side's answer) both carry numbers, and [`IpcError::limit`] gives them as a [`LimitExceeded`] so a layer above
//! keeps the type (a plugin host's error, a job's failure, a health cause) instead of the text.

use crate::error::IpcError;
use crate::frame::{DEFAULT_MAX_FRAME, HARD_MAX_FRAME, MIN_FRAME};
use paddock_limits::{
    LimitExceeded, LimitId, LimitRow, Quantity, Registry, Remedy, RowKind, RowValue, Scope, Unit,
};

/// The largest frame a connection carries: 16 MiB by default, 64 MiB at most.
pub const FRAME: LimitRow = LimitRow {
    id: LimitId::new("ipc.frame"),
    unit: Unit::Bytes,
    default: Quantity::Bytes(DEFAULT_MAX_FRAME as u64),
    min: Quantity::Bytes(MIN_FRAME as u64),
    max: Quantity::Bytes(HARD_MAX_FRAME as u64),
    value: RowValue::Hard,
    kind: RowKind::Stop,
    selections: &[],
    remedy: Remedy::Narrow,
};

static ROWS: [LimitRow; 1] = [FRAME];

/// The rows of this crate.
pub const REGISTRY: Registry = Registry::new(&ROWS);

impl IpcError {
    /// The limit this error is, when it is one: a frame over the connection's limit, in either direction (`reached` is the
    /// frame's length, `limit` the connection's maximum).
    #[must_use]
    pub fn limit(&self) -> Option<LimitExceeded> {
        match *self {
            IpcError::TooLarge { len, max } | IpcError::ReplyTooLarge { len, max } => {
                LimitExceeded::new(FRAME.id, Quantity::Bytes(max as u64), Scope::Process)
                    .reaching(Quantity::Bytes(len as u64))
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
        frame_over_the_limit: "ipc.frame" => stop(
            got = IpcError::TooLarge { len: 5000, max: 1024 }.limit().unwrap(),
            limit = Quantity::Bytes(1024),
            setting = None
        ),
    }

    #[test]
    fn the_registry_is_well_formed() {
        REGISTRY.check().unwrap();
    }

    #[test]
    fn both_directions_and_only_they_are_a_limit() {
        let ours = IpcError::TooLarge { len: 9, max: 5 }.limit().unwrap();
        let theirs = IpcError::ReplyTooLarge { len: 9, max: 5 }.limit().unwrap();
        assert_eq!(ours, theirs);
        assert_eq!(ours.reached, Some(Quantity::Bytes(9)));
        for other in [
            IpcError::Timeout,
            IpcError::Closed("x".into()),
            IpcError::Protocol("x".into()),
        ] {
            assert_eq!(other.limit(), None);
        }
    }
}
