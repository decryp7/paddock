//! Why targets run and why records are not written (ER-27): the counts a log line states, per reason, so a `held back 50` or a
//! `reused 0 of 61` always carries its cause. Pure: the lines are made from the decisions of [`super::decide`] and nothing else.
//!
//! Run 9335 (train aa): every Linux and Windows record step printed `recorded 0 packages (61 in the log; 14 held back: a trusted record,
//! or no definite answer, ...)` and no cause, because the store lookup had timed out (`curl: (28)` after 5 s) and the record step never
//! said so. The reasons below are the answer to that line, the way [`super::untrusted_summary`] is for untrusted records.

use super::{Decision, Why};

/// The decisions of one lookup counted by reason.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Tally {
    /// A trusted record covers the target ([`Decision::Reuse`]).
    pub reused: usize,
    /// No record under the key.
    pub no_record: usize,
    /// Its crate reads inputs the graph cannot resolve.
    pub blind: usize,
    /// Listed in `no_test_cache`.
    pub never: usize,
    /// Uncommitted changes under its inputs.
    pub dirty: usize,
    /// A record older than the limit.
    pub too_old: usize,
    /// A record that failed the trust check.
    pub untrusted: usize,
    /// A record whose trust check could not answer.
    pub undecided: usize,
    /// The lookup did not answer, or reuse is off for the run.
    pub off: usize,
    /// The first cause of the targets counted in `off`.
    pub cause: Option<String>,
}

impl Tally {
    /// The count of `decisions`.
    pub fn of<'a>(decisions: impl IntoIterator<Item = &'a Decision>) -> Tally {
        let mut t = Tally::default();
        for d in decisions {
            match d {
                Decision::Reuse(_) => t.reused += 1,
                Decision::Run(Why::Miss) => t.no_record += 1,
                Decision::Run(Why::Blind(_)) => t.blind += 1,
                Decision::Run(Why::Never) => t.never += 1,
                Decision::Run(Why::Dirty) => t.dirty += 1,
                Decision::Run(Why::TooOld) => t.too_old += 1,
                Decision::Run(Why::Untrusted(_)) => t.untrusted += 1,
                Decision::Run(Why::Undecided(_)) => t.undecided += 1,
                Decision::Run(Why::Off(c)) => {
                    t.off += 1;
                    t.cause.get_or_insert_with(|| c.clone());
                }
            }
        }
        t
    }

    /// How many targets are counted.
    pub fn total(&self) -> usize {
        self.reused + self.ran()
    }

    /// How many were not reused.
    pub fn ran(&self) -> usize {
        self.no_record + self.blind + self.never + self.dirty + self.too_old + self.untrusted + self.undecided + self.off
    }

    fn cause_text(&self) -> String {
        self.cause.as_ref().map_or_else(String::new, |c| format!(" ({c})"))
    }

    /// `no record 3, blind 1, reuse off 2 (curl failed: ...)`: the reasons targets ran, the non-zero ones; empty when all were reused.
    pub fn ran_because(&self) -> String {
        let parts = [
            (self.no_record, "no record"),
            (self.blind, "blind"),
            (self.never, "never cached"),
            (self.dirty, "dirty"),
            (self.too_old, "record too old"),
            (self.untrusted, "record untrusted"),
            (self.undecided, "record trust undecided"),
        ];
        let mut out: Vec<String> = parts.iter().filter(|(n, _)| *n > 0).map(|(n, w)| format!("{w} {n}")).collect();
        if self.off > 0 {
            out.push(format!("reuse off {}{}", self.off, self.cause_text()));
        }
        out.join(", ")
    }

    /// `reused 14 of 61; ran because: no record 47`: the one line of a lookup (the shadow report of a plan, a record step, a local lookup).
    pub fn lookup_line(&self) -> String {
        match self.ran() {
            0 => format!("reused {} of {}", self.reused, self.total()),
            _ => format!("reused {} of {}; ran because: {}", self.reused, self.total(), self.ran_because()),
        }
    }

    /// `2 already trusted, 1 undecided, 3 blind or never cached, 4 store lookup failed (cause)`: why passes were not stored, over the
    /// decisions of the targets whose pass was held back (non-zero reasons only).
    pub fn held_because(&self) -> String {
        let blind = self.blind + self.never;
        let parts = [
            (self.reused, "already trusted"),
            (self.undecided, "undecided (no definite answer)"),
            (blind, "blind or never cached"),
            (self.dirty, "dirty"),
        ];
        let mut out: Vec<String> = parts.iter().filter(|(n, _)| *n > 0).map(|(n, w)| format!("{n} {w}")).collect();
        if self.off > 0 {
            out.push(format!("{} store lookup failed{}", self.off, self.cause_text()));
        }
        out.join(", ")
    }

    /// `held back 3: 1 already trusted, 2 store lookup failed (cause)`.
    pub fn held_line(&self) -> String {
        match self.total() {
            0 => "held back 0".to_string(),
            n => format!("held back {n}: {}", self.held_because()),
        }
    }
}

/// The plan's line for one class, workspace and filter: the counts first (a host leg's filterset is hundreds of characters, run 9341), the
/// filter after them, cut to a short head with its length.
pub fn plan_line(class: &str, workspace: &str, filter: &str, tally: &Tally) -> String {
    let scope = match filter.chars().count() {
        0 => "whole package".to_string(),
        n if n <= 60 => format!("filter `{filter}`"),
        n => format!("filter `{}...` ({n} characters)", filter.chars().take(40).collect::<String>()),
    };
    format!("test-cache plan: {class} {workspace}: {} ({scope})", tally.lookup_line())
}
