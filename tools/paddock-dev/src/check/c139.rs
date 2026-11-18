//! C139: an e2e spec that switches off `paddock/e2e-ready-reads` says why, and the exceptions only shrink. The ESLint rule (an error in
//! every app's and the ui's e2e lint block, so the existing `--max-warnings 0` lint steps block a new unready read) has one escape, ESLint's
//! own `// eslint-disable-next-line paddock/e2e-ready-reads -- <why>`. ESLint cannot require the description or count the directives; this
//! check does (ER-21 class "a read on 'it is in the DOM'", ER-46: a list of exceptions only shrinks). `reportUnusedDisableDirectives` (ESLint,
//! on by default as a warning, which the lint steps block) removes a directive whose read was fixed.
//!
//! Flagged, one finding per line, in `.ts` files of an `e2e` folder of an app or the ui:
//! - a `eslint-disable` directive that names no rule (it would switch off everything: say which rule);
//! - the block form (`/* eslint-disable paddock/e2e-ready-reads */`, no `-next-line` or `-line`): it covers any number of reads;
//! - a directive for the rule without a description (`-- ` and at least three characters);
//! - a directive that the allow list `conformance/c139-allow.json` does not name, or that is beyond its entry's `count`.
//!
//! The list is read by the shared [`super::allow_list`] (path, site `paddock/e2e-ready-reads`, reason, count): it only shrinks against the
//! merge base (`baseline::growth`, as C53 and C115 use it; "not measured" is an error in CI, ADR 0089), a stale entry is a warning.
//! Rust host; `conformance/src/checks/C139-e2e-ready-directives.mjs` is the port and `tests/it/check_c139.rs` compares them.

use super::allow_list::{self, Admit, Entry, Spec};
use super::finding::Finding;
use super::jsre::rx;
use super::{Check, CheckOutput, Control, Ctx};
use regex::Regex;
use std::sync::LazyLock;

/// The allow list both hosts read.
pub const ALLOW: &str = "conformance/c139-allow.json";
/// The rule the directive names, and the `site` of an allow entry.
pub const RULE_ID: &str = "paddock/e2e-ready-reads";

/// The check's own files: they name the pattern, their invented lines are test data.
pub const IMPLEMENTATION: [&str; 5] = [
    "conformance/src/checks/C139-e2e-ready-directives.mjs",
    "conformance/test/c139-e2e-ready-directives.test.mjs",
    "tools/paddock-dev/src/check/c139.rs",
    "tools/paddock-dev/tests/it/check_c139.rs",
    ALLOW,
];

static E2E_TS: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:apps/[^/]+/app|ui)/e2e/.+\.ts$"));
static NOT_READ: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)(?:node_modules|[^/]*-snapshots)/"));
static DIRECTIVE: LazyLock<Regex> = LazyLock::new(|| rx(r"(?://|/\*)\s*eslint-disable(-next-line|-line)?\b(.*)$"));

const FIX_RULES: &str = "Name the rule: `// eslint-disable-next-line paddock/e2e-ready-reads -- <why>`. Better, wait with a ready helper from @decryp7/paddock-e2e (ui/projects/paddock-e2e/src/ready.ts) so the read needs no exception.";
const FIX_BLOCK: &str = "Use the one-line form, `// eslint-disable-next-line paddock/e2e-ready-reads -- <why>`: a block covers any number of reads.";
const FIX_WHY: &str = "Say why the read is immediate by design: `// eslint-disable-next-line paddock/e2e-ready-reads -- <why>` (at least three characters after the dashes).";
const FIX_LIST: &str = "Wait with a ready helper from @decryp7/paddock-e2e (ui/projects/paddock-e2e/src/ready.ts) instead. An exception has to be listed in conformance/c139-allow.json (path, site `paddock/e2e-ready-reads`, reason, count), and the list only shrinks: a new entry needs the user's approval (ER-46).";

/// Whether `path` is a spec file this check reads.
pub fn readable(path: &str) -> bool {
    E2E_TS.is_match(path) && !NOT_READ.is_match(path) && !IMPLEMENTATION.contains(&path)
}

/// What a directive line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Names no rule.
    Unrestricted,
    /// The block form for the rule.
    Block,
    /// A line form for the rule without a description.
    NoReason,
    /// A line form for the rule with a description: counted against the allow list.
    Counted,
}

