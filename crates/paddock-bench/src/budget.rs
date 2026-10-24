use crate::error::{BenchError, Result};
use crate::kind::{Direction, Kind};
use crate::results::{Results, read_bounded};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// How far past the limit a number may be before the budget fails, when the file does not say.
pub const DEFAULT_TOLERANCE_PCT: f64 = 10.0;
/// How much worse than the previous run counts as a regression, when the file does not say.
pub const DEFAULT_REGRESSION_PCT: f64 = 20.0;
/// Most budgets in one file.
pub const MAX_BUDGETS: usize = 1000;

fn default_tolerance() -> f64 {
    DEFAULT_TOLERANCE_PCT
}

fn default_regression() -> f64 {
    DEFAULT_REGRESSION_PCT
}

/// A stored budget for one measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// The measurement it applies to.
    pub name: String,
    /// The kind the measurement must have (guards against a renamed measurement changing meaning); optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    /// The limit: a maximum for times and memory, a minimum for throughput.
    pub limit: f64,
    /// How far past the limit, in percent, still passes (as a warning). Machines differ; this is the noise margin.
    #[serde(default = "default_tolerance")]
    pub tolerance_pct: f64,
    /// How much worse than the previous run, in percent, fails even inside the limit.
    #[serde(default = "default_regression")]
    pub regression_pct: f64,
}

impl Budget {
    /// The value the measurement may reach before it fails (the limit moved by the tolerance).
    pub fn allowed(&self, direction: Direction) -> f64 {
        match direction {
            Direction::LowerIsBetter => self.limit * (1.0 + self.tolerance_pct / 100.0),
            Direction::HigherIsBetter => self.limit * (1.0 - self.tolerance_pct / 100.0),
        }
    }
}

/// The budgets file (`bench/budgets.toml`): a list of `[[budget]]` tables.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    /// The budgets, in file order.
    #[serde(default)]
    pub budget: Vec<Budget>,
}

impl Budgets {
    /// Reads and validates a budgets file (bounded size).
    pub fn load(path: &Path) -> Result<Budgets> {
        Budgets::parse(&read_bounded(path)?, path)
    }

    /// Reads budgets from TOML text; `path` names the source in errors. TOML is data: nothing is executed.
    pub fn parse(text: &str, path: &Path) -> Result<Budgets> {
        let b: Budgets = toml::from_str(text).map_err(|e| BenchError::Parse {
            path: path.into(),
            message: e.message().to_string(),
        })?;
        b.validate()?;
        Ok(b)
    }

    /// Limits must be positive finite numbers, percentages sensible, names present and unique.
    pub fn validate(&self) -> Result<()> {
        if self.budget.len() > MAX_BUDGETS {
            return Err(BenchError::Invalid(format!(
                "more than {MAX_BUDGETS} budgets."
            )));
        }
        for (i, b) in self.budget.iter().enumerate() {
            let bad = |what: &str| BenchError::Invalid(format!("budget {}: {what}.", b.name));
            if b.name.trim().is_empty() {
                return Err(BenchError::Invalid(
                    "a budget has no name; set `name` to the measurement's name.".into(),
                ));
            }
            if !b.limit.is_finite() || b.limit <= 0.0 {
                return Err(bad("`limit` must be a number above zero"));
            }
            if !(0.0..=100.0).contains(&b.tolerance_pct) {
                return Err(bad("`tolerance_pct` must be between 0 and 100"));
            }
            if !(0.0..=1000.0).contains(&b.regression_pct) {
                return Err(bad("`regression_pct` must be between 0 and 1000"));
            }
            if self.budget[..i].iter().any(|o| o.name == b.name) {
                return Err(bad("is listed twice"));
            }
        }
        Ok(())
    }

    /// The budget for `name`.
    pub fn get(&self, name: &str) -> Option<&Budget> {
        self.budget.iter().find(|b| b.name == name)
    }

    /// Budgets set from a good run, with `margin_pct` of room (a time may grow by it, a rate may drop by it): the way a
    /// first `budgets.toml` is written, and how a budget is tightened after an improvement.
    pub fn propose(results: &Results, margin_pct: f64) -> Budgets {
        let margin = margin_pct.clamp(0.0, 100.0) / 100.0;
        let budget = results
            .measurements
            .iter()
            .filter(|m| m.value > 0.0)
            .map(|m| Budget {
                name: m.name.clone(),
                kind: Some(m.kind),
                limit: match m.kind.direction() {
                    Direction::LowerIsBetter => m.value * (1.0 + margin),
                    Direction::HigherIsBetter => m.value * (1.0 - margin),
                },
                tolerance_pct: DEFAULT_TOLERANCE_PCT,
                regression_pct: DEFAULT_REGRESSION_PCT,
            })
            .collect();
        Budgets { budget }
    }

