//! [`Position`], [`Span`] and [`Location`].

use serde::{Deserialize, Serialize};

use crate::path::RepoPath;

/// Why a [`Span`] is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpanError {
    /// A line or column is 0 (both are 1-based).
    #[error("lines and columns start at 1")]
    ZeroBased,
    /// The end is before the start.
    #[error("the span ends before it starts")]
    Reversed,
}

/// A point in a file: 1-based line, 1-based column counted in UTF-8 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    /// 1-based line.
    pub line: u32,
    /// 1-based column in UTF-8 bytes.
    pub column: u32,
}

impl Position {
    /// A position; `None` when a coordinate is 0.
    pub fn new(line: u32, column: u32) -> Option<Position> {
        (line >= 1 && column >= 1).then_some(Position { line, column })
    }
}

/// A range of text: `start <= end`, end exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawSpan")]
pub struct Span {
    start: Position,
    end: Position,
}

/// The wire form of a [`Span`]; reading it goes through [`Span::new`].
#[derive(Deserialize)]
struct RawSpan {
    start: Position,
    end: Position,
}

impl TryFrom<RawSpan> for Span {
    type Error = SpanError;
    fn try_from(raw: RawSpan) -> Result<Span, SpanError> {
        Span::new(raw.start, raw.end)
    }
}

impl Span {
    /// A span from two positions.
    pub fn new(start: Position, end: Position) -> Result<Span, SpanError> {
        if start.line == 0 || start.column == 0 || end.line == 0 || end.column == 0 {
            return Err(SpanError::ZeroBased);
        }
        if end < start {
            return Err(SpanError::Reversed);
        }
        Ok(Span { start, end })
    }

    /// A span covering whole lines `first..=last` (columns 1 to the end of `last`, shown as column 1 of the next line).
    pub fn lines(first: u32, last: u32) -> Result<Span, SpanError> {
        let start = Position { line: first, column: 1 };
        let end = Position {
            line: last.saturating_add(1),
            column: 1,
        };
        Span::new(start, end)
    }

    /// The first position.
    pub fn start(&self) -> Position {
        self.start
    }

    /// The position after the last.
    pub fn end(&self) -> Position {
        self.end
    }

    /// True when `other` lies inside this span.
    pub fn contains(&self, other: &Span) -> bool {
        self.start <= other.start && other.end <= self.end
    }
}

/// A place in the repository: a file and a span in it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    /// The file.
    pub path: RepoPath,
    /// The range in it.
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(l: u32, c: u32) -> Position {
        Position::new(l, c).unwrap()
    }

    #[test]
    fn positions_are_one_based() {
        assert!(Position::new(0, 1).is_none());
        assert!(Position::new(1, 0).is_none());
        assert_eq!(p(3, 4), Position { line: 3, column: 4 });
    }

    #[test]
    fn a_reversed_span_is_refused_and_an_empty_one_is_fine() {
        assert_eq!(Span::new(p(2, 1), p(1, 9)), Err(SpanError::Reversed));
        assert!(Span::new(p(2, 5), p(2, 5)).is_ok());
        assert_eq!(Span::new(Position { line: 0, column: 1 }, p(1, 1)), Err(SpanError::ZeroBased));
    }

    #[test]
    fn contains_and_lines() {
        let outer = Span::lines(1, 10).unwrap();
        assert!(outer.contains(&Span::lines(3, 4).unwrap()));
        assert!(!outer.contains(&Span::lines(9, 11).unwrap()));
        assert!(Span::lines(5, 4).is_ok(), "an empty line range");
    }

    #[test]
    fn reading_a_span_validates_it() {
        let bad = r#"{"start":{"line":3,"column":1},"end":{"line":2,"column":1}}"#;
        assert!(serde_json::from_str::<Span>(bad).is_err());
    }

    #[test]
    fn a_location_round_trips_through_json() {
        let loc = Location {
            path: RepoPath::parse("a/b.rs").unwrap(),
            span: Span::lines(1, 2).unwrap(),
        };
        let json = serde_json::to_string(&loc).unwrap();
        assert_eq!(serde_json::from_str::<Location>(&json).unwrap(), loc);
    }

    #[test]
    fn accessors_return_the_ends() {
        let s = Span::new(p(1, 2), p(3, 4)).unwrap();
        assert_eq!((s.start(), s.end()), (p(1, 2), p(3, 4)));
    }
}
