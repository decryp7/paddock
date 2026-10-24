//! Typed ids (ADR 0005): a value that exists is valid. Charset `[A-Za-z0-9._:/#-]`, 1 to 64 characters.

use std::fmt;

use crate::error::PlanError;

/// Longest id.
pub const MAX_ID_LEN: usize = 64;

pub(crate) fn clip(value: &str) -> String {
    value.chars().take(80).collect()
}

fn id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '#' | '-')
}

fn check(kind: &'static str, value: &str, ok: impl Fn(char) -> bool) -> Result<(), PlanError> {
    if value.is_empty() || value.len() > MAX_ID_LEN || !value.chars().all(ok) {
        return Err(PlanError::BadId {
            kind,
            value: clip(value),
        });
    }
    Ok(())
}

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident, $kind:literal, $ok:expr) => {
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            /// Parses the id, refusing anything outside the charset or length rule.
            pub fn new(value: &str) -> Result<Self, PlanError> {
                check($kind, value, $ok)?;
                Ok(Self(value.to_owned()))
            }

            /// The id as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(
    /// A requirement id from the tracker or Pacenotes: opaque text. `#` is reserved for [`AcceptanceRef`].
    RequirementId, "requirement id", |c| id_char(c) && c != '#'
);
id_type!(
    /// The id of a test case.
    CaseId, "case id", id_char
);
id_type!(
    /// The name of a screen a plan expects.
    ScreenName, "screen name", id_char
);
id_type!(
    /// The id of a plan.
    PlanId, "plan id", id_char
);
id_type!(
    /// The name of a data table.
    DataName, "data table name", id_char
);
id_type!(
    /// A free tag on a case.
    Tag, "tag", id_char
);
id_type!(
    /// A column of a data table, used as `{{data.COLUMN}}`.
    Column, "column name", |c| c.is_ascii_alphanumeric() || c == '_'
);
id_type!(
    /// The name of a secret: the only way a secret appears in a file. Resolved by the app, never by this crate.
    SecretRef, "secret name", |c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'
);

/// One acceptance criterion of a requirement: `REQ-142#AC-1`, the first entry of that requirement's `acceptance` list.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AcceptanceRef {
    requirement: RequirementId,
    number: u32,
}

impl AcceptanceRef {
    /// Parses `REQ#AC-n` with n at least 1.
    pub fn new(value: &str) -> Result<Self, PlanError> {
        let bad = || PlanError::BadId {
            kind: "acceptance reference (REQ#AC-n)",
            value: clip(value),
        };
        if value.len() > MAX_ID_LEN + 12 {
            return Err(bad());
        }
        let (req, ac) = value.split_once('#').ok_or_else(bad)?;
        let number = ac
            .strip_prefix("AC-")
            .and_then(|n| n.parse::<u32>().ok())
            .filter(|n| *n >= 1 && ac == format!("AC-{n}"))
            .ok_or_else(bad)?;
        Ok(Self {
            requirement: RequirementId::new(req).map_err(|_| bad())?,
            number,
        })
    }

    pub(crate) fn of(requirement: &RequirementId, number: u32) -> Self {
        Self {
            requirement: requirement.clone(),
            number,
        }
    }

    /// The requirement this criterion belongs to.
    pub fn requirement(&self) -> &RequirementId {
        &self.requirement
    }

    /// The one-based position in the requirement's `acceptance` list.
    pub fn number(&self) -> u32 {
        self.number
    }
}

impl fmt::Display for AcceptanceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#AC-{}", self.requirement, self.number)
    }
}

/// What a case or a step result links to: a whole requirement or one of its acceptance criteria.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ReqRef {
    /// `REQ-142`.
    Requirement(RequirementId),
    /// `REQ-142#AC-1`.
    Acceptance(AcceptanceRef),
}

impl ReqRef {
    /// Parses either form.
    pub fn new(value: &str) -> Result<Self, PlanError> {
        if value.contains('#') {
            AcceptanceRef::new(value).map(Self::Acceptance)
        } else {
            RequirementId::new(value).map(Self::Requirement)
        }
    }

