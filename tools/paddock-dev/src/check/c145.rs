//! C145: `docs/lessons.md` keeps the format the family relies on. The file is merged with git's union driver (`.gitattributes`: two
//! branches that both add a section merge without a conflict), and a union merge works on lines: twice it dropped the blank line
//! between two sections and left a `## ` heading directly under the last line of the section above (9ec43be3c, b9bf5fc4b), so the
//! heading rendered as text and the anchor links of rules.md broke. Nothing noticed, because nothing read the file's shape.
//!
//! Flagged, one finding per site, outside fenced code blocks:
//! - `glued`: a `## ` heading whose line above is not blank (the union-merge defect);
//! - `title`: the file's first line is not the one `# ` title, or a second `# ` heading follows;
//! - `duplicate`: the same `## ` heading twice (a union merge that kept both sides of an edited section);
//! - `final-newline`: the file does not end in exactly one newline (a union merge glues the last line of one side to the next).
//!
//! The format is read from the file's own habit (lessons.md and `.gitattributes` state no further rule): a heading `##` is followed by
//! its text on the next line, with no blank line, so that is not flagged. An absent file on a whole-tree run is a not-measured error.
//! The merge check (`./dev merge check`) does not flag it at merge time: it diffs a merge against its parents for stale reverts, and
//! the defect is only visible in the merged text, which this check reads on the combined tree in the conformance job that every
//! train member's gate and the train run (ER-8, ER-41).
//!
//! **Report-only (warnings) until its first green CI run on the real tree (ER-41), recorded in docs/gates.toml.**

use super::finding::Finding;
use super::outcome::Outcome;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use std::collections::BTreeMap;

/// The file the check reads.
pub const LESSONS: &str = "docs/lessons.md";
const FIX: &str =
    "Put one blank line before the heading (a union merge drops it: re-read the section boundary after every merge of docs/lessons.md).";

/// One format defect: its 1-based line, kind and message.
#[derive(Debug, PartialEq, Eq)]
pub struct Site {
    /// The 1-based line.
    pub line: usize,
    /// `glued`, `title`, `duplicate` or `final-newline`.
    pub kind: &'static str,
    /// What is wrong.
    pub message: String,
}

/// Every format defect in the text of a lessons file.
pub fn lessons_sites(text: &str) -> Vec<Site> {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut fence: Option<&str> = None;
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    let mut title_seen = false;
    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        // A fenced block opens and closes on a line that starts with the same three characters (``` or ~~~).
        for mark in ["```", "~~~"] {
            if line.starts_with(mark) {
                fence = match fence {
                    None => Some(mark),
                    Some(open) if open == mark => None,
                    other => other,
                };
            }
        }
        if fence.is_some() || line.starts_with("```") || line.starts_with("~~~") {
            continue;
        }
        if line.starts_with("# ") {
            if title_seen {
                out.push(Site {
                    line: n,
                    kind: "title",
                    message: format!("a second title (`{line}`): the file has one `# ` title, on its first line"),
                });
            }
            title_seen = true;
        } else if line.starts_with("## ") {
            if i > 0 && !lines[i - 1].trim().is_empty() {
                out.push(Site {
                    line: n,
                    kind: "glued",
                    message: format!("the heading `{line}` is not preceded by a blank line (a union merge dropped it)"),
                });
            }
            if let Some(first) = seen.insert(line, n) {
                out.push(Site {
                    line: n,
                    kind: "duplicate",
                    message: format!(
                        "the heading `{line}` appears twice (first at line {first}): a union merge kept both sides of one section"
                    ),
                });
            }
        }
    }
    if !lines.first().is_some_and(|l| l.starts_with("# ")) {
        out.push(Site {
            line: 1,
            kind: "title",
            message: "the first line is not the `# ` title: the file starts with one `# ` heading".to_string(),
        });
    }
    // The text ends in exactly one newline: `split` gives one empty last element for it.
    let n = lines.len();
    if lines.last() != Some(&"") {
        out.push(Site {
            line: n,
            kind: "final-newline",
            message: "the file does not end in a newline (a union merge glues the last line of one side to the next)".to_string(),
        });
    } else if n >= 2 && lines[n - 2].trim().is_empty() {
        out.push(Site {
            line: n - 1,
            kind: "final-newline",
            message: "the file ends in more than one newline (an empty last line)".to_string(),
        });
    }
    out
}

