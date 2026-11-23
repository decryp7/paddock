//! Time-based conditions of the gate register (ER-41, user 2026-11-23): a review date that has passed never blocks an unrelated
//! push. A push warns; a push that touches that entry blocks; a release tag blocks; the scheduled job blocks; a reminder comes a
//! week before the date. The same rule holds for dependency advisories (`pre_push::deny`) and delete-by tags
//! (`docs/known-copies.toml`: a reached tag is a warning only, user 2026-11-11).
//!
//! Pure: the findings, the registers and today's date come in; the caller prints.

use super::evaluate::Finding;
use super::register::{Date, Register, stamps, upcoming};
use std::collections::BTreeSet;

/// How strict the run is about time-based conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A push: an expired entry blocks only when the push touches it.
    Push,
    /// A release tag or the scheduled job: every expired entry blocks.
    Strict,
}

/// Findings split by what they do to this run.
#[derive(Debug, Default, PartialEq)]
pub struct Split {
    /// Findings that fail the run.
    pub blocking: Vec<Finding>,
    /// Time-based findings that only warn.
    pub warnings: Vec<Finding>,
}

/// The keys of the entries this change touched: new against the base or different from it. With no base register (not measured),
/// every entry counts as touched, so an unknown base never lets an expired entry through (ADR 0089).
pub fn touched(reg: &Register, base: Option<&Register>) -> BTreeSet<String> {
    let now = stamps(reg);
    let Some(base) = base else {
        return now.into_keys().collect();
    };
    let before = stamps(base);
    now.into_iter().filter(|(k, s)| before.get(k) != Some(s)).map(|(k, _)| k).collect()
}

/// Splits `findings` for `mode`: a finding that is not time-based always blocks.
pub fn split(findings: Vec<Finding>, mode: Mode, touched: &BTreeSet<String>) -> Split {
    let mut out = Split::default();
    for f in findings {
        let warns = match (&f.expired, mode) {
            (Some(key), Mode::Push) => !touched.contains(key),
            _ => false,
        };
        if warns {
            out.warnings.push(f);
        } else {
            out.blocking.push(f);
        }
    }
    out
}

/// What a release tag refuses for time: every entry of the register whose review date has passed (ER-41: tags and releases block).
/// `None` text (no register) refuses nothing here; a register that does not parse refuses the tag, because no date can be read.
pub fn release_refusals(register_text: Option<&str>, today: Date) -> Vec<String> {
    let Some(text) = register_text else { return Vec::new() };
    let (reg, errors) = super::register::parse(text);
    if !errors.is_empty() {
        return vec![format!(
            "docs/gates.toml is not valid ({}), so no review date can be read",
            errors[0]
        )];
    }
    super::register::expired(&reg, today)
        .into_iter()
        .map(|e| format!("{}. Fix: {}", e.message, e.fix))
        .collect()
}

