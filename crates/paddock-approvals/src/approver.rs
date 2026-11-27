//! Who gives an approval, and what an entry claims about it.

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
