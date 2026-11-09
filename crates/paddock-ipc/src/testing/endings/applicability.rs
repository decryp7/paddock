//! The scenarios that do not apply to an endpoint, read from the register (`docs/ipc-endpoints.toml`, `[[not_applicable]]`:
//! `endpoint`, `scenario`, `reason` and optionally `os`). The reasons live in the register, where a reviewer reads them and
//! the check C102 validates them; a rig cannot declare a scenario away in code, and a scenario it cannot drive is a rig
//! failure ([`super::Outcome::RigFailure`]).

use super::Scenario;
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

/// The `[[not_applicable]]` entries of the register.
#[derive(Debug, Clone, Default)]
pub struct Applicability {
    /// Every entry.
    pub entries: Vec<Entry>,
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
        }
    }

    /// Parses the register's text; the other tables of the file are skipped.
    ///
    /// # Errors
    /// A `[[not_applicable]]` line that is not `key = "value"`, an unknown key or a missing `endpoint`, `scenario` or `reason`.
    pub fn parse(text: &str) -> Result<Applicability, String> {
        let mut entries = Vec::new();
        let mut current: Option<Vec<(String, String)>> = None;
        let mut flush = |fields: Option<Vec<(String, String)>>, at: usize| -> Result<(), String> {
            let Some(fields) = fields else { return Ok(()) };
            let get = |k: &str| {
                fields
                    .iter()
                    .find(|(n, _)| n == k)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            for k in ["endpoint", "scenario", "reason"] {
                if get(k).trim().is_empty() {
                    return Err(format!(
                        "the [[not_applicable]] entry before line {at} has no `{k}`"
                    ));
                }
            }
            entries.push(Entry {
                endpoint: get("endpoint"),
                scenario: get("scenario"),
                reason: get("reason"),
                os: get("os"),
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
                if line == "[[not_applicable]]" {
                    current = Some(Vec::new());
                }
                continue;
            }
            let Some(fields) = current.as_mut() else {
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
            if !["endpoint", "scenario", "reason", "os"].contains(&key) {
                return Err(format!(
                    "line {}: unknown key `{key}` in [[not_applicable]]",
                    i + 1
                ));
            }
            fields.push((key.to_string(), value.replace("\\\"", "\"")));
        }
        flush(current.take(), text.lines().count() + 1)?;
        Ok(Applicability { entries })
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
}
