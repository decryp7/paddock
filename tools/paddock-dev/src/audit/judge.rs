//! What a change adds: the pure part of the audit. A finding the merge base has too was decided there, so it only warns; a finding
//! the change introduces fails; a run that did not look is not measured and never counts as a pass (ER-33). No list of accepted
//! findings is kept: main's findings are measured again on the merge base's lockfiles each time, so no list can grow silently (ER-46).

use super::parse::{Finding, Measure};
use std::collections::BTreeSet;

/// The judgement of one lockfile's audit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    /// Findings the change introduces (every finding, in a nightly run on main).
    pub new: Vec<Finding>,
    /// Findings the merge base has too.
    pub existing: Vec<Finding>,
    /// `(source, why)` of an audit that did not look, on the change or on the merge base.
    pub not_measured: Vec<(String, String)>,
}

impl Verdict {
    /// The verdict of a nightly run on main: every finding is new, nothing is compared.
    pub fn nightly(source: &str, head: &Measure) -> Verdict {
        match head {
            Measure::Found(f) => Verdict {
                new: f.clone(),
                ..Verdict::default()
            },
            Measure::NotMeasured(why) => Verdict::missing(source, why),
        }
    }

    /// The verdict of a train or branch run: `base` is asked only when the change has findings (nothing to compare otherwise).
    pub fn compare(source: &str, head: &Measure, base: &mut dyn FnMut() -> Measure) -> Verdict {
        let found = match head {
            Measure::Found(f) if f.is_empty() => return Verdict::default(),
            Measure::Found(f) => f,
            Measure::NotMeasured(why) => return Verdict::missing(source, why),
        };
        match base() {
            Measure::NotMeasured(why) => Verdict::missing(
                source,
                &format!(
                    "the merge base could not be audited, so the {} finding(s) cannot be compared: {why}",
                    found.len()
                ),
            ),
            Measure::Found(b) => {
                let on_base: BTreeSet<_> = b.iter().map(Finding::key).collect();
                let (existing, new): (Vec<Finding>, Vec<Finding>) = found.iter().cloned().partition(|f| on_base.contains(&f.key()));
                Verdict {
                    new,
                    existing,
                    not_measured: Vec::new(),
                }
            }
        }
    }

    fn missing(source: &str, why: &str) -> Verdict {
        Verdict {
            not_measured: vec![(source.to_string(), why.to_string())],
            ..Verdict::default()
        }
    }

    /// Adds another lockfile's verdict.
    pub fn merge(&mut self, other: Verdict) {
        self.new.extend(other.new);
        self.existing.extend(other.existing);
        self.not_measured.extend(other.not_measured);
    }

    /// Whether the run passes when it is enforced: nothing new, and everything measured.
    pub fn clean(&self) -> bool {
        self.new.is_empty() && self.not_measured.is_empty()
    }
}

/// Where the decision on a finding is made, for the text of its annotation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Every night on main.
    Nightly,
    /// A train or branch against its merge base.
    Compare,
}

fn exception_step(f: &Finding) -> String {
    if f.source.starts_with("cargo ") {
        let dir = f.source.rsplit_once('/').map_or(".", |(d, _)| d.trim_start_matches("cargo "));
        format!(
            "decide it: add {} to `ignore` in {dir}/deny.toml with a reason and a review date (needs the user's approval, ER-46), or fix it with `cargo update -p {}` in {dir}",
            f.id, f.package
        )
    } else {
        format!(
            "decide it: record an exception for {} with a reason and a review date where the npm audit exceptions live (needs the user's approval, ER-46), or fix it by updating {} in package-lock.json",
            f.id, f.package
        )
    }
}

fn describe(f: &Finding) -> String {
    let version = if f.version.is_empty() {
        String::new()
    } else {
        format!(" {}", f.version)
    };
    format!(
        "{} hits {}{version} ({}) in {}, reached by {}",
        f.id, f.package, f.title, f.source, f.path
    )
}

/// The annotation lines of a verdict. `enforce` false is report-only (ER-41): a finding that would fail becomes a warning that
/// says so.
pub fn lines(v: &Verdict, mode: Mode, enforce: bool) -> Vec<String> {
    let level = if enforce { "error" } else { "warning" };
    let tag = if enforce { "" } else { "report-only, would fail: " };
    let mut out = Vec::new();
    for f in &v.new {
        let what = match mode {
            Mode::Nightly => "is on main",
            Mode::Compare => "is new in this change (main does not have it)",
        };
        out.push(format!(
            "::{level}::{tag}{}; it {what}. Next step: {}.",
            describe(f),
            exception_step(f)
        ));
    }
    for f in &v.existing {
        out.push(format!(
            "::warning::{}; it is already present on main, decided there; this change did not add it, so it does not fail here.",
            describe(f)
        ));
    }
    for (source, why) in &v.not_measured {
        out.push(format!(
            "::{level}::{tag}audit of {source} NOT MEASURED: {why}. A missing measurement never counts as a pass. Next step: run `./dev audit` again where the tool and the network work."
        ));
    }
    out
}
