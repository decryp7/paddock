//! The scenarios that do not apply to an endpoint, read from the register (`docs/ipc-endpoints.toml`, `[[not_applicable]]`:
//! `endpoint`, `scenario`, `reason` and optionally `os`). The reasons live in the register, where a reviewer reads them and
//! the check C102 validates them; a rig cannot declare a scenario away in code, and a scenario it cannot drive is a rig
//! failure ([`super::Outcome::RigFailure`]).
//!
//! The same file's `[[precondition_exempt]]` entries (`endpoint`, `scenario`, `precondition`, `reason`) narrow a scenario's
//! default preconditions for one endpoint (ADR 0122 §1): the only way a rig declares less than
//! [`Scenario::default_preconditions`]. The list only shrinks (ER-46).

use super::{Precondition, Scenario};
use std::path::{Path, PathBuf};

/// The register file, relative to the repository root.
pub const FILE: &str = "docs/ipc-endpoints.toml";

/// A path to the register file, for a test binary that runs away from the source tree.
pub const FILE_ENV: &str = "PADDOCK_ENDINGS_REGISTER_FILE";

/// One declared "does not apply".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The endpoint id.
    pub endpoint: String,
    /// The scenario name.
    pub scenario: String,
    /// Why.
    pub reason: String,
    /// The operating systems it holds on (empty: all).
    pub os: String,
}

/// One declared precondition exemption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exempt {
    /// The endpoint id.
    pub endpoint: String,
    /// The scenario.
    pub scenario: Scenario,
    /// The default precondition of the scenario that does not apply to this endpoint.
    pub precondition: Precondition,
    /// Why.
    pub reason: String,
}

/// The `[[not_applicable]]` and `[[precondition_exempt]]` entries of the register.
#[derive(Debug, Clone, Default)]
pub struct Applicability {
    /// Every `[[not_applicable]]` entry.
    pub entries: Vec<Entry>,
    /// Every `[[precondition_exempt]]` entry.
    pub exempt: Vec<Exempt>,
}

impl Applicability {
    /// Nothing is declared not applicable: every scenario runs.
    pub fn none() -> Applicability {
        Applicability::default()
    }

    /// A list given in code, for the controls of the harness itself (the mini protocol is not in the register).
    pub fn of(endpoint: &str, list: &[(Scenario, &str)]) -> Applicability {
        Applicability {
            entries: list
                .iter()
                .map(|(s, why)| Entry {
                    endpoint: endpoint.to_string(),
                    scenario: s.name().to_string(),
                    reason: (*why).to_string(),
                    os: String::new(),
                })
                .collect(),
            exempt: Vec::new(),
        }
    }

    /// The same list with one precondition exemption more, for the controls of the harness itself.
    pub fn with_exempt(
        mut self,
        endpoint: &str,
        scenario: Scenario,
        precondition: Precondition,
        reason: &str,
    ) -> Applicability {
        self.exempt.push(Exempt {
            endpoint: endpoint.to_string(),
            scenario,
            precondition,
            reason: reason.to_string(),
        });
        self
    }

