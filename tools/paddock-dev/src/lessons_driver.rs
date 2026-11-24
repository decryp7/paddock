//! `./dev merge lessons-driver <base> <ours> <theirs>`: the git merge driver of `docs/lessons.md` (`merge=lessons` in .gitattributes;
//! `./dev hooks install` sets `merge.lessons.driver`). The result is written to `<ours>`, as git's drivers do.
//!
//! The file only gains sections, so two branches that both add one at the end are merged with git's union merge (both sides' lines
//! kept). That drops a blank line: a line both sides added identically (the separator before a `## ` heading) is taken as one shared
//! line, so the second branch's heading ends up glued to the last line of the first branch's section (9ec43be3c, b9bf5fc4b, 75ce0d2c8,
//! 5c241d93f; measured with `git merge-file --union` on two branches that each append "\n## X\n- x\n", and with a trailing blank line
//! per entry too). After the union [`restore_blank_lines`] puts the blank line back before every heading line outside a fenced block:
//! a markdown heading of any level (the `## ` rule conformance C145 checks), and the first line of a block either side added after a
//! blank line (a section that starts with a plain paragraph line, "Measuring and reporting (2026-11-24)", lost its blank line in
//! train 2026-11-25-a). A clone without the driver set gets git's plain merge, which conflicts: loud, never a silent drop.

use crate::check::c145;
use paddock_cli::{CliError, Report};
use std::collections::BTreeSet;
use std::path::Path;

/// `text` with a blank line before every heading line that follows a non-blank line, outside fenced blocks: a markdown heading of
/// any level (`#` to `######` and a space), or a line of `starts` ([`block_starts`]). Line endings are kept.
pub fn restore_blank_lines(text: &str, starts: &BTreeSet<String>) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let mut fence = false;
    let mut last_blank = true;
    for (i, raw) in text.split_inclusive('\n').enumerate() {
        let line = raw.trim_end_matches(['\n', '\r']);
        if is_fence(line) {
            fence = !fence;
        } else if !fence && i > 0 && !last_blank && (is_heading(line) || starts.contains(line)) {
            out.push_str(if raw.ends_with("\r\n") { "\r\n" } else { "\n" });
        }
        out.push_str(raw);
        last_blank = line.trim().is_empty();
    }
    debug_assert!(c145::lessons_sites(&out).iter().all(|s| s.kind != "glued"));
    out
}

fn is_fence(line: &str) -> bool {
    line.starts_with("```") || line.starts_with("~~~")
}

/// A markdown heading: one to six `#` and a space.
fn is_heading(line: &str) -> bool {
    let hashes = line.len() - line.trim_start_matches('#').len();
    (1..=6).contains(&hashes) && line[hashes..].starts_with(' ')
}

/// The lines that start a block `side` added: outside fenced blocks, after a blank line, not in `base`, and once only in `side` (a
/// repeated line could stand anywhere). The union keeps a blank line both sides added as one shared line, so such a line can end up
/// glued to the other side's block.
pub fn block_starts(base: &str, side: &str) -> BTreeSet<String> {
    let old: BTreeSet<&str> = base.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut count: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let mut starts = BTreeSet::new();
    let (mut fence, mut last_blank) = (false, false);
    for line in side.lines().map(|l| l.trim_end_matches('\r')) {
        *count.entry(line).or_default() += 1;
        if is_fence(line) {
            fence = !fence;
        } else if !fence && last_blank && !line.trim().is_empty() && !old.contains(line) {
            starts.insert(line.to_string());
        }
        last_blank = line.trim().is_empty();
    }
    starts.retain(|l| count.get(l.as_str()) == Some(&1));
    starts
}

