//! Who gives an approval, and what an entry claims about it.

use crate::verdict::Verdict;

/// Who approved. A rule says which approver an exception needs; the user's approval satisfies every rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Approver {
    /// The coordinator (an agent role): enough for the register's ordinary entries.
    Coordinator,
    /// The user: needed to take an operating system away, to grow a shrink-only list and for a ratchet.
    User,
}

impl Approver {
    /// The word a register writes for this approver.
    pub fn as_str(self) -> &'static str {
        match self {
            Approver::Coordinator => "coordinator",
            Approver::User => "user",
        }
    }

    /// The approver a register word names, if it names one.
    pub fn parse(word: &str) -> Option<Approver> {
        match word {
            "coordinator" => Some(Approver::Coordinator),
            "user" => Some(Approver::User),
            _ => None,
        }
    }

    /// Whether an approval by `self` satisfies a rule that needs `needed`.
    pub fn satisfies(self, needed: Approver) -> bool {
        self == Approver::User || needed == Approver::Coordinator
    }
}

/// What a record says about who approved it: parsed once, so a verdict never re-reads a string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// The record has no approval field.
    Absent,
    /// The record names a known approver.
    By(Approver),
    /// The record names something else (the text as written).
    Unrecognised(String),
}

impl Claim {
    /// The claim of a record that carries its approver as one word (a JSON string, a TOML string read as text, a trailer's word): no
    /// word, or an empty one, is [`Claim::Absent`].
    pub fn from_word(word: Option<&str>) -> Claim {
        match word.map(str::trim).filter(|w| !w.is_empty()) {
            None => Claim::Absent,
            Some(w) => {
                Approver::parse(w).map_or_else(|| Claim::Unrecognised(w.to_string()), Claim::By)
            }
        }
    }

    /// The verdict on this claim for a record that `needs` an approver: the ONE judgement every approval kind shares (user decision
    /// 125). `field` names the record's field in the message and `line_to_add` is what the approver records when there is no claim.
    pub fn verdict(&self, field: &str, needs: Approver, line_to_add: impl Into<String>) -> Verdict {
        match self {
            Claim::Absent => Verdict::Missing {
                line_to_add: line_to_add.into(),
            },
            Claim::Unrecognised(v) => Verdict::Mismatch {
                why: format!("{field} = {v:?} must be \"coordinator\" or \"user\""),
            },
            Claim::By(a) if a.satisfies(needs) => Verdict::Approved,
            Claim::By(a) => Verdict::Mismatch {
                why: format!(
                    "{field} = \"{}\" but this needs the {}",
                    a.as_str(),
                    needs.as_str()
                ),
            },
        }
    }

    pub(crate) fn from_value(value: Option<&toml::Value>) -> Claim {
        match value {
            None => Claim::Absent,
            Some(toml::Value::String(s)) => {
                Approver::parse(s).map_or_else(|| Claim::Unrecognised(s.clone()), Claim::By)
            }
            Some(other) => Claim::Unrecognised(other.to_string()),
        }
    }
}
