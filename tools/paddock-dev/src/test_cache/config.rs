//! `docs/test-cache.toml` through its one accessor (ER-42): the epoch, the age rule while the nightly is paused, the nextest profile, the
//! never-cached list and the switch (ADR 0116 §2, §3, §6, §8).

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

/// The file, repository-relative.
pub const FILE: &str = "docs/test-cache.toml";

/// How many green train runs with no soundness finding switch reuse on (ADR 0116 §8, ER-41).
pub const GREEN_RUNS_TO_SWITCH: usize = 2;

/// The switch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Switch {
    /// The runs (ids) that showed zero soundness findings in shadow mode.
    pub green_runs: Vec<u64>,
}

/// The settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Bumped to invalidate every record.
    pub epoch: u32,
    /// The nightly does not run, so old records are refused.
    pub nightly_paused: bool,
    /// The age limit while `nightly_paused`.
    pub max_age_hours: u64,
    /// The nextest profile of a target's run.
    pub profile: String,
    /// Targets never cached, by node id.
    pub no_test_cache: Vec<String>,
    /// The switch.
    pub switch: Switch,
}

/// Whether a would-be reuse skips the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Every target runs; a would-be reuse is only reported, and a matched target that fails is a soundness finding.
    Shadow,
    /// A target with a valid record is skipped.
    On,
}

impl Config {
    /// The settings in `text`. An unknown key, a missing one or one of the wrong type is an error naming it (never a default).
    pub fn parse(text: &str) -> Result<Config, String> {
        let bad = |m: String| format!("{FILE} cannot be read: {m}");
        let t: toml::Table = text.parse().map_err(|e: toml::de::Error| bad(e.to_string()))?;
        for k in t.keys() {
            if !["epoch", "nightly_paused", "max_age_hours", "profile", "no_test_cache", "switch"].contains(&k.as_str()) {
                return Err(bad(format!("unknown key `{k}`")));
            }
        }
        let int = |k: &str| -> Result<u64, String> {
            t.get(k)
                .and_then(toml::Value::as_integer)
                .and_then(|n| u64::try_from(n).ok())
                .ok_or_else(|| bad(format!("`{k}` must be a non-negative integer")))
        };
        let strings = |v: Option<&toml::Value>, what: &str| -> Result<Vec<String>, String> {
            match v {
                None => Ok(Vec::new()),
                Some(v) => v
                    .as_array()
                    .and_then(|a| a.iter().map(|x| x.as_str().map(str::to_string)).collect())
                    .ok_or_else(|| bad(format!("`{what}` must be a list of strings"))),
            }
        };
        let green_runs = match t.get("switch") {
            None => Vec::new(),
            Some(s) => {
                let s = s.as_table().ok_or_else(|| bad("`switch` must be a table".into()))?;
                if let Some(k) = s.keys().find(|k| k.as_str() != "green_runs") {
                    return Err(bad(format!("unknown key `switch.{k}`")));
                }
                match s.get("green_runs") {
                    None => Vec::new(),
                    Some(v) => v
                        .as_array()
                        .and_then(|a| a.iter().map(|x| x.as_integer().and_then(|n| u64::try_from(n).ok())).collect())
                        .ok_or_else(|| bad("`switch.green_runs` must be a list of run ids".into()))?,
                }
            }
        };
        let c = Config {
            epoch: u32::try_from(int("epoch")?).map_err(|_| bad("`epoch` is too large".into()))?,
            nightly_paused: t
                .get("nightly_paused")
                .and_then(toml::Value::as_bool)
                .ok_or_else(|| bad("`nightly_paused` must be true or false".into()))?,
            max_age_hours: int("max_age_hours")?,
            profile: t
                .get("profile")
                .and_then(toml::Value::as_str)
                .filter(|p| !p.is_empty())
                .ok_or_else(|| bad("`profile` must be a non-empty string".into()))?
                .to_string(),
            no_test_cache: strings(t.get("no_test_cache"), "no_test_cache")?,
            switch: Switch { green_runs },
        };
        if c.nightly_paused && c.max_age_hours == 0 {
            return Err(bad("max_age_hours must be above 0 while nightly_paused (a zero limit refuses every record)".into()));
        }
        Ok(c)
    }

    /// The settings of the checkout at `root`.
    pub fn read(root: &Path) -> Result<Config, String> {
        let p = root.join(FILE);
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{} cannot be read: {e}", p.display()))?;
        Config::parse(&text)
    }

    /// Shadow until [`GREEN_RUNS_TO_SWITCH`] distinct runs are recorded.
    pub fn mode(&self) -> Mode {
        let runs: BTreeSet<&u64> = self.switch.green_runs.iter().collect();
        if runs.len() >= GREEN_RUNS_TO_SWITCH { Mode::On } else { Mode::Shadow }
    }

    /// The oldest record a run accepts: `None` when the nightly runs (it is the net).
    pub fn max_age(&self) -> Option<Duration> {
        self.nightly_paused.then(|| Duration::from_secs(self.max_age_hours * 3600))
    }

    /// Whether the target (a model node id) is listed as never cached.
    pub fn never_cached(&self, node: &str) -> bool {
        self.no_test_cache.iter().any(|n| n == node)
    }
}

/// The `no_test_cache` entries the merge base has and `head` does not: a weakening that needs the user's approval (ER-46 read in its
/// own direction). A text that cannot be read counts as having no entries on the head side, so the check never passes on a broken file.
pub fn removed(base: &str, head: &str) -> Result<Vec<String>, String> {
    let base = Config::parse(base)?;
    let head = Config::parse(head)?;
    Ok(base.no_test_cache.into_iter().filter(|n| !head.no_test_cache.contains(n)).collect())
}