    /// The requirement, whichever form this is.
    pub fn requirement(&self) -> &RequirementId {
        match self {
            Self::Requirement(r) => r,
            Self::Acceptance(a) => a.requirement(),
        }
    }
}

impl fmt::Display for ReqRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Requirement(r) => r.fmt(f),
            Self::Acceptance(a) => a.fmt(f),
        }
    }
}

/// A path relative to the result file: no `..`, no absolute form, no backslash, no empty or `.` part.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelPath(String);

/// Longest relative path.
pub const MAX_REL_PATH: usize = 1024;

impl RelPath {
    /// Parses the path, refusing anything that could leave the result file's folder.
    pub fn new(value: &str) -> Result<Self, PlanError> {
        let bad = |msg: &str| PlanError::BadField {
            field: format!("path `{}`", clip(value)),
            msg: msg.to_owned(),
        };
        if value.is_empty() || value.len() > MAX_REL_PATH {
            return Err(bad("must be 1 to 1024 characters"));
        }
        if value
            .chars()
            .any(|c| c == '\\' || c == '\0' || c.is_control())
        {
            return Err(bad("must use `/` and no control characters"));
        }
        if value.starts_with('/') || value.as_bytes().get(1) == Some(&b':') {
            return Err(bad("must be relative to the result file, not absolute"));
        }
        if value
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        {
            return Err(bad("must not contain `..`, `.` or empty parts"));
        }
        Ok(Self(value.to_owned()))
    }

    /// The path as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An RFC 3339 timestamp (`2026-10-25T08:30:00Z`, optional fraction, `Z` or `+hh:mm`), kept as text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Timestamp(String);

impl Timestamp {
    /// Parses the shape and the ranges of the fields.
    pub fn new(value: &str) -> Result<Self, PlanError> {
        let bad = || PlanError::BadField {
            field: format!("timestamp `{}`", clip(value)),
            msg: "expected RFC 3339, like 2026-10-25T08:30:00Z".to_owned(),
        };
        let b = value.as_bytes();
        let digits = |r: std::ops::Range<usize>| {
            b.get(r)
                .filter(|s| s.iter().all(u8::is_ascii_digit))
                .map(|s| s.iter().fold(0u32, |a, d| a * 10 + u32::from(d - b'0')))
        };
        let (y, mo, d, h, mi, s) = (
            digits(0..4),
            digits(5..7),
            digits(8..10),
            digits(11..13),
            digits(14..16),
            digits(17..19),
        );
        let (Some(_), Some(mo), Some(d), Some(h), Some(mi), Some(s)) = (y, mo, d, h, mi, s) else {
            return Err(bad());
        };
        if b.get(4) != Some(&b'-')
            || b.get(7) != Some(&b'-')
            || b.get(10) != Some(&b'T')
            || b.get(13) != Some(&b':')
            || b.get(16) != Some(&b':')
        {
            return Err(bad());
        }
        if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
            return Err(bad());
        }
        let mut rest = &value[19..];
        if let Some(frac) = rest.strip_prefix('.') {
            let n = frac.bytes().take_while(u8::is_ascii_digit).count();
            if n == 0 {
                return Err(bad());
            }
            rest = &frac[n..];
        }
        let zone_ok = rest == "Z"
            || (rest.len() == 6
                && rest.is_ascii()
                && matches!(rest.as_bytes()[0], b'+' | b'-')
                && rest.as_bytes()[3] == b':'
                && rest[1..3].bytes().all(|c| c.is_ascii_digit())
                && rest[4..6].bytes().all(|c| c.is_ascii_digit())
                && rest[1..3].parse::<u32>().is_ok_and(|v| v < 24)
                && rest[4..6].parse::<u32>().is_ok_and(|v| v < 60));
        if !zone_ok {
            return Err(bad());
        }
        Ok(Self(value.to_owned()))
    }

    /// The timestamp as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
