//! The typed error of this crate: a register or record that cannot be read.

use std::fmt;

/// A register that cannot be read as approval records. A broken register approves nothing (fail closed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Which record (for example `approved #2`), or empty for the whole text.
    pub at: String,
    /// What is wrong.
    pub what: String,
}

impl Problem {
    pub(crate) fn new(at: impl Into<String>, what: impl Into<String>) -> Problem {
        Problem {
            at: at.into(),
            what: what.into(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.at.is_empty() {
            f.write_str(&self.what)
        } else {
            write!(f, "{} {}", self.at, self.what)
        }
    }
}

impl std::error::Error for Problem {}