    /// Parses the register's text; the other tables of the file are skipped.
    ///
    /// # Errors
    /// A line of the two tables that is not `key = "value"`, an unknown key, a missing `endpoint`, `scenario` or `reason`
    /// (and `precondition` for an exemption), an unknown scenario or precondition in an exemption, or an exemption of a
    /// precondition that is not among the scenario's defaults.
    pub fn parse(text: &str) -> Result<Applicability, String> {
        const NOT_APPLICABLE: &str = "not_applicable";
        const EXEMPT: &str = "precondition_exempt";
        let mut found = Applicability::default();
        let mut current: Option<(&str, Vec<(String, String)>)> = None;
        let mut flush = |table: Option<(&str, Vec<(String, String)>)>,
                         at: usize|
         -> Result<(), String> {
            let Some((kind, fields)) = table else {
                return Ok(());
            };
            let get = |k: &str| {
                fields
                    .iter()
                    .find(|(n, _)| n == k)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            let need: &[&str] = if kind == EXEMPT {
                &["endpoint", "scenario", "precondition", "reason"]
            } else {
                &["endpoint", "scenario", "reason"]
            };
            for k in need {
                if get(k).trim().is_empty() {
                    return Err(format!(
                        "the [[{kind}]] entry before line {at} has no `{k}`"
                    ));
                }
            }
            if kind == NOT_APPLICABLE {
                found.entries.push(Entry {
                    endpoint: get("endpoint"),
                    scenario: get("scenario"),
                    reason: get("reason"),
                    os: get("os"),
                });
                return Ok(());
            }
            let scenario = Scenario::from_name(&get("scenario")).ok_or_else(|| {
                format!(
                    "the [[{kind}]] entry before line {at} names an unknown scenario `{}`",
                    get("scenario")
                )
            })?;
            let precondition = Precondition::from_name(&get("precondition")).ok_or_else(|| {
                format!(
                    "the [[{kind}]] entry before line {at} names an unknown precondition `{}`",
                    get("precondition")
                )
            })?;
            if !scenario.default_preconditions().contains(&precondition) {
                return Err(format!(
                    "the [[{kind}]] entry before line {at}: {precondition} is not a precondition of {scenario}, so there is nothing to exempt"
                ));
            }
            found.exempt.push(Exempt {
                endpoint: get("endpoint"),
                scenario,
                precondition,
                reason: get("reason"),
            });
            Ok(())
        };
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with("[[") {
                flush(current.take(), i + 1)?;
                current = match line {
                    "[[not_applicable]]" => Some((NOT_APPLICABLE, Vec::new())),
                    "[[precondition_exempt]]" => Some((EXEMPT, Vec::new())),
                    _ => None,
                };
                continue;
            }
            let Some((kind, fields)) = current.as_mut() else {
                continue;
            };
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!(
                    "line {}: expected `key = \"value\"`, got `{line}`",
                    i + 1
                ));
            };
            let (key, value) = (key.trim(), value.trim());
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .ok_or_else(|| {
                    format!(
                        "line {}: the value of `{key}` must be a quoted string",
                        i + 1
                    )
                })?;
            let known: &[&str] = if *kind == EXEMPT {
                &["endpoint", "scenario", "precondition", "reason"]
            } else {
                &["endpoint", "scenario", "reason", "os"]
            };
            if !known.contains(&key) {
                return Err(format!("line {}: unknown key `{key}` in [[{kind}]]", i + 1));
            }
            fields.push((key.to_string(), value.replace("\\\"", "\"")));
        }
        flush(current.take(), text.lines().count() + 1)?;
        Ok(found)
    }

    /// The register of the repository that contains `manifest_dir` (found by walking up, or the file [`FILE_ENV`] names).
    ///
    /// # Errors
    /// The file was not found or does not parse.
    pub fn load_from(manifest_dir: &str) -> Result<Applicability, String> {
        let path = match std::env::var(FILE_ENV) {
            Ok(p) => PathBuf::from(p),
            Err(_) => {
                let mut dir: PathBuf = Path::new(manifest_dir).to_path_buf();
                loop {
                    let candidate = dir.join(FILE);
                    if candidate.is_file() {
                        break candidate;
                    }
                    if !dir.pop() {
                        return Err(format!("{FILE} was not found above {manifest_dir}"));
                    }
                }
            }
        };
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Applicability::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Why `scenario` does not apply to `endpoint` on this OS, if the register says so.
    pub fn reason(&self, endpoint: &str, scenario: Scenario) -> Option<String> {
        self.entries
            .iter()
            .find(|e| {
                e.endpoint == endpoint
                    && e.scenario == scenario.name()
                    && (e.os.is_empty()
                        || e.os.split(',').any(|o| o.trim() == std::env::consts::OS))
            })
            .map(|e| e.reason.clone())
    }

    /// Why the register exempts `endpoint`'s `scenario` from `precondition`, if it does.
    pub fn exempt_reason(
        &self,
        endpoint: &str,
        scenario: Scenario,
        precondition: Precondition,
    ) -> Option<&str> {
        self.exempt
            .iter()
            .find(|e| {
                e.endpoint == endpoint && e.scenario == scenario && e.precondition == precondition
            })
            .map(|e| e.reason.as_str())
    }
}
