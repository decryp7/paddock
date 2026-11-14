//! Inline waivers, one mechanism for every check (ER-33): a comment `// conformance: <tag> — <reason>` on a site's line or the line
//! above waives that site. The engine recognises the comment ([`recognise`]) and lists every waived site in the report's `waived`
//! with the reason, the file and the line (`expires` is `inline`: there is no date), so suppressions stay visible. A check no
//! longer carries its own waiver regexes: it passes the lines it considers "near" the site and its tag, and builds its finding
//! through [`InlineWaiver::apply`]. The port of `conformance/src/util/inline-waiver.mjs`; the parity test compares the two.
//!
//! The marker, the tag, the dashes and the reason are on one line (spaces and tabs only between them): a dash at the end of a line
//! does not take the next line as its reason, and a list dash on the next line is no dash. A marker without a reason is not a waiver: the site stays a finding, with the check's own text.

use super::finding::Finding;
use super::jsre::{is_js_space, rx, units};

/// What the comment near a site says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineWaiver {
    /// No comment for this tag.
    No,
    /// The comment without a reason.
    NoReason,
    /// The comment with its reason.
    Reason(String),
}

/// What a file's comments are written in: Rust, shell (C120's `.sh` files) or YAML (workflows).
pub use super::lex::Kind;

/// `text` with everything but its comments blanked (newlines kept, so lines and columns stay), read off the language's real lexer or
/// parser (`lex`). A waiver marker in a string literal, a here-document or a block scalar is data, not a waiver (ER-33): [`CommentView`]
/// reads only this text. The port of `commentsOf`.
pub fn comments_of(text: &str, kind: Kind) -> String {
    super::lex::comments_only(text, kind)
}

/// The comments of one file, read by line: what a check asks to find out whether a site is waived.
pub struct CommentView {
    lines: Vec<String>,
    whole: String,
}

impl CommentView {
    /// Reads the comments of `text`.
    pub fn of(text: &str, kind: Kind) -> CommentView {
        let whole = comments_of(text, kind);
        CommentView {
            lines: whole.split('\n').map(str::to_string).collect(),
            whole,
        }
    }

    /// The waiver comment for `tag` on the 1-based `line` or the line above.
    pub fn waiver(&self, line: usize, tag: &str) -> InlineWaiver {
        let at = |n: usize| n.checked_sub(1).and_then(|i| self.lines.get(i)).map_or("", String::as_str);
        recognise(&format!("{}\n{}", at(line), at(line.wrapping_sub(1))), tag)
    }

    /// The waiver comment for `tag` anywhere in the file (a file-level marker).
    pub fn file(&self, tag: &str) -> InlineWaiver {
        recognise(&self.whole, tag)
    }
}

/// The waiver comment for `tag` in `near` (the site's line and the line above, as the check defines them).
pub fn recognise(near: &str, tag: &str) -> InlineWaiver {
    let tag = regex::escape(tag);
    let marker = rx(&format!(r"conformance:[ \t]*{tag}[ \t]*([—–-]+)"));
    for caps in marker.captures_iter(near) {
        let Some(run) = caps.get(1) else { continue };
        let dashes: Vec<usize> = run.as_str().char_indices().map(|(i, _)| run.start() + i).collect();
        // `[—–-]+` is greedy and gives dashes back one at a time, so the reason may start with a dash (`-- a` has a reason).
        for j in (1..=dashes.len()).rev() {
            let start = if j < dashes.len() {
                dashes[j]
            } else {
                let rest = &near[run.end()..];
                run.end() + (rest.len() - rest.trim_start_matches([' ', '\t']).len())
            };
            let rest = &near[start..];
            let Some(first) = rest.chars().next() else { continue };
            let group: String = rest
                .chars()
                .take_while(|c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
                .collect();
            if !is_js_space(first) && units(&group) >= 3 {
                return InlineWaiver::Reason(group);
            }
        }
    }
    if rx(&format!(r"conformance:[ \t]*{tag}")).is_match(near) {
        InlineWaiver::NoReason
    } else {
        InlineWaiver::No
    }
}

impl InlineWaiver {
    /// The finding for a site: `site` marked waived when the comment has a reason (the host lists it under `waived`), `bare()` when
    /// the comment has none (the check's "gives no reason" finding), `site` itself when there is no comment.
    pub fn apply(self, site: Finding, bare: impl FnOnce() -> Finding) -> Finding {
        match self {
            InlineWaiver::Reason(reason) => site.inline_waived(reason),
            InlineWaiver::NoReason => bare(),
            InlineWaiver::No => site,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reason_follows_the_dashes_on_the_same_line() {
        assert_eq!(
            recognise("x // conformance: t — because", "t"),
            InlineWaiver::Reason("because".into())
        );
        assert_eq!(recognise("conformance: t -- a", "t"), InlineWaiver::Reason("- a".into()));
        assert_eq!(recognise("conformance: t — ab", "t"), InlineWaiver::NoReason);
        assert_eq!(recognise("conformance: t", "t"), InlineWaiver::NoReason);
        assert_eq!(recognise("nothing", "t"), InlineWaiver::No);
        assert_eq!(recognise("conformance: other — because", "t"), InlineWaiver::No);
    }

    #[test]
    fn a_dash_at_the_end_of_a_line_does_not_take_the_next_line_as_its_reason() {
        assert_eq!(recognise("// conformance: t —\nsome code here", "t"), InlineWaiver::NoReason);
    }

    #[test]
    fn a_later_marker_with_a_reason_counts() {
        assert_eq!(
            recognise("conformance: t — a\nconformance: t — long reason", "t"),
            InlineWaiver::Reason("long reason".into())
        );
    }

    #[test]
    fn apply_picks_the_site_the_bare_finding_or_the_waived_site() {
        let site = Finding::error("C1", 1, "a.rs", 3, "site", "fix");
        let bare = || Finding::error("C1", 1, "a.rs", 3, "bare", "fix");
        assert_eq!(InlineWaiver::No.apply(site.clone(), bare).message, "site");
        assert_eq!(InlineWaiver::NoReason.apply(site.clone(), bare).message, "bare");
        let w = InlineWaiver::Reason("why".into()).apply(site, bare);
        assert_eq!((w.message.as_str(), w.inline_waiver.as_deref()), ("site", Some("why")));
    }
}
