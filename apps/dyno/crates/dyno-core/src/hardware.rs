//! What the hardware itself reports about its limits and its wear: the OS's own CPU speed limit, SSD wear counters
//! and battery capacity. The platform reads them (behind the ports in [`crate::ports`]); the pure detectors in
//! [`crate::throttle`] and [`crate::lifespan`] judge them. Anything a machine does not report is a
//! [`Reading::Unavailable`] with the reason, never a zero.

use serde::{Deserialize, Serialize};

/// The reason a machine without a battery gives; the Lifespan page leaves the battery out when it sees it.
pub const NO_BATTERY: &str = "No battery";

/// How sure Dyno is of a finding. Shown in words next to every estimate and explanation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    /// Little evidence: treat it as a hint.
    Low,
    /// Some evidence: probably right.
    Medium,
    /// Strong evidence: the OS said so, or the measurements agree.
    High,
}

impl Confidence {
    /// A word for the UI.
    pub fn word(self) -> &'static str {
        match self {
            Confidence::Low => "low",
            Confidence::Medium => "medium",
            Confidence::High => "high",
        }
    }
}

/// What the operating system says about limits on the CPU's speed, where it says anything. Each field is `None` when
/// the OS does not report that fact (which is different from "no limit").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OsThrottle {
    /// The speed the OS currently allows, as a percentage of the maximum (100 = no limit).
    pub speed_limit_percent: Option<u8>,
    /// The OS reports a thermal limit in effect.
    pub thermal: Option<bool>,
    /// The OS reports a power limit in effect (battery, adapter or a charger that is too weak).
    pub power: Option<bool>,
}

impl OsThrottle {
    /// Whether the OS reports any limit at all.
    pub fn limited(&self) -> bool {
        self.speed_limit_percent.is_some_and(|p| p < 100) || self.thermal == Some(true) || self.power == Some(true)
    }
}

/// Wear counters of one SSD (SMART / NVMe health log).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskWear {
    /// A stable id for this physical disk (its serial number when the drive reports one, else its device name).
    pub id: String,
    /// Model or friendly name.
    pub name: String,
    /// Capacity in bytes, when reported.
    pub capacity: Option<u64>,
    /// "Percentage used" of the rated endurance, 0–100 and beyond, when reported.
    pub percent_used: Option<f32>,
    /// Total bytes written over the disk's life, when reported.
    pub bytes_written: Option<u64>,
    /// The manufacturer's rated endurance in bytes written, when known.
    pub rated_bytes: Option<u64>,
    /// Hours the disk has been powered on, when reported.
    pub power_on_hours: Option<u64>,
}

/// Capacity of the battery in whatever unit the OS gives (mWh, mAh or a percentage); only ratios are used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryCapacity {
    /// A stable id for the battery (its serial number when reported, else a fixed name).
    pub id: String,
    /// Capacity when new.
    pub design: u32,
    /// Capacity when fully charged now.
    pub full: u32,
    /// Charge cycles so far, when reported.
    pub cycles: Option<u32>,
    /// The maker's rated number of cycles before the capacity is expected to fall to 80 %, when known.
    pub rated_cycles: Option<u32>,
}

impl BatteryCapacity {
    /// Full-charge capacity as a percentage of the design capacity (`None` when the design capacity is unknown or 0).
    pub fn health_percent(&self) -> Option<f32> {
        (self.design > 0).then(|| self.full as f32 * 100.0 / self.design as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_only_reported_when_the_os_says_so() {
        assert!(!OsThrottle::default().limited());
        assert!(!OsThrottle {
            speed_limit_percent: Some(100),
            ..Default::default()
        }
        .limited());
        assert!(OsThrottle {
            speed_limit_percent: Some(70),
            ..Default::default()
        }
        .limited());
        assert!(OsThrottle {
            thermal: Some(true),
            ..Default::default()
        }
        .limited());
        assert!(OsThrottle {
            power: Some(true),
            ..Default::default()
        }
        .limited());
        assert!(!OsThrottle {
            thermal: Some(false),
            power: Some(false),
            ..Default::default()
        }
        .limited());
    }

    #[test]
    fn battery_health_is_a_ratio() {
        let b = BatteryCapacity {
            id: "b".into(),
            design: 5000,
            full: 4000,
            cycles: Some(300),
            rated_cycles: None,
        };
        assert_eq!(b.health_percent(), Some(80.0));
        assert_eq!(BatteryCapacity { design: 0, ..b }.health_percent(), None);
        assert_eq!(Confidence::High.word(), "high");
        assert!(Confidence::Low < Confidence::High);
    }
}