/// Runs the driver over the three files git hands it.
pub fn command(base: &Path, ours: &Path, theirs: &Path) -> Result<Report, CliError> {
    let fail = |why: String| {
        CliError::failure(format!(
            "::error::lessons-driver: {why}. Next step: merge docs/lessons.md by hand and keep both sides' sections."
        ))
    };
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| fail(format!("{} could not be read ({e})", p.display())));
    // The sides as git handed them, read before the union overwrites `ours`.
    let (base_text, ours_text, theirs_text) = (read(base)?, read(ours)?, read(theirs)?);
    // `--union` reports resolved conflicts as a count, never as an error; a status of 128 or more is a failure.
    let out = crate::git_env::git()
        .arg("merge-file")
        .arg("--union")
        .arg(ours)
        .arg(base)
        .arg(theirs)
        .output()
        .map_err(|e| fail(format!("git could not be started ({e})")))?;
    if out.status.code().is_none_or(|c| !(0..128).contains(&c)) {
        return Err(fail(format!(
            "git merge-file failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let text = read(ours)?;
    let mut starts = block_starts(&base_text, &ours_text);
    starts.extend(block_starts(&base_text, &theirs_text));
    let fixed = restore_blank_lines(&text, &starts);
    std::fs::write(ours, &fixed).map_err(|e| fail(format!("{} could not be written ({e})", ours.display())))?;
    Ok(Report::new(serde_json::json!({"changed": fixed != text}), String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heading_glued_by_the_union_gets_its_blank_line_back() {
        assert_eq!(
            restore_blank_lines("# T\n\n## A\n- a\n## B\n- b\n", &BTreeSet::new()),
            "# T\n\n## A\n- a\n\n## B\n- b\n"
        );
        assert_eq!(
            restore_blank_lines("## A\r\n- a\r\n## B\r\n", &BTreeSet::new()),
            "## A\r\n- a\r\n\r\n## B\r\n"
        );
    }

    /// Train 2026-11-25-a: a section that starts with a plain paragraph line ("Measuring and reporting (2026-11-24)"), not a `## `
    /// heading, was glued to the other branch's last bullet by the union. The driver runs git's real union over each shape.
    #[test]
    fn the_union_of_two_appended_blocks_keeps_the_blank_line_before_any_heading_line() {
        let base = "# T\n\n## A\n- a\n";
        let cases = [
            ("\n## B\n- b\n", "\nMeasuring and reporting (2026-11-24)\n- m\n"),
            ("\n## B\n- b\n", "\n### C\n- c\n"),
            ("\n## B\n- b\n", "\n## C\n- c\n"),
        ];
        for (ours_add, theirs_add) in cases {
            let d = tempfile::tempdir().unwrap();
            let (b, o, t) = (d.path().join("base"), d.path().join("ours"), d.path().join("theirs"));
            std::fs::write(&b, base).unwrap();
            std::fs::write(&o, format!("{base}{ours_add}")).unwrap();
            std::fs::write(&t, format!("{base}{theirs_add}")).unwrap();
            // The control: git's union alone glues the second block's first line to the first block's last.
            let raw = crate::git_env::git()
                .args(["merge-file", "--union", "-p"])
                .arg(&o)
                .arg(&b)
                .arg(&t)
                .output()
                .unwrap();
            let raw = String::from_utf8(raw.stdout).unwrap();
            let first = theirs_add.trim_start().lines().next().unwrap();
            assert!(raw.contains(&format!("- b\n{first}\n")), "the union glues it: {raw:?}");
            command(&b, &o, &t).unwrap();
            assert_eq!(
                std::fs::read_to_string(&o).unwrap(),
                format!("{base}{ours_add}{theirs_add}"),
                "{theirs_add:?}"
            );
        }
    }

    #[test]
    fn only_a_line_a_side_added_after_a_blank_line_and_once_starts_a_block() {
        let base = "# T\n\n## A\n- a\n";
        let side = format!("{base}\nNew section\n- n\n- n\nwrapped\n\n- n\n\n- a\n");
        // `- n` is repeated and `- a` is in the base; `wrapped` follows a non-blank line: none of them starts a block.
        assert_eq!(block_starts(base, &side), BTreeSet::from(["New section".to_string()]));
        // Positive control: the same text with `wrapped` after a blank line names it too.
        let blank = side.replace("- n\nwrapped", "- n\n\nwrapped");
        assert!(block_starts(base, &blank).contains("wrapped"));
    }

    #[test]
    fn nothing_else_changes() {
        let ok = "# T\n\n## A\n- a\n\n```\ntext\n## not a heading\n```\n\n## B\ntext\n";
        assert_eq!(restore_blank_lines(ok, &BTreeSet::new()), ok);
        assert_eq!(restore_blank_lines("", &BTreeSet::new()), "");
        assert_eq!(
            restore_blank_lines("no newline at the end", &BTreeSet::new()),
            "no newline at the end"
        );
    }
}
