//! The ratchet (ADR 0072 §5, D8): what fails today, per instance of the ADR's table, in `docs/ipc-endpoint-ratchet.toml`.
//! An ending test is green with the ratchet and red without it, and the ratchet only shrinks: a fault that is not listed
//! fails the run (a new failure), and a listed fault that no longer happens fails it too (remove the entry). Each entry
//! names the tag pattern it blocks (`blocks_tag`); the static check C102 reads the same file.
//!
//! The file is a tiny TOML subset (`[[ratchet]]` tables of `key = "value"` lines), read here without a TOML dependency so
//! that every crate with an ending test can use it.

use super::{Report, Scenario};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The ratchet file, relative to the repository root.
pub const FILE: &str = "docs/ipc-endpoint-ratchet.toml";

/// A path to the ratchet file, for a test binary that runs away from the source tree (the Windows test VM): it replaces the
/// walk up from the crate's folder.
pub const FILE_ENV: &str = "PADDOCK_ENDINGS_RATCHET_FILE";

/// Set to `1` to run an ending test without its ratchet: it then fails on every fault that exists today.
pub const NO_RATCHET_ENV: &str = "PADDOCK_ENDINGS_NO_RATCHET";

/// One expected failure.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Entry {
    /// The instance of the ADR's table (`I11`).
    pub instance: String,
    /// The endpoint id of `docs/ipc-endpoints.toml`.
    pub endpoint: String,
    /// The scenario name, or `*` for every scenario that ran (used for faults every scenario shows, such as `no-end-line`).
    pub scenario: String,
    /// The fault kind ([`super::Fault::kind`]).
    pub fault: String,
    /// The tag pattern a release of which is refused while this entry exists (`dyno-v*`).
    pub blocks_tag: String,
    /// The operating systems on which the fault happens (`linux`, `macos`, `windows`, comma separated); empty: all. An
    /// entry for another OS is neither expected nor stale here (a reset on a closed listener's backlog is Linux only).
    pub os: String,
    /// Why, in words.
    pub note: String,
}

impl Entry {
    /// Whether the entry applies on the OS this test runs on.
    pub fn applies_here(&self) -> bool {
        self.os.is_empty() || self.os.split(',').any(|o| o.trim() == std::env::consts::OS)
    }
}

/// The entries.
#[derive(Debug, Clone, Default)]
pub struct Ratchet {
    /// Every entry, in file order.
    pub entries: Vec<Entry>,
}

impl Ratchet {
    /// Parses the file's text.
    ///
    /// # Errors
    /// A line that is not a comment, a `[[ratchet]]` header or `key = "value"`, an unknown key, or an entry without all
    /// of `instance`, `endpoint`, `scenario`, `fault` and `blocks_tag`.
    pub fn parse(text: &str) -> Result<Ratchet, String> {
        let mut entries = Vec::new();
        let mut current: Option<Vec<(String, String)>> = None;
        let mut finish = |fields: Vec<(String, String)>, at: usize| -> Result<(), String> {
            let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
            let need = |k: &str| {
                get(k)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| format!("the entry before line {at} has no `{k}`"))
            };
            entries.push(Entry {
                instance: need("instance")?,
                endpoint: need("endpoint")?,
                scenario: need("scenario")?,
                fault: need("fault")?,
                blocks_tag: need("blocks_tag")?,
                os: get("os").unwrap_or_default(),
                note: get("note").unwrap_or_default(),
            });
            Ok(())
        };
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line == "[[ratchet]]" {
                if let Some(fields) = current.take() {
                    finish(fields, i + 1)?;
                }
                current = Some(Vec::new());
                continue;
            }
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
            if ![
                "instance",
                "endpoint",
                "scenario",
                "fault",
                "blocks_tag",
                "os",
                "note",
            ]
            .contains(&key)
            {
                return Err(format!("line {}: unknown key `{key}`", i + 1));
            }
            let Some(fields) = current.as_mut() else {
                return Err(format!(
                    "line {}: `{key}` is outside a [[ratchet]] table",
                    i + 1
                ));
            };
            fields.push((key.to_string(), value.replace("\\\"", "\"")));
        }
        if let Some(fields) = current.take() {
            finish(fields, text.lines().count() + 1)?;
        }
        Ok(Ratchet { entries })
    }

    /// The ratchet file of the repository that contains `manifest_dir` (a crate's `CARGO_MANIFEST_DIR`), found by walking
    /// up to the first folder with `docs/ipc-endpoint-ratchet.toml` (or the file [`FILE_ENV`] names). With [`NO_RATCHET_ENV`] set
    /// to `1`, an empty ratchet.
    ///
    /// # Errors
    /// The file was not found or does not parse.
    pub fn load_from(manifest_dir: &str) -> Result<Ratchet, String> {
        if std::env::var(NO_RATCHET_ENV).is_ok_and(|v| v == "1") {
            return Ok(Ratchet::default());
        }
        if let Ok(path) = std::env::var(FILE_ENV) {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
            return Ratchet::parse(&text).map_err(|e| format!("{path}: {e}"));
        }
        let mut dir: PathBuf = Path::new(manifest_dir).to_path_buf();
        loop {
            let candidate = dir.join(FILE);
            if candidate.is_file() {
                let text = std::fs::read_to_string(&candidate)
                    .map_err(|e| format!("{}: {e}", candidate.display()))?;
                return Ratchet::parse(&text).map_err(|e| format!("{}: {e}", candidate.display()));
            }
            if !dir.pop() {
                return Err(format!("{FILE} was not found above {manifest_dir}"));
            }
        }
    }

    /// Compares what the reports found with the entries of `endpoint`.
    ///
    /// # Errors
    /// Every new failure (a fault that is not listed) and every stale entry (listed, no longer observed), in words.
    pub fn check(&self, endpoint: &str, reports: &[Report]) -> Result<(), String> {
        let mut observed: BTreeSet<(String, String)> = BTreeSet::new();
        for r in reports.iter().filter(|r| r.endpoint == endpoint) {
            for f in r.faults() {
                observed.insert((r.scenario.name().to_string(), f.key()));
            }
        }
        let mine: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| e.endpoint == endpoint && e.applies_here())
            .collect();
        let mut problems: Vec<String> = reports
            .iter()
            .filter(|r| r.endpoint == endpoint)
            .filter_map(|r| match &r.outcome {
                super::Outcome::RigFailure(why) => Some(format!("rig failure: {endpoint} / {}: {why} (the rig could not drive the scenario; a scenario that does not apply is declared in the register's [[not_applicable]], never skipped in code)", r.scenario)),
                _ => None,
            })
            .collect();
        for (scenario, fault) in &observed {
            let listed = mine
                .iter()
                .any(|e| e.fault == *fault && (e.scenario == *scenario || e.scenario == "*"));
            if !listed {
                problems.push(format!(
                    "new failure: {endpoint} / {scenario}: {fault} (fix it, or record the instance in {FILE} with its blocks_tag)"
                ));
            }
        }
        for e in &mine {
            let seen = observed
                .iter()
                .any(|(s, f)| *f == e.fault && (e.scenario == *s || e.scenario == "*"));
            if !seen {
                problems.push(format!(
                    "stale ratchet entry {}: {endpoint} / {}: {} no longer happens (remove the entry; the ratchet only shrinks)",
                    e.instance, e.scenario, e.fault
                ));
            }
            if e.scenario != "*" && !Scenario::ALL.iter().any(|s| s.name() == e.scenario) {
                problems.push(format!(
                    "ratchet entry {} names an unknown scenario `{}`",
                    e.instance, e.scenario
                ));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("\n"))
        }
    }
}