/// `(line, kind)` of every `eslint-disable` directive in `text` that names the rule or no rule at all.
pub fn sites(path: &str, text: &str) -> Vec<(usize, Kind)> {
    if !readable(path) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        // A CRLF file: the carriage return is not part of the line (JavaScript's `.` stops at it, so both hosts drop it first).
        let line = line.trim_end_matches('\r');
        let Some(caps) = DIRECTIVE.captures(line) else { continue };
        let line_form = caps.get(1).is_some();
        let rest = caps.get(2).map_or("", |m| m.as_str());
        let (rules, why) = match rest.split_once(" -- ") {
            Some((r, w)) => (r, w),
            None => (rest.trim_end().trim_end_matches("*/").trim_end(), ""),
        };
        let rules = rules.trim_end().trim_end_matches("*/");
        let names: Vec<&str> = rules.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        let kind = if names.is_empty() {
            Kind::Unrestricted
        } else if !names.contains(&RULE_ID) {
            continue;
        } else if !line_form {
            Kind::Block
        } else if why.trim().encode_utf16().count() >= 3 {
            Kind::Counted
        } else {
            Kind::NoReason
        };
        out.push((i + 1, kind));
    }
    out
}

/// Reads the allow list: its entries, or why it cannot be read (the shared parser, [`super::allow_list`]).
pub fn parse_allow(text: &str) -> Result<Vec<Entry>, String> {
    allow_list::parse(text, ALLOW)
}

static SPEC: Spec = Spec {
    id: "C139",
    rule: 49,
    file: ALLOW,
    unseen: "an e2e spec",
    restore: "Restore conformance/c139-allow.json: {\"allow\": [{\"path\": \"...\", \"site\": \"paddock/e2e-ready-reads\", \"reason\": \"...\", \"count\": 1}]}.",
};

/// The check.
pub struct C139;

impl Check for C139 {
    fn id(&self) -> &'static str {
        "C139"
    }

    fn rule(&self) -> u32 {
        49
    }

    fn title(&self) -> &'static str {
        "an e2e spec that disables paddock/e2e-ready-reads says why, and the exceptions only shrink"
    }

    fn scope(&self) -> super::CheckScope {
        super::CheckScope::Root
    }

    fn allow_list(&self) -> Option<&'static allow_list::Spec> {
        Some(&SPEC)
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    ALLOW,
                    "{\"allow\":[{\"path\":\"ui/e2e/ok.spec.ts\",\"site\":\"paddock/e2e-ready-reads\",\"reason\":\"the read is immediate by design\"}]}",
                ),
                ("ui/e2e/ok.spec.ts", "// eslint-disable-next-line paddock/e2e-ready-reads -- immediate by design\nconst a = 1;\n"),
                ("ui/e2e/bad.spec.ts", "const b = 1;\n// eslint-disable-next-line paddock/e2e-ready-reads -- not listed anywhere\nconst c = 2;\n"),
                ("ui/e2e/other.spec.ts", "// eslint-disable-next-line no-console -- another rule is not this check's\n"),
                ("ui/src/not-e2e.ts", "// eslint-disable-next-line paddock/e2e-ready-reads -- outside an e2e folder\n"),
            ],
            expect: vec![("ui/e2e/bad.spec.ts", 2)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut allow = match allow_list::read(ctx, &SPEC, allow_list::entries_only(ALLOW)) {
            Ok((allow, ())) => allow,
            Err(broken) => return *broken,
        };
        let mut findings = Vec::new();
        for f in ctx.files().iter() {
            if !readable(f) {
                continue;
            }
            let text = match allow.read(ctx, f) {
                Ok(text) => text,
                Err(unread) => {
                    findings.push(*unread);
                    continue;
                }
            };
            for (line, kind) in sites(f, &text) {
                let finding = match kind {
                    Kind::Unrestricted => Finding::error("C139", 49, f, line, "an eslint-disable in an e2e spec that names no rule switches off every rule on its line".to_string(), FIX_RULES),
                    Kind::Block => Finding::error("C139", 49, f, line, "a block eslint-disable of paddock/e2e-ready-reads covers any number of reads".to_string(), FIX_BLOCK),
                    Kind::NoReason => Finding::error("C139", 49, f, line, "a disable of paddock/e2e-ready-reads without a description".to_string(), FIX_WHY),
                    Kind::Counted => match allow.admit(f, |site| site == RULE_ID) {
                        Admit::Allowed => continue,
                        Admit::Beyond => Finding::error("C139", 49, f, line, "a disable of paddock/e2e-ready-reads beyond the count of its allow entry".to_string(), FIX_LIST),
                        Admit::Unlisted => Finding::error("C139", 49, f, line, "a disable of paddock/e2e-ready-reads that the allow list does not name".to_string(), FIX_LIST),
                    },
                };
                findings.push(finding);
            }
        }
        allow.output(ctx, findings)
    }
}