    /// The file's text.
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|e| BenchError::Write(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::Measurement;

    fn p() -> &'static Path {
        Path::new("budgets.toml")
    }

    #[test]
    fn reads_budgets_with_defaults() {
        let b = Budgets::parse(
            "[[budget]]\nname = \"startup\"\nkind = \"startup\"\nlimit = 1500\n\n[[budget]]\nname = \"import\"\nlimit = 3e5\ntolerance_pct = 5\nregression_pct = 15\n",
            p(),
        )
        .unwrap();
        assert_eq!(b.budget.len(), 2);
        let s = b.get("startup").unwrap();
        assert_eq!(
            (s.kind, s.limit, s.tolerance_pct, s.regression_pct),
            (Some(Kind::Startup), 1500.0, 10.0, 20.0)
        );
        assert_eq!(b.get("import").unwrap().regression_pct, 15.0);
        assert!(b.get("none").is_none());
        assert!(Budgets::parse("", p()).unwrap().budget.is_empty());
    }

    #[test]
    fn allowed_moves_with_the_direction() {
        let b = Budget {
            name: "x".into(),
            kind: None,
            limit: 100.0,
            tolerance_pct: 10.0,
            regression_pct: 20.0,
        };
        assert!((b.allowed(Direction::LowerIsBetter) - 110.0).abs() < 1e-9);
        assert!((b.allowed(Direction::HigherIsBetter) - 90.0).abs() < 1e-9);
    }

    #[test]
    fn refuses_unusable_budgets_with_the_fix() {
        let cases = [
            ("[[budget]]\nname = \"\"\nlimit = 1\n", "no name"),
            ("[[budget]]\nname = \"a\"\nlimit = 0\n", "above zero"),
            ("[[budget]]\nname = \"a\"\nlimit = -3\n", "above zero"),
            ("[[budget]]\nname = \"a\"\nlimit = nan\n", "above zero"),
            (
                "[[budget]]\nname = \"a\"\nlimit = 1\ntolerance_pct = 101\n",
                "tolerance_pct",
            ),
            (
                "[[budget]]\nname = \"a\"\nlimit = 1\nregression_pct = -1\n",
                "regression_pct",
            ),
            (
                "[[budget]]\nname = \"a\"\nlimit = 1\n[[budget]]\nname = \"a\"\nlimit = 2\n",
                "twice",
            ),
            (
                "[[budget]]\nname = \"a\"\nlimit = 1\ncolour = \"red\"\n",
                "colour",
            ),
            (
                "[[budget]]\nname = \"a\"\nlimit = 1\nkind = \"speed\"\n",
                "speed",
            ),
            ("name = ", "expected"),
        ];
        for (text, want) in cases {
            let e = Budgets::parse(text, p()).unwrap_err().to_string();
            assert!(e.contains(want), "{text:?} -> {e}");
        }
        let many = "[[budget]]\nname = \"a\"\nlimit = 1\n".to_string();
        let mut b = Budgets::parse(&many, p()).unwrap();
        b.budget = (0..=MAX_BUDGETS)
            .map(|i| Budget {
                name: i.to_string(),
                ..b.budget[0].clone()
            })
            .collect();
        assert!(b.validate().is_err());
    }

    #[test]
    fn proposed_budgets_leave_room_in_the_right_direction_and_round_trip() {
        let mut r = Results::new("demo", "1");
        r.measurements.push(Measurement {
            name: "q".into(),
            kind: Kind::QueryLatency,
            value: 100.0,
            runs: 3,
        });
        r.measurements.push(Measurement {
            name: "t".into(),
            kind: Kind::Throughput,
            value: 1000.0,
            runs: 3,
        });
        r.measurements.push(Measurement {
            name: "zero".into(),
            kind: Kind::Memory,
            value: 0.0,
            runs: 1,
        });
        let b = Budgets::propose(&r, 25.0);
        assert_eq!(b.budget.len(), 2, "a zero measurement gives no budget");
        assert!((b.get("q").unwrap().limit - 125.0).abs() < 1e-9);
        assert!((b.get("t").unwrap().limit - 750.0).abs() < 1e-9);
        let text = b.to_toml().unwrap();
        assert_eq!(Budgets::parse(&text, p()).unwrap(), b);
    }

    #[test]
    fn load_reads_a_file_and_reports_a_missing_one() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("budgets.toml");
        std::fs::write(&f, "[[budget]]\nname = \"a\"\nlimit = 1\n").unwrap();
        assert_eq!(Budgets::load(&f).unwrap().budget.len(), 1);
        assert!(matches!(
            Budgets::load(&dir.path().join("none.toml")),
            Err(BenchError::Io { .. })
        ));
    }
}