/// The check.
pub struct C145;

impl Check for C145 {
    fn id(&self) -> &'static str {
        "C145"
    }

    fn rule(&self) -> u32 {
        8
    }

    fn title(&self) -> &'static str {
        "docs/lessons.md keeps its format: a blank line before every heading, one title, no repeated heading"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (LESSONS, "# Lessons\n\n## One\n- a\n## Glued\n- b\n\n```\n## in a fence\n```\n"),
                ("docs/other.md", "x\n## not read\n"),
            ],
            expect: vec![(LESSONS, 5)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let in_run = ctx.files().iter().any(|f| f == LESSONS);
        let text = if in_run { ctx.read(LESSONS) } else { None };
        let mut findings = Vec::new();
        let mut probes = 0;
        match text {
            Some(text) => {
                probes = 1;
                for s in lessons_sites(&text) {
                    findings.push(Finding::warn("C145", 8, LESSONS, s.line, s.message, FIX));
                }
            }
            None if in_run => findings.push(
                Finding::error(
                    "C145",
                    8,
                    LESSONS,
                    1,
                    "not measured: docs/lessons.md could not be read".to_string(),
                    "Make the file readable and run the check again.",
                )
                .unwaivable()
                .with_outcome(Outcome::NotMeasured),
            ),
            None => {}
        }
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: true,
                detail: "docs/lessons.md is read when the run includes it".to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<String> {
        lessons_sites(text).iter().map(|s| format!("{}:{}", s.line, s.kind)).collect()
    }

    #[test]
    fn a_heading_under_a_non_blank_line_is_glued() {
        // the shape of 9ec43be3c and b9bf5fc4b: the section above ends, the heading follows with no blank line
        assert_eq!(kinds("# T\n\n## A\n- x\n## B\n- y\n"), ["5:glued"]);
        assert_eq!(kinds("# T\n\n## A\n- x\n\n## B\n- y\n"), Vec::<String>::new());
        assert_eq!(kinds("# T\n\n## A\n- x\r\n## B\r\n- y\r\n"), ["5:glued"]);
    }

    #[test]
    fn a_heading_directly_followed_by_its_text_is_the_file_habit() {
        assert!(kinds("# T\n\n## A\n- x\n\n## B\ntext\n").is_empty());
    }

    #[test]
    fn headings_in_fenced_code_are_not_headings() {
        assert!(kinds("# T\n\n```md\ntext\n## not a heading\n```\n\n~~~\nx\n## nor this\n~~~\n").is_empty());
    }

    #[test]
    fn the_title_is_the_first_line_and_the_only_h1() {
        assert_eq!(kinds("## A\n\ntext\n"), ["1:title"]);
        assert_eq!(kinds("# T\n\n# Again\n"), ["3:title"]);
        assert_eq!(kinds("\n# T\n"), ["1:title"]);
    }

    #[test]
    fn a_repeated_section_heading_is_a_duplicate() {
        assert_eq!(kinds("# T\n\n## A\n\nx\n\n## A\n\ny\n"), ["7:duplicate"]);
    }

    #[test]
    fn the_file_ends_in_exactly_one_newline() {
        assert_eq!(kinds("# T\n\n## A\n\nx"), ["5:final-newline"]);
        assert_eq!(kinds("# T\n\n## A\n\nx\n\n"), ["6:final-newline"]);
        assert!(kinds("# T\n\n## A\n\nx\n").is_empty());
    }

    #[test]
    fn near_misses_are_not_headings() {
        assert!(kinds("# T\n\ntext\n#hashtag and ##x\n    ## indented code\n### a third level\n").is_empty());
    }
}
