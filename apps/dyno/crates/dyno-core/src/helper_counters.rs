//! The helper's `Counters` answer (ADR 0027 WP-6, ADR 0022 section 5): the readings a standard account cannot take, one outcome
//! per requested id so a missing fan sensor never hides the temperatures. The fields are numbers and short labels only.

use crate::helper_requests::{CounterId, CounterQuery};
use crate::ports::PlatformError;
use serde::{Deserialize, Serialize};

/// The most entries one reading carries.
pub const MAX_ENTRIES: usize = 64;
/// The longest label.
pub const MAX_LABEL: usize = 64;

/// One thermal zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThermalZone {
    /// The zone's name as the OS gives it (`acpitz`, `CPU`).
    pub label: String,
    /// Temperature in thousandths of a degree Celsius.
    pub milli_celsius: i32,
}

/// One fan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FanSpeed {
    /// The fan's name.
    pub label: String,
    /// Revolutions per minute.
    pub rpm: u32,
}

/// One energy domain: a cumulative counter (Linux RAPL), or an instantaneous power (macOS `powermetrics`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnergyDomain {
    /// `package-0`, `core`, `CPU`.
    pub label: String,
    /// Cumulative energy in microjoules, when the OS keeps a counter.
    pub microjoules: Option<u64>,
    /// The counter wraps at this value, when the OS says.
    pub wraps_at_microjoules: Option<u64>,
    /// Power now in milliwatts, when the OS reports it directly.
    pub milliwatts: Option<u32>,
}

/// What one requested reading came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CounterValue {
    /// Thermal zones.
    Thermal(Vec<ThermalZone>),
    /// CPU energy domains.
    CpuEnergy(Vec<EnergyDomain>),
    /// Fan speeds.
    Fans(Vec<FanSpeed>),
    /// This reading is not available here, with the reason in words.
    Unavailable(String),
}

/// The answer to a `Counters` request: one entry per id, in the order asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterAnswer {
    /// Which reading.
    pub id: CounterId,
    /// What it came to.
    pub value: CounterValue,
}

/// The helper's reader of one counter kind. A failure is reported per id, never for the whole request.
pub trait CounterReader: Send + Sync {
    /// Reads `id` now.
    fn read(&self, id: CounterId) -> Result<CounterValue, PlatformError>;
}

/// What a system without these readings says.
pub struct NoCounters;

impl CounterReader for NoCounters {
    fn read(&self, _: CounterId) -> Result<CounterValue, PlatformError> {
        Err(PlatformError::Unsupported(
            "This system's sensors are not read through the helper.".into(),
        ))
    }
}

/// Answers `query`: each id once, a failed one as `Unavailable` with the error's words.
pub fn answer(reader: &dyn CounterReader, query: &CounterQuery) -> Vec<CounterAnswer> {
    query
        .ids
        .iter()
        .map(|id| {
            let value = match reader.read(*id) {
                Ok(v) => bounded(v),
                Err(e) => CounterValue::Unavailable(e.to_string().chars().take(200).collect()),
            };
            CounterAnswer { id: *id, value }
        })
        .collect()
}

/// Caps the entry count and label length so hostile or odd firmware cannot make a large answer.
pub fn bounded(v: CounterValue) -> CounterValue {
    let label = |s: String| -> String { s.chars().take(MAX_LABEL).collect() };
    match v {
        CounterValue::Thermal(z) => CounterValue::Thermal(
            z.into_iter()
                .take(MAX_ENTRIES)
                .map(|z| ThermalZone {
                    label: label(z.label),
                    ..z
                })
                .collect(),
        ),
        CounterValue::Fans(f) => CounterValue::Fans(
            f.into_iter()
                .take(MAX_ENTRIES)
                .map(|f| FanSpeed {
                    label: label(f.label),
                    ..f
                })
                .collect(),
        ),
        CounterValue::CpuEnergy(e) => CounterValue::CpuEnergy(
            e.into_iter()
                .take(MAX_ENTRIES)
                .map(|e| EnergyDomain {
                    label: label(e.label),
                    ..e
                })
                .collect(),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_requests::RequestClass;

    struct Scripted;
    impl CounterReader for Scripted {
        fn read(&self, id: CounterId) -> Result<CounterValue, PlatformError> {
            match id {
                CounterId::Thermal => Ok(CounterValue::Thermal(
                    (0..100)
                        .map(|i| ThermalZone {
                            label: "z".repeat(200) + &i.to_string(),
                            milli_celsius: 40_000,
                        })
                        .collect(),
                )),
                CounterId::Fans => Err(PlatformError::Unsupported("No fan sensor on this computer.".into())),
                CounterId::CpuEnergy => Ok(CounterValue::CpuEnergy(vec![])),
            }
        }
    }

    #[test]
    fn each_id_gets_its_own_outcome_in_order_and_a_failure_does_not_hide_the_rest() {
        let q = CounterQuery {
            ids: vec![CounterId::Fans, CounterId::Thermal, CounterId::CpuEnergy],
            class: RequestClass::User,
        };
        let a = answer(&Scripted, &q);
        assert_eq!(a.iter().map(|x| x.id).collect::<Vec<_>>(), q.ids);
        assert_eq!(a[0].value, CounterValue::Unavailable("No fan sensor on this computer.".into()));
        match &a[1].value {
            CounterValue::Thermal(z) => assert_eq!((z.len(), z[0].label.chars().count()), (MAX_ENTRIES, MAX_LABEL)),
            other => panic!("{other:?}"),
        }
        let back: Vec<CounterAnswer> = postcard::from_bytes(&postcard::to_stdvec(&a).unwrap()).unwrap();
        assert_eq!(back, a);
    }
}
