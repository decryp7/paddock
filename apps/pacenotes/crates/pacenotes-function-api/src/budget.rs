//! [`Budget`]: the bound every call across the port carries (a cap in the sense of ADR 0031).
//!
//! The time limit counts **host ticks**, not seconds: the host counts one tick per beat of its own monitor, so a pause
//! (the machine asleep, the app stopped by the OS) spends none, and a call that reports progress is not cut. Its value is
//! a setting ([`Budget::setting_specs`] holds the defaults, in Settings, not in code), and when a limit is reached the error
//! says which limit, its value and where to change it ([`BudgetExceeded`]). Nothing here reads a clock.
//!
//! TODO(shared-later, ADR 0002 "Shared later"): the cap vocabulary belongs with ADR 0031's policy types in `paddock-jobs`.

use serde::{Deserialize, Serialize};

use crate::codec::MAX_MESSAGE_BYTES;
use crate::contribution::{SettingSpec, SettingValue};

/// The most ticks a single call may be given.
pub const MAX_TICKS: u32 = 3600;
/// The most items (rows, hits, symbols) a single call may be asked for.
pub const MAX_ITEMS: u32 = 100_000;
/// The most bytes a single reply may carry: what one message can hold, from the codec's single bound.
pub const MAX_BYTES: u32 = MAX_MESSAGE_BYTES as u32;

/// Settings keys of the three limits. They are owned by the host (the `host_` prefix is reserved: a function's own settings
/// may not use it, see [`Contributions::new`](crate::Contributions::new)), which adds [`Budget::setting_specs`] once.
pub const SETTING_TICKS: &str = "host_budget_ticks";
/// Settings key of the item limit.
pub const SETTING_ITEMS: &str = "host_budget_items";
/// Settings key of the byte limit.
pub const SETTING_BYTES: &str = "host_budget_bytes";

/// What a budget runs out of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BudgetKind {
    /// Host ticks.
    Ticks,
    /// Items.
    Items,
    /// Bytes.
    Bytes,
}

impl BudgetKind {
    fn key(self) -> &'static str {
        match self {
            BudgetKind::Ticks => SETTING_TICKS,
            BudgetKind::Items => SETTING_ITEMS,
            BudgetKind::Bytes => SETTING_BYTES,
        }
    }

    fn unit(self) -> &'static str {
        match self {
            BudgetKind::Ticks => "host ticks",
            BudgetKind::Items => "items",
            BudgetKind::Bytes => "bytes",
        }
    }
}

/// A limit that was reached. The message explains itself (ADR 0019/0031): what ran out, how much was allowed and the
/// setting that changes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("the call used up its {limit} {unit} (raise \"{key}\" in Settings if this is expected)", unit = .kind.unit(), key = .kind.key())]
pub struct BudgetExceeded {
    /// Which limit.
    pub kind: BudgetKind,
    /// Its value.
    pub limit: u32,
}

/// Why a budget is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BudgetError {
    /// A limit is 0 or above its maximum.
    #[error("the {0:?} limit must be between 1 and its maximum")]
    OutOfRange(BudgetKind),
}

/// The wire form of a [`Budget`].
#[derive(Serialize, Deserialize)]
pub(crate) struct BudgetWire {
    ticks: u32,
    items: u32,
    bytes: u32,
}

impl TryFrom<BudgetWire> for Budget {
    type Error = BudgetError;
    fn try_from(w: BudgetWire) -> Result<Budget, BudgetError> {
        Budget::new(w.ticks, w.items, w.bytes)
    }
}

/// The limits of one call: host ticks the caller will wait, items and bytes the reply may hold. A call that would exceed
/// one stops and says which ([`BudgetExceeded`]); it never runs unbounded. Reading one from the wire goes through
/// [`Budget::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BudgetWire")]
pub struct Budget {
    ticks: u32,
    items: u32,
    bytes: u32,
}

impl Budget {
    /// A budget; each limit must be 1 up to its maximum.
    pub fn new(ticks: u32, items: u32, bytes: u32) -> Result<Budget, BudgetError> {
        for (kind, value, max) in [
            (BudgetKind::Ticks, ticks, MAX_TICKS),
            (BudgetKind::Items, items, MAX_ITEMS),
            (BudgetKind::Bytes, bytes, MAX_BYTES),
        ] {
            if value == 0 || value > max {
                return Err(BudgetError::OutOfRange(kind));
            }
        }
        Ok(Budget { ticks, items, bytes })
    }

    /// A budget from the three settings' integer values.
    pub fn from_setting_values(ticks: i64, items: i64, bytes: i64) -> Result<Budget, BudgetError> {
        let to = |v: i64, kind| u32::try_from(v).map_err(|_| BudgetError::OutOfRange(kind));
        Budget::new(
            to(ticks, BudgetKind::Ticks)?,
            to(items, BudgetKind::Items)?,
            to(bytes, BudgetKind::Bytes)?,
        )
    }

