//! A text format of a profile (an output template or a regex) and how its captures are taken from one line.
//!
//! Most text formats end in a greedy rest-of-line group, `(?P<message>.*)$`. The `regex` crate resolves capture groups
//! with a backtracker over the whole match, so that tail cost about half of the capture time on log lines (915 against
//! 470 ns a line on log4net lines, performance.md "Parse speed"). For a single line (no `\n`) the tail always matches
//! whatever its prefix matched, so the prefix alone decides every other group, in the same leftmost-first order: such a
//! format captures with its prefix and takes the group as the rest of the line, with identical results.

use regex::{Captures, Regex};
use regex_syntax::ast::{self, Ast};

pub struct TextFormat {
    pub regex: Regex,
    /// True when the timestamp carries no date (console-style templates).
    pub time_only: bool,
    /// The pattern without its trailing rest-of-line group, and that group's name (see the module).
    tail: Option<(Regex, String)>,
}

/// The captures of one line by a [`TextFormat`].
pub struct FormatCaptures<'f, 'l> {
    caps: Captures<'l>,
    /// The rest-of-line group: its name and its text.
    tail: Option<(&'f str, &'l str)>,
}

impl<'l> FormatCaptures<'_, 'l> {
    /// The text of the group `name`, if it took part in the match.
    pub fn name(&self, name: &str) -> Option<&'l str> {
        match self.tail {
            Some((n, text)) if n == name => Some(text),
            _ => self.caps.name(name).map(|m| m.as_str()),
        }
    }
}

impl TextFormat {
    /// A format of `regex` (compiled with the user-regex limits); the tail is split off when the pattern allows it.
    pub fn new(regex: Regex, time_only: bool) -> TextFormat {
        let tail = split_tail(regex.as_str()).and_then(|(prefix, name)| {
            // The prefix is part of a pattern that compiled within the limits, so it does too; if not, keep the whole.
            crate::user_regex::compile(&prefix, false).ok().map(|r| (r, name))
        });
        TextFormat { regex, time_only, tail }
    }

    /// The captures of `line` (one line, without its line break), as the whole pattern would give them.
    pub fn captures<'f, 'l>(&'f self, line: &'l str) -> Option<FormatCaptures<'f, 'l>> {
        match &self.tail {
            // A line break inside the line (or a `\r`, which `.` skips in CRLF mode) leaves the tail to the whole pattern.
            Some((prefix, name)) if !line.contains(['\n', '\r']) => {
                let caps = prefix.captures(line)?;
                let end = caps.get(0).map_or(0, |m| m.end());
                Some(FormatCaptures {
                    caps,
                    tail: Some((name.as_str(), &line[end..])),
                })
            }
            _ => self.regex.captures(line).map(|caps| FormatCaptures { caps, tail: None }),
        }
    }
}

/// `(prefix, name)` when `pattern` is a concatenation ending in a named group of a starred `.` followed by `$`; the
/// prefix is the pattern's text before that group.
fn split_tail(pattern: &str) -> Option<(String, String)> {
    let parsed = ast::parse::Parser::new().parse(pattern).ok()?;
    let Ast::Concat(concat) = &parsed else { return None };
    let [.., group, end] = concat.asts.as_slice() else { return None };
    let Ast::Assertion(a) = end else { return None };
    if !matches!(a.kind, ast::AssertionKind::EndLine | ast::AssertionKind::EndText) {
        return None;
    }
    let Ast::Group(g) = group else { return None };
    let ast::GroupKind::CaptureName { name, .. } = &g.kind else {
        return None;
    };
    let Ast::Repetition(rep) = g.ast.as_ref() else { return None };
    if !matches!(rep.op.kind, ast::RepetitionKind::ZeroOrMore) || !matches!(rep.ast.as_ref(), Ast::Dot(_)) {
        return None;
    }
    Some((pattern[..g.span.start.offset].to_string(), name.name.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(p: &str) -> TextFormat {
        TextFormat::new(Regex::new(p).unwrap(), false)
    }

    #[test]
    fn a_rest_of_line_group_is_split_off() {
        assert_eq!(
            split_tail(r"^(?P<a>\d+) (?P<message>.*)$"),
            Some((r"^(?P<a>\d+) ".into(), "message".into()))
        );
        assert_eq!(split_tail(r"^x(?<m>.*?)$"), Some(("^x".into(), "m".into())));
        // Not a tail: no end anchor, an unnamed or bounded group, an alternation at the top, an escaped bracket.
        for p in [
            r"^(?P<m>.*)",
            r"^(.*)$",
            r"^(?P<m>.+)$",
            r"^(?P<m>.{0,9})$",
            r"^a|(?P<m>.*)$",
            r"^(?P<m>\w*)$",
            r"^\\(?P<m>.*)\)$",
        ] {
            assert_eq!(split_tail(p), None, "{p}");
        }
    }

    #[test]
    fn split_captures_equal_the_whole_patterns() {
        let patterns = [
            r"^(?P<timestamp>\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}(?:[.,]\d{1,9})?)\s+\[(?P<Thread>[^\]]+)\]\s+(?P<level>[A-Za-z]+)\s+(?P<Logger>\S+)\s+(?:\[(?P<Ndc>[^\]]*)\]\s+)?-\s(?P<message>.*)$",
            r"(?P<level>[A-Z]+):? ?(?P<message>.*)$",
            r"^(?P<a>x*)(?P<b>x*?)(?P<message>.*)$",
            r"(?m)^(?P<a>[a-z]+)(?P<message>.*)$",
            r"(?mR)^(?P<a>[a-z]+)(?P<message>.*)$",
        ];
        let lines = [
            "2026-10-14 00:09:56,030 [56] INFO  Contoso.Billing.Export - Tariff cache refreshed host=WEB01",
            "2026-10-14 00:09:56,030 [56] INFO  Contoso.Billing.Export [ndc] - with ndc",
            "2026-10-14 00:09:56,030 [56] INFO  Contoso - ",
            "   at Contoso.Orders.Process() in Order.cs:line 42",
            "ERROR: disk full",
            "xxxx rest",
            "",
            "ünïcode ERROR öü",
            "two\nlines",
            "carriage\rreturn",
        ];
        for p in patterns {
            let f = format(p);
            assert!(f.tail.is_some(), "{p} has a tail");
            let names: Vec<&str> = f.regex.capture_names().flatten().collect();
            for l in lines {
                let whole = f
                    .regex
                    .captures(l)
                    .map(|c| names.iter().map(|n| c.name(n).map(|m| m.as_str())).collect::<Vec<_>>());
                let split = f.captures(l).map(|c| names.iter().map(|n| c.name(n)).collect::<Vec<_>>());
                assert_eq!(whole, split, "{p} on {l:?}");
            }
        }
    }

    #[test]
    fn formats_without_a_tail_use_the_whole_pattern() {
        let f = format(r"^(?P<level>[A-Z]+) (?P<message>\S+)");
        assert!(f.tail.is_none());
        assert_eq!(f.captures("WARN disk full").unwrap().name("message"), Some("disk"));
        assert!(f.captures("warn").is_none());
    }
}