/// The reminder lines for entries that expire within `days` days: `::warning::` annotations, soonest first.
pub fn reminders(reg: &Register, today: Date, days: i64) -> Vec<String> {
    upcoming(reg, today, days)
        .into_iter()
        .map(|(key, on, left)| {
            format!(
                "::warning file=docs/gates.toml::C100 {key}: review_by {on} is in {left} day(s). Fix: fix the cause and remove the entry, or move the date with a `Gates-date:` trailer (approved)."
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gates::register::{expired, parse};

    const HEAD: &str = "version = 1\n[docs]\ninclude = [\"AGENTS.md\"]\nexclude = []\n";

    /// The C50 entry of docs/gates.toml with `review_by` and a `reason`.
    fn c50(review_by: &str, reason: &str) -> String {
        format!(
            "{HEAD}\n[[not_gated]]\nid = \"C50\"\nreason = \"{reason}\"\nowner = \"Dyno shell start (ADR 0029)\"\nsince = 2026-11-08\nreview_by = {review_by}\napproved = \"coordinator\"\n\n[[not_gated]]\nid = \"C51\"\nreason = \"report-only\"\nowner = \"o\"\nsince = 2026-11-08\nreview_by = 2027-01-07\napproved = \"coordinator\"\n"
        )
    }

    fn finding_of(reg: &Register, today: &str) -> Vec<Finding> {
        expired(reg, Date::parse(today).unwrap())
            .into_iter()
            .map(|e| Finding {
                file: Some("docs/gates.toml".into()),
                line: None,
                id: "register".into(),
                message: e.message,
                fix: e.fix,
                expired: Some(e.key),
            })
            .collect()
    }

    fn other_finding() -> Finding {
        Finding {
            file: None,
            line: None,
            id: "conformance".into(),
            message: "C999 is run by no reachable CI line".into(),
            fix: "wire it".into(),
            expired: None,
        }
    }

    #[test]
    fn an_expired_c50_warns_on_an_unrelated_push_and_blocks_a_push_that_edits_it() {
        // C50 expired on 2026-11-22; today is 2026-11-23.
        let (base, _) = parse(&c50("2026-11-22", "fails on the current tree"));
        let found = finding_of(&base, "2026-11-23");
        assert_eq!(found.len(), 1, "{found:?}");
        // An unrelated push: the register is the base's.
        let none = touched(&base, Some(&base));
        assert!(none.is_empty(), "{none:?}");
        let s = split(found.clone(), Mode::Push, &none);
        assert!(s.blocking.is_empty() && s.warnings.len() == 1, "{s:?}");
        assert!(s.warnings[0].warning().starts_with("::warning"), "{}", s.warnings[0].warning());
        // A push that edits C50 (its reason): blocked.
        let (edited, _) = parse(&c50("2026-11-22", "fails on the current tree; fix queued"));
        let hit = touched(&edited, Some(&base));
        assert!(hit.contains("C50") && !hit.contains("C51"), "{hit:?}");
        let s = split(finding_of(&edited, "2026-11-23"), Mode::Push, &hit);
        assert_eq!((s.blocking.len(), s.warnings.len()), (1, 0), "{s:?}");
        assert!(s.blocking[0].annotation().starts_with("::error"));
    }

    #[test]
    fn a_release_tag_and_the_scheduled_job_block_every_expired_entry() {
        let (base, _) = parse(&c50("2026-11-22", "x"));
        let s = split(finding_of(&base, "2026-11-23"), Mode::Strict, &touched(&base, Some(&base)));
        assert_eq!((s.blocking.len(), s.warnings.len()), (1, 0), "{s:?}");
    }

    #[test]
    fn an_entry_new_against_the_base_or_an_unknown_base_counts_as_touched() {
        let (now, _) = parse(&c50("2026-11-22", "x"));
        let (empty, _) = parse(HEAD);
        assert!(touched(&now, Some(&empty)).contains("C50"));
        // Not measured never lets an expired entry through.
        assert!(touched(&now, None).contains("C50"));
        let s = split(finding_of(&now, "2026-11-23"), Mode::Push, &touched(&now, None));
        assert_eq!(s.blocking.len(), 1, "{s:?}");
    }

    #[test]
    fn a_finding_that_is_not_time_based_blocks_in_every_mode() {
        for mode in [Mode::Push, Mode::Strict] {
            let s = split(vec![other_finding()], mode, &BTreeSet::new());
            assert_eq!((s.blocking.len(), s.warnings.len()), (1, 0), "{mode:?}");
        }
    }

    #[test]
    fn a_release_tag_is_refused_for_an_expired_entry_whoever_touched_it() {
        let text = c50("2026-11-22", "x");
        let day = Date::parse("2026-11-23").unwrap();
        let refused = release_refusals(Some(&text), day);
        assert_eq!(refused.len(), 1, "{refused:?}");
        assert!(refused[0].contains("C50") && refused[0].contains("has passed"), "{refused:?}");
        // Positive control: the day the entry is still valid, nothing is refused; an unreadable register refuses.
        assert!(release_refusals(Some(&text), Date::parse("2026-11-22").unwrap()).is_empty());
        assert_eq!(release_refusals(Some("not = toml = at all"), day).len(), 1);
        assert!(release_refusals(None, day).is_empty());
    }

    #[test]
    fn a_reminder_names_each_entry_that_expires_within_a_week_and_no_other() {
        let (r, _) = parse(&c50("2026-11-29", "x"));
        let lines = reminders(&r, Date::parse("2026-11-23").unwrap(), 7);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].starts_with("::warning") && lines[0].contains("C50") && lines[0].contains("in 6 day(s)"),
            "{lines:?}"
        );
        // Eight days away: no reminder yet. Already expired: the finding says so, not a reminder.
        assert!(reminders(&r, Date::parse("2026-11-21").unwrap(), 7).is_empty());
        assert!(reminders(&r, Date::parse("2026-11-30").unwrap(), 7).is_empty());
    }
}