    /// The settings that hold the limits, with their defaults: 30 ticks, 1000 items, 1 MiB. The defaults live here, in
    /// the settings a function contributes, and not as constants of the budget.
    pub fn setting_specs() -> Vec<SettingSpec> {
        vec![
            SettingSpec {
                key: SETTING_TICKS.into(),
                title: "Most host ticks one call may take".into(),
                default: SettingValue::Int(30),
            },
            SettingSpec {
                key: SETTING_ITEMS.into(),
                title: "Most items one call may return".into(),
                default: SettingValue::Int(1000),
            },
            SettingSpec {
                key: SETTING_BYTES.into(),
                title: "Most bytes one call may return".into(),
                default: SettingValue::Int(1 << 20),
            },
        ]
    }

    /// Host ticks the caller will wait.
    pub fn ticks(&self) -> u32 {
        self.ticks
    }

    /// Items the reply may hold.
    pub fn items(&self) -> u32 {
        self.items
    }

    /// Bytes the reply may hold.
    pub fn bytes(&self) -> u32 {
        self.bytes
    }

    /// Checks a reply size against the budget: the first limit it exceeds, if any.
    pub fn exceeded_by(&self, items: usize, bytes: usize) -> Option<BudgetExceeded> {
        if items > self.items as usize {
            Some(BudgetExceeded {
                kind: BudgetKind::Items,
                limit: self.items,
            })
        } else if bytes > self.bytes as usize {
            Some(BudgetExceeded {
                kind: BudgetKind::Bytes,
                limit: self.bytes,
            })
        } else {
            None
        }
    }

    /// Checks the ticks a call has used against the budget (the host passes the count of its own ticks).
    pub fn ticks_exceeded_by(&self, used: u32) -> Option<BudgetExceeded> {
        (used > self.ticks).then_some(BudgetExceeded {
            kind: BudgetKind::Ticks,
            limit: self.ticks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_required_and_bounded() {
        assert!(Budget::new(1, 1, 1).is_ok());
        assert_eq!(Budget::new(0, 1, 1), Err(BudgetError::OutOfRange(BudgetKind::Ticks)));
        assert_eq!(Budget::new(1, MAX_ITEMS + 1, 1), Err(BudgetError::OutOfRange(BudgetKind::Items)));
        assert_eq!(Budget::new(1, 1, MAX_BYTES + 1), Err(BudgetError::OutOfRange(BudgetKind::Bytes)));
        assert!(Budget::new(MAX_TICKS, MAX_ITEMS, MAX_BYTES).is_ok());
        assert_eq!(MAX_BYTES as usize, MAX_MESSAGE_BYTES, "one bound for what a reply can hold");
    }

    #[test]
    fn limits_have_an_effect_and_explain_themselves() {
        let b = Budget::new(10, 2, 100).unwrap();
        assert_eq!(b.exceeded_by(2, 100), None);
        let items = b.exceeded_by(3, 0).unwrap();
        assert_eq!((items.kind, items.limit), (BudgetKind::Items, 2));
        assert!(
            items.to_string().contains("2 items") && items.to_string().contains(SETTING_ITEMS),
            "{items}"
        );
        assert_eq!(b.exceeded_by(0, 101).unwrap().kind, BudgetKind::Bytes);
        assert_eq!(b.ticks_exceeded_by(10), None);
        let ticks = b.ticks_exceeded_by(11).unwrap();
        assert!(ticks.to_string().contains("host ticks") && ticks.to_string().contains(SETTING_TICKS));
        assert_eq!((b.ticks(), b.items(), b.bytes()), (10, 2, 100));
    }

    #[test]
    fn the_defaults_live_in_the_settings_and_build_a_valid_budget() {
        let specs = Budget::setting_specs();
        let int = |key: &str| match specs.iter().find(|s| s.key == key).unwrap().default {
            SettingValue::Int(v) => v,
            _ => panic!("not an integer"),
        };
        let b = Budget::from_setting_values(int(SETTING_TICKS), int(SETTING_ITEMS), int(SETTING_BYTES)).unwrap();
        assert_eq!((b.ticks(), b.items(), b.bytes()), (30, 1000, 1 << 20));
        assert_eq!(
            Budget::from_setting_values(-1, 1, 1),
            Err(BudgetError::OutOfRange(BudgetKind::Ticks))
        );
        assert_eq!(
            Budget::from_setting_values(1, 1, i64::MAX),
            Err(BudgetError::OutOfRange(BudgetKind::Bytes))
        );
    }
}
