//! `./dev merge lessons-driver <base> <ours> <theirs>`: the git merge driver of `docs/lessons.md` (`merge=lessons` in .gitattributes;
//! `./dev hooks install` sets `merge.lessons.driver`). The result is written to `<ours>`, as git's drivers do.
//!
//! The file only gains sections, so two branches that both add one at the end are merged with git's union merge (both sides' lines
//! kept). That drops a blank line: a line both sides added identically (the separator before a `## ` heading) is taken as one shared
//! line, so the second branch's heading ends up glued to the last line of the first branch's section (9ec43be3c, b9bf5fc4b, 75ce0d2c8,
//! 5c241d93f; measured with `git merge-file --union` on two branches that each append "\n## X\n- x\n", and with a trailing blank line
//! per entry too). After the union [`restore_blank_lines`] puts the blank line back before every `## ` heading outside a fenced block,
//! the rule conformance C145 checks. A clone without the driver set gets git's plain merge, which conflicts: loud, never a silent drop.

use crate::check::c145;
use paddock_cli::{CliError, Report};
use std::path::Path;

/// `text` with a blank line before every `## ` heading that follows a non-blank line, outside fenced blocks. Line endings are kept.
pub fn restore_blank_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let mut fence = false;
    let mut last_blank = true;
    for (i, raw) in text.split_inclusive('\n').enumerate() {
        let line = raw.trim_end_matches(['\n', '\r']);
        if line.starts_with("```") || line.starts_with("~~~") {
            fence = !fence;
        } else if !fence && line.starts_with("## ") && i > 0 && !last_blank {
            out.push_str(if raw.ends_with("\r\n") { "\r\n" } else { "\n" });
        }
        out.push_str(raw);
        last_blank = line.trim().is_empty();
    }
    debug_assert!(c145::lessons_sites(&out).iter().all(|s| s.kind != "glued"));
    out
}

/// Runs the driver over the three files git hands it.
pub fn command(base: &Path, ours: &Path, theirs: &Path) -> Result<Report, CliError> {
    let fail = |why: String| {
        CliError::failure(format!(
            "::error::lessons-driver: {why}. Next step: merge docs/lessons.md by hand and keep both sides' sections."
        ))
    };
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
    let text = std::fs::read_to_string(ours).map_err(|e| fail(format!("{} could not be read ({e})", ours.display())))?;
    let fixed = restore_blank_lines(&text);
    std::fs::write(ours, &fixed).map_err(|e| fail(format!("{} could not be written ({e})", ours.display())))?;
    Ok(Report::new(serde_json::json!({"changed": fixed != text}), String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heading_glued_by_the_union_gets_its_blank_line_back() {
        assert_eq!(
            restore_blank_lines("# T\n\n## A\n- a\n## B\n- b\n"),
            "# T\n\n## A\n- a\n\n## B\n- b\n"
        );
        assert_eq!(restore_blank_lines("## A\r\n- a\r\n## B\r\n"), "## A\r\n- a\r\n\r\n## B\r\n");
    }

    #[test]
    fn nothing_else_changes() {
        let ok = "# T\n\n## A\n- a\n\n```\ntext\n## not a heading\n```\n\n## B\ntext\n";
        assert_eq!(restore_blank_lines(ok), ok);
        assert_eq!(restore_blank_lines(""), "");
        assert_eq!(restore_blank_lines("no newline at the end"), "no newline at the end");
    }
}
