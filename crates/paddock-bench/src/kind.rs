use serde::{Deserialize, Serialize};

/// Which way is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Smaller is better (times, memory); the budget is a maximum.
    LowerIsBetter,
    /// Larger is better (throughput); the budget is a minimum.
    HigherIsBetter,
}

/// What a measurement measures (rule 16). The kind fixes the unit and the direction, so a budget never has to say them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Process start to a usable window or ready endpoint, milliseconds.
    Startup,
    /// Items (lines, events, files, bytes) per second.
    Throughput,
    /// Start of an operation to its first result, milliseconds.
    FirstEvent,
    /// One query or interaction, milliseconds.
    QueryLatency,
    /// Memory (peak RSS or PSS), megabytes.
    Memory,
}

impl Kind {
    /// Every kind.
    pub const ALL: [Kind; 5] = [
        Kind::Startup,
        Kind::Throughput,
        Kind::FirstEvent,
        Kind::QueryLatency,
        Kind::Memory,
    ];

    /// Which way is better.
    pub fn direction(self) -> Direction {
        match self {
            Kind::Throughput => Direction::HigherIsBetter,
            _ => Direction::LowerIsBetter,
        }
    }

    /// The unit of the numbers.
    pub fn unit(self) -> &'static str {
        match self {
            Kind::Startup | Kind::FirstEvent | Kind::QueryLatency => "ms",
            Kind::Throughput => "/s",
            Kind::Memory => "MB",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_and_directions_follow_the_kind() {
        assert_eq!(Kind::Throughput.direction(), Direction::HigherIsBetter);
        for k in [
            Kind::Startup,
            Kind::FirstEvent,
            Kind::QueryLatency,
            Kind::Memory,
        ] {
            assert_eq!(k.direction(), Direction::LowerIsBetter);
        }
        let units: Vec<&str> = Kind::ALL.iter().map(|k| k.unit()).collect();
        assert_eq!(units, ["ms", "/s", "ms", "ms", "MB"]);
        assert_eq!(
            serde_json::to_string(&Kind::QueryLatency).unwrap(),
            "\"query-latency\""
        );
        assert_eq!(
            serde_json::to_string(&Kind::FirstEvent).unwrap(),
            "\"first-event\""
        );
    }
}
